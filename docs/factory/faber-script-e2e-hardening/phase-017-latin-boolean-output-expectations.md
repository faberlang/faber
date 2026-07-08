# Phase 017 - Latin Boolean Output Expectations

## Target

Two script-executable fixtures now run successfully but fail output comparison
because their expected files use Rust boolean text:

- `../radix/crates/exempla/corpus/binarius/binarius.fab`
- `../radix/crates/exempla/corpus/vel/vel.fab`

The in-process script host displays direct `bivalens` values as Faber surface
text, `verum` and `falsum`. Most existing script-checked boolean fixtures already
expect that form.

## Invariant

Direct script diagnostic output for `bivalens` uses the Faber surface literals
`verum` and `falsum`. Explicit `↦ textus` conversion behavior is separate and
not changed by this phase.

## Scope

- Update `binarius.expected` and `vel.expected` to the script host's direct
  boolean output.
- Remove those two paths from `SCRIPT_EXPECTED_FAILURES` once they pass.
- Run direct fixture checks and the script e2e harness.
- Update the factory ledger.

## Out of Scope

- Changing Rust codegen boolean formatting.
- Changing `bivalens ↦ textus` conversion output.
- Reclassifying or fixing unrelated remaining failures.

## Validation

- `cargo run -- run ../radix/crates/exempla/corpus/binarius/binarius.fab`
- `cargo run -- run ../radix/crates/exempla/corpus/vel/vel.fab`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p exempla script_expected_failure -- --nocapture`
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored
  --nocapture`
