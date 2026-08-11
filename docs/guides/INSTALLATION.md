# Installation

Quisum `1.0.0-rc.1` is a release candidate. Automated artifacts may be unsigned and are not stable releases. Use only synthetic or disposable data until the open gates in the [V1 readiness matrix](../V1_READINESS.md) are closed.

## Verify a downloaded artifact

Download the application or CLI archive, `SHA256SUMS`, and the SPDX JSON SBOM from the same draft release. A checksum detects corruption; it does not identify the publisher. Compare the artifact hash before opening it:

```sh
shasum -a 256 Quisum-artifact-name
```

On Linux, `sha256sum` provides the equivalent command. On Windows PowerShell:

```powershell
Get-FileHash .\Quisum-artifact-name -Algorithm SHA256
```

For a repository with GitHub artifact attestations enabled, verify provenance with a current [GitHub CLI](https://cli.github.com/manual/gh_attestation_verify):

```sh
gh attestation verify Quisum-artifact-name --repo <owner>/quisum
```

Do not bypass an operating-system signature warning for a stable release. RC release notes must explicitly say when an artifact is unsigned.

## Build from source

The pinned baseline is Rust 1.88, Node.js 22.13 or later in the Node 22 line, and npm with lockfile support. Clone a trusted commit, then run the repository checks before installing anything:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --all-features --locked
node scripts/validate-assets.mjs
node scripts/check-skills.mjs
node scripts/check-release.mjs
npm ci --prefix apps/desktop
npm run check --prefix apps/desktop
```

Tauri also needs its [platform prerequisites](https://v2.tauri.app/start/prerequisites/): Xcode command-line tools on macOS, Microsoft C++ Build Tools and WebView2 on Windows, or WebKitGTK 4.1 and related development packages on Linux. The CI Linux package set is the reference:

```sh
sudo apt-get install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf xdg-utils
```

Build the CLI and desktop shell without changing lockfiles:

```sh
cargo build --release --locked --bin quisum
npm run build --prefix apps/desktop
npm run tauri --prefix apps/desktop -- build --no-bundle -- --locked
```

`--no-bundle` verifies the native desktop executable without producing an installer. For a local unsigned macOS bundle in a noninteractive shell, use `CI=true npm run tauri --prefix apps/desktop -- build --no-sign -- --locked`; Windows PowerShell sets the same variable with `$env:CI = "true"`. These remain development artifacts, not official releases.

The CLI binary is `target/release/quisum` (`quisum.exe` on Windows). A packaged CLI archive uses this layout so the binary can discover the official methods:

```text
bin/quisum
share/quisum/methods/
share/quisum/skills/
share/quisum/scripts/install-skills.mjs
LICENSE
THIRD_PARTY_NOTICES.md
```

When running a binary outside that layout, pass the source checkout explicitly:

```sh
quisum --methods /trusted/quisum/methods method list
```

To install the bundled Codex or Claude Code skills and connect the local stdio MCP server, follow the [host AI guide](HOST_AI.md). Installing skills alone never grants vault access.

## First run

Do not place a real vault in a Git worktree, synchronized folder, or shared directory. The CLI rejects a Git-worktree location by default.

```sh
quisum init
quisum status
quisum doctor
```

Initialization creates an encrypted SQLCipher vault and a separate passphrase-protected recovery envelope in the platform application-data directory. Save the recovery passphrase in a password manager and make an encrypted backup before entering important data. The app cannot recover a forgotten passphrase. Continue with the [recovery guide](RECOVERY.md) and [provider guide](PROVIDERS.md).

## Uninstall

Uninstalling an application bundle does not delete the vault, recovery envelope, provider credentials, exports, or backups. Follow [privacy and deletion](PRIVACY_AND_DELETION.md) first if the intent is to erase data. Never assume an operating-system uninstall removed personal material.
