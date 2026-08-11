# Vault backup and recovery

Quisum uses three different artifacts. Keep their purposes separate:

- The vault (`vault.db`) is the encrypted live database.
- The recovery envelope (`vault.db.recovery.json` by default) contains the random vault key encrypted by the initialization passphrase. It is useless without that passphrase.
- A `.qsb` backup is a self-contained encrypted copy with its own passphrase-wrapped key, integrity metadata, and authenticated header.

Portable JSON, JSONL, and Markdown exports are plaintext and are not recovery backups.

## Create and verify a backup

Choose a destination outside the application-data directory and outside the source repository. Quisum never overwrites an existing artifact.

```sh
quisum backup /secure/offline/location/quisum-2026-08-12.qsb
```

Enter a new backup passphrase twice. A successful command checkpoints the database, writes a private file, verifies its structure and encrypted payload, reopens its key envelope, and records a content-free backup audit event. Copy the artifact to at least one separate device; do not store the only copy next to the computer it protects.

Test the backup into a new, empty destination:

```sh
quisum restore /secure/offline/location/quisum-2026-08-12.qsb /tmp/quisum-restore-test/vault.db
quisum --vault /tmp/quisum-restore-test/vault.db doctor
```

Restore never overwrites an existing vault. It creates a new recovery envelope beside the destination and attempts to register the recovered key with the OS credential store. Inspect content using synthetic data, then securely remove the test destination and its recovery envelope.

## Recover the live vault without the OS credential store

If the vault and its recovery envelope still exist but the credential-store entry is missing, unlock from the passphrase-protected envelope:

```sh
quisum --vault /path/to/vault.db \
  --recovery /path/to/vault.db.recovery.json \
  --no-keyring status
```

The passphrase is read from an interactive terminal, not a command-line argument. For a controlled noninteractive process, use `--unlock-passphrase-stdin`; provide exactly one line through a protected pipe and ensure shell history and logs cannot capture it.

## Recover from a `.qsb` backup

On a clean machine, install the same or a newer compatible Quisum version, then restore to a new path:

```sh
quisum restore /media/offline/latest.qsb /safe/local/path/vault.db
quisum --vault /safe/local/path/vault.db doctor
```

The restore authenticates the header, key envelope, encrypted database hash, vault identity, schema version, and database integrity before success. A wrong passphrase or altered artifact leaves no destination vault.

## Migration snapshots

Opening an older supported schema first creates an encrypted same-key snapshot beside the vault, with a name containing `.pre-migration-v<version>-`. It then applies every migration transactionally and verifies the append-only migration ledger. This snapshot is not a passphrase-independent `.qsb` backup; retain the matching recovery envelope or OS key while it is needed. See [migration policy](MIGRATIONS.md).

## Failure rules

- Do not edit a vault, recovery envelope, backup, or migration snapshot.
- Do not restore over the only surviving copy.
- If `doctor` fails, stop writing to the vault, preserve exact copies, and report the redacted error category through the [security policy](../../SECURITY.md).
- Quisum cannot recover a lost vault key when both the OS credential entry and a usable passphrase artifact are gone.
- Backups protect availability, not an already unlocked or compromised device.
