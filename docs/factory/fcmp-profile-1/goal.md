# GOAL: FCMP profile 1 — amend the draft before a separate freeze decision

**Status**: planned — draft; U-2a landed `02327f9`; not frozen; operator addendum 2026-08-18 requires sequencing restructure (HIR/package schema first; envelope freeze is not the next action)
**Created**: 2026-08-17
**Campaign:** `—` (standalone)
**Source:** operator intake: `faber/docs/faber-messagepack-profile-v1.md` (amended from commit `4878b6f`; review memo `604d6a30`; CTO `5a4e974a`)
**Repos:** `faber` (spec + registry); `radix` (reference implementation — deferred, see Sequencing)
**Related:** `radix/docs/factory/radix-top-level-recomposition/goal.md` (hard sequencing dependency — operator flagged 2026-08-17); FHIR artifact format goal (unwritten; addendum says write it now or fold HIR schema into this goal)
**Amendment spec:** `docs/factory/fcmp-profile-1/amendment.md`
**Operator addendum:** `docs/factory/fcmp-profile-1/addendum.md` (2026-08-18) — product is HIR package serialization; envelope-first freeze is the disconnect to restructure

---

## Invariant

Faber durable artifacts (FHIR unit/package first) have exactly one byte-deterministic encoding across implementations and languages: the FCMP frame + canonical MessagePack rules in `faber/docs/faber-messagepack-profile-v1.md` §1–§12, admitted only by strict decoders.

## Problem

FHIR artifacts today serialize mixed compiler structs through postcard + serde
(`radix-hir-fhir` `decode.rs` / `package.rs`: `postcard::to_allocvec` /
`from_bytes`). Field order, enum ordinals, and map iteration order are
serializer accidents, not a contract. A u32 ratchet exists (`SCHEMA_VERSION =
3` on the unit, `PACKAGE_SCHEMA_VERSION = 1` on the envelope) and rejects
mismatch with no compatibility decoder; that is not FCMP schema versioning (no
framing, no named fields, no canonical-byte admission, no limits, no
independent profile/document majors). Cross-language consumers (TS first, per
§11.3) cannot rely on bytes or hashes. The live wire is not rmp-serde; the
protocol's rmp-serde citation is non-normative evidence of a rejected path.

## Proposal

The amended draft profile supplies: a pinned MessagePack base revision; a 20-byte
frame with magic/profile/payload-length; root and record maps with REQUIRED
named string fields; shortest-form canonical primitives (float64-only with
canonical NaN for current Faber FHIR `f64` carriers, and map keys sorted by
UTF-8 bytes); schema shapes (records, tagged unions, tuples, sorted logical
maps/sets); strict admission (validate-then-reject, never normalize); finite
per-kind resource limits; profile and document major/minor evolution kept
separate from Faber product semver; explicit profile-major migration and
release-boundary rules; a per-kind current-plus-two document-major support
window tied to release policy; format-owned wire DTOs (never compiler
structs); SHA-256 frame digest as content identity; a document-kind registry
owned by this repo; and a conformance ladder (generic vectors → document
vectors → cross-language gate).

`fhir.unit` remains a reservation for the post-RTR2 module contract: a stable
module ID/record with schema-defined contents and import edges. `fhir.package`
remains a reservation for the post-RTR2/RTR3b `AnalyzedProgram` graph with
explicit roots, optional entry, stable module records, import edges, library
identities, and graph-specific limits. A kind MUST NOT be registered without
its owning schema's concrete finite limits.

### Sequencing (operator ruling, 2026-08-17)

This goal's implementation **conflicts with radix-top-level-recomposition in flight**:

- FCMP's first consumers are FHIR unit/package artifacts whose schemas depend
  on the post-RTR2 module contract and the post-RTR2/RTR3b `AnalyzedProgram`
  graph contract.
- Wire DTOs written against the current vector-era surface would be rewritten
  when those contracts land. The current implementation shape is migration
  input, not the FCMP semantic schema.
- **Gate:** FCMP implementation units dispatch only after RTR2 lands (module
  contract stability), and file into final crate homes after RTR3b (analyzed
  program graph stability). The protocol review and amendment phase has no code
  surface and runs now.

### Draft locks (CTO `5a4e974a`; planner `3fc06e37`)

These close the two freeze holes. They are not a freeze. Full BCP-14 fold-in
text is in `amendment.md`.

1. **Optional vs default identity — (a).** Absence of an OPTIONAL field and a
   present encoding equal to that field's schema default are the same schema
   value. Encoders MUST omit defaults. Present-at-default is `noncanonical`.
   `nil` stays distinct and is never the omitted-optional encoding. OPTIONAL
   fields MUST NOT use implicit-null as the default; nullable types are
   REQUIRED fields that always carry the key.
2. **Envelope-prefix limits.** Before kind admission: kind-string ≤ 64 bytes;
   root map exactly 3 entries; schema array exactly 2 elements. A decoder with
   no enabled kind MUST reject a declared payload length > 256 bytes and MUST
   NOT allocate `value`. Root keys are read in canonical order so `value` is
   not allocated to learn `kind`.

A later §14 re-check after these land in the protocol file is still not a
freeze.

### Non-goals

- No implementation while the profile is draft (doc §14: review gates before freeze).
- No freeze in this amend. Status stays planned/draft.
- No generic framework before a second consumer exists (doc Abstract).
- No FMIR codec changes (doc Non-goals).
- No FHIR schema definition inside this goal — this draft reserves names only;
  schemas and registration come with the FHIR format goal after the RTR2/RTR3b
  contracts and publication of concrete finite limits.
- No postcard → rmp-serde swap.
- Do not reopen unit 2. Do not dispatch unit 3.

## Units (lowering sketch — refine via `$delivery`)

| Unit | Scope | Depends on | Hand evidence |
| --- | --- | --- | --- |
| 1 | Head protocol review of the draft: §14 gate list + BCP-14 clause audit + conflict-of-interest check against serializer-library defaults | — | memo `604d6a30` |
| 2 | Fold the review verdicts into the draft; keep Status draft, preserve the RTR2/RTR3b gate, and keep `fhir.unit`/`fhir.package` as reservations until schemas publish concrete finite limits | 1 | handle `cfcb44be` |
| 2a | Fold `amendment.md` into `docs/faber-messagepack-profile-v1.md` (identity + prefix limits only). Status stays draft. Not a freeze. | 2 | `02327f9` |
| 3 | Rust reference: strict FCMP codec + generic profile vectors (frame, canonicality, limits) | separate profile freeze, RTR2 | none |
| 4 | FHIR unit/package wire DTOs + fixtures + TS independent decoder/encoder (cross-language gate §11.3) | 3, RTR3b | none |
| 5 | FHIR writer/reader switch + legacy postcard rejection with structured diagnostic | 4 | none |

## Validation

Unit 1–2: review memo + amended draft commit, with Status still draft and the
review gate re-checked before any freeze decision. Unit 2a: protocol file
contains the identity rule and the four prefix limits; Status still draft.
A §14 re-check after 2a is still not a freeze. Unit 3: generic conformance
vectors all green in Rust. Unit 4: TS implementation re-encodes every positive
fixture byte-identically; shared negative fixtures rejected with expected error
classes. Unit 5: FHIR e2e on FCMP only; postcard rejected closed.

## Ledger

| Unit | Status | Receipt | Notes |
| --- | --- | --- | --- |
| 1 | complete | `604d6a30` | head-cto review verdicts received |
| 2 | complete | `cfcb44be` | amendments folded; do not reopen |
| 2a | complete | `02327f9` | identity + prefix 64/3/2/256 folded; not a freeze |
| 3 | blocked | — | freeze + RTR2; do not dispatch |

## Review disposition

The head-cto review is folded into the draft and closes the prior open questions:

1. Records and roots REQUIRE named string fields; numeric IDs are not an
   alternate record representation.
2. Float64 with canonical NaN covers current Faber FHIR `f64` carriers, while
   each future FHIR schema MUST define numeric semantics and NaN/infinity
   admission.
3. The flagless 20-byte frame remains the FCMP 1.0 design; adding in-frame
   flags is a profile-major change.
4. Profile versions remain separate from Faber product semver. Newer-minor
   rejection, profile-major migration boundaries, and the per-kind
   current-plus-two document-major window are explicit.
5. `fhir.unit` and `fhir.package` are reservations only until their schemas
   publish concrete finite limits; registration without limits fails.
6. Lowercase normative clauses were made explicit BCP-14 requirements, and the
   FHIR kinds are tied to the post-RTR2 module and post-RTR2/RTR3b analyzed-
   program graph contracts.

The draft MUST be re-checked against §14 before a separate freeze decision.
That re-check is still not a freeze.

CTO `5a4e974a` (`correct_before_next_phase`): optional/default identity and
envelope-prefix limits were unset. They are folded as (a) omit-defaults and
the 64 / 3 / 2 / 256 prefix table in the protocol file (`02327f9`). Do not
freeze here. Do not reopen unit 2. Do not dispatch unit 3.

## Operator addendum (2026-08-18)

Operator ruling in [`addendum.md`](addendum.md). Product is save/load of
analyzed HIR as a distributable library package (Norma, Gradus); MessagePack
replaces postcard. The envelope profile is not the thing to freeze or
implement first. Managing session must re-lower so the HIR/package field spec
(live `HirArtifact` / `FhirPackage`) is the critical path. Do not dispatch
unit 3 as a generic codec. Do not ask for a FCMP 1.0 freeze while `value` is
undefined.
