# Delivery: NGAB6 — Linux/CUDA container and RunPod portability

**Goal**: `faber/docs/factory/native-gpu-application-bundle/CAMPAIGN.md` (NGAB6 gate)
**Status**: scoped 2026-08-08 — entry-gated; **authorization-gated before any paid run**; full P3 confirmation at gate by planner; Mind owns admission
**Repos**: faber (artifact), radix (RunPod verification campaign owns the paid evidence operation)
**Predecessors**: NGAB5 local CUDA receipt; runpod-gpu-verification contracts (read-only)

## Phase Intent

The same release artifact (or a declared target-triple rebuild) runs in a pinned minimal container with driver-only runtime prerequisites; a locally replayed receipt passes before one explicitly authorized ephemeral RunPod run; model/config inputs and teardown are recorded separately from the binary. This proves portability, not provisioning, ingress, serving, autoscaling, or multi-node execution.

**Entry gate**: NGAB5 CUDA receipt accepted; container prerequisites frozen. **Stop condition**: NO external RunPod mutation without fresh operator authorization (campaign dependency rule 8; AGENTS safety).

## Unit Graph

### NGAB6-U1 — Pinned minimal container definition
- **done_when**: a pinned minimal container (base image, driver-only runtime deps, no build toolchain) is defined and recorded; the artifact's target triple + PTX arch set are declared; container build is reproducible (hash-pinned).
- **write_scope**: `faber/docs/factory/native-gpu-application-bundle/` (container spec), `examples/` (container files). **est_work_tokens**: 8k–16k. **tool_latency**: medium (container build).
- **dependencies**: NGAB5-U4, NGAB0-U5 (PTX arch set).
- **parallel_children_considered**: none.

### NGAB6-U2 — Same-artifact / declared-rebuild run in container
- **done_when**: the release artifact (or a declared target-triple rebuild) runs in the container on local CUDA hardware; the local replay receipt (per NGAB0-U10 schema) passes: same artifact identity, model/config inputs recorded separately from the binary, teardown clean.
- **write_scope**: `examples/` (receipts). **est_work_tokens**: 10k–20k. **tool_latency**: high (container + CUDA run — named boundary).
- **dependencies**: U1.
- **parallel_children_considered**: none.

### NGAB6-U3 — One authorized RunPod receipt (authorization stop)
- **done_when**: ONLY after fresh operator authorization: one ephemeral RunPod run replays the same receipt; the paid run is recorded per the runpod-gpu-verification contracts; no provisioning/ingress/secrets beyond the minimal run; teardown recorded. If authorization is withheld, the phase closes with the local container receipt and an explicit NOT-ATTEMPTED row (honest negative).
- **write_scope**: radix runpod-gpu-verification evidence (owned by that campaign), faber/examples receipts. **est_work_tokens**: 10k–20k. **tool_latency**: high (paid device run — audit boundary).
- **dependencies**: U2, operator authorization.
- **parallel_children_considered**: none.

## Parallelism

- Lane: U1 → U2 → U3 (serial; U3 blocks on authorization). Cross-campaign: runs beside Gradus PML6 (quality) and MD planning (downstream); no shared hot paths. Paid spend is a separate authorization gate (operator).
- **Phase gate**: U1–U3 done — local container receipt passes; RunPod receipt passes OR recorded NOT-ATTEMPTED; README regen + audit 0 findings.

## Validation

```bash
cd faber && python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check
cd faber && ./scripta/check-factory-goal-status
```
Container + device runs at the named boundary (auditor-owned); RunPod run only with fresh authorization.

## Council Dispositions (applicable)

| Item | Mandate | Where |
| --- | --- | --- |
| R5 | Semantic vs binary artifact identity stays distinct | U2 receipts record both identity classes |
| cmo | "Portable" needs exact container/target qualifiers | U1/U2 record base image, triple, PTX set |
| cso | No silent conversion, no credential leakage | U3 minimal-run scope; teardown recorded |

## Open Questions

- Minimum portable CUDA PTX architecture set (NGAB0 open question; default: admitted row's arch set).
- Whether to run the paid RunPod receipt now or defer (operator authorization).
