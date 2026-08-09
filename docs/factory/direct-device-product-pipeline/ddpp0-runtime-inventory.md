# DDPP0 Runtime Inventory — faber-runtime consumers, modules, and import sites with exactly one destination + a deletion receipt

**Unit**: DDPP0-U5 (lane B — inventory; council C3/C4). **Date**: 2026-08-09.
**Repo**: faber (control plane). `faber-runtime/` is **read-only** — no repo edits, no moves.
**Status**: frozen by DDPP0-U5.
**Write scope**: this file only.
**Destination authority**: campaign §`faber-runtime` Decomposition Target (routing authority) +
`ddpp0-contract.md` §PartitionOwnership / §GeneratedRustSupport / §DeletionRule (DDPP0-U4) +
`ddpp0-support-archive.md` (DDPP0-U8) + `ddpp0-feature-isolation.md` (DDPP0-U7).
**Snapshot baseline**: `evidence/ddpp0-snapshot.md` (DDPP0-U1; faber-runtime HEAD `10d48ea47435`).
**Existing substrate inventory reconciled**: `faber-runtime/docs/factory/autograd-substrate-inventory.md` (§10).

**Row count**: this document contains **143** table rows (`grep -c '^| ' ddpp0-runtime-inventory.md`),
covering 42 faber-runtime top-level modules + `faber-runtime/hosts/llvm` + every live consumer
import, path dependency, manifest schema, and route named in the DDPP0-U5 scope.

**Authority order**: live source/tests and live `faber targets` → accepted artifact schemas +
hardware receipts → this phase's frozen contracts → campaign prose. Every module and import row
below carries a live-source citation (file:line) so a later DDPP8 migration unit can re-verify
before moving anything. **Every module and every import has exactly one destination + one
deletion receipt.** No row may ship to two owners; where a module contains two campaign families,
the single module destination is named and the intra-module sub-surface split is recorded in the
deletion receipt (never as dual authority — see §2 rule 2).

---

## 1. Destination vocabulary (exactly one destination per module/import)

Shorthand names for the campaign §`faber-runtime` Decomposition Target rows (DDPP0-U4
§PartitionOwnership is the owner-per-surface authority; each destination inherits that section's
product consequence).

| # | Destination | Campaign Decomposition Target row | Closeout rule |
| --- | --- | --- | --- |
| 1 | **GENRUST** — Faber-owned generated-Rust support crate | `ascii`, `textus`, `valor`, `json`, `instans`, `intervallum`, display/failable/recovery helpers, CPU `tensor`/`sparsa` semantics needed by generated Rust | target-specific name + dependency (DDPP1 operator gate, OQ1); no device session behavior; must not transitively pull device or Hosts (C2) |
| 2 | **GENRUST-CONTRACT** — Faber-owned generated-Rust support contract | Sermo/frame language carriers, generated-Rust client calls, Rust `HostDispatch` trait | Hosts depends on this narrow contract and installs an implementation; no concrete effect or GPU launch behavior here |
| 3 | **HOSTS-PROVIDERS** — Hosts providers | `frame` built-in filesystem/process/console/time/random/HTTP implementations and `http` client effects | one provider authority; no runtime fallback duplication |
| 4 | **RADIX-HOST-ABI** — `radix-host-abi` contract (+ Hosts-owned LLVM process support on the host side) | LLVM/C `ad` and host-call symbols/layouts | one versioned ABI; ordinary host effects preserved |
| 5 | **RADIX-RUNTIME-CONTRACT** — `radix-runtime-contract` | generated-language value/frame wire contracts | compiler-owned internal ABI facts; no Hosts implementation or driver code |
| 6 | **RADIX-ARTIFACT+FABER-BUILD** — Radix artifact contract plus Faber build configuration | `device` selection/build metadata | absent from generated language values |
| 7 | **HOSTS-COORD** — Hosts or a later execution coordinator | physical device handles, discovery, identity, topology, partition, health, transport, transactions, bound plans | no generated-Rust/runtime ownership |
| 8 | **GRADUS-PML** — Gradus PML stages | `gguf`, tokenizer, quantized model interpretation, logical decode/KV/sampling/prefill semantics | old paths deleted after accepted Gradus consumer |
| 9 | **RADIX-LEAF** — Radix leaf (lowering/repacking); physical upload/repacking in Hosts | backend-specific lowering/repacking | no model semantics in Hosts |
| 10 | **REPO-ORACLE** — repo-owned test/oracle fixtures | CPU logits/decode/greedy/autograd reference code | not linked into production support |
| 11 | **HOSTS-LLVM** — Hosts-owned LLVM process support | `faber-runtime/hosts/llvm` | linked by Faber `llvm-host`; no fake physical GPU claim |

Rules (normative, folded from DDPP0-U4 §DeletionRule):

1. **Exactly one destination per row.** Each module and each import has exactly one destination
   column value. The temporary `faber-runtime` repository is in migration scope; it is **not a
   forward owner** and **cannot become a containment facade** (§DeletionRule rule 2).
2. **Intra-module splits are recorded, not dual authority.** Where a module compiles two campaign
   families into one file (notably `device`, `frame`, `host_abi`), the module's single destination
   is the family that owns its contract surface, and the deletion receipt names the sub-surface
   that must land with the other family. A later migration splits the file; no route alias or
   forwarding crate is installed (§DeletionRule rule 2/3).
3. **Deletion receipt = the condition that closes the row.** Each row's receipt states what must
   be true before the `faber-runtime` module/import can be deleted. The **final deletion gate**
   checks `Cargo.toml`, `Cargo.lock`, `core-support-manifest.txt`, `build.rs`, generated Cargo
   manifests, CI sibling checkouts, release notes, and all source imports (§DeletionRule rule 5).
4. **Renaming ≠ decomposition.** Moving only the crate name while keeping the ownership mix
   satisfies nothing (§DeletionRule rule 3).
5. **Module count closed.** The delivery baseline said "~45 modules"; the snapshot (GT-8) closed
   the count at **42 top-level modules** (41 `pub mod` + 1 private `mod autograd` in
   `src/lib.rs:6–47`). This inventory covers all 42 + the `hosts/llvm` crate + the 10 submodule
   source files (`execution_transaction/{backend,errors,mirror,receipt,reservation,state_machine,
   transaction}.rs`, `tokenizer/{bpe,pretoken}.rs` with `mod.rs`).

---

## 2. Module-by-module inventory — `faber-runtime/src/` (42 top-level modules) + `hosts/llvm`

Module registration: `faber-runtime/src/lib.rs:6–47` (`pub mod arena; … pub mod valor;`, with the
private `mod autograd;` at line 8). `hosts/llvm` = `faber-host-llvm` crate
(`hosts/llvm/Cargo.toml:2,12,15`; dep `faber-runtime`, `crate-type = ["rlib","staticlib"]`).

| Module | Role evidence (live source) | Destination (exactly one) | Deletion receipt |
| --- | --- | --- | --- |
| `arena` | Generational arena handles for stable identity; re-exported for generated Rust (`lib.rs:49`) | **GENRUST** | `faber::Arena`/`ArenaHandle` re-point to the support crate; generated code + `examples/arena-handle` (semantics mirror, `arena-handle/src/main.fab:57`) verified against the moved carrier |
| `ascii` | `ascii` runtime newtype; re-exported (`lib.rs:50`) | **GENRUST** | moved with the generated-Rust value carriers; no behavior change |
| `autograd` (private) | Internal dense `Tensor<f32>` autograd tape (scaffold, not PyTorch-equivalent) | **REPO-ORACLE** | retained as a repo-owned test/oracle fixture (`autograd-substrate-inventory.md` §10: tape survives as oracle/debug path under G-A-01); not linked into production support |
| `bound_plan` | `BoundDistributedPlan` topology-bound plan (multi-device MD2-B1) | **HOSTS-COORD** | moves with the multi-device coordinator surface; no generated-Rust/runtime ownership (§1 row 7) |
| `capability` | GI3-2 S2 structured backend capability result types | **HOSTS-COORD** | backend-capability result types are device-lifecycle facts; move to Hosts leaf/coordinator surface |
| `cpu_oracle` | GI2-3 CPU one-position logits oracle (pinned SmolLM2 row) | **REPO-ORACLE** | re-homed as a repo-owned oracle fixture; never linked into a product binary |
| `cursor_stream` | Cursor-stream materialization host binding (P5; `__faber_rt_v1_cursor_stream` ABI row) | **GENRUST-CONTRACT** | the Rust client-call binding moves with the generated-Rust support contract; the ABI row stays `radix-host-abi` (row 4); `hosts/wasm/src/collections.rs` parity note re-pointed |
| `decoder_ops` | GI2-2 CPU decoder op surface — correctness oracle | **REPO-ORACLE** | re-homed as repo-owned oracle fixture; not production |
| `dequant` | GI2-1 CPU dequant core (GGML block dequantization for the pinned row) | **GRADUS-PML** | consumed by faber `prefill_run.rs:62`; after the accepted Gradus PML consumer (PML0-U7 decision: migrate or retire; PML2-U5 enforces) the old path is deleted |
| `device` | `DeviceBackend`/`DeviceSelection` (build/selection metadata, `from_spelling`) + `DeviceHandleKind`/`DeviceHandle` (opaque host-owned handle → `Valor` lowering, `device.rs:125–183`) | **RADIX-ARTIFACT+FABER-BUILD** | selection spelling/identity moves to the Radix artifact contract + Faber build configuration; **intra-module split**: `DeviceHandle`/`DeviceHandleKind` (physical-handle carriers) land with **HOSTS-COORD** row 7 — they must not ride the support crate (§1 rule 2) |
| `device_identity` | Physical device identity + health epoch types (MD1-D1) | **HOSTS-COORD** | moves with physical device identity surface |
| `device_set` | Device-set membership + topology schema (MD1-S1) | **HOSTS-COORD** | moves with device topology surface |
| `discovery` | Timestamped device discovery facts (MD1-D1) | **HOSTS-COORD** | moves with discovery surface |
| `display` | Shared scalar display helpers (`display.rs`; re-exported via `lib.rs:52–57`) | **GENRUST** | moved with generated-Rust support crate |
| `execution_transaction` | `ExecutionTransaction` state machine + staged write-set + atomic publication (MD3-X1); 7 submodule files (`backend.rs`, `errors.rs`, `mirror.rs`, `receipt.rs`, `reservation.rs`, `state_machine.rs`, `transaction.rs`) | **HOSTS-COORD** | moves with transactions/coordinator surface; all 7 submodule files move together |
| `failable` | Failable status/payload host bindings (P10) | **GENRUST** | "display/failable/recovery helpers" family (row 1) |
| `fake_device` | MD3-F1 fault-injecting `DeviceExecutionBackend` (wraps X1 happy-path fake) | **REPO-ORACLE** | fake backends validate sequencing and numerics; they never prove physical GPU execution (campaign §Development Posture) |
| `frame` | Sermo/frame carriers + `HostDispatch` trait + `builtin_route_frames` (`frame.rs:881`; re-exports `lib.rs:58–60`) | **GENRUST-CONTRACT** | Sermo/frame language carriers + `HostDispatch` trait move with the support contract (row 2); **intra-module split**: the `builtin_route_frames`/`dispatch_builtin_route` implementation arms (tempus/solum/processus/consolum/aleator) land with **HOSTS-PROVIDERS** row 3 (§1 rule 2) |
| `gguf` | GGUF v3 container admission core (pinned row) | **GRADUS-PML** | PML2-U5 migrates/retires the admission code by code location; no dual authority |
| `greedy_run` | GI2-4 free-running greedy decode + agreement/first-divergence record | **REPO-ORACLE** | CPU decode reference → repo-owned oracle fixture |
| `host_abi` | Stable C ABI shared by LLVM-emitted programs and the LLVM host runtime (`host_abi.rs`; `FaberRtContextV1` etc.) | **RADIX-HOST-ABI** | the C ABI symbols/layouts move to `radix-host-abi` (row 4); the LLVM host side of the symbols moves to **HOSTS-LLVM** (row 11) — one versioned ABI authority |
| `http` | HTTP client effects (`http.rs`) | **HOSTS-PROVIDERS** | `http` client effects → one Hosts provider authority; no runtime fallback duplication |
| `instans` | `instans` runtime — absolute point-in-time + precision contract | **GENRUST** | moved with generated-Rust value carriers |
| `intervallum` | `intervallum<T>` numeric interval with glyph-encoded inclusivity | **GENRUST** | moved with generated-Rust value carriers |
| `json` | Object-rooted JSON document carrier | **GENRUST** | moved with generated-Rust value carriers (`prefill_run.rs:64` re-points) |
| `kv_cache` | GI4-1 typed `KvCacheLayout` (CTO S5) | **GRADUS-PML** | logical KV semantics → Gradus PML; old path deleted after accepted consumer |
| `or_recovery` | `_or` recovery host bindings (P6) | **GENRUST** | "display/failable/recovery helpers" family (row 1) |
| `packed_numeric` | Packed U4 block carriers + reference tensor integration row | **GRADUS-PML** | quantized model interpretation family (row 8); carriers move with the Gradus quantized-storage consumer |
| `partition` | Virtual device partition contract (MD1-V1) | **HOSTS-COORD** | moves with partition/coordinator surface |
| `policy` | Priority policy engine — two-level evaluator (MD-A8/MD1-P1) | **HOSTS-COORD** | moves with multi-device policy surface |
| `prefill` | GI3-5 prefill program builder + GPU-vs-oracle comparison harness | **GRADUS-PML** | prefill semantics → Gradus PML; `prefill_run.rs:65` re-points after accepted consumer |
| `quantized_tensor_layout` | `QuantizedTensorLayout` — sole admitted quantized storage contract (GI1-2) | **GRADUS-PML** | quantized storage contract → Gradus PML quantized surface |
| `regex` | `regex` runtime carrier (compile-time literal only today) | **GENRUST** | moved with generated-Rust value carriers |
| `repack_plan` | GI3-2 S1 per-tensor-class representation/repack plan types | **RADIX-LEAF** | backend-specific lowering/repacking plan facts → Radix leaf (row 9); physical upload/repacking stays in Hosts |
| `session` | GI4-1 frozen inference session contract (CTO S5 surface) | **GRADUS-PML** | logical decode/KV/sampling/prefill semantics → Gradus PML |
| `sparsa` | Sparse numeric tensor runtime for generated Rust | **GENRUST** | CPU `tensor`/`sparsa` semantics needed by generated Rust (row 1) |
| `tensor` | Dense numeric tensor runtime for generated Rust | **GENRUST** | CPU `tensor`/`sparsa` semantics needed by generated Rust (row 1) |
| `tensor_view` | GI1-4 deterministic host-readable (CPU) tensor view over the admitted GGUF row | **GRADUS-PML** | GGUF-seam tensor view → Gradus PML quantized surface (`prefill_run.rs:69` re-points) |
| `textus` | `textus` scalar helpers for generated Rust | **GENRUST** | moved with generated-Rust value carriers |
| `tokenizer/` | gpt2 BPE + `smollm` pre-tokenizer (pinned row parity; `tokenizer/mod.rs`, `bpe.rs`, `pretoken.rs`) | **GRADUS-PML** | tokenizer family (row 8); PML2-U5 migrates/retires by code location; tokenizer identity facts (PML0 capsule) re-point |
| `transport` | Typed/ranged transport + host-staged adapter + receipts (MD3-T1) | **HOSTS-COORD** | moves with transport/coordinator surface |
| `valor` | Canonical dynamic value carrier for `valor`/`ignotum` lowering | **GENRUST** | moved with generated-Rust value carriers; `faber::Valor` re-points |
| `hosts/llvm` (`faber-host-llvm`) | rlib+staticlib LLVM host runtime archive (`hosts/llvm/Cargo.toml:2,12,15`; produces `libfaber_host_llvm.a`), consumed by `llvm_host.rs` | **HOSTS-LLVM** | becomes Hosts-owned LLVM process support with explicit ABI version + SHA-256 content receipt (DDPP0-U8); stale last-good archive reuse removed (fail-closed rebuild) |

---

## 3. Faber-side import sites (each with exactly one destination)

| Import site (live source) | What it consumes | Destination (exactly one) | Deletion receipt |
| --- | --- | --- | --- |
| `src/cli/mod.rs:383–387` | `BackendSelection::selection() → faber::device::DeviceSelection` (Metal/Cuda/Auto) | **RADIX-ARTIFACT+FABER-BUILD** | CLI selection re-points to the Radix artifact-contract selection identity + Faber build-config spelling |
| `src/commands/run.rs:14` | `use faber::device::{DeviceBackend, DeviceSelection}` | **RADIX-ARTIFACT+FABER-BUILD** | same re-point; `faber run` backend selection unchanged in behavior |
| `src/package/host_factory.rs:30` | `use faber::device::{DeviceBackend, DeviceSelection}` (host-construction policy) | **RADIX-ARTIFACT+FABER-BUILD** | host factory re-points; `resolve_backend_selection` uses the Faber build-config surface |
| `src/package/device/mod.rs:48` | `use faber::device::{DeviceBackend, DeviceSelection}` (device-program constructor) | **RADIX-ARTIFACT+FABER-BUILD** | device-program constructor re-points; no device-handle surface enters generated values |
| `src/package/device/device_test.rs:2`, `prefill_run_test.rs:19` | test imports of `faber::device` | **RADIX-ARTIFACT+FABER-BUILD** | tests re-point with the production import |
| `src/package/mir/mod.rs:32,127,140,268–276` (+ `routes.rs`) | `faber::device::DeviceSelection` mapped from `FmirDeviceSelection` (image-runner route selection) | **RADIX-ARTIFACT+FABER-BUILD** | FMIR-image route selection re-points; route selection never reaches GPU submission (§5) |
| `src/package/manifest.rs:373–377` | `manifest_backend_selection` → `faber::device::DeviceSelection::from_spelling` (`faber.toml [device] backend`) | **RADIX-ARTIFACT+FABER-BUILD** | manifest backend spelling re-points to the Faber build-configuration surface |
| `src/package/manifest_test.rs:5`, `host_factory_test.rs:8` | test imports of `faber::device::DeviceSelection` | **RADIX-ARTIFACT+FABER-BUILD** | tests re-point with the production import |
| `src/package/device/prefill_run.rs:62` | `use faber::dequant::{dequant_tensor, OracleReceipt}` | **GRADUS-PML** | after the accepted Gradus PML consumer, `dequant_tensor` comes from Gradus |
| `src/package/device/prefill_run.rs:63` | `use faber::gguf::admit_file` | **GRADUS-PML** | `admit_file` comes from Gradus (PML2-U5 migration) |
| `src/package/device/prefill_run.rs:64` | `use faber::json::Json` | **GENRUST** | `Json` re-points to the generated-Rust support crate |
| `src/package/device/prefill_run.rs:65` | `use faber::prefill::{compare_gpu_logits, ExecutableRegime, …}` | **GRADUS-PML** | prefill comparison/regime semantics come from Gradus |
| `src/package/device/prefill_run.rs:69` | `use faber::tensor_view::TensorView` | **GRADUS-PML** | tensor-view over the GGUF row comes from Gradus |
| `src/package/device/prefill_run.rs:70` | `use faber::valor::Valor` | **GENRUST** | `Valor` re-points to the generated-Rust support crate |
| `src/package/cargo.rs:127–138` | materialized `support.faber_runtime()` + generated manifest `faber = { package = "faber-runtime", path = … }` | **GENRUST** | generated Cargo manifests regenerate to the support crate's target-specific name; `render_generated_cargo_toml_with_support` re-points |
| `src/package/cargo.rs:13–19` | `RustRuntimePlan.needs_faber` (HIR/plan fact) | **GENRUST** | the "needs faber" fact becomes "needs the generated-Rust support crate" |
| `src/package/dispatch.rs:43–47` | `is_builtin_ad_route` — routes covered by `faber-runtime` `BuiltinRuntimeDispatch`/`builtin_route_frames`; "Keep in sync with `faber-runtime/src/frame.rs`" | **HOSTS-PROVIDERS** | the keep-in-sync contract re-targets to the Hosts provider authority; ordinary `ad` host-effect routes stay selectable, GPU submission never enters route selection (§5) |
| `faber/packages/http/rust/router.rs:6`, `shim.rs:9` | `use faber::Valor` (http package generated Rust) | **GENRUST** | generated http package re-points to the support crate |
| `faber/crates/http-transport/Cargo.toml:14` | `faber = { package = "faber-runtime", path = "../../../faber-runtime" }` | **GENRUST** | path dep re-points to the support crate |
| `src/package_test.rs:5071,5206,5541` | generated-code tests asserting/refuting `use faber::Valor as valor;` | **GENRUST** | test expectations re-point to the support crate name |
| `src/package/binding_probe_test.rs:275`, `runtime_dependency_test.rs:39`, `library_link_test.rs:126`, `cargo_test.rs:50,76` | generated-manifest fixtures path-linking `faber-runtime` | **GENRUST** | fixtures regenerate to the support crate path |
| `src/package/llvm_host.rs:147–172` | builds `libfaber_host_llvm.a` from `../faber-runtime` (`ensure_llvm_runtime_archive`) | **HOSTS-LLVM** | the archive build re-sources from Hosts-owned LLVM support; ABI version + content receipt per DDPP0-U8 |
| `src/package/llvm_host.rs:190–199` | **last-good-archive reuse fallback** on rebuild failure | **HOSTS-LLVM** | removed at DDPP3/DDPP8 — stale last-good reuse forbidden; rebuild failure or identity mismatch fails closed (DDPP0-U8) |
| `src/package/llvm_host.rs:655–661` | `runtime_artifact_metadata` reads `faber-runtime/hosts/llvm/Cargo.toml` | **HOSTS-LLVM** | metadata source re-points to the Hosts-owned LLVM support crate |

---

## 4. Hosts path dependencies + `hosts/AGENTS.md`

| Path dep / doc (live source) | What it consumes | Destination (exactly one) | Deletion receipt |
| --- | --- | --- | --- |
| `hosts/macos-arm64/Cargo.toml:10` | `faber = { package = "faber-runtime", path = "../../faber-runtime" }` | **GENRUST-CONTRACT** | the macos-arm64 host installs a `HostDispatch` implementation over the narrow support contract; the faber-runtime path dep is replaced by the support-contract dep |
| `hosts/macos-arm64/Cargo.toml:11–16` | `host-kernel`/`aleator`/`consolum`/`processus`/`solum`/`tempus` path deps (already Hosts-owned) | unchanged (Hosts crates) | no change — already the correct owner |
| `hosts/macos-arm64/Cargo.toml:24,28–29` | `libloading = "0.8"` + cfg-gated `metal = "0.33"` | unchanged (Hosts) | no change — driver bindings stay in the Hosts leaf |
| `hosts/wasm/Cargo.toml:14` | `radix-host-abi` only (no faber-runtime dep) | unchanged (RADIX-HOST-ABI) | no faber-runtime edge to delete; verified by the absence of the dep |
| `hosts/crates/host-kernel/Cargo.toml:13` | `faber = { package = "faber-runtime", path = "../../../faber-runtime" }` | **GENRUST-CONTRACT** | provider/kernel crates consume the frame carriers + `HostDispatch` contract; path dep replaced |
| `hosts/crates/aleator/Cargo.toml:11` | same faber-runtime path dep | **GENRUST-CONTRACT** | same replacement |
| `hosts/crates/consolum/Cargo.toml:10` | same faber-runtime path dep | **GENRUST-CONTRACT** | same replacement |
| `hosts/crates/processus/Cargo.toml:11` | same faber-runtime path dep | **GENRUST-CONTRACT** | same replacement |
| `hosts/crates/solum/Cargo.toml:11` | same faber-runtime path dep | **GENRUST-CONTRACT** | same replacement |
| `hosts/crates/tempus/Cargo.toml:11` | same faber-runtime path dep | **GENRUST-CONTRACT** | same replacement |
| `hosts/crates/http/Cargo.toml:12` | same faber-runtime path dep | **GENRUST-CONTRACT** | same replacement |
| `hosts/crates/provider-contracts/Cargo.toml:14` | same faber-runtime path dep | **GENRUST-CONTRACT** | same replacement |
| `hosts/Cargo.lock` | `faber-runtime` lockfile entries (lines 24, 124, 460, 484, 716, 725, 735, 746, 1139, 1419, 1492) | gate surface item | `Cargo.lock` re-generated at DDPP8 with no `faber-runtime` dependents (§DeletionRule rule 5) |
| `hosts/AGENTS.md:15` | "Path deps expect sibling `faberlang/{faber-runtime,radix}`" orientation row | doc authority update | updated to the Hosts-owned support sources + `radix-host-abi`; `faber-runtime` removed from the orientation |
| `hosts/README.md:29` | "`faber-runtime/` — Runtime types (`use faber::…`)" | doc authority update | table row re-pointed to the generated-Rust support crate |
| `hosts/webgpu-browser/` | no faber-runtime reference (browser JS host) | non-consumer | no edge to delete |

---

## 5. Core-support / release manifest schemas + examples

| Surface (live source) | Role | Destination (exactly one) | Deletion receipt |
| --- | --- | --- | --- |
| `faber/core-support-manifest.txt` (root `faber-runtime`, line 6) | core-support logical roots embedded in the RC payload | reassembly at DDPP8 | the `faber-runtime` root is replaced by the new support destinations' source roots; the other 9 roots (radix-runtime-contract + hosts/crates/*) stay |
| `faber/build.rs:33–41` | assembles `core-support.tar.zst` + `.sha256` + `files.sha256` | reassembly at DDPP8 | `faber-runtime` root removed from `read_roots`; archive reassembled and re-hashed |
| `faber/src/core_support/materialize.rs:37` | `MaterializedCoreSupport::faber_runtime()` verified extraction root | **GENRUST** | `required_directory("faber-runtime")` re-points to the support crate root |
| `faber/src/core_support/assembler.rs` | `read_roots`/`assemble` over the manifest | reassembly at DDPP8 | manifest-driven; no code change beyond the manifest |
| `faber/release-manifest.yaml` (`pinnedInputs.source` faber-runtime @ `10d48ea47435`) | release pin schema §4/§7 (single release-manifest schema) | release pin update | the faber-runtime source pin is removed/replaced when the repo is deleted; prepare step regenerates the manifest |
| `faber/docs/release/release-manifest-schema.md` + `release-manifest.schema.json` | the single release-manifest schema (companion revisions: radix, cista, faber-runtime, hosts) | schema update at DDPP8 | faber-runtime component row removed/re-routed; schema §4 pin set updated |
| `faber/docs/release/v1.6.0-rc.1-sibling-pins.md` | sibling-pins record (faber-runtime `10d48ea`) | release-notes gate item | release notes and version pins citing faber-runtime updated (§DeletionRule rule 5) |
| `faber/docs/release/worktree-rehearsal-procedure.md` (faber-runtime worktree pin, lines 35, 67–68, 109) | CI/rehearsal sibling checkout | CI sibling-checkout gate item | sibling checkout no longer detaches faber-runtime; pins re-point to the new support sources |
| Generated Cargo manifests (`src/package/cargo.rs` §3 rows) | `support.faber_runtime()` + path-links | **GENRUST** | regenerated to the support crate's target-specific name |

---

## 6. Triga engine / hello-voxel / graphics-MIR routes (grep-discovered)

| Route (live source) | Relationship to faber-runtime | Destination (exactly one) | Deletion receipt |
| --- | --- | --- | --- |
| Triga engine — `triga/docs/factory/triga-engine/GOAL.md` §`faber-runtime` ("receives only the generated-code representations the application lane actually needs") | generated-code runtime for the Triga engine application lane | **GENRUST** | Triga engine generated code re-points to the support crate; it never becomes the WebGPU engine or a duplicate of Triga semantic contracts |
| `triga/docs/factory/triga-threejs-80/CAMPAIGN.md:120` + `PROOF-HARNESS.md:55` ("Generated application runtime \| faber-runtime") | proof-harness runtime table | **GENRUST** | harness table re-points to the support crate |
| `triga/docs/factory/triga-threejs-80/goals/01,02,03,06,08,09` ("faber-runtime when generated storage needs it") | generated storage/runtime dependencies | **GENRUST** | each goal's faber-runtime reference re-points at activation |
| `triga/docs/factory/triga-threejs-80/01-math-transform-delivery.md:44,95` | builds emitted Rust with the local faber-runtime package | **GENRUST** | delivery re-points to the support crate |
| `triga/docs/factory/triga-threejs-90/CAMPAIGN.md` ("host/runtime ownership to be reconciled at activation") | deferred ownership reconciliation | **GENRUST** | reconcile to the support crate at activation; no new faber-runtime surface |
| `examples/hello-voxel` (`faber.toml`: target `ts`, deps `web`+`triga`) | browser TS product — **no faber-runtime dep** | non-consumer (no migration) | verified absent; the graphics-MIR/WebGPU route stays Triga/Radix source facts + Hosts WebGPU host (campaign §Scope Routing) |
| `triga/docs/factory/triga-threejs-80/goals/04-graphics-mir-shader-stages.md` | graphics source-to-MIR contracts | Triga/Radix (no faber-runtime) | graphics source facts stay Triga/Radix-owned; no WebGPU handles in public types |
| `examples/arena-handle/src/main.fab:57` | "Semantics mirror faber-runtime::Arena / ArenaHandle" | **GENRUST** | mirror semantics re-point to the support crate's arena carrier |

---

## 7. TR7 training RC — consumer row (council C3)

| Consumer | Relationship to faber-runtime | Destination (exactly one) | Deletion receipt |
| --- | --- | --- | --- |
| TR7 training RC (`radix/docs/factory/gpu-training-lowering/stage-7-delivery.md:67,136`) | the training RC rides faber-runtime via core-support (the `release-manifest.yaml` core-support pack); TR7-U1 pins companion revisions incl. faber-runtime; every E6/E7 receipt MUST contain the faber-runtime component SHA | **GENRUST** (support surface) + core-support/release pins | TR7's **immutable pinned receipts stay historical** (C3 — never rewritten); the DDPP8 migration routes the TR7 companion-revision pin (sibling-pins record) to the new support sources; TR7's faber-runtime *runtime* consumption is the generated-Rust support surface |

---

## 8. PML2 GI1 admission migration + PML0 capsule carriage (council C4)

| Reference | Requirement | Reconciliation (exactly one destination) | Deletion receipt |
| --- | --- | --- | --- |
| `gradus/docs/factory/production-ml-library/pml2-delivery.md` PML2-U5 | GI1's accepted `faber-runtime` admission rows (`gguf.rs`, `tokenizer/`, `dequant.rs`) are moved into gradus or formally retired per PML0-U7; no dual authority, enforced by code location | **GRADUS-PML** | **named as a DDPP8 prerequisite**: PML2 executes the GI1 admission migration before DDPP8 deletes faber-runtime; the old owning location must host no admission entry points (grep proof) |
| `gradus/docs/factory/production-ml-library/pml0-model-capsule-contract.md` ("only the capsule carries identity across Gradus ↔ faber-runtime/hosts") | the typed model capsule is the identity carrier across the Gradus/faber-runtime/hosts boundary | **GRADUS-PML** + decomposed boundary | **PML0 capsule carriage reconciled**: the capsule's "faber-runtime" boundary partner becomes the Gradus PML-owned model surface + GENRUST/GENRUST-CONTRACT + Hosts leaves — the capsule identity rule survives with **no universal runtime owner** (C5 wording; §DeletionRule rule 4) |
| `gradus/docs/factory/production-ml-library/pml0-delivery.md:179,237–243` (PML0-U7/U11) | GI1 admission code (`gguf.rs`, `tokenizer/`, `dequant.rs`) migration decision; GI4+ ownership amendment — no GI4+ doc still assigns model runtime/tokenizer/serving to `faber-runtime` | **GRADUS-PML** | the GI-dir grep assertion (amendment + reconciliation citations only) is a pre-DDPP8 dependency; aligns with this inventory's GRADUS-PML rows |

---

## 9. Existing `faber-runtime/docs/factory/autograd-substrate-inventory.md` reconciled

The substrate inventory documents the dense tensor/autograd substrate. Under the campaign
Decomposition Target it is re-labeled by destination (no production link):

| Substrate area (autograd-substrate-inventory rows) | Old claim | Reconciled destination (exactly one) | Deletion receipt |
| --- | --- | --- | --- |
| Dense carrier / shape / arithmetic / matmul / reductions / views (`src/tensor.rs`, `src/sparsa.rs`) | "Runtime tensor type" for generated code | **GENRUST** | CPU `tensor`/`sparsa` semantics needed by generated Rust move to the support crate (row 1) |
| Autograd scaffold + finite-difference oracle + `TestOnlySgdSession` (`src/autograd.rs`, `autograd_reference_test.rs`) | internal dense tape | **REPO-ORACLE** | the tape and its oracles are repo-owned test/oracle fixtures; under G-A-01 the tape view policy survives as an oracle/debug path only; never linked into production support |
| Packed / dequant quantized carriers (`src/packed_numeric.rs`, `src/dequant.rs`) | bridge materialization surfaces | **GRADUS-PML** | quantized model interpretation family (row 8) |
| Host ABI tensor symbols (`src/host_abi.rs`, `hosts/llvm/src/tensor.rs`) | ABI symbol surface | **RADIX-HOST-ABI** / **HOSTS-LLVM** | one versioned ABI; the LLVM host side lands with Hosts-owned LLVM process support |
| The inventory's "Current Proof Boundary" (contiguous materialized `Tensor<f32>` only) | proof constraint | **REPO-ORACLE** | the proof boundary is a test/oracle discipline, not a production support contract |

---

## 10. Ordinary `ad` preserved; GPU submission statically excluded

| Surface | Rule | Destination (exactly one) | Deletion receipt |
| --- | --- | --- | --- |
| Ordinary `ad` host-effect seam | `Faber ad` → Radix Sermo MIR → generated-language/LLVM effect ABI → Hosts dispatch/kernel → concrete provider (campaign §Decomposition Target) | **HOSTS-PROVIDERS** (dispatch/kernel) + **RADIX-HOST-ABI** (effect ABI) | preserved end-to-end; `is_builtin_ad_route` (`dispatch.rs:43–47`) stays as ordinary host-effect route selection; generated-Rust and LLVM fixtures keep ordinary `ad` behavior |
| GPU submission | statically unreachable from Sermo, `Valor`, `HostDispatch`, and route selection — it is a compiled artifact path, not an effect-provider path (campaign §Decomposition Target; ddpp0-contract §ProductShape effect/capability requirements) | **RADIX-ARTIFACT+FABER-BUILD** (selection) + **HOSTS-COORD** (physical leaves) + **HOSTS-PROVIDERS** (never) | the device route (`prefill_run.rs`, `device/mod.rs`) never flows through `ad` dispatch or `builtin_route_frames`; `DeviceHandle`→`Valor` lowering (`device.rs:174–183`) is a control-plane descriptor, never an execution carrier; the generated-Rust support crate has no device session behavior (C2) |
| `Valor`/frame carriers | value carriers carry data, never GPU launch state | **GENRUST** / **GENRUST-CONTRACT** | carriers re-point to the support crate/contract with no device fields |

---

## 11. Deletion gate surface (summary)

The rows above close per-module and per-import. The final gate is DDPP8: every production
manifest, source import, build script, support archive, CI checkout, host crate, and doc
authority listed in §§2–10 must stop depending on `faber-runtime` (ddpp0-contract §DeletionRule
rule 5 gate table: `Cargo.toml`, `Cargo.lock`, `core-support-manifest.txt`, `build.rs`, generated
Cargo manifests, CI sibling checkouts, release notes, stale-archive fallback), the external-
consumer audit passes (**TR7 included**; **PML2 GI1 admission migration** is a DDPP8
prerequisite; **PML0 capsule carriage** reconciled), accepted native/browser capstones land, and
no forwarding crate / route alias / dual authority preserves the old architecture. The remaining
generated-Rust support is explicitly **Rust-target support, not a universal runtime** — there is
**no universal runtime owner** (C5 wording).

---

*Planning/contract artifact only. No product code was written; `faber-runtime/` untouched. All
citations are grep-verified at write time against faber HEAD `1dc4513`, faber-runtime
`10d48ea47435`, hosts `e066ee0ae98a`, triga `e6394b30f3ba`, gradus HEAD as recorded in PML0,
and the faber release-manifest at `release-manifest.yaml`.*
