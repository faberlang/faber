# Phase 021 - Classify Instans Norma Import

## Target

`instans/instans.fab` still appears as an unexpected script e2e failure, but it
imports `norma:hal/tempus` and `norma:toml`. The goal explicitly excludes
`norma:*` imports from script-mode implementation and Phase 018 added a
`norma-import` bucket for this case.

## Invariant

Classifying `instans/instans.fab` as `norma-import` documents that it is outside
the current in-memory script obligation. This must not hide runnable MIR debt in
fixtures that do not import `norma:*`.

## Scope

- Add `instans/instans.fab` to the script expected-failure taxonomy under
  `norma-import`.
- Validate the taxonomy and full script harness counts.
- Update the factory ledger.

## Out of Scope

- Implementing `norma:hal/tempus` or `norma:toml` in script mode.
- Classifying `conversio/valor-genus.fab`, which is a real frontend conversion
  gap and does not fit the `norma-import` bucket.
- Any MIR lowering or stepper behavior changes.

## Validation

- `timeout 120 cargo test -p exempla script_expected_failure -- --nocapture`
- `timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored --nocapture`
