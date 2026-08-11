# Project charter

## Mission

Quisum gives an adult user a portable, evidence-backed way to explore themselves with any compatible AI while retaining local custody and final interpretive authority.

It is not a profile generator. It supports an ongoing loop:

```text
observe -> hypothesize -> seek evidence and counterevidence -> user review -> experiment -> revise
```

## Product promise

- Models may propose explanations; only a human user may confirm them.
- Every durable interpretation keeps its source, context, evidence, counterevidence, and revision history.
- Private material remains local by default; provider disclosure is purpose-limited and auditable.
- Switching a model or interface never changes ownership of the self model.
- The product supports autonomy and real-world relationships rather than attachment to an assistant.

## Initial users

V1.0 serves adults who are interested in reflection or journaling, understand local files and API keys, can install open-source software, and value data ownership. Consumer onboarding, accounts, mobile clients, and synchronization are later concerns.

## Core capabilities

1. Capture sessions, concrete events, open questions, and experiments.
2. Separate observed facts, user self-assessment, current state, contextual reaction, hypotheses, confirmed insight, and formal assessment.
3. Generate candidate claims with support, counterevidence, alternatives, scope, and provenance.
4. Let the user confirm, revise, defer, reject, deprecate, or delete.
5. Build a self map and reflog without hiding contradiction or change.
6. Retrieve only relevant, eligible memory and show an auditable context packet.
7. Work through a local CLI/MCP host or independent provider-backed client.
8. Export and restore a portable, documented representation.

## Explicit exclusions

Quisum does not diagnose illness, replace professional care, infer hidden childhood events, claim access to the unconscious, profile another person without consent, make employment/credit/insurance decisions, create a digital personality clone, operate a social network, or use engagement pressure.

## Governance commitments

Changes that weaken human confirmation, provenance, deletion consistency, least disclosure, or crisis handling require an explicit architecture decision and a security review. Method claims should be reviewed by relevant professionals before they are presented as clinically informed. Community methods remain untrusted until installed and approved by the user.

## V1.0 definition

V1.0 requires stable schemas and migrations; encrypted local storage and backups; functioning CLI, MCP, providers, and desktop adapters; seven versioned method packages; eight installable Codex/Claude Code Agent Skills; local semantic retrieval; Windows/macOS/Linux release automation; provider compatibility tests; and complete user, privacy, safety, threat-model, recovery, host-AI, and contribution documentation.
