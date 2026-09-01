# Typed error unions — the message-mirror and remap-chain problem

**Status**: design discussion — fork 1 working shape recorded (`@ commune`
shared variant fields, operator session 2026-08-21); fork 3 reframed as a
radix defect dependency; gradus policy choice (B1 vs B2) open
**Origin**: canonical-faber idiom audit of gradus (Vivi task `be9013ce` → audit
mail `0606c6d6` → receipt memo `b0bb1fb8`, observation O7); the
`typed-error-union` operator TBD in the canonical-faber pattern library
(authority surface `gradus/src/shape.fab`)
**Related rulings same day**: F14 restructure (`9041f01b`), O6 facade split
(`614987f8`), O7 extraction (`640db2ff`) — all Vivi memos, project
`/Users/ianzepp/work/faberlang`

This doc frames the problem and the design forks. The 2026-08-21 operator
session recorded a working shape for fork 1 and a reframing of fork 3 (see
"Session 2026-08-21"); the gradus-side policy choice remains open.

---

## Problem

The settled error idiom — failable operations return `T ⇥ XError` over a
`union` whose variants each carry a `string message`, guarded by
`require … throw variant` — carries two pieces of boilerplate that scale badly:

1. **The message mirror.** Every error union needs a hand-written
   `fn message(XError e) → string` whose every arm extracts the same single
   field:

   ```fab
   case UnknownName const message {
       return message
   }
   case UnknownVersion const message {
       return message
   }
   …
   ```

   The variant identity and the message field state the same fact twice per
   union: once in the variant list, once per mirror arm. Operator 2026-08-21:
   "match arms everywhere that have exactly one message field. It's honestly
   quite ugly."

2. **The remap chain.** Cross-module variant matching was recorded as a
   language constraint (`SEM001`/`SEM041`, recorded PML1 and in
   `gradus/docs/api-shape-policy.md`): a caller cannot match another module's
   variants, so `message()` text is the only cross-module error-identity
   surface. Wrappers therefore recover error identity by string comparison —
   `_map_error` chains at `gradus/src/transformer.fab:259`,
   `gradus/src/gradus.fab:160` (relocates to `src/mlp.fab` under the O6
   facade split), and `_map_cached` in `gradus/src/model/dense.fab`. Text
   matching to recover type identity is the fragile half of the problem.

   **Session correction**: the premise is stale — `SEM001`/`SEM041` are
   pinned radix *defects*, not a language law. See "SEM001/SEM041 are
   defects, not language law" below.

## Evidence census (verified live 2026-08-21)

| Surface | Count |
| --- | --- |
| gradus source files with a hand-written `fn message()` mirror | 33 of 34 modules |
| gradus variant payloads that are a single `string message` field | ~260 |
| string→variant remap chains | 3 (`transformer.fab`, `gradus.fab`→`mlp.fab`, `model/dense.fab`) |
| norma / triga / tela / examples mirrors | 1 message field, 0 mirrors (norma); 0 elsewhere |

The pain is gradus-concentrated today, but the idiom is the pattern-library
authority surface — the design decision is language-level, not
gradus-local. Every new gradus module adds another mirror.

## What is settled (the core stays)

From the pattern-library entry (operator ruling 2026-08-21): the
union / `⇥` / `require … throw variant` core **is** the pattern and is not
in question. The redesign targets the per-variant `string message` +
hand-written `message()` mirror and the text-matching remap chains — the
mirror boilerplate is explicitly "not required" by the pattern's own entry.

## Design forks (with 2026-08-21 session outcomes)

Not mutually exclusive; (1)+(3) is plausibly the real shape.

1. **Compiler-side derive.** The accessor is generated (or variant identity
   renders natively) and ~33 hand-written mirrors are deleted from gradus.
   Needs a Radix surface decision; nothing changes at the Faber source level
   for library authors beyond deleting the mirrors.
   **Session outcome**: superseded by *declared* shared fields (`@ commune`)
   — neither inference nor derive; the union declares the shared field once
   and `e.message` is plain field access. See the session section.
2. **Convention-only.** Library-side restructure (single-message union
   shapes, a shared message-carrying convention). Smallest blast radius, but
   expressibility is bounded by the language: if `message()` remains the only
   cross-module error surface, a mirror of some form remains required.
   **Session outcome**: rejected — cannot fix identity, and subsumed by the
   fork-1 shape plus the fork-3 reframing.
3. **Typed wrapping / chaining.** A typed error-composition surface across
   module boundaries — kills the string-matching remap chains (the fragile
   half). May require relaxing or working within the `SEM001`/`SEM041`
   cross-module variant law, or an error-carrier abstraction that carries
   identity without variant matching.
   **Session outcome**: reframed — the "law" is two pinned radix defects
   with recheck triggers, so this fork is defect resolution plus a gradus
   policy choice (B1 vs B2 below), not a language redesign.

## Session 2026-08-21

### SEM001/SEM041 are defects, not language law

The original framing recorded the cross-module variant law as a language
constraint. The radix defect ledger disagrees:
`radix/crates/radix-module-boundary/src/walk.rs:372-399` pins both boundary
directions as known defects in the defect-sprint registry, each with a
recheck trigger:

- **Registry row 1 (P0, S4)**: consumer-side `match x { case u.Bonum … }`
  over an imported union fails `SEM001.unknown_variant`; a same-module match
  over the same union is clean. Recheck trigger: "consumer-side match arms
  over imported union variants bind".
- **Registry row 9 (S3+S4)**: qualified variant *construction*
  `variant u.Bonum { … } ∷ u.Exitus` fails SEM001+SEM002. Recheck trigger:
  "qualified variant construction under an import alias resolves
  semantically".

Consequences: the remap chains work around a bug, not a settled constraint;
and `gradus/docs/api-shape-policy.md`'s PML1 note ("a language constraint,
recorded PML1") needs correcting when the defect rows land.

### `omnia` / `match all` — the existing exhaustiveness promise

Findings that shaped the fork-1 design:

- The exhaustive-match marker is `discerne omnia` (la) / `match all` (en);
  corpus exemplum `radix/corpus/omnia/omnia.fab`. Semantics
  (`radix/crates/radix-semantic/src/passes/exhaustive.rs:118-166`): valid
  only over a single enum scrutinee; forbids catchall arms (`casu _`, bare
  bindings, `ceterum`/`default`, mixed `casu A, _`); hard errors, because
  "`omnia` is an explicit author promise".
- Plain `match` over a single enum scrutinee with no catchall is **already
  exhaustiveness-checked** — hard error `NonExhaustiveMatch`
  (`exhaustive.rs:241-257`). The gradus mirrors have no `default` arm, so
  adding a variant without updating a mirror already fails compilation
  today. `omnia`'s added content is forbidding the catchall escape — a
  promise against future edits, not exhaustiveness itself.
- Library adoption is zero: no `match all` / `discerne omnia` in gradus,
  norma, triga, tela, or examples; only radix corpus exempla (and their
  worktree copies).
- Housekeeping note: `omnia` is overloaded three ways (import wildcard field,
  test-hook scope, exhaustive match); all contextual, no grammar collision,
  but the en locale glosses all three as "all".

Design moral: Faber already prices "adding a variant breaks downstream code"
as a feature when the author opts in. The fork-1 shape follows the same
philosophy — the invariant is declared by the type's author, checked by the
compiler, and violating it points the error at the declaration, not at
consumers.

### Fork 1 working shape: `@ commune` shared variant fields

The union declares shared fields once, in an annotated declaration region
before the variant list. Projection is then not a separate feature:
`e.message` is field access to a field the union always has by construction.

```fab
@ public { }
union ShapeError {
    @ commune
    string message

    NegativeDimension,
    DimensionAboveLimit,
    ProductAboveLimit,
    Incompatible,
    ElementMismatch {
        int expected
        int actual
    }
}
```

Decisions recorded in session:

- **Explicit declaration, not inference.** Automatic projection by uniform
  shape was considered and rejected: coincidental field-name uniformity would
  silently become public API, and breaking uniformity would error at consumer
  call sites instead of at the union definition. The declared contract
  localizes blame at the declaration — the `omnia` philosophy, one level up.
- **Not per-variant.** Annotating each variant's own field was rejected:
  optional participation makes projection conditional again, and "if any
  variant says it, all must" keeps the repetition while buying nothing.
  Declare-once is what keeps `e.message` unconditionally sound.
- **Annotation spelling, own line.** `@ commune` on its own line above the
  field declaration — Faber annotations are line-oriented
  (`annotation_sugar ::= '@' annotation_name … NEWLINE`, `EBNF.md:132`) —
  not a bare keyword in type position, where `de`/`ref`/`in`/`mut`/`own`/
  `copy` already live. The `@` spelling also reuses annotation parsing
  instead of growing the contextual-keyword table in two locales. en
  annotation name TBD (`@ shared` / `@ common`).
- **Grammar support.** Field-level annotations exist for genus members
  (`genus_member ::= annotation* (field_decl | functio_method_decl)`,
  `EBNF.md:109`) but not for variant fields
  (`variant_fields ::= (type_annotation IDENTIFIER)*`, `EBNF.md:173`). The
  union body gains a declaration region reusing the genus-member pattern;
  stacking (e.g. `@ public` + `@ commune`) is grammatically natural there.

Semantics:

- Construction requires commune fields alongside the variant's own:
  `variant DimensionAboveLimit { message = "…" }` — throw-site form
  unchanged.
- Matching unchanged: `case Incompatible const message` still binds the
  shared field; variants keep their own payload fields alongside commune
  ones (see `ElementMismatch` above).
- `e.message` is member access on a typed value, not variant resolution —
  it works across module boundaries under SEM001 as it stands today,
  replacing the mirror's cross-module render role.
- Read-only through the projection, initially.
- A variant redeclaring/shadowing a commune field: reject.
- `@ commune` + `sponte`: deferred (the projection type becomes
  `T ∪ nihil`; no current demand).

Migration (gradus, pre-1.0 clean break): ~260 variant declarations lose
their `string message` lines; 33 unions gain one `@ commune` declaration
each; the 33 `fn message()` mirrors are deleted; ~260 throw sites are
untouched; `mod.message(e)` call sites become `e.message`.

### Fork 3 remainder: gradus policy choice (open)

Once the radix defect rows land, gradus chooses:

- **B1 — shared package error union.** `gradus:error` owns `GradusError`;
  all modules import it and throw its variants directly. The remap chains
  and the error-translating wrappers disappear entirely. Cost: one
  package-global variant namespace; the per-module vocabulary documentation
  property moves into function docs.
- **B2 — per-module unions + typed remap.** Wrappers stay, but match typed
  variants instead of strings (`catch err { match err { case nn.X … } }`).
  Compiler-checked identity; the N×M wrapper wall remains.

Lean recorded in session: B1 for package-internal composition, with
per-module unions only at genuine public boundaries.

Rejected and recorded: structural error-set widening in `⇥` (Zig-style
inferred error unions). It would also kill the remap chains, but it is a far
larger language change — catch typing, throw-site ambiguity, loss of the
explicit per-function error contract — to solve what B1 solves library-side.

## Constraints and adjacent surfaces

- Gradus is pre-1.0: whatever ships lands as a clean break
  (`gradus/docs/compatibility-policy.md` v1.2.2; no shims).
- The cross-module variant boundary (`SEM001`/`SEM041`) is pinned as
  defect-sprint registry rows 1 and 9 in radix (`P0`/`S4`, recheck triggers
  recorded) — the fix is radix-scheduled compiler work, not a language-law
  change. Session correction of this doc's original framing.
- **Fold-don't-churn stands until this design lands**: no mirror or
  remap-chain edits ride the gradus cleanup train (U1–U13) or the
  F14/O6-spawned units (U14 block records, U15 facade split).
- Adjacent but distinct — do not fuse: `radix/docs/factory/diagnostic-identity-and-errors/`
  governs the compiler's own Rust error enums and diagnostics. Same theme
  family, different artifact and repo.

## Open questions — post-session

Answered in session (2026-08-21):

- ~~Does the message mirror become compiler-generated, or does the variant
  shape change so no mirror is needed?~~ → the variant shape changes:
  `@ commune` shared fields; no mirror and no derive.
- ~~Should cross-module error identity travel as text at all?~~ → not once
  the defect rows land; text remains only as the human render surface via
  `e.message` (which works cross-module today).
- ~~What happens to the ~260 existing throw sites and 33 mirrors~~ → throw
  sites untouched; variant declarations migrate; mirrors deleted in one
  clean-break wave after the language lands.
- Do some variants want structured payloads once the mirror problem is
  solved? → yes, freely: variants keep their own fields alongside the
  commune ones; only the shared field is declared at union level.

Still open:

- **B1 vs B2** — gradus policy ruling; can be made before the radix defect
  fix lands, mechanically blocked on it.
- **Emitter cost** — commune fields materialize into every variant payload
  per target (Rust enum struct-variants, TS discriminated unions, …); the
  per-target work is the bulk of the implementation.
- **Visibility** — whether `@ commune` fields need explicit `@ public`
  stacking or inherit the union's visibility.
- **en annotation name** — `@ shared` / `@ common`.
- **Generalization** — the shape is opt-in by declaration, so norma/triga/
  tela carry no adoption pressure; the pattern-library entry's wording
  becomes the post-design authority once the session closes.

## Provenance

```bash
vivi mail show 0606c6d6 --project /Users/ianzepp/work/faberlang   # the audit (O7 block, cleanup plan)
vivi memo show b0bb1fb8 --project /Users/ianzepp/work/faberlang   # receipt: F14/O6/O7 parked
vivi memo show 640db2ff --project /Users/ianzepp/work/faberlang   # this ruling: extract to design doc
rg -c 'fn message\(' gradus/src --glob '*.fab'                   # the 33 mirrors
rg -n 'fn _map_error|fn _map_cached' gradus/src --glob '*.fab'   # the remap chains
```

Session 2026-08-21 evidence:

- `radix/crates/radix-module-boundary/src/walk.rs:372-399` — defect-sprint
  registry rows 1 (SEM001 imported-union match, P0/S4) and 9 (SEM001+SEM002
  qualified variant construction, S3+S4), with recheck triggers.
- `radix/crates/radix-semantic/src/passes/exhaustive.rs:118-166,241-257` —
  `omnia` promise semantics; plain catchall-free enum matches already
  exhaustiveness-checked (`NonExhaustiveMatch`).
- `radix/corpus/omnia/omnia.fab` — en spelling `match all`; zero library
  adoption outside corpus exempla.
- `faber/docs/EBNF.md:109,132,173` — annotations on genus members
  (field-level support exists), line-oriented annotation sugar, no
  annotation slot on variant fields.
