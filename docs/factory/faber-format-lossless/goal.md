# Faber Lossless Format — Factory Goal (Follow-on)

**Status**: vision / not started  
**Created**: 2026-06-28  
**Target repo**: `/Users/ianzepp/work/faberlang/faber`
**Factory artifact dir**: `docs/factory/faber-format-lossless/`
**Parent track**: sibling radix [`docs/factory/faber-format/goal.md`](../../../../radix/docs/factory/faber-format/goal.md) — canonical HIR formatter ships first (`radix::forma`)

---

## Summary

Deliver a **trivia-preserving** Faber formatter with `rustfmt`-grade behavior:
comments, blank lines, and string delimiters survive; only layout (indentation,
line breaks, inter-token whitespace) changes.

This is **not** a replacement for the HIR canonical emitter. It is the path to
author-facing format when comment preservation is a hard requirement.

---

## Problem

The HIR-backed canonical emitter (`forma::canonical`; legacy `codegen/faber` in
radix until removed) cannot preserve comments or frontmatter
because lowering discards trivia. Projects that treat formatting like `cargo fmt`
but **also** require comments in diffs need a token- or parse-tree-aware formatter.

Phase 0 of [`faber-format`](../faber-format/goal.md) should measure comment density
on exempla and stdlib to estimate ROI.

---

## Proposed pipeline

```text
Source
  → Lexer (token stream + trivia spans)
  → Parse tree (structure only, or lossless CST)
  → Layout engine (indent, break, align)
  → Source text out
```

No HIR. No typecheck required for layout-only rules (optional `check` mode for CI).

---

## Crate placement (likely)

Lossless layout lives in the same formatter package as canonical emit:

```text
crates/forma/        — formatter crate (name locked; distinct from forma literal)
  canonical.rs       — HIR-backed emit in forma (not radix codegen)
  lossless.rs        — token layout engine (this goal)
```

The **forma** crate name is shared with the language's `` `...` `` captured-string
literal — disambiguate in docs as **forma crate** vs **forma literal**. Phase 0 of
the parent goal records the split module layout inside `crates/forma`.

---

## Goals

1. Preserve comments (line, block, trailing) and meaningful blank lines.
2. Preserve string delimiters: `"`, `«»`, `` ` `` forma, `'ascii'`, `|octeti|`.
3. Idempotent layout on a lossless corpus fixture set.
4. `faber format --lossless` or separate `faber fmt` subcommand — **TBD**; do not
   alias canonical HIR format.

---

## Non-goals

- Changing semantic spellings (`f32` sugar expansion) — that stays on the canonical path.
- Replacing `emit -t faber` round-trip validation.
- Package compilation or codegen.

---

## Dependency

**Start only after** [`faber-format`](../faber-format/goal.md) Stage 0 measures
trivia and [`faber-format`](../faber-format/goal.md) Stage 7 ships the canonical
command — otherwise two format stories launch without a stable default.

---

## Acceptance criteria (draft)

- Fixture corpus of ≥20 `.fab` files with comments round-trip layout idempotently.
- `faber format --lossless --check` CI mode exists.
- Documented distinction from canonical format in user-facing help.

---

## Handoff readiness

**Not ready for factory** — vision only. Promote when parent goal Stage 0 trivia
measurement completes and a staffing decision favors comment preservation over
canonical-only format.