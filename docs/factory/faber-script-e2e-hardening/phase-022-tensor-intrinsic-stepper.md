# Phase 022 - Tensor Intrinsic Stepper

## Target

Four remaining unexpected script e2e failures are tensor fixtures blocked by
`method call before runtime/provider MIR lowering` and related unresolved tensor
runtime values:

- `tensor/shape.fab`
- `tensor/textus.fab`
- `tensor/arithmetic-elementwise.fab`
- `tensor/arithmetic-reduction.fab`

The intrinsic registry already owns these method names for Rust codegen, but the
MIR/stepper path still treats most tensor methods as codegen-only.

## Invariant

MIR script execution should model existing tensor runtime semantics for the
compiler-owned tensor intrinsic catalog. The stepper may use an eager in-memory
tensor value, but this phase must not redesign MIR or invent new tensor language
semantics.

## Scope

- Add MIR collection operation variants for tensor shape, construction,
  reshape, indexing, mutation, fill, flatten, slicing, elementwise arithmetic,
  and reductions.
- Promote the matching tensor registry rows from codegen-only to MIR collection
  operations.
- Add an explicit stepper tensor value carrying flat data and runtime shape.
- Preserve existing Rust runtime semantics closely:
  rank is `longitudo`, `magnitudines` returns the shape vector, `structa`
  checks element count, `forma` checks element count, `sectio` slices axis 0,
  and arithmetic is elementwise for compatible flat data.
- Improve array display so tensor fixture expected output can match
  Rust-backed list output.
- Run focused tensor fixture tests, direct CLI runs, backend probe checks, and
  the script e2e harness.

## Out of Scope

- General ndarray broadcasting beyond fixture-compatible elementwise data.
- A full MIR tensor ABI or backend execution model.
- Fixing `conversio/valor-genus.fab`.
- S-expression or Wasm tensor representation design if existing probe
  architecture still lacks tensor carriers.

## Validation

- `timeout 120 cargo test -p radix lowers_tensor_intrinsic_methods_to_collection_ops -- --nocapture`
- `timeout 120 cargo test -p radix stepper_runs_tensor_shape_fixture -- --nocapture`
- `timeout 120 cargo test -p radix stepper_runs_tensor_textus_fixture -- --nocapture`
- `timeout 120 cargo test -p radix stepper_runs_tensor_arithmetic_elementwise_fixture -- --nocapture`
- `timeout 120 cargo test -p radix stepper_runs_tensor_arithmetic_reduction_fixture -- --nocapture`
- `cargo run -p faber-cli -- run crates/exempla/corpus/tensor/shape.fab`
- `cargo run -p faber-cli -- run crates/exempla/corpus/tensor/textus.fab`
- `cargo run -p faber-cli -- run crates/exempla/corpus/tensor/arithmetic-elementwise.fab`
- `cargo run -p faber-cli -- run crates/exempla/corpus/tensor/arithmetic-reduction.fab`
- `cargo run -p radix --bin radix -- emit -t sexp crates/exempla/corpus/tensor/shape.fab`
- `cargo run -p radix --bin radix -- emit -t wasm crates/exempla/corpus/tensor/shape.fab`
- `cargo run -p radix --bin radix -- emit -t llvm crates/exempla/corpus/tensor/shape.fab`
- `timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored --nocapture`
