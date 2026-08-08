# NGAB0 Claim/Capability Register

**Unit**: NGAB0-U9 (C5) — cross-campaign claim/capability register skeleton —
see `ngab0-delivery.md` NGAB0-U9.
**Status**: skeleton — Faber composite-executable surface rows admitted
(evidence-backed); further campaigns add rows per the cross-campaign scope note
below.

## Purpose

One register mapping **claims** (what a product surface can do) to
**capabilities** (the concrete mechanism that delivers each claim), with a
single owner, an evidence reference, and a status. It is cross-campaign: every
row names its source campaign, and later campaigns add rows rather than
rewriting this file.

**Authority order** (from `ngab0-composite-contract.md`): live source/tests →
accepted artifact schemas + hardware receipts → frozen contracts → campaign
prose. Evidence refs below point at committed artifacts only.

## Columns

- **claim** — product-level statement of what the surface can do.
- **capability** — concrete mechanism that delivers the claim.
- **owner** — single owner per surface (`ngab0-composite-contract.md`
  §OwnershipMatrix); non-owners excluded.
- **evidence ref** — committed artifact (frozen contract section, snapshot,
  live source) that supports the claim.
- **status** — current truth: contract frozen / in progress / implemented /
  planned, naming the unit or campaign that owns it.
- **surface** — product surface the claim lands on.
- **campaign** — source campaign; arrows mark the lower-to chain.

## Rows — Faber composite executable surface

| claim | capability | owner | evidence ref | status | surface | campaign |
| --- | --- | --- | --- | --- | --- | --- |
| `faber build` produces one native executable per composite application (one source package → one binary) | Composite build/link: host LLVM modules + embedded device artifacts assembled into one native executable with an inspectable build dir + link manifest | faber (assembly) | `ngab0-composite-contract.md` §PackageGraph ("one native executable per composite build"; node "One native executable"); `evidence/ngab0-snapshot.md` §2 (`llvm_host.rs` `link-manifest.toml` + `write_runtime_identity` precedent) | Contract frozen (U2); implementation planned — NGAB2 | composite executable (build) | NGAB0 → NGAB2 |
| The composite binary embeds content-addressed device artifacts (MSL/metallib, PTX) and a versioned manifest; artifact identity is never reconstructed from emitted text or path conventions | Embedded-artifact manifest assembly; digest-addressed artifact identity; target-neutral `DeviceProgram` serialization | faber (assembly); radix (device-program emission/serialization) | `ngab0-composite-contract.md` §PackageGraph ("embedded content-addressed device artifacts + manifest"; hot-path serialization list #1/#3); `evidence/ngab0-snapshot.md` §2 (`section.rs`, `mir/image.rs` artifacts) | Contract frozen (U2, §PackageGraph); §Manifest/§Verification detail frozen at U4 (pending) | composite executable (artifact layout) | NGAB0 (U4) → NGAB2 |
| Backend execution (Metal/CUDA) is admitted through capability admission gates; unsupported variants fail closed with no CPU fallback; verification precedes backend selection | Capability admission surfaced as product commands; backend selection; host `ProgramSession` dispatch | faber (admission/product workflow); hosts (effects/sessions) | `ngab0-composite-contract.md` §PackageGraph ("unsupported variants fail closed"; "verification precedes backend selection") + §OwnershipMatrix; live baseline `faber/src/package/device/run.rs` (`DeviceBackend`, `artifact_for_backend`) per `evidence/ngab0-snapshot.md` §2 | Contract frozen (U2); live baseline exists; full admission gate implemented at NGAB2/NGAB3 | composite executable (run) | NGAB0 → NGAB2/NGAB3 |
| Embedded-artifact identity is verified before launch; tamper/mismatch → pre-launch failure | Pre-launch verification of embedded-artifact identity + manifest coherence | hosts (verification/effects); faber (executable integration) | `ngab0-composite-contract.md` §PackageGraph ("verification + host session"); `ngab0-delivery.md` §Interpreted Scope item 5 (C8: tamper → pre-launch failure); `CAMPAIGN.md` NGAB2 gate (corrupt/missing/incoherent variants fail before launch) | Contract frozen (U2, §PackageGraph); C8 security freeze at U4 (pending) | composite executable (verification) | NGAB0 (U4) → NGAB2/NGAB3 |

## Cross-campaign scope note

- This register is **cross-campaign**: rows are added as each owning campaign
  lowers, never by reusing another campaign's row.
- **PML0 / Gradus** adds rows for the ML-semantics surface (paired
  `gradus/docs/factory/production-ml-library/pml0-gradus-contract.md`).
  **NGAB1+** (radix host/device partition + ABI, NGAB2 composite build,
  NGAB3 sessions, NGAB4–NGAB7 qualification) add rows as their units land.
  The **separate inference-product repo** adds rows for serving/HTTP,
  scheduling, batching, and deployment when that repository is drafted.
  Rows citing evidence in sibling repos use that repo's relative path.
- **No row claims a capability without evidence.** A row is admitted only when
  its evidence ref points at a committed artifact (frozen contract section,
  snapshot, live source, or receipt) that supports the claim. Contract prose is
  evidence only when the referenced contract section is frozen.
- Editing is additive. A row's status moves as its owning unit lands; a claim
  that is superseded is archived, never silently rewritten.
