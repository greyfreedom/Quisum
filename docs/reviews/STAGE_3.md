# Stage 3 review — providers, recovery, portability, CLI, and MCP

Date: 2026-08-11

## Scope

- Versioned declarative method loading, provider profiles, reflection sessions, open-question review, and provider-bound model provenance
- Hybrid local FTS/hashed-feature retrieval, exact context excerpts, sensitivity propagation, context approval, and final dispatch policy
- OS credential-store adapter, Argon2id/XChaCha20-Poly1305 recovery envelopes, encrypted verified backups, restore, and encrypted pre-migration snapshots
- Bounded JSON/JSONL portability, derived Markdown, full provenance/history graph validation, and empty-vault atomic import
- OpenAI Responses, Anthropic Messages, OpenAI-compatible Chat Completions, and loopback HTTP adapters with bounded non-streaming/SSE handling
- User-authorized CLI workflows and the exact ten-tool proposal-only stdio MCP surface

## Review method

This was an internal engineering self-review, not an external security or professional-method review. The review traced every Stage 3 boundary from adapter input through Core authorization, Vault transactions, provider egress, serialized artifacts, recovery failure, and user-visible CLI output. It exercised valid and hostile cases, then fixed every identified P1/P2 issue before closing the stage.

The hostile cases included forged provider/context provenance, invented context excerpts, imported fake human-review histories, duplicate assistant turns, sensitivity downgrades, unsafe imported language, crisis queries through MCP, local-only/high-sensitivity disclosure attempts, malformed/truncated provider responses, response streaming before validation, oversized artifacts, hostile KDF parameters, tampered backups, old-schema recovery, partial-file failures, credential-reference aliasing, path traversal, and model attempts to invoke human-only actions.

## Findings resolved

| Severity | Finding | Resolution |
|---|---|---|
| P1 | Model-derived events, claims, questions, experiments, or messages could be credible only if their caller-supplied provenance was bound to the actual dispatched provider context. | Every model write now resolves an existing dispatched packet and requires exact method, model, provider identity, eligible evidence, and packet membership; assistant turns also require the immediately preceding matching user query. |
| P1 | Context packets could otherwise preserve forged/stale excerpts, dispatch a record deleted after preview, or use a provider whose trust settings changed before dispatch. | Persistence and final dispatch both resolve every item against its current reviewed/active projection and exact deterministic excerpt. Dispatch atomically rechecks those records plus the enabled profile, model, query sensitivity, item sensitivities, approval, and policy. |
| P1 | A streamed response could expose deltas before discovering unsafe text, a truncated terminal state, or invalid structured output. | SSE is read incrementally but deltas remain buffered until the complete response passes size, terminal-state, local safety, and optional JSON Schema validation. Redirects and ambient proxies are disabled. |
| P1 | Imported records could forge provider provenance, human-confirmed model claims, claim histories, assistant ordering, or context excerpts while remaining individually schema-valid. | Import now validates the complete cross-record graph, reconstructs append-only claim revision chains, pairs human reviews one-to-one with semantic transitions, binds context excerpts and evidence, and rejects branches, gaps, duplicates, deleted/crisis records, and unsupported projections. |
| P1 | Raw portable insertion and MCP context construction could bypass the runtime crisis/overclaim/dependency safety gates. | Import replays user-input or generated-output safety checks by source actor, and MCP context construction performs the same pre-retrieval crisis/third-party routing as CLI. Regression tests cover both bypasses. |
| P1 | An imported `credential_ref` could alias an existing OS keyring entry on the same device, implicitly carrying provider authority even though the artifact contains no secret. | Import disables every provider profile, rotates any credential reference into an import-specific empty namespace, and revalidates the graph. Provider use requires an explicit new/reconfigured profile. |
| P1 | Assistant transcript output could be persisted at a lower sensitivity than the query or included context, or appended with ambiguous/equal timestamps. | Persistence and import require strict turn ordering, a preceding user turn matching the packet query, and assistant sensitivity at least the maximum of session, query, and every context item. |
| P2 | MCP schemas advertised looser limits than runtime and could accept high-sensitivity proposals. | All ten tools now have matching runtime and JSON Schema string/list bounds. Proposal sensitivity is restricted to `normal` or `private`; administrative and human-review verbs remain absent. |
| P2 | Provider protocols accepted missing/partial terminal states and did not consistently bound redirects, proxies, time, body size, or attacker-controlled output schemas. | The HTTP layer has fixed connect/request/body limits, no redirects/proxies, HTTPS-only cloud mode, loopback-only local mode, strict successful stop states, bounded schemas, local schema revalidation, and redacted errors/debug output. |
| P2 | Recovery/setup errors could leave partial recovery JSON, restored databases, WAL/SHM files, or world-readable creation windows. | Exclusive file guards remove partial artifacts and sidecars on failure; new vault, backup, export, restore, and recovery files are atomically created owner-only on Unix. |
| P2 | Restore of an authenticated old-schema `.qsb` created a redundant orphan pre-migration snapshot. | Normal opens still create encrypted pre-migration snapshots; verified restore upgrades against the already-preserved authenticated backup and leaves no redundant snapshot. |
| P2 | Portable export could allocate the full vault before detecting a size violation, and individual JSON records were not independently capped. | SQL preflight estimates bound record count/bytes, every portable record is capped at 1 MiB, complete artifacts at 128 MiB, and all output is re-read and verified before audit/commit. |
| P2 | Bounded semantic scans accidentally discarded old records that were still exact FTS matches. | Recent semantic projections remain bounded, while up to 768 exact FTS hits are merged by typed ID before deterministic privacy filtering and ranking. |
| P2 | Provider enable/revoke and key-store input handling allowed confusing unavailable states or unnecessarily large/invalid secret buffers. | Secrets are non-empty, NUL-free, and capped at 16 KiB before keyring access; enable verifies a referenced credential exists, revoke disables first, and profile policy timestamps advance monotonically. |
| P2 | Supported migration-prefix, ledger-tampering, backup schema authentication, and failure rollback coverage was incomplete. | Tests now upgrade every supported schema prefix, verify append-only names/checksums, prove transaction rollback, authenticate backup schema before migration, and preserve an encrypted rollback source. |

## Verification

```text
node scripts/validate-assets.mjs
  25 JSON schemas/assets
  26 JSONL files
  7 method packages
  21 eval cases

cargo test --workspace --all-features
  5 CLI tests
  34 Core tests
  7 schema/fixture/runtime contract tests
  5 MCP tests
  10 provider protocol tests
  54 encrypted Vault/recovery/portability tests
  all doc tests passed

cargo clippy --workspace --all-targets --all-features -- -D warnings
  passed

cargo fmt --all --check
  passed

RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
  passed

cargo check --workspace --all-targets --all-features
  passed with rustc 1.95.0; the declared 1.85 MSRV remains a CI release gate

git diff --check
  passed
```

A fresh no-keyring CLI run completed event capture, claim proposal and human confirmation, claim-to-open-question conversion, experiment activation/completion, session lifecycle, hybrid search, provider/context preview, provider disable/enable, method validation, reflog, plaintext JSONL/Markdown export, empty-vault import, encrypted backup, restore, and integrity checks. Imported and restored claim/revision projections matched the source. Every generated artifact was mode `0600` on Unix; encrypted vault/backup marker scans found no test plaintext. The temporary artifacts were deleted after verification.

Executable-source scanning found no `TODO`, `FIXME`, `HACK`, `todo!`, `unimplemented!`, `panic!`, `unwrap`, `expect`, conflict marker, private-key block, or credential-shaped token. Debug/error paths for vault keys and provider credentials remain redacted.

## Deferred V1 work

The Tauri/React desktop adapter, executable eval runner/provider compatibility matrix, CI/release packaging, clean-install documentation, checksums/provenance/signing workflows, third-party notices, and final repository-wide audit remain later stages. Official method manifests remain accurately marked `unreviewed`; no external professional or independent security review is claimed.

## Result

No open P0, P1, or P2 issue remains in the Stage 3 scope.
