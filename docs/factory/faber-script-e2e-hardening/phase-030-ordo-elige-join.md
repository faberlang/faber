# Phase 030 - Ordo Elige Join Reachability

## Invariant

Unit-variant `elige`/`discerne` lowering preserves normal arm fallthrough. When
an arm body completes normally, it must jump to the join block and later source
statements must continue lowering. Only the unmatched no-default path is
unreachable. All-terminating unit-variant `discerne` arms must not synthesize a
no-value fallthrough return, and `→ nihil` side-effect functions must accept
no-value returns.

## Scope

- Fix unit-variant `discerne` lowering to track reachable arm bodies.
- Keep all-returning unit-variant `discerne` functions free of synthetic
  no-value fallthrough returns.
- Align MIR validation for `→ nihil` side-effect functions with semantic
  return-path checking and Rust backend behavior.
- Cover `ordo/ordo.fab`, which has two statement-level `elige` expressions in
  sequence.
- Remove `ordo/ordo.fab` from `unsupported-mir` only after the script harness
  proves it passes.

## Non-Scope

- Changing `mori`/panic `Unreachable` semantics.
- Implementing broad `Unreachable` execution in the stepper.
- New `ordo` syntax or enum representation changes.
- CLI-specific operand lowering.

## Backend Status To Record

- Rust is the reference for `ordo` selection output.
- S-expression, Wasm, and LLVM already support branch/unreachable terminators;
  inspect representative `ordo` emits after the MIR join fix.

## Validation Plan

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix lowers_ordo_elige_statement_continues_after_match -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix lowers_all_returning_variant_discerne_without_fallthrough_return -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix allows_no_value_return_for_nihil_side_effect_function -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_ordo_fixture -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_discerne_fixture -- --nocapture`
- Direct `faber run` for `../radix/crates/exempla/corpus/ordo/ordo.fab`.
- Direct `faber run` for `../radix/crates/exempla/corpus/discerne/discerne.fab`.
- Representative `radix emit` probes for Rust, S-expression, Wasm, and LLVM.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
- `timeout 120 cargo fmt --all -- --check`
- `git diff --check`
