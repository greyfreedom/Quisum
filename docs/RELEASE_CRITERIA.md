# V1.0 release criteria

A release candidate is not V1.0 until all criteria below are evidenced in CI or release documentation.

## Functional

- The shortest loop—event, candidate, evidence/counterevidence, human review, reflog, export—works through CLI and desktop.
- MCP can perform bounded search and proposal operations and is unable to invoke human-only operations.
- Independent chat works with OpenAI, Anthropic, OpenAI-compatible, and local HTTP profiles, including streaming and connection tests.
- All seven official methods load, validate, and pass their invariant evals.
- Self Map, Review, Timeline, Reflog, Experiments, Privacy & Models, and Explore flows are usable without direct database access.

## Data and recovery

- Schemas and migration policy are stable and documented.
- Fresh creation, upgrade from every supported schema, wrong-key handling, interrupted migration, backup, tamper detection, and restore are tested.
- JSON/JSONL round trips preserve provenance and revisions; Markdown is clearly marked derived.
- Local lexical and semantic retrieval never bypasses deterministic privacy eligibility.

## Security and privacy

- The threat model is current and every required security test passes.
- SQLCipher support is verified in release builds; no plaintext fallback exists.
- Secrets are absent from config, logs, diagnostics, frontend state, and exported support bundles.
- Official binaries default to no telemetry or crash upload.
- An independent review finds no unresolved P0, P1, or P2 defect.

## Safety

- Required eval suites pass across supported provider capability classes.
- Crisis resources are versioned and their limitations are disclosed.
- Official method professional-review status is accurate and traceable.
- No UI or prompt copy implies diagnosis, hidden truth, exclusivity, or guaranteed crisis detection.

## Distribution

- Windows, macOS, and Linux artifacts build in CI and publish checksums and provenance.
- Applicable artifacts are signed; unsigned development artifacts are visibly labeled.
- Installation, vault recovery, provider configuration, privacy, deletion, and migration guides are tested from a clean environment.
- License, security reporting, contribution, code of conduct, changelog, and third-party notices are present.
