# Phase 026 - Explicit Script Entry

## Invariant

Script-mode execution runs only the lowered `incipit` entry block. Declaration
and test-runner surfaces such as `proba ... omitte` may lower to MIR functions,
but they are not script entries and must not be executed by `faber run`.

## Scope

- Make the stepper select only an explicit lowered `incipit` function.
- Preserve the broader MIR probe naming fallback for S-expression, Wasm, and
  LLVM unless a backend-specific probe requires a narrower policy.
- Cover `omitte/omitte.fab` so skipped test bodies do not execute under script
  mode.
- Classify newly exposed declaration-only test fixtures as `no-entry-reference`
  when the script harness proves they have no runnable `incipit`.

## Non-Scope

- Implementing a script test runner.
- Executing `proba`, `probandum`, `omitte`, or `futurum` bodies in script mode.
- Redesigning MIR function metadata to add a first-class entry identifier.
- Changing S-expression, Wasm, or LLVM entry fallback behavior without direct
  evidence of an avoidable backend inconsistency.

## Backend Status To Record

- Rust already lowers `omitte` to an ignored Rust test, not to an application
  entry point.
- S-expression, Wasm, and LLVM use `MirNames::entry_function()` as a probe
  convenience; this phase should inspect them but avoid changing probe fallback
  behavior unless required.

## Validation Plan

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_rejects_omitte_without_incipit -- --nocapture`
- `timeout 120 cargo run -- run ../radix/crates/exempla/corpus/omitte/omitte.fab`
- `timeout 120 cargo run --manifest-path ../radix/Cargo.toml -p radix --bin radix -- emit -t rust ../radix/crates/exempla/corpus/omitte/omitte.fab`
- Representative `radix emit` probes for S-expression, Wasm, and LLVM.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
- `timeout 120 cargo fmt --all -- --check`
- `git diff --check`
