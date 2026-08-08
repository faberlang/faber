# Delivery: NGAB3 — Native bootstrap and persistent device sessions

**Goal**: `faber/docs/factory/native-gpu-application-bundle/CAMPAIGN.md` (NGAB3 gate)
**Status**: scoped 2026-08-08 — entry-gated; full P3 confirmation at gate by planner; Mind owns admission
**Repos**: hosts (physical lifecycle — primary), faber (executable integration)
**Predecessors**: NGAB2 (composite binary + manifest)

## Phase Intent

The executable discovers capabilities, verifies and loads its own embedded variant, opens one persistent session, performs declared transfers and launches, exposes observations to host Faber code, and releases resources on normal exit, error, and cancellation. Hosts own physical effects; generated/application runtime holds only versioned logical handles and call state.

**Entry gate**: NGAB2 accepted. **Non-goals**: multi-device (MD campaign), server lifecycle (product repo), model semantics (Gradus).

## Unit Graph

### NGAB3-U1 — Capability discovery + backend admission
- **done_when**: the executable discovers local capabilities (hardware, driver, artifact version, arch, dtype, quant, kernel capability) and admits a backend per the manifest's verification order (identity verified BEFORE selection); unsupported combinations fail closed with typed errors — no CPU fallback.
- **write_scope**: `hosts/crates/*/` (discovery/admission), `faber/src/package/device/` (integration). **est_work_tokens**: 10k–20k. **tool_latency**: medium.
- **dependencies**: NGAB2-U2/U3.
- **parallel_children_considered**: none (admission precedes everything).

### NGAB3-U2 — Verify + load embedded variant
- **done_when**: the embedded MSL/metallib/PTX variant is verified (digest match, model↔kernel binding) and loaded into a module; corrupt/tampered/mismatched artifacts fail before any dispatch; load path never consults developer-tree kernel paths.
- **write_scope**: `hosts/crates/*/` (module loading), tests. **est_work_tokens**: 10k–20k. **tool_latency**: medium.
- **dependencies**: U1.
- **parallel_children_considered**: none (verify-then-load is one sequence).

### NGAB3-U3 — Persistent session: transfers, launches, observations
- **done_when**: one persistent session opens per the accepted host contract (reuse `ProgramSession`/composite-host precedent from the training lane, accepted receipts); declared transfers and launches execute; observations (results, timings) are exposed to host Faber code as typed values; session reused across calls without rebuild.
- **write_scope**: `hosts/crates/*/` (session), `faber/src/package/device/run.rs`. **est_work_tokens**: 15k–30k. **tool_latency**: high (device runs — named boundary).
- **dependencies**: U2, accepted training-lane session receipts (read-only).
- **parallel_children_considered**: none (session semantics indivisible); backend-provider split after the first accepted (U4).

### NGAB3-U4 — Teardown: normal, error, cancellation
- **done_when**: resources release on normal exit, error, and cancellation; teardown is deterministic and idempotent (double-teardown safe); cancellation paths tested with in-flight work; no leak or stale-handle reuse after teardown.
- **write_scope**: `hosts/crates/*/` (teardown), tests. **est_work_tokens**: 8k–16k. **tool_latency**: medium (cancellation/device tests).
- **dependencies**: U3.
- **parallel_children_considered**: split per backend provider after the first accepted lifecycle.

## Parallelism

- Lane: U1 → U2 → U3 → U4 (serial — same session surfaces). Cross-campaign: runs beside PML3–PML5 (disjoint repos), MD0-style read-only discovery (no schema changes), training lane (session contracts consumed read-only). Host-construction surface is a shared hot path — serialize with any in-flight training/MD host edits (one named owner per revision).
- **Phase gate**: U1–U4 done; one persistent session with verified embedded variant, observations, and clean teardown on all three paths; README regen + audit 0 findings.

## Validation

```bash
cd faber && python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check
cd faber && ./scripta/check-factory-goal-status
```
Narrow unit checks per touched crate (`cargo check -p <crate>`); targeted session/lifecycle tests once at closeout; device runs at the named boundary (auditor-owned receipts).

## Council Dispositions (applicable)

| Item | Mandate | Where |
| --- | --- | --- |
| C8 | Verify before select; tamper → pre-launch failure | U1/U2 |
| cso | No CPU fallback, no loose kernel paths | U1/U2 hard invariants |
| R3 | Singleton-session narrowing must not hard-code | U3/U4 keep independent session identities + provider tables extensible |

## Open Questions

- Whether the composite executable reuses the training lane's `ProgramSession` unchanged or needs a composite-session revision (decision: reuse where semantics match; extend only with a named missing fact).
