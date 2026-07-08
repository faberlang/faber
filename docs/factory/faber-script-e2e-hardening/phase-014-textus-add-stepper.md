# Phase 014 - Textus `+` Stepper Concatenation

## Target

Several script-reasonable exempla fail after reaching a binary `+` whose MIR type
is `textus`:

```text
numeric operand mismatch
```

Confirmed examples:

- `crates/exempla/corpus/assignatio/assignatio.fab`
- `crates/exempla/corpus/ego/ego.fab`
- `crates/exempla/corpus/redde/redde.fab`
- `crates/exempla/corpus/sub/sub.fab`

Rust codegen already emits `textus + textus` as string concatenation. MIR
lowering also preserves the result type as `textus`; the stepper currently routes
all `MirBinOp::Add` through numeric arithmetic.

## Invariant

When MIR binary `Add` carries `textus` values, the stepper must concatenate the
two strings. Non-text non-numeric operands should still fail closed.

## Scope

- Teach the MIR stepper to evaluate `Value::Str + Value::Str`.
- Add focused stepper tests for direct text concatenation and at least one
  exemplar that previously failed before producing its expected output.
- Inspect S-expression, Wasm, and LLVM MIR-boundary emitters for existing text
  `Add` support and record the status.
- Run the script e2e harness and record the pass/run/output count effect.

## Out of Scope

- Mixed `textus` plus non-`textus` coercions. Formatting and `↦ textus`
  conversions remain the explicit ways to stringify other values.
- Changing parser, HIR, or MIR lowering for `+`.
- Adding new `textus` receiver methods.

## Validation

- `timeout 120 cargo test -p radix <focused filters>`
- `cargo run -p faber-cli -- run
  crates/exempla/corpus/assignatio/assignatio.fab`
- `cargo run -p faber-cli -- run crates/exempla/corpus/ego/ego.fab`
- Targeted S-expression, Wasm, and LLVM `radix emit` checks for a representative
  text-add fixture
- `timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored
  --nocapture`
