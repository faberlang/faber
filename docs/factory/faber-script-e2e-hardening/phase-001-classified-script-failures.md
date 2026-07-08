# Phase 001 - Classified Script Expected Failures

## Interpreted Unit

Create the first factory checkpoint for `exempla_script_e2e`: replace the flat
script expected-failure list with structured reason buckets while preserving the
current gate behavior for unclassified failures.

## Normalized Spec

Functional requirements:

- The script harness classifies every expected failure with one named bucket.
- Bucket names match the goal taxonomy where applicable.
- Paths outside the classified expected set still fail as unexpected failures.
- The harness output summarizes classified buckets so future phases can choose
  the next implementation category from live evidence.
- Do not absorb the 2026-06-29 unclassified failures into expected buckets in
  this phase.

Constraints and non-goals:

- Classification-only phase; no MIR lowering, stepper, or backend codegen
  behavior changes.
- No floor weakening.
- No full release validation as an inner-loop command.
- S-expression, Wasm, and LLVM inspection is not applicable because this phase
  does not implement a MIR surface.

## Repo-Aware Baseline

Evidence:

- Goal source: `docs/factory/faber-script-e2e-hardening/goal.md`.
- Harness: `crates/exempla/src/exempla_e2e/script.rs`.
- Shared helpers: `crates/exempla/src/exempla_e2e/common.rs`.
- Live baseline command:
  `timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored --nocapture`.

Live baseline on this phase start:

- `173/258` pass.
- `175/258` stepper ran.
- `28/258` output checked.
- Harness fails on 31 paths outside the current expected-failure list.

## Stage Graph

1. Introduce typed script expected-failure buckets and classified entries.
2. Update gate checks to use classified entries.
3. Print expected-failure bucket counts in the harness summary.
4. Add focused unit coverage for classification lookup/counting.
5. Run focused tests and the script e2e gate.

## Implementation Work

Primary write surfaces:

- `crates/exempla/src/exempla_e2e/script.rs`
- `crates/exempla/src/exempla_e2e/script_test.rs` if focused tests require a
  dedicated sibling module
- `crates/exempla/src/exempla_e2e/mod.rs` only to wire the test module
- This phase artifact

## Checkpoints And Gates

Checkpoint target:

- The script e2e harness uses structured expected-failure classifications and
  still fails on currently unclassified failures.

Gate expectations:

- Focused classification tests pass.
- `exempla_script_e2e` still reports the same live pass/run/output counts unless
  unrelated current-state behavior changes are discovered.
- Failure on unclassified paths is acceptable and required in this phase.

Release checkpoint:

- Deferred. This phase is test harness hardening, not a release-worthy user
  feature.

## Validation

Planned commands:

```bash
timeout 120 cargo test -p exempla script_expected_failure
timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored --nocapture
cargo fmt --all -- --check
git diff --check
```

## Companion Skill Plan

- `factory`: phase execution and checkpoint discipline.
- `faber`: repo authority, grammar/exempla discipline.

## Open Questions

No blocking open questions.

