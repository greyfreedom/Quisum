# Stage 5 internal review — executable safety evaluations

Date: 2026-08-12

Scope: versioned crisis resources, Core resource selection, CLI/desktop crisis presentation, complete-payload adapter prechecks, the offline policy gate, and strict imported provider-result assertions.

This is a maintainer self-review, not the independent security review, professional method review, provider certification, or crisis-service review required for a final V1.0 release.

## Findings resolved before acceptance

- **P1 — negative assertions could pass on a missing or wrong-typed value.** Missing targets, incompatible `contains` operands, non-text regex targets, and malformed memory items now fail explicitly. Regression tests cover each vacuous-pass shape.
- **P1 — the initial offline crisis projection constructed a provider request after selecting the crisis route.** Crisis projection now emits an empty provider request, and both critical crisis cases assert no dispatch.
- **P2 — provider reports discarded their evaluation timestamp.** Provider-result mode now requires one identical timestamp, provider identity, capability class, and prompt bundle across the complete 21-case set and includes them in the report.
- **P2 — expired region-specific numbers remained displayable.** Core now suppresses regional numbers and sources after `source_check_due`, returning the conservative no-number fallback. The stale-path test is date-controlled.
- **P2 — generic guidance told a nearby person to remain with the user even in a potential other-harm case.** Guidance now asks for help without putting that person in danger.
- **P2 — secondary durable fields reached rich crisis presentation only at the Vault fallback.** CLI and desktop precheck the same complete payload fields enforced by Vault, while Vault remains the authoritative invariant.
- **P2 — structured desktop errors became large enough to trigger `result_large_err`.** Optional guidance is boxed; strict Clippy passes.

## Evidence

- `cargo test -p quisum-core -p quisum-evals -p quisum-cli -p quisum-desktop`
- `cargo clippy -p quisum-core -p quisum-evals -p quisum-cli -p quisum-desktop --all-targets --all-features -- -D warnings`
- `cargo run -p quisum-evals --bin quisum-eval -- --json` — 21/21 offline policy cases, zero critical failures
- `npm run check --prefix apps/desktop` — TypeScript plus seven frontend tests
- `node scripts/validate-assets.mjs` — schemas/assets, 26 JSONL files, seven method packages, 21 eval cases
- `git diff --check` and targeted panic/TODO scans

The provider-result path is exercised with a complete synthetic run in unit tests. No live provider output was evaluated or certified in this stage. Official methods remain `unreviewed`.

## Result

No known P0, P1, or P2 finding remains in this stage's scope. The final V1.0 release gates that require external people, platform signing identities, or live provider evidence remain open and are not represented as complete.
