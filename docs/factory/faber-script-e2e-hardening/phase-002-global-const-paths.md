# Phase 002 - Global Const Path MIR Lowering

## Interpreted Unit

Fix the `unsupported MIR lowering: path that does not resolve to a local value`
case where a function body references a top-level `fixum`/const declaration.
Use `conversio/fallibilis.fab` as the representative script e2e exemplar.

## Normalized Spec

Functional requirements:

- Function-body MIR lowering can resolve top-level constant paths.
- Top-level constants stay immutable and are not treated as assignable locals.
- Existing entry-prefix const behavior remains intact for `incipit` blocks.
- The representative `conversio/fallibilis.fab` path-resolution errors are
  removed from MIR lowering.
- Do not broaden the phase into runtime method-call lowering, tensor lowering,
  package imports, or full global storage architecture.

Constraints and non-goals:

- No language semantics change; Rust emission already treats the source as a
  top-level constant.
- No floor weakening and no expected-failure absorption.
- MIR-boundary backend inspection is required for any new MIR shape. If the
  implementation reuses existing local assignment/path MIR, S-expression, Wasm,
  and LLVM need only be checked for existing local support.

## Repo-Aware Baseline

Evidence:

- `crates/exempla/corpus/conversio/fallibilis.fab` defines
  `fixum instans epochZero` and references it inside multiple functions.
- `cargo run -p radix --bin radix -- emit -t rust
  crates/exempla/corpus/conversio/fallibilis.fab` emits `pub const epochZero`
  and uses it from functions.
- `crates/radix/src/mir/lower.rs` currently lowers top-level consts only as
  entry-prefix locals in `lower_entry`.
- `lower_path` currently rejects non-local paths unless they are unit enum
  variants.

## Stage Graph

1. Add top-level const metadata to the shared MIR lowering context.
2. Teach `lower_path` to lazily materialize a referenced top-level const into
   the current function builder.
3. Add a targeted MIR lowering regression test.
4. Inspect S-expression, Wasm, and LLVM MIR consumers for compatibility with the
   emitted MIR shape.
5. Run focused MIR and script checks.

## Implementation Work

Primary write surfaces:

- `crates/radix/src/mir/lower/context.rs`
- `crates/radix/src/mir/lower.rs`
- `crates/radix/src/mir/lower_test.rs`
- `docs/factory/faber-script-e2e-hardening/ledger.md`

Out of scope:

- `crates/exempla/src/exempla_e2e/script.rs` bucket list changes.
- Runtime method-call support for lista/tensor/numeric intrinsic families.
- Stepper support for `instans` conversion unless exposed as the next failure
  after the path-resolution fix.

## Checkpoints And Gates

Checkpoint target:

- The representative exemplar no longer reports
  `path that does not resolve to a local value`.

Gate expectations:

- Focused MIR regression passes.
- `radix check` or `radix mir` on `conversio/fallibilis.fab` proves the path
  diagnostic is gone.
- The script e2e harness still fails on remaining unclassified/unsupported
  surfaces unless this phase naturally moves the representative further.

Release checkpoint:

- Deferred. This is internal MIR hardening.

## Validation

Planned commands:

```bash
timeout 120 cargo test -p radix <focused-filter>
cargo run -p radix --bin radix -- mir crates/exempla/corpus/conversio/fallibilis.fab
timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored --nocapture
cargo fmt --all -- --check
git diff --check
```

## Companion Skill Plan

- `factory`: phase execution and checkpoint discipline.
- `faber`: grammar/exempla and compiler-boundary authority.

## Open Questions

No blocking open questions.

