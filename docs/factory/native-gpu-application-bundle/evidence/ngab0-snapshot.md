# NGAB0 Snapshot — Source revisions and stale-plan reconciliation

**Unit**: NGAB0-U1 (snapshot refresh + stale-plan reconciliation) — see
`../ngab0-delivery.md` NGAB0-U1.
**Captured**: 2026-08-08
**Method**: `git rev-parse HEAD` + `git status --porcelain` per repo; `test -f`
on the four role source files; `grep` proofs on `faber/src/commands/targets.rs`
and `radix/scripta/audit-factory-goal-status.py`. No cargo, no binary runs.

## 1. Repo revisions and dirty state

| Repo | HEAD (full) | Campaign snapshot (`CAMPAIGN.md` §Ground Truth) | Verdict |
| --- | --- | --- | --- |
| `faber` | `44008e63de5352b7985723397f0a0821f0684060` | `26b503a0e3bb` | **drifted** — refreshed to delivery-admission HEAD |
| `radix` | `a01543b06bfe8d99bfb3e6e6e21a2220eda5e6c4` | `a01543b06bfe` | confirmed |
| `hosts` | `e066ee0ae98afa5c7556e1f765072a6357050149` | `e066ee0ae98a` | confirmed |
| `gradus` | `5f92c409f7004dec6fd0068c7201e2cbf96af0b9` | `29d26735d0d9` | **drifted** — refreshed (moved `d7e85aa6` → `5f92c409` during U1 capture; recorded at final observation) |
| `norma` | `84f27dacd6f9bffa4882f789a243ce32f0f060b4` | `84f27dacd6f9` | confirmed |
| `examples` | `aad199ecf07cb23f5d4127c3f68974cab3901235` | `aad199ecf07c` | confirmed |

**Dirty-state declarations**: all six repos report a clean working tree
(`git status --porcelain` empty at capture time). No uncommitted changes in
any repo.

The campaign snapshot must be replaced with this table before NGAB1 lowers
(the authority-order note in `CAMPAIGN.md` §Ground Truth requires refreshed
revisions first). The campaign's `faber` and `gradus` rows are stale.

## 2. Source-file role confirmation

| File | Exists | Size (lines) | Role match (grep refs) |
| --- | --- | --- | --- |
| `faber/src/package/llvm_host.rs` | yes | 720 | `target/faber-llvm/{debug|release}/` layout (ln 8, 42, 80), inspectable `link-manifest.toml` (ln 88, 353), `write_runtime_identity` (ln 346), package→LLVM builder (ln 257) |
| `faber/src/package/device/section.rs` | yes | 833 | FMIR device-section assembly emitting MSL always + PTX via clang NVPTX (ln 35-36), `DeviceSelection`/`FmirDeviceSection`/wire `DeviceProgram` |
| `faber/src/package/mir/image.rs` | yes | 900 | FMIR image construction; Metal MSL + CUDA PTX artifacts (ln 153, 320, 335) |
| `faber/src/package/device/run.rs` | yes | 603 | `faber run --backend` host-session execution: `DeviceBackend` (ln 184), `artifact_for_backend` (ln 201), selection match (ln 220) |

All four files exist and match their delivery-spec roles (`ngab0-delivery.md`
§Repo-Aware Baseline line counts: 720 / 833 / 900 / 603 — all exact).

## 3. `faber targets` row proof

`grep -n 'llvm-host\|metal-text\|llvm-text' faber/src/commands/targets.rs`:

```text
15:    (radix::codegen::Target::MirLlvm, "llvm-text"),
16:    (radix::codegen::Target::MirLlvmHost, "llvm-host"),
17:    (radix::codegen::Target::MirMetal, "metal-text"),
```

All three rows exist statically in the `faber targets` table
(`FABER_TARGET_ROWS`, `targets.rs`). Capability values are derived at runtime
via `target_capabilities_for_surface`; the row presence is grep-proved here
rather than by running the binary (per `ngab0-delivery.md` §Repo-Aware
Baseline). Live-binary capability verification is auditor-owned.

## 4. Ground-Truth table claim reconciliation

Source: `CAMPAIGN.md` §Ground Truth Researched. Every row is marked
confirmed / drifted / superseded with a reference.

| # | Claim | Verdict | Reference |
| --- | --- | --- | --- |
| 1 | `llvm-host` currently reports build/run/package yes | **confirmed** | `targets.rs` row 16 (`MirLlvmHost → "llvm-host"`) + `llvm_host.rs` product path; row-presence grep proof above. Runtime capability reporting is auditor-owned |
| 2 | `llvm-host` emits inspectable modules, link manifest, runtime identity, and native binary | **confirmed** | `llvm_host.rs` ln 8-9 (`target/faber-llvm/{debug|release}/` layout with inspectable link manifest and runtime identity), ln 88 (`link-manifest.toml`), ln 346 (`write_runtime_identity`) |
| 3 | Faber device images already carry MSL and PTX | **confirmed** | `mir/image.rs` ln 153 (MSL + PTX artifacts), `device/section.rs` ln 35-36 (MSL always; PTX through admitted clang NVPTX compiler) |
| 4 | Device execution is currently selected by `faber run --backend` | **confirmed** | `device/run.rs` ln 184 (`backend: DeviceBackend`), ln 201 (`artifact_for_backend`), ln 220 (selection match) |
| 5 | `DeviceProgram` is target-neutral and typed | **confirmed** | Radix MIR: `radix/crates/radix-mir/src/device_semantics.rs`; Faber wire code: `device/section.rs`, `device/run.rs`, `device/host_factory_test.rs` |
| 6 | GI3 proves selected LLM prefill kernels and GPU/oracle comparison | **confirmed** | `radix/docs/factory/gpu-inference-gguf/CAMPAIGN.md` status: GI3-5 landed 2026-08-08 — Q2 GPU-vs-oracle comparison PASSES under `Q2_ENVELOPE` 6.5e-3 (top-1 30 exact, max delta 5.755e-3); evidence files `gi3-prefill-comparison.json` + `gi3-prefill-receipts.md` present |
| 7 | RunPod currently proves infrastructure and small CUDA artifacts | **confirmed** (with nuance) | `radix/docs/factory/runpod-gpu-verification/goal.md` status: Phase 0-1 largely complete (U-01..U-12 landed, first matrix receipt 5/6 cards PASS incl. datacenter L40S/H100, teardown proven live); **U-14 same-artifact run still pending** — infra + small-CUDA proof confirmed, same-artifact portability not yet |
| 8 | Gradus is device-neutral ML computation | **confirmed** | `gradus/docs/factory/production-ml-library/CAMPAIGN.md` (PML0 selected; device-neutral model semantics; Ground-Truth row "Gradus is self-contained and device-neutral") |

No Ground-Truth-table claim is superseded. Two are context-limited by current
state (row 7: same-artifact run pending; row 1: runtime capability values not
re-proven without a binary run).

## 5. Validation-section prose claim — drifted

`CAMPAIGN.md` §Validation (ln ~410):

> "The shared status-audit script is hard-bound to Radix's `docs/factory` and
> has no `--factory-root`."

**Verdict: drifted.** `radix/scripta/audit-factory-goal-status.py` accepts
`--factory-root`:

```text
583:        "--factory-root",
591:    factory = args.factory_root.resolve(),
```

The argument is defined (default = the radix `FACTORY` constant, help text
`argparse.SUPPRESS`) and consumed at line 591. The script therefore is **not**
hard-bound to Radix's `docs/factory`; it can be invoked with any factory root.

**Correction**: a Faber-scoped audit entrypoint is feasible by wrapping the
shared script with `--factory-root docs/factory` (NGAB0-U10 adds the
`faber/scripta/check-factory-goal-status` wrapper). The campaign's remaining
prose — "NGAB0 must add or select a Faber-scoped audit entrypoint before
claiming the full status-audit gate" — stands and is now aligned with the
audit script's actual interface.
