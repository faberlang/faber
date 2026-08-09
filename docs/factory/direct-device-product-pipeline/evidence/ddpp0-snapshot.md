# DDPP0 Snapshot — source SHAs, dirty-state classification, Ground-Truth baseline

**Unit**: DDPP0-U1 (phase root) — refresh campaign source snapshot and classify dirty state.
**Date**: 2026-08-08 (refresh run; all commands `git`/`grep`/`test` only — no cargo).
**Scope**: this file only. No other repo edits. `faber-runtime/` read-only per phase contract.
**Purpose**: C0 gate root — the baseline every later DDPP0 unit (U2–U8) and the U9 closeout compare against.

---

## 1. Repo HEAD SHAs + dirty state (seven repos)

| Repo | HEAD SHA | Branch | Dirty files (porcelain count) |
| --- | --- | --- | --- |
| `faber` | `754a8e6450424805f7719bfa6796a61854d6fe5b` | main | 3 |
| `radix` | `0bb5ebbd6860e31c57900fbe7ba46a951ad6b7c3` | main | 4 |
| `hosts` | `e066ee0ae98afa5c7556e1f765072a6357050149` | main | 0 |
| `faber-runtime` | `10d48ea474358e6a6d36e54d56f051ae20b3824f` | main | 0 |
| `gradus` | `de017eb80723cfeafe974681fa9e17fc68a61ba2` | main | 0 |
| `triga` | `e6394b30f3bab8a2b76f29a51c3bac411befd64b` | main | 0 |
| `examples` | `aad199ecf07cb23f5d4127c3f68974cab3901235` | main | 1 |

Recorded via `git -C <repo> rev-parse HEAD` and `git -C <repo> status --porcelain`
at snapshot time. No compare gate — this is the baseline, not a drift check against
anything prior. Note: `faber-runtime` HEAD (`10d48ea47435…`) matches the prefix
`10d48ea47435` pinned in `ddpp0-delivery.md` §Repo-Aware Baseline (GT-8 below).

### 1.1 Dirty paths (exact)

**`faber` (3):**
- `docs/factory/README.md` (M) — factory README regenerated to include the new
  `direct-device-product-pipeline` goal (goals scanned 20→21, planned 7→8, DDPP0
  row added). This is the generator's expected output for the campaign tree; **in-scope
  for DDPP0-U9** (README regeneration is U9's write_scope) — not foreign WIP. Excluded
  from the U1 write set by write_scope.
- `src/package/device/prefill_run_test.rs` (M) — a temporary env-gated GI3-7 diagnostic
  probe test (comment: "TEMPORARY GI3-7 diagnostic probe … Reverted before the unit
  commit"). **Foreign WIP (GI3 campaign)**. Excluded from all DDPP0 write sets.
- `docs/factory/direct-device-product-pipeline/` (??) — untracked; the DDPP0 campaign
  artifact tree itself (`CAMPAIGN.md`, `council-review-2026-08-08.md`,
  `ddpp0-delivery.md`). In-scope campaign surface; this snapshot is written into its
  `evidence/` subdir.

**`radix` (4):**
- `crates/radix-mir-llvm/src/nvvm/artifact.rs` (M) — **foreign WIP** (source; NVVM
  artifact, GPU-target work). Excluded.
- `docs/factory/README.md` (M) — regenerated radix README (DDCP0 + TR7 stage-7
  entries). Foreign to DDPP0 write sets (DDPP0 does not write radix docs). Excluded.
- `docs/factory/direct-device-compilation-pipeline/` (??) — untracked; **DDCP0 paired
  delivery**, in-flight sibling campaign. Not a DDPP0 write path; DDPP0-U6 records
  `PENDING-AGREEMENT` for fields not yet landed. Excluded from DDPP0 write sets.
- `docs/factory/gpu-training-lowering/stage-7-delivery.md` (??) — untracked; **TR7
  stage-7 delivery doc**, in-flight. TR7 is a DDPP0-U5 consumer row. Excluded from
  DDPP0 write sets.

**`examples` (1):**
- `training/mlp/oracle/tools/__pycache__/` (??) — untracked Python bytecode cache.
  Junk. Excluded.

**`hosts`, `faber-runtime`, `gradus`, `triga`:** clean (0 dirty paths).

### 1.2 Dirt classification (in-scope vs foreign)

- **In-scope dirty for DDPP0-U6** — the NGAB0 packet + amendment paths
  `faber/docs/factory/native-gpu-application-bundle/{ngab0-composite-contract.md,
  ngab0-receipt-schema.md, ngab0-fixture-contract.md, ngab1-delivery.md,
  ngab2-delivery.md}`. All five files exist and are currently **committed-clean**
  (no uncommitted changes). They are campaign-authorized **write paths for the U6
  amendment** (one-prepared-region revision + NGAB1/NGAB2 retarget) — "in-scope
  dirty" in the sense that U6 owns modifications to them, and no other dirty path
  may be touched by DDPP0.
- **In-scope for DDPP0-U9** — `faber/docs/factory/README.md` regeneration (the
  currently-modified README is the stale state U9 will regenerate and commit).
- **In-scope campaign tree** — `faber/docs/factory/direct-device-product-pipeline/`
  (untracked; all DDPP0 units' artifacts land here).
- **All other foreign dirt is excluded from DDPP0 write sets**: faber
  `src/package/device/prefill_run_test.rs` (GI3-7 probe); radix
  `crates/radix-mir-llvm/src/nvvm/artifact.rs`, `docs/factory/README.md`,
  `docs/factory/direct-device-compilation-pipeline/` (DDCP0),
  `docs/factory/gpu-training-lowering/stage-7-delivery.md` (TR7); examples
  `training/mlp/oracle/tools/__pycache__/`. DDPP0 units will not stage, revert, or
  format any of these.

---

## 2. Role-match grep proofs (files required by the unit)

All paths verified present (`test -f`) and role-matched (`grep -n`) at snapshot time.

| File | Role in contract | Proof (file:line) |
| --- | --- | --- |
| `src/package/device/section.rs` | FMIR device-section assembly | `device_section_for_program` (section.rs:45); "Assemble the FMIR `device` section for a constructed device program" (section.rs:33) |
| `src/package/device/run.rs` | device session route execution | `execute_device_route` (run.rs:182); `execute_session_receipts` (run.rs:88) |
| `src/package/device/prefill_run.rs` | `faber::` runtime imports (dequant/gguf/prefill) | `use faber::dequant::{dequant_tensor, OracleReceipt}` (prefill_run.rs:62); `use faber::gguf::admit_file` (prefill_run.rs:63); `use faber::prefill::{…}` (prefill_run.rs:64–65) |
| `src/package/llvm_host.rs` | builds/links `faber-host-llvm` archive | `../faber-runtime` root join (llvm_host.rs:157); `libfaber_host_llvm.a` (llvm_host.rs:172); archive-build doc "`faber-runtime/hosts/llvm` (staticlib)" (llvm_host.rs:147); `runtime_artifact_metadata` reading `faber-runtime/hosts/llvm/Cargo.toml` (llvm_host.rs:655–661) |
| `src/package/dispatch.rs` | routes covered by faber-runtime dispatch | "Routes covered by `faber-runtime` `BuiltinRuntimeDispatch` / `builtin_route_frames`" (dispatch.rs:43–46); "Keep in sync with `faber-runtime/src/frame.rs` `builtin_route_frames`" (dispatch.rs:46); `is_builtin_ad_route` (dispatch.rs:47) |
| `src/package/cargo.rs` | generated host registration / faber-runtime path-link | `support.faber_runtime()` (cargo.rs:127); `toml_string("faber-runtime")` (cargo.rs:138) |
| `Cargo.toml` (features block) | `default = ["full-targets"]`; `hir-rust` feature | `default = ["full-targets"]` (Cargo.toml:70); `full-targets = ["hir-rust", …]` (Cargo.toml:71–72); `hir-rust = ["radix/hir-rust", "dep:faber-hir-rust"]` (Cargo.toml:86) |
| `core-support-manifest.txt` | core-support logical roots | 10 roots: `faber-runtime`, `radix/crates/radix-runtime-contract`, `hosts/crates/{host-kernel,host-native,aleator,http,consolum,processus,solum,tempus}` (entire file) |
| `hosts/macos-arm64/Cargo.toml` | faber-runtime path dep + host crates + cfg-gated metal | `faber = { package = "faber-runtime", path = "../../faber-runtime" }` (macos-arm64/Cargo.toml:10); `host-kernel`/`aleator`/`consolum`/`processus`/`solum`/`tempus` (lines 11–16); `libloading = "0.8"` (line 24); `[target.'cfg(target_os = "macos")'.dependencies] metal = "0.33"` (lines 28–29) |
| `hosts/wasm/Cargo.toml` | `radix-host-abi` only, no faber-runtime | `radix-host-abi = { path = "../../radix/crates/radix-host-abi" }` (wasm/Cargo.toml:14); no faber-runtime dep in file |
| `faber-runtime/hosts/llvm/Cargo.toml` | `faber-host-llvm` (rlib+staticlib, dep faber-runtime) | `name = "faber-host-llvm"` (line 2); `crate-type = ["rlib", "staticlib"]` (line 12); `faber = { package = "faber-runtime", path = "../.." }` (line 15) |
| `faber-runtime/src/lib.rs` | module list (~45 modules) | 41 `pub mod` declarations (lib.rs:6–47) + 1 private `mod autograd` (lib.rs:8) = **42 top-level modules**; every module named in the Ground-Truth table is present (`device`, `device_identity`, `device_set`, `discovery`, `fake_device`, `partition`, `transport`, `session`, `frame`, `http`, `gguf`, `dequant`, `prefill`, `kv_cache`, `decoder_ops`, `greedy_run`, `cpu_oracle`, `autograd`, `tensor`, `sparsa`, `packed_numeric`, `valor`, `textus`, `json`, `instans`, `intervallum`, `display`, `failable`, `ascii`, `arena`, `host_abi`, `bound_plan`, `capability`, `policy`, `repack_plan`, `execution_transaction`, `tokenizer`, …) |

Additional role-match greps (import sites listed in the Ground-Truth table):
`faber::device::DeviceSelection`/`DeviceBackend` at `src/cli/mod.rs:383–387`,
`src/commands/run.rs:14`, `src/package/host_factory.rs:30`,
`src/package/device/mod.rs:48`, `src/package/mir/mod.rs:32`
(+ `mir/routes.rs` present; device selection in `mir/mod.rs:127,140,268–273`),
`src/package/manifest.rs:373–377`. All present.

---

## 3. Ground-Truth table claim status (per `ddpp0-delivery.md` §Repo-Aware Baseline)

**Status vocabulary**: each Ground-Truth claim is marked **confirmed** (matches live
source at snapshot), **drifted** (live source differs from the recorded claim — the
delta is recorded), or **superseded** (the claim no longer applies — the replacement
is recorded). Every claim carries a reference. In this refresh every claim is
**confirmed**; no claim is marked **drifted** or **superseded**.

| # | Ground-Truth claim | Status | Reference |
| --- | --- | --- | --- |
| GT-1 | `faber/Cargo.toml`: `default = ["full-targets"]`; `hir-rust = ["radix/hir-rust", "dep:faber-hir-rust"]` | confirmed | `faber/Cargo.toml:70, 86` |
| GT-2 | Unconditional GPU/runtime/Hosts deps: `radix-mir-metal`, `radix-mir-llvm`, `radix-mir-fmir`, `faber-runtime` (package `faber`), `faber-host-macos-arm64`, `faber-host-wasm` | confirmed | `faber/Cargo.toml:26, 30, 31, 39, 40, 43` (plain `[dependencies]` path deps, no `optional`) |
| GT-3 | `faber::device::DeviceSelection`/`DeviceBackend` import sites: `cli/mod.rs:383–387`, `commands/run.rs:14`, `host_factory.rs:30`, `device/mod.rs:48`, `mir/{mod,routes}.rs`, `manifest.rs:373–377` | confirmed | greps in §2 (exact lines match) |
| GT-4 | `faber::dequant::dequant_tensor` + `faber::gguf::admit_file` + `faber::prefill::*` in `prefill_run.rs:62–65` | confirmed | `prefill_run.rs:62–65` |
| GT-5 | `faber::Valor` + generated host registration via `cargo.rs:127–138` (materialized `support.faber_runtime()`, path-links `faber-runtime`) | confirmed | `cargo.rs:127, 138` |
| GT-6 | `dispatch.rs:43–46` routes covered by `faber-runtime` `BuiltinRuntimeDispatch`/`builtin_route_frames` (keep-in-sync with `faber-runtime/src/frame.rs`) | confirmed | `dispatch.rs:43–47` |
| GT-7 | `faber-runtime` snapshot `10d48ea47435` | confirmed | HEAD `10d48ea474358e6a6d36e54d56f051ae20b3824f` (prefix match) |
| GT-8 | `faber-runtime` carries ~45 modules (named list) | confirmed | `faber-runtime/src/lib.rs:6–47` — 42 top-level modules (41 `pub mod` + 1 private `mod autograd`); every named module present; "~45" tolerance noted, exact count recorded in §2 |
| GT-9 | `faber-runtime/hosts/llvm/` = `faber-host-llvm` (rlib+staticlib, dep `faber-runtime`) | confirmed | `faber-runtime/hosts/llvm/Cargo.toml:2, 12, 15` |
| GT-10 | `faber-runtime/docs/factory/autograd-substrate-inventory.md` is the existing substrate inventory U5 must reconcile | confirmed | file present |
| GT-11 | `llvm_host.rs:147–172, 655–661` builds `libfaber_host_llvm.a` from `../faber-runtime` and links it | confirmed | `llvm_host.rs:147–172` (archive build), `:655–661` (`runtime_artifact_metadata`) |
| GT-12 | `hosts/macos-arm64/Cargo.toml:10` `faber` path dep + host-kernel/aleator/consolum/processus/solum/tempus + libloading + cfg-gated `metal` | confirmed | `hosts/macos-arm64/Cargo.toml:10–16, 24, 28–29` |
| GT-13 | `hosts/wasm/Cargo.toml` = `radix-host-abi` only (no faber-runtime) | confirmed | `hosts/wasm/Cargo.toml:14` |
| GT-14 | `faber/core-support-manifest.txt` roots faber-runtime, `radix/crates/radix-runtime-contract`, `hosts/crates/{host-kernel,host-native,aleator,http,consolum,processus,solum,tempus}` | confirmed | `core-support-manifest.txt` (10 roots) |
| GT-15 | `faber/build.rs` assembles `core-support.tar.zst` + `.sha256` + files sha256 | confirmed | `faber/build.rs:33–41` (`core-support.tar.zst`, `core-support.sha256`, `core-support.files.sha256`) |
| GT-16 | TR7 (`radix/docs/factory/gpu-training-lowering/stage-7-delivery.md`); TR7-U1 pins companion revisions incl. faber-runtime | confirmed | `stage-7-delivery.md:67` (component SHAs incl. faber-runtime), `:136` (sibling-pins record incl. faber-runtime) |
| GT-17 | PML/NGAB packet authority: `ngab0-composite-contract.md` §Versioning | confirmed | `ngab0-composite-contract.md:729` (`## Versioning`) |
| GT-18 | Tooling: `faber/scripta/check-factory-goal-status` exists; README generator `../radix/scripta/generate-factory-readme.py` exists | confirmed | both files present |

**Result**: 18/18 claims confirmed; 0 drifted; 0 superseded at snapshot time.

---

## 4. Snapshot validity

- `git rev-parse HEAD` for all seven repos: recorded in §1 (no compare gate).
- `test -f` on all files listed in the unit: all present (§2).
- `grep -n 'hir-rust' faber/Cargo.toml`: 4 hits — `:24` (faber-hir-rust dep),
  `:72` (in `full-targets`), `:86` (feature definition), `:101` (workspace member).
- `git diff --check`: clean on the snapshot commit.

---

*Snapshot artifact — C0 gate root for DDPP0. Planning/evidence only; no product code;
`faber-runtime/` untouched.*
