# Release process

Quisum treats a version number, a GitHub job, and a trustworthy release as different things. The repository can build a release candidate automatically; stable V1.0 remains blocked until every required gate has genuine evidence.

## 1. Prepare the candidate

1. Update the workspace, frontend, lockfile, Tauri, readiness, and changelog versions together.
2. Verify every official method manifest. Do not upgrade a review label without traceable qualified review.
3. Recheck the versioned crisis-resource sources before `source_check_due`; remove stale regional details when they cannot be verified.
4. Regenerate `THIRD_PARTY_NOTICES.md` from both lockfiles.
5. Review migrations, public schemas, security/privacy/safety policies, provider compatibility, and user guides.

Run the complete local gate:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
cargo run --locked -p quisum-evals --bin quisum-eval -- --json
node scripts/validate-assets.mjs
node scripts/check-skills.mjs
bash scripts/check-workflows.sh
node scripts/generate-notices.mjs --check
node scripts/check-release.mjs
npm ci --prefix apps/desktop
npm run check --prefix apps/desktop
npm run build --prefix apps/desktop
npm run tauri --prefix apps/desktop -- build --no-bundle -- --locked
```

Provider transport tests bind a local loopback socket. Record exact totals and failures in the stage review. A maintainer self-review is not independent security review.

## 2. Validate readiness

`release/readiness.json` is authoritative machine-readable state. An open gate must have null evidence; a completed gate must point to a real repository file. The release checker enforces version alignment, current crisis resources, official method review metadata, required documentation, action SHA pinning, tag/version identity, and stable-gate closure.

```sh
node scripts/check-release.mjs --tag v1.0.0-rc.1
```

For a stable version, the same command additionally rejects every `required_for_stable` gate that is still open and requires all official methods to have traceable `professional` review metadata. Never create evidence whose only purpose is to satisfy the parser.

## 3. Tag and build

Create a signed Git tag only after reviewing the exact commit:

```sh
git tag -s v1.0.0-rc.1 -m "Quisum 1.0.0-rc.1"
git push origin v1.0.0-rc.1
```

The release workflow re-runs the repository gates, creates a draft GitHub release, builds native desktop bundles on Windows, macOS, and Linux through the pinned Tauri action, and packages the CLI with `share/quisum/methods`, `share/quisum/skills`, and the no-overwrite skill installer. It then publishes:

- native desktop bundles;
- platform CLI archives;
- `SHA256SUMS` covering release assets;
- an SPDX JSON SBOM;
- GitHub build-provenance attestations.

Release candidates are visibly titled **UNSIGNED release candidate**. The workflow leaves every release as a draft.

## 4. Inspect the draft

On clean, non-developer accounts for each platform:

1. Verify tag signature, provenance, and every checksum.
2. Confirm expected signing/notarization or the explicit RC unsigned warning.
3. Install without source-tree paths or existing application data.
4. Create a synthetic vault and restart/unlock it.
5. Complete event → candidate → evidence/counterevidence → review → reflog → export in CLI and desktop.
6. Exercise wrong-passphrase, backup, restore, import, deletion, provider test, context preview/approval, and crisis-route cases.
7. Confirm no secrets or bodies appear in logs, diagnostics, process arguments, frontend state, or release metadata.
8. Link the hosted workflow, test record, signing verification, and reviewer dispositions in release evidence.

Do not publish a draft with a failed/missing platform, mismatched checksum, missing attestation, stale source pack, unresolved P0–P2 issue, or ambiguous signing state.

## 5. Stable promotion

Stable V1.0 additionally requires the external gates in [V1 readiness](V1_READINESS.md): independent security review, qualified professional review of every official method, accepted live provider evidence, clean-install/deletion validation, and applicable macOS/Windows signing. Update the readiness record and method manifests only after those events occur, then rerun all checks on a new candidate commit.

Publishing is a deliberate maintainer action in the GitHub UI after draft inspection. Never convert an unsigned RC artifact into a stable asset by renaming it.

## 6. Failure, revocation, and rollback

- A failed workflow leaves a draft; delete or clearly mark unusable assets rather than reusing their version.
- If an artifact or key is compromised, keep the release record, mark it withdrawn, revoke the credential with its authority, publish the impact, and issue a new version.
- Do not mutate a published artifact under an existing filename or checksum.
- Data migrations are forward-only. Users roll back by restoring a pre-upgrade encrypted backup into a new destination, not by opening a newer vault with an older binary.
