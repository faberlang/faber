# GOAL: FCMP profile 1 — envelope rules, registry, and conformance (draft; consumer-gated)

**Status**: planned — re-scoped 2026-08-18 after the FCMP restructure; protocol doc units landed (U-1/2/2a, PRE-1, PRE-2); remainder gated on fhir-package-format FS-3; freeze withdrawn as blocked-by-design
**Created**: 2026-08-17
**Re-scoped**: 2026-08-18 (task `97367613`; restructure mail `08252f89`)
**Campaign:** `—` (standalone)
**Source:** operator intake: `faber/docs/faber-messagepack-profile-v1.md` (amended from commit `4878b6f`; review memo `604d6a30`; CTO `5a4e974a`); operator addendum 2026-08-18; restructure mail `08252f89`
**Repos:** `faber` (protocol + registry + conformance fixtures); `radix` (reference codec — implemented under fhir-package-format, not here)
**Related:**
- `../fhir-package-format/goal.md` — the consumer goal this profile now serves; FCMP is its dependency, not the reverse (mail `08252f89`)
- [`addendum.md`](addendum.md) (2026-08-18) — the operator restructure authority
- [`amendment.md`](amendment.md) (U-2a fold authority; do not reopen unit 2)
**Amendment spec:** `docs/factory/fcmp-profile-1/amendment.md`
**Operator addendum:** `docs/factory/fcmp-profile-1/addendum.md` (2026-08-18) — product is HIR package serialization; envelope-first freeze is the disconnect this re-scope absorbs

---

## Invariant

Faber durable artifacts (FHIR unit/package first) have exactly one byte-deterministic encoding across implementations and languages: the FCMP frame + canonical MessagePack rules in `faber/docs/faber-messagepack-profile-v1.md` §1–§12, admitted only by strict decoders.

## Problem

The profile rules are written and folded (identity (a), prefix 64/3/2/256,
kind-reuse `MUST NOT`, `fcmp.test` reservation, §5.1 error classes, §14 gate
list), but the profile is still **draft with no registered consumer**: the
`value` it frames was undefined until fhir-package-format FS-2 bound one
(`93f5228`), and the two FHIR kinds remain reserved because no concrete finite
per-kind limits are published (protocol §6/§10: registration without limits
MUST fail). The 2026-08-18 operator restructure ruled that freezing or
implementing the envelope in isolation — ahead of the HIR cargo it frames —
inverts the product (addendum §Disconnect; mail `08252f89`). What remains of
this goal is the profile's own surface: registry publication once the
consumer publishes limits, the profile conformance ladder (§11.1 generic
vectors, §11.3 independent cross-language codec), custody of the protocol
document, and the re-formed freeze gate.

## Proposal

This goal owns the FCMP profile document and its conformance claims; it does
not own the FHIR cargo or the reference codec implementation:

1. **Custody (standing).** `docs/faber-messagepack-profile-v1.md` stays draft
   here. Envelope-rule gaps surfaced by fhir-package-format FS-3/FS-4
   implementation fold into the protocol under this goal, routed by Mind.
2. **FC-R1 — registration.** When fhir-package-format publishes concrete
   finite per-kind limits (expected with FS-3), register `fhir.unit` and
   `fhir.package` in protocol §10 with limits, owning-schema path, window,
   and fixture-manifest path. FS-2 deliberately did not invent those numbers.
3. **FC-R2 — generic profile vectors (§11.1).** The profile's own conformance
   rung: canonical-byte vectors (positives + one negative per reachable §5.1
   class) under `faber/docs/fcmp/vectors/generic/`, proven by conformance
   tests riding the FS-3 reference codec (crate home per fhir-package-format
   delivery). `fcmp.test` may be admitted by the conformance decoder only;
   product decoders never.
4. **FC-R3 — independent TypeScript codec (§11.3).** The cross-language leg
   of the invariant: an independent TS implementation re-encodes every shared
   positive fixture byte-identically and rejects shared negatives with the
   same §5.1 class. No shared encoder source with Rust.
5. **GATE-FREEZE-2 (not a Hand).** Head §14 re-check plus a separate operator
   freeze decision, re-openable only after a consumer schema is registered
   with limits (addendum rule 6). The 2026-08-18 freeze ask was withdrawn as
   blocked-by-design (mail `08252f89`); no operator decision is currently
   open.

### Restructure (2026-08-18) — what moved and what stayed

Dependency inverted by mail `08252f89`: fhir-package-format is the consumer
goal; this profile is its dependency.

| Former scope | Disposition | Receipt |
| --- | --- | --- |
| U-1 head-cto protocol review | landed | memo `604d6a30` |
| U-2 fold review verdicts | landed | `cfcb44be` |
| U-2a identity + prefix fold | landed | `02327f9` |
| U-PRE-1 policy window (delivery v1) | landed | radix `c5f79ccbd` |
| U-PRE-2 protocol residuals (delivery v1) | landed | `0c65e72`, `04cda43`, `c85d807` |
| FHIR field spec / MessagePack mapping | moved to fhir-package-format FS-1/FS-2 | `3dfc219`, `93f5228` |
| U-3 strict FCMP codec (generic dispatch) | absorbed by fhir-package-format FS-3 — never dispatched standalone (addendum: do not dispatch unit 3 as a generic codec) | — |
| U-4 FHIR DTOs + document vectors | moved to fhir-package-format (FS-3 chain) | mail `08252f89` |
| U-5 writer/reader switch + postcard reject | moved to fhir-package-format FS-4 | mail `08252f89` |
| GATE-FREEZE (freeze-first) | withdrawn — blocked-by-design | mail `08252f89` |

Retained here: protocol custody, kind registration (FC-R1), §11.1 generic
vectors (FC-R2), §11.3 TS cross-language codec (FC-R3), the freeze gate
(GATE-FREEZE-2). RTR2/RTR3b have landed (RTR2 `55a7b72a9` / merge
`ddc0810f1`; RTR3b `eb24be9ff` / merge `a8a9055ee`); the old "conflicts with
RTR in flight" sequencing is closed.

### Non-goals

- No FHIR schema, field spec, mapping, DTO, document vectors, or
  writer/reader switch — all owned by fhir-package-format (FS-1…FS-4).
- No standalone generic reference codec dispatch; the codec arrives with
  fhir-package-format FS-3 against the FS-2-bound value.
- No freeze while the profile has no registered consumer kind (addendum
  rule 6). Status stays draft until GATE-FREEZE-2.
- No generic framework before a second consumer exists (doc Abstract).
- No FMIR codec changes (doc Non-goals). No postcard → rmp-serde swap.
- Do not reopen U-2/U-2a.

## Units (lowering sketch — lowered in [`delivery.md`](delivery.md))

| Unit | Scope | Depends on | Hand evidence |
| --- | --- | --- | --- |
| FC-R1 | register `fhir.unit`/`fhir.package` in protocol §10 with finite limits | fhir-package-format FS-3 limits publication | none |
| FC-R2 | §11.1 generic vectors + conformance tests on the FS-3 codec | fhir-package-format FS-3 codec | none |
| FC-R3 | independent TS codec over shared fixtures (§11.3) | FC-R2; FHIR fixtures from fhir-package-format | none |
| GATE-FREEZE-2 | Head §14 re-check + operator freeze | FC-R1 (minimum); ladder evidence | not a Hand |

## Validation

FC-R1: registration rows present with concrete finite limits; §6/§10
registration-without-limits rule intact; `fcmp.test` still reserved.
FC-R2: every §11.1 bullet has a fixture; positives re-encode byte-identically
with recorded SHA-256; negatives hit the locked §5.1 class. FC-R3: §11.3
holds on the shared fixtures for generic + FHIR vectors, byte-identical
re-encode, same-class rejects, no shared encoder source. Goal close: all
three landed and the freeze decision recorded (either frozen, or explicitly
re-blocked by the operator with cause).

## Ledger

| Unit | Status | Receipt | Notes |
| --- | --- | --- | --- |
| 1 | complete | `604d6a30` | head-cto review verdicts received |
| 2 | complete | `cfcb44be` | amendments folded; do not reopen |
| 2a | complete | `02327f9` | identity + prefix 64/3/2/256 folded; not a freeze |
| PRE-1 | complete | radix `c5f79ccbd` | policy.md carries FCMP current-plus-two window |
| PRE-2 | complete | `0c65e72`, `04cda43`, `c85d807` | kind-reuse MUST NOT, `fcmp.test` reserved, §5.1 error classes folded |
| 3 (codec) | moved | — | absorbed by fhir-package-format FS-3; never dispatched standalone |
| 3 (vectors) | gated | — | re-minted as FC-R2; waits on FS-3 codec |
| 4 | moved | — | FHIR DTOs/vectors → fhir-package-format; TS profile gate retained as FC-R3 |
| 5 | moved | — | writer/reader switch → fhir-package-format FS-4 |
| GATE-FREEZE | withdrawn | mail `08252f89` | blocked-by-design; re-formed as GATE-FREEZE-2 |
| FC-R1 | gated | — | waits on FS-3 limits publication |
| FC-R2 | gated | — | waits on FS-3 codec |
| FC-R3 | gated | — | waits on FC-R2 + FHIR fixtures |
| GATE-FREEZE-2 | gated | — | §14 re-check + operator freeze after FC-R1 |

## Review disposition

U-1/U-2 dispositions stand as folded (named string fields; float64 +
canonical NaN per schema; flagless 20-byte frame; profile versions separate
from product semver; kinds reserved until limits publish; BCP-14 fold).
CTO `5a4e974a` holes closed by U-2a (`02327f9`). CTO `0dc52153` freeze
blockers closed by PRE-1 (`c5f79ccbd`) and PRE-2 (`0c65e72`…`c85d807`).
The freeze-first sequencing those receipts fed was then withdrawn by the
operator restructure (mail `08252f89`); freeze returns only through
GATE-FREEZE-2 after registration.

## Open questions

1. **Freeze trigger.** Default: GATE-FREEZE-2 may be re-raised once FC-R1
   lands; the operator may additionally wait for the full ladder (FC-R3).
   Operator's call at the time; no ask is open now.
2. **`fcmp.test` registration.** Default: stay reserved; FC-R2 admits it in
   the conformance decoder only if the suite needs a kind string.
3. **TS runner policy.** Default: FC-R3 records the runner it adds; no new
   Node/Bun product toolchain (Faber tooling law).

## Stop conditions

- Stop and route to Mind if fhir-package-format FS-3 publishes limits that
  contradict the draft envelope rules (protocol amend under custody, not a
  silent FS-side fork).
- Stop if FC-R2/FC-R3 would require weakening a §5.1 class or admitting
  noncanonical bytes to go green.
