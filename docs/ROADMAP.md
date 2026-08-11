# Roadmap

The implementation milestones below are complete in `1.0.0-rc.1`. Stable V1.0 promotion is intentionally separate: external security/professional review, live-provider evidence, clean-install validation, and applicable signing remain open in the [readiness matrix](V1_READINESS.md).

## Milestone 0 — specification and vertical prototype

Freeze the charter, epistemic/safety/privacy policies, threat model, schemas, method format, synthetic fixtures, and first safety evals. Demonstrate the event-to-reviewed-claim-to-export loop.

## V0.1 — host-AI MVP

Deliver Rust Core, encrypted Vault, CLI, local stdio MCP, Codex and Claude Code skills, free reflection, event debrief, values clarification, pattern validation, review, Self Map, Reflog, import/export, and safety evals. Quisum does not call model APIs in this milestone.

## V0.2 — independent CLI

Add provider profiles, OpenAI, Anthropic, OpenAI-compatible, and local HTTP adapters; chat streaming; context preview; decision reflection; behavior experiments; and periodic review.

## V0.3 — desktop reference client

Add Tauri 2 + React Explore, Self Map, Review, Timeline, Reflog, Experiments, Privacy & Models, import/export, provider settings, context preview, and encrypted backups.

## V1.0 — stable open-source product

The code, schemas, migrations, backup/restore, local semantic retrieval, method authoring, contribution process, provider adapters, desktop reference client, executable safety gate, and cross-platform release automation are implemented in the RC.

Stable publication still requires cross-platform signed artifacts, accepted hosted release evidence, qualified professional review of every official method, independent security review, live-provider compatibility runs, and clean-install/deletion validation. Track these gates in [V1 readiness](V1_READINESS.md) rather than inferring completion from the version string.

## After V1.0

Potential work includes accessible consumer onboarding, additional locally evaluated provider protocols, opt-in attachment support, mobile clients, and user-controlled synchronization. Accounts, social features, covert profiling, diagnosis, engagement pressure, and an imitation “AI companion” remain outside the product promise unless governance documents change through explicit review.
