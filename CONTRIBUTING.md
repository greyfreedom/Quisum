# Contributing to Quisum

Thank you for helping build user-owned self-inquiry infrastructure.

## Before contributing

Read the project charter, epistemic policy, safety policy, privacy model, and threat model. A feature that bypasses user review, increases model authority, or broadens data disclosure will not be accepted even when it improves convenience.

Use synthetic data only. Do not include real personal histories in source, fixtures, screenshots, issues, or pull requests.

## Development

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
node scripts/validate-assets.mjs
```

Frontend commands are documented in `apps/desktop/README.md` once that application is bootstrapped.

## Change requirements

- Add tests for domain, migration, permission, or safety behavior.
- Preserve append-only revision and audit history.
- Document migrations and keep downgrade/recovery behavior explicit.
- Add or update an eval case for model-facing safety changes.
- Keep logs and errors free of conversation bodies and secrets.
- Explain privacy-impacting changes in the pull request.

Method packages follow `docs/METHOD_AUTHORING.md`. Security issues follow `SECURITY.md` rather than the public issue tracker.
