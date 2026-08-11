# Privacy, export, and deletion

Quisum has no account, sync, telemetry, analytics, updater, or crash-upload service. Primary data stays in the encrypted local vault. That design limits routine disclosure; it does not protect an unlocked process, screenshots, OS swap, a compromised endpoint, or context you explicitly approve for a provider.

## What can leave the device

Only Rust provider adapters can make model requests. Before egress, Quisum builds an immutable context packet with purpose, method, provider/model, query, selected excerpts, exclusions, sensitivity, and approval state. `local_only`, crisis-related, rejected, deleted, and ineligible records are excluded. The WebView and MCP surface cannot bypass dispatch policy.

The provider still receives the approved query and excerpts. Review the provider's own retention and training terms; a profile retention note is user-supplied metadata, not a guarantee by Quisum.

## Plaintext export

JSON and JSONL are portable, checked data formats. Markdown is a derived human-readable view and is not lossless. Every export is plaintext and requires explicit acknowledgement:

```sh
quisum export /private/destination/quisum-export.jsonl \
  --format jsonl \
  --acknowledge-plaintext
```

Portable checksums detect accidental or malicious changes after export; they are not digital signatures and do not prove authorship. Imports require an empty vault and validate sizes, schemas, lifecycle transitions, revisions, provenance, checksums, and cross-record references atomically. Imported provider profiles are disabled and never inherit credentials.

Use encrypted `.qsb` files—not exports—for recovery. Delete obsolete plaintext exports using operating-system tools appropriate to the storage medium; flash storage, snapshots, cloud-sync history, and backups may retain copies beyond a normal delete.

## Delete individual records

Event deletion is irreversible and transactionally repairs or purges dependent claims:

```sh
quisum event delete <event-id> --yes
```

Session deletion removes its transcript and local retrieval vector, and repairs claims that cited its user messages:

```sh
quisum session delete <session-id> --yes
```

The content-free audit trail records that an entity was deleted without preserving its text. Some claim histories remain as necessary to explain later revisions; active projections and exports exclude deleted source content.

## Revoke provider credentials

Before removing a vault, revoke every provider credential from Privacy & Models or the CLI:

```sh
quisum provider revoke-secret <provider-id>
```

This disables new calls and removes the secret from the OS credential facility. Deleting only the vault cannot reliably enumerate or remove credentials afterward.

## Delete the whole vault

V1 RC does not pretend that uninstalling is data deletion. First unlock the vault, revoke provider credentials, make or deliberately decline a final encrypted backup, then lock and quit every Quisum process. Read the exact vault and recovery paths from `quisum status` or the desktop Privacy & Models screen.

Delete all of the following that you intentionally want removed:

- the vault and any `-wal` / `-shm` sidecars;
- the adjacent recovery envelope;
- `.pre-migration-...` encrypted snapshots;
- selected `.qsb` backups and every copy of plaintext exports;
- the Quisum vault-key entry in the OS credential store if it was saved.

The current RC intentionally leaves whole-vault erasure as an explicit platform operation because no cross-platform API can guarantee secure physical erasure on snapshots or flash media. Follow the operating system's documented credential and file deletion procedures. Verify paths one by one—never use a recursive wildcard against a home or application-data directory. The [V1 readiness matrix](../V1_READINESS.md) keeps clean-install/deletion validation open until this procedure is exercised on all supported systems.

## Limits of deletion

Quisum cannot delete data already sent to a provider, copied into a plaintext export, included in an OS/cloud backup, captured in a screenshot, or retained by storage snapshots. Use provider account controls for remote data. On copy-on-write or flash storage, cryptographic erasure by removing the only key is generally more meaningful than claiming a file overwrite, but Quisum does not certify physical erasure.
