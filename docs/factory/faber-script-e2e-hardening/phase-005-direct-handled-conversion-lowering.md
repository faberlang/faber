# Phase 005 - Direct Handled Conversion Lowering

## Interpreted Unit

Lower direct bare runtime conversions inside `fac { ... } cape ...` through an
explicit MIR error edge so the script stepper can execute the handler.

## Normalized Spec

Functional requirements:

- A direct `expr ↦ T` with no inline recovery inside an active local `cape`
  handler lowers to MIR with success and error control-flow edges.
- The lowering reuses existing `TryCall` execution instead of inventing a
  stepper-only handler lookup.
- On conversion success, the converted value is available at the original
  expression site.
- On conversion failure, the error message is written to the `cape` binding and
  execution jumps to the handler block.
- Inline conversion recovery (`expr ↦ T ⇥ fallback`) remains local and does not
  lower through the handler edge.

Constraints and non-goals:

- Do not add a new MIR terminator unless existing `TryCall` cannot represent the
  control flow.
- Do not change grammar or semantic typing.
- Do not change Rust codegen.
- Do not broaden conversion semantics beyond the current stepper conversion
  failures.

## Repo-Aware Baseline

Evidence:

- `tutumDirect` in `crates/exempla/corpus/conversio/fallibilis.fab` currently
  lowers a direct handled conversion as an ordinary runtime conversion statement
  followed by `return`, leaving the handler block disconnected.
- Existing MIR already supports failable synthetic functions and `TryCall`.
- Phase 004 made the stepper propagate conversion failure from a failable
  function as an alternate return.

## Stage Graph

1. Extend handler lowering context with the handler error type.
2. Lower direct handled runtime conversion through a synthetic failable helper
   function.
3. Emit a `TryCall` from the original expression site to the helper.
4. Add a focused stepper regression for `fac { redde v ↦ instans } cape err`.
5. Run representative `fallibilis` and script e2e validation.

## Implementation Work

Primary write surfaces:

- `crates/radix/src/mir/lower.rs`
- `crates/radix/src/mir/lower/control.rs`
- `crates/radix/src/mir/lower/runtime.rs`
- `crates/radix/src/mir/stepper/mod.rs`
- `crates/radix/src/mir/stepper_test.rs`
- `docs/factory/faber-script-e2e-hardening/ledger.md`

Out of scope:

- S-expression/Wasm/LLVM failable backend support for the new synthetic helper
  shape beyond recording current status.
- `conversio/instans.fab` precision expectation cleanup.

## Checkpoints And Gates

Checkpoint target:

- `cargo run -p faber-cli -- run
  crates/exempla/corpus/conversio/fallibilis.fab` passes or moves to a new,
  recorded blocker after `tutumDirect`.

Gate expectations:

- Focused direct handled conversion stepper test passes.
- `radix mir` shows `tutumDirect` using `try_call` for the direct conversion.
- The ignored script e2e gate is run and changed failure status is recorded.

Release checkpoint:

- Deferred. This is internal script/MIR hardening.

## Validation

Planned commands:

```bash
timeout 120 cargo test -p radix stepper_handles_direct_conversio_inside_fac_cape
cargo run -p radix --bin radix -- mir crates/exempla/corpus/conversio/fallibilis.fab
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
