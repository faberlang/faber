# Phase 024 - Conversio Output Ratchet

## Invariant

The script e2e expected output for `conversio/conversio.fab` reflects the
fixture's declared target types and the Rust/backend behavior already emitted
for the same program.

## Scope

- Update `conversio.expected` so the recovered `fractus` value is `0.0`.
- Remove `conversio/conversio.fab` from `SCRIPT_EXPECTED_FAILURES` once the
  output fixture matches live script behavior.
- Keep the phase classification-only/output-only; do not alter compiler or
  stepper semantics.

## Evidence To Check

- `f2` in the fixture is declared by `sit f2 ← "invalid" ↦ fractus ⇥ 0.0`.
- Rust emission lowers the value as `f64` with `unwrap_or(0.0)`.
- `faber run` prints the script result with `0.0`.

## Validation Plan

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_conversio_exemplum -- --nocapture`
- `timeout 120 cargo run -- run ../radix/crates/exempla/corpus/conversio/conversio.fab`
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
- `timeout 120 cargo fmt --all -- --check`
- `git diff --check`
