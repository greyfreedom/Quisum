# Changelog

All notable changes are documented here. Versions follow Semantic Versioning; vault schema and method-package versions have independent compatibility rules.

## [Unreleased]

No changes yet.

## [1.0.0-rc.1] — 2026-08-12

### Added

- Evidence-governed event, claim, review, correction, open-question, experiment, context, session, and audit domain lifecycles.
- SQLCipher local vault with OS-held random keys, bounded Argon2 recovery envelopes, transactional migrations, encrypted pre-migration snapshots, authenticated backups, and verified restore.
- Strict JSON/JSONL portability, derived Markdown export, cross-record import validation, deletion repair, hybrid lexical/semantic retrieval, and content-free reflog.
- Least-privilege CLI and stdio MCP surfaces with human-only confirmation, deletion, privacy, and export authority kept out of MCP.
- OpenAI Responses, Anthropic Messages, OpenAI-compatible Chat Completions, and loopback-only local HTTP adapters with bounded streaming and local output validation.
- Seven versioned declarative reflection methods, all accurately labeled `unreviewed`.
- Eight cross-host Codex and Claude Code Agent Skills with canonical method snapshots, proposal-only MCP guidance, deterministic validation, and a no-overwrite installer.
- Tauri 2 and React desktop reference client for Explore, Timeline, Self Map, Review, Reflog, Experiments, and Privacy & Models.
- Conservative crisis routing with versioned regional sources, stale-source suppression, third-party profiling refusal, and no crisis persistence/provider dispatch.
- Executable 21-case offline safety gate and strict imported provider-result assertions.
- Pinned cross-platform CI and draft-release workflows with CLI packaging, checksums, SPDX SBOM generation, and GitHub provenance.
- Installation, host-AI, recovery, provider, privacy/deletion, migration, compatibility, release, security, and readiness documentation.

### Security and privacy

- Redirects and ambient proxies are disabled for provider requests; cloud endpoints require HTTPS and local endpoints require loopback hosts.
- Provider credentials remain outside the vault in the OS credential store and are never accepted as command-line values.
- Complete user payloads are screened at adapter and authoritative Vault boundaries; dispatch revalidates every reviewed context item and provider policy atomically.
- Application defaults include no account, sync, telemetry, analytics, updater, or crash upload.

### Known release gates

- Independent security review is not complete.
- Qualified professional review of official methods is not complete.
- A complete live-provider compatibility/evaluation matrix is not complete.
- Cross-platform clean-install/deletion validation and applicable platform signing are not complete.

Because these gates remain open, this version is an unsigned release candidate and must not be presented as stable V1.0.
