# Delivery: typed-error-union — `@ commune` shared fields + typed error identity

**Goal ref**: faber `docs/factory/typed-error-union/goal.md` (Vivi task `c72f9481`).
**Status**: lowered 2026-08-21 — READY for admission by Mind. Planning-only:
no compiler or product code is written by this lowering.
**Repos**: radix (compiler implementation) · faber (grammar source + rendered
EBNF, both regenerated — never hand-edited) · gradus (migration + policy).
**Entry gate**: MET — design settled in operator session 2026-08-21
(`docs/design/typed-error-union.md`); every code anchor re-verified live
2026-08-21 after gradus U14/U15 landed; external dependency X1 identified
with named owning units.

## Interpreted Unit

Land the fork-1 language surface (`@ commune` shared variant fields —
declared-once union region, `e.message` as plain member access, grammar via
the genus-member pattern), migrate gradus onto it as a pre-1.0 clean break,
and land the gradus-side consumers of the cross-module variant fix:
the B1-vs-B2 policy ruling, its execution, and the api-shape-policy PML1
correction. Fork 2 and structural `⇥` widening are rejected non-goals; the
radix defect fixes themselves (registry rows 1+9) are owned by
`compiler-defect-sprint` and enter this delivery only as external
dependency X1.

## Normalized Spec

One coherent outcome, many Hands: "gradus errors are typed end to end with
zero mirrors and zero string-matching remap chains, on a declared,
compiler-checked shared-field surface." Success is oracle-shaped — grep
counts go to zero, `faber check .` stays green, cross-module `e.message`
binds — not activity-shaped.

## Repo-Aware Baseline (verified 2026-08-21)

- **faber** — `main` at `80f1329`; dirty `docs/design/typed-error-union.md`
  (operator WIP — the source doc; never edited by these units).
  Grammar is generated: edit `docs/grammar/source.fg` (`genus_member`
  annotation slot at :833; `variant_fields` without one at :880), render via
  `python3 ../radix/scripta/grammar-pipeline.py --render --emit
  --render-locales` from faber root; `--check` green at HEAD (212
  productions; locale renders ar, hi, th-TH, vi, zh-Hans, zh-Hant).
  EBNF.md:109/132/173 anchors verified against the design doc's claims.
- **radix** — defect rows 1+9 verified still `known()` gaps at
  `crates/radix-module-boundary/src/walk.rs:372-399`; owning delivery
  `docs/factory/compiler-defect-sprint/compiler-defect-sprint-delivery.md`
  lowers them as `cds-u1-union-match` (row 1, P0/S4, wave 1) and
  `cds-u7-generic-construction` + `cds-u8-import-binding-collisions`
  (row 9, S3+S4) — none implemented at HEAD. Exhaustiveness verified:
  `crates/radix-semantic/src/passes/exhaustive.rs:118-166` (omnia contract),
  `:241-257` (`NonExhaustiveMatch`). Emitter leaves: `radix-hir-ts`,
  `radix-hir-rust`, `radix-hir-go`, `radix-hir-swift`, `radix-hir-python`,
  `radix-hir-haskell`, `radix-hir-lean`, `radix-hir-faber`,
  `radix-hir-fhir`, `faber-hir-rust`; MIR lowering under
  `crates/radix/src/mir`.
- **gradus** — `main` at `5ed351e`; U14 records + U15 facade split landed
  (census anchors refreshed accordingly): 33 `fn message(` mirrors across 35
  modules (`data.fab`, `gradus.fab` excepted); 260 `^\s+string message$`
  payload lines in 32 files; 111 `message(` occurrences in src + 200 in
  probas; remap chains at `src/transformer.fab:259`, `src/mlp.fab:122`
  (post-U15 location; design doc's `gradus.fab:160` is stale),
  `src/model/dense.fab:311`. Pre-1.0 v0.1.0;
  `docs/compatibility-policy.md` v1.2.2 clean-break posture;
  `docs/api-shape-policy.md` "Cross-module variant constraint" still frames
  SEM001/SEM041 as "a language constraint, recorded PML1". Standing proof:
  `faber check .` (README:111).

## Stage Graph — Hand units

All compiler units are additive-first: the `@ commune` region is new syntax,
so nothing existing stops parsing/checking mid-wave. No unit carries a
package/source/compile/`--stage`/`--e2e`/`--full` closeout — those are
lane-owned (§Checkpoints). Effort scale follows the compiler-defect-sprint
convention (1–5).

### TEU1 — `teu1-grammar-parse-surface`

| Field | Value |
| --- | --- |
| `outcome` | The union body accepts an annotated declaration region before the variant list (genus-member shape: `annotation* field_decl`), and radix-parser parses it into HIR. Canonical token `commune` (la). |
| `write_scope` | faber: `docs/grammar/source.fg`, regenerated `docs/EBNF.md` + `docs/EBNF.{ar,hi,th-TH,vi,zh-Hans,zh-Hant}.md` + `docs/grammar/grammar.jsonl` (pipeline-rendered only). radix: `crates/radix-parser` + its tests. |
| `done_when` | (a) `source.fg` validates and `--render`/`--emit`/`--render-locales` regenerate clean; `--check` green. (b) Parser round-trips the goal's `ShapeError` exemplum (commune region + bare variants + structured-payload variant) and rejects a variant redeclaring a commune field at parse/AST shape where that is a parse-level fact. (c) Existing grammar unchanged for unions without the region (212-production spine only grows). |
| `sanity` | `python3 ../radix/scripta/grammar-pipeline.py --check` from faber root; radix-parser unit tests for the three exemplum shapes. |
| `non_goals` | No semantic checks (TEU2), no emitters (TEU3/4), no en-name prose ruling beyond the rendering default (`@ shared`, open question 1). |
| `risk` | medium — grammar is the public identity surface; the region must not collide with `variant` heads (IDENTIFIER vs annotation lookahead). |
| `integrable` | yes — additive syntax; repo precedent: EBNF changes land alone (`833e018`, `5ed3c8df`). |
| effort | 3 |

### TEU2 — `teu2-semantic-contract`

| Field | Value |
| --- | --- |
| `outcome` | The commune contract is checked: region valid only on unions; construction requires commune fields (`variant DimensionAboveLimit { message = "…" }`); matching binds shared fields (`case Incompatible const message`); `e.message` is member access on union-typed values — same-module and cross-module under SEM001 as it stands; read-only; shadow-reject where not already parse-level. |
| `write_scope` | radix: `crates/radix-semantic` (typecheck/resolve/pattern passes) + tests; new corpus exemplum under `radix/corpus/` (e.g. `commune/`). |
| `done_when` | (a) Corpus exemplum checks green: declare → construct → match → `e.message`, including a consumer-module member access over an imported union. (b) Negative tests: commune field on non-union rejected; variant redeclaring commune field rejected; commune field assigned through projection rejected (read-only). (c) Existing unions (no region) check identically. |
| `sanity` | `cargo test -p radix-semantic` narrow filter + `radix check` on the new exemplum. |
| `non_goals` | No emit (TEU3/4); no `sponte` interaction (deferred); no exhaustiveness changes — plain-match checking already stands (`exhaustive.rs:241-257`). |
| `risk` | low-medium — mirrors existing member-access and pattern machinery. |
| `integrable` | yes — checks new shapes only. |
| effort | 3 |

### TEU3 — `teu3-emit-primaries`

| Field | Value |
| --- | --- |
| `outcome` | Commune fields materialize into every variant payload on the primary lane: HIR carries the commune region; `radix-hir-ts` (discriminated unions) and `radix-hir-rust` (enum struct-variants) emit them; MIR lowering (`crates/radix/src/mir`) carries them so proba-style execution and the check surface agree. |
| `write_scope` | radix: `crates/radix-hir`, `crates/radix-hir-ts`, `crates/radix-hir-rust`, `crates/radix/src/mir` (lowering path only), matching tests + exempla. |
| `done_when` | (a) TEU2 exemplum emits and compiles per target (TS + Rust); emitted construction sites require the commune field; emitted match arms and `e.message` reads behave. (b) MIR stepper-level test exercises `e.message` on a commune union. (c) fire-9 norm: cross-crate consumer enumeration at the unit boundary + tree-build proof (the DDCP2 lesson). |
| `sanity` | Narrow `cargo test -p radix-hir-ts -p radix-hir-rust` filters + exemplum build. |
| `non_goals` | Tail emitters (TEU4); gradus migration (TEU6); any emit for `sponte` unions (deferred). |
| `risk` | medium — per-target payload shape is the design's named bulk cost; Rust/TS shapes differ structurally. |
| `integrable` | yes — new feature path; existing unions emit unchanged. |
| effort | 4 |

### TEU4 — `teu4-emit-tail`

| Field | Value |
| --- | --- |
| `outcome` | Remaining backend leaves materialize commune payloads: `radix-hir-go`, `radix-hir-swift`, `radix-hir-python`, `radix-hir-haskell`, `radix-hir-lean`, `radix-hir-faber`, `radix-hir-fhir`, `faber-hir-rust`. Homogeneous follow-through on the TEU3 shape. |
| `write_scope` | radix: the eight leaf crates above + their tests. |
| `done_when` | (a) Each leaf emits the TEU2 exemplum with commune fields in payload; per-leaf test green. (b) fire-9: all-backend enumeration + tree-build proof once, at this unit's boundary. |
| `sanity` | Per-leaf narrow test filters. |
| `non_goals` | No new semantics; no gradus work; no matrix hand-edits (matrix renders from radix measurement — TEU9 checks). |
| `risk` | low — pattern proven by TEU3; mechanical breadth. |
| `integrable` | yes. |
| effort | 3 |

### TEU5 — `teu5-gradus-policy-ruling` (decision-first)

| Field | Value |
| --- | --- |
| `outcome` | The B1-vs-B2 ruling is recorded in `gradus/docs/api-shape-policy.md` with the session lean cited as default-not-settled: **B1** — shared package error union `gradus:error GradusError` for package-internal composition, per-module unions only at genuine public boundaries; B2 (per-module unions + typed remap) recorded as the rejected alternative with its cost (N×M wrapper wall remains). |
| `write_scope` | gradus: `docs/api-shape-policy.md` (new ruling section). |
| `done_when` | Ruling section present, names the decider and date, cites the design doc + lean, and states the execution dependency (X1) explicitly. |
| `sanity` | Doc renders; no source touched. |
| `non_goals` | No PML1 re-frame (TEU8 — different claim, different trigger); no code. |
| `risk` | low — docs-only. If the operator flips to B2, only TEU7's shape changes; the graph holds. |
| `integrable` | yes. |
| effort | 1 |

### TEU6 — `teu6-gradus-commune-migration` (clean break)

| Field | Value |
| --- | --- |
| `outcome` | Gradus migrates onto `@ commune`: 33 unions gain the region; per-variant `string message` lines drop (260 → 33, one declaration per union); 33 `fn message()` mirrors delete; `mod.message(e)` call sites become `e.message` (~78 src + 200 proba); throw sites untouched. |
| `write_scope` | gradus: `src/**/*.fab`, `src/**/*.proba`, plus the clean-break record in `docs/compatibility-policy.md`. |
| `done_when` | (a) `rg -c 'fn message\(' src --glob '*.fab'` → no matches. (b) `rg -c '^\s+string message$' src --glob '*.fab'` → exactly the union count (33). (c) `faber check .` green; probas green. (d) Compatibility-policy records the break per §1 (landing commit + policy + factory receipt). |
| `sanity` | The two grep oracles + `faber check .`. |
| `non_goals` | No remap-chain changes (TEU7 — different mechanism); no public-boundary redesign (that is TEU7 under the ruling); no mirror edits before this unit (fold-don't-churn). |
| `risk` | medium — 33 modules + 33 probas is breadth, not depth; mechanical transform, but proba surfaces (file-interface field visibility, `nn.proba:50` family) can surface unrelated pre-existing residuals — record, don't fix here. |
| `integrable` | yes — clean break lands whole on gradus main (pre-1.0 posture); path-limited commits per module group allowed inside the unit. |
| effort | 4 |

### TEU7 — `teu7-gradus-error-identity` (execute the ruling)

| Field | Value |
| --- | --- |
| `outcome` | Under the TEU5 ruling (B1 default): `gradus:error` owns `GradusError`; modules import it and throw its variants directly; the three remap chains (`transformer.fab:259`, `mlp.fab:122`, `model/dense.fab:311`) and the error-translating wrappers delete; per-module unions survive only at the public boundaries the ruling names. Under B2: wrappers stay but match typed variants (`case nn.X …`), no string-comparison bodies. |
| `write_scope` | gradus: `src/**/*.fab`, `src/**/*.proba` (call-site fallout), `faber.toml`/`cista.toml` only if the ruling adds the `gradus:error` module coordinate. |
| `done_when` | (a) Under B1: `rg -n 'fn _map_error\|fn _map_cached' src --glob '*.fab'` → none; cross-module error identity is typed variant construction/match. Under B2: zero string-comparison remap bodies remain. (b) `faber check .` green; probas green. (c) Recheck triggers for registry rows 1+9 hold on gradus as a live consumer (consumer-side match arms over imported union variants bind; qualified construction resolves). |
| `sanity` | Grep oracle + `faber check .` + one cross-module error-path proba. |
| `non_goals` | No radix fixes (X1 owns them; this unit only consumes); no api-shape-policy prose (TEU8). |
| `risk` | high — depends on X1 landing correctly; first real consumer of imported-union matching at scale. |
| `integrable` | yes (on gradus main, pre-1.0), but **must not be tasked before X1 lands**. |
| effort | 4 |

### TEU8 — `teu8-pml1-correction`

| Field | Value |
| --- | --- |
| `outcome` | `gradus/docs/api-shape-policy.md` "Cross-module variant constraint" is re-framed: SEM001/SEM041 were pinned radix defects (registry rows 1+9, `walk.rs:372-399`), fixed by `compiler-defect-sprint` cds-u1/cds-u7/cds-u8 — not a language law. Downstream conventions that cited the law (DType single-module + factory functions; accessor discipline) are marked as choices to revisit, not repealed here. |
| `write_scope` | gradus: `docs/api-shape-policy.md` (the constraint section only). |
| `done_when` | No "a language constraint, recorded PML1" claim remains; the section cites the registry rows, the fixing units, and the date; conventions are marked revisit-not-repealed. |
| `sanity` | Doc renders; grep for the stale phrase is empty. |
| `non_goals` | No convention changes themselves (DType layout etc. — separate decisions if wanted); no source. |
| `risk` | low — docs-only, but timing-gated: lands only after X1, else it documents a falsehood. |
| `integrable` | yes. **Blocked on X1.** |
| effort | 1 |

### TEU9 — `teu9-closeout`

| Field | Value |
| --- | --- |
| `outcome` | The goal closes honestly: ledger statuses, validation battery green, matrix currency checked, census re-run recorded, pattern-library wording located and updated or routed. |
| `write_scope` | faber: `docs/factory/typed-error-union/goal.md` (ledger/status), `docs/factory/typed-error-union/delivery.md` (statuses); `git mv` to `docs/archived/` per template convention when done. |
| `done_when` | (a) Validation battery (goal §Validation) all green, outputs recorded. (b) `python3 scripta/render-matrices.py --check` clean or the matrix legitimately refreshed by the radix release that carried TEU1–TEU4 (never hand-edited). (c) Census commands re-run; counts recorded in the ledger. (d) Open question 5 resolved: pattern-library entry located (design doc cites authority surface `gradus/src/shape.fab`; no standalone doc found 2026-08-21) — wording updated or routed to Mind. |
| `sanity` | The battery itself. |
| `non_goals` | No code; no new scope admission (that is a goal amendment). |
| `risk` | low. |
| `integrable` | yes. |
| effort | 1 |

**External dependency X1** (never implemented here): radix
`docs/factory/compiler-defect-sprint/` — `cds-u1-union-match` (registry row
1, SEM001 imported-union match, P0/S4) and `cds-u7-generic-construction` +
`cds-u8-import-binding-collisions` (registry row 9, G3 qualified variant
construction, S3+S4). Blocks TEU7 and TEU8. If X1's units re-slice before
landing, Mind re-points these deps at the successor unit ids — no amendment
to this delivery needed for a re-point.

## Implementation Work

Mind files Hand tasks as pointers: goal path + unit id. Ordering:

```
TEU1 ─→ TEU2 ─→ TEU3 ─→ TEU4
                     └────→ TEU6 ──────────────┐
TEU5 (parallel, docs-only)                     │
X1 (external, compiler-defect-sprint) ─┬───────┤
                                       ├→ TEU7 ┤
                                       └→ TEU8 ┤
                                               └→ TEU9
```

- Parallel from the start: TEU1-chain ∥ TEU5 ∥ X1 (external lane).
- TEU6 taskable the moment TEU3 lands (gradus checks/emits on the primary
  lane); TEU4 may trail TEU6 without blocking it — gradus does not gate on
  tail targets, only the TEU9 matrix check needs TEU4 shipped.
- TEU7/TEU8 taskable only when X1's owning goal reports its rows fixed.

## Checkpoints and Gates

- **Batching / split decision**: several Hands + no merge gate — every unit
  is integrable alone; atomicity is per-repo clean-break landing (gradus
  pre-1.0 posture), not a dual-authority window. TEU6 lands whole as one
  clean break with path-limited commits allowed inside it.
- **Lane-owned, named once** (never copied onto child Hands): radix —
  `./scripta/test --stage 1-3` per compiler unit, stages 4–6/`--e2e`/full
  profiles to auditor/operator; grammar `--check` is TEU1's own sanity and
  thereafter the lint lane's. gradus — `scripta/check-source`,
  `scripta/check-compile`; `faber check .` is the standing admission proof.
- **Hand sanity only** on children: the narrow greps/cargo filters/faber
  check named per unit above.
- **Release posture**: `release-prep` — radix grammar+compiler surface and a
  gradus clean break are package-facing; the radix release that carries
  TEU1–TEU4 is what refreshes the EBNF/conversio matrices (TEU9 verifies
  with `render-matrices.py --check`). gradus version stays 0.1.0; the break
  records in `compatibility-policy.md` per its §1.

## Validation

Hand sanity per unit: see unit tables. Goal closeout battery: goal
§Validation (grammar triple-check, mirror/payload/remap grep oracles,
`faber check .`, matrix `--check`).

## Companion Skill Plan

- `$delivery` (this artifact) → `$factory` phases per unit at execution.
- `$polish` on each unit's primary files before its closeout commit.
- Audit: planner-receipt then auditor seat after the compiler wave (TEU1–4)
  and again after the gradus wave (TEU6–8), matching the compiler-defect-
  sprint two-audit cadence.

## Open Questions

1. **en annotation name** (`@ shared` vs `@ common`) — default `@ shared`
   (session vocabulary). Mind settles, Head input optional. Affects en
   prose/rendering only; the canonical la token `commune` and the grammar
   are unaffected. Gates: TEU1's en-sidecar wording, nothing structural.
2. **B1 vs B2** — default B1 (lean, default-not-settled). TEU5 records it;
   the operator may flip before TEU7 tasks. TEU7's done_when carries both
   branches.
3. **Commune-field visibility** — default: inherit the union's visibility;
   `@ public` stacking allowed but not required. Settles inside TEU2's
   checks; revisit only if a consumer needs to force it.
4. **Emitter tail breadth** — default: all leaves in-goal (TEU4). If the
   matrix policy prefers measurement-gated ○ cells for some leaves, Mind
   may narrow TEU4's leaf list without touching the rest of the graph.
5. **Pattern-library wording** — locate at TEU9 (no standalone doc found
   2026-08-21; design doc cites `gradus/src/shape.fab` as authority
   surface).
