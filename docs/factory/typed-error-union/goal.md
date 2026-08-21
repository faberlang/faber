# GOAL: typed-error-union — `@ commune` shared variant fields + typed error identity

**Status**: active — 4/9 units landed: TEU1 grammar+parser (`a5140f879`+`b258b80`), TEU2 semantic contract (`d6e869059`), TEU5 lean-B1 ruling (`6fb6897`), TEU3 emit primaries (`9f3ae1dce` — HIR commune_fields, Rust+TS payloads, MIR lowering + stepper; Mind re-verified 4/4 + 5/5 commune filters at the commit); TEU4 next; TEU7/8 gated on X1 defect-sprint
**Created**: 2026-08-21
**Campaign:** `—` (standalone; operator design session 2026-08-21)
**Source:** [`docs/design/typed-error-union.md`](../../design/typed-error-union.md) (operator session decisions; 316 lines); Vivi task `c72f9481`; origin chain `be9013ce` → `0606c6d6` → `b0bb1fb8` → `640db2ff`
**Repos:** faber (grammar source + rendered EBNF + this artifact) · radix (compiler) · gradus (migration + policy)
**Related:** radix `docs/factory/compiler-defect-sprint/` (owns defect rows 1+9 — external dependency, referenced not duplicated) · radix `docs/factory/diagnostic-identity-and-errors/` (adjacent, do-not-fuse) · gradus `docs/api-shape-policy.md` (PML1 correction target) · gradus `docs/compatibility-policy.md` v1.2.2 (clean-break posture)

---

## Invariant

A union may declare shared fields once in a `@ commune` region; every
variant carries them by construction, `e.message` is plain member access
that binds across module boundaries, and no hand-written `message()` mirror
or string-matching remap chain remains in gradus.

## Problem

The settled error idiom (`T ⇥ XError` over a union with per-variant
`string message`, guarded by `require … throw variant`) carries two costs
that scale badly. Both were measured live 2026-08-21, refreshed after
gradus U14 (records, `5ed351e`) and U15 (facade split, `96f5d59`) landed:

1. **The message mirror.** 33 hand-written `fn message(X) → string` mirrors
   across 35 gradus modules (every module except `data.fab` and the
   `gradus.fab` facade), each arm re-stating the variant's one field.
   260 variant payload lines are exactly `^\s+string message$` (32 files).
   Mirror *use* compounds it: 111 `message(` occurrences in `src` (33 are
   the mirror definitions) plus 200 in the 33 `.proba` files. The design
   session recorded the operator's verdict: "match arms everywhere that
   have exactly one message field. It's honestly quite ugly."
2. **The remap chain.** Three string→variant remap chains recover error
   identity by text matching: `gradus/src/transformer.fab:259`,
   `gradus/src/mlp.fab:122` (relocated from `gradus.fab:160` by the U15
   facade split — design-doc anchor stale, refreshed here), and
   `gradus/src/model/dense.fab:311` (`_map_cached`). These work around
   radix defects, not a language law:
   `radix/crates/radix-module-boundary/src/walk.rs:372-399` pins registry
   row 1 (SEM001 imported-union match, P0/S4) and row 9 (SEM001+SEM002
   qualified variant construction, S3+S4) as `known()` gaps — both verified
   still failing at HEAD. `gradus/docs/api-shape-policy.md` ("Cross-module
   variant constraint — a language constraint, recorded PML1") encodes the
   stale framing and needs correcting once the rows land.

Counter-evidence that bounds the change: plain catchall-free `match` over
an enum is *already* exhaustiveness-checked
(`radix/crates/radix-semantic/src/passes/exhaustive.rs:241-257`,
`NonExhaustiveMatch`), and `omnia` hard-errors off-contract shapes
(`exhaustive.rs:118-166`). Adding a variant without updating a mirror
already fails compilation today; the fork-1 shape extends that same
philosophy — the invariant is declared by the type's author and violating
it points at the declaration. Adoption elsewhere is zero (no `discerne
omnia`/`match all` in gradus, norma, triga, tela; corpus exemplum
`radix/corpus/omnia/omnia.fab` only), so norma/triga/tela carry no
migration pressure: the surface is opt-in.

## Proposal

**Fork 1 — `@ commune` shared variant fields (radix language work).** The
union declares shared fields once in an annotated declaration region
before the variant list, reusing the genus-member grammar shape; projection
stops being a feature because `e.message` is field access to a field the
union always has:

```fab
@ public { }
union ShapeError {
    @ commune
    string message

    NegativeDimension,
    Incompatible,
    ElementMismatch {
        int expected
        int actual
    }
}
```

Session-settled decisions (do not relitigate): explicit declaration, not
inference (uniform-shape projection was rejected — coincidental uniformity
would silently become API); not per-variant (optional participation buys
nothing); annotation spelling on its own line (`@ commune`, la) because
Faber annotations are line-oriented (`annotation_sugar`, EBNF.md:132) and
the `@` spelling reuses annotation parsing instead of growing the
contextual-keyword table in two locales; grammar via the genus-member
pattern — field-level annotations exist for genus members (EBNF.md:109;
`faber/docs/grammar/source.fg:833`) but `variant_fields` has no annotation
slot (EBNF.md:173; `source.fg:880`). Semantics: construction requires
commune fields (`variant DimensionAboveLimit { message = "…" }` — throw
sites unchanged); matching unchanged (`case Incompatible const message`
still binds); `e.message` is member access, which works cross-module under
SEM001 as it stands today; read-only initially; a variant redeclaring a
commune field is rejected; `@ commune` + `sponte` deferred (no demand).

Grammar provenance: `faber/docs/EBNF.md` is generated — the edit surface is
`faber/docs/grammar/source.fg`, re-rendered via
`radix/scripta/grammar-pipeline.py` (`--render`, `--emit`,
`--render-locales`; `--check` is the fail-closed gate, green at HEAD with
212 productions). Emitter cost is the bulk of implementation: commune
fields materialize into every variant payload per target.

**Fork 3 — defect dependency + gradus policy.** The cross-module fix is
*not* this goal's write: registry rows 1+9 are owned and lowered in radix
`docs/factory/compiler-defect-sprint/` (row 1 → unit `cds-u1-union-match`;
row 9 → units `cds-u7-generic-construction` + `cds-u8-import-binding-collisions`;
none implemented yet). This goal carries the *consumers* of that fix: the
gradus B1-vs-B2 policy ruling and its execution, plus the api-shape-policy
PML1 correction. Lean recorded in session — **B1** (shared package error
union `gradus:error GradusError`, package-internal; per-module unions only
at genuine public boundaries) — as **default-not-settled**. B2 (per-module
unions + typed remap) remains choosable at the ruling unit.

**Gradus migration (clean break).** Pre-1.0 posture governs
(`compatibility-policy` v1.2.2; no shims): 33 unions gain one `@ commune`
declaration; 260 payload lines drop; 33 mirrors delete; ~78 src + 200
proba `mod.message(e)` call sites become `e.message`; throw sites
(~260) untouched.

### Non-goals

- **Fork 2 (convention-only library restructure)** — rejected in session;
  cannot fix identity.
- **Structural error-set widening in `⇥`** (Zig-style inferred error
  unions) — rejected in session; far larger language change for what B1
  solves library-side.
- **Fixing radix defect rows 1+9 here** — owned by
  `compiler-defect-sprint`; referenced as external dependency X1 only.
- **`diagnostic-identity-and-errors`** (compiler's own Rust error enums) —
  adjacent family, different artifact; never fused into these units.
- **Mirror/remap edits riding the gradus cleanup train** (U1–U15) —
  fold-don't-churn stands until this goal lands.
- **Automatic projection by uniform shape; per-variant opt-in; `@ commune`
  + `sponte`** — rejected/defered in session.
- **norma/triga/tela migration** — no adoption pressure; opt-in surface.
- **runtime/packages changes** — no evidence of need; revisit at closeout
  only if a target support package must carry the surface.

## Units (lowering sketch — refined in `delivery.md`)

| Unit | Scope | Depends on | Hand evidence |
| --- | --- | --- | --- |
| TEU1 | Grammar + parse surface: `source.fg` union declaration region (genus-member pattern), re-render EBNF/locale/jsonl, radix-parser + tests | — | none |
| TEU2 | Semantic contract: commune region on unions, construction/matching/member-access rules, shadow-reject, cross-module `e.message`; corpus exemplum | TEU1 | none |
| TEU3 | Emit primaries: HIR commune payload + `radix-hir-ts`, `radix-hir-rust`, MIR lowering + exempla | TEU2 | none |
| TEU4 | Emit tail: remaining `radix-hir-*` leaves materialize commune payloads | TEU3 | none |
| TEU5 | Gradus policy ruling (decision-first): B1-vs-B2 recorded in api-shape-policy, lean cited | — | none |
| TEU6 | Gradus `@ commune` migration clean break (src + probas + compatibility-policy entry) | TEU3 | none |
| TEU7 | Gradus error-identity restructure per ruling (B1: `GradusError`, remap chains + translating wrappers deleted) | TEU5, X1 | none |
| TEU8 | api-shape-policy PML1 correction (defects-not-law re-frame) | X1 | none |
| TEU9 | Closeout: ledger, matrix check, census re-run, archive | all | none |

**External dependency X1** — `compiler-defect-sprint` `cds-u1` (row 1) +
`cds-u7`/`cds-u8` (row 9) landed. Blocks TEU7/TEU8; owned elsewhere, never
re-implemented here.

## Validation

Closeout gate (TEU9), run from the sibling-checkout layout:

```sh
cd faber   && python3 ../radix/scripta/grammar-pipeline.py --check   # normative triple + locales
cd gradus  && rg -c 'fn message\(' src --glob '*.fab'               # oracle: no output (0 mirrors)
cd gradus  && rg -c '^\s+string message$' src --glob '*.fab'         # oracle: 33 (one @ commune decl per union)
cd gradus  && rg -n 'fn _map_error|fn _map_cached' src --glob '*.fab' # oracle: none under B1
cd gradus  && faber check .                                           # standing proof (README)
cd faber   && python3 scripta/render-matrices.py --check              # matrix not stale after radix release
```

Lane-owned (never on child Hands): radix `./scripta/test --stage 1-3` per
compiler unit; gradus `scripta/check-source`/`check-compile`; stages 4–6,
`--e2e`, full profiles are auditor/operator gates.

## Ledger

| Unit | Status | Hand seat | Receipt | Notes |
| --- | --- | --- | --- | --- |
| TEU1 | done | — | `a5140f879`+`b258b80` | grammar+parser |
| TEU2 | done | — | `d6e869059` | semantic contract |
| TEU3 | done | aa1bc13b | `9f3ae1dce` | emit primaries; HIR commune_fields + Rust/TS payloads + MIR stepper |
| TEU4 | pending | — | — | emit tail |
| TEU5 | done | — | `6fb6897` | lean-B1 ruling recorded |
| TEU6 | pending | — | — | clean break v1.2.2 |
| TEU7 | pending | — | — | blocked on X1 |
| TEU8 | pending | — | — | blocked on X1 |
| TEU9 | pending | — | — | closeout |

## Open questions

1. **en annotation name** — `@ shared` vs `@ common`. Default `@ shared`
   (matches the session's own vocabulary, "shared variant fields");
   Mind settles, with Head input if wanted. Gates only the en rendering
   prose, not the canonical la token or the grammar.
2. **B1 vs B2** — gradus policy ruling. Default B1 (lean, session-recorded
   as default-not-settled); mechanically blocked on X1 for execution, not
   for the ruling itself (TEU5 is decision-first).
3. **Commune-field visibility** — explicit `@ public` stacking vs inheriting
   the union's visibility. Default: inherit; stacking stays grammatically
   natural per the genus-member shape but is not required.
4. **Emitter tail breadth** — all backend leaves in-goal (default; the
   grammar×target matrix otherwise renders unsupported) vs
   measurement-gated follow-through. Revisitable at TEU4 tasking.
5. **Pattern-library wording** — the canonical-faber pattern entry's
   post-design wording ("the entry's wording becomes the post-design
   authority"); no standalone pattern-library doc found under
   `faber/docs` or `gradus/docs` — locate at TEU9 closeout (authority
   surface cited by the design doc: `gradus/src/shape.fab`).
