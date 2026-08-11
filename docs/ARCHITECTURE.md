# Architecture

## Dependency direction

```text
methods/schemas
      |
      v
quisum-core <------ quisum-providers
      |
      v
quisum-vault
   ^    ^    ^
   |    |    |
 CLI   MCP  Tauri commands <-> React UI
        ^
        |
 cross-host Agent Skills <-> Codex / Claude Code
```

`quisum-core` contains domain types, authorization, claim state transitions, safety screening, retrieval eligibility, context-packet construction, and import/export contracts. It has no dependency on Tauri, MCP transport, provider HTTP protocols, or SQLite.

The versioned crisis resource pack is embedded into Core so CLI and desktop use the same deterministic route, provenance, freshness cutoff, and no-number fallback without a network request. Locale-based region selection is advisory and never enters a provider context packet.

`quisum-vault` implements Core repository traits with SQLCipher, ordered migrations, transactional projections, audit append, backup, and restore.

`quisum-providers` implements a provider-neutral streaming and structured-generation trait for OpenAI Responses, Anthropic Messages, OpenAI-compatible Chat Completions, and loopback HTTP. Redirects and ambient proxies are disabled, request/response sizes and deadlines are bounded, and cloud profiles require HTTPS. It cannot decide which vault material may be sent; it accepts an already-approved and dispatch-audited context packet.

`quisum-cli`, `quisum-mcp`, and the Tauri adapter are delivery surfaces. All call the same Core and Vault policy boundaries. Adapters may validate presentation-specific input, but durable lifecycle, safety, provenance, and privacy invariants are enforced again below the adapter boundary.

The eight installable Agent Skills are host adapters, not policy enforcement. Seven generated references mirror the canonical method manifests, facilitator instructions, safety rules, memory policies, and output schemas; the curation reference mirrors every method's memory policy. CI rejects stale snapshots. Skills may use only the exposed MCP reads and pending-proposal writes, and they fall back to an explicitly non-persistent conversation when MCP or valid provenance is unavailable.

## Core flow

```text
input
  -> deterministic safety pre-check
  -> eligible-memory query
  -> minimum context packet + optional user preview
  -> provider generation
  -> schema validation
  -> safety/overclaim post-check
  -> pending candidate append
  -> human review
  -> append revision + update projection
```

Provider failure, truncated terminal state, unsafe generated language, or invalid structured output ends without persisting a candidate. Streaming transport is consumed incrementally, but text deltas are released to product surfaces only after the complete response passes terminal-state, size, safety, and optional JSON Schema validation. The product never “repairs” arbitrary invalid JSON into a durable identity claim.

`quisum-eval` is a separate executable adapter over Core and declarative cases. Offline mode is a deterministic CI gate. Provider-result mode accepts one bounded strict JSONL run and binds its report to exact provider and prompt metadata; neither mode can mutate a vault.

## Storage model

Event history and audit records are append-only. Current rows are projections optimized for display and retrieval. A projection update and its revision/audit record occur in one transaction. Foreign keys and explicit dependency checks maintain evidence integrity.

SQLite FTS supports exact and lexical retrieval. A deterministic, on-device hashed feature vector adds similarity candidates without downloading a model or sending text elsewhere. Similarity scans are bounded to recent projections; exact FTS hits are merged by ID so older lexical matches are not lost. Final lifecycle, crisis, sensitivity, relevance, provider-trust, and approval checks remain deterministic and run after ranking.

## Authorization

Core commands receive an `Actor` with a capability set:

- `User`: review and administrative capabilities after local authorization.
- `ModelHost`: bounded read and proposal capabilities.
- `System`: migrations and internal maintenance only.

Capability checks happen before repository access. Sensitivity checks happen both while retrieving and immediately before provider dispatch.

The stdio MCP adapter exposes exactly ten bounded operations: search, four exact reads/context operations, four proposal operations, and counterevidence proposal. It has no review, confirmation, deletion, export, backup, restore, unlock, credential, encryption, or privacy-policy operation. Vault unlock occurs before the MCP transport starts and requires the OS credential store because stdin is reserved for protocol frames.

## Desktop boundary

Tauri 2 is an adapter. React invokes only the named commands registered by the Rust handler and receives redacted DTOs. The sole local window has an empty Tauri core/plugin permission set: no SQL, filesystem, shell, arbitrary HTTP, window-management, secret-store, updater, or vault-key API is exposed to the WebView. Production CSP permits packaged resources and Tauri IPC only. Provider connectivity, artifact I/O, and keyring access occur in Rust, and secret-bearing DOM controls are cleared before their command promises settle.

## Interoperability

Schemas are versioned using semantic versions and stable string enums. JSON and JSONL preserve domain records, provenance, reviews, and revisions and include a checksum for accidental-corruption detection. They are plaintext, explicitly authorized imports—not authenticated backups. Import revalidates the complete record graph, disables imported provider profiles, and replaces credential references so an artifact cannot inherit an existing OS secret. `WHOAMI.md` is a human-readable derived export and never a backup. Encrypted `.qsb` artifacts are the recovery format.
