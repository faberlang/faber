# Phase 034 - Nested JSON Valor MIR Lowering

## Invariant

Compile-time JSON `valor` literals lower nested object and array values
recursively into MIR aggregate construction. Nested values must not fall
through to unsupported nested-aggregate diagnostics or path fallout.

## Scope

- Lower nested JSON object values inside `valor` literals as `valor`-typed map
  aggregates.
- Lower nested JSON array values inside `valor` literals as `valor`-typed array
  aggregates.
- Cover `destructura/literal.fab`.
- Remove `destructura/literal.fab` from `unsupported-mir` only after the script
  harness proves it passes.

## Non-Scope

- Canonicalizing map display/output ordering for `valor` debug output.
- Runtime JSON parsing/serialization providers.
- General anonymous object literal support.
- Broad aggregate representation redesign.

## Backend Status To Record

- Rust is the reference and already emits recursive `faber::Valor::Tabula` and
  `faber::Valor::Lista` trees for nested JSON literals.
- S-expression, Wasm, and LLVM should be inspected as representative aggregate
  probes. New runtime import names should not be needed if MIR lowers nested
  JSON through existing aggregate construction.

## Validation Plan

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix lowers_nested_json_valor_literal -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_destructura_literal_fixture -- --nocapture`
- Direct `faber run` for `../radix/crates/exempla/corpus/destructura/literal.fab`.
- Representative `radix emit` probes for Rust, S-expression, Wasm, and LLVM.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
- `timeout 120 cargo fmt --all -- --check`
- `git diff --check`
