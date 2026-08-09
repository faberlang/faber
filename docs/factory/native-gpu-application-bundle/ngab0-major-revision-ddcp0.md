# NGAB0 Major Revision Record — DDCP0 (NGAB0-R1)

**Packet**: `ngab0-composite-contract.md` (NGAB0 composite artifact and
ownership contract).
**Revision id**: **NGAB0-R1** — MAJOR revision under the §Versioning bump
semantics.
**Date**: 2026-08-08 (DDCP0 admission; council review 2026-08-08).
**Recorded by**: DDCP0-U4 (H1 — the NGAB1-HOLD unlock). The NGAB amendment
write set is **single-writer = DDCP0** for the compiler-facts surfaces
(§Partition, §Abi, §Manifest, §Verification, §FrozenVsReserved, §Versioning);
the paired DDPP0 campaign consumes this revision at its field-by-field
agreement gate and may add product-side clauses to the same packet only after
this revision lands — never a silent second edit of the same revision.
**Prior state**: NGAB0-U2–U7 frozen packet (initial acceptance;
`manifest-1.0.0`; one-call/one-kernel ABI; SHA-256 byte-only `artifact_id`).

## Reason

Call meaning and manifest identity change — both frozen-now facts:

1. **Call meaning**: one host function previously invoked exactly one device
   kernel. Under this revision, one host function invokes **one prepared
   submission region containing one or more kernels** through the versioned
   typed boundary (§Partition/§Abi). The one-kernel case remains the minimal
   prepared region, not a separate ABI shape (§FrozenVsReserved).
2. **Manifest identity**: the manifest's byte-only `artifact_id` is re-roled
   as `content_sha256` (SHA-256 over canonical decoded payload bytes only),
   and the manifest gains the `packet_sha256` packet/admission identity and
   the `compiler_input_packet_sha256` parent-provenance row for finalized
   packets (§Manifest/§Verification). Identity domains follow the shared
   DDCP0/DDPP0 normative table (DDCP0 §IdentityDomains; DDPP0 §IdentityDomains
   — field-for-field the same six domains).

Both changes are **major** under the §Versioning bump semantics (meaning
change + manifest schema change): every consumer re-validates under the new
revision and the admitted manifest version is re-pinned.

## Authority sign-off

Recorded under the joint PML/NGAB packet authority (packet §Versioning,
"Version authority (named owner)").

> **Sign-off (NGAB0-R1)** — admitted by the joint PML/NGAB packet authority:
> the **PML campaign Mind** and the **NGAB campaign Mind** acting together;
> the **operator** is the binding decision owner for disputed bumps. This
> major revision is recorded as NGAB0 packet revision **NGAB0-R1**. DDCP0
> (compiler facts) is the single-writer that recorded it; DDPP0 (product
> side) consumes it.

## Change scope

| § | Amendment |
| --- | --- |
| Partition | one host function invokes **one prepared submission region containing one or more kernels** through the versioned typed boundary |
| Abi | same call meaning — the region is the call's submission unit, not the kernel |
| Manifest | `artifact_id` → `content_sha256` (canonical payload bytes only); new `packet_sha256` row; new `compiler_input_packet_sha256` parent-provenance row for finalized packets; manifest schema re-pinned `manifest-1.0.0` → `manifest-2.0.0` |
| Verification | packet identity admission (`packet_sha256`) verified **before backend selection**, after per-artifact `content_sha256` verification |
| FrozenVsReserved | the one-kernel fixture remains the **minimal case, not the ABI limit** |
| Versioning | major bump recorded with its re-validation requirement (this record) |

## Re-validation scope

- **Every consumer re-validates under NGAB0-R1.** No consumer may rely on the
  pre-R1 call shape (one host call → one kernel) or the byte-only `artifact_id`
  manifest identity without re-validating against this revision.
- **Admitted manifest version re-pinned**: `manifest-1.0.0` →
  **`manifest-2.0.0`**. A manifest carrying a rejected packet revision is
  rejected at admission, not coerced (§Versioning manifest relationship).
- Named consumers: radix emission (device-program/artifact producers), faber
  assembly (manifest producer; `faber build/run` product surface), hosts
  (packet identity admission + session), and any NGAB1–NGAB4 implementation
  lowering against the packet.
- **NGAB1-HOLD (H1)**: NGAB1-U1/U4 stay unfiled until the fixture/delivery
  retargets (DDCP0-U5) land; the one-kernel fixture then proves the minimal
  one-region case.
- **No implementation of the pre-R1 (one-call/one-kernel) ABI anywhere.**
- The revision and its re-validation evidence are recorded in the joint
  cross-repo receipt schema (NGAB0-U10) so a later auditor re-verifies rather
  than re-litigating from scratch (§Versioning rejection policy evidence).

## Predecessor / successor

- Predecessor: DDCP0-U3 (identity domains normative before the packet cites
  them) — committed as `1a50474d`.
- Successor: DDCP0-U5 (fixture contract + delivery retargets; H1 completion).
