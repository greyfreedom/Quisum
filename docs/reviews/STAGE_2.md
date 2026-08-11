# Stage 2 review — domain core and encrypted vault

Date: 2026-08-11

## Scope

- Typed identifiers, provenance, sensitivity, actor capabilities, and validation primitives
- Event, claim, review, revision, correction, open-question, experiment, context-packet, audit, and safety domain policies
- SQLCipher vault creation/open/close, zeroizing key wrapper, integrity checks, owner-only Unix permissions, and redacted failures
- Versioned transactional migrations, relational projections, append-only records, FTS eligibility, evidence links, and audit records
- Event/claim/experiment review lifecycles, counterevidence review, context approval/dispatch, and evidence-deletion repair
- Runtime-to-JSON-Schema contract tests and synthetic encrypted-vault tests

## Review method

The review traced the Stage 1 epistemic, privacy, safety, and authorization invariants through Core transitions, Vault transactions, SQLite constraints, search indexes, and serialized public contracts. It exercised both permitted flows and hostile/invalid flows: wrong keys, forged model provenance, model attempts at final-state mutation, high-sensitivity egress without approval, unsafe generated language, unreviewed evidence, transaction failure, migration failure/tampering, evidence deletion, and direct audit mutation.

Every P1/P2 finding was fixed and regression-tested before this stage was closed.

## Findings resolved

| Severity | Finding | Resolution |
|---|---|---|
| P1 | Core allowed a model-host actor to apply counterevidence directly, and a model host could mark a context packet dispatched. | Applying counterevidence now requires a user review capability and user actor; model counterevidence remains a pending proposal. Only user/system egress paths can mark dispatch. |
| P1 | `revise_and_confirm` collapsed into `confirmed`, while model reads omitted the accepted `revised` state. | Edited acceptance now produces `revised`; accepted-state retrieval and FTS eligibility include both `confirmed` and `revised`. |
| P1 | A model-derived candidate could cite an invented context packet or evidence not present in the dispatched packet. | Model provenance now requires an existing dispatched packet with matching method/model; claim evidence, related experiment claims, and counterevidence proposals must be present in that packet. |
| P1 | Highly sensitive records could be returned by direct model-host bounded reads. | Direct model-host reads now exclude `highly_sensitive` and `local_only`; highly sensitive egress is available only through a one-time user-approved packet. |
| P1 | Proposed/rejected events and pending/rejected claims were inserted into FTS before human acceptance. | Search indexes now contain only confirmed events and accepted/contradicted claims; every lifecycle update removes stale index rows before conditionally reindexing. |
| P1 | Failed domain transitions could leave an in-memory claim or experiment partially mutated. | Transitions validate a cloned projection and replace the original only after success; failure-atomic regression tests cover claim and experiment paths. |
| P1 | Crisis-tagged or deterministically unsafe model candidates could reach durable proposal storage. | Event, claim, and experiment writes block crisis candidates, and model payloads pass the provider-independent overclaim/false-memory/dependency safety gate before a transaction begins. |
| P1 | The `System` actor inherited every user capability. | System is now restricted to bounded read/search, context dispatch, session summaries, and vault maintenance; it cannot review, delete, export, or change privacy. |
| P2 | Rust audit records omitted the public schema's `entity_type`, and several new lifecycle actions were not constrained in SQLite. | Audit entity type is derived from the typed ID and validated against it; Rust, JSON Schema, and SQLite action/entity constraints now match. |
| P2 | Evidence grades could overstate a one-record evidence set or ignore present counterevidence. | `moderate`/`repeated` require at least two supporting records in active states, and any counterevidence forces the `contested` grade; public schema conditions mirror Core. |
| P2 | Deleting the sole source of a user-marked uncertain hypothesis could roll back the entire deletion. | An uncertain hypothesis may remain explicitly unsupported and contested; confirmed claims are contradicted and unreviewed sole-source claims are purged transactionally. |
| P2 | Context items were not checked against stored status/sensitivity immediately before packet persistence. | The system-only persistence path verifies item kind, existence, accepted/active state, exact sensitivity, crisis exclusion, and current-state expiry. |
| P2 | Migration rollback/checksum behavior, file/sidecar permissions, and plaintext absence lacked direct tests. | Tests now cover tampered checksums, failed migration rollback, wrong keys, SQLCipher integrity, ciphertext headers/content, owner-only Unix modes, and no plaintext marker in DB/WAL/SHM files. |
| P2 | A failed multi-record review write could have gone unverified for atomicity. | A fault-injection trigger proves projection, revision, review, and audit all roll back together. |

## Verification

```text
node scripts/validate-assets.mjs
  25 JSON schemas/assets
  26 JSONL files
  7 method packages
  21 eval cases

cargo test --workspace --all-features
  25 Core unit tests
  7 schema/fixture/runtime contract tests
  21 encrypted Vault and migration tests
  all workspace and doc tests passed

cargo clippy --workspace --all-targets --all-features -- -D warnings
  passed

cargo fmt --all --check
  passed

RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
  passed

cargo +1.88 check --workspace --all-targets --all-features
  passed

git diff --check
  passed
```

Executable-source scanning found no `TODO`, `FIXME`, `HACK`, `todo!`, `unimplemented!`, `unwrap`, `expect`, or debug rendering of key material. Synthetic marker tests found no durable plaintext in the encrypted database or its sidecars.

## Deferred V1 work

Key derivation/keychain integration, encrypted backup/restore, portable import/export, provider transports, MCP/CLI surfaces, semantic retrieval, and desktop UI are later planned stages and are not exposed as completed features by this stage. External professional method review and release signing remain explicit release gates and are not claimed.

## Result

No open P0, P1, or P2 issue remains in the Stage 2 scope.
