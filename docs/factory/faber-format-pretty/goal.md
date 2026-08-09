# Faber Format Pretty Policy — Factory Goal

**Status**: planned — pre-implementation; heads' synthesis complete, goal-forge / delivery lowering in flight (planner-2)
**Created**: 2026-08-09
**Target repo**: `faber` (product surface; goal inventory lives here) + `radix` (`radix::forma` engine)
**Factory artifact dir**: `docs/factory/faber-format-pretty/`
**Source**: operator formatter discussion 2026-08-09; head-cpo + head-cxo advisory reports, synthesized by Mind

---

## Summary

Make `faber format` an opinionated, deterministic, prettier/rustfmt-class
source formatter — via **named Faber-owned policies**, not a Prettier-style
option matrix:

- `normalise-v1` — the exact current normalizer behavior, promoted to an
  explicit compatibility policy. Stays the default until the corpus and user
  packages have deliberately migrated.
- `pretty-v1` — the new pretty layout policy (4-space indent, 100-column soft
  width, structural block expansion, delimiter-aware wrapping, bounded
  blank-line normalization, comment preservation without reflow).

Policy is selected in `[format]` of the product manifest (`faber.toml`). No
second config language (`forma.toml` is rejected). Small CLI additions:
`--write` (explicit in-place spelling) and `--stdin` (stdin → stdout);
`--config` remains deferred and, when it lands, must load the same `[format]`
schema. Locale stays a separate input with the existing precedence
(CLI → frontmatter → package `[reader] locale` → en).

## Problem

- `faber format` exists and is package-wide capable (no-arg = whole package;
  dirs recurse; `--check`; `--stdout`; `--locale`), but it is a **normalizer,
  not a prettifier**: it swaps dialect keywords, whitespace, and trailing
  newline, and preserves the source's line structure byte-for-byte otherwise.
  Verified: `--locale en --stdout` on `triga/src/face.fab` is byte-identical
  to the input — collapsed one-line records and 15-line single-line calls are
  formatter-stable.
- `FormatOptions` is a single `canonical: bool`. No line width, no indent
  rules, no expansion, no wrapping.
- No manifest format config: no `[format]` / `[forma]` section in
  `faber.toml`; the `--config forma.toml` flag's schema is deferred.
- AGENTS.md direction: "Faber should own user-facing formatting policy, with
  rule slugs/options rather than a single all-or-nothing surface enum."
- The exempla corpus asserts **byte-exact** output (`.expected` files) —
  formatter rule changes ripple into fixtures. Rollout must be controlled.
- Operator motivation: example source (Triga, stdlib exempla) is structurally
  less readable than it could be (collapsed records, over-long single lines).

## Decision — heads synthesis (answer to the operator's question)

Both heads agree on the core architecture; this is the merged product shape.

| Question | Decision |
| --- | --- |
| What should it DO? | Opinionated deterministic layout pass: stable blocks/indentation, width-aware wrapping at syntax boundaries, fit-if-possible calls/records, bounded blank lines, comment preservation. No semantic rewriting, no alignment. |
| What flags? | Keep existing surface; add `--write` + `--stdin`; `--config` later (same schema). No `--line-width` / `--indent-width` / `--trailing-comma` / per-rule toggles in v1. |
| What config surface? | `[format] policy = "pretty-v1"` in `faber.toml`. Default `normalise-v1` until migration. No `[forma]`, no `forma.toml`. |
| Output shape? | 4-space indent, 100-col soft width, brace-attached blocks, `si`/`sin`/`secus` chains as one chain, one-arg-per-line when wrapped, no field alignment, ≤1 blank line between top-level decls, author blank lines inside bodies preserved. |
| Rollout? | Golden corpus first → one migration switch → advisory `--check` → one dedicated rebaseline commit → default flip after migration window. |
| Complexity line? | No alignment, no config sprawl, no import sorting, no comment reflow, no quote/trailing-comma policy, no per-file rule lists, no environment-dependent behavior, unknown slugs fail clearly. |

Divergence noted for lowering: head-cpo proposed a temporary `--pretty`
migration switch; head-cxo preferred policy selection via the manifest only.
Merged recommendation: a single `--policy <slug>` CLI override (mirrors the
manifest key, serves both as migration switch and steady-state override; one
switch, not a matrix). Exact spelling is a lowering decision.

## Rule set (`pretty-v1`), prioritized

**P0 — blocks and indentation.** One statement per line inside a block; four
spaces per nesting level; no tabs; braces attached to the owning construct;
`si … sin … secus` chains rendered as one readable chain. Compact
one-statement forms stay compact when they fit (`si code ≡ 1 ergo redde
prima()`); if a compact arm exceeds the width, expand it into a block. Empty
blocks stay compact. Indentation is structural, not configurable in v1.

**P0 — width-aware wrapping.** 100-column soft limit (a wrapping trigger, not
an absolute promise). Break only at syntactic boundaries: function parameters,
call arguments, record/constructor fields, already-safe grouping. Never split
strings, URLs, identifiers, comment text, or operators in ways that obscure
precedence. A line long because of an unbreakable string stays long.

**P0 — calls, parameters, records.** Fit if possible, expand when needed.
Short calls/records stay inline; wrapped lists go one per line; nested records
one field per line. No trailing commas unless the grammar explicitly supports
them (currently young — do not add punctuation churn).

**P0 — blank lines.** One blank line between top-level declarations; collapse
repeated runs to one; preserve the author's blank line between logical
sections inside a body; never manufacture blank lines between statements.

**P0 — comments and frontmatter.** Re-indent comments with their surrounding
syntax; normalize comment-marker spacing only; preserve comment text and line
breaks byte-for-byte; no prose reflow. Preserve `+++` frontmatter as
frontmatter; never format TOML metadata with Faber layout rules.

**P1 — binding and operator spacing.** Normalize spacing around `←`, `→`,
`=`, commas, and delimiters. **Never vertically align bindings, fields,
parameters, or match arms** — alignment is the highest-churn class of
formatting behavior.

**Record risk — line-sensitive grammar.** Faber has explicit line-sensitive
forms (compact `ergo` bodies, annotation sugar). The formatter must never
flatten source merely because tokens could fit. If the printer cannot preserve
a construct confidently: leave the file unchanged, report the path/location,
and fail in `--check` mode. A partial rewrite is worse than an unformatted
file.

## Flags and options surface

CLI (steady state):

```text
faber format [PATH...]      # no path = whole package; dirs recurse; files format
faber format --check        # never write; nonzero exit if any file would change; report paths
faber format --write        # explicit spelling of the in-place default (scripts/editors)
faber format --stdout FILE  # exactly one source file → stdout
faber format --stdin        # read one source document from stdin → stdout
faber format --locale <X>   # dialect selection; precedence unchanged
faber format --policy <slug>  # optional; overrides manifest policy (migration + override)
faber format --config <PATH>  # deferred; must load the same faber.toml [format] schema
```

Combo constraints: `--stdin` implies stdout and cannot combine with
package-wide formatting; `--stdout` accepts exactly one file; `--check` cannot
combine with `--stdout`/`--stdin`; `--write` and `--stdout` are mutually
exclusive. Parse/config failures are distinct from formatting differences.

Manifest (`faber.toml`):

```toml
[format]
policy = "pretty-v1"     # or "normalise-v1" (default until migration)
```

No `[forma]` section, no `forma.toml`, no global dotfile config layer, no
config inheritance. Precedence: built-in defaults < package `[format]` < CLI
options.

Per-file: **no per-file rule lists in v1** (that makes every file its own
dialect). If migration proves a bounded escape hatch is necessary, add only a
frontmatter policy selector (`format-policy = "normalise-v1"`).

Locale: separate input from layout. Precedence unchanged (CLI → frontmatter →
package → en). For `--stdin` (no package discovery): `--locale` → frontmatter
→ en. Changing locale changes keyword spelling, never wrapping/indentation.

Deterministic inputs: source bytes + selected locale + selected policy. No
environment-dependent, filesystem-ordering, or host-tool behavior.

## Target output shape

```fab
functio compone(
    textus code,
    numerus width,
    numerus height
) → FaceQuad {
    fixum textus key ← normaliza(code)

    si key ≡ "" ergo redde FaceQuad {
        a = Vertex { x = 0, y = 0 }
        b = Vertex { x = width, y = height }
    }

    redde FaceQuad {
        a = Vertex { x = 0, y = 0 }
        b = Vertex { x = width, y = height }
    }
}
```

`si` chain (block form):

```fab
si code ≡ 1 {
    redde prima()
} sin code ≡ 2 {
    redde secunda()
} secus {
    redde fallback()
}
```

Long call wraps at delimiters; already-multiline calls are not collapsed when
a later edit makes them fit:

```fab
fixum Resultado result ← compone(
    request,
    options,
    callback
)
```

Nested records expand structurally; no field alignment:

```fab
redde FetchRequest {
    url = endpoint
    method = "POST"
    body = Payload {
        name = name
        value = value
    }
}
```

Principle: preserve author structure unless there is a clear readability or
width reason to change it. The formatter is not a compression engine.

## Idempotence

```text
format(format(source, policy, locale), policy, locale)
    == format(source, policy, locale)
```

This does **not** require every semantically equivalent source to collapse to
one byte sequence — preserving a deliberate multiline choice is compatible
with idempotence and friendlier to existing authors.

## Rollout safety

1. Promote the current normalizer to explicit `normalise-v1` — zero behavior
   change; the compat baseline.
2. Build a formatter-specific **golden corpus** first: blocks, compact `ergo`
   arms, calls, records, comments, blank lines, frontmatter, locale output,
   long unbreakable lines, idempotence checks.
3. Land `pretty-v1` behind **one** migration switch (`--policy pretty-v1`),
   no per-rule opt-in flags.
4. Advisory CI: `faber format --policy pretty-v1 --check` against
   representative packages and the exempla corpus.
5. Rebaseline corpus `.expected` files in **one dedicated mechanical commit**;
   never mix with semantic fixture changes or unrelated rustfmt/clippy cleanup.
   A changed `.expected` file is classified as formatting-only (rebaselined)
   or behavior-changing (investigated separately).
6. Flip the default to `pretty-v1` after the migration window; if needed, a
   time-limited legacy-normalizer mode for one release. Do not preserve two
   permanent formatter products.
7. Pin the corpus to the policy version; `--check` then means "matches the
   current Faber formatter profile," not "matches whichever historical
   normalizer ran." The pre-existing rustfmt/clippy gate debt is unrelated and
   must not be bundled into this rollout.

## Complexity guardrails — v1 reject list

- Column alignment of `←`/`→`, record fields, parameters, or match arms.
- User-configurable line width, indent width, or tabs.
- Per-file width/indent settings; per-rule boolean matrices; dozens of toggles.
- Import sorting or declaration reordering; semantic blank-line grouping.
- Comment prose reflow; string or interpolation rewriting; quote-style
  preferences; trailing-comma policy before the grammar supports it.
- Semicolon insertion/removal; arrow-parentheses toggles; arbitrary operator
  breaking.
- `forma.toml`; global dotfile config; multiple config files with inheritance.
- Environment- or editor-specific formatting; silent policy changes under the
  same policy name.
- A HIR-only printer that loses comments or source trivia.
- Permanent "author"/"canonical" style families as product vocabulary.
- Unknown rule slugs silently ignored — they must fail clearly.

A future `format = "off"` escape hatch may be justified for generated or
embedded source, but as one visible, bounded mechanism — not a way to make
every file unique.

## Scope boundaries

- **faber**: CLI (`--write`, `--stdin`, `--policy`, combos), `[format]`
  manifest schema + precedence, golden corpus, product docs, exempla
  rebaseline.
- **radix** (`radix::forma`): engine gains policy selection, the
  `pretty-v1` layout rules, and the rule-slug registry. `normalise-v1` must
  remain byte-identical to today's behavior.

## Relationship to existing goals

- `faber-format-lossless` (planned, vision-only): its trivia-preservation
  concern (comments, blank lines, string delimiters) is largely satisfied by
  `pretty-v1`'s normalizer-based approach in v1 — the normalizer already
  preserves line structure byte-for-byte. Lossless remains a potential
  follow-on only if deeper trivia fidelity (e.g. string-delimiter/`forma`
  literal preservation) proves necessary. `pretty-v1` does **not** require a
  token/CST rewrite in v1.
- `radix::forma` canonical HIR emit: stays distinct; `pretty-v1` is the author
  path. The `canonical` knob remains an implementation detail, not product
  vocabulary.

## Acceptance criteria (draft — delivery will refine)

- `normalise-v1` is byte-identical to current formatter behavior across the
  exempla corpus.
- Golden corpus committed; `pretty-v1` idempotent on it (and
  `format(format(x)) == format(x)` tested).
- `faber format --policy pretty-v1 --check` on exempla reports expected diffs
  only; rebaseline landed as one dedicated commit with the corpus pinned to
  the policy version.
- CLI additions and combo constraints (`--check`/`--stdout`/`--stdin`/
  `--write`) covered by tests.
- Unknown policy/rule slugs fail clearly.
- `[format] policy` manifest selection + precedence (defaults < package < CLI)
  tested; locale precedence unchanged and layout-independent.

## Handoff readiness

**Ready for lowering** — goal-forge → `$goal-check` → `$delivery` in flight
(planner-2). Delivery must resolve the open items: exact `--policy` flag
spelling vs `--pretty` alias, whether `line-width`/`indent-width` become
manifest keys in a later minor (v1: implementation constants only), and the
rebaseline commit's place in the stage graph.
