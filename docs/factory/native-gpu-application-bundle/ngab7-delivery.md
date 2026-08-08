# Delivery: NGAB7 — Qualification, documentation, and release checkpoint

**Goal**: `faber/docs/factory/native-gpu-application-bundle/CAMPAIGN.md` (NGAB7 gate — final)
**Status**: scoped 2026-08-08 — entry-gated; full P3 confirmation at gate by planner; Mind owns admission
**Repos**: faber (primary), with component evidence from radix, hosts, gradus, examples
**Predecessors**: accepted NGAB contracts and receipts; PML6/PML7 closeouts (paired campaign)

## Phase Intent

Target discovery, CLI help, artifact inspection, ABI/schema versions, support matrix, clean-install build, Metal/CUDA receipts, failure cases, portability limits, and release notes **agree** (zombie-doc discipline); Faber version impact is decided; unsupported claims remain false. Closeout of the campaign.

**Entry gate**: NGAB6 accepted (or closed with honest NOT-ATTEMPTED rows) + Gradus PML7 accepted for the paired capstone claim. **Non-goals**: new functionality; serving; deployment; external spend.

## Unit Graph

### NGAB7-U1 — Surface truth: targets, CLI help, artifact inspection
- **done_when**: `faber targets` row(s) for the composite path match live behavior; CLI help documents the composite build/run surface and the backend/device override as operator/diagnostic (not default UX); artifact inspection (manifest, digests, embedded variants) is documented and demonstrable; ABI/schema versions agree across packet, manifest, and runtime.
- **write_scope**: `faber/src/commands/` (help/targets text), `faber/docs/`. **est_work_tokens**: 10k–20k. **tool_latency**: low.
- **dependencies**: NGAB6; all accepted NGAB contracts.
- **parallel_children_considered**: split per surface after the first accepted section.

### NGAB7-U2 — Support matrix + clean-install + receipts
- **done_when**: support matrix (composite surface, admitted backends, rows per NGAB0 packet) populated with qualifiers; clean-install build passes (temporary home, pinned toolchains); Metal/CUDA receipts (NGAB0-U10 schema) attached; failure cases + portability limits documented; no unsupported claim present (claim register updated).
- **write_scope**: `faber/docs/`, `examples/` (receipts), claim register. **est_work_tokens**: 10k–20k. **tool_latency**: high (clean-install — named boundary).
- **dependencies**: U1, PML7 receipts.
- **parallel_children_considered**: none (aggregate truth).

### NGAB7-U3 — Release decision + notes
- **done_when**: Faber version impact decided (composite path ships in which version — release/version review per campaign); release notes claim only the admitted surface; unsupported claims (server, deployment, multi-device) remain explicitly false; campaign status → done + archive per factory convention (generator actions executed).
- **write_scope**: `faber/docs/`, release notes, campaign archive move. **est_work_tokens**: 8k–16k. **tool_latency**: low.
- **dependencies**: U2, faber release protocol owner.
- **parallel_children_considered**: none (closeout).

## Parallelism

- Lane: U1 → U2 → U3 (serial closeout). Cross-campaign: pairs with PML7 (both closeouts) and the phase-close strategic council review (tugboat: run the strategic architecture review before admitting the next phase's candidates — here the next phase is the inference-product campaign).
- **Phase gate**: U1–U3 done; surface truth + receipts + clean-install agree; version decision recorded; campaign closed + archived; README regen + audit 0 findings.

## Validation

```bash
cd faber && python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check
cd faber && ./scripta/check-factory-goal-status
```
Clean-install + release gates at the named boundary (auditor-owned); one pass.

## Council Dispositions (applicable)

| Item | Mandate | Where |
| --- | --- | --- |
| C5 | Claim register final pass — no unsupported claims | U2 |
| cmo | Qualifiers on "GGUF/GPU/executable/portable" | U1/U2 support matrix + notes |
| C7 | Joint receipts | U2 uses the NGAB0-U10 schema |
| cto | Status ≠ artifact proof | U2 receipts are content-addressed evidence, not bookkeeping |

## Open Questions

- Faber version impact (composite path ships in which release — decided with the release protocol owner at gate).
- Inference-product campaign start (next phase; C6 stub is the input).
