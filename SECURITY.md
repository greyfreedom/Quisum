# Security policy

Quisum stores unusually sensitive personal material. Please do not open a public issue containing a real vault, API key, conversation, database page, crash dump, or identifying screenshot.

## Reporting a vulnerability

Until a dedicated security mailbox is published, use GitHub's private vulnerability reporting feature for this repository. If that feature is unavailable, use the repository owner's private profile contact and include only the minimum detail needed to establish a secure follow-up channel. Do not fall back to a public issue with sensitive evidence. Include affected versions, impact, reproduction steps using synthetic data, and any suggested mitigation. Maintainers will acknowledge a report within seven days and coordinate disclosure after a fix is available.

## Supported versions

Only the latest release candidate is supported until stable V1.0 is published. After V1.0, the latest minor release on the current major line receives security fixes. Source snapshots and unsigned local builds are not supported release artifacts.

## Sensitive development data

- Use only synthetic fixtures committed under `fixtures/`.
- Never place a vault inside the repository.
- Never paste real conversations into tests, issues, telemetry, or model evaluations.
- Scrub command output before sharing it.
- Treat prompt injection found in stored text as untrusted data, not runtime instructions.

See `docs/THREAT_MODEL.md` for security boundaries and assumptions.

## Release trust

Release-candidate artifacts are experimental and may be unsigned. Stable tags are blocked by repository checks until the external-review and signing gates recorded in `release/readiness.json` have evidence. Verify release checksums and GitHub artifact attestations as described in `docs/guides/INSTALLATION.md`; a checksum detects corruption but is not an identity signature.
