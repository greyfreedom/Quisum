# Stage 4 review — secure desktop reference client

Date: 2026-08-12

## Scope

- Tauri 2 application shell and React/TypeScript interface for Explore, Self Map, Review, Timeline, Reflog, Experiments, and Privacy & Models
- Encrypted-vault creation, keyring/recovery unlock, explicit lock, integrity diagnostics, encrypted backup, and fresh-target restore
- Provider configuration, credential isolation, connection testing, exact context preview/approval, independent reflection sessions, and transcript lifecycle
- Event capture/deletion, candidate creation, exact evidence/provenance inspection, human review, counterevidence, open questions, and behavioral experiments
- Plaintext JSON/JSONL/Markdown export warnings, atomic empty-vault import, production CSP, desktop icons, and cross-platform bundle metadata

## Review method

This was an internal engineering self-review, not an external security assessment, accessibility certification, or professional review of the bundled methods. The review traced every WebView command through Rust input parsing, actor authorization, Vault transactions, provider egress, secret-store calls, and user-visible error handling. It also reviewed the production bundle, generated Tauri capability manifest, CSP, dependency lockfiles, and secret-bearing forms.

The seven desktop surfaces were exercised in the in-app browser at a 1280×720 desktop viewport using synthetic fixtures. Explore, Timeline, Self Map, Review, Reflog, Experiments, and Privacy & Models rendered without console warnings or errors. The fixture-only preview path was then removed before the production build, and the built JavaScript was scanned for its markers.

## Findings resolved

| Severity | Finding | Resolution |
|---|---|---|
| P1 | A failed second exclusive recovery-envelope write could remove the pre-existing recovery file. | Cleanup now occurs only when the current invocation created the file; regression coverage verifies unchanged bytes and Unix mode `0600`. |
| P1 | Safety prechecks covered primary text but secondary durable fields could carry acute-risk language. | Complete user-authored payloads are rechecked at the Vault boundary, including event topics, claim review text, session titles, context purpose/query, and experiment results; a multi-surface regression test proves rollback/no persistence. |
| P2 | The initial Tauri `core:default` grant exposed unnecessary window, event, path, image, menu, and tray commands. | The sole local window now has an empty core/plugin permission list. Only the exact application commands registered by the Rust invoke handler remain callable. |
| P2 | Writing a provider credential before checking the unlocked vault could leave an inaccessible secret, and a later metadata failure lacked cleanup. | Provider creation first obtains the authorized Vault handle; a metadata failure triggers best-effort deletion of the newly created opaque secret reference. |
| P2 | A disabled provider-boundary selector was omitted from browser `FormData`, breaking creation of fixed-boundary OpenAI, Anthropic, and local profiles. | A state-bound hidden value carries the enforced boundary while the explanatory selector remains disabled; a frontend regression test asserts the Rust payload. |
| P2 | Claim review showed evidence counts but not enough material to judge a proposal. | Review now shows every evidence ID, available concrete event facts, full model provenance, and the immutable context packet's exact request/excerpts before the decision form. |
| P2 | Requested keyring persistence could fail silently after successful create, recovery unlock, or restore. | Rust continues to preserve the usable unlocked vault and returns `keyringSaved`; the React shell now keeps a dismissible warning explaining that the recovery passphrase will be needed after restart. |
| P2 | A rare metadata-read failure after new vault creation could leave a vault/recovery pair that blocked a clean retry. | The creation adapter drops the connection and removes only the just-created vault sidecars and recovery envelope before returning the redacted failure. |
| P2 | The context UI allowed a 4,000-character request while local retrieval rejected more than 500 characters. | Local hybrid retrieval now accepts the same bounded 4,000-character request as the context schema; FTS terms, hashed features, result counts, and scans remain independently capped. |
| P2 | Raising the Vault query ceiling could silently loosen MCP's advertised 500-character tool contract. | MCP now enforces its own explicit 500-character runtime bound before calling Vault; the existing schema/runtime regression test caught and verifies the boundary. |
| P2 | A failed close audit could consume the unlocked Vault while leaving the React shell visually unlocked. | Status refresh now runs in a `finally` path, and the lock surface catches and reports the redacted close failure. |
| P2 | A development-only synthetic preview branch risked becoming an accidental alternate runtime surface. | It was used only for visual QA, removed from source, and production output was scanned for preview and synthetic-path markers. |

## Least-privilege boundary

- React has no direct database, filesystem, network, keyring, shell, updater, telemetry, crash-upload, or arbitrary Tauri core capability.
- Production `connect-src` is limited to Tauri IPC. Provider HTTP runs only in the hardened Rust adapter with no redirects or ambient proxy inheritance.
- Recovery passphrases and API credentials enter uncontrolled password inputs, are copied once for IPC, and are cleared before the command promise settles. Rust immediately wraps received secret strings in zeroizing storage.
- Command failures return stable, redacted categories. Filesystem paths, provider response bodies, prompts, authorization values, database keys, and API credentials are absent from errors.
- Plaintext portability and encrypted recovery remain separate, visibly labeled operations; all destination writes are exclusive and never overwrite an existing artifact.

## Verification

```text
npm run check
  strict TypeScript passed
  5 Vitest tests passed

npm run build
  production Vite build passed
  preview/synthetic marker scan returned no matches

npm run tauri -- build --no-bundle
  optimized Rust + production frontend build passed
  application binary produced

npm run tauri -- build --bundles app
  unsigned macOS application bundle produced
  all 7 official method packages present in bundle resources

cargo test --workspace --all-features
  121 Rust unit/integration tests passed
  all workspace doc tests passed

cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all --check
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
node scripts/validate-assets.mjs
git diff --check
  passed
```

The final workspace-wide tests, lint, documentation, eval runner, CI/release workflows, clean-install guides, third-party notices, and release-candidate audit remain Stage 5 work. Official method packages remain accurately marked `unreviewed`; no signing, external security review, or qualified professional method review is claimed.

## Result

No open P0, P1, or P2 issue remains in the Stage 4 scope.
