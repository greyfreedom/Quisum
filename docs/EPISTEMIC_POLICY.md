# Epistemic policy

This policy defines what Quisum is allowed to say and persist about a user.

## Knowledge classes

| Class | Origin | Durable without review | Example |
|---|---|---:|---|
| Explicit fact | Direct user statement | Yes, unless sensitive policy blocks it | “I have a two-year-old child.” |
| User self-assessment | Direct user interpretation | Yes, labeled as self-assessment | “I dislike public speaking.” |
| Current state | Time-bounded report | Yes, with expiry/review date | “Work pressure has been high for two weeks.” |
| Event | Concrete occurrence | After user confirmation or explicit capture | “I stayed silent in Tuesday's meeting.” |
| Model hypothesis | Model inference | No | “Evaluation authority may inhibit disagreement.” |
| Confirmed insight | Reviewed interpretation | Yes | User accepts or edits a hypothesis. |
| Formal assessment | Named instrument and version | Yes, separately labeled | A scored public-domain questionnaire. |

The UI and exports must never collapse these classes into one generic profile fact.

## Claim requirements

An interpretive claim contains a bounded statement, applicable contexts, evidence links, counterevidence links, qualitative evidence grade, sensitivity, provenance, status, timestamps, and optional predecessor. It may also record alternative explanations and a revisit date.

Numeric confidence such as “62%” is prohibited unless it is a result produced by a documented formal instrument. Runtime display uses qualitative terms: limited evidence, moderate evidence, repeated observations, contested, user partially endorses, and needs observation.

## Evidence grades

- `limited`: one observation or indirect material.
- `moderate`: more than one relevant observation with no decisive conflict.
- `repeated`: recurring evidence across time or contexts.
- `contested`: material counterevidence or plausible alternatives remain.

A grade is not statistical probability. It describes the evidence set and can decrease.

## Lifecycle

```text
proposed -> pending_review -> confirmed
                           -> revised
                           -> uncertain
                           -> rejected

confirmed | revised -> contradicted | superseded | deprecated | deleted
```

Only a human-authorized surface may create the transition to `confirmed`, `revised`, `rejected`, `deprecated`, or `deleted`. Model-facing tools may only propose and add evidence or counterevidence.

Every transition appends a revision and audit event. Mutation of the latest projection must not erase earlier content.

## Evidence and context rules

- A personality-level hypothesis includes at least one alternative explanation and asks for an exception context.
- A claim must not infer a stable trait solely from a recent state or a single event.
- Counterevidence is returned beside supporting evidence, not hidden in a secondary view.
- Crisis-related utterances are ineligible as identity evidence by default.
- Formal assessment results name the instrument, version, source, scoring method, and date; conversation inference cannot impersonate an assessment.

## Corrections

A user correction has higher retrieval priority than any contradictory AI inference. Corrections are never silently summarized away. A correction can suppress a phrase, interpretation, topic, or source claim from future model context.

## Deletion consistency

Deleting an event triggers a dependency check. Claims that lose evidence are regraded and may return to review as `contradicted` or be marked unsupported. A deleted source cannot remain as invisible support for a durable conclusion. Audit entries retain non-sensitive structural metadata needed to prove that a deletion occurred, not the deleted content itself.

## Provenance

Generated candidates record method package/version, provider profile, model identifier, creation time, and input context-packet identifier. Imported data records format version and import event. User-authored entries identify the author as `user`, never a model.
