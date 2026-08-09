# Council of Minds Review — 2026-08-08

**Subject**: Direct Device Product Pipeline (DDPP) and Direct Device Compilation Pipeline (DDCP) — verification of the operator's design statement, stage sizing, and threading into the ongoing work.
**Mode**: advisory — the council verifies and sizes; it does not implement, lower goals, or verify code.
**Council**: head-ceo (`proceed with correction`), head-cpo (`approve for lowering`), head-cmo (`proceed, soft gate`), head-cso (`AMBER — contract work only`), head-cto (`conditional ready for stage-0 delivery`), head-cxo (`accept with conditions`).
**Reviewed artifacts**: `faber/docs/factory/direct-device-product-pipeline/CAMPAIGN.md` (DDPP0-8), `radix/docs/factory/direct-device-compilation-pipeline/CAMPAIGN.md` (DDCP0-7), the operator's design statement, and the live PML/NGAB/GI/TR7/MIR-Swarm state.

## Verification — operator design statement

**12/12 clauses embodied.** The campaigns carry the statement faithfully (full clause table in the paired radix council record; identical verdicts here). Partial marks are implementation-stage facts: faber-runtime live until DDPP8; AMD/WebGPU without physical leaves; hot-path exclusions mechanically unproven; HIR lanes beyond Rust capability-gated. **Missing item**: a mechanical hot-path conformance mechanism and, per cso, signature/attestation (SHA-256 identity is not provenance).

## Sizing (stage est_work_tokens bands, council consensus)

| Stage | Band | Stage | Band |
| --- | --- | --- | --- |
| DDPP0 | 55k–90k (6–10 units) | DDCP0 | 45k–75k (5–8 units) |
| DDPP1 | 55k–120k | DDCP1 | 55k–160k |
| DDPP2 | 70k–180k | DDCP2 | 40k–100k |
| DDPP3 | 90k–320k (NGAB1-4 absorbed) | DDCP3 | 55k–140k |
| DDPP4 | 40k–90k | DDCP4 | 55k–140k |
| DDPP5 | 75k–230k | DDCP5 | 70k–240k (AMD greenfield) |
| DDPP6 | 60k–180k | DDCP6 | 65k–180k |
| DDPP7 | 100k–230k (hardware-gated) | DDCP7 | 35k–70k |
| DDPP8 | 80k–340k (deletion gate) | | |

## Correct-before / hold items (product-side view)

| # | Item | Disposition source |
| --- | --- | --- |
| H1 | **NGAB1 HOLD.** NGAB1-U1's one-call/one-kernel ABI must not become authority. DDPP0 lands the NGAB0 major packet revision first (one-call → one-prepared-region; artifact_id → content_sha256), then NGAB1-U1 is retargeted so the one-kernel fixture proves the minimal one-region case. NGAB1-NGAB4 become DDPP3 child packets; NGAB5 → DDPP7 capstone; NGAB6-7 → DDPP8. | cto, cso, ceo, cxo |
| C1 | **DDPP0 product-shape decisions, not Hands' picks**: v1 single-backend default release (the host×device matrix is capability truth, not a shipping promise — every product cell needs its own residency+performance receipt); PTX-vs-cubin and MSL-vs-metallib are startup-preparation tradeoffs resolved in DDPP0; AMD stays `amd`+HSA-native (no HIP/CUDA-translation identity); fat binaries deferred. | cpo, cmo |
| C2 | **Feature isolation is the developer-story contract**: `cargo check -p faber --no-default-features --features hir-rust` excluding GPU emitters/physical Hosts leaves/device runtime (DDPP1 gate) protects compile time + toolchain footprint for the non-GPU majority; if the generated-Rust support crate transitively pulls device/Hosts, the faber-runtime deletion regresses the developer experience. | cpo |
| C3 | **TR7 in the faber-runtime consumer inventory + DDPP8 gate**: training RC rides faber-runtime via core-support; immutable pinned receipts; deletion requires the support-ABI/product-version gate. | cxo, ceo |
| C4 | **faber-runtime is not a rename**: delete only after every consumer migrates; PML2 executes the GI1 admission migration before DDPP8; no forwarding crate; the PML0 capsule's faber-runtime carriage reference reconciled. | ceo, cso, cto |
| C5 | **Message rules**: say "no universal runtime owner" (not "no runtime"); label every claim by evidence tier; migration note before DDPP8 release work (deletion, support surfaces, feature/toolchain changes, core-support-manifest, sibling checkouts, no facade); DDPP7 is a device-integration receipt — never the server or the PML7 capstone. | cmo |

## Recorded risks

| Risk | Trigger | Recheck |
| --- | --- | --- |
| Identity-domain drift between the DDPP0 and DDCP0 freezes | field-by-field agreement gate | the campaign pair's hardest gate |
| "Direct GPU execution" over-claimed (AMD/WebGPU/prepared path) | evidence-tier labels | every claim tied to a named receipt |
| DDPP8 deletion gate uncloseable | TR7/core-support long pole | TR7 immutable receipts + support-ABI gate |
| Developer-experience regression on faber-runtime deletion | feature isolation not real | DDPP1 gate proof |

## Disposition

**DDPP0 proceeds to delivery lowering (planning-only), paired with DDCP0.** Contract-before-implementation stands. NGAB1 is held per H1. The council records fold into the Stage-0 delivery specs. No `reopen_phase` findings; the PML/NGAB/GI/TR7 phase-audited ranges are untouched.
