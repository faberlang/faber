# Phase 010 - Lista `summa` MIR Runtime Op

## Target

`../radix/crates/exempla/corpus/in/in.fab` fails in script mode because its helper body
uses `values.summa()`:

```text
unsupported MIR lowering: method call before runtime/provider MIR lowering
```

Rust emission already treats `lista<numerus>.summa()` as a compiler-owned
receiver intrinsic and lowers it to `.iter().copied().sum::<i64>()`. Semantic
analysis also rejects `summa()` on non-numeric `lista` element types.

## Invariant

`lista<numerus>.summa()` and `lista<fractus>.summa()` are existing
compiler-owned numeric list surfaces; MIR should lower them to one narrow
runtime op and the script stepper should fold numeric array values without
introducing generic reducer lowering.

## Scope

- Promote the existing `summa` registry row from codegen-only to a MIR
  collection runtime op.
- Validate that the runtime op has one collection receiver and returns that
  collection's numeric element type.
- Implement stepper evaluation for integer and float arrays, including empty
  lists.
- Add focused MIR/stepper tests for the `in` exemplar and an explicitly typed
  empty numeric list.
- Inspect S-expression, Wasm, and LLVM MIR-boundary emitters and add the
  established symbol/name plumbing when the existing runtime-call pattern
  applies.

## Out of Scope

- General `reducta` lowering.
- Tensor reductions.
- New numeric type-constraint architecture.
- Full backend runtime implementations for imported collection calls.

## Validation

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix <focused stepper filters>`
- `cargo run -- run ../radix/crates/exempla/corpus/in/in.fab`
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
- Targeted S-expression, Wasm, and LLVM `radix emit` checks for
  `../radix/crates/exempla/corpus/in/in.fab`, recording any architectural backend gaps.
