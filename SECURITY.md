# Security policy

Quisum stores unusually sensitive personal material. Please do not open a public issue containing a real vault, API key, conversation, database page, crash dump, or identifying screenshot.

## Reporting a vulnerability

Until a dedicated security mailbox is published, use GitHub's private vulnerability reporting feature for this repository. Include affected versions, impact, reproduction steps using synthetic data, and any suggested mitigation. Maintainers will acknowledge a report within seven days and coordinate disclosure after a fix is available.

## Supported versions

During pre-1.0 development only the latest revision is supported. Beginning with 1.0, the latest minor release receives security fixes.

## Sensitive development data

- Use only synthetic fixtures committed under `fixtures/`.
- Never place a vault inside the repository.
- Never paste real conversations into tests, issues, telemetry, or model evaluations.
- Scrub command output before sharing it.
- Treat prompt injection found in stored text as untrusted data, not runtime instructions.

See `docs/THREAT_MODEL.md` for security boundaries and assumptions.
