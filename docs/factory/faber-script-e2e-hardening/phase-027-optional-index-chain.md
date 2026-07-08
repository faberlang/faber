# Phase 027 - Optional Index Chain

## Invariant

Optional access is null-safe: if an optional receiver or omitted `sponte` field
is absent, or if `receiver?[index]` misses/out-of-bounds, script execution
returns `nihil`. Ordinary index access `receiver[index]` remains strict and
reports bounds/type errors.

## Scope

- Make the MIR stepper return `Value::Nil` for optional-chain array and map
  index misses.
- Make omitted `sponte` struct fields remain absent in MIR construction and
  project as `Value::Nil` through optional field access.
- Keep ordinary projection and collection index behavior strict.
- Cover `optionalis/optionalis.fab` and `operatores/optional-chain.fab`.
- Remove those fixtures from `unsupported-mir` only if the full script harness
  proves they now pass.

## Non-Scope

- Optional call `?(...)` execution.
- Text slicing or range semantics beyond existing strict index behavior.
- Changing semantic typing of optional chains.
- Broad collection method lowering.

## Backend Status To Record

- Rust already emits optional-chain code paths for nullable field/call access;
  this phase should inspect representative Rust output for `?[index]`.
- Wasm and LLVM already route optional-chain index through projection-import
  probe helpers; inspect representative emits and defer only if a backend gap is
  architectural.
- S-expression status should be recorded from a representative emit probe.

## Validation Plan

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_optionalis_fixture -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_optional_chain_operator_fixture -- --nocapture`
- Direct `faber run` for both fixtures.
- Representative `radix emit` probes for Rust, S-expression, Wasm, and LLVM.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
- `timeout 120 cargo fmt --all -- --check`
- `git diff --check`
