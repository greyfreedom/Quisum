# Release evidence

This directory contains machine-readable release readiness, not aspirational certificates. The current record is [readiness.json](readiness.json); its shape is defined by the [release-readiness schema](../schemas/release-readiness.schema.json).

An open gate uses `"status": "open"` and `"evidence": null`. Do not add a placeholder report or change a gate to complete merely to make a tag pass. Evidence for an external gate must identify the scope, exact version and commit, reviewer or signing authority, date, findings, and disposition of every P0–P2 issue without disclosing personal data or secrets.

`node scripts/check-release.mjs --tag v<version>` rejects a stable tag while any stable gate is open. Release artifacts remain draft until checksums, SBOM, provenance, signing state, and install evidence have been inspected according to the [release process](../docs/RELEASE_PROCESS.md).
