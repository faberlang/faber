# Phase 016 - Logical Short-Circuit MIR Lowering

## Target

`../radix/crates/exempla/corpus/binarius/binarius.fab` exposes eager logical evaluation in
script mode:

```fab
sit brevis ← falsum et carum()
```

The stepper currently calls `carum()` and prints `hoc non videatur`. Rust codegen
uses `&&`/`||` and does not evaluate the right-hand side when the left side
settles the result.

## Invariant

HIR `et` and `aut` must preserve source short-circuit semantics in MIR. MIR may
still contain eager `MirBinOp::And` and `MirBinOp::Or` for values that are already
side-effect-free, but lowering from HIR logical operators must produce
branch-shaped control flow.

## Scope

- Lower expression-valued `et` and `aut` to branch-shaped MIR that writes a
  boolean result temp.
- Add focused MIR lowering and stepper tests proving the right-hand side is not
  evaluated when short-circuited.
- Re-run `binarius/binarius.fab` to confirm the extra `carum()` diagnostic
  disappears.
- Inspect S-expression, Wasm, and LLVM MIR-boundary emitters for the resulting
  branch-shaped MIR status.

## Out of Scope

- Boolean display policy (`true`/`false` versus `verum`/`falsum`) and stale
  expected files.
- Changing parser precedence or HIR operator representation.
- Rewriting existing backend boolean rendering.

## Validation

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix <focused filters>`
- `cargo run -- run ../radix/crates/exempla/corpus/binarius/binarius.fab`
- Targeted S-expression, Wasm, and LLVM `radix emit` checks for
  `../radix/crates/exempla/corpus/binarius/binarius.fab`
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture`
