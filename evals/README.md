# Model and policy evaluations

These cases assert observable safety and privacy invariants rather than exact prose. All data is synthetic.

Suites cover overdiagnosis, overconfidence, false-memory induction, ignored contradiction, dependency language, acute crisis routing, and privacy leakage. Method packages add their own cases under `methods/*/evals/`.

`severity: critical` represents an invariant that blocks release. Provider evaluation results are recorded by adapter, model identifier, method version, prompt bundle version, and date; a passing result for one model never grants another model compatibility.

## Executable gates

Run the deterministic offline gate from the repository root:

```bash
cargo run -p quisum-evals --bin quisum-eval --
cargo run -p quisum-evals --bin quisum-eval -- --json
```

The offline gate executes all 21 cases, loads every official method package, invokes Core crisis routing, applies ordinary memory eligibility, and runs the same strict assertion engine used for provider results. It is a policy and harness test—not evidence that any model passed.

To evaluate captured results from one exact provider run:

```bash
cargo run -p quisum-evals --bin quisum-eval -- \
  --results /path/to/synthetic-provider-results.jsonl \
  --json
```

Every line must match [the provider eval-result schema](../schemas/eval-result.schema.json). A file is rejected unless it contains exactly one result for every repository case and every record names the same adapter, model identifier, capability class, prompt bundle version, and evaluation timestamp. Missing targets and wrong target types fail negative assertions instead of passing vacuously.

Provider-result files must contain synthetic inputs only. They are evidence for exactly the named run; they are not committed as a general compatibility claim and cannot be substituted by the offline reference policy.
