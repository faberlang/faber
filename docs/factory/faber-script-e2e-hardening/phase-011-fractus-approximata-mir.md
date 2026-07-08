# Phase 011 - Fractus `approximata` MIR Runtime Op

## Target

`crates/exempla/corpus/intrinseca/fractus-approximata.fab` fails in script
mode:

```text
unsupported MIR lowering: method call before runtime/provider MIR lowering
unsupported MIR lowering: path that does not resolve to a local value
```

The second error is fallout from the failed `near` initializer. Rust emission
already lowers `a.approximata(b, toleratio)` to
`((a - b).abs() <= toleratio)`.

## Invariant

`fractus.approximata(aliud, toleratio)` is a resolved fractus-only receiver
method with explicit absolute tolerance. MIR should represent it as one narrow
runtime comparison returning `bivalens`; `≈` must remain exact numeric value
equality and must not gain hidden tolerance behavior.

## Scope

- Promote the existing `approximata` registry row from codegen-only to a MIR
  collection runtime op.
- Validate three `fractus` operands and a `bivalens` result.
- Implement script-stepper evaluation as `abs(receiver - other) <= tolerance`.
- Add focused tests for the exemplar and for the registry/lowering shape.
- Inspect S-expression, Wasm, and LLVM MIR-boundary emitters and add only the
  existing runtime-call name plumbing where applicable.

## Out of Scope

- `numerus.approximata`.
- Tensor tolerance methods.
- Hidden epsilon behavior for `≈`.
- General expansion of mathesis catalog functions.

## Validation

- `timeout 120 cargo test -p radix <focused filters>`
- `cargo run -p faber-cli -- run
  crates/exempla/corpus/intrinseca/fractus-approximata.fab`
- `timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored
  --nocapture`
- Targeted S-expression, Wasm, and LLVM `radix emit` checks for
  `crates/exempla/corpus/intrinseca/fractus-approximata.fab`, recording any
  architectural backend gaps.
