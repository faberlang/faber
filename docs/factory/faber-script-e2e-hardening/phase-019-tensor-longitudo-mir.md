# Phase 019 - Tensor Longitudo MIR

## Target

Several remaining tensor failures stop at `method call before runtime/provider
MIR lowering` on `tensor.longitudo()`. The intrinsic registry already recognizes
tensor `longitudo`, but marks it codegen-only, so MIR lowering cannot reuse the
existing collection length runtime operation.

## Invariant

Tensor `longitudo()` lowers to the same MIR collection length intrinsic used by
other eager collection-like values. This phase does not implement the broader
tensor method catalog or introduce a new tensor runtime representation.

## Scope

- Promote only tensor `longitudo()` from codegen-only to MIR collection length.
- Add focused tests showing tensor `longitudo()` lowers to `MirCollectionOp::Length`
  and runs through the stepper for `tensor/decl.fab`.
- Inspect S-expression, Wasm, and LLVM probe status for the promoted MIR surface.
- Run the script e2e harness and update the factory ledger.

## Out of Scope

- Tensor `magnitudines`, `forma`, `structa`, `accipe`, arithmetic, `planata`, or
  other tensor runtime methods.
- A dedicated MIR tensor value representation.
- Fixing frontend failures in `instans/instans.fab` or
  `conversio/valor-genus.fab`.

## Validation

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix lowers_tensor_longitudo_to_collection_length -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_tensor_decl_fixture -- --nocapture`
- `cargo run -- run ../radix/crates/exempla/corpus/tensor/decl.fab`
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t sexp ../radix/crates/exempla/corpus/tensor/decl.fab`
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t wasm ../radix/crates/exempla/corpus/tensor/decl.fab`
- `cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t llvm ../radix/crates/exempla/corpus/tensor/decl.fab`
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
