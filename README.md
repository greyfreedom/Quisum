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
- Eight installable Codex and Claude Code Agent Skills backed by those method packages
- Tauri 2 + React reference desktop application
- JSON/JSONL import and export, plus derived Markdown export
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

The adapters reuse the same core authorization, lifecycle, retrieval, and safety rules. Agent Skills are cross-host facilitation adapters whose generated references stay synchronized with the canonical methods; they never receive direct vault access. React never receives raw database access. MCP intentionally excludes confirmation, deletion, full export, and privacy-policy mutation tools.

## Project status

The complete V1.0 feature set is available as **`1.0.0-rc.1`**. It is a release candidate, not a stable V1.0 release: independent security review, qualified professional review of the bundled methods, live provider compatibility evidence, clean-install validation, and platform signing are still open gates. Official methods therefore remain visibly marked `unreviewed`, and development artifacts are unsigned.

No real personal data belongs in this source repository; fixtures are synthetic. See the machine-readable [release readiness record](release/readiness.json) and the human-readable [V1 readiness matrix](docs/V1_READINESS.md) for what has and has not been established.

## Start here

- [Install or build Quisum](docs/guides/INSTALLATION.md)
- [Create and recover a vault](docs/guides/RECOVERY.md)
- [Configure a model provider](docs/guides/PROVIDERS.md)
- [Install Agent Skills and connect Codex or Claude Code](docs/guides/HOST_AI.md)
- [Understand privacy, export, and deletion](docs/guides/PRIVACY_AND_DELETION.md)

Project foundations are in the [charter](docs/PROJECT_CHARTER.md), [architecture](docs/ARCHITECTURE.md), and [threat model](docs/THREAT_MODEL.md). Contributors should read [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and the [Code of Conduct](CODE_OF_CONDUCT.md).

## Safety

Quisum is a reflective tool, not medical care, psychotherapy, diagnosis, or an emergency service. If anyone may be in immediate danger, contact local emergency services now or go to the nearest emergency department. The runtime stops identity analysis when acute-risk language is detected and does not promote crisis statements into long-term claims. Detection is conservative and cannot guarantee that it will recognize every crisis.

## License

Licensed under the Apache License, Version 2.0.
