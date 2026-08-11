# Stage 1 review — specification and engineering baseline

Date: 2026-08-11

## Scope

- Project charter, architecture, product, epistemic, safety, privacy, threat, authoring, roadmap, and release policies
- Versioned public JSON Schemas
- Seven official method-package skeletons and synthetic examples
- Safety/privacy evaluation corpus
- Synthetic vault fixture
- Rust workspace and schema validation tests
- Repository contribution, security, license, and formatting baseline

## Review method

The review traced each final-plan invariant into documentation, schemas, examples, or eval cases, then checked the staged change for malformed assets, broken references, secret-like values, unsafe placeholders, formatting defects, and Rust lint failures. Schema tests compile every schema against the Draft 2020-12 meta-schema and validate all public fixtures and method examples using a local reference registry.

## Findings resolved

| Severity | Finding | Resolution |
|---|---|---|
| P1 | An outbound context-packet item could declare `local_only` sensitivity. | The outbound schema now excludes `local_only`; it can only appear in the exclusions audit. A negative schema test protects the rule. |
| P1 | A model output routed to crisis handling could still carry a summary or durable candidates. | The method-output schema now requires null summary, empty candidate arrays, and `allow_persistence: false` for the crisis route. |
| P2 | The first approval rule rejected an unapproved high-sensitivity packet before the UI could preview it. | Approval is now required only for dispatch; an undispatched packet remains valid for preview. |
| P2 | Alternative explanations were enforced only for pending pattern hypotheses, not the full hypothesis lifecycle. | Every `pattern_hypothesis` now requires at least one alternative explanation. |
| P2 | Two synthetic fixture IDs were shorter than the canonical entity-ID format. | IDs were corrected and the asset validator now checks fixture references recursively. |
| P2 | Stable current-state claims could omit a revisit time, and crisis eligibility was implicit on several durable types. | Current states require `revisit_after`; claims, questions, and experiments carry explicit crisis flags. |
| P3 | Generated text assets contained an extra blank line at EOF. | The files were normalized and `git diff --check` is part of the gate. |

## Verification

```text
node scripts/validate-assets.mjs
  25 JSON schemas/assets
  26 JSONL files
  7 method packages
  21 eval cases

cargo test --workspace --all-features
  6 schema/fixture boundary tests passed
  all workspace and doc tests passed

cargo clippy --workspace --all-targets --all-features -- -D warnings
  passed

cargo fmt --all --check
  passed

git diff --cached --check
  passed
```

Secret-pattern scanning found no credential or private-key material. Rust/JavaScript placeholder scanning found no `TODO`, `FIXME`, `HACK`, `unwrap`, or `expect` in executable sources.

## Result

No open P0, P1, or P2 issue remains in the Stage 1 scope. “Professional review” is intentionally not claimed: method manifests remain marked `unreviewed` until a qualified external review is actually completed.
