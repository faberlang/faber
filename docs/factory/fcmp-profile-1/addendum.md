# Operator addendum — product is HIR package serialization, not envelope freeze

**Status**: operator ruling — sequencing restructure required; not a freeze; not implementation
**Date**: 2026-08-18
**Goal**: `docs/factory/fcmp-profile-1/goal.md` (`gol_58147dc9ef95b704`)
**Authority**: operator, this session
**Audience**: managing session (Mind) — restructure lowering, delivery, and freeze order from this addendum
**Do not**: treat a FCMP 1.0 freeze as the next product action; dispatch unit 3 as a generic codec; invent FHIR DTOs that drop today's analyzed-HIR snapshot; treat `HirArtifact` as disposable "migration input" without a replacement that rehydrates the same structure

This addendum is operator opinion distilled for replanning. It does not edit
`docs/faber-messagepack-profile-v1.md`. It does not freeze. It does not
narrow admitted end-state (durable FHIR packages in MessagePack). It
changes **what gets specified first** and **what "approve the protocol"
is allowed to mean**.

---

## Product

The job is a distributable, semi-compiled library payload.

Take Norma, Gradus, or any Faber library. Analyze it to HIR. Save that
analyzed HIR as a package. Zip it. Load it in a later session. Rebuild
the same internal compiler structure. Lower or emit from that load
without re-parsing source as the semantic authority.

MessagePack replaces postcard as the codec that wraps that payload and
makes it parsable. That is the whole reason this work exists.

Live code already does the save/load job in postcard:

- unit: `radix-hir-fhir` `HirArtifact` + `encode` / `decode`
- package: `FhirPackage` / `encode_package` / `decode_package` (`.fhirpkg`)
- reconstruction: radix driver rebuilds `AnalyzedModule` from the snapshot

The missing contract is **how that snapshot is written in MessagePack**
so decode yields a structurally equal artifact and the same analyzed
program. It is not a new shipping label in search of a schema.

## What is not the job

Canonical Faber emit is already supposed to be stable on the second
pass: source → HIR → Faber (same locale) → HIR → Faber, with the second
emit byte-identical to the first. Inconsistencies in author source are
removed on the first pass. That proves HIR is a stable semantic core.
It is **not** the package problem.

The package does not have to round-trip original author bytes. It has
to round-trip **analyzed HIR**. FHIR is the envelope for that HIR, not
a reprint of `.fab`.

Locale respelling is a reader-surface feature of HIR. Same-locale
canonical emit is the source-side proof. Package load must not require
going back through Faber source.

## Disconnect

The current goal and protocol specify a generic FCMP envelope
(20-byte `FABERMP\0` frame, root `{kind, schema, value}`, canonical
MessagePack, omit-defaults, limits, version windows) and **explicitly
refuse** to specify HIR serialization.

Quoted from this goal's non-goals and delivery:

- no FHIR schema definition inside this goal; names reserved only
- no postcard → rmp-serde swap
- current `HirArtifact` / vector-era envelopes are "migration input,
  not DTO templates"
- concrete field lists wait on an unwritten FHIR format goal
- implementation waits on a separate profile freeze

That inverts the product. The envelope is a framing detail around a
HIR schema. Approving FCMP 1.0 as written does not get a Norma package.
It freezes rules for a `value` nobody has defined.

The 2026-08-14 portable-format note stated the actual codec job: pin
how each existing FHIR schema type is encoded in MessagePack. This
goal rejected that path. That rejection is the defect to restructure,
not a lock the operator is affirming.

## Losslessness (the metric)

Load of a FHIR package must rehydrate the analyzed program with no
semantic confusion: items, types, imports, facts, presentation that
HIR already carries.

In particular:

- Numeric **width** and tensor **dtype/shape** are HIR type-table
  facts (`numerus`/`fractus` widths, `Type::Tensor(elem, shape)`,
  buffers/`octeti`). They MUST appear in the FHIR `value`. They MUST
  NOT be inferred from MessagePack opcodes.
- FCMP "float fields use float64" is a rule for an IEEE `f64`
  carrier on the wire. It is not permission to crunch `f32` / `f16`
  tensors, integer widths, or buffer payloads into one float.
- Buffer payloads are binary (or an equivalent exact byte sequence)
  plus schema-stated dtype and dimensionality. Not an array of
  generic floats.
- Comments already stored on `HirPresentation` (line-start, legal
  attachments) travel with the unit. Canonical emit from the loaded
  HIR must remain as capable as emit from in-session HIR.

If a format-owned DTO cannot reconstruct today's `HirArtifact` →
`AnalyzedModule` path, it is not a FHIR schema. It is a lossy export.

## Restructure (what Mind must change)

1. **Specify the cargo first.** Next planning artifact is the
   unit/package field spec: every `HirArtifact` / `FhirPackage` fact
   family as named MessagePack, including how `TypeId` / `HirId` /
   `Symbol` arenas rehydrate. Live postcard types are the source of
   truth until a replacement is proven equivalent.
2. **Envelope sits under that spec.** Magic, kind, schema version,
   fail-closed load, and canonical primitive rules are real, but they
   are not the thing to freeze or implement in isolation. Do not ask
   the operator to "approve the protocol" while `value` is empty.
3. **Do not park HIR behind an unwritten goal** as if that were
   sequencing hygiene. If FHIR schema work needs its own goal, write
   it **now** and make this profile a dependency of it, not the other
   way around. Envelope-only FCMP is not a shippable increment.
4. **Keep the postcard snapshot honest.** A DTO rewrite is allowed
   only as a complete projection of the current analyzed-HIR snapshot
   (post-RTR module / `AnalyzedProgram` graph included). "Migration
   input, not a template" is not a license to drop node kinds,
   presentation, type widths, or package import links.
5. **Proof.** Encode live `HirArtifact` / `FhirPackage` → MessagePack
   → decode → structurally equal snapshot → reconstructed analyzed
   program equivalent to the postcard path. Then, and only then,
   switch writers and reject postcard as `legacy`. Generic FCMP
   vectors without a FHIR `value` do not prove the product.
6. **Freeze order.** Operator freeze of FCMP 1.0 is not in front of
   the HIR field spec. A framing profile may stay draft until the
   first consumer schema exists to bind it.

## What this addendum does not reopen

- FHIR remains the portable artifact; FMIR remains the executable
  image. No FMIR codec change.
- Postcard FHIR stays the live wire until a proven MessagePack path
  replaces it. No dual writer, no silent fallback.
- Canonical MessagePack (named fields, one encoding per schema value,
  strict reject) is acceptable **as the codec law for that HIR
  schema**. It is not a substitute for the schema.
- RTR2 / RTR3b have landed. Do not keep "wait for RTR" as a reason
  to specify an empty `value`.

## Managing-session action

Re-lower this goal (or split a FHIR-format goal immediately) so the
critical path is:

```text
HIR/package field spec (live HirArtifact / FhirPackage)
  → MessagePack encoding of that spec (FCMP rules applied to it)
    → reference codec + round-trip vs postcard snapshot
      → writer/reader switch, postcard rejected
```

Stop treating "freeze the envelope, implement a generic `radix-fcmp`,
wait for an unwritten FHIR goal" as the plan.
