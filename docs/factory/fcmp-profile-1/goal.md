# GOAL: FCMP profile 1 — freeze the canonical MessagePack profile after protocol review

**Status**: planned — draft spec committed; protocol review required before implementation
**Created**: 2026-08-17
**Campaign:** `—` (standalone)
**Source:** operator intake: `faber/docs/faber-messagepack-profile-v1.md` (commit `4878b6f`)
**Repos:** `faber` (spec + registry); `radix` (reference implementation — deferred, see Sequencing)
**Related:** `radix/docs/factory/radix-top-level-recomposition/goal.md` (hard sequencing dependency — operator flagged 2026-08-17); FHIR artifact format goal (unwritten, follows the freeze)

---

## Invariant

Faber durable artifacts (FHIR unit/package first) have exactly one byte-deterministic encoding across implementations and languages: the FCMP frame + canonical MessagePack rules in `faber/docs/faber-messagepack-profile-v1.md` §1–§12, admitted only by strict decoders.

## Problem

FHIR artifacts today serialize compiler structs through `rmp-serde` defaults (see the doc's non-normative reference): field order, enum ordinals, and map iteration order are implementation accidents, not a contract. There is no framing, no schema versioning, no limits, no canonical-byte admission. Cross-language consumers (TS first, per §11.3) cannot rely on bytes or hashes.

## Proposal

The draft profile (581 lines, 14 sections) supplies: 20-byte frame with magic/profile/payload-length; root map `kind`/`schema`/`value`; shortest-form canonical primitives (float64-only, NaN-normalized, map keys sorted by UTF-8 bytes); schema shapes (records, tagged unions, tuples, sorted logical maps/sets); strict admission (validate-then-reject, never normalize); per-kind resource limits; profile+document major/minor evolution with current-plus-two retention; format-owned wire DTOs (never compiler structs); SHA-256 frame digest as content identity; a document-kind registry owned by this repo; and a conformance ladder (generic vectors → document vectors → cross-language gate).

### Sequencing (operator ruling, 2026-08-17)

This goal's implementation **conflicts with radix-top-level-recomposition in flight**:

- FCMP's first consumers are FHIR unit/package artifacts — exactly the surfaces RTR1 (`AnalyzedUnit`→`AnalyzedModule`), RTR2 (`AnalyzedProgram` graph contract replaces the unit vector), and RTR3b (`radix-package`→`radix-program`) rewrite.
- Wire DTOs written against the current vector contract would be rewritten on RTR2 landing.
- **Gate:** FCMP implementation units dispatch only after RTR2 lands (contract stability), and file into final crate homes after RTR3b. The protocol review phase has no code surface and runs now.

### Non-goals

- No implementation while the profile is draft (doc §14: review gates before freeze).
- No generic framework before a second consumer exists (doc Abstract).
- No FMIR codec changes (doc Non-goals).
- No FHIR schema definition inside this goal — the freeze reserves kinds; schemas come with the FHIR format goal.

## Units (lowering sketch — refine via `$delivery`)

| Unit | Scope | Depends on | Hand evidence |
| --- | --- | --- | --- |
| 1 | Head protocol review of the draft: §14 gate list + BCP-14 clause audit + conflict-of-interest check against serializer-library defaults | — | none |
| 2 | Fold review verdicts into the spec; flip Status to frozen 1.0; register `fhir.unit`/`fhir.package` kinds in the registry section | 1 | none |
| 3 | Rust reference: strict FCMP codec + generic profile vectors (frame, canonicality, limits) | 2, RTR2 | none |
| 4 | FHIR unit/package wire DTOs + fixtures + TS independent decoder/encoder (cross-language gate §11.3) | 3, RTR3b | none |
| 5 | FHIR writer/reader switch + legacy postcard rejection with structured diagnostic | 4 | none |

## Validation

Unit 1–2: review memo + frozen spec commit. Unit 3: generic conformance vectors all green in Rust. Unit 4: TS implementation re-encodes every positive fixture byte-identically; shared negative fixtures rejected with expected error classes. Unit 5: FHIR e2e on FCMP only; postcard rejected closed.

## Ledger

| Unit | Status | Receipt | Notes |
| --- | --- | --- | --- |
| 1 | pending | — | review dispatch queued this session |

## Open questions

1. Review verdict pending on: named string fields vs numeric field IDs; float64 sufficiency for FHIR's numeric domain; the no-flags 20-byte frame. Defaults live in the draft; the review may overturn them.
