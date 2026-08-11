# Schema and migration policy

The `1.0.0-rc.1` vault schema is version 4. Schema versioning is independent of application SemVer and method-package versions.

## Guarantees

- Migrations are ordered, contiguous SQL files compiled into the Vault crate.
- Every applied version records its stable name, SHA-256 migration checksum, and timestamp in an append-only ledger.
- The SQLite `user_version`, ledger maximum, and ledger count must agree before any migration runs.
- Each migration and its ledger update commit in one immediate transaction.
- A database created by a newer unsupported schema is rejected without downgrade attempts.
- Opening an older supported schema creates an encrypted pre-migration snapshot before applying changes.
- No plaintext database fallback exists.

Tests create every supported schema prefix, upgrade it to the latest version, repeat the migration, tamper with checksums, inject a failing migration, and reject a future version.

## Upgrade procedure

1. Close other Quisum processes and create a verified `.qsb` backup with the old version.
2. Preserve the matching recovery information.
3. Install the new build and run `quisum doctor` against the intended vault.
4. Open the vault. Migration runs automatically after cipher verification and before normal access.
5. Run `quisum doctor` again and inspect the content-free schema/audit summary.
6. Keep the `.qsb` backup and encrypted pre-migration snapshot until the upgraded vault has been exercised.

The snapshot filename contains `.pre-migration-v<old-version>-<timestamp>`. It uses the same SQLCipher key as the original and is not a standalone recovery envelope.

## Failure and rollback

If migration fails, the active migration transaction rolls back and Quisum returns a redacted category. Stop. Preserve the vault, recovery envelope, `.qsb` backup, and pre-migration snapshot without editing them. Do not copy individual SQLite pages or change `user_version` manually.

Quisum has no in-place downgrade path. To return to an older application version, restore the verified pre-upgrade `.qsb` backup into a new destination using that compatible version. Never point an older binary at a successfully upgraded vault.

## Authoring a migration

Append exactly one numbered file in `crates/vault/migrations/`, register it after the current final entry, and increment `LATEST_SCHEMA_VERSION`. Never edit, rename, reorder, or reuse a released migration. Add tests for:

- every previous supported prefix;
- idempotent reopen;
- transaction rollback on an injected failure;
- checksum/name tampering;
- backup/restore across the change;
- deletion and provenance repairs affected by the schema.

Document user-visible effects in [CHANGELOG.md](../../CHANGELOG.md) and update portability schemas separately when the public format changes.
