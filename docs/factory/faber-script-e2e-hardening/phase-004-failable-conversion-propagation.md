# Phase 004 - Failable Runtime Conversion Propagation

## Interpreted Unit

Implement script-stepper propagation for bare runtime conversion failures inside
functions with an alternate-exit channel, so existing MIR `try_call` handlers can
observe those failures.

## Normalized Spec

Functional requirements:

- A bare runtime conversion failure inside a function declared with `⇥` becomes
  that function's alternate return value instead of a top-level stepper error.
- `try_call` observes that alternate return and routes to its error block.
- A `fac { ... } cape err { ... }` that calls a failable function through
  `try_call` can catch a
  propagating conversion failure.
- Inline conversion recovery (`⇥ fallback`) continues to return the fallback
  value locally and does not propagate.
- Non-failable functions keep current hard failure behavior for bare conversion
  failures.
- Unsupported conversions and interpreter defects remain ordinary
  `StepperError`s; only runtime conversion parse/coercion failure participates
  in alternate-exit flow.

Constraints and non-goals:

- Do not change grammar, HIR, semantic typing, or MIR lowering.
- Do not implement direct handled conversion lowering for
  `fac { expr ↦ T } cape ...`; current MIR does not encode an error edge for
  that form.
- Do not change Rust codegen behavior; this phase aligns the script stepper with
  already-tested Rust failable conversion semantics.

## Repo-Aware Baseline

Evidence:

- `conversio/fallibilis.fab` currently lowers to MIR but script execution fails
  with `textus to instans conversion failed`.
- MIR already has `ReturnError` and `TryCall` execution support in the stepper.
- Stepper runtime conversion currently returns `StepperError` for bare
  conversion failures, so `run_until_depth` treats them as interpreter errors
  before failable control flow can observe them.
- Rust codegen tests under
  `crates/radix/src/codegen/rust/tests/failable_test.rs` already assert bare
  `↦` propagation in declared failable and `fac` contexts.

## Stage Graph

1. Introduce an internal runtime-call outcome for recoverable conversion
   failures.
2. Convert recoverable conversion failures to alternate returns only when the
   active MIR function has `error_ty`.
3. Preserve hard stepper errors for unsupported conversion targets and
   non-failable functions.
4. Add focused stepper tests for direct failable propagation, `try_call`/`fac`
   handling, inline recovery, and non-failable hard failure.
5. Run representative `fallibilis`/`instans` probes and the script e2e gate.

## Implementation Work

Primary write surfaces:

- `crates/radix/src/mir/stepper/runtime.rs`
- `crates/radix/src/mir/stepper/conversio.rs`
- `crates/radix/src/mir/stepper/mod.rs`
- Focused stepper tests under `crates/radix/src/mir/stepper*_test.rs`
- `docs/factory/faber-script-e2e-hardening/ledger.md`

Out of scope:

- S-expression/Wasm/LLVM failable backend support.
- Rewriting `conversio/instans.fab` precision expectations.
- Broader exception or `cape` lowering beyond current MIR.

## Checkpoints And Gates

Checkpoint target:

- The `parseInstans`/`tutum` path from `conversio/fallibilis.fab` is covered by
  a focused stepper regression and moves from hard conversion failure to handled
  `try_call` recovery.
- The full `conversio/fallibilis.fab` may still fail on direct handled
  conversion lowering; record that status if observed.

Gate expectations:

- Focused stepper propagation tests pass.
- `radix mir` still lowers `conversio/fallibilis.fab`.
- The ignored script e2e gate is run and changed failure status is recorded.

Release checkpoint:

- Deferred. This is internal script/MIR hardening.

## Validation

Planned commands:

```bash
timeout 120 cargo test -p radix stepper_propagates_failable_runtime_conversio
timeout 120 cargo test -p radix stepper_keeps_inline_conversio_recovery_local
timeout 120 cargo test -p radix stepper_keeps_non_failable_conversio_failure_hard
cargo run -p faber-cli -- run crates/exempla/corpus/conversio/fallibilis.fab
timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored --nocapture
cargo fmt --all -- --check
git diff --check
```

## Companion Skill Plan

- `factory`: phase execution and checkpoint discipline.
- `faber`: grammar/exempla/compiler authority.

## Open Questions

No blocking open questions.
