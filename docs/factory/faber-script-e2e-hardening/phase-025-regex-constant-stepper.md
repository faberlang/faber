# Phase 025 - Regex Constant Stepper

## Invariant

Script-mode `regex` values mirror `faber::Regex`: they are displayable pattern
carriers. This phase does not add matching, search, or regex-engine semantics.

## Scope

- Evaluate `MirConstant::Regex` in the stepper.
- Display regex values as their pattern text, matching `faber::Regex::Display`.
- Cover shipped `"..." ↦ regex` fixtures:
  `literalia/regex.fab` and `lege/lege.fab`.

## Non-Scope

- Slash regex literal lexer work.
- Regex matching/search APIs.
- Broad `lege` stdin semantics beyond the already-lowered `ReadLine` intrinsic.
- S-expression support for octeti/regex constants.

## Backend Status To Record

- Rust already emits `faber::Regex::new(...)` and displays the carrier pattern.
- Wasm and LLVM already expose regex literal/import probe paths.
- S-expression still rejects octeti/regex constants as a pre-existing probe gap.

## Validation Plan

- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_regex_conversio_fixture -- --nocapture`
- `timeout 120 cargo test --manifest-path ../radix/Cargo.toml -p radix stepper_runs_lege_regex_fixture -- --nocapture`
- Direct `faber run` for `literalia/regex.fab` and `lege/lege.fab` with stdin.
- Representative `radix emit` probes for S-expression, Wasm, and LLVM.
- `timeout 300 cargo test --manifest-path ../radix/Cargo.toml -p exempla exempla_script_e2e -- --ignored --nocapture`
- `timeout 120 cargo fmt --all -- --check`
- `git diff --check`
