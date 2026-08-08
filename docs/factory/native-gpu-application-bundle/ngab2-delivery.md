# Delivery: NGAB2 — Faber composite build and embedded artifact assembly

**Goal**: `faber/docs/factory/native-gpu-application-bundle/CAMPAIGN.md` (NGAB2 gate)
**Status**: scoped 2026-08-08 — entry-gated; full P3 confirmation at gate by planner; Mind owns admission
**Repo**: faber (`src/package/llvm_host.rs`, `src/package/device/section.rs`, `src/package/mir/image.rs`)
**Predecessors**: NGAB1 (partition/ABI/device-program facts)

## Phase Intent

`faber build` produces one native executable and an inspectable build directory; the binary contains content-addressed admitted GPU artifacts (MSL/metallib and/or PTX) plus a manifest; debug/release toolchains are recorded; corrupt, missing, incoherent, or unsupported variants fail **before launch** (C8). Faber orchestrates existing Radix emitters and external toolchains — it does not own kernel semantics.

**Entry gate**: NGAB1 accepted. **Non-goals**: host loading/sessions (NGAB3), kernel semantics (Radix), model semantics (Gradus), server behavior (product repo).

## Unit Graph

### NGAB2-U1 — Composite build: one native executable + inspectable build dir
- **done_when**: `faber build` (composite path per NGAB0 packet UX) produces one native executable + an inspectable build directory (`target/faber-llvm/{debug|release}/` precedent); debug/release toolchains recorded in the manifest; the NGAB1 fixture builds end-to-end.
- **write_scope**: `faber/src/package/llvm_host.rs` (composite section), `faber/src/package/build.rs`. **est_work_tokens**: 15k–30k. **tool_latency**: high (external toolchains: llvm-as/opt/clang — named, not in dev loop).
- **dependencies**: NGAB1-U1.
- **parallel_children_considered**: none (build orchestration is the cohesion root).

### NGAB2-U2 — Content-addressed embedded artifacts + manifest
- **done_when**: embedded MSL/metallib/PTX artifacts are content-addressed per the NGAB0-U4 manifest schema; the manifest (digest algorithm named, verification-before-selection order, model↔kernel binding, tamper → pre-launch failure) is embedded in the binary; identity is never reconstructed from emitted text or path conventions.
- **write_scope**: `faber/src/package/device/section.rs`, `faber/src/package/mir/image.rs` (embedding). **est_work_tokens**: 12k–24k. **tool_latency**: medium.
- **dependencies**: U1, NGAB0-U4.
- **parallel_children_considered**: none (manifest is one contract); backend-artifact variants batch after the first accepted embedding (U3).

### NGAB2-U3 — Fail-before-launch admission matrix
- **done_when**: corrupt, missing, incoherent, and unsupported variants fail before launch with typed errors (no CPU fallback, no loose developer-tree kernel paths); the admission matrix (unsupported hardware/driver/version/arch/dtype/quant/capability) is tested negative; fail-closed invariants from the council are preserved.
- **write_scope**: `faber/src/package/device/run.rs` (admission), tests. **est_work_tokens**: 10k–20k. **tool_latency**: medium.
- **dependencies**: U2.
- **parallel_children_considered**: split per failure class after the first accepted row (batch-by-default per campaign).

## Parallelism

- Lane: U1 → U2 → U3 (serial — same files). Cross-campaign: runs beside PML2–PML4 (disjoint repos), GI3-8 (manifest facts consumed read-only), NGAB3 planning (planner may lower while NGAB2 implements — Rule 5). No Gradus write scope.
- **Phase gate**: U1–U3 done; one native executable carries verified embedded artifacts; fail-before-launch proven; README regen + audit 0 findings.

## Validation

```bash
cd faber && python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check
cd faber && ./scripta/check-factory-goal-status   # (NGAB0-U10 entrypoint)
```
Narrow unit checks: `cargo check -p faber` per unit; targeted admission tests once at closeout. Build/link runs at the named boundary (not in dev loop).

## Council Dispositions (applicable)

| Item | Mandate | Where |
| --- | --- | --- |
| C8 | Content-addressed manifest, verify-before-select, tamper → pre-launch, no text/path identity | U2/U3 |
| cto | Fail-closed, no CPU fallback is a hard invariant | U3 keeps it for every later unit |

## Open Questions

- Metal source vs metallib for the first admitted macOS row (NGAB0 open question; default: source first, metallib reserved).
- CUDA PTX arch set (NGAB0 open question; default: admitted row's arch set).
