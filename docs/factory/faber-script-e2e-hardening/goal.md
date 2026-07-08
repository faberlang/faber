# Faber Script E2E Hardening - Factory Goal

**Status**: paused (2026-07-04) — factory work on hold; harness and floors unchanged
**Created**: 2026-06-29
**Target repo**: `/Users/ianzepp/work/faberlang/faber`
**Factory artifact dir**: `docs/factory/faber-script-e2e-hardening/`
**Primary surfaces**: `crates/exempla/src/exempla_e2e/script.rs`, `crates/radix/src/mir/`, `crates/radix/src/mir/stepper/`, `crates/radix/src/driver/`
**MIR-adjacent codegen surfaces**: S-expression, Wasm, and LLVM codegen paths

**Predecessor factory** (complete):

- [`../faber-script-kernel/goal.md`](../faber-script-kernel/goal.md) — v1 `faber:*`
  host kernel (`solum`, `processus`, `aleator`, `json`). This factory owns
  language-surface MIR/stepper debt; kernel import dispatch is not in scope.

**Related factories**:
- [`../mir-lowering/goal.md`](../mir-lowering/goal.md)
- [`../exempla-e2e-speed/plan.md`](../exempla-e2e-speed/plan.md)
- [`../backend-smoke-check/goal.md`](../backend-smoke-check/goal.md)

---

## Objective

Turn the ignored script end-to-end harness into the fast ratchet gate for
reasonable MIR/script execution surfaces represented in exempla.

The harness is:

```bash
timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored --nocapture
```

It is valuable because it is fast, mostly in-memory, and exercises the real
compiler path through analysis, HIR-to-MIR lowering, MIR validation, and the
script stepper.

## Reference implementation

This is primarily a surface implementation and bug-fix loop. Use the Rust
backend as the behavioral reference because it is the most mature executable
implementation.

When an exemplar already works through Rust emission/build/run, script-mode MIR
should usually converge on that behavior. If Rust behavior and MIR behavior
disagree, treat the mismatch as evidence to investigate before changing language
semantics.

Updating MIR is allowed, but only to implement existing surfaces faithfully or
fix concrete bugs exposed by exempla. This factory does not own broad MIR
architecture decisions, new intermediate representations, or sweeping lowering
redesigns.

## MIR-adjacent codegen scope

Each phase that implements a script/MIR surface must also inspect the
MIR-adjacent codegen paths for S-expression, Wasm, and LLVM.

If one of those backends already has a matching implementation pattern for the
newly supported MIR surface, the phase should add equivalent support and focused
tests. If a backend does not have a matching architecture or would require
designing a broad new lowering/codegen model, record that as deferred instead of
expanding the phase.

This scope is not a request to make S-expression, Wasm, or LLVM fully complete.
It is a consistency rule: when MIR gains support for an existing surface, the
MIR-boundary codegen backends should not be left with avoidable gaps for the
same surface.

"Focused tests" here means targeted unit tests, small emitter snapshots, or one
representative `radix emit -t <target> <exemplum>` check. It does not mean
running full S-expression, Wasm, or LLVM end-to-end harnesses during each
surface phase.

## Current baseline

Last green run (ledger Phase 034, 2026-06-29):

```text
Script e2e exempla: 203/264 exempla files pass end-to-end
  stepper ran: 203/264
  output checked: 47/264
  floors: run>=203, output_checked>=47
  unsupported-mir bucket: empty
```

Post-push note (2026-06-30): corpus or type-policy changes may introduce
**unclassified** failures (for example `stdlib-nativum/*` frontend mismatches)
that trip the run floor before classification catches up. Treat that as harness
maintenance work in this factory, not a kernel regression.

The harness fails when files outside the expected-failure list fail under script
mode. That is useful signal, not a reason to disable the gate.

## Desired end state

All exempla that reasonably represent script-executable Faber surfaces should
pass under `exempla_script_e2e`.

Failures that are not script-mode obligations should be classified explicitly
with durable reason buckets instead of living in an undifferentiated path list.

The final harness should answer three questions clearly:

1. Which exempla run successfully through MIR script execution?
2. Which exempla are intentionally outside script mode, and why?
3. Which remaining failures represent real MIR lowering or stepper debt?

## Architectural invariants

1. **MIR first**: Fix missing or incorrect MIR lowering and stepper behavior
   when the source surface is reasonable for scripts.
2. **Rust is the reference**: Use the Rust backend's mature behavior to guide
   surface semantics when implementing script-mode MIR support.
3. **No broad MIR architecture decisions**: Keep MIR changes constrained to
   implementing existing surfaces and fixing concrete bugs from the harness.
   Escalate if a phase appears to require a new MIR architecture.
4. **MIR-boundary codegen consistency**: For each implemented MIR surface,
   inspect S-expression, Wasm, and LLVM codegen. Add matching support when the
   backend has an established pattern; defer only when support would require
   broad backend design.
5. **No policy weakening**: Do not raise floors downward, hide failures, broaden
   expected failures, or skip directories merely to get green tests.
6. **Classified exclusions only**: Non-script surfaces may be excluded only with
   a named reason bucket and a short explanation.
7. **One failure category per phase**: Each factory phase chooses one coherent
   failure class, fixes it end to end, verifies it, polishes, and commits.
8. **Exempla are the gate**: Focused unit tests may support a fix, but the
   script e2e harness is the phase gate unless the chosen category is explicitly
   classification-only.
9. **Release gate stays release-level**: Do not run `./scripta/release --dry-run`
   or full release validation as the normal inner loop for this factory.

## Phase loop

Each factory phase follows this loop:

1. Run the script e2e harness and capture the live failure list.
2. Pick one failure category, preferring missing or incorrect MIR lowering.
3. Write a delivery spec for that category under this artifact directory.
4. Compare the failing surface against Rust backend behavior when a Rust
   executable path exists.
5. Implement the fix or classification.
6. Inspect S-expression, Wasm, and LLVM codegen for matching MIR-surface support;
   implement matching support when the backend pattern already exists.
7. Add focused tests when a small unit/regression/emitter test would make the
   behavior easier to preserve; do not run full backend e2e sweeps per surface.
8. Run targeted validation for the changed surface.
9. Run the script e2e harness.
10. Polish changed source files for correctness, cleanliness, and maintainability.
11. Commit the completed phase.
12. Update the ledger/checkpoint for the next category.

## Good first categories

Prefer categories that increase real script coverage:

- `unsupported MIR lowering: method call before runtime/provider MIR lowering`
- `unsupported MIR lowering: path that does not resolve to a local value`
- numeric operand mismatches in script-stepper execution
- optional-chain or indexing crashes
- stepper aborts or assertions, especially `omitte/omitte.fab`
- stdout mismatches where stepper output or `.expected` files are stale
- unsupported runtime conversions that should be script-safe

## Classification buckets

Expected failures should move from a flat string list to structured buckets.
Initial buckets:

| Bucket | Meaning |
| --- | --- |
| `frontend-negative` | The exemplar intentionally demonstrates an invalid program or diagnostic. |
| `package-only` | The exemplar requires package/build behavior, not script execution. |
| `norma-import` | The exemplar imports `norma:*`; script mode intentionally uses `faber:*` kernel imports instead. |
| `cli-program` | The exemplar depends on CLI program-specific lowering or operand parsing. |
| `mir-backed-target-only` | The exemplar is valid only for MIR-backed emit targets, not script/default Rust analysis. |
| `externa-linkage` | The exemplar declares linker/runtime-provided symbols. |
| `capability-stream` | The exemplar uses `ad`, `SermoOpen`, or frame-gateway surfaces not yet owned by script mode. |
| `no-entry-reference` | The file is a reference/type example with no runnable `incipit`. |
| `unsupported-mir` | Real MIR lowering/stepper debt that should eventually be fixed. |

The `unsupported-mir` bucket should shrink over time. Other buckets may remain
if they accurately describe non-script surfaces.

## Non-goals

- Making `norma:*` imports work in script mode as part of this factory.
- Implementing package build behavior through `faber run`.
- Implementing CLI operand/program lowering unless selected as its own future
  script-mode design.
- Implementing `externa` linkage in the in-memory stepper.
- Implementing `ad` / frame-gateway stream semantics unless a separate phase
  explicitly chooses that runtime model.
- Treating every documentation/reference exemplar as required to have an
  executable `incipit`.
- Using this factory to redesign MIR architecture, invent new language
  semantics, or replace the existing Rust-backed behavior model.
- Completing all S-expression, Wasm, or LLVM backend gaps unrelated to the
  selected MIR surface.
- Designing broad new S-expression, Wasm, or LLVM architecture inside a surface
  implementation phase.
- Running S-expression, Wasm, or LLVM end-to-end harnesses as routine
  per-surface validation.
- Running full release validation after every small phase.

## Success criteria

- [ ] `exempla_script_e2e` has structured expected-failure classifications.
- [ ] Every remaining expected failure has a reason bucket.
- [ ] All unclassified failures are treated as real work or explicitly resolved.
- [ ] Script-reasonable MIR surfaces represented in exempla pass the harness.
- [ ] Each implemented MIR surface records S-expression, Wasm, and LLVM status:
      supported with tests, not applicable, or deferred with reason.
- [ ] `unsupported-mir` count monotonically decreases across implementation
      phases unless a phase records a justified reclassification.
- [ ] The harness remains fast enough for repeated factory use.

## Validation

Primary gate:

```bash
timeout 300 cargo test -p exempla exempla_script_e2e -- --ignored --nocapture
```

Common supporting checks:

```bash
timeout 120 cargo test -p radix <focused-filter>
timeout 120 cargo test -p exempla <focused-filter>
cargo fmt --all -- --check
git diff --check
```

When a phase changes S-expression, Wasm, or LLVM codegen, add the matching
focused backend check for that surface. Prefer a targeted unit test, emitter
snapshot, or `cargo run -p radix --bin radix -- emit -t <target> <exemplum>`
check. Do not run full S-expression, Wasm, or LLVM end-to-end harnesses as
routine inner-loop validation.

Do not pass multiple test filters to one `cargo test` command.

## First milestone

Create a classified failure ledger from the current harness output, then choose
the first MIR-owned implementation category. The recommended first category is
one of:

1. method-call lowering gaps,
2. path resolution gaps,
3. stepper abort/assertion behavior.

Do not start by absorbing all failures into expected buckets. Classification is
there to make the work honest and selectable, not to excuse missing MIR support.

## Stop conditions

Pause and report before continuing if:

- a phase requires changing language semantics rather than implementing existing
  MIR/runtime behavior;
- a phase appears to require broad MIR architectural design rather than a
  bounded surface implementation or bug fix;
- matching S-expression, Wasm, or LLVM support would require broad backend
  architecture rather than following an existing codegen pattern;
- a category would require broad `norma:*`, package, CLI, `externa`, or `ad`
  support contrary to the non-goals;
- the harness becomes materially slower or stops being useful for repeated
  factory loops;
- unexpected dirty files appear outside the active phase scope.
