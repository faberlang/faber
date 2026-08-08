# Council of Minds Review — 2026-08-08

**Subject**: Native GPU Application Bundle (NGAB) and Production ML Library (PML) pre-delivery strategic review; parallel-lane plan with the in-flight GPU campaigns.
**Mode**: advisory — the council does not implement, lower goals, or verify code. Dispositions are inputs to NGAB0/PML0 lowering and to Mind routing.
**Council**: head-ceo (`sequencing`), head-cpo (`soft_gate`), head-cmo (`sequencing`), head-cso (`hard_gate`), head-cto (`false_gate`), head-cxo (`sequencing`).
**Reviewed artifacts**: `faber/docs/factory/native-gpu-application-bundle/CAMPAIGN.md`, `gradus/docs/factory/production-ml-library/CAMPAIGN.md`, `radix/docs/factory/gpu-inference-gguf/CAMPAIGN.md`, `radix/docs/factory/gpu-inference-multi-device/CAMPAIGN.md`, `radix/docs/factory/gpu-training-lowering/CAMPAIGN.md` (status).

## Verdict summary

The executable-bundle design (Faber product workflow + Radix compiler facts + hosts effects + Gradus semantics + separate product repo for serving) is architecturally sound. **Six of six heads proceed on the ownership boundaries and on PML0↔NGAB0 as the first cross-campaign freeze.** The review is a `sequencing`/`soft_gate` family verdict: the design is right, but **eight corrections must land at or with NGAB0–PML0** before either campaign generalizes its boundary, and several risks must be recorded and rechecked.

## Correct-before-next-phase items (C1–C8)

| # | Item | Present commitment | Future pressure | Affected boundary | Disposition source |
| --- | --- | --- | --- | --- | --- |
| C1 | **GI4+ ownership amendment is a hard prerequisite, not an NGAB0 side-output.** Commit the amendment + migration map in `radix/docs/factory/gpu-inference-gguf/` before GI4 builds persistent decode. Stale clauses assigning model runtime to `faber-runtime` or serving to the old owners must be reconciled. | PML0/NGAB0 require the amendment | Gradus and GI both claim KV/decode | GI ↔ Gradus ↔ NGAB ↔ product | ceo, cpo, cmo, cso, cto, cxo |
| C2 | **MD3I gate amendment.** "Legacy GI4 accepted" is no longer the contract authority once Gradus owns logical decode/KV and NGAB owns composite sessions. Amend `radix/docs/factory/gpu-inference-multi-device/` MD3I entry gate (fold into C1's migration map). | MD3I entry = "MD3 + GI4 accepted" | Multi-device token commit waits on a stale gate | MD ↔ GI ↔ PML5/NGAB4 | cto |
| C3 | **Model-admission migration mechanics named in PML0** (code-move vs retire) — "no dual authority" by code location, not prose. | PML2 migrates `norma:model` + GI1 admission | Two GGUF admission truths | GI1 ↔ PML2 ↔ norma | cso, cxo |
| C4 | **Shared interface packet: content + version authority + revisability.** For NGAB this includes: host/device partition + entry/call ABI; versioned embedded-artifact manifest schema (content-addressed); resource identity; backend variants; artifact layout; build/run UX; error taxonomy; ownership matrix; frozen-now vs reserved seams; explicit unsupported behavior. Named version owner + change procedure; labeled revisable through PML1/NGAB1. | "Exact interface packet" | ABI/schema drift across parallel lanes | PML0 ↔ NGAB0 | ceo, cto, cso |
| C5 | **Cross-campaign claim/capability register.** One register so "accepted"/"partial"/"in flight" never reads as product support; exact architecture/quantization/backend/limits qualifiers. | Separate capability tables | Release notes and product positioning misread | Campaign index ↔ release surface | cmo, cpo |
| C6 | **Inference-product campaign stub** drafted before PML5/NGAB5 convergence; HTTP/serving stays out of both current campaigns. The NGAB executable is the native substrate the future server consumes — never position it as the server itself. | Product campaign "not yet drafted" | Launch story ends at "a local executable" | NGAB5/PM L5 → product | ceo, cmo, cpo |
| C7 | **Joint cross-repo receipt schema + scoped audit entrypoints.** NGAB0 must add/select a Faber-scoped audit entrypoint (shared radix status audit is bookkeeping, not artifact proof) and a content-addressed convergence receipt for NGAB7/PML7. | Component receipts; radix-bound audit | Release convergence unprovable | Release ladder ↔ campaign index | cto |
| C8 | **Security contracts at the freeze.** Canonical embedded-artifact identity + verification order: digest algorithm, verification BEFORE backend selection, model↔kernel compatibility binding, tamper → pre-launch failure. Manifest identity must never be reconstructed from emitted text, path conventions, or naming. Admitted-model capsule (typed handoff) per PML0 — raw GGUF bytes/paths are not trust anchors. | Content-addressed manifest; bytes/paths cross owners | External model substitution/tampering; server inherits unsafe loading | Gradus ↔ Faber ↔ hosts ↔ product | cso |

## Recorded risks (R1–R7)

| # | Risk | Trigger | Recheck | Owner |
| --- | --- | --- | --- | --- |
| R1 | PML0↔NGAB0 paper freeze precedes compiled proof — PML1 may invalidate tensor/shape contract | PML1 tensor/dtype/shape contract lands | At PML1/NGAB1 lowering | planner-1/2, Mind |
| R2 | PML5 generation-config surface frozen before any oracle exercises them | PML5 lowering | Only admit config values with a live oracle at PML5 close | Mind |
| R3 | One-row / one-backend / scalar-first / greedy-decode narrowing hard-codes into shared ABI or public API shape | NGAB1/NGAB5/PML2 lowerings | Typed resource tables + variant tables + support rows stay extensible | Mind |
| R4 | KV identity / principal handoff for the future server unspecified (MD-A9/A10 hold) | Product campaign drafting | Before product repo lowers | Mind + head-cso |
| R5 | NGAB6 "same artifact or declared target-triple rebuild" — semantic vs binary identity must stay distinct | NGAB6 lowering | Record identity classes in packet | Mind |
| R6 | Pending GI3-6/7/8 units may touch FMIR/device facts before NGAB1 freezes the partition | Each GI3 sub-stage dispatch | Classify every pending GI/training/MD unit hot-path vs disjoint before dispatch; one named lane per hot-path revision | Mind |
| R7 | NGAB5 tuning surface becomes a second configuration authority; backend/device selection becomes default end-user UX | NGAB5 lowering | NGAB5 = adapter over the Gradus generation-config contract; backend/device = operator/diagnostic override, never the default UX | Mind |

## Parallel-lane plan (what can run concurrently)

```
PML0 <-> NGAB0            parallel; shared interface packet + GI4+/MD3I amendment
  |         |
PML1      NGAB1 -> NGAB2 -> NGAB3      (serial: compiler facts -> packaging -> host loading)
  |                       |
PML2+PML3            NGAB4 generic composite proof
  |         \              |
PML4      PML5 ----------> NGAB5 LLM executable (convergence)
  |            |              |
PML6 ---------+-----------> NGAB6 portability
  \-----------------------> PML7 + NGAB7 closeout

PML5 + NGAB4 -> multi-device continuation (separate lane)
```

- **Runs now, in parallel**: NGAB0 (faber), PML0 (gradus), GI3-6/7/8 (radix, disjoint units only), training Stage 6+ capstone (examples), MD0-style read-only discovery, llvm-host-parity (independent).
- **Hard serialization points (shared hot paths)**: DeviceProgram, FMIR device wire schema, materializer, host construction, package/model admission, public session APIs — one named owner per revision; and the shared docs/factory README + status audit (radix-bound).
- **Gates that must NOT be treated as implementation-ready**: NGAB5 (waits PML2/3/5 + NGAB4), MD3I (waits C1/C2 contract freeze), GI4+ (waits re-lowering).
- **NGAB-specific serial chain**: NGAB1 (Radix compiler facts) → NGAB2 (Faber composite build) → NGAB3 (hosts bootstrap) is a hard chain; NGAB4 generic proof before NGAB5 LLM complexity.

## What the Mind must not get wrong

1. The GI4+ ownership amendment (C1/C2) is a **hard prerequisite committed in GI's own docs** before GI4 builds persistent decode — not an NGAB0 side-output.
2. NGAB2/NGAB3 fail-closed guarantees (embedded-artifact verification, no CPU fallback, no loose developer-tree kernel paths) are **hard invariants** for every later unit — never weakened for a green.
3. The NGAB executable is the **substrate the future inference server consumes** — never the server, never a second owner of generation semantics.

## Disposition

NGAB0 and PML0 proceed to delivery lowering incorporating C1–C8 and recording R1–R7. Mind owns routing the C-items and the R-rechecks. No `reopen_phase` findings.
