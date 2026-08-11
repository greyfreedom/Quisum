use std::path::PathBuf;

use quisum_core::DomainError;

/// Failures from encrypted storage. User-facing adapters should preserve redaction.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    /// Refusing to replace an existing path.
    #[error("vault already exists")]
    AlreadyExists,
    /// Vault path does not exist.
    #[error("vault does not exist")]
    NotFound,
    /// Key was wrong or the database is corrupt; the distinction is intentionally hidden.
    #[error("vault key is invalid or the vault is corrupt")]
    InvalidKeyOrCorrupt,
    /// Linked SQLite lacks SQLCipher support.
    #[error("SQLCipher support is unavailable")]
    CipherUnavailable,
    /// Cipher or database integrity check failed.
    #[error("vault integrity check failed")]
    IntegrityCheckFailed,
    /// On-disk schema is newer than this binary.
    #[error("vault schema version {found} is newer than supported version {supported}")]
    UnsupportedSchema {
        /// Version found on disk.
        found: u32,
        /// Highest version understood by this binary.
        supported: u32,
    },
    /// A migration checksum no longer matches its applied record.
    #[error("migration {version} checksum mismatch")]
    MigrationChecksumMismatch {
        /// Migration version.
        version: u32,
    },
    /// Migration failed and was rolled back.
    #[error("migration {version} failed")]
    MigrationFailed {
        /// Migration version.
        version: u32,
        /// Internal database error.
        #[source]
        source: rusqlite::Error,
    },
    /// Domain policy rejected a record or transition.
    #[error(transparent)]
    Domain(#[from] DomainError),
    /// Serialization of a validated domain object failed.
    #[error("vault record serialization failed")]
    Serialization(#[from] serde_json::Error),
    /// A database operation failed.
    #[error("vault database operation failed")]
    Database(#[source] rusqlite::Error),
    /// A filesystem operation failed.
    #[error("vault filesystem operation failed for {path}")]
    FileSystem {
        /// Affected path.
        path: PathBuf,
        /// Internal I/O error.
        #[source]
        source: std::io::Error,
    },
    /// Operating-system entropy was unavailable.
    #[error("secure random generation failed")]
    RandomUnavailable,
    /// A recovery envelope is malformed, unsupported, or exceeds safe work limits.
    #[error("recovery key envelope is invalid")]
    InvalidRecoveryEnvelope,
    /// Wrong recovery passphrase or authenticated envelope tampering.
    #[error("recovery failed: the passphrase is wrong or the envelope was modified")]
    RecoveryFailed,
    /// The platform credential service is unavailable or rejected the operation.
    #[error("operating-system secret store is unavailable")]
    SecretStoreUnavailable,
    /// No secret exists under the requested opaque reference.
    #[error("secret was not found")]
    SecretNotFound,
    /// A backup or portable artifact is malformed or exceeds a safety limit.
    #[error("vault artifact is invalid")]
    InvalidArtifact,
    /// A backup payload failed its authenticated checksum or database verification.
    #[error("vault artifact was modified or is corrupt")]
    ArtifactTampered,
    /// Requested record was not found.
    #[error("vault record not found")]
    NotFoundRecord,
    /// A record already exists.
    #[error("vault record already exists")]
    Conflict,
}

impl From<rusqlite::Error> for VaultError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Database(error)
    }
}
