use std::{
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::{Component, Path, PathBuf},
};

use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use chacha20poly1305::{
    KeyInit as _, XChaCha20Poly1305, XNonce,
    aead::{Aead as _, Payload},
};
use chrono::{DateTime, Utc};
use quisum_core::{
    Actor, ActorKind, AuditAction, AuditDetails, AuditEvent, Capability, EntityId, EntityKind,
};
use rusqlite::TransactionBehavior;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{RecoveryKeyEnvelope, SqlCipherVault, VaultError, VaultKey, store};

const MAGIC: &[u8; 8] = b"QSB1\r\n\x1a\n";
const FORMAT: &str = "quisum-encrypted-backup";
const FORMAT_VERSION: u32 = 1;
const MAX_HEADER_BYTES: usize = 64 * 1_024;
const MAX_DATABASE_BYTES: u64 = 8 * 1_024 * 1_024 * 1_024;
const COPY_BUFFER_BYTES: usize = 128 * 1_024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BackupHeader {
    format: String,
    format_version: u32,
    backup_id: EntityId,
    vault_id: EntityId,
    schema_version: u32,
    created_at: DateTime<Utc>,
    database_bytes: u64,
    database_sha256: String,
    key_envelope: RecoveryKeyEnvelope,
    auth_nonce: String,
    auth_tag: String,
}

/// Metadata returned only after an encrypted backup has been written and verified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupReceipt {
    /// Backup identifier.
    pub backup_id: EntityId,
    /// Source vault identifier.
    pub vault_id: EntityId,
    /// Canonical artifact path.
    pub path: PathBuf,
    /// Artifact creation time.
    pub created_at: DateTime<Utc>,
    /// Encrypted database payload bytes.
    pub database_bytes: u64,
    /// Encrypted database SHA-256.
    pub database_sha256: String,
}

/// A verified restored vault and the key needed to register it with an OS secret store.
pub struct RestoredVault {
    /// Open verified vault.
    pub vault: SqlCipherVault,
    /// Recovered random database key.
    pub key: VaultKey,
}

impl std::fmt::Debug for RestoredVault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RestoredVault")
            .field("vault", &self.vault)
            .field("key", &"[REDACTED]")
            .finish()
    }
}

impl SqlCipherVault {
    /// Create an encrypted, passphrase-recoverable backup without plaintext temporary files.
    pub fn create_backup(
        &self,
        actor: Actor,
        destination: impl AsRef<Path>,
        recovery_passphrase: &str,
        created_at: DateTime<Utc>,
    ) -> Result<BackupReceipt, VaultError> {
        require_user_encryption_actor(actor)?;
        let destination = resolve_new_destination(destination.as_ref())?;
        let mut connection = self.connection.lock();
        checkpoint(&connection)?;
        let database_bytes = fs::metadata(&self.path)
            .map_err(|source| VaultError::FileSystem {
                path: self.path.clone(),
                source,
            })?
            .len();
        if database_bytes == 0 || database_bytes > MAX_DATABASE_BYTES {
            return Err(VaultError::InvalidArtifact);
        }
        let database_sha256 = hash_file(&self.path, database_bytes)?;
        let vault_id = self.vault_id_with_connection(&connection)?;
        let mut auth_nonce = [0_u8; 24];
        getrandom::fill(&mut auth_nonce).map_err(|_| VaultError::RandomUnavailable)?;
        let mut header = BackupHeader {
            format: FORMAT.to_owned(),
            format_version: FORMAT_VERSION,
            backup_id: EntityId::new(EntityKind::Backup),
            vault_id: vault_id.clone(),
            schema_version: connection
                .pragma_query_value(None, "user_version", |row| row.get(0))?,
            created_at,
            database_bytes,
            database_sha256: database_sha256.clone(),
            key_envelope: RecoveryKeyEnvelope::seal(&self.key, recovery_passphrase)?,
            auth_nonce: URL_SAFE_NO_PAD.encode(auth_nonce),
            auth_tag: String::new(),
        };
        header.auth_tag = create_header_auth_tag(&header, &self.key)?;
        let header_bytes = serde_json::to_vec(&header)?;
        if header_bytes.len() > MAX_HEADER_BYTES {
            return Err(VaultError::InvalidArtifact);
        }

        let mut guard = ArtifactGuard::create(&destination)?;
        {
            let mut writer = BufWriter::new(guard.file_mut()?);
            writer
                .write_all(MAGIC)
                .map_err(|source| file_error(&destination, source))?;
            let header_length =
                u32::try_from(header_bytes.len()).map_err(|_| VaultError::InvalidArtifact)?;
            writer
                .write_all(&header_length.to_be_bytes())
                .and_then(|()| writer.write_all(&header_bytes))
                .map_err(|source| file_error(&destination, source))?;
            copy_exact_file(&self.path, &mut writer, database_bytes)?;
            writer
                .flush()
                .map_err(|source| file_error(&destination, source))?;
        }
        guard.sync()?;
        let verified = read_and_verify_header(&destination)?;
        let recovered = verified.key_envelope.open(recovery_passphrase)?;
        verify_header_auth_tag(&verified, &recovered)?;
        if verified != header || !self.key.matches(&recovered) {
            return Err(VaultError::ArtifactTampered);
        }

        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::BackupCreated,
            Some(header.backup_id.clone()),
            AuditDetails::Artifact {
                format: "encrypted_backup".to_owned(),
                record_count: 1,
                encrypted: true,
            },
            created_at,
        );
        store::insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        guard.commit();
        Ok(BackupReceipt {
            backup_id: header.backup_id,
            vault_id,
            path: destination,
            created_at,
            database_bytes,
            database_sha256,
        })
    }

    /// Restore a verified backup to a new destination without replacing any existing file.
    pub fn restore_backup(
        actor: Actor,
        source: impl AsRef<Path>,
        destination: impl AsRef<Path>,
        recovery_passphrase: &str,
        restored_at: DateTime<Utc>,
    ) -> Result<RestoredVault, VaultError> {
        require_user_encryption_actor(actor)?;
        let source = source
            .as_ref()
            .canonicalize()
            .map_err(|error| file_error(source.as_ref(), error))?;
        if !source.is_file() {
            return Err(VaultError::InvalidArtifact);
        }
        let destination = resolve_new_destination(destination.as_ref())?;
        let (header, payload_offset) = read_header(&source)?;
        validate_header(&header)?;
        let key = header.key_envelope.open(recovery_passphrase)?;
        verify_header_auth_tag(&header, &key)?;
        let mut guard = ArtifactGuard::create(&destination)?;
        let actual_hash = extract_database(
            &source,
            payload_offset,
            guard.file_mut()?,
            header.database_bytes,
        )?;
        if actual_hash != header.database_sha256 {
            return Err(VaultError::ArtifactTampered);
        }
        guard.sync()?;
        let raw_connection = crate::open_connection(&destination)?;
        crate::configure_cipher(&raw_connection, &key)?;
        let raw_schema: u32 =
            raw_connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if raw_schema != header.schema_version {
            return Err(VaultError::ArtifactTampered);
        }
        drop(raw_connection);
        let vault = SqlCipherVault::open_verified_restore(&destination, &key)?;
        if vault.vault_id()? != header.vault_id
            || vault.schema_version()? != crate::LATEST_SCHEMA_VERSION
        {
            return Err(VaultError::ArtifactTampered);
        }
        vault.record_restore_audit(actor, header.backup_id, restored_at)?;
        guard.commit();
        Ok(RestoredVault { vault, key })
    }

    fn vault_id_with_connection(
        &self,
        connection: &rusqlite::Connection,
    ) -> Result<EntityId, VaultError> {
        use std::str::FromStr as _;
        let value: String = connection.query_row(
            "SELECT value FROM vault_metadata WHERE key = 'vault_id'",
            [],
            |row| row.get(0),
        )?;
        EntityId::from_str(&value).map_err(VaultError::from)
    }

    fn record_restore_audit(
        &self,
        actor: Actor,
        backup_id: EntityId,
        restored_at: DateTime<Utc>,
    ) -> Result<(), VaultError> {
        let mut connection = self.connection.lock();
        let transaction = connection.transaction()?;
        let audit = AuditEvent::new(
            actor.kind(),
            AuditAction::RestoreCompleted,
            Some(backup_id),
            AuditDetails::Artifact {
                format: "encrypted_backup".to_owned(),
                record_count: 1,
                encrypted: true,
            },
            restored_at,
        );
        store::insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(())
    }
}

fn require_user_encryption_actor(actor: Actor) -> Result<(), VaultError> {
    actor.require(Capability::ManageEncryption)?;
    if actor.kind() == ActorKind::User {
        Ok(())
    } else {
        Err(quisum_core::DomainError::Forbidden(Capability::ManageEncryption).into())
    }
}

fn checkpoint(connection: &rusqlite::Connection) -> Result<(), VaultError> {
    let (busy, _remaining, _checkpointed): (u32, u32, u32) =
        connection.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?;
    if busy == 0 {
        Ok(())
    } else {
        Err(VaultError::IntegrityCheckFailed)
    }
}

pub(crate) fn create_pre_migration_snapshot(
    connection: &rusqlite::Connection,
    source: &Path,
    version: u32,
) -> Result<PathBuf, VaultError> {
    checkpoint(connection)?;
    let bytes = fs::metadata(source)
        .map_err(|error| file_error(source, error))?
        .len();
    if bytes == 0 || bytes > MAX_DATABASE_BYTES {
        return Err(VaultError::InvalidArtifact);
    }
    let filename = source
        .file_name()
        .ok_or(VaultError::InvalidArtifact)?
        .to_string_lossy();
    let timestamp = Utc::now().timestamp_millis();
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    for sequence in 0_u8..100 {
        let destination = parent.join(format!(
            "{filename}.pre-migration-v{version}-{timestamp}-{sequence}.bak"
        ));
        let mut guard = match ArtifactGuard::create(&destination) {
            Ok(guard) => guard,
            Err(VaultError::AlreadyExists) => continue,
            Err(error) => return Err(error),
        };
        copy_exact_file(source, guard.file_mut()?, bytes)?;
        guard.sync()?;
        if hash_file(source, bytes)? != hash_file(&destination, bytes)? {
            return Err(VaultError::ArtifactTampered);
        }
        guard.commit();
        return Ok(destination);
    }
    Err(VaultError::AlreadyExists)
}

pub(crate) fn resolve_new_destination(path: &Path) -> Result<PathBuf, VaultError> {
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(VaultError::InvalidArtifact);
    }
    let filename = path
        .file_name()
        .filter(|name| !name.is_empty())
        .ok_or(VaultError::InvalidArtifact)?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let canonical_parent = parent
        .canonicalize()
        .map_err(|source| file_error(parent, source))?;
    if !canonical_parent.is_dir() {
        return Err(VaultError::InvalidArtifact);
    }
    let destination = canonical_parent.join(filename);
    if destination
        .try_exists()
        .map_err(|source| file_error(&destination, source))?
    {
        return Err(VaultError::AlreadyExists);
    }
    Ok(destination)
}

fn read_and_verify_header(path: &Path) -> Result<BackupHeader, VaultError> {
    let (header, offset) = read_header(path)?;
    validate_header(&header)?;
    let metadata = fs::metadata(path).map_err(|source| file_error(path, source))?;
    if metadata.len() != offset + header.database_bytes {
        return Err(VaultError::InvalidArtifact);
    }
    let actual = hash_region(path, offset, header.database_bytes)?;
    if actual != header.database_sha256 {
        return Err(VaultError::ArtifactTampered);
    }
    Ok(header)
}

fn read_header(path: &Path) -> Result<(BackupHeader, u64), VaultError> {
    let file = File::open(path).map_err(|source| file_error(path, source))?;
    let mut reader = BufReader::new(file);
    let mut magic = [0_u8; MAGIC.len()];
    reader
        .read_exact(&mut magic)
        .map_err(|source| file_error(path, source))?;
    if &magic != MAGIC {
        return Err(VaultError::InvalidArtifact);
    }
    let mut length = [0_u8; size_of::<u32>()];
    reader
        .read_exact(&mut length)
        .map_err(|source| file_error(path, source))?;
    let length =
        usize::try_from(u32::from_be_bytes(length)).map_err(|_| VaultError::InvalidArtifact)?;
    if length == 0 || length > MAX_HEADER_BYTES {
        return Err(VaultError::InvalidArtifact);
    }
    let mut bytes = vec![0_u8; length];
    reader
        .read_exact(&mut bytes)
        .map_err(|source| file_error(path, source))?;
    let header: BackupHeader = serde_json::from_slice(&bytes)?;
    let offset = u64::try_from(MAGIC.len() + size_of::<u32>() + length)
        .map_err(|_| VaultError::InvalidArtifact)?;
    Ok((header, offset))
}

fn validate_header(header: &BackupHeader) -> Result<(), VaultError> {
    if header.format != FORMAT
        || header.format_version != FORMAT_VERSION
        || header.backup_id.kind() != EntityKind::Backup
        || header.vault_id.kind() != EntityKind::Vault
        || header.schema_version == 0
        || header.database_bytes == 0
        || header.database_bytes > MAX_DATABASE_BYTES
        || header.database_sha256.len() != 64
        || !header
            .database_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        || header.auth_nonce.len() > 64
        || header.auth_tag.len() > 64
    {
        return Err(VaultError::InvalidArtifact);
    }
    Ok(())
}

fn create_header_auth_tag(header: &BackupHeader, key: &VaultKey) -> Result<String, VaultError> {
    let nonce = decode_backup_field::<24>(&header.auth_nonce)?;
    let associated_data = authenticated_header_bytes(header)?;
    let cipher =
        XChaCha20Poly1305::new_from_slice(key.bytes()).map_err(|_| VaultError::InvalidArtifact)?;
    let tag = cipher
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &[],
                aad: &associated_data,
            },
        )
        .map_err(|_| VaultError::ArtifactTampered)?;
    Ok(URL_SAFE_NO_PAD.encode(tag))
}

fn verify_header_auth_tag(header: &BackupHeader, key: &VaultKey) -> Result<(), VaultError> {
    let nonce = decode_backup_field::<24>(&header.auth_nonce)?;
    let tag = URL_SAFE_NO_PAD
        .decode(&header.auth_tag)
        .map_err(|_| VaultError::InvalidArtifact)?;
    if tag.len() != 16 {
        return Err(VaultError::InvalidArtifact);
    }
    let associated_data = authenticated_header_bytes(header)?;
    let cipher =
        XChaCha20Poly1305::new_from_slice(key.bytes()).map_err(|_| VaultError::InvalidArtifact)?;
    cipher
        .decrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: &tag,
                aad: &associated_data,
            },
        )
        .map(|_| ())
        .map_err(|_| VaultError::ArtifactTampered)
}

fn authenticated_header_bytes(header: &BackupHeader) -> Result<Vec<u8>, VaultError> {
    let mut unsigned = header.clone();
    unsigned.auth_tag.clear();
    serde_json::to_vec(&unsigned).map_err(VaultError::from)
}

fn decode_backup_field<const LENGTH: usize>(value: &str) -> Result<[u8; LENGTH], VaultError> {
    URL_SAFE_NO_PAD
        .decode(value)
        .map_err(|_| VaultError::InvalidArtifact)?
        .try_into()
        .map_err(|_| VaultError::InvalidArtifact)
}

fn hash_file(path: &Path, expected_bytes: u64) -> Result<String, VaultError> {
    hash_region(path, 0, expected_bytes)
}

fn hash_region(path: &Path, offset: u64, expected_bytes: u64) -> Result<String, VaultError> {
    use std::io::Seek as _;
    let mut file = File::open(path).map_err(|source| file_error(path, source))?;
    file.seek(std::io::SeekFrom::Start(offset))
        .map_err(|source| file_error(path, source))?;
    let mut reader = file.take(expected_bytes);
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    let mut copied = 0_u64;
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|source| file_error(path, source))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        copied = copied
            .checked_add(u64::try_from(read).map_err(|_| VaultError::InvalidArtifact)?)
            .ok_or(VaultError::InvalidArtifact)?;
    }
    if copied != expected_bytes {
        return Err(VaultError::InvalidArtifact);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn copy_exact_file(
    source: &Path,
    destination: &mut impl Write,
    expected_bytes: u64,
) -> Result<(), VaultError> {
    let file = File::open(source).map_err(|error| file_error(source, error))?;
    let mut reader = file.take(expected_bytes);
    let copied =
        std::io::copy(&mut reader, destination).map_err(|error| file_error(source, error))?;
    if copied == expected_bytes {
        Ok(())
    } else {
        Err(VaultError::InvalidArtifact)
    }
}

fn extract_database(
    source: &Path,
    offset: u64,
    destination: &mut File,
    expected_bytes: u64,
) -> Result<String, VaultError> {
    use std::io::Seek as _;
    let source_file = File::open(source).map_err(|error| file_error(source, error))?;
    let metadata = source_file
        .metadata()
        .map_err(|error| file_error(source, error))?;
    if metadata.len() != offset + expected_bytes {
        return Err(VaultError::InvalidArtifact);
    }
    let mut reader = source_file;
    reader
        .seek(std::io::SeekFrom::Start(offset))
        .map_err(|error| file_error(source, error))?;
    let mut remaining = expected_bytes;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    while remaining > 0 {
        let maximum = usize::try_from(remaining.min(COPY_BUFFER_BYTES as u64))
            .map_err(|_| VaultError::InvalidArtifact)?;
        let read = reader
            .read(&mut buffer[..maximum])
            .map_err(|error| file_error(source, error))?;
        if read == 0 {
            return Err(VaultError::InvalidArtifact);
        }
        destination
            .write_all(&buffer[..read])
            .map_err(|error| file_error(source, error))?;
        hasher.update(&buffer[..read]);
        remaining -= u64::try_from(read).map_err(|_| VaultError::InvalidArtifact)?;
    }
    Ok(hex::encode(hasher.finalize()))
}

pub(crate) struct ArtifactGuard {
    path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl ArtifactGuard {
    pub(crate) fn create(path: &Path) -> Result<Self, VaultError> {
        let options = &mut OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        let file = options.open(path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::AlreadyExists {
                VaultError::AlreadyExists
            } else {
                file_error(path, source)
            }
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            file: Some(file),
            committed: false,
        })
    }

    pub(crate) fn file_mut(&mut self) -> Result<&mut File, VaultError> {
        self.file.as_mut().ok_or(VaultError::InvalidArtifact)
    }

    pub(crate) fn sync(&mut self) -> Result<(), VaultError> {
        self.file_mut()?
            .sync_all()
            .map_err(|source| file_error(&self.path, source))
    }

    pub(crate) fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for ArtifactGuard {
    fn drop(&mut self) {
        if !self.committed {
            self.file.take();
            let _ = fs::remove_file(&self.path);
            for suffix in ["-wal", "-shm"] {
                let mut sidecar = OsString::from(self.path.as_os_str());
                sidecar.push(suffix);
                let _ = fs::remove_file(PathBuf::from(sidecar));
            }
        }
    }
}

pub(crate) fn file_error(path: &Path, source: std::io::Error) -> VaultError {
    VaultError::FileSystem {
        path: path.to_path_buf(),
        source,
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error, fs};

    use quisum_core::{Event, Sensitivity};
    use tempfile::tempdir;

    use super::*;

    const PASSPHRASE: &str = "synthetic backup recovery phrase";

    #[test]
    fn backup_round_trip_is_encrypted_verified_and_never_overwrites() -> Result<(), Box<dyn Error>>
    {
        let directory = tempdir()?;
        let vault_path = directory.path().join("vault.db");
        let backup_path = directory.path().join("vault.qsb");
        let restore_path = directory.path().join("restored.db");
        let key = VaultKey::generate()?;
        let vault = SqlCipherVault::create(&vault_path, &key)?;
        let marker = "SYNTHETIC_BACKUP_PLAINTEXT_SENTINEL";
        let event = Event::user_confirmed(
            "Backup test",
            vec![marker.to_owned()],
            None,
            Utc::now(),
            Sensitivity::Private,
        );
        vault.insert_event(Actor::user(), &event)?;
        let source_id = vault.vault_id()?;
        let receipt = vault.create_backup(Actor::user(), &backup_path, PASSPHRASE, Utc::now())?;
        assert_eq!(receipt.vault_id, source_id);
        let artifact = fs::read(&backup_path)?;
        assert!(
            !artifact
                .windows(marker.len())
                .any(|bytes| bytes == marker.as_bytes())
        );
        assert!(matches!(
            vault.create_backup(Actor::user(), &backup_path, PASSPHRASE, Utc::now()),
            Err(VaultError::AlreadyExists)
        ));

        let restored = SqlCipherVault::restore_backup(
            Actor::user(),
            &backup_path,
            &restore_path,
            PASSPHRASE,
            Utc::now(),
        )?;
        assert_eq!(restored.vault.vault_id()?, source_id);
        restored.vault.integrity_check()?;
        assert!(matches!(
            SqlCipherVault::restore_backup(
                Actor::user(),
                &backup_path,
                &restore_path,
                PASSPHRASE,
                Utc::now()
            ),
            Err(VaultError::AlreadyExists)
        ));
        Ok(())
    }

    #[test]
    fn wrong_passphrase_and_payload_tampering_leave_no_destination() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let vault =
            SqlCipherVault::create(directory.path().join("vault.db"), &VaultKey::generate()?)?;
        let backup_path = directory.path().join("vault.qsb");
        vault.create_backup(Actor::user(), &backup_path, PASSPHRASE, Utc::now())?;
        let wrong_destination = directory.path().join("wrong.db");
        assert!(matches!(
            SqlCipherVault::restore_backup(
                Actor::user(),
                &backup_path,
                &wrong_destination,
                "different synthetic phrase",
                Utc::now()
            ),
            Err(VaultError::RecoveryFailed)
        ));
        assert!(!wrong_destination.exists());

        let mut bytes = fs::read(&backup_path)?;
        let last = bytes
            .len()
            .checked_sub(1)
            .ok_or(VaultError::InvalidArtifact)?;
        bytes[last] ^= 0x01;
        fs::write(&backup_path, bytes)?;
        let tampered_destination = directory.path().join("tampered.db");
        assert!(matches!(
            SqlCipherVault::restore_backup(
                Actor::user(),
                &backup_path,
                &tampered_destination,
                PASSPHRASE,
                Utc::now()
            ),
            Err(VaultError::ArtifactTampered)
        ));
        assert!(!tampered_destination.exists());
        Ok(())
    }

    #[test]
    fn parent_directory_components_are_rejected() {
        assert!(matches!(
            resolve_new_destination(Path::new("synthetic/../backup.qsb")),
            Err(VaultError::InvalidArtifact)
        ));
    }

    #[test]
    fn failed_artifact_guard_removes_database_sidecars() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let path = directory.path().join("failed.db");
        let guard = ArtifactGuard::create(&path)?;
        let wal = directory.path().join("failed.db-wal");
        let shm = directory.path().join("failed.db-shm");
        fs::write(&wal, b"synthetic wal")?;
        fs::write(&shm, b"synthetic shm")?;
        drop(guard);
        assert!(!path.exists());
        assert!(!wal.exists());
        assert!(!shm.exists());
        Ok(())
    }

    #[test]
    fn restore_authenticates_the_backed_up_schema_before_upgrading_it() -> Result<(), Box<dyn Error>>
    {
        let directory = tempdir()?;
        let vault_path = directory.path().join("older.db");
        let backup_path = directory.path().join("older.qsb");
        let restore_path = directory.path().join("upgraded.db");
        let key = VaultKey::generate()?;
        let vault = SqlCipherVault::create(&vault_path, &key)?;
        {
            let connection = vault.connection.lock();
            connection.execute_batch("DROP TRIGGER schema_migrations_no_delete")?;
            connection.execute(
                "DELETE FROM schema_migrations WHERE version = ?1",
                [crate::LATEST_SCHEMA_VERSION],
            )?;
            connection.pragma_update(None, "user_version", crate::LATEST_SCHEMA_VERSION - 1)?;
        }
        vault.create_backup(Actor::user(), &backup_path, PASSPHRASE, Utc::now())?;
        let (header, _) = read_header(&backup_path)?;
        assert_eq!(header.schema_version, crate::LATEST_SCHEMA_VERSION - 1);

        let restored = SqlCipherVault::restore_backup(
            Actor::user(),
            &backup_path,
            &restore_path,
            PASSPHRASE,
            Utc::now(),
        )?;
        assert_eq!(
            restored.vault.schema_version()?,
            crate::LATEST_SCHEMA_VERSION
        );
        restored.vault.integrity_check()?;
        assert!(
            fs::read_dir(directory.path())?
                .filter_map(Result::ok)
                .all(
                    |entry| !entry.file_name().to_string_lossy().contains(&format!(
                        "upgraded.db.pre-migration-v{}-",
                        crate::LATEST_SCHEMA_VERSION - 1
                    ))
                )
        );
        Ok(())
    }
}
