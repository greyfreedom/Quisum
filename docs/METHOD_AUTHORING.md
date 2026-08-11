# Method authoring

A method package defines reflective content while Rust Core owns state transitions, authorization, memory eligibility, and safety invariants. Method packages are declarative and do not execute arbitrary code.

## Required layout

```text
methods/<slug>/
├── method.yaml
├── facilitator.md
├── output.schema.json
├── memory-policy.yaml
├── safety.md
├── examples.jsonl
└── evals/cases.jsonl
```

## Manifest

`method.yaml` declares a stable ID, semantic version, title, description, supported purposes, required provider capabilities, output schema, memory policy, safety policy, review status, and license. IDs and versions become claim provenance and cannot be reused for materially different behavior.

## Facilitator guidance

Use open questions, reflective listening, plain uncertainty, and summaries in the user's language. Start from a concrete event when possible. Do not front-load a label. Ask one useful question at a time and leave room for “I don't know.”

A personality-level candidate must include an alternative explanation and invite an exception. Methods must not make clinical claims or imply professional care.

## Outputs

Outputs are bounded structured candidates, not prose hidden in a transcript. Schemas should reject unknown fields, oversized strings, unsupported statuses, and unclassified sensitivity. Only proposal statuses are accepted from model-facing methods.

## Memory policy

The policy names fields that may become event candidates, claim candidates, session summaries, or ephemeral working notes. It states retention, crisis exclusions, review requirements, sensitivity defaults, and retrieval eligibility. Silence means do not persist.

## Safety policy

Each method defines conditions for pausing, changing to the crisis route, refusing third-party profiling, and avoiding overclaiming. It may strengthen but never weaken the global safety policy.

## Examples and evals

All examples are synthetic. Include a normal case, ambiguity, counterevidence, user correction, an irrelevant sensitive memory, and a safety boundary. Eval records use the repository schema and assert observable invariants rather than exact prose.

## Review and release

Run `node scripts/validate-assets.mjs`. Changes require a version bump, changelog note, relevant model evals, and review appropriate to any claimed evidence basis. Community packages are visibly marked unreviewed until the required review is complete.
