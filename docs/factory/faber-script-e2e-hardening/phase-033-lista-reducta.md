# Phase 033 - Lista Reducta MIR Lowering

## Invariant

Compiler-owned `lista.reducta(reducer, init)` lowers to explicit MIR loop
control flow using a synthetic two-parameter reducer function. It returns the
final accumulator value and does not fall through to unresolved method-call or
path diagnostics.

## Scope

- Promote `lista.reducta((acc, x) ergo expr, init)` from `CodegenOnly` to the
  existing higher-order lista MIR lowering path.
- Reuse the current synthetic-closure function model used by `filtrata` and
  `mappata`.
- Cover `lista/methodi-functionales.fab`.
- Pin its stdout and remove it from `unsupported-mir` only after the script
  harness proves it passes.

## Non-Scope

- Nested JSON valor lowering in `destructura/literal.fab`.
- Failable callbacks or callable-value architecture changes.
- Broad higher-order collection redesign beyond the existing array receiver
  model.
- New S-expression, Wasm, or LLVM execution semantics beyond inspecting the
  MIR-boundary output paths.

## Backend Status To Record

- Rust is the reference and emits `.iter().cloned().fold(init, |acc, x| ...)`.
- S-expression, Wasm, and LLVM should be inspected as representative probes.
  Because MIR lowers `reducta` into ordinary loop, call, assignment, and array
  index operations rather than a new collection opcode, those backends should
  not need a new runtime import name for this phase.

## Validation Plan

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix lowers_lista_reducta_with_synthetic_reducer -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_lista_methodi_functionales_fixture -- --nocapture`
- Direct `faber run` for `../radix/crates/exempla/corpus/lista/methodi-functionales.fab`.
- Representative `radix emit` probes for Rust, S-expression, Wasm, and LLVM.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
- `timeout 120 cargo fmt --all -- --check`
- `git diff --check`
