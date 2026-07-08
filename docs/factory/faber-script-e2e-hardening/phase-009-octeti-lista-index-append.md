# Phase 009 - Octeti Lista Index And Append

## Interpreted Unit

Make the script stepper honor the shipped `octeti` as `lista<numerus<u8>>`
surface for the representative `octeti/unify.fab` fixture.

## Normalized Spec

Functional requirements:

- `octeti.accipe(index)` returns the byte at `index` as a `numerus` value.
- Out-of-bounds octeti index still reports an ordinary stepper index error.
- `octeti.appende(byte)` accepts a numeric byte value and mutates a place-backed
  octeti receiver.
- Byte append rejects non-numeric or out-of-range values instead of truncating.
- Existing array, set, map, and text collection behavior remains unchanged.

Constraints and non-goals:

- Do not redesign `Value::Octeti` storage for this phase.
- Do not implement every lista method on octeti; target the fixture's index and
  append requirements.
- Do not change semantic typing; it already accepts the fixture.
- Do not weaken collection errors or hide out-of-bounds behavior.

## Repo-Aware Baseline

Evidence:

- `octeti/unify.fab` currently prints `4` and then fails with
  `index receiver type mismatch`.
- MIR lowers `buf.accipe(0)` to `runtime collection index(_0, const int 0)`.
- MIR lowers `buf.appende(b)` to `runtime collection append(_0, _1)`.
- `../norma/src/README.md` records that `octeti` is semantic sugar for
  `lista<numerus<u8>>` and carries every `lista` method.
- `docs/design/lista-intrinsics.md` defines `accipe`, `appende`, and
  `longitudo` as compiler-owned methods.

## Stage Graph

1. Add octeti byte indexing to the script stepper collection runtime.
2. Add place-backed octeti append with checked byte conversion.
3. Add focused fixture and byte-append tests.
4. Run the representative fixture and script e2e gate.
5. Inspect S-expression, Wasm, and LLVM text status for the representative
   fixture and record backend blockers.

## Implementation Work

Primary write surfaces:

- `crates/radix/src/mir/stepper/runtime.rs`
- `crates/radix/src/mir/stepper_test.rs`
- `docs/factory/faber-script-e2e-hardening/ledger.md`

Out of scope:

- Full octeti/lista method parity.
- Rust codegen changes.
- S-expression/Wasm/LLVM broad octeti backend design.

## Checkpoints And Gates

Checkpoint target:

- `cargo run -- run ../radix/crates/exempla/corpus/octeti/unify.fab`
  passes.

Gate expectations:

- Focused stepper tests pass.
- The ignored script e2e gate is run and changed failure status is recorded.

Release checkpoint:

- Deferred. This is internal script/MIR hardening only.

## Validation

Planned commands:

```bash
timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_octeti_unify_fixture
timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_appends_octeti_byte_in_place
cargo run -- run ../radix/crates/exempla/corpus/octeti/unify.fab
timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture
cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp ../radix/crates/exempla/corpus/octeti/unify.fab
cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm-text ../radix/crates/exempla/corpus/octeti/unify.fab
cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm-text ../radix/crates/exempla/corpus/octeti/unify.fab
cargo fmt --all -- --check
git diff --check
```

## Companion Skill Plan

- `factory`: phase execution and checkpoint discipline.
- `faber`: grammar/exempla/compiler authority.

## Open Questions

No blocking open questions.
