# Phase 008 - Instans Exemplar Precision Reconciliation

## Interpreted Unit

Resolve the remaining script abort in `conversio/instans.fab` by reconciling the
fixture with the already-shipped `instans` runtime precision contract.

## Normalized Spec

Functional requirements:

- `conversio/instans.fab` must pass under the script stepper.
- Bare `instans` remains seconds precision: parsing a subsecond RFC3339 valor as
  bare `instans` stores only second precision.
- `instans ↦ instans<ms>` re-tags the stored value; it must not recover
  fractional bits already discarded by the source value.
- Compact numeric offsets (`+HHMM`) remain accepted, and the fixture's offset
  example must normalize to the same UTC instant it asserts.
- `valor ↦ instans<N>` recovery behavior remains unchanged.

Constraints and non-goals:

- Do not change the runtime precision contract in `../faber-runtime/src/instans.rs`.
- Do not weaken the fixture assertions; correct stale expected values instead.
- Do not change Rust codegen's broader `valor` literal boxing behavior in this
  phase; the generated binary is currently not a reliable oracle for this file.
- Do not absorb unrelated `instans/instans.fab` frontend failures.

## Repo-Aware Baseline

Evidence:

- `conversio/instans.fab` currently aborts in script mode.
- `../faber-runtime/src/instans.rs` truncates construction to declared precision,
  and `../faber-runtime/src/instans_test.rs` asserts that behavior.
- sibling radix `docs/factory/instans-primitive/goal.md` records construction zeroing finer
  bits and coarse-to-fine widening as lossless zero-padding.
- Runtime tests prove `1979-05-27T16:32:00+0900` normalizes to the same instant
  as `1979-05-27T07:32:00Z`; the fixture currently uses `07:32+0900`, which is
  not the same UTC instant.
- `faber build` currently compiles the fixture but the generated binary fails
  earlier because Rust codegen boxes `valor ← "..."` as `Valor::Textus`, so this
  phase uses the shipped runtime contract and script behavior as the local
  authority.

## Stage Graph

1. Correct `conversio/instans.fab` expected milliseconds and offset literal.
2. Add a focused stepper fixture test for the real exemplar.
3. Run ``faber run`` for the fixture.
4. Run the script e2e gate and record count movement.
5. Inspect MIR-boundary codegen status for the fixture and record blockers.

## Implementation Work

Primary write surfaces:

- `../radix/crates/exempla/corpus/conversio/instans.fab`
- `crates/radix/src/mir/stepper_test.rs`
- `docs/factory/faber-script-e2e-hardening/ledger.md`

Out of scope:

- `faber::Instans` runtime precision semantics.
- Rust codegen `valor` literal boxing.
- `../radix/crates/exempla/corpus/instans/instans.fab` frontend cleanup.

## Checkpoints And Gates

Checkpoint target:

- `cargo run -- run ../radix/crates/exempla/corpus/conversio/instans.fab`
  passes.

Gate expectations:

- Focused stepper fixture test passes.
- The ignored script e2e gate is run and changed failure status is recorded.

Release checkpoint:

- Deferred. This is exemplar/script harness hardening only.

## Validation

Planned commands:

```bash
timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_instans_conversio_fixture
cargo run -- run ../radix/crates/exempla/corpus/conversio/instans.fab
timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture
cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp ../radix/crates/exempla/corpus/conversio/instans.fab
cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm-text ../radix/crates/exempla/corpus/conversio/instans.fab
cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm-text ../radix/crates/exempla/corpus/conversio/instans.fab
cargo fmt --all -- --check
git diff --check
```

## Companion Skill Plan

- `factory`: phase execution and checkpoint discipline.
- `faber`: grammar/exempla/compiler authority.

## Open Questions

No blocking open questions.
