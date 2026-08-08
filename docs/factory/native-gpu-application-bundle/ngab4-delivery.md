# Delivery: NGAB4 — Cross-vendor generic application proof

**Goal**: `faber/docs/factory/native-gpu-application-bundle/CAMPAIGN.md` (NGAB4 gate)
**Status**: scoped 2026-08-08 — entry-gated; full P3 confirmation at gate by planner; Mind owns admission
**Repos**: faber (product path), examples (fixture/receipts); radix/hosts keep component boundaries
**Predecessors**: NGAB1–NGAB3; accepted Metal/CUDA receipts from gpu-training-lowering (read-only)

## Phase Intent

One Faber CLI application performs ordinary host work around at least two kernel calls from the same source; local Metal and CUDA executions match a CPU oracle; receipts prove embedded-artifact provenance, persistent-session reuse, bounded transfers, and clean teardown. **Generic composition is proven before LLM complexity enters (NGAB5).**

**Entry gate**: NGAB3 accepted. **Non-goals**: LLM model semantics (Gradus PML5), servers, multi-device, performance headlines.

## Unit Graph

### NGAB4-U1 — Generic CLI application fixture
- **done_when**: one Faber CLI application (in `examples/`) does host work (CLI parse, file read, control flow) around ≥2 kernel calls from the same source package; the NGAB0-U11 fixture contract is executed (not just specified).
- **write_scope**: `examples/` (fixture app), `faber/src/` (only if a product-path gap surfaces — flagged, not silently added). **est_work_tokens**: 12k–24k. **tool_latency**: medium.
- **dependencies**: NGAB1–NGAB3.
- **parallel_children_considered**: none (fixture is the phase root).

### NGAB4-U2 — CPU-oracle equivalence (both backends)
- **done_when**: the fixture's device results match the CPU oracle for Metal AND CUDA (same source, same ABI version); divergence is recorded at the first mismatch; no per-backend semantic shortcuts; receipts capture device/link identities.
- **write_scope**: `examples/` (receipts), `hosts/` only for named provider gaps. **est_work_tokens**: 12k–24k. **tool_latency**: high (Metal + CUDA device runs — named boundary).
- **dependencies**: U1, accepted training-lane cross-vendor receipts.
- **parallel_children_considered**: split by backend receipt after the fixture runs on the first backend.

### NGAB4-U3 — Receipts: provenance, session reuse, bounded transfers, teardown
- **done_when**: receipts (per the NGAB0-U10 joint schema) prove embedded-artifact provenance (digests), persistent-session reuse across calls, bounded transfers (bytes declared), and clean teardown; the same-artifact evidence rule holds (no mixed-artifact receipts).
- **write_scope**: `examples/` (receipts), docs. **est_work_tokens**: 8k–16k. **tool_latency**: low (aggregate).
- **dependencies**: U2.
- **parallel_children_considered**: none (receipts aggregate U1–U2).

## Parallelism

- Lane: U1 → U2 (per backend) → U3. Cross-campaign: runs beside PML4–PML5 (disjoint repos — the PML5 + NGAB4 convergence is the next merge point), GI3-8 (read-only), training lane (receipts consumed read-only). Metal and CUDA receipts may run on separate machines (burgus + named CUDA machine) in parallel.
- **Phase gate**: U1–U3 done; generic composite proven on Metal + CUDA vs CPU oracle; receipts complete; README regen + audit 0 findings.

## Validation

```bash
cd faber && python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check
cd faber && ./scripta/check-factory-goal-status
```
Device runs at the named boundary (auditor-owned); one oracle equivalence pass per backend.

## Council Dispositions (applicable)

| Item | Mandate | Where |
| --- | --- | --- |
| cpo/cmo | "One executable" needs exact backend/limits qualifiers | U3 receipts carry exact device/link/artifact identities |
| R5 | Same-artifact evidence | U3 enforces one artifact set per receipt |

## Open Questions

- None phase-blocking (backend set = Metal + CUDA per campaign).
