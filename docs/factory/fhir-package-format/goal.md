# GOAL: FHIR package format — the HIR cargo spec

**Status**: planned — minted 2026-08-18 from operator addendum ([`../fcmp-profile-1/addendum.md`](../fcmp-profile-1/addendum.md)); not lowered; the critical-path first artifact
**Created**: 2026-08-18
**Campaign:** `—` (standalone; fcmp-profile-1 becomes a dependency of this goal, not the reverse)
**Source:** operator addendum 2026-08-18 — the product is HIR package serialization; the envelope is a framing detail around this schema
**Repos:** `radix` (radix-hir-fahir: `HirArtifact`, `FhirPackage`, the postcard encode/decode path), `faber` (format docs)
**Related:**
- [`../fcmp-profile-1/addendum.md`](../fcmp-profile-1/addendum.md) — the authority this goal executes
- [`../../docs/faber-messagepack-profile-v1.md`](../../docs/faber-messagepack-profile-v1.md) — envelope rules; stay draft until this schema binds a `value`

---

## Invariant

A FHIR package load rehydrates the analyzed program with **no semantic
confusion**: items, types, imports, facts, and presentation that HIR carries.
Numeric width and tensor dtype/shape are type-table facts in the `value`, never
inferred from MessagePack opcodes. If a format DTO cannot reconstruct today's
`HirArtifact` → `AnalyzedModule` path, it is not a FHIR schema — it is a lossy
export.

## Problem

Live code already saves/loads analyzed HIR in postcard (`HirArtifact`
encode/decode, `FhirPackage` `.fhirpkg`, driver reconstruction of
`AnalyzedModule`). The missing contract is how that snapshot is written in
MessagePack so decode yields a structurally equal artifact and the same
analyzed program. The prior plan froze an envelope around an undefined
`value`; the operator blocked that sequencing.

## Proposal

Specify the cargo first:

1. **Field spec** — every `HirArtifact` / `FhirPackage` fact family as named
   MessagePack, including `TypeId` / `HirId` / `Symbol` arena rehydration.
   Live postcard types are the source of truth until a replacement is proven
   equivalent.
2. **MessagePack encoding of that spec** — FCMP envelope rules applied to it.
3. **Reference codec + round-trip proof** — encode live artifact → decode →
   structurally equal snapshot → reconstructed program equivalent to the
   postcard path.
4. **Writer/reader switch** — then, and only then, reject postcard as legacy.

A DTO rewrite is allowed only as a complete projection of the current
analyzed-HIR snapshot (post-RTR module / `AnalyzedProgram` graph included):
node kinds, presentation, type widths, package import links all travel.

## Non-goals

- Envelope framing rules (owned by fcmp-profile-1, which stays draft and
  becomes this goal's dependency).
- FMIR / executable image changes.
- Round-tripping author source bytes (canonical-emit stability already proves
  HIR is the semantic core; the package round-trips analyzed HIR).
- Dual writers or silent fallback: postcard stays the live wire until the
  proven switch.

## Units (lowering sketch)

| Unit | Scope |
| --- | --- |
| FS-1 | Inventory + field spec: every postcard-carried fact family in `HirArtifact`/`FhirPackage` named, with arena rehydration rules |
| FS-2 | MessagePack mapping of the spec (FCMP rules applied to a defined `value`) |
| FS-3 | Reference codec + round-trip vs postcard snapshot |
| FS-4 | Writer/reader switch; postcard rejected as legacy |

## Validation

Round-trip proof per the addendum: loaded HIR emits canonical Faber as capable
as in-session HIR; structural equality on the snapshot; program equivalence to
the postcard path.

## Release posture

Ships with a Faber cut; not standalone.
