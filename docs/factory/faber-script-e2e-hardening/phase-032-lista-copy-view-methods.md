# Phase 032 - Lista Copy/View Method MIR Lowering

## Invariant

Compiler-owned `lista` copy/view methods that Rust already executes lower to
explicit MIR collection operations. They return copied list values and never
fall through to generic unresolved method-call diagnostics.

## Scope

- Promote `lista.sectio(lo, hi)` into MIR as an exclusive range copy.
- Promote `lista.prima(n)` into MIR as a copied first-`n` view.
- Promote `lista.ultima(n)` into MIR as a copied last-`n` view with saturating
  start semantics.
- Promote `lista.omissa(n)` into MIR as a copied list with the first `n`
  elements skipped.
- Cover `lista/methodi-copiae.fab`.
- Remove `lista/methodi-copiae.fab` from `unsupported-mir` only after the
  script harness proves it passes.

## Non-Scope

- Higher-order list folding such as `reducta`.
- Nested JSON valor lowering in `destructura/literal.fab`.
- Broad collection intrinsic redesign or target backend execution semantics.

## Backend Status To Record

- Rust is the reference for copied list output and clipping behavior.
- S-expression, Wasm, and LLVM should expose the promoted MIR collection
  operations through their existing runtime-call/import probe paths when those
  paths can represent them.

## Validation Plan

- `timeout 120 cargo test -p radix lowers_lista_copy_view_methods -- --nocapture`
- `timeout 120 cargo test -p radix stepper_runs_lista_methodi_copiae_fixture -- --nocapture`
- Direct `faber run` for `crates/exempla/corpus/lista/methodi-copiae.fab`.
- Representative `radix emit` probes for Rust, S-expression, Wasm, and LLVM.
- `timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored --nocapture`
- `timeout 120 cargo fmt --all -- --check`
- `git diff --check`
