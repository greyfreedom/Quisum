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
```

`quisum-core` contains domain types, authorization, claim state transitions, safety screening, retrieval eligibility, context-packet construction, and import/export contracts. It has no dependency on Tauri, MCP transport, provider HTTP protocols, or SQLite.

`quisum-vault` implements Core repository traits with SQLCipher, ordered migrations, transactional projections, audit append, backup, and restore.

`quisum-providers` implements a provider-neutral streaming and structured-generation trait for OpenAI Responses, Anthropic Messages, OpenAI-compatible Chat Completions, and loopback HTTP. Redirects and ambient proxies are disabled, request/response sizes and deadlines are bounded, and cloud profiles require HTTPS. It cannot decide which vault material may be sent; it accepts an already-approved and dispatch-audited context packet.

`quisum-cli`, `quisum-mcp`, and the Tauri adapter are delivery surfaces. All call the same Core application service. No adapter reimplements lifecycle or privacy rules.

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

Tauri 2 is an adapter. React invokes named domain commands and receives redacted DTOs. It has no SQL plugin, filesystem shell, arbitrary HTTP, secret-store, or vault-key access. A content security policy permits packaged application resources only, with narrowly declared provider connectivity performed from Rust.

## Interoperability

Schemas are versioned using semantic versions and stable string enums. JSON and JSONL preserve domain records, provenance, reviews, and revisions and include a checksum for accidental-corruption detection. They are plaintext, explicitly authorized imports—not authenticated backups. Import revalidates the complete record graph, disables imported provider profiles, and replaces credential references so an artifact cannot inherit an existing OS secret. `WHOAMI.md` is a human-readable derived export and never a backup. Encrypted `.qsb` artifacts are the recovery format.
