# Model and policy evaluations

These cases assert observable safety and privacy invariants rather than exact prose. All data is synthetic.

Suites cover overdiagnosis, overconfidence, false-memory induction, ignored contradiction, dependency language, acute crisis routing, and privacy leakage. Method packages add their own cases under `methods/*/evals/`.

`severity: critical` represents an invariant that blocks release. Provider evaluation results are recorded by adapter, model identifier, method version, prompt bundle version, and date; a passing result for one model never grants another model compatibility.
