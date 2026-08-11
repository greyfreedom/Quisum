# Quisum

> *Quis sum? — Who am I?*
>
> Not a profile. A living inquiry.

Quisum is a local-first, model-agnostic system for continuous self-inquiry. It helps people turn conversations and concrete events into an evidence-backed, user-governed living self model.

Quisum follows one non-negotiable rule:

> Models propose interpretations, the system manages evidence, and the user decides what may represent them.

## Why Quisum

Ordinary AI memory tends to flatten context into claims such as “the user is bad at social situations.” Quisum keeps the event, supporting evidence, counterevidence, applicable context, user review, and every later revision. Unreviewed model output never becomes a confirmed identity claim.

All primary data lives in an encrypted local vault. Cloud model calls receive a purpose-built minimum context packet, never an unrestricted profile dump. Changing AI providers does not discard the user's self model.

## V1.0 scope

- Rust domain core with explicit claim and review lifecycles
- SQLCipher-backed local vault, migrations, encrypted backups, and recovery
- CLI and local stdio MCP server with least-privilege tools
- OpenAI, Anthropic, OpenAI-compatible, and local HTTP provider adapters
- Seven versioned reflection method packages
- Tauri 2 + React reference desktop application
- JSON/JSONL/Markdown import and export
- Safety, privacy-leakage, contradiction, and overclaiming evaluations
- Windows, macOS, and Linux release workflows

Mobile apps, cloud accounts, sync, social features, diagnosis, covert profiling of other people, and synthetic “AI companion” behavior are explicitly out of scope.

## Architecture

```text
methods + policies
        |
        v
   quisum-core <---- providers
        |
        v
   quisum-vault
      / | \
     /  |  \
   CLI MCP Desktop
```

The adapters reuse the same core authorization, lifecycle, retrieval, and safety rules. React never receives raw database access. MCP intentionally excludes confirmation, deletion, full export, and privacy-policy mutation tools.

## Project status

V1.0 implementation is in progress. No real personal data belongs in this source repository; fixtures are synthetic.

Start with [the project charter](docs/PROJECT_CHARTER.md), [architecture](docs/ARCHITECTURE.md), and [threat model](docs/THREAT_MODEL.md). Contributors should read [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## Safety

Quisum is a reflective tool, not medical care, psychotherapy, diagnosis, or an emergency service. In an immediate crisis, contact local emergency services and a trusted person nearby. The runtime stops identity analysis when acute-risk language is detected and does not promote crisis statements into long-term claims.

## License

Licensed under the Apache License, Version 2.0.
