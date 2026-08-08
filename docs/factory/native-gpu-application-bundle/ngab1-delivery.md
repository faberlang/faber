# Delivery: NGAB1 — Radix host/device partition and callable device boundary

**Goal**: `faber/docs/factory/native-gpu-application-bundle/CAMPAIGN.md` (NGAB1 gate)
**Status**: scoped 2026-08-08 — entry-gated; full P3 confirmation at gate by planner; Mind owns admission
**Repo**: radix (compiler facts — primary), faber (wire/package surfaces where shared)
**Predecessors**: NGAB0 accepted (U2–U7 packet, U8 amendment — C1 hard prerequisite), GI3 accepted compiler facts (read-only)

## Phase Intent

One analyzed package produces validated host MIR/LLVM **and** a typed device program; host code invokes the device boundary through a versioned ABI; resource/lifetime/mutation/observation facts survive lowering; invalid cross-boundary values fail at compile time. **No reconstruction of calls from LLVM, MSL, or PTX text** (extend typed IR/schema before emitters).

**Entry gate**: NGAB0 closed — including U8 (C1: GI4+ ownership amendment committed; C2: MD3I gate amended). **Non-goals**: packaging/assembly (NGAB2), host loading (NGAB3), model semantics (Gradus), backend performance work.

## Unit Graph

### NGAB1-U1 — One vertical slice: host MIR/LLVM + typed device program
- **done_when**: the NGAB0-U11 fixture (one scalar host function calling one device kernel) is the proof: one analyzed package derives validated host MIR → LLVM AND a typed `DeviceProgram`; device facts (identity, resources, launches, lifetimes, observations) are typed, not text-parsed; the call links and executes through the existing llvm-host path (targeted, narrow test).
- **write_scope**: `radix/crates/radix-mir*/` device-program + host-partition surfaces, `faber/src/package/` wire where the ABI crosses. **est_work_tokens**: 20k–40k. **tool_latency**: high (radix + faber check; no cargo build/test in dev loop — `cargo check -p <crate>` only).
- **dependencies**: NGAB0 packet (partition/ABI), GI3 compiler contracts (read-only).
- **parallel_children_considered**: none — the vertical slice is the phase's cohesion root; everything else extends it.

### NGAB1-U2 — Versioned call ABI + compile-time rejection
- **done_when**: the host→device call ABI is versioned (per NGAB0-U3); invalid cross-boundary values (wrong type, shape, lifetime, mutation of read-only resource, observation of unlaunched work) fail at compile time with typed diagnostics; a negative fixture proves rejection without a launch.
- **write_scope**: `radix/crates/radix-mir*/` ABI validation, `faber/src/package/` diagnostics. **est_work_tokens**: 12k–24k. **tool_latency**: medium.
- **dependencies**: U1.
- **parallel_children_considered**: none (ABI indivisible); negative cases batch after the happy path.

### NGAB1-U3 — Resource/lifetime/mutation/observation facts survive lowering
- **done_when**: resource identity, lifetimes, mutation rules, and observation facts from the semantic program survive to the device program and the ABI (grep/test proof: no fact dropped or re-derived from text); the shared hot-path serialization list (DeviceProgram, FMIR wire, materializer) is respected — one named owner per revision.
- **write_scope**: `radix/crates/radix-mir*/`, `faber/src/package/mir/image.rs` (only if a wire fact is missing — flagged, not silently added). **est_work_tokens**: 12k–24k. **tool_latency**: medium.
- **dependencies**: U2.
- **parallel_children_considered**: none (facts are the phase thesis).

### NGAB1-U4 — Batch compatible call shapes
- **done_when**: after the first accepted call shape, compatible shapes (vector/2D args, two-kernel composition) batch through the same ABI without new mechanisms; each batched shape has a red-green test; no per-shape special-casing in the ABI (R3: extensible variant tables).
- **write_scope**: `radix/crates/radix-mir*/`, `faber/src/package/`. **est_work_tokens**: 8k–16k per batch. **tool_latency**: medium.
- **dependencies**: U2.
- **parallel_children_considered**: split per call shape after the first accepted pattern.

## Parallelism

- Lane: U1 → U2 → U3 → U4 (serial spine — same shared surfaces). **No parallel radix-write lanes** (hot paths; one owner).
- Cross-campaign: runs beside PML1–PML3 (disjoint repos), GI3-8 (serialize only if FMIR/device facts change — R6 classification before dispatch), training Stage 6+ capstone (disjoint). Serialization with MD work on DeviceProgram/FMIR is mandatory (named owner per revision).
- **Phase gate**: U1–U4 done; one vertical slice runs; compile-time rejection proven; facts survive; README regen + audit 0 findings.

## Validation

```bash
cd radix && ./scripta/check-factory-goal-status --json --fail-on error   # or scoped entrypoint
cd radix && python3 scripta/generate-factory-readme.py --check
cd faber && python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check
```
Narrow unit checks: `cargo check -p <touched crate>` per unit (Cargo discipline); targeted device-program tests once at closeout. Full ladder runs are auditor-owned.

## Council Dispositions (applicable)

| Item | Mandate | Where |
| --- | --- | --- |
| C4 | Packet revisable through PML1/NGAB1 | U2 records ABI drift vs packet as a need (not silent) |
| C8 | No text/path-derived identity | U1/U3 enforce typed facts only |
| R6 | Pending GI units vs shared surfaces | Every unit's pre-dispatch check: classify GI3-6/7/8 FMIR/device edits |
| R3 | Scalar-first must not hard-code | U4 keeps variant tables extensible |

## Open Questions

- Whether any missing wire fact requires an FMIR revision vs a faber-side projection (decision routed to Mind/radix owner; default: extend typed schema, never text).
