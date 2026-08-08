# Delivery: NGAB0 — Composite artifact and ownership contract

**Goal ref**: `faber/docs/factory/native-gpu-application-bundle/CAMPAIGN.md` — NGAB0 stage (status `planned — selected`), Dependency Rules §1, §6, §7, Validation section, Open Questions, Stop Conditions
**Status**: lowered 2026-08-08 — ready for admission by Mind
**Repo**: faber (control plane). Write scope spans **faber** and **radix** docs; paired with `gradus/docs/factory/production-ml-library/pml0-delivery.md` (PML0 packet unit — cross-repo dependency, not lowered here).

## Phase Intent

NGAB0 is **discovery-first**: freeze the composite native-GPU application
contract and the cross-campaign ownership facts so NGAB1–NGAB3 can implement
one vertical slice (one scalar host function calling one embedded device
kernel). No product code. No runnable composite binary — the executable proof
belongs to NGAB1–NGAB4. Every unit produces a contract/decision/measurement
artifact with a hard `done_when` and a grep-or-script proof.

## Interpreted Scope

Deliver, for the NGAB0 gate:

1. **One frozen interface packet** (`ngab0-composite-contract.md`) covering: host/device partition + entry/call ABI; versioned content-addressed embedded-artifact manifest schema; resource identity; backend variants; artifact layout; build/run UX; error taxonomy; ownership matrix; frozen-now vs reserved seams; explicit unsupported behavior; and a **named version authority + change procedure** labeled *"revisable through PML1/NGAB1"* (C4).
2. **C1 (hard prerequisite)**: a committed ownership amendment + migration map in `radix/docs/factory/gpu-inference-gguf/` such that GI4+ retains neither model-runtime nor serving ownership, **plus C2 in the same unit**: MD3I's entry gate in `radix/docs/factory/gpu-inference-multi-device/` amended so "legacy GI4 accepted" is replaced by the new contract authority (Gradus PML5 decode/KV semantics + NGAB composite session facts).
3. **C5**: cross-campaign claim/capability register skeleton with first rows for Faber's composite executable surface.
4. **C7**: joint cross-repo receipt schema + a Faber-scoped audit entrypoint selected or added (required before the full status-audit gate is claimed).
5. **C8**: canonical embedded-artifact identity + verification order frozen in the packet: digest algorithm named, verification **before** backend selection, model-to-kernel compatibility binding, tamper → pre-launch failure, and identity never reconstructed from emitted text or path conventions (NGAB1 forbids it; NGAB0 freezes the contract).
6. **cpo/cxo design rules** recorded in the packet's UX contract (a unit output, not code): NGAB5's tuning surface is an adapter over the Gradus generation-config contract, never a second authority; backend/device selection is an operator/diagnostic override, not default UX.
7. Snapshot refresh + stale-plan reconciliation, one generic **fixture contract** (spec only; NGAB1 executes it), and phase closeout.

## Normalized Spec

`ngab0-composite-contract.md` section inventory (each section frozen by exactly one unit):

| § | Content | Unit |
| --- | --- | --- |
| PackageGraph + OwnershipMatrix | package-graph nodes, owner-per-surface table, hot-path serialization list | U2 |
| Partition + Abi | host/device partition, entry/call ABI, typed boundary, no text reconstruction | U3 |
| Manifest + ResourceIdentity + Verification | content-addressed manifest schema, resource identity, C8 security freeze | U4 |
| BackendVariants + ArtifactLayout + Admission | variant matrix, binary layout, fail-closed admission, operator decision gates | U5 |
| Ux + Errors | build/run UX, error taxonomy, cpo/cxo design rules | U6 |
| FrozenVsReserved + Unsupported + Versioning | seams, explicit unsupported behavior, version authority + change procedure | U7 |

Supporting artifacts: `evidence/ngab0-snapshot.md` (U1), `ngab0-claim-register.md` (U9), `ngab0-receipt-schema.md` (U10), `ngab0-fixture-contract.md` (U11), `radix/docs/factory/gpu-inference-gguf/gi4-ownership-amendment.md` + campaign edits (U8).

## Repo-Aware Baseline

Verified 2026-08-08 (grounding, not implementation):

- `faber/src/package/llvm_host.rs` (720 ln) — product `llvm-host` orchestration: package graph → one `.ll` per unit via `super::llvm::build_package_llvm` → `llvm-as` verify → pinned `opt -O2` (release) → `clang` link against `faber-host-llvm` runtime archive → inspectable `target/faber-llvm/{debug|release}/` + link manifest + runtime identity.
- `faber/src/package/device/section.rs` (833 ln) — FMIR device-section assembly: `DeviceSelection`, `FmirDeviceSection`, wire `DeviceProgram`, `DEVICE_RUN_PLAN_VERSION`; carries MSL/PTX artifacts.
- `faber/src/package/mir/image.rs` (900 ln) — FMIR image construction/loading (source-built/text/binary), merged-program revalidation.
- `faber/src/package/device/run.rs` (603 ln) — `faber run --backend metal|cuda` host-session execution: `DeviceBackend`, `DeviceSelection`, `ProgramSession`, step-run reports.
- `faber/src/commands/targets.rs` — static `faber targets` table; `MirLlvmHost → "llvm-host"` row (campaign asserts build/run/package yes); `MirMetal → "metal-text"`, `MirLlvm → "llvm-text"` rows. U1 grep-proves these rows rather than running the binary.
- `radix/docs/factory/gpu-inference-gguf/` — GI0–GI2 accepted, GI3 landed (GI3-5 2026-08-08), **GI4 planned** (`gi4-contract.md` + `gi4-delivery.md` exist, contract frozen). U8 must land before GI4 implementation fans out; serialize with in-flight GI3-6/7/8 status edits.
- `radix/docs/factory/gpu-inference-multi-device/CAMPAIGN.md` — MD3I entry gate line: *"Entry gate: MD3 + GI4 accepted — GI4 contract freeze"* (also in stage table and dependency rule 4). U8 amends this authority.
- `gradus/docs/factory/production-ml-library/CAMPAIGN.md` PML0 requires `pml0-gradus-contract.md` + the same gguf ownership amendment — the paired packet (PML0 `⇄` NGAB0). No device handle, no HTTP policy in the Gradus packet.

**Authority order**: live source/tests and live `faber targets` → accepted artifact schemas + hardware receipts → this packet's frozen contracts → campaign prose. NGAB0 refreshes the campaign's source snapshot (`faber 26b503a0e3bb`, radix `a01543b06bfe`, hosts `e066ee0ae98a`, gradus `29d26735d0d9`, norma `84f27dacd6f9`, examples `aad199ecf07c`) before NGAB1 lowers.

## Unit Graph

```text
U1 snapshot/reconcile
  └─ U2 package graph + ownership ──┬─ U3 partition + ABI ─ U4 manifest + C8 ─ U5 variants/layout ─ U6 UX/errors ─ U7 packet complete
                                    ├─ U8 radix amendment (C1+C2) [lane B]
                                    ├─ U9 claim register (C5)      [lane C]
                                    └─ (U4) → U10 receipt + audit   [lane C]
                                          └─ (U5) → U11 fixture contract [lane D]
                                                       └─ U12 phase closeout (needs all; operator decisions)
```

## Units

### NGAB0-U1 — Snapshot refresh + stale-plan reconciliation
- **title**: Refresh campaign source snapshot and reconcile drifted claims.
- **done_when**: `evidence/ngab0-snapshot.md` records current HEAD SHAs + dirty-state declarations for faber, radix, hosts, gradus, norma, examples; confirms `llvm_host.rs`, `device/section.rs`, `mir/image.rs`, `device/run.rs` exist and role-match; grep-proves the `llvm-host`/`metal-text`/`llvm-text` rows in `faber/src/commands/targets.rs`; marks every Ground-Truth-table claim confirmed/drifted/superseded with a reference; also marks the campaign Validation-section claim that the shared status-audit script "has no `--factory-root`" as drifted and records the correction (`radix/scripta/audit-factory-goal-status.py` accepts `--factory-root`, consumed at line 591).
- **write_scope**: `faber/docs/factory/native-gpu-application-bundle/evidence/ngab0-snapshot.md` (create); no other repo edits.
- **validation**: `git -C <repo> rev-parse HEAD` for the six repos (record, no compare gate); `test -f` on the four source files; `grep -n '"llvm-host"' faber/src/commands/targets.rs`; `grep -n "drifted|superseded" evidence/ngab0-snapshot.md`.
- **est_work_tokens**: 3k–6k (multi-repo git + grep + write).
- **tool_latency**: medium (six `git rev-parse` calls, no build).
- **dependencies**: none.
- **parallel_children_considered**: none (phase root).

### NGAB0-U2 — Package graph + ownership matrix
- **title**: Freeze the accepted package graph and ownership matrix.
- **done_when**: `ngab0-composite-contract.md` §PackageGraph + §OwnershipMatrix frozen: one source package → host MIR/LLVM modules + device program/artifacts → composite build/link → one native executable; owner-per-surface rows (faber = product workflow/assembly/UX; radix = compiler facts/emission/device program; hosts = effects/sessions; gradus = ML semantics, **no device handle**; inference product repo = later, not this phase); hot-path serialization list (DeviceProgram, FMIR/device wire versions, materializer, host construction, package admission) citing cross-campaign rules; paired PML0 packet cited as the exchange partner.
- **write_scope**: `faber/docs/factory/native-gpu-application-bundle/ngab0-composite-contract.md` (create; §PackageGraph, §OwnershipMatrix).
- **validation**: `grep -n '^## .*PackageGraph'` and `grep -n '^## .*OwnershipMatrix'` present; `grep -n 'pml0-gradus-contract' ngab0-composite-contract.md`; `grep -n 'no device handle' ngab0-composite-contract.md`.
- **est_work_tokens**: 3k–5k.
- **tool_latency**: low.
- **dependencies**: U1.
- **parallel_children_considered**: none (lane-A root; single-file coherence).

### NGAB0-U3 — Host/device partition + entry/call ABI
- **title**: Freeze the host/device partition and typed entry/call ABI.
- **done_when**: §Partition + §Abi frozen: one host function calls one device kernel through a versioned typed boundary; identity/type/lifetime facts survive lowering (never reconstructed from LLVM/MSL/PTX text or naming conventions — NGAB1 rule, frozen here); invalid cross-boundary values fail at compile time (enforcement is NGAB1; contract is NGAB0); call/entry surface matches the GI4 session facts referenced by U8.
- **write_scope**: `ngab0-composite-contract.md` (same file; §Partition, §Abi).
- **validation**: `grep -n '^## .*Abi'` present; `grep -n 'never reconstructed from emitted text' ngab0-composite-contract.md`; `grep -n 'compile time' ngab0-composite-contract.md`.
- **est_work_tokens**: 4k–7k.
- **tool_latency**: low.
- **dependencies**: U2.
- **parallel_children_considered**: none (serial in file; see U4 note).

### NGAB0-U4 — Manifest schema + resource identity + C8 security freeze
- **title**: Freeze the versioned content-addressed embedded-artifact manifest, resource identity, and verification order.
- **done_when**: §Manifest + §ResourceIdentity + §Verification frozen: versioned manifest schema over embedded MSL/metallib/PTX artifacts, content-addressed; canonical digest algorithm named (default SHA-256, matching MD-A9 collision-resistant precedent — operator confirm at U12); verification order fixed (identity verified **before** backend selection); model-to-kernel compatibility binding; tamper/mismatch → **pre-launch failure**; manifest identity never reconstructed from emitted text or path conventions; resource identity (buffers, lifetimes, generations, observations) bound to the composite session.
- **write_scope**: `ngab0-composite-contract.md` (§Manifest, §ResourceIdentity, §Verification).
- **validation**: `grep -n 'content-addressed' ngab0-composite-contract.md`; `grep -n 'before backend selection' ngab0-composite-contract.md`; `grep -n 'pre-launch' ngab0-composite-contract.md`; `grep -n 'SHA-256\|sha-256' ngab0-composite-contract.md`.
- **est_work_tokens**: 4k–7k.
- **tool_latency**: low.
- **dependencies**: U3.
- **parallel_children_considered**: U4 ∥ U5 was considered (split-section files) and rejected — single-file coherence; cross-lane parallelism (U8/U9/U10/U11) absorbs throughput.

### NGAB0-U5 — Backend variants + artifact layout + admission
- **title**: Freeze backend variant matrix, composite artifact layout, and fail-closed admission policy.
- **done_when**: §BackendVariants + §ArtifactLayout + admission frozen: variant rows (MSL source vs metallib vs PTX), artifact layout (one native executable + embedded artifacts + inspectable build dir; `target/faber-llvm/{debug|release}/` precedent), fail-closed admission for unsupported hardware/driver/version/arch/dtype/quant/capability — no CPU fallback; the three operator open questions (llvm-host vs broader identity; Metal source vs metallib; CUDA PTX arch set) recorded with defaults and explicit decision gates.
- **write_scope**: `ngab0-composite-contract.md` (§BackendVariants, §ArtifactLayout, §Admission).
- **validation**: `grep -n 'fail-closed\|fail closed' ngab0-composite-contract.md`; `grep -n 'metallib' ngab0-composite-contract.md`; `grep -n 'PTX' ngab0-composite-contract.md`; `grep -n 'operator decision' ngab0-composite-contract.md`.
- **est_work_tokens**: 3k–6k.
- **tool_latency**: low.
- **dependencies**: U4.
- **parallel_children_considered**: none (serial in file).

### NGAB0-U6 — Build/run UX + error taxonomy + design rules
- **title**: Freeze composite build/run UX, error taxonomy, and the cpo/cxo design rules.
- **done_when**: §Ux + §Errors frozen: build/run UX for the composite path (`faber build/run` product surface; `--backend` becomes capability admission, not default UX); error taxonomy (identity, admission, capability, session, teardown failure classes); cpo/cxo rules recorded verbatim as design rules: **NGAB5's tuning surface is an adapter over the Gradus generation-config contract, never a second authority**; **backend/device selection is an operator/diagnostic override, not default UX**.
- **write_scope**: `ngab0-composite-contract.md` (§Ux, §Errors).
- **validation**: `grep -n 'adapter over the Gradus generation-config' ngab0-composite-contract.md`; `grep -n 'operator/diagnostic override' ngab0-composite-contract.md`; `grep -n '^## .*Errors' ngab0-composite-contract.md`.
- **est_work_tokens**: 3k–5k.
- **tool_latency**: low.
- **dependencies**: U5.
- **parallel_children_considered**: none.

### NGAB0-U7 — Packet completeness: seams, unsupported behavior, version authority
- **title**: Close the packet with frozen/reserved seams, explicit unsupported behavior, and the named version authority.
- **done_when**: §FrozenVsReserved + §Unsupported + §Versioning frozen; named version authority + change procedure labeled *"revisable through PML1/NGAB1"*; explicit unsupported behavior list (no weights embedded, no server/HTTP, no distributed placement, no external spend, no CPU/subprocess-compiler/llama.cpp fallback); consistency pass: all seven section pairs present, cross-references resolve, no dangling decision gates except the three operator questions.
- **write_scope**: `ngab0-composite-contract.md` (§FrozenVsReserved, §Unsupported, §Versioning).
- **validation**: `grep -n 'revisable through PML1/NGAB1' ngab0-composite-contract.md`; `grep -n '^## ' ngab0-composite-contract.md` (all § present); `grep -n 'unsupported' ngab0-composite-contract.md`.
- **est_work_tokens**: 3k–5k.
- **tool_latency**: low.
- **dependencies**: U6.
- **parallel_children_considered**: none (packet gate).

### NGAB0-U8 — C1+C2 radix ownership amendment + MD3I gate amendment
- **title**: Commit the gpu-inference-gguf ownership amendment + migration map and amend the MD3I entry gate.
- **done_when**: `radix/docs/factory/gpu-inference-gguf/gi4-ownership-amendment.md` committed: GI4+ retains neither model-runtime nor serving ownership; model-runtime/decode/KV semantics → **Gradus PML5**, serving/HTTP → **separate inference product repo**; migration map for in-flight GI4 facts (`gi4-contract.md` session facts stay compiler evidence); gguf CAMPAIGN.md edited (dependency rule 6 + GI4–GI7 status/ownership clauses). **C2 same unit**: `radix/docs/factory/gpu-inference-multi-device/CAMPAIGN.md` MD3I entry gate + stage table + dependency rule 4 replaced — "legacy GI4 accepted" → new contract authority: *Gradus PML5 decode/KV semantics + NGAB composite session facts*. Both radix factory READMEs regenerated.
- **write_scope**: `radix/docs/factory/gpu-inference-gguf/` (gi4-ownership-amendment.md + CAMPAIGN.md), `radix/docs/factory/gpu-inference-multi-device/CAMPAIGN.md`, regenerated READMEs. **Radix docs only — no radix source.**
- **validation**: `test -f ../radix/docs/factory/gpu-inference-gguf/gi4-ownership-amendment.md`; `grep -n 'Gradus PML5' ../radix/docs/factory/gpu-inference-multi-device/CAMPAIGN.md`; `grep -n 'does not retain' ../radix/docs/factory/gpu-inference-gguf/CAMPAIGN.md`; `python3 ../radix/scripta/generate-factory-readme.py --check` (radix, both dirs); `git -C ../radix diff --check`.
- **est_work_tokens**: 4k–7k.
- **tool_latency**: medium (README generator runs; no cargo).
- **dependencies**: U2 (ownership-matrix authority).
- **parallel_children_considered**: independent of U3–U7/U9–U11 (different repo); **serializes with in-flight GI3-6/7/8 status edits** in the gguf campaign — read current status line before editing, additive amendment only.

### NGAB0-U9 — C5 claim/capability register skeleton
- **title**: Create the cross-campaign claim/capability register skeleton with Faber composite-executable first rows.
- **done_when**: `ngab0-claim-register.md` with skeleton columns (claim, capability, owner, evidence ref, status, surface, campaign) and first rows for Faber's composite executable surface: one-native-executable build, embedded content-addressed artifacts, admitted backend execution, pre-launch identity verification; cross-campaign scope note (PML0/NGAB1+ and inference-product add rows; no row claims a capability without evidence).
- **write_scope**: `faber/docs/factory/native-gpu-application-bundle/ngab0-claim-register.md` (create).
- **validation**: `test -f ngab0-claim-register.md`; `grep -n 'composite executable' ngab0-claim-register.md`; `grep -n 'evidence' ngab0-claim-register.md`.
- **est_work_tokens**: 2k–4k.
- **tool_latency**: low.
- **dependencies**: U2.
- **parallel_children_considered**: none (small, one file).

### NGAB0-U10 — C7 joint receipt schema + Faber-scoped audit entrypoint
- **title**: Freeze the joint cross-repo receipt schema and add/select the Faber-scoped audit entrypoint.
- **done_when**: `ngab0-receipt-schema.md` frozen: fields for compiler, faber, host, gradus, OS, driver, device, artifact identities + content digests + dirty-state declarations + exact commands, aligned with §Manifest/§Verification; **Faber-scoped audit entrypoint added or selected** — default: add `faber/scripta/check-factory-goal-status` (thin wrapper invoking radix `audit-factory-goal-status.py` with faber's `docs/factory` root, mirroring the README generator's `--factory-root`; the only code this phase writes) with selection rationale recorded; entrypoint runs clean and the faber README `--check` passes.
- **write_scope**: `faber/docs/factory/native-gpu-application-bundle/ngab0-receipt-schema.md` (create); `faber/scripta/check-factory-goal-status` (create, wrapper only); `faber/docs/factory/README.md` (regenerated, never hand-edited).
- **validation**: `test -f ngab0-receipt-schema.md`; `test -x faber/scripta/check-factory-goal-status`; run `faber/scripta/check-factory-goal-status` (exit 0); `python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check`.
- **est_work_tokens**: 3k–6k.
- **tool_latency**: medium (two python scripts).
- **dependencies**: U4 (manifest/verification shape feeds receipt).
- **parallel_children_considered**: none.

### NGAB0-U11 — Generic fixture contract
- **title**: Freeze the generic host-plus-device fixture contract for NGAB1 execution.
- **done_when**: `ngab0-fixture-contract.md` frozen: one scalar host function calling one device kernel; package source sketch (shape, not runnable); CPU oracle definition; expected evidence rows (partition, ABI version, manifest identity, admitted backend, observations); explicit NGAB1 handoff reference. **No runnable code changes** — execution is NGAB1's vertical slice.
- **write_scope**: `faber/docs/factory/native-gpu-application-bundle/ngab0-fixture-contract.md` (create).
- **validation**: `test -f ngab0-fixture-contract.md`; `grep -n 'NGAB1' ngab0-fixture-contract.md`; `grep -n 'oracle' ngab0-fixture-contract.md`.
- **est_work_tokens**: 2k–4k.
- **tool_latency**: low.
- **dependencies**: U3 (partition/ABI), U5 (admitted backend rows).
- **parallel_children_considered**: none.

### NGAB0-U12 — Phase closeout
- **title**: Close NGAB0: status update, evidence consolidation, decisions folded, gates green.
- **done_when**: campaign status line updated (`NGAB0 → done/accepted` per factory status vocabulary); the three operator open questions answered and folded into the packet **or** explicitly deferred with recorded defaults (phase cannot claim the full gate while they dangle); all required outputs listed (composite contract, gguf amendment, pml0 exchange confirmed via U8/U2 references); faber README regenerated; `git diff --check` clean; audit entrypoint + receipt schema referenced from the campaign Validation section.
- **write_scope**: `faber/docs/factory/native-gpu-application-bundle/CAMPAIGN.md` (status line + NGAB0 stage Notes), `faber/docs/factory/README.md` (regenerated).
- **validation**: `grep -n '^\*\*Status\*\*.*NGAB0.*done\|NGAB0.*accepted' CAMPAIGN.md`; `python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check`; `git diff --check -- docs/factory/native-gpu-application-bundle docs/factory/README.md`; `faber/scripta/check-factory-goal-status` exit 0.
- **est_work_tokens**: 2k–4k.
- **tool_latency**: medium (python scripts).
- **dependencies**: U1–U11, plus operator answers to the three open questions.
- **parallel_children_considered**: none (gate).

## Parallelism + Lane Notes

- **Lane A (packet, serial)**: U1 → U2 → U3 → U4 → U5 → U6 → U7. Single-file coherence on `ngab0-composite-contract.md`; parallel section files were considered and rejected (U4 ∥ U5).
- **Lane B (radix docs)**: U8 after U2 — different repo, fully parallel with U3–U7 and lanes C/D. **Hot-path caution**: U8 edits `radix/docs/factory/gpu-inference-gguf/` while GI3-6/7/8 land — read current status, additive amendment only, regenerate README last.
- **Lane C**: U9 after U2; U10 after U4 — distinct files, parallel with lane A/B.
- **Lane D**: U11 after U3+U5 — distinct file.
- **Cross-campaign**: NGAB0 runs in parallel with PML0 (paired packet exchange via U2/U7), GI3/MD0/training, and MD3I gate work. **No shared code hot paths are touched** (all units are docs + the single audit wrapper); the only serialization burden is U8's radix-docs status lines.
- **Overlap rule**: NGAB0 units never write radix/hosts/gradus source, never write `src/package/*` code, never run cargo.

## Checkpoints And Gates

- **C1 gate (hard prerequisite)**: U8 lands — gguf ownership amendment + MD3I gate amendment committed — before NGAB1 lowers and before the phase claims the status-audit gate.
- **Contract-core checkpoint**: after U4 — manifest/identity/verification frozen; U5–U7 may proceed.
- **Packet gate**: U7 — all § present, version authority named.
- **Phase gate**: U12 — full gate from the campaign (package graph, partition, ABI, manifest, resource identity, backend variants, layout, UX, error taxonomy, ownership matrix, one generic fixture contract, stale-plan reconciliation) + C1/C2/C4/C5/C7/C8 dispositions + operator decisions folded + Faber-scoped audit entrypoint selected.
- **Release**: no version bump (docs/tooling only).

## Validation

Phase-level commands (unit-level proofs in each unit; **no cargo build/test** per Cargo discipline):

```bash
cd faber
grep -n '^## ' docs/factory/native-gpu-application-bundle/ngab0-composite-contract.md   # all § present
grep -n 'revisable through PML1/NGAB1' docs/factory/native-gpu-application-bundle/ngab0-composite-contract.md
test -f ../radix/docs/factory/gpu-inference-gguf/gi4-ownership-amendment.md
grep -n 'Gradus PML5' ../radix/docs/factory/gpu-inference-multi-device/CAMPAIGN.md
python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check
scripta/check-factory-goal-status
git diff --check -- docs/factory/native-gpu-application-bundle docs/factory/README.md
```

## Open Questions

Decision owner: **operator** (per campaign). Each is recorded in the packet with a default; U12 folds the answers or defers with recorded defaults.

1. **Artifact identity**: keep the stable user-facing selector `llvm-host` with an embedded-device capability, or define a broader application artifact identity with `llvm-host` as the host lane? Default: retain `llvm-host`, extend capability.
2. **Metal embedding**: MSL source, metallib, or both for the first admitted macOS row? Default: source first (matches current FMIR-carried MSL), metallib reserved.
3. **CUDA PTX arch set**: minimum portable PTX architecture set for NGAB6. Default: the admitted row's arch set recorded, operator-confirmed at U12.

Related (not blocking U1–U11): inference-product repo owner — recorded in U8's migration map, not chosen here.

## Council Dispositions Folded

| Council item | Landed in |
| --- | --- |
| C1 — gguf ownership amendment + migration map (hard prerequisite) | U8 (radix docs write scope) |
| C2 — MD3I entry-gate amendment ("legacy GI4 accepted" → Gradus PML5 + NGAB composite session facts) | U8 (same unit) |
| C4 — shared interface packet: version authority + change procedure, labeled "revisable through PML1/NGAB1", with the listed contents; paired with Gradus packet (no device handle, no HTTP policy) | U2–U7 (packet sections); U7 version authority; U2/§OwnershipMatrix cites the paired `pml0-gradus-contract.md` |
| C5 — cross-campaign claim/capability register skeleton | U9 |
| C7 — joint cross-repo receipt schema + Faber-scoped audit entrypoint | U10 |
| C8 — frozen artifact identity + verification order (digest, verify-before-select, model-to-kernel binding, tamper → pre-launch, no text/path reconstruction) | U4 (also referenced by U10 receipt schema) |
| cpo/cxo — NGAB5 tuning adapts Gradus generation-config; backend/device selection is operator override, not default UX | U6 (§Ux design rules, unit output not code) |
