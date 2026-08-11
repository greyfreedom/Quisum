# V1 readiness

Version `1.0.0-rc.1` contains the intended V1.0 feature set. It is not represented as stable V1.0 because several release criteria require people, accounts, signing identities, hosted runs, or live services that this repository cannot manufacture. Machine-readable state is in [release/readiness.json](../release/readiness.json).

Status meanings:

- **Verified locally**: implementation plus deterministic tests or maintainer review exist in this commit.
- **Automated**: a pinned CI/release workflow exists; a successful hosted run must still be attached to a release.
- **Open**: required stable evidence does not exist.

## Functional and data gates

| Criterion | Status | Evidence or remaining work |
| --- | --- | --- |
| CLI and desktop shortest loop | Verified locally | [Stage 3](reviews/STAGE_3.md) and [Stage 4](reviews/STAGE_4.md) reviews |
| Least-privilege MCP | Verified locally | MCP permission/schema tests and [architecture](ARCHITECTURE.md) |
| Four provider protocol kinds | Verified locally | Synthetic protocol tests and [compatibility matrix](PROVIDER_COMPATIBILITY.md) |
| Seven official method packages | Verified locally | Loader/schema/eval tests; review labels remain unreviewed |
| Eight Codex/Claude Code Agent Skills | Verified locally | Canonical-reference sync, structural validation, and no-overwrite installer tests for both hosts |
| Desktop V1 surfaces | Verified locally | Stage 4 browser/build review |
| Schema/migration integrity | Verified locally | Every-prefix, rollback, tamper, snapshot, and future-schema tests; [migration guide](guides/MIGRATIONS.md) |
| Encrypted backup/restore | Verified locally | Wrong-passphrase/tamper/partial-file/restore tests and [recovery guide](guides/RECOVERY.md) |
| Portable round trips | Verified locally | JSON/JSONL graph tests; Markdown is explicitly derived |
| Privacy-eligible retrieval | Verified locally | Hybrid lexical/semantic and dispatch revalidation tests |

## Security, privacy, and safety gates

| Criterion | Status | Evidence or remaining work |
| --- | --- | --- |
| SQLCipher with no plaintext fallback | Verified locally | Cipher marker/integrity tests and [threat model](THREAT_MODEL.md) |
| Secret/log/frontend isolation | Verified locally | Redaction tests, Tauri CSP/capabilities, no secret CLI arguments |
| No telemetry/crash upload | Verified locally | No plugin or network surface; [privacy model](PRIVACY_MODEL.md) |
| Executable repository safety cases | Verified locally | 21-case offline gate and strict provider-result validator; [Stage 5](reviews/STAGE_5.md) |
| Versioned crisis resources | Verified locally | Public-source check dated 2026-08-12; no professional crisis review claimed |
| Internal P0–P2 disposition | Verified locally | Stage reviews 1–6; maintainer self-review only |
| Independent security review | **Open** | No external report exists |
| Qualified professional method review | **Open** | All official manifests accurately say `unreviewed` |
| Complete live-provider eval matrix | **Open** | Deterministic fixtures pass; no accepted live run |

## Distribution gates

| Criterion | Status | Evidence or remaining work |
| --- | --- | --- |
| Windows/macOS/Linux CI definitions | Automated | [CI workflow](../.github/workflows/ci.yml); hosted success not yet linked |
| Draft native release pipeline | Automated | [Release workflow](../.github/workflows/release.yml) |
| Checksums, SPDX SBOM, provenance | Automated | Generated during the draft workflow; per-artifact evidence pending its first hosted run |
| Install/host-AI/recovery/provider/privacy/migration guides | Verified locally | [Guides](guides/INSTALLATION.md), [host setup](guides/HOST_AI.md), and link validation |
| License/security/contribution/conduct/changelog/notices | Verified locally | Required repository documents and generated lockfile inventory |
| Clean-install and deletion validation | **Open** | Must be performed on every supported platform outside a developer environment |
| macOS signing and notarization | **Open** | No identity or signed artifact evidence configured |
| Windows code signing | **Open** | No Authenticode identity or signed artifact evidence configured |

## Promotion decision

The repository is ready to produce an **unsigned V1.0 release candidate** for external validation. It is not ready to publish stable V1.0. The stable tag check fails while any open gate above remains open; this is expected and intentional.
