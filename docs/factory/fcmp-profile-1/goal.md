# GOAL: FCMP profile 1 — amend the draft before a separate freeze decision

**Status**: planned — amendments folded into the draft; protocol review re-check required before implementation or freeze
**Created**: 2026-08-17
**Campaign:** `—` (standalone)
**Source:** operator intake: `faber/docs/faber-messagepack-profile-v1.md` (amended from commit `4878b6f`; review memo `604d6a30`)
**Repos:** `faber` (spec + registry); `radix` (reference implementation — deferred, see Sequencing)
**Related:** `radix/docs/factory/radix-top-level-recomposition/goal.md` (hard sequencing dependency — operator flagged 2026-08-17); FHIR artifact format goal (unwritten, follows the freeze)

---

## Invariant

Faber durable artifacts (FHIR unit/package first) have exactly one byte-deterministic encoding across implementations and languages: the FCMP frame + canonical MessagePack rules in `faber/docs/faber-messagepack-profile-v1.md` §1–§12, admitted only by strict decoders.

## Problem

FHIR artifacts today serialize compiler structs through `rmp-serde` defaults (see the doc's non-normative reference): field order, enum ordinals, and map iteration order are implementation accidents, not a contract. There is no framing, no schema versioning, no limits, no canonical-byte admission. Cross-language consumers (TS first, per §11.3) cannot rely on bytes or hashes.

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

### Non-goals

- No implementation while the profile is draft (doc §14: review gates before freeze).
- No generic framework before a second consumer exists (doc Abstract).
- No FMIR codec changes (doc Non-goals).
- No FHIR schema definition inside this goal — this draft reserves names only;
  schemas and registration come with the FHIR format goal after the RTR2/RTR3b
  contracts and publication of concrete finite limits.

## Units (lowering sketch — refine via `$delivery`)

| Unit | Scope | Depends on | Hand evidence |
| --- | --- | --- | --- |
| 1 | Head protocol review of the draft: §14 gate list + BCP-14 clause audit + conflict-of-interest check against serializer-library defaults | — | memo `604d6a30` |
| 2 | Fold the review verdicts into the draft; keep Status draft, preserve the RTR2/RTR3b gate, and keep `fhir.unit`/`fhir.package` as reservations until schemas publish concrete finite limits | 1 | handle `cfcb44be` |
| 3 | Rust reference: strict FCMP codec + generic profile vectors (frame, canonicality, limits) | separate profile freeze, RTR2 | none |
| 4 | FHIR unit/package wire DTOs + fixtures + TS independent decoder/encoder (cross-language gate §11.3) | 3, RTR3b | none |
| 5 | FHIR writer/reader switch + legacy postcard rejection with structured diagnostic | 4 | none |

## Validation

Unit 1–2: review memo + amended draft commit, with Status still draft and the
review gate re-checked before any freeze decision. Unit 3: generic conformance
vectors all green in Rust. Unit 4: TS implementation re-encodes every positive
fixture byte-identically; shared negative fixtures rejected with expected error
classes. Unit 5: FHIR e2e on FCMP only; postcard rejected closed.

## Ledger

| Unit | Status | Receipt | Notes |
| --- | --- | --- | --- |
| 1 | complete | `604d6a30` | head-cto review verdicts received |
| 2 | complete | `cfcb44be` | amendments folded; draft remains unfrozen pending review re-check |

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
