# Phase 023 - Valor Genus Stepper

## Invariant

Script-mode `valor ↔ genus` follows the Rust backend's shipped JSON object
extraction and boxing semantics without changing interval-clamp conversion
syntax.

## Scope

- Parse capitalized user-defined conversio targets such as `↦ Persona` as type
  targets, while preserving lowercase stored interval operands such as
  `↦ fines`.
- Run `valor ↦ genus` in the MIR stepper using existing struct field metadata.
- Run `genus ↦ valor` in the MIR stepper as a `valor` tabula carrier.
- Keep the phase bounded to `conversio/valor-genus.fab`.

## Non-Scope

- Redesigning conversio target grammar.
- Full parity for every nested Rust backend `valor` extraction case.
- S-expression, Wasm, or LLVM aggregate carrier redesign.

## Validation Plan

- Parser regression for `payload ↦ Persona`.
- Focused stepper regression for `conversio/valor-genus.fab`.
- Direct ``faber run`` for `conversio/valor-genus.fab`.
- Script e2e harness to confirm the unexpected failure is resolved.
- Formatter and diff whitespace checks.
