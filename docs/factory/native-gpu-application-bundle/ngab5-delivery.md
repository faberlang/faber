# Delivery: NGAB5 — Gradus-backed LLM executable capstone

**Goal**: `faber/docs/factory/native-gpu-application-bundle/CAMPAIGN.md` (NGAB5 gate)
**Status**: scoped 2026-08-08 — entry-gated; full P3 confirmation at gate by planner; Mind owns admission
**Repos**: faber (executable path), examples (capstone), gradus (ML semantics — consumed, not owned here)
**Predecessors**: NGAB4 (generic composite proof), Gradus PML2/PML3/PML5 (model/forward/decode/KV/sampling contracts)

## Phase Intent

A Faber application binary loads an admitted external model, tokenizes input, performs prefill and persistent autoregressive decode through embedded kernels, maintains logical KV state through Gradus, accepts bounded tuning parameters, and produces oracle-matching tokens — with **no server and no `llama.cpp` runtime dependency**. This is the convergence point of the two campaigns.

**Entry gate**: NGAB4 accepted AND Gradus PML2/PML3/PML5 accepted. **Non-goals**: serving/HTTP (product repo), new ML semantics (Gradus), multi-device (MD), deployment (NGAB6+).

## Unit Graph

### NGAB5-U1 — Gradus-capable composite executable
- **done_when**: the composite executable path links a Gradus consumer package (the `mir-library-imports` precedent: linked `gradus:*` calls through FMIR); model load goes through the admitted-model capsule (PML2) — no raw-path/bytes trust anchors in the executable.
- **write_scope**: `faber/src/package/` (link path), `examples/` (capstone). **est_work_tokens**: 15k–30k. **tool_latency**: high (link/build — named boundary).
- **dependencies**: NGAB2-U2 (embedding), Gradus PML2 capsule.
- **parallel_children_considered**: none (link root).

### NGAB5-U2 — Prefill + persistent decode through embedded kernels
- **done_when**: prefill and one-token decode run through embedded kernels over Gradus's logical KV (PML5-U2); logical state and physical residency stay separate (Gradus owns semantics; hosts own residency); oracle-matching tokens per PML5-U6; no per-token session rebuild.
- **write_scope**: `examples/` (capstone), `faber/src/package/device/` (dispatch integration). **est_work_tokens**: 15k–30k. **tool_latency**: high (device decode runs).
- **dependencies**: U1, Gradus PML5, NGAB3 session.
- **parallel_children_considered**: none (decode semantics indivisible).

### NGAB5-U3 — Tuning-parameter adapter (single authority rule)
- **done_when**: the executable accepts bounded tuning parameters (model path, prompt, context length, prompt batch size, maximum generated tokens, seed, temperature, top-k, top-p, min-p, repetition penalty, explicit backend/device selection) as an **adapter over the Gradus generation-config contract** — never a second authority (cpo/cxo rule); backend/device selection is an operator/diagnostic override, not default UX; unsupported controls are explicit reject rows.
- **write_scope**: `examples/` (capstone CLI), docs. **est_work_tokens**: 10k–20k. **tool_latency**: medium.
- **dependencies**: U2, Gradus PML5-U4.
- **parallel_children_considered**: none (one adapter).

### NGAB5-U4 — Oracle-matching + no-dependency proof
- **done_when**: bounded generation (256 tokens) produces oracle-matching tokens per the GI0–GI2 pinned oracle; divergence recorded at first token; receipts prove no `llama.cpp` runtime dependency and no server; backend receipts (Metal, CUDA) per the NGAB0-U10 joint schema.
- **write_scope**: `examples/` (receipts), docs. **est_work_tokens**: 12k–24k. **tool_latency**: high (backend runs — named boundary, auditor-owned).
- **dependencies**: U3.
- **parallel_children_considered**: split by backend receipt after the first admitted backend passes.

## Parallelism

- Lane: U1 → U2 → U3 → U4 (serial spine). Cross-campaign: NGAB5 is the PML↔NGAB convergence — PML5 and NGAB4 land first (parallel), then this phase. MD3I's amended gate (C2) consumes Gradus PML5 decode/KV semantics + this composite-session authority — coordination at the contract level. RunPod/multi-device are downstream.
- **Phase gate**: U1–U4 done; bounded generation from one native program, oracle-matching, no llama.cpp/server; README regen + audit 0 findings.

## Validation

```bash
cd faber && python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check
cd faber && ./scripta/check-factory-goal-status
```
Device decode + oracle runs at the named boundary (auditor-owned); one closeout pass per backend.

## Council Dispositions (applicable)

| Item | Mandate | Where |
| --- | --- | --- |
| R7 | NGAB5 never a second config authority | U3 (adapter over Gradus contract) |
| cpo | Backend/device = operator override, not default UX | U3 |
| C8 | No CPU fallback, no llama.cpp, typed model handoff | U1/U4 |
| C6 | Product-campaign stub drafted before this convergence | U4 references the stub; serving stays out |

## Open Questions

- Explicit backend/device selection UX (operator/diagnostic override confirmed at gate).
- Which inference-product repo direction the capstone feeds (consumes C6 stub).
