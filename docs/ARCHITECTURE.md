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

`quisum-providers` implements a provider-neutral streaming and structured-generation trait. It cannot decide which vault material may be sent; it accepts an already-authorized context packet.

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

Provider failure or invalid structured output ends without persisting a candidate. The product never “repairs” arbitrary invalid JSON into a durable identity claim without validation.

## Storage model

Event history and audit records are append-only. Current rows are projections optimized for display and retrieval. A projection update and its revision/audit record occur in one transaction. Foreign keys and explicit dependency checks maintain evidence integrity.

SQLite FTS supports exact and lexical retrieval. Local embeddings add semantic candidates at V1.0; the final eligibility and disclosure filters remain deterministic and run after ranking.

## Authorization

Core commands receive an `Actor` with a capability set:

- `User`: review and administrative capabilities after local authorization.
- `ModelHost`: bounded read and proposal capabilities.
- `System`: migrations and internal maintenance only.

Capability checks happen before repository access. Sensitivity checks happen both while retrieving and immediately before provider dispatch.

## Desktop boundary

Tauri 2 is an adapter. React invokes named domain commands and receives redacted DTOs. It has no SQL plugin, filesystem shell, arbitrary HTTP, secret-store, or vault-key access. A content security policy permits packaged application resources only, with narrowly declared provider connectivity performed from Rust.

## Interoperability

Schemas are versioned using semantic versions and stable string enums. JSONL is the lossless portable event format; JSON packages include metadata and checksums; `WHOAMI.md` is a human-readable derived export and never the sole backup format.
