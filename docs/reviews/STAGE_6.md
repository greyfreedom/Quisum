# Stage 6 internal review — release candidate engineering

Date: 2026-08-12

Scope: repository-wide V1 version alignment, minimum Rust version, installable Agent Skills, user and maintainer documentation, dependency notices, release-readiness state, CI/release workflows, CLI packaging, checksums, SBOM/provenance automation, and native Tauri release-candidate packaging.

This is a maintainer self-review of the repository and release automation. It is not a successful hosted multi-platform run, clean-device validation, independent security review, qualified professional method review, live-provider certification, or platform-signing review.

## Findings resolved before acceptance

- **P1 — the declared Rust 1.85 baseline could not build the locked dependency graph.** Dependency metadata requires versions through Rust 1.88. The workspace, CI, release workflow, and installation documentation now use Rust 1.88, and the complete locked workspace was checked, linted, tested, and documented with that exact minimum toolchain.
- **P1 — the initial release workflow used a nonexistent Tauri action input and allowed release creation to be coupled to the build matrix.** The workflow now creates or reuses one draft in a singleton job, passes its release ID to every builder, uses the action's documented `uploadUpdaterJson` and `uploadUpdaterSignatures` inputs, and verifies the exact tag, draft state, and prerelease state before uploading.
- **P1 — a signature marker was initially mistaken for cryptographic tag verification.** The guard still requires an annotated PGP/SSH signature but now also requires GitHub's tag-object API to report that signature as verified; a pasted or invalid signature block cannot pass.
- **P1 — the original host-AI milestone still contained only placeholder skill directories.** They are replaced by eight focused, cross-host Agent Skills initialized and validated with the Codex skill tooling. Seven generated references are derived from canonical versioned method assets; profile curation is bounded by every method's memory policy. All skills stop persistence during acute-risk routing, disclose unreviewed status, require explicit write consent and exact provenance, and exclude human-only authority.
- **P1 — the first host guide registered `quisum mcp`, which prints help instead of serving stdio.** Codex and Claude Code registration commands now invoke the executable's required `mcp serve` subcommand and were checked against the CLI help surface.
- **P2 — third-party actions were initially referenced by mutable version tags.** Every `uses:` reference is pinned to a full commit SHA, and the release checker rejects mutable references.
- **P2 — the first action-reference matcher did not account for YAML list markers and therefore inspected zero real `uses:` entries.** The checker now requires and validates every `- uses:` entry; a mutable-reference negative test fails the release gate.
- **P2 — several CI and Tauri runner invocations did not initially force the checked-in lockfile.** All dependency-bearing Cargo commands now use `--locked`, including arguments passed through the Tauri CLI.
- **P2 — the project summary implied that derived Markdown could be imported.** The V1 scope now distinguishes lossless JSON/JSONL import/export from one-way derived Markdown export.
- **P2 — the workflow checker assumed GNU `sha256sum --check --status`, which is not portable to the compatibility binary available on macOS.** It now computes the digest with `shasum` or `sha256sum` and compares the first output field. The pinned actionlint binary and digest were exercised successfully on macOS.
- **P2 — checksum generation initially allowed an output path outside the artifact root and used a predictable temporary name.** The generator now requires a direct-child output file, rejects symlinks and newline-bearing paths, excludes its own output, and commits through an unpredictable atomic temporary file. Repeated runs over the same package produced identical manifests.
- **P2 — local DMG generation entered Tauri's interactive Finder layout path and timed out.** The release path is documented and tested with `CI=true`, which selects Tauri's non-interactive `--skip-jenkins` path. The resulting unsigned DMG passed `hdiutil verify`.
- **P2 — SBOM generation could have uploaded release assets independently of the controlled finalization job.** Automatic release-asset upload is disabled; the finalization job alone uploads the SPDX document, checksums, and attestations to the existing draft.
- **P2 — a source-tree-dependent CLI archive could appear valid while omitting official methods, and the initial copier did not reject symlinked package inputs.** Packaging now rejects recursive output placement and every symlink/special input, installs all seven method packages under `share/quisum/methods`, and the extracted binary discovered and validated them while run outside the repository.
- **P2 — the first release archive design did not carry the host skills or a safe update path.** CLI packages now include all eight skills, generated references, the host guide, and a transactional no-overwrite installer. The installer requires a dedicated absolute `skills` directory and an exact non-executable package layout; it rejects source extras, symlinks and special files, existing targets, and partial ordinary failures. Top-level targets are added to rollback ownership only after exclusive creation, so a raced external target cannot be deleted; success and collision behavior are exercised for both hosts.
- **P2 — skill installation tests initially checked only for each copied `SKILL.md`, and package-output containment was lexical.** Tests now compare every installed path and byte against its source for both hosts. Packaging requires an existing non-symlink output parent, resolves it before containment checks, rejects ancestor-symlink aliases into source assets, exclusively creates its output, and removes partial output after ordinary failure.

## Evidence

- `rustup run 1.88-aarch64-apple-darwin cargo check --workspace --all-targets --all-features --locked`
- `rustup run 1.88-aarch64-apple-darwin cargo fmt --all --check`
- `rustup run 1.88-aarch64-apple-darwin cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `rustup run 1.88-aarch64-apple-darwin cargo test --workspace --all-features --locked` — 134 Rust tests passed; documentation test harnesses also passed
- `RUSTDOCFLAGS="-D warnings" rustup run 1.88-aarch64-apple-darwin cargo doc --workspace --all-features --no-deps --locked`
- `rustup run 1.88-aarch64-apple-darwin cargo run -p quisum-evals --bin quisum-eval --locked -- --json` — 21/21 offline cases, zero critical failures
- `npm run check --prefix apps/desktop` — TypeScript plus seven frontend tests
- `npm run build --prefix apps/desktop`
- `RUSTUP_TOOLCHAIN=1.88-aarch64-apple-darwin npm run tauri --prefix apps/desktop -- build --no-bundle -- --locked`
- `CI=true RUSTUP_TOOLCHAIN=1.88-aarch64-apple-darwin npm run tauri --prefix apps/desktop -- build --bundles dmg --no-sign -- --locked` — unsigned native macOS ARM64 DMG created; `hdiutil verify` passed
- `node scripts/validate-assets.mjs` — 39 JSON assets, 26 JSONL files, seven method packages, and 21 eval cases
- `node scripts/check-skills.mjs` — eight skill packages, exact canonical-reference sync, byte-identical installs for both hosts, collision rollback, unsafe destination, and injected executable rejection validated; all eight also pass the Codex `quick_validate.py` structural validator
- `node scripts/generate-notices.mjs --check` — 753 Cargo/npm packages represented without unknown license markers
- `bash scripts/check-workflows.sh` — both workflows pass pinned actionlint 1.7.12
- `node scripts/check-release.mjs --tag v1.0.0-rc.1`
- Detached CLI package execution, packaged Claude Code skill installation, two deterministic checksum passes across 86 files, negative tag/output-path tests, `git diff --check`, and targeted panic/TODO/secret/action-reference scans

The local native bundle test covers macOS ARM64 only. Windows, Linux, macOS x86_64, signing/notarization, hosted provenance, installation on clean accounts, deletion validation, live providers, and external reviews remain explicit release evidence gates. The seven official methods remain `unreviewed`.

## Result

No known P0, P1, or P2 finding remains in this stage's scope. The repository is suitable for an unsigned `1.0.0-rc.1` draft release and deliberately cannot satisfy the stable V1.0 gate while required external evidence remains open.
