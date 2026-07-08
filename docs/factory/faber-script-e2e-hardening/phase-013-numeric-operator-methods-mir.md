# Phase 013 - Numeric Operator Methods MIR Lowering

## Target

`crates/exempla/corpus/intrinseca/numeric-operator-methods.fab` fails in script
mode on compiler-owned numeric receiver operator methods:

```text
unsupported MIR lowering: method call before runtime/provider MIR lowering
unsupported MIR lowering: method call before runtime/provider MIR lowering
unsupported MIR lowering: method call before runtime/provider MIR lowering
path that does not resolve to a local value
path that does not resolve to a local value
```

The fixture exercises pure receiver operators (`addita`, `multiplicata`,
`coniuncta`) and an imperativus update (`adde`).

## Invariant

Compiler-owned numeric operator receiver methods are syntax sugar for ordinary
MIR arithmetic, comparison, bitwise, or assignment operations; they are not late
runtime/provider calls.

## Scope

- Lower the fixture's numeric operator methods to existing MIR binary
  primitives and assignment statements.
- Preserve source receiver chaining: `x.addita(1.0).multiplicata(3.0)` lowers as
  `(x + 1.0) * 3.0`.
- Add focused MIR lowering and stepper tests for the fixture's arithmetic and
  bitwise methods.
- Inspect S-expression, Wasm, and LLVM MIR-boundary emitters. Because this phase
  lowers to existing binary MIR, backend work is expected to be either already
  supported or recorded as a pre-existing binary-op gap.

## Out of Scope

- Changing Rust codegen for chained numeric operator method precedence. The
  current Rust output is useful evidence, but it appears to emit
  `x + 1.0 * 3.0` for the chained expression instead of preserving the parsed
  receiver chain.
- Implementing tensor operator methods.
- Implementing every numeric method in the registry if it requires new MIR
  value kinds, runtime calls, or backend architecture.
- Redesigning method dispatch or introducing a generic intrinsic-call MIR node.

## Validation

- `timeout 120 cargo test -p radix <focused filters>`
- `cargo run -p faber-cli -- run
  crates/exempla/corpus/intrinseca/numeric-operator-methods.fab`
- `cargo run -p radix --bin radix -- mir
  crates/exempla/corpus/intrinseca/numeric-operator-methods.fab`
- Targeted S-expression, Wasm, and LLVM `radix emit` checks for
  `crates/exempla/corpus/intrinseca/numeric-operator-methods.fab`
- `timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored
  --nocapture`
