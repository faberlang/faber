# Named Template Holes

**Status**: design locked 2026-08-20 (operator session); pre-implementation
**Syntax authority**: [`radix/docs/factory/named-template-holes/goal.md`](../../../radix/docs/factory/named-template-holes/goal.md)
**Surface**: [`docs/EBNF.md`](../EBNF.md)

This note records the locked ruling, its adopted open-question defaults, and the
baseline corrections found by the live implementation probes. The five ruling
clauses below are verbatim from the goal.

## Locked ruling

1. **Surface** — `§{label}` named hole in template-bearing literals: `"…"`
   (textus), `«…»` (block textus), `` `…` `` (forma). Not in `'…'` (ascii —
   `§` is already forbidden there). Label is an identifier spelling; keyword
   spellings are legal labels under the contextual law. Labels unique per
   template.
2. **Position law** — all holes, named and anonymous, occupy positions in
   order of appearance, one numbering domain shared with `§N`. `§` is sugar
   for the next position, `§N` an explicit position, `§{name}` an alias for
   its position. A named hole remains addressable by its index.
   `"§{greet} §"` is `[greet: 0, anonymous: 1]`.
3. **Labeled actuals** — template-call sites (string/forma literal callee +
   call suffix) accept `label: expression` actuals that fill that label's
   hole: `"§{greet} world"(greet: "salve")`. **Scope fence:** labeled actuals
   are legal only at template-call sites — this is not general named-function
   argument syntax. **Operator economy (locked 2026-08-20, operator
   formulation):** `x : y` **labels** (addresses a slot in something that
   already exists — no assignment); `x = y` **binds at compile**
   (declaration/construction: field defaults, enum values, aliases, record
   construction — resolved statically); `x ← y` **assigns at runtime**. The
   `=` form at a template-call site is rejected with a diagnostic stating
   the label/bind distinction.
4. **Erasure** — labels are erased at lowering. The template text reduces to
   its positional `§`-form, labeled actuals reorder to positional, and the
   lowered form is **identical** to the positional equivalent
   (`"§{greet} world"(greet: x)` ≡ `"§ world"(x)` ≡
   `scriptum("§ world", x)`). Zero runtime cost, zero emitter surface. `forma`
   capture stores the erased template plus positional params; labels document
   the parameter contract in source only.
5. **Checking** — every labeled actual names a declared label; every named
   hole is fillable (by label or position); arity equals hole count (existing
   rule). Diagnostics for: unknown label at call site, duplicate label in a
   template, labeled actual on a non-template call.

## Adopted open-question defaults

- **OQ1 — Diagnostic issue families.** New diagnostic issue slugs follow the
  `*_unsupported` / `unknown_*` families with position context arguments.
- **OQ2 — Mixed actuals.** Mixed labeled and positional actuals at one
  template-call site are rejected, mirroring the `iuncta` mixed-pattern
  default. A template with mixed holes is called positionally.
- **OQ3 — `forma` capture metadata.** No label metadata is carried in `forma`
  capture. Labels are erased everywhere, and tooling names parameters
  positionally (status quo).
- **OQ4 — Interleaving edges.** The position law decides `§`/`§N`/`§{name}`
  interleaving edges. A hole may be addressed by index even when named, and
  the edge cases are pinned with fixtures.

## Baseline corrections

- **C-1 (arity).** Goal clause 5 says "arity equals hole count (**existing**
  rule)". Live: oracles G check ok — no arity enforcement exists at parse,
  check, or lint. The rule itself is locked; NTH-3 **adds** it for all
  template applications (positional included). If any live source then
  violates arity, that is a finding to Mind — no silent weakening, no blanket
  corpus rewrite from the Hand seat.
- **C-2 (ascii fence).** Goal clause 1 says "`§` is already forbidden" in
  `'…'`. Live: `'§'` lexes/parses as plain content; the real fence is at
  application (oracle I). No change required; NTH-3 pins the fence as-is.
