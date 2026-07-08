# Phase 029 - Tacet No-op Lowering

## Invariant

`tacet` is an explicit no-op statement. MIR lowering must preserve control flow
by emitting no statement and leaving the current block open.

## Scope

- Lower `HirStatementKind::Tacet` as a no-op in statement-level MIR lowering.
- Replace the old unsupported-lowering regression with a no-op lowering test.
- Cover `tacet/tacet.fab` and promote it from `unsupported-mir` only after the
  script harness proves it passes.

## Non-Scope

- Changing unreachable terminator semantics.
- Changing `tacet` parsing or Faber/Rust/Go pretty-print behavior.
- Broad statement lowering refactors.

## Backend Status To Record

- Rust already emits `tacet` as an explicit no-op comment in source codegen.
- S-expression, Wasm, and LLVM should not require new support because no MIR
  statement is emitted for `tacet`; inspect representative emits.

## Validation Plan

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix lowers_tacet_to_noop -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_tacet_fixture -- --nocapture`
- Direct `faber run` for `../radix/crates/exempla/corpus/tacet/tacet.fab`.
- Representative `radix emit` probes for Rust, S-expression, Wasm, and LLVM.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
- `timeout 120 cargo fmt --all -- --check`
- `git diff --check`
