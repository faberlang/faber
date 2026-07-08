# Phase 006 - Script Stepper Collection Conversions

## Interpreted Unit

Implement script-stepper runtime conversions for the collection bridges exercised
by `conversio/collectiones.fab`.

## Normalized Spec

Functional requirements:

- `lista ↦ copia` deduplicates hashable values.
- `copia ↦ lista` materializes set members.
- `lista ↦ tensor` and `tensor ↦ lista` use the script stepper's eager array
  representation.
- `lista ↦ cursor` and `cursor ↦ lista` use the script stepper's eager array
  representation.
- `tabula ↦ lista<K>` materializes keys and `tabula ↦ lista<V>` materializes
  values for primitive key/value shapes used by the corpus.

Constraints and non-goals:

- Do not implement full tensor storage, shape validation, or lazy cursor
  runtime behavior in the stepper.
- Do not broaden map/set key support beyond the existing `MapKey` subset.
- Do not change grammar, semantic typing, or Rust codegen.

## Repo-Aware Baseline

Evidence:

- `conversio/collectiones.fab` lowers to MIR but script execution fails with
  `FaberScript unsupported: runtime conversio to Set(TypeId(1))`.
- The stepper already stores `lista` as `Value::Array`, `copia` as `Value::Set`,
  and `tabula` as `Value::Map`.
- The representative corpus file checks only materialized lengths, so eager
  array-backed cursor/tensor representation is sufficient for this phase.

## Stage Graph

1. Add target-directed collection conversion support to `stepper/conversio.rs`.
2. Preserve existing hard errors for unsupported values or unhashable set items.
3. Add a focused regression covering list/set/map/tensor/cursor conversion
   lengths.
4. Run `conversio/collectiones.fab` and the ignored script e2e gate.

## Implementation Work

Primary write surfaces:

- `crates/radix/src/mir/stepper/conversio.rs`
- `crates/radix/src/mir/stepper/value.rs` if helper access is needed
- `crates/radix/src/mir/stepper_test.rs`
- `docs/factory/faber-script-e2e-hardening/ledger.md`

Out of scope:

- S-expression/Wasm/LLVM collection conversion backend support beyond recording
  current status.
- Tensor arithmetic or cursor laziness.

## Checkpoints And Gates

Checkpoint target:

- `cargo run -- run
  ../radix/crates/exempla/corpus/conversio/collectiones.fab` passes or moves to a new,
  recorded blocker after set conversion.

Gate expectations:

- Focused collection conversion stepper test passes.
- The ignored script e2e gate is run and changed failure status is recorded.

Release checkpoint:

- Deferred. This is internal script/MIR hardening.

## Validation

Planned commands:

```bash
timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_collection_conversio_fixture
cargo run -- run ../radix/crates/exempla/corpus/conversio/collectiones.fab
timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture
cargo fmt --all -- --check
git diff --check
```

## Companion Skill Plan

- `factory`: phase execution and checkpoint discipline.
- `faber`: grammar/exempla/compiler authority.

## Open Questions

No blocking open questions.
