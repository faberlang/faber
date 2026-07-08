# Faber Polish Command Factory Goal

**Status**: planning
**Created**: 2026-06-26
**Target repo**: `/Users/ianzepp/work/faberlang/faber`
**Factory Artifact Dir**: `docs/factory/faber-polish/`
**Target Seams**:
- Lint engine (private radix): `crates/radix/src/semantic/passes/lint.rs`
- Diagnostic model (private radix): `crates/radix/src/semantic/error.rs`
- Semantic pipeline (private radix): `crates/radix/src/semantic/mod.rs`
- CLI surface (this repo): `src/cli/mod.rs`, `src/commands/mod.rs`
- Tool commands (private radix): `crates/radix/src/tool/`

## Objective

Add a `faber polish` command that emits **idiom-level advisory suggestions**
the way Clippy does for Rust: not correctness errors, but "prefer this form"
recommendations grounded in Faber's canonical language surface.

Today `faber check` already runs a lint pass
(`passes::lint::lint`, wired at `crates/radix/src/semantic/mod.rs:177`), but
that pass is correctness-adjacent only: unused bindings, unreachable code,
redundant casts, explicit `ignotum`, shadowing. It produces `(WarningKind,
String, Span)` triples with **no suggested replacement**. Polish closes that
gap: advisory diagnostics that carry machine-applicable source rewrites where
possible.

## Non-Goals

- Polish must never block compilation. It runs strictly advisory.
- Polish does not replace `check`. It composes with it.
- Polish is not a formatter. `faber format` is tracked separately in
  `docs/factory/faber-format/`.
- Polish does not weaken existing lint policy. The current `lint.rs` rules stay
  severity-promoted as today; polish adds a parallel advisory lane.

## Existing Capability (Do Not Rebuild)

- `LintContext` in `passes/lint.rs` is already an `HirVisitor` with scope
  tracking, `DefId` usage sets, and read-only `TypeTable` access. It is the
  correct template for typed-HIR idiom rules.
- `WarningKind` (`crates/radix/src/semantic/error.rs:140`) is the existing
  warning catalog. Polish rules should extend this enum, not invent a parallel
  one.
- `SemanticErrorKind::Warning` is the existing carrier. Suggestions need an
  extension to this model, not a new result type (see Design Decision A).
- The `Command` enum (`crates/faber/src/cli/mod.rs:24`) and dispatch
  (`commands/mod.rs`) make a new variant mechanical to add.

## Design Decisions

### A. Diagnostic model must carry suggested fixes

Current `LintContext` pushes `(WarningKind, String, Span)`. Polish needs an
optional `Suggestion { span, replacement, label, applicability }`. Two
acceptable shapes:

1. Extend `SemanticError` (or `SemanticErrorKind::Warning`) with an optional
   `Vec<Suggestion>` field. Single carrier, render path extends naturally.
2. Add a parallel `LintResult` returning `Vec<Diagnostic>` where `Diagnostic`
   carries zero-or-more suggestions. Cleaner separation but a new type.

Recommend option 1 for the first cut: smallest blast radius, reuses the
existing render path, and suggestions stay advisory-only through
`applicability` (MachineApplicable vs DisplayOnly).

### B. Polish rules split across parse-time and HIR-time

The repo's own "Common LLM Failure Modes" list (root `AGENTS.md`) is the
candidate rule set. Critically, **half of these are parse-time** — the parser
already rejects them as hard errors, so they cannot become HIR lints. Polish
must decide its scope:

**HIR-time idiom rules** (fit `LintContext` cleanly):
- Block ending in single `redde x` → suggest `∴ redde x`
- Empty collection literal missing `vacua` → suggest `vacua` form
- String concatenation / target interpolation → suggest `"§"(arg)` template
  application
- Postfix `⊕`/`⊖` used in expression position → suggest statement form

**Parse-time idiom rules** (cannot run on HIR):
- `name: Type` → suggest `Type name`
- `x = …` → suggest `x ← …`
- Removed compound-assignment (`x ⊕ 2`, `x ⊛ 3`) → suggest `x ← x + …`
- Re-introduced banned aliases (`qua`, `innatum`, `novum`) → suggest canonical
- `tempta`/`demum` → suggest `fac { } cape err { }`

Recommendation: **ship HIR-time rules first** as `polish` v1. Parse-time rules
require either a token-layer advisory pass (lexer-layer) or recovery in the
parser, both of which are larger. Document parse-time rules as a v2 family in
the ledger, not a v1 blocker.

### C. Advisory routing around warnings-as-errors

Today `lint.rs` returns warnings through the error vector and surrounding
policy (`config.lint`) may promote warnings. Polish rules must route around
that promotion. Two options:

1. A `LintProfile::Polish` that only emits `Suggestion`-kind diagnostics and
   never `WarningKind`.
2. A new `passes::polish::polish(&hir, &resolver, &types) -> Vec<Suggestion>`
   that does not touch the error vector at all.

Recommend option 2: a separate pass is easier to reason about, runs only when
`faber polish` is invoked, and cannot accidentally affect `check` severity.

## Proposed Command Surface

```
faber polish [INPUT]          # report idiom suggestions (advisory only)
faber polish --fix [INPUT]    # apply machine-applicable suggestions in place
faber polish --list           # list enabled rules
faber polish --json [INPUT]   # machine-readable output for editor integration
```

`--fix` is optional for v1. Ship report mode first; `--fix` is a bounded
source-rewrite layer (sort suggestions by span descending, splice
`replacement`) that only makes sense once the rule set is stable.

## Candidate Phase Families

### Phase 1: Diagnostic model extension

- Add `Suggestion { span, replacement, label, applicability }` to
  `crates/radix/src/semantic/error.rs`.
- Extend render path in `crates/radix/src/diagnostics/render.rs` to show
  suggestions.
- No behavior change to `check`. Purely additive.

Checkpoint: render tests show a suggestion for a synthetic diagnostic.

### Phase 2: Polish pass skeleton + command wiring

- Add `passes::polish::polish(...)` as an empty stub returning `Vec<Suggestion>`.
- Add `tool::cmd_polish` and a `Command::Polish` variant in faber.
- Wire dispatch in `commands/mod.rs`, mirroring `Check`.
- Report mode only (no `--fix`).

Checkpoint: `faber polish file.fab` runs and prints zero suggestions on clean
code without erroring.

### Phase 3: HIR-time idiom rules

- Implement the HIR-time rules from Design Decision B inside `passes/polish.rs`
  using the `LintContext` template.
- Each rule gets a focused test in `polish_test.rs`.

Checkpoint: representative exemplars trigger the expected suggestions; clean
exemplars trigger none.

### Phase 4: `--fix` application (optional)

- Source-rewrite layer over the suggestion list.
- Only apply `MachineApplicable` suggestions.
- Refuse to apply overlapping suggestions; error loudly.

Checkpoint: `faber polish --fix` rewrites a fixture file and a follow-up
`faber polish` reports zero suggestions.

### Phase 5 (deferred): Parse-time rule family

- Token-layer advisory pass for the parse-time rules.
- Tracked separately; not a v1 blocker.

## Validation Commands

```bash
cargo test -p radix semantic::passes::polish -- --nocapture
cargo test -p radix diagnostics::render -- --nocapture
cargo test -p faber cli -- --nocapture
cargo test -p radix
./scripta/lint
```

## Completion Criteria (v1)

- `faber polish [INPUT]` reports HIR-time idiom suggestions without blocking
  compilation.
- Suggestions render with span, label, and (where applicable) replacement.
- No regression in `cargo test -p radix` or `cargo test -p faber`.
- Parse-time rules are documented in the ledger as a deferred family with clear
  next-phase recommendations, not silently dropped.

## Open Questions

- Should polish rules be individually toggleable (`--allow`, `--deny`,
  `--warn` per rule id, Clippy-style)? Recommend deferring until the rule set
  is large enough to warrant it.
- Should `faber check` optionally surface polish suggestions as warnings under
  a flag? Recommend no — keep `check` severity-only and let `polish` be a
  separate advisory invocation.
