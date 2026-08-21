# Ratio record type

This document records the locked ratio-record-type ruling and its adopted
open-question defaults.

## Ruling clauses

### 1. Invariant

A `ratio` is a **named-field aggregate with no positional form and no
structural equivalence**: every field is reachable only by its label, labels
are compile-time facts, and conversion to any other shape (genus, another
ratio) is explicit (`↦` or reconstruction), never implicit subtyping.

### 2. Spelling and rejected candidates

**Spelling: `ratio`** (locked 2026-08-20; ledger sense — *rationes* are the
account ledgers; classical usage *rationes putare*, settle the accounts).
Operator reasoning recorded: the Latin meaning is exact; the English
false-friend (numeric fraction-ratio) does not apply because `en` and `la`
surfaces never appear together (pack-authoritative surface law); `record` and
`ratio` share the leading `r`, keeping the mnemonic bridge for English-first
readers. Sibling candidates rejected: `recordatum` (participle pattern but
English-reading stem), `descriptum` (stem collides with live `descriptio`
annotation keyword), `adsignatio` (exact but heavy), `acta`, `catastrum`,
`perscriptio`, `matricula`. The cartographic "mapping" word (`tabula`) is
already correctly spent on the map type.

### 3. Surface composition

**Surface** (composition of existing law only — no new mechanisms):

```fab
ratio<gx: tensor<f32,[2,2]>, gw: tensor<f32,[2,2]>> { gx = w1, gw = gw1 }
```

- Type: `ratio<IDENTIFIER ':' typeAnnotation (',' …)* '>'` — bracketed
  type-argument list (`iuncta<…>` / `tabula<…>` shape); labels mandatory (a
  ratio with an unlabeled slot is nothing — inverse of `iuncta`, where
  positions are mandatory and labels optional).
- Construction: `ratio<…> '{' fieldInit (',' fieldInit)* '}'` — the existing
  `Type { field = value }` form; `=` is the compile-time bind (operator
  trichotomy law).
- Access: `.name` member access only. No bracket form exists, by definition
  (no canonical order — the CLI seam's scattered `@ optio` declarations are
  the proof that order cannot be canonical).
- Destructuring: by-label via the existing `objectPattern`.
- Holes: element types admit `_`; `∪` rejected (monomorphic witness slots,
  same ruling family as `iuncta` / `explicit_union_type_arg_unsupported`).

### 4. Aggregate duality

**The duality this completes** (the aggregate 2×2):

|  | Ordered (positions) | Named (labels) |
| --- | --- | --- |
| **Declared / one-line** | `genus` (fields, methods, nominal) | `genus` |
| **Ad-hoc / inline** | `iuncta` (positions mandatory, labels optional) | `ratio` (labels mandatory, positions nonexistent) |

`iuncta` and `ratio` are duals: positions-with-optional-labels versus
labels-with-no-positions. The CLI record becomes a lawful instance of a user
kind instead of a compiler secret.

### 5. Fence

**Fence (load-bearing):** construction-and-access only, no structural
equivalence. `ratio<A: T>` never implicitly coerces to `ratio<A: T>` spelled
elsewhere, to a genus with the same fields, or from one; conversions go
through `↦` (registered) or reconstruction. No implicit subtyping lattice is
opened.

### 6. Non-goals

- Structural typing / row polymorphism / implicit record↔record or
  record↔genus coercion (the expensive lattice, fenced off)
- Touching `genus` (nominal declaration stays the named-type authority)
- Positional access of any form on ratios
- Reopening the Stage 6 anonymous-object retirement — `ratio<…>` is a
  keyword-prefixed form, no bare-delimiter claim
- Runtime names, reflection, dynamic field lookup
- Migrating the CLI seam to user syntax (internal `Type::Record` may be
  promoted or paralleled; that decision is Open question 2)

## Adopted open-question defaults

The delivery adopts OQ 1–4 from the goal:

1. **Diagnostic issue slugs** (missing label, duplicate label, positional
   access on ratio, implicit coercion attempt). Default: follow
   `unknown_*` / `*_unsupported` families.
2. **Representation:** promote the internal `Type::Record`
   (`FxHashMap<Symbol, TypeId>`, unordered — fits the no-canonical-order law)
   rather than introduce a parallel ordered representation.
3. **Inference:** no for v1 — explicit spelling only; the CLI seam keeps its
   annotation-derived inference unchanged.
4. **Pure literal inference:** reject `fixum r ← ratio<…> { … }` without the
   type head; the type head is part of the construction form (parallel to
   `iuncta<…> […]`, which also requires its head).

### OQ-A: sorted representation default

The adopted default is "promote `Type::Record`, emitter order = declaration
order at the type site… revisit if the map's unorderedness leaks." It leaks,
structurally: interning is by field-set value, so `ratio<g: A, w: B>` and
`ratio<w: B, g: A>` are the same `TypeId`, HIR carries `TypeId` only (no
spelling), and the hir-* leaf crates cannot see checker-side tables —
declaration order is not recoverable at any emit site, and a side-table "first
spelling wins" rule is per-program nondeterministic. **Recommended default:
canonical sorted-by-label emission order** — deterministic, visible from the
`Type` alone, identical in every backend, and consistent with the ruling's own
"order cannot be canonical" reasoning about access (rendering order is not a
semantic claim). Alternatives: checker-side order table (declaration order,
first-spelling-wins — needs new carriage into every emitter; strictly worse),
or the parallel ordered representation OQ-2 named (`Vec<(Symbol, TypeId)>` —
~40 consumer sites; the "parallel representation" fork the default already
rejected). **RR-4/RR-5 are written against the sorted default; a different
resolution refiles them.**
