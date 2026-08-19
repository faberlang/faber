# Delivery: FCMP profile 1 — registration, profile conformance, and the re-formed freeze gate

**Assignment**: Vivi task `97367613` (Mind → planner, 2026-08-18) — re-lower goal `gol_58147dc9ef95b704` after the FCMP restructure
**Planner**: planner (handle `97367613`); no packet; evidence from main checkouts only
**Goal**: [`goal.md`](goal.md) — **Status** `planned`; re-scoped 2026-08-18
**Restructure authority**: mind mail `08252f89` (2026-08-18 23:44Z) + [`addendum.md`](addendum.md)
**Protocol**: [`../../faber-messagepack-profile-v1.md`](../../faber-messagepack-profile-v1.md) @ faber `c85d807` — draft 1; residuals folded
**Consumer goal**: [`../fhir-package-format/goal.md`](../fhir-package-format/goal.md) — FS-1 `3dfc219`, FS-2 `93f5228` landed; FS-3/FS-4 not yet lowered there
**Baselines at re-lowering**: faber `c6e36c9` (main, clean); radix `38ca0cf4e` (main; policy receipt `c5f79ccbd` in history)
**Status**: planned — re-lowered 2026-08-18; **zero dispatchable units today**; all remaining units gate on fhir-package-format FS-3
**This artifact is planning only.** No product code, no freeze, no FHIR schema work (owned next door).

This spec replaces the 2026-08-18 freeze-path delivery (commit `be34cfd`),
whose entire dispatchable surface has since landed and whose freeze-first
sequencing was withdrawn by mail `08252f89`.

---

## 1. Goal-check summary

| Field | Value |
| --- | --- |
| goal_path | `faber/docs/factory/fcmp-profile-1/goal.md` (`gol_58147dc9ef95b704`) |
| evaluator mode | cold self-pass against live protocol, live policy, landed receipts, fhir-package-format state |
| intended consumer | Mind — re-activation dispatch when fhir-package-format FS-3 lands |
| verdict | **READY** (gated; nothing dispatchable now) |

**Reasoning.** Every unit of the prior delivery's dispatchable surface is
landed with receipts (§4). The restructure moved the FHIR cargo, DTOs,
vectors-for-FHIR, and the writer/reader switch to fhir-package-format
(FS-1/FS-2 landed; FS-3/FS-4 pending lowering there), withdrew the freeze ask
as blocked-by-design, and left this goal owning the profile surface: protocol
custody, kind registration, §11.1 generic vectors, §11.3 independent TS
codec, and the re-formed freeze gate. Each remainder is a bounded
one-logical-change Hand once its external gate opens; none can start before
fhir-package-format FS-3 exists (codec to run vectors against, limits to
register).

**Key points.**

- Delivery v1's U-PRE-1 and U-PRE-2 landed **after** that spec was written
  (receipts in §4). Its U-3A–E codec spine is absorbed by fhir-package-format
  FS-3 — the addendum forbids dispatching a generic codec ahead of the
  consumer. Its U-4/U-5 chains moved with mail `08252f89`.
- FS-2 (`93f5228`) binds a draft `value` (19 families, kinds `[1,0]` draft,
  limits deliberately unpublished). That satisfies the addendum's
  "value must be defined before approval" bar for *future* freeze, but kinds
  stay reserved and the profile stays draft (FS-2 header; protocol §10).
- No operator decision is open: the reserved fcmp freeze decision was closed
  BLOCKED-BY-DESIGN (mail `08252f89`).

**Blocking gaps:** none at the planning boundary. External gate:
fhir-package-format FS-3 (reference codec + limits publication), not yet
lowered in that goal — Mind dispatches that lowering separately; it is not
this assignment.

**Recommended next step.** No fcmp Hand is filed now. When
fhir-package-format FS-3 lands, file **FC-R1 ‖ FC-R2** (write-disjoint),
then FC-R3 after FHIR fixtures exist, then GATE-FREEZE-2.

---

## 2. Interpreted unit

Re-scope the goal to the profile's own surface after the 2026-08-18
restructure, mark landed/moved work with receipts (never re-mint), and lower
the remainder:

1. **FC-R1** — register the two FHIR kinds with concrete finite limits once
   the consumer goal publishes them.
2. **FC-R2** — author the §11.1 generic profile vector suite and prove it on
   the FS-3 reference codec.
3. **FC-R3** — independent TypeScript codec over the shared fixtures (the
   profile's cross-language claim).
4. **GATE-FREEZE-2** — Head §14 re-check + operator freeze after
   registration; not a Hand; no ask open now.

This lowering does not freeze the profile, implement the reference codec,
define FHIR schemas, or move any cargo work back here.

## 3. Normalized spec

**End state (remaining, whole goal).** Protocol §10 registers `fhir.unit` and
`fhir.package` with concrete finite per-kind limits and full registry fields.
The §11.1 generic suite exists as shared bytes with SHA-256 identities and is
green on the reference codec. An independent TS implementation re-encodes
every shared positive (generic + FHIR) byte-identically and rejects shared
negatives with the same §5.1 class. Freeze is decided through GATE-FREEZE-2
or explicitly re-blocked with cause.

**Split boundary.**

```text
fhir-package-format FS-3 lands (reference codec + limits publication)   [external gate]
  -> {FC-R1 ‖ FC-R2}
       -> FC-R3            [+ external: FHIR fixtures published by fhir-package-format]
            -> GATE-FREEZE-2 (Head §14 re-check + operator freeze; not a Hand)
```

**Locked decisions (Hands do not reopen).**

1. Rules already folded are settled text: identity (a) omit-defaults;
   prefix 64/3/2/256; kind strings `MUST NOT` be reused; §5.1 error-class
   vocabulary (no new classes in implementation — route a protocol amend
   instead); `fcmp.test` reserved, product decoders MUST NOT admit it.
2. **Limits are not invented here.** FC-R1 transcribes the limits
   fhir-package-format publishes (expected with FS-3). If none exist at
   dispatch time, FC-R1 does not run — registration without limits MUST fail
   (protocol §6/§10).
3. **Registry home** stays protocol §10 (single registry; no second format).
4. **Shared fixtures** live under `faber/docs/fcmp/vectors/`; Rust and TS
   load the same bytes and MUST NOT share encoder source.
5. **TS home** is a new `faber/fcmp/` conformance package — not
   `@faber/runtime`. No new Node/Bun product toolchain; the runner the unit
   adds is recorded in its receipt.
6. **Crate home** for codec-side conformance tests is whatever
   fhir-package-format's FS-3 delivery names (its lowering, not this spec).
   This goal adds no crate.
7. Profile versions ≠ product semver; the per-kind current-plus-two window
   is already carried by `radix/docs/release/faber/policy.md` (`c5f79ccbd`).

## 4. Repo-aware baseline (verified 2026-08-18)

Authority order: live files + git receipts → mail `08252f89` → goal prose.

| Claim | Evidence |
| --- | --- |
| U-1/2/2a landed | `604d6a30`; `cfcb44be`; `02327f9` (faber main history) |
| PRE-1 policy window landed | radix `c5f79ccbd`; `docs/release/faber/policy.md` §"FCMP document-major window" (lines 92–106) |
| PRE-2 protocol residuals landed | faber `0c65e72` (kind-reuse MUST NOT, `fcmp.test` reserved, §5.1 table), `04cda43` (MUST-rejects → classes), `c85d807` (invariant decoder error class); protocol lines 401–427, 613–626 |
| FS-1 field spec landed | faber `3dfc219` (fhir-package-format; 19 families) |
| FS-2 mapping landed | faber `93f5228` (fhir-package-format; binds draft `value`; kinds stay reserved; §20 operator losslessness question open there) |
| Freeze withdrawn | mail `08252f89`: "FREEZE DECISION WITHDRAWN … stays draft until a consumer schema binds its value"; reserved operator decision closed BLOCKED-BY-DESIGN |
| Kinds still reserved | protocol §10 table: `fhir.unit`/`fhir.package` "reserved; schema and limits not published" |
| No FCMP codec exists | workspace grep `FABERMP` hits only protocol + docs; live FHIR wire is postcard (`radix-hir-fhir` `SCHEMA_VERSION = 3`, `PACKAGE_SCHEMA_VERSION = 1`) |
| fhir-package-format not lowered | goal dir has goal.md + fs-1 + fs-2 only; no delivery.md; its Status line predates FS-1/FS-2 landing (stale there — Mind's next planner dispatch) |
| faber main clean at `c6e36c9` | `git status` empty; branch `main` |

## 5. Hand unit graph

Units carry the campaign fields. `sanity` is the unit's own narrow check;
lane gates are named once in §7.

### FC-R1 — register `fhir.unit` / `fhir.package` with finite limits

| Field | Value |
| --- | --- |
| `outcome` | Protocol §10 initial-reservations table rows for `fhir.unit` and `fhir.package` become registered entries: concrete finite per-kind limits (§6 fields), owning schema path (`fhir-package-format/fs-2-mapping.md`), current stable schema version, supported window, fixture-manifest path (may point at the FC-R2/FS-3 fixture dirs). `fcmp.test` stays reserved; the registration-without-limits rule text is untouched. Status line stays draft (freeze is GATE-FREEZE-2, not this unit). |
| `write_scope` | `faber/docs/faber-messagepack-profile-v1.md` only |
| `done_when` | Both kinds show registered with numeric finite limits and full registry fields; `rg -n 'reserved; schema and limits not published'` no longer matches those two rows; `fcmp.test` row unchanged; Status line still draft |
| `sanity` | `rg -n 'fhir\.(unit|package)' docs/faber-messagepack-profile-v1.md` shows registered rows with limits (cwd: faber) |
| `depends_on` | **External:** fhir-package-format FS-3 limits publication (if absent at dispatch, do not run — do not invent numbers) |
| `non_goals` | No freeze language; no `fcmp.test` registration; no limit invention; no schema edits in fhir-package-format docs |
| `risk` | low — docs-only transcription of published numbers |
| `integrable` | yes |

### FC-R2 — §11.1 generic profile vectors + conformance tests

| Field | Value |
| --- | --- |
| `outcome` | Shared generic suite under `faber/docs/fcmp/vectors/generic/`: positives per §11.1 bullet (shortest-form primitives, sorted map keys, the U-2a omit-defaults pair, well-formed root) with SHA-256 frame digests in a manifest; negatives covering every §5.1 class reachable without a FHIR schema (`truncated`, `trailing_bytes`, `bad_magic`, `profile_unsupported`, `payload_limit`, `kind_string_limit`, `root_map`, `schema_array`, `kind_unregistered`, `schema_unsupported`, `noncanonical`, `duplicate_key`, `unknown_field`, `missing_field`, `type`, `utf8`, `extension`, `limit`, `overflow`); conformance tests riding the FS-3 reference codec load and assert the suite. If a kind string is required, `fcmp.test` is admitted by the conformance decoder only (protocol §10 reservation stands for product decoders). |
| `write_scope` | `faber/docs/fcmp/vectors/generic/**` (new); tests inside the FS-3 codec crate as named by fhir-package-format's delivery (radix; this unit adds test files only, no crate changes beyond test registration); protocol §10 `fcmp.test` row only if the conformance-decoder admission needs stating |
| `done_when` | Every §11.1 bullet has a fixture; positives re-encode byte-identically with recorded SHA-256; each listed negative fails with its locked class; suite green via the crate's focused test target |
| `sanity` | the focused vector-test command this unit adds (record exact command in receipt); `ls docs/fcmp/vectors/generic` non-empty (cwd: faber) |
| `depends_on` | **External:** fhir-package-format FS-3 codec landed; FC-R1 is not a dependency (vectors exercise the profile, not FHIR kinds) |
| `non_goals` | No FHIR document fixtures (fhir-package-format owns those); no new error classes; no codec implementation changes beyond test hooks |
| `risk` | medium — fixture authoring plus cross-repo test wiring; negative-class coverage must match §5.1 exactly |
| `integrable` | yes |

### FC-R3 — independent TypeScript codec (§11.3)

| Field | Value |
| --- | --- |
| `outcome` | New `faber/fcmp/` TS conformance package: decodes and re-encodes every shared positive fixture (generic + FHIR) byte-identically; rejects every shared negative with the same §5.1 `class`; no shared encoder source with the Rust codec. Runner recorded in the receipt; no new Node/Bun product toolchain. |
| `write_scope` | `faber/fcmp/**` (new). May read `faber/docs/fcmp/vectors/**`. Must not edit rust crates or the protocol. |
| `done_when` | §11.3 cross-language items hold on the shared fixtures; repeated runs byte-identical; runner command recorded; `git grep` shows no encoder source shared with radix |
| `sanity` | the package's own test entry this unit adds (exact command in receipt) |
| `depends_on` | FC-R2; **external:** FHIR document fixtures published by fhir-package-format (FS-3/FS-4 outputs). If only generic fixtures exist, the unit may land generic coverage with FHIR coverage explicitly outstanding only if Mind accepts a split — default is to wait. |
| `non_goals` | No product runtime integration (`@faber/runtime` untouched); no fixture authoring (consumes shared bytes); no protocol edits |
| `risk` | medium — toolchain policy (stop and route a need rather than invent a Node product path) |
| `integrable` | yes |

### GATE-FREEZE-2 (not a Hand)

After FC-R1 (minimum) and preferably the full ladder (FC-R2 + FC-R3), Mind
files `head-cto` for a §14 re-check against the registered protocol, then the
operator takes a separate freeze decision recorded on the protocol and goal
Status lines. Disposition `proceed` on the re-check is still not a freeze.
No ask is open today (mail `08252f89` closed the reserved decision as
BLOCKED-BY-DESIGN); this gate re-opens only when its dependencies land.

## 6. Mind pointer table

| id | write_scope (short) | done_when | dispatchable? |
| --- | --- | --- | --- |
| `FC-R1` | protocol §10 registration rows | kinds registered with finite limits | after FS-3 publishes limits |
| `FC-R2` | `faber/docs/fcmp/vectors/generic/` + codec-crate tests | §11.1 suite green | after FS-3 codec lands (‖ FC-R1) |
| `FC-R3` | `faber/fcmp/**` | §11.3 TS gate on shared fixtures | after FC-R2 + FHIR fixtures |
| `GATE-FREEZE-2` | protocol + goal Status lines (Head + operator) | freeze decided or re-blocked with cause | after FC-R1 minimum; **not a Hand** |

## 7. Checkpoints and lane-owned validation

| Gate | When | Owner | Content |
| --- | --- | --- | --- |
| **SG-R** | after FC-R1 ‖ FC-R2 | Mind | registration rows carry published limits; §11.1 suite green; Status still draft |
| **SG-T** | after FC-R3 | Mind | TS re-encodes shared positives byte-identically; same-class rejects |
| **SG-F2** | GATE-FREEZE-2 | operator | §14 re-check folded; freeze decided or explicitly re-blocked |

**Standing custody:** envelope-rule gaps surfaced by fhir-package-format
FS-3/FS-4 fold into the protocol under this goal as as-needed Mind-routed
amends; they are not silent FS-side forks.

**Lane-owned (named once):** lint/test/merge own workspace compile, stages
1–4, and `./scripta/check-factory-goal-status` after Status-line updates.
Hands run only their unit `sanity`.

## 8. Validation

Hand sanity = each unit's `sanity` command only. Existence rule: every
command either exists today (`rg`, `ls`, focused crate tests once FS-3
lands) or is created by that same unit (vector suite, TS package test
entry).

## 9. Open questions (defaults recorded)

1. **Freeze trigger breadth.** Default: GATE-FREEZE-2 may be raised after
   FC-R1; operator may require the full ladder first. Not open now.
2. **FC-R3 split on fixture availability.** Default: wait for FHIR fixtures
   rather than land partial coverage.
3. **fhir-package-format lowering.** FS-3/FS-4 are not lowered and that
   goal's Status line predates FS-1/FS-2 landing. Mind's next planner
   dispatch, separate from this assignment.

## 10. Scope closure

Nothing is narrowed silently. Landed work is marked complete with receipts
(goal Ledger); moved work (U-3 codec spine, U-4, U-5, FHIR spec/mapping)
is cited to mail `08252f89` and the fhir-package-format receipts — moved,
not dropped. The completion contract for this goal is now FC-R1 + FC-R2 +
FC-R3 + GATE-FREEZE-2 disposition. Completing FC-R1 is not goal completion;
completing the ladder without the freeze disposition is not goal completion.

---

*Planning artifact only. 3 Hand units (FC-R1, FC-R2, FC-R3) plus
GATE-FREEZE-2; zero dispatchable today. Verified against faber `c6e36c9`
and radix main (policy receipt `c5f79ccbd`) on 2026-08-18.*
