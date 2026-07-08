# Phase 028 - Named Variant Payloads

## Invariant

Named `discretio` variant payloads construct and project by field symbol.
`finge Variant { field = value }` and `discerne` payload destructuring must
agree on the same field metadata; symbol ids are not payload indices.

## Scope

- Make the MIR stepper execute `MirAggregateKind::EnumVariant` aggregates with
  named payload fields.
- Make variant field projection and assignment use field symbols instead of
  treating symbol ids as positional indices.
- Cover the `expected ordered aggregate fields` script failures:
  `finge/finge.fab`, `omnia/omnia.fab`,
  `integratio/discerne-insanum.fab`, and `unio/unio.fab`.
- Remove those fixtures from `unsupported-mir` only after the script harness
  proves they pass.

## Non-Scope

- Redesigning enum/variant MIR representation.
- Broad pattern matching semantics beyond existing lowered variant-field
  projections.
- Inline union Rust backend gaps unrelated to script-mode execution.
- New display formatting for aggregate values.

## Backend Status To Record

- Rust is the behavioral reference for the fixture outputs when it supports the
  surface.
- Wasm and LLVM already route aggregate construction through opaque aggregate
  helper imports; inspect representative named-variant emits.
- S-expression currently rejects aggregate construction; record the
  representative status instead of designing broad S-expression aggregate
  support.

## Validation Plan

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_finge_fixture -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_omnia_fixture -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_discerne_insanum_fixture -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_unio_fixture -- --nocapture`
- Direct `faber run` for all four fixtures.
- Representative `radix emit` probes for Rust, S-expression, Wasm, and LLVM.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
- `timeout 120 cargo fmt --all -- --check`
- `git diff --check`
