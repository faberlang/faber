# Phase 031 - Lista Mutation Method MIR Lowering

## Invariant

Compiler-owned `lista` mutation methods that Rust already executes lower to
explicit MIR collection operations or equivalent MIR assignments. They must not
fall through to generic unresolved method-call diagnostics.

## Scope

- Promote `lista.decapita()` into MIR as a front-removal operation returning
  `T ∪ nihil`.
- Lower `lista.ordina()` as in-place sorting by assigning the existing sorted
  collection result back to the receiver.
- Cover `lista/methodi-mutatio.fab`, which also exercises existing `appende`
  and `inverte` MIR collection operations.
- Cover `morphologia/morphologia.fab` when the same `ordina` blocker is removed
  and the harness proves the fixture now passes.
- Remove fixtures from `unsupported-mir` only after the script harness proves
  they pass.

## Non-Scope

- New higher-order list method lowering such as `reducta`.
- New copy/view list method lowering such as `sectio`, `prima`, `ultima`, and
  `omissa`.
- Broad collection intrinsic redesign or target backend execution semantics.

## Backend Status To Record

- Rust is the reference for mutation output.
- S-expression, Wasm, and LLVM should expose the promoted MIR collection
  operation through their existing runtime-call/import probe paths when those
  paths can represent it.

## Validation Plan

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix lowers_lista_mutatio_methods -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_lista_methodi_mutatio_fixture -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_morphologia_fixture -- --nocapture`
- Direct `faber run` for `../radix/crates/exempla/corpus/lista/methodi-mutatio.fab`.
- Direct `faber run` for `../radix/crates/exempla/corpus/morphologia/morphologia.fab`.
- Representative `radix emit` probes for Rust, S-expression, Wasm, and LLVM.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
- `timeout 120 cargo fmt --all -- --check`
- `git diff --check`
