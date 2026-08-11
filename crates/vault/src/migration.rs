use chrono::{SecondsFormat, Utc};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};

use crate::VaultError;

/// Highest on-disk schema version understood by this build.
pub const LATEST_SCHEMA_VERSION: u32 = 1;

struct Migration {
    version: u32,
    name: &'static str,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "initial",
    sql: include_str!("../migrations/0001_initial.sql"),
}];

pub(crate) fn migrate(connection: &mut Connection) -> Result<(), VaultError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY NOT NULL,
            name TEXT NOT NULL,
            checksum TEXT NOT NULL,
            applied_at TEXT NOT NULL
         ) STRICT;
         CREATE TRIGGER IF NOT EXISTS schema_migrations_no_update
         BEFORE UPDATE ON schema_migrations
         BEGIN
           SELECT RAISE(ABORT, 'schema migrations are append-only');
         END;
         CREATE TRIGGER IF NOT EXISTS schema_migrations_no_delete
         BEFORE DELETE ON schema_migrations
         BEGIN
           SELECT RAISE(ABORT, 'schema migrations are append-only');
         END;",
    )?;

    let user_version: u32 =
        connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let applied_version: u32 = connection.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )?;
    let found = user_version.max(applied_version);
    if found > LATEST_SCHEMA_VERSION {
        return Err(VaultError::UnsupportedSchema {
            found,
            supported: LATEST_SCHEMA_VERSION,
        });
    }
    if user_version != applied_version {
        return Err(VaultError::IntegrityCheckFailed);
    }

    for migration in MIGRATIONS {
        let checksum = checksum(migration.sql);
        let applied_checksum = connection
            .query_row(
                "SELECT checksum FROM schema_migrations WHERE version = ?1",
                [migration.version],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        if let Some(applied_checksum) = applied_checksum {
            if applied_checksum != checksum {
                return Err(VaultError::MigrationChecksumMismatch {
                    version: migration.version,
                });
            }
            continue;
        }

        apply_migration(connection, migration, &checksum)?;
    }

    Ok(())
}

fn apply_migration(
    connection: &mut Connection,
    migration: &Migration,
    checksum: &str,
) -> Result<(), VaultError> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|source| VaultError::MigrationFailed {
            version: migration.version,
            source,
        })?;
    if let Err(source) = transaction.execute_batch(migration.sql) {
        return Err(VaultError::MigrationFailed {
            version: migration.version,
            source,
        });
    }
    let applied_at = Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true);
    if let Err(source) = transaction.execute(
        "INSERT INTO schema_migrations(version, name, checksum, applied_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![migration.version, migration.name, checksum, applied_at],
    ) {
        return Err(VaultError::MigrationFailed {
            version: migration.version,
            source,
        });
    }
    if let Err(source) = transaction.pragma_update(None, "user_version", migration.version) {
        return Err(VaultError::MigrationFailed {
            version: migration.version,
            source,
        });
    }
    transaction
        .commit()
        .map_err(|source| VaultError::MigrationFailed {
            version: migration.version,
            source,
        })
}

fn checksum(sql: &str) -> String {
    hex::encode(Sha256::digest(sql.as_bytes()))
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    #[test]
    fn migration_is_idempotent_and_records_checksum() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        migrate(&mut connection)?;
        migrate(&mut connection)?;
        let count: u32 =
            connection.query_row("SELECT COUNT(*) FROM schema_migrations", [], |row| {
                row.get(0)
            })?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        assert_eq!(count, 1);
        assert_eq!(version, LATEST_SCHEMA_VERSION);
        Ok(())
    }

    #[test]
    fn future_schema_is_rejected() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        connection.pragma_update(None, "user_version", LATEST_SCHEMA_VERSION + 1)?;
        let result = migrate(&mut connection);
        assert!(matches!(result, Err(VaultError::UnsupportedSchema { .. })));
        Ok(())
    }

    #[test]
    fn checksum_tampering_is_rejected() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        migrate(&mut connection)?;
        connection.execute_batch("DROP TRIGGER schema_migrations_no_update")?;
        connection.execute(
            "UPDATE schema_migrations SET checksum = 'tampered' WHERE version = 1",
            [],
        )?;
        assert!(matches!(
            migrate(&mut connection),
            Err(VaultError::MigrationChecksumMismatch { version: 1 })
        ));
        Ok(())
    }

    #[test]
    fn failed_migration_rolls_back_schema_and_version() -> Result<(), Box<dyn Error>> {
        let mut connection = Connection::open_in_memory()?;
        migrate(&mut connection)?;
        let broken = Migration {
            version: 2,
            name: "synthetic_failure",
            sql: "CREATE TABLE should_rollback(id INTEGER); THIS IS NOT SQL;",
        };
        assert!(matches!(
            apply_migration(&mut connection, &broken, &checksum(broken.sql)),
            Err(VaultError::MigrationFailed { version: 2, .. })
        ));
        let table_exists: bool = connection.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM sqlite_master
               WHERE type = 'table' AND name = 'should_rollback'
             )",
            [],
            |row| row.get(0),
        )?;
        let version: u32 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        assert!(!table_exists);
        assert_eq!(version, 1);
        Ok(())
    }
}
