# Phase 020 - Valor Array Carrier MIR

## Target

`conversio/valor-tensor.fab` fails MIR validation before script execution:
`fixum valor arr ← [1, 2, 3, 4]` lowers as an ordered array aggregate carrying
semantic type `valor`, but validation currently accepts ordered aggregates only
when the aggregate type is `lista` or `copia`.

## Invariant

A valor-typed ordered array literal may act as an eager script carrier for
`valor ↦ lista<T>` conversion. This is a validation/modeling fix for the already
lowered carrier, not a general implementation of nested JSON valor aggregates.

## Scope

- Allow `MirAggregateKind::Array` with semantic type `valor` in MIR validation.
- Validate each ordered element operand, but skip homogeneous element
  assignability because the dynamic `valor` carrier intentionally accepts mixed
  scalar values.
- Add focused tests covering the `conversio/valor-tensor.fab` path.
- Inspect S-expression, Wasm, and LLVM probe status for the fixture.
- Run the script e2e harness and update the factory ledger.

## Out of Scope

- Nested JSON valor arrays or objects.
- Map/object valor carriers.
- Dedicated `Valor` MIR runtime representation.
- Tensor shape/arithmetic method support.

## Validation

- `timeout 120 cargo test -p radix validates_valor_array_carrier_aggregate -- --nocapture`
- `timeout 120 cargo test -p radix stepper_runs_valor_tensor_conversio_fixture -- --nocapture`
- `cargo run -p faber-cli -- run crates/exempla/corpus/conversio/valor-tensor.fab`
- `cargo run -p radix --bin radix -- emit -t sexp crates/exempla/corpus/conversio/valor-tensor.fab`
- `cargo run -p radix --bin radix -- emit -t wasm crates/exempla/corpus/conversio/valor-tensor.fab`
- `cargo run -p radix --bin radix -- emit -t llvm crates/exempla/corpus/conversio/valor-tensor.fab`
- `timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored --nocapture`
