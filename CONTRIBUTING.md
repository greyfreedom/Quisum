# Contributing to Quisum

Thank you for helping build user-owned self-inquiry infrastructure.

## Before contributing

Read the project charter, epistemic policy, safety policy, privacy model, and threat model. A feature that bypasses user review, increases model authority, or broadens data disclosure will not be accepted even when it improves convenience.

Use synthetic data only. Do not include real personal histories in source, fixtures, screenshots, issues, or pull requests.

## Development

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
cargo doc --workspace --all-features --no-deps --locked
cargo run --locked -p quisum-evals --bin quisum-eval -- --json
node scripts/validate-assets.mjs
node scripts/check-skills.mjs
bash scripts/check-workflows.sh
npm ci --prefix apps/desktop
npm run check --prefix apps/desktop
npm run build --prefix apps/desktop
node scripts/check-release.mjs
```

Provider transport tests bind a loopback port and may require an environment that permits local sockets. Frontend commands are also documented in `apps/desktop/README.md`.

## Change requirements

- Add tests for domain, migration, permission, or safety behavior.
- Preserve append-only revision and audit history.
- Document migrations and keep downgrade/recovery behavior explicit.
- Add or update an eval case for model-facing safety changes.
- Keep logs and errors free of conversation bodies and secrets.
- Explain privacy-impacting changes in the pull request.

Method packages follow `docs/METHOD_AUTHORING.md`. After changing a canonical method, run `node scripts/sync-skills.mjs`, review the generated installable references, and rerun the skill check. Security issues follow `SECURITY.md` rather than the public issue tracker.

## Provider compatibility contributions

Provider behavior changes over time. A compatibility change must name the exact adapter kind, endpoint protocol, model identifier, capability declaration, test date, and whether the evidence came from a synthetic loopback fixture or a live endpoint. Never commit credentials, response bodies containing personal data, or vendor account identifiers. Submit live evaluation results only in the bounded format accepted by `quisum-eval --provider-results`; passing results are evidence for that exact run, not a blanket provider certification. See `docs/PROVIDER_COMPATIBILITY.md`.

## Review labels

Use P0 for actively exploitable catastrophic loss or disclosure, P1 for serious security/safety/integrity defects, P2 for material correctness or usability defects, and P3 for lower-impact improvements. Each implementation stage must resolve all known P0–P2 findings before acceptance. Maintainer self-review must never be described as independent security review or qualified professional method review.
