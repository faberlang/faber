# Proba Companion Tests — Local Import Resolution + Test-Process Coverage

**Status**: planned — pre-implementation; discovered 2026-08-10 via housekeeping run on norma (test-boundary stage)
**Created**: 2026-08-10
**Target repo**: `/Users/ianzepp/work/faberlang/faber`
**Factory artifact dir**: `docs/factory/proba-companion-tests/`

## Summary

Make the Faber test process capable of test-boundary hygiene: colocated
`.proba` companion tests must be able to import their module, and the proba
runner must be exercisable on a library root — so inline test blocks can be
moved out of production `.fab` files losslessly, and the testing process itself
gains real coverage.

## Problem

- Inline `probandum { … adfirma … }` blocks inside production `.fab` files run
  fine in `faber test <file>` single-file mode.
- The norma test convention is colocated `src/**/*.proba` (discovered via
  `include_proba`), but a companion `.proba` that imports its module
  (`importa ex "./model"`) fails in proba/package context with
  `FaberScript unsupported: provider sym#12/13`.
- Therefore moving inline tests out of production files is **not lossless**
  today — there is no extraction path that keeps the proba cases green.
- `faber test` package mode expects a `main.fab` app package; a library root
  like `norma/` (no `faber.toml`) cannot be exercised directly — the canonical
  path is the radix ladder's `scripta/proba-canary.list`.
- The proba runner itself has thin test coverage — the testing process is
  largely untested.

## Goals

- Resolve local relative `importa ex "./…"` from `.proba` companions in
  proba/package context (the `provider sym#12/13` failure).
- Allow `faber test` on a library root without `main.fab` (or a documented
  equivalent entry — e.g. proba canary list support).
- Prove lossless extraction: proba case count before == after, all cases green.
- Add coverage for the proba runner / stepper itself (test the test process).

## Non-goals

- The `faber script` / interpret host surface (separate: `faber-script-e2e-hardening`).
- The `'la'`-locale `faber format` gap (separate goal: `faber-format-locale-la`).
- Fixing norma's `json` module compile-gap (SEM016, tracked as `P2 soft_gate` in norma).

## Ground Truth Researched

- Housekeeping run `wf_019feb5c7dbc7760b1e6b360f7ca1bb9`, child transcript
  `~/.grok/sessions/…/subagents/019feb5f-7e65-7180-97b2-9f2d0c94b1f1/chat_history.jsonl`:
  the full investigation trail — single-file mode passes inline tests; companion
  `.proba` import fails; `FABER_LIBRARY_HOME` explored; resolver traced.
- `faber/src/package/compile.rs` (`resolve_import`): `importa ex "./model"` resolves
  against the library home first and fails for local-relative specs in proba context.
- `faber/src/library.rs` (`LibraryResolver`): the resolver used by package compile.
- `faber/src/commands/test_test.rs` + `src/package/source_files.rs`: `include_proba`
  discovery; proba files are not importable as modules.
- `radix/scripta/test` stage 3 (proba): runs `faber test` over `scripta/proba-canary.list`
  (paths relative to the container root) — the existing library-root test path.
- Concrete target: `norma/src/model.fab:54` inline `probandum` block (the extraction case).

## Reference Packet

Before editing, inspect:

- `faber/src/commands/test.rs` and `src/commands/test_test.rs`: current runner + tests.
- `faber/src/package/compile.rs` (`resolve_import`): the failing resolution path.
- `faber/src/package/source_files.rs`: `include_proba` discovery rules.
- `radix/scripta/proba-canary.list` + `radix/scripta/test`: the library-root proba path.
- `norma/src/model.fab` + `norma/src/mathesis.proba`: the extraction target and the
  colocated-convention exemplar.

## Constraints And Invariants

- Proba cases run on the MIR stepper — no Cargo / rustc on the package (`faber test` contract).
- `faber test` must never silently drop or skip proba cases (count loss is a failure).
- Norma convention stands: production `.fab` in `src/`, tests colocated as `src/**/*.proba`.
- No new manifest requirement for library roots unless paired with a documented fallback.

## Supporting Skills

- `faber`: compiler/package mechanics grounding (authoritative on `faber test` behavior).
- `housekeeping` Step 2 (test-boundary): the consumer that needs lossless extraction.

## Implementation Shape

- Phase 1: minimal failing repro in `faber/src/commands/test_test.rs` — a temp
  package with a `.proba` companion importing its module.
- Phase 2: fix local-relative `importa ex "./…"` resolution from proba companions.
- Phase 3: library-root `faber test` (no `main.fab`) or documented canary-list entry.
- Phase 4: extraction proof on `norma/src/model.fab` (count preserved) + runner coverage.

## Release Posture

Decision: defer release — no publication at the end of this goal.

- Land as normal commits; release only if the radix/faber release protocol picks it up.

## Exit Strategy

Decision: not included.

## Acceptance Criteria

- A `.proba` companion can `importa ex "./model"` and run its cases green.
- `faber test` on a library root runs the colocated proba cases (count exact).
- Extracting `model.fab`'s inline `probandum` to a companion keeps the proba count
  identical and all cases green.
- New runner/stepper tests cover the resolution path (the process is tested).

## Validation

- `cargo test -p faber` / `cargo test -p faber --lib commands::test` should stay green.
- Manual: temp package with companion `.proba` importing its module — `faber test` passes.
- Review check: extraction diff on `model.fab` moves bytes, never edits test bodies.

## Open Questions

- Should local-relative imports resolve in proba context, or should proba
  companions be implicitly bound to their sibling module (no explicit `importa`)?
- Does the canary-list path stay the canonical library-root entry, or does
  `faber test` gain a manifest-less library mode?

## Stop Conditions

- Stop if changing proba semantics would break the MIR-stepper contract.
- Stop if a case-count change (drop or skip) is required to go green.
