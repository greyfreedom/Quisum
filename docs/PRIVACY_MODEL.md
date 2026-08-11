# Privacy model

## Defaults

Quisum has no required account, remote service, telemetry, crash upload, or background synchronization. The encrypted vault is the system of record. API credentials are held by the OS credential facility and referenced only by opaque names in the vault; they are never accepted as command-line values, serialized into portable exports, or included in ordinary diagnostics.

## Sensitivity

Every stored item has one of four levels:

- `normal`: eligible for relevant retrieval.
- `private`: eligible only when relevant to the current purpose.
- `highly_sensitive`: requires an explicit policy grant and is shown in context preview.
- `local_only`: never leaves the local process through a provider adapter.

V1 enforcement is based on item sensitivity, lifecycle eligibility, relevance, provider-profile trust, and one-packet approval. Topic labels improve local retrieval but do not silently grant disclosure authority.

## Context packets

Every outbound model request references an immutable context packet containing:

- declared purpose and active method;
- provider profile and model identifier;
- selected item IDs, excerpts, sensitivity, and reason for selection;
- items intentionally excluded and the policy reason;
- approval status when highly sensitive content is present;
- creation and dispatch timestamps.

Selection is deny-by-default. `local_only`, rejected, deleted, crisis-related, and unrelated highly sensitive entries are ineligible. User corrections are considered before candidate claims.

## Host AI and Agent Skills

Installing an Agent Skill copies inspectable instructions and generated method references; it does not grant vault, filesystem, shell, database, provider-secret, or human-review authority. A Codex or Claude Code conversation is still handled under that host's own privacy terms before Quisum is involved. The user must separately register the local stdio MCP server and explicitly ask to retrieve memory or stage a proposal.

The host may receive only the eligible excerpts in an audited minimum context packet. Invoking a skill does not consent to storing the conversation: every model write requires a separate explicit request and remains pending human review. If the host, skill, or MCP setup cannot establish the exact method, provider profile name/ID, model, and dispatched context-packet provenance, nothing is persisted. A modified installed skill remains untrusted; Core authorization is the enforcement boundary.

## Provider metadata

Provider profiles state cloud or local operation, base URL, supported capabilities, retention notes supplied by the user or adapter, and whether the endpoint is trusted for private or highly sensitive content. Quisum does not guarantee a provider's behavior; it makes the disclosure visible and enforceable locally.

Cloud endpoints require HTTPS; local endpoints must be syntactic loopback hosts. Provider HTTP clients reject redirects, ignore ambient proxy configuration, and use bounded connect/request deadlines. A profile's endpoint, protocol, model, name, and credential reference are immutable provenance identity; changed identities are represented by new profiles. Disabling a profile is reversible. Revoking a credential disables the profile before deleting its OS secret.

## Logs and diagnostics

Application logs use stable IDs, operation names, timings, and redacted error categories. They do not contain message bodies, claim statements, keys, authorization headers, database keys, or exported data. Diagnostics and crash reports are local and opt-in for sharing.

## Export, backup, and deletion

- Plaintext exports require an explicit user action and a destination warning.
- Backups are encrypted by default and verified before being declared usable.
- Portable imports validate checksums and cross-record provenance, then disable imported provider profiles and replace any credential reference. Provider credentials never transfer with a portable artifact.
- Full deletion is a deliberate platform operation in the V1 RC: unlock first, revoke stored provider credentials, close Quisum, then remove the exact vault, recovery, snapshot, backup, export, and OS-key entries chosen by the user. The app never claims that uninstalling or deleting one file erases copies held by providers, snapshots, or other devices.
- Derived claims are re-evaluated when sources are removed.
- SQLCipher protects data at rest while locked; it does not protect an unlocked process, screenshots, swap configured by the OS, compromised endpoints, or a malicious provider receiving approved context.

Portable JSON/JSONL checksums are not digital signatures and do not prove who created an artifact. Import is an explicit trust decision backed by strict schema, size, lifecycle, and graph validation. Use encrypted `.qsb` backup/restore for recovery rather than portable export.

The exact user procedure and storage limitations are documented in [privacy and deletion](guides/PRIVACY_AND_DELETION.md); host-AI boundaries are documented in the [Codex and Claude Code setup guide](guides/HOST_AI.md).

## Repository safety

Initialization detects Git worktrees and refuses to place a real vault within one unless the user gives an explicit override after a prominent warning. Source fixtures remain synthetic.
