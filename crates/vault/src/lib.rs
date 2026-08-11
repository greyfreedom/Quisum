#![doc = "SQLCipher-backed local persistence for Quisum."]
#![forbid(unsafe_code)]

mod error;
mod key;
mod migration;
mod store;

use std::{
    ffi::OsString,
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    str::FromStr,
    time::Duration,
};

use chrono::Utc;
use parking_lot::Mutex;
use quisum_core::{Actor, AuditAction, AuditDetails, AuditEvent, EntityId, EntityKind, Validate};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

pub use error::VaultError;
pub use key::VaultKey;
pub use migration::LATEST_SCHEMA_VERSION;
pub use store::{DeletionImpact, EvidenceProposal, ReviewCommit};

/// A verified connection to one encrypted local vault.
pub struct SqlCipherVault {
    path: PathBuf,
    connection: Mutex<Connection>,
}

impl std::fmt::Debug for SqlCipherVault {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SqlCipherVault")
            .field("path", &self.path)
            .field("connection", &"[LOCKED CONNECTION]")
            .finish()
    }
}

impl SqlCipherVault {
    /// Create a new encrypted vault without replacing an existing file.
    pub fn create(path: impl AsRef<Path>, key: &VaultKey) -> Result<Self, VaultError> {
        let path = path.as_ref().to_path_buf();
        if path.exists() {
            return Err(VaultError::AlreadyExists);
        }
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|source| VaultError::FileSystem {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|source| {
                if source.kind() == std::io::ErrorKind::AlreadyExists {
                    VaultError::AlreadyExists
                } else {
                    VaultError::FileSystem {
                        path: path.clone(),
                        source,
                    }
                }
            })?;
        let mut guard = NewVaultGuard::new(path.clone());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;

            if let Err(source) = file.set_permissions(fs::Permissions::from_mode(0o600)) {
                drop(file);
                return Err(VaultError::FileSystem {
                    path: path.clone(),
                    source,
                });
            }
        }
        drop(file);

        let mut connection = open_connection(&path)?;
        configure_cipher(&connection, key)?;
        migration::migrate(&mut connection)?;

        let vault_id = EntityId::new(EntityKind::Vault);
        let created_at = Utc::now();
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO vault_metadata(key, value) VALUES
             ('vault_id', ?1),
             ('schema_version', ?2),
             ('created_at', ?3)",
            params![
                vault_id.to_string(),
                LATEST_SCHEMA_VERSION.to_string(),
                created_at.to_rfc3339()
            ],
        )?;
        let audit = AuditEvent::new(
            Actor::system().kind(),
            AuditAction::VaultCreated,
            Some(vault_id),
            AuditDetails::None,
            created_at,
        );
        audit.validate()?;
        store::insert_audit(&transaction, &audit)?;
        transaction.commit()?;

        guard.commit();
        Ok(Self {
            path,
            connection: Mutex::new(connection),
        })
    }

    /// Open and verify an existing encrypted vault.
    pub fn open(path: impl AsRef<Path>, key: &VaultKey) -> Result<Self, VaultError> {
        let path = path.as_ref().to_path_buf();
        if !path.is_file() {
            return Err(VaultError::NotFound);
        }
        let mut connection = open_connection(&path)?;
        configure_cipher(&connection, key)?;
        verify_integrity(&connection)?;
        migration::migrate(&mut connection)?;
        verify_integrity(&connection)?;
        let vault_id: String = connection.query_row(
            "SELECT value FROM vault_metadata WHERE key = 'vault_id'",
            [],
            |row| row.get(0),
        )?;
        let vault_id = EntityId::from_str(&vault_id)?;
        let opened_at = Utc::now();
        let transaction = connection.transaction()?;
        let audit = AuditEvent::new(
            Actor::system().kind(),
            AuditAction::VaultUnlocked,
            Some(vault_id),
            AuditDetails::None,
            opened_at,
        );
        store::insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        Ok(Self {
            path,
            connection: Mutex::new(connection),
        })
    }

    /// Return the vault file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Read the stable vault identifier.
    pub fn vault_id(&self) -> Result<EntityId, VaultError> {
        let connection = self.connection.lock();
        let value: String = connection.query_row(
            "SELECT value FROM vault_metadata WHERE key = 'vault_id'",
            [],
            |row| row.get(0),
        )?;
        EntityId::from_str(&value).map_err(VaultError::from)
    }

    /// Return the applied schema version.
    pub fn schema_version(&self) -> Result<u32, VaultError> {
        let connection = self.connection.lock();
        Ok(connection.pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    /// Run SQLCipher page authentication plus SQLite structural integrity checks.
    pub fn integrity_check(&self) -> Result<(), VaultError> {
        verify_integrity(&self.connection.lock())
    }

    /// Record an explicit lock event and close the encrypted database connection.
    pub fn close(self) -> Result<(), VaultError> {
        let mut connection = self.connection.into_inner();
        let vault_id: String = connection.query_row(
            "SELECT value FROM vault_metadata WHERE key = 'vault_id'",
            [],
            |row| row.get(0),
        )?;
        let vault_id = EntityId::from_str(&vault_id)?;
        let closed_at = Utc::now();
        let transaction = connection.transaction()?;
        let audit = AuditEvent::new(
            Actor::system().kind(),
            AuditAction::VaultLocked,
            Some(vault_id),
            AuditDetails::None,
            closed_at,
        );
        store::insert_audit(&transaction, &audit)?;
        transaction.commit()?;
        connection
            .close()
            .map_err(|(_, error)| VaultError::Database(error))
    }
}

fn open_connection(path: &Path) -> Result<Connection, VaultError> {
    Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    )
    .map_err(VaultError::from)
}

fn configure_cipher(connection: &Connection, key: &VaultKey) -> Result<(), VaultError> {
    let literal = key.sqlcipher_literal();
    connection
        .pragma_update(None, "key", literal.as_str())
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;

    let cipher_version = connection
        .pragma_query_value(None, "cipher_version", |row| row.get::<_, String>(0))
        .optional()
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    if cipher_version.as_deref().is_none_or(str::is_empty) {
        return Err(VaultError::CipherUnavailable);
    }

    connection
        .query_row("SELECT COUNT(*) FROM sqlite_master", [], |row| {
            row.get::<_, u32>(0)
        })
        .map_err(|_| VaultError::InvalidKeyOrCorrupt)?;
    connection.pragma_update(None, "cipher_memory_security", "ON")?;
    connection.pragma_update(None, "foreign_keys", "ON")?;
    connection.pragma_update(None, "secure_delete", "ON")?;
    connection.pragma_update(None, "temp_store", "MEMORY")?;
    connection.pragma_update(None, "trusted_schema", "OFF")?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    connection.pragma_update(None, "synchronous", "FULL")?;
    connection.busy_timeout(Duration::from_secs(5))?;
    Ok(())
}

fn verify_integrity(connection: &Connection) -> Result<(), VaultError> {
    let quick_check: String = connection
        .pragma_query_value(None, "quick_check", |row| row.get(0))
        .map_err(|_| VaultError::IntegrityCheckFailed)?;
    if quick_check != "ok" {
        return Err(VaultError::IntegrityCheckFailed);
    }

    let mut statement = connection
        .prepare("PRAGMA cipher_integrity_check")
        .map_err(|_| VaultError::IntegrityCheckFailed)?;
    let messages = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|_| VaultError::IntegrityCheckFailed)?;
    for message in messages {
        let message = message.map_err(|_| VaultError::IntegrityCheckFailed)?;
        if message != "ok" {
            return Err(VaultError::IntegrityCheckFailed);
        }
    }
    Ok(())
}

struct NewVaultGuard {
    path: PathBuf,
    committed: bool,
}

impl NewVaultGuard {
    const fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }

    const fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for NewVaultGuard {
    fn drop(&mut self) {
        if self.committed {
            return;
        }
        let _ = fs::remove_file(&self.path);
        for suffix in ["-wal", "-shm"] {
            let mut sidecar = OsString::from(self.path.as_os_str());
            sidecar.push(suffix);
            let _ = fs::remove_file(PathBuf::from(sidecar));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{error::Error, fs};

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn creates_encrypted_database_and_rejects_wrong_key() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let path = directory.path().join("vault.db");
        let key = VaultKey::generate()?;
        let vault = SqlCipherVault::create(&path, &key)?;
        let expected_id = vault.vault_id()?;
        assert_eq!(vault.schema_version()?, LATEST_SCHEMA_VERSION);
        vault.integrity_check()?;
        vault.close()?;

        let header = fs::read(&path)?;
        assert!(header.len() >= 16);
        assert_ne!(&header[..16], b"SQLite format 3\0");

        let reopened = SqlCipherVault::open(&path, &key)?;
        assert_eq!(reopened.vault_id()?, expected_id);
        reopened.close()?;

        let wrong_key = VaultKey::generate()?;
        assert!(matches!(
            SqlCipherVault::open(&path, &wrong_key),
            Err(VaultError::InvalidKeyOrCorrupt)
        ));
        Ok(())
    }

    #[test]
    fn create_never_overwrites_an_existing_file() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let path = directory.path().join("vault.db");
        fs::write(&path, b"do not replace")?;
        let key = VaultKey::generate()?;
        assert!(matches!(
            SqlCipherVault::create(&path, &key),
            Err(VaultError::AlreadyExists)
        ));
        assert_eq!(fs::read(&path)?, b"do not replace");
        Ok(())
    }

    #[test]
    fn domain_content_is_not_visible_in_database_or_sidecars() -> Result<(), Box<dyn Error>> {
        let directory = tempdir()?;
        let path = directory.path().join("vault.db");
        let key = VaultKey::generate()?;
        let vault = SqlCipherVault::create(&path, &key)?;
        let marker = "QUISUM_SYNTHETIC_PLAINTEXT_SENTINEL_7E4A";
        let event = quisum_core::Event::user_confirmed(
            marker,
            vec![marker.to_owned()],
            None,
            Utc::now(),
            quisum_core::Sensitivity::Private,
        );
        vault.insert_event(Actor::user(), &event)?;

        for entry in fs::read_dir(directory.path())? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let bytes = fs::read(entry.path())?;
                assert!(
                    !bytes
                        .windows(marker.len())
                        .any(|window| window == marker.as_bytes())
                );
            }
        }
        drop(vault);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn new_vault_file_is_owner_only() -> Result<(), Box<dyn Error>> {
        use std::os::unix::fs::PermissionsExt as _;

        let directory = tempdir()?;
        let path = directory.path().join("vault.db");
        let key = VaultKey::generate()?;
        let vault = SqlCipherVault::create(&path, &key)?;
        let event = quisum_core::Event::user_confirmed(
            "Synthetic permissions event",
            vec!["A synthetic fact.".to_owned()],
            None,
            Utc::now(),
            quisum_core::Sensitivity::Private,
        );
        vault.insert_event(Actor::user(), &event)?;
        for entry in fs::read_dir(directory.path())? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                assert_eq!(entry.metadata()?.permissions().mode() & 0o077, 0);
            }
        }
        Ok(())
    }
}
