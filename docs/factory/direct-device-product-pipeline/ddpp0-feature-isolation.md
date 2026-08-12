# DDPP0 Feature Isolation — exact product features + dependency/module gates + DDPP1 gate proof plan

**Unit**: DDPP0-U7 (council C2) — feature isolation is the developer-story contract.
**Date**: 2026-08-08.
**Repo**: faber (control plane). `faber-runtime/` read-only; no product code; no cargo.
**Council disposition folded**: **C2** (cpo) — "Feature isolation is the developer-story
contract; DDPP1 gate proof plan specified (`cargo check -p faber --no-default-features
--features hir-rust` excludes GPU emitters/physical Hosts leaves/device runtime; `faber
targets` matches truth); generated-Rust support crate must not transitively pull
device/Hosts" (council dispositions table in `ddpp0-delivery.md`).
**Sections frozen here**: `## Current state`, `## Small-build product feature set`,
`## Dependency-gate table`, `## Module-gate list`, `## DDPP1 gate proof PLAN`.
**Cross-references**: `evidence/ddpp0-snapshot.md` §3 (GT-1/GT-2, confirmed at snapshot
time) and §2 (module/import role-match greps); `ddpp0-contract.md`
§GeneratedRustSupport (DDPP0-U4 — generated-Rust support destination default: Faber-owned
support crate, target-specific name, campaign OQ1); campaign §Development Posture
("Feature gates must become real") and the DDPP1 stage gate.

**Authority order** (campaign §Repo-Aware Baseline): live source/tests and live `faber
targets` → accepted artifact schemas + hardware receipts → **this phase's frozen
contracts** → campaign prose. The current-state rows below are grounded in the live
`radix/crates/faber/Cargo.toml` and `src/package/mod.rs` as confirmed by DDPP0-U1 (GT-1/GT-2; 18/18
confirmed, 0 drifted, 0 superseded). **The DDPP1 gate proof is a PLAN specified here,
not a run.** No cargo command in this document is executed by DDPP0; the proof executes
at the DDPP1 gate.

---

## Current state

Frozen as of the DDPP0 baseline (snapshot §3 GT-1/GT-2). Today the GPU/runtime/Hosts
dependencies and the device-runtime modules are **unconditional**: the dependencies are
declared in plain `[dependencies]` (no `optional = true`) and the modules are declared in
`src/package/mod.rs` / `src/package/device/mod.rs` without a feature gate. `default =
["full-targets"]` turns on all `hir-*`/`mir-*` target features today, which is why the
unconditional deps and modules are compiled into every build including a would-be small
Rust-only one.

### Current-state table — unconditional dependencies

| Dependency | Role | Declared as |
| --- | --- | --- |
| `radix-mir-metal` | GPU emitter — Metal MSL device artifact emitter (S1-6 differentiable-GPU; consumed through the shared radix-mir surface) | `radix/crates/faber/Cargo.toml` plain path dep (`radix-mir-metal = { path = "../radix/crates/radix-mir-metal" }`) — not `optional` |
| `radix-mir-llvm` | GPU emitter — LLVM/NVVM device artifact emitter (CUDA PTX/NVVM side; `crates/radix-mir-llvm/src/nvvm/`) | `radix/crates/faber/Cargo.toml` plain path dep — not `optional` |
| `faber-runtime` (package `faber`) | Device-runtime dependency — the `faber::device` / `faber::dequant` / `faber::gguf` / `faber::prefill` surface (plus the generated-Rust carriers the runtime also carries today) | `radix/crates/faber/Cargo.toml` plain path dep (`faber = { package = "faber-runtime", path = "../faber-runtime" }`) — not `optional` |
| `faber-host-macos-arm64` | Physical Hosts leaf — native Metal/CUDA host (`hosts/macos-arm64`) | `radix/crates/faber/Cargo.toml` plain path dep — not `optional` |
| `faber-host-wasm` | Physical Hosts leaf — wasm product host (`hosts/wasm`) | `radix/crates/faber/Cargo.toml` plain path dep — not `optional` |

> Note: the delivery baseline (snapshot GT-2) also lists `radix-mir-fmir` as an
> unconditional faber dependency. It is the FMIR device-schema/text crate, **not** a GPU
> emitter, physical Hosts leaf, or device-runtime dependency, so it is outside this
> unit's exclusion target set; whether a fully minimal build needs to gate it is a DDPP1
> implementation decision, not a DDPP0 freeze. The current-state table above follows the
> DDPP0-U7 done_when enumeration (five unconditional deps).

### Current-state table — unconditional modules

| Module | Role | Declared as |
| --- | --- | --- |
| `package/device/*` | Device-runtime module subtree — device section assembly, session-route execution, prefill run, training, wire (all of `src/package/device/`: `mod.rs`, `program`, `prefill_run`, `run`, `section`, `training`, `wire`) | `src/package/mod.rs` `pub mod device;` + `src/package/device/mod.rs` submodules — no feature gate |
| `package/host_factory.rs` | Host factory that consumes `faber::device::DeviceSelection`/`DeviceBackend` (`src/package/host_factory.rs:30`) | `src/package/mod.rs` `mod host_factory;` — no feature gate |
| `package/device/prefill_run.rs` | The `faber::dequant` / `faber::gguf` / `faber::prefill` runtime-import site (`src/package/device/prefill_run.rs:62-65`) | `src/package/device/mod.rs` `mod prefill_run;` — no feature gate |

Additional unconditional callers that reference the device surface (snapshot §2
role-match greps): `src/cli/mod.rs:383-387`, `src/commands/run.rs:14`,
`src/package/mir/mod.rs:32` (+ `mir/routes.rs`), `src/package/manifest.rs:373-377` —
these reference `faber::device` types and must compile the device path out once the
device-runtime feature is off (module-gate list below).

---

## Small-build product feature set

**Frozen**: the exact product feature set for a small Rust-only build is

```text
default-features off + hir-rust-only target features
cargo check -p faber --no-default-features --features hir-rust
```

Concretely, at DDPP1 the small build must activate **only** the faber `hir-rust` feature
(`hir-rust = ["radix/hir-rust", "dep:faber-hir-rust"]` — the HIR-Rust host lane plus the
`faber-hir-rust` support crate), and **nothing else**:

| Axis | Small build (`--no-default-features --features hir-rust`) |
| --- | --- |
| faber features enabled | `hir-rust` only |
| faber features disabled | all other `hir-*` (`hir-faber`, `hir-ts`, `hir-go`, `hir-swift`, `hir-fhir`) and all `mir-*` (`mir-fmir`, `mir-sexp`, `mir-metal`, `mir-wgsl`, `mir-llvm`, `mir-wasm`, `mir-coverage`) |
| radix features | `radix/hir-rust` only (the `radix` dep is `default-features = false`) |
| gated GPU emitter deps (DDPP1) | excluded — `radix-mir-metal`, `radix-mir-llvm` |
| gated physical Hosts leaf deps (DDPP1) | excluded — `faber-host-macos-arm64`, `faber-host-wasm` |
| gated device-runtime dep (DDPP1) | excluded — `faber` (`faber-runtime`) |
| device-runtime modules (DDPP1) | excluded — `package/device/*`, `package/host_factory.rs`, `package/device/prefill_run.rs` |

The default product build stays `default = ["full-targets"]` (unchanged product surface);
the small build is an explicit opt-in. DDPP1 adds the new gating features below to
`full-targets` so default behavior is preserved while the small build becomes real. The
campaign DDPP1 gate is the done oracle for this feature set: the exact command excludes
GPU emitters, physical Hosts leaves, and device runtime modules, and `faber targets`
reports matching capability truth.

---

## Dependency-gate table

**Frozen**: each GPU emitter, physical Hosts leaf, and device-runtime dependency becomes
an **optional dependency** wired behind an explicit feature. Exact Cargo wiring is DDPP1
implementation; the feature names and gate rules below are the contract.

| Dependency | Class | Optional feature | Gate rule |
| --- | --- | --- | --- |
| `radix-mir-metal` | GPU emitter (Metal MSL) | `mir-metal` (exists today; must become `optional = true` + pulled only by `mir-metal`) | No build without `mir-metal` compiles the Metal MSL emitter; a `mir-metal`-less build carries no Metal device capability row |
| `radix-mir-llvm` | GPU emitter (LLVM/NVVM, CUDA PTX/NVVM side) | `mir-llvm` (exists today; must become `optional = true` + pulled only by `mir-llvm`) | No build without `mir-llvm` compiles the NVVM/LLVM device emitter; a `mir-llvm`-less build carries no CUDA device capability row |
| `faber` (package `faber-runtime`) | device-runtime dependency (`faber::device`/`dequant`/`gguf`/`prefill` surface) | `device-runtime` (proposed new feature) | The runtime package is pulled only when device-runtime modules are compiled in; the generated-Rust support carriers it also carries today migrate to the Faber-owned support crate (§GeneratedRustSupport, U4), so the residual gated dependency is device-runtime-only |
| `faber-host-macos-arm64` | physical Hosts leaf (native Metal/CUDA host) | `host-macos-arm64` (proposed new feature) | The native host leaf is compiled only when the native-host feature is enabled; a small build compiles no native driver/session code |
| `faber-host-wasm` | physical Hosts leaf (wasm product host) | `host-wasm` (proposed new feature) | The wasm product host is compiled only when the host-wasm feature is enabled; a small build compiles no wasm host |

Gate-rule invariants that apply to every row:

1. **Optional deps only.** No GPU emitter / physical Hosts leaf / device-runtime crate
   remains in plain `[dependencies]` after DDPP1. Each row is `optional = true` and is
   enabled exclusively by its feature (directly, or through `full-targets`).
2. **Exclusion is compile-level, not runtime-level.** An excluded crate is not merely
   unreachable at runtime; it is absent from the build graph. Proof: the DDPP1 gate proof
   PLAN below.
3. **Capability truth.** `faber targets` reports only the capabilities the build actually
   compiled (C1 — host×device matrix is capability truth, not a shipping promise;
   §SelectionPolicy, U3). A small Rust-only build reports the `rust` host lane and no
   Metal/CUDA/host-leaf device rows.
4. **Default unchanged.** `default = ["full-targets"]` keeps today's product surface;
   `full-targets` includes the new host/device-runtime features at DDPP1.

Unconditional faber deps **outside** this unit's exclusion set (not GPU emitters,
physical Hosts leaves, or device-runtime): `radix` (facade, `default-features = false`),
`radix-mir`, `radix-mir-fmir` (see current-state note), `radix-types`, `cista`. DDPP1
decides whether any of these needs gating for a *fully* minimal build; this contract
freezes the C2 exclusion set only.

---

## Module-gate list

**Frozen**: the device-runtime modules are excluded from the build without their feature
(`device-runtime`). The module gates below are the compile-level counterpart of the
dependency-gate table: a module in this list must not be compiled (and its body must not
reference the excluded `faber::*` runtime surface) when the feature is off.

| Module | Feature required | Gate rule |
| --- | --- | --- |
| `package/device/` (whole subtree: `mod.rs`, `program`, `prefill_run`, `run`, `section`, `training`, `wire`) | `device-runtime` | Compile the device-runtime subtree only when `device-runtime` is on; excluded without it |
| `package/host_factory.rs` | `device-runtime` | Compile the host factory only when `device-runtime` is on (it consumes `faber::device::DeviceSelection`/`DeviceBackend`); excluded without it |
| `package/device/prefill_run.rs` | `device-runtime` | Compile the prefill run only when `device-runtime` is on (it imports `faber::dequant`/`gguf`/`prefill`); excluded without it — listed explicitly because it is the named runtime-import site |

Caller modules whose `faber::device` references must compile out without the feature
(device-selection plumbing becomes a compile-out or feature-gated path at DDPP1; the
exact mechanism is DDPP1 implementation, the exclusion is the contract):
`src/cli/mod.rs`, `src/commands/run.rs`, `src/package/mir/mod.rs` + `mir/routes.rs`,
`src/package/manifest.rs`. The DDPP1 gate proof must confirm none of these pulls the
device-runtime surface into the small build.

---

## DDPP1 gate proof PLAN

**Frozen**: this section is a **PLAN**. The commands are specified exactly and run **at
DDPP1**, not here. DDPP0 executes no cargo. The plan matches the campaign DDPP1 gate
verbatim and is the done oracle for C2.

### Proof 1 — exact command + expected exclusion list

Command (exact):

```bash
cargo check -p faber --no-default-features --features hir-rust
```

Expected result:

- Exit 0 (the small Rust-only build compiles).
- The build graph contains **no** GPU emitters, **no** physical Hosts leaves, and **no**
  device runtime. Expected exclusion list (verified, e.g. via `cargo tree -p faber
  --no-default-features --features hir-rust` not listing the crates, or an equivalent
  scripted grep over the build graph):
  - GPU emitters: `radix-mir-metal`, `radix-mir-llvm`;
  - physical Hosts leaves: `faber-host-macos-arm64`, `faber-host-wasm`;
  - device runtime: `faber` (package `faber-runtime`);
  - device-runtime modules absent from compilation: `package/device/*`,
    `package/host_factory.rs`, `package/device/prefill_run.rs`.

### Proof 2 — `faber targets` capability truth

Command (run against the small build at DDPP1):

```bash
faber targets
```

Expected result: the reported capability surface **matches the enabled feature set** — the
`rust` host lane (from `hir-rust`) is present; no Metal/CUDA device rows, no
native/wasm host-leaf rows, and no device-runtime rows are reported for capabilities the
build did not compile. `faber targets` reports capability truth, not a shipping promise
(C1 / §SelectionPolicy).

### Proof 3 — generated-Rust support crate non-pull rule (DDPP1 gate)

**Restated from C2**: the Faber-owned generated-Rust support crate (target-specific name,
campaign OQ1 default; §GeneratedRustSupport, U4) is **Rust-target support only** and
**must not transitively pull device or Hosts**. It must not, directly or transitively,
depend on `faber-runtime` (`faber`), `faber-host-macos-arm64`, `faber-host-wasm`,
`radix-mir-metal`, `radix-mir-llvm`, or any physical Hosts leaf.

Gate command (run at DDPP1):

```bash
cargo tree -p faber-hir-rust            # or the DDPP1-fixed Faber-owned support crate
```

Expected result: the support crate's dependency graph (transitive closure) contains no
device-runtime crate and no Hosts leaf crate. Any transitive pull of device/Hosts by the
support crate fails the gate. This rule is the C2 restatement of
§GeneratedRustSupport's "no device session behavior" as a build-graph property.

---

## Residuals / cross-references

- **C2 landed here** (this file) and in `ddpp0-contract.md` §GeneratedRustSupport (U4).
- **OQ1** (generated-Rust support crate name/home) default recorded in
  §GeneratedRustSupport (U4); the DDPP1 gate proof Proof 3 names the crate and fixes the
  final name.
- **`radix-mir-fmir` gating** (whether a fully minimal build also gates the FMIR
  device-schema crate) is a DDPP1 implementation decision — recorded here, not frozen.
- **DDPP1 gate authority**: campaign DDPP1 stage gate — "`cargo check -p faber
  --no-default-features --features hir-rust` excludes GPU emitters, physical Hosts
  leaves, and device runtime modules, and `faber targets` reports matching capability
  truth." The proofs above are that gate's done oracle.

---

*Contract artifact — DDPP0-U7 (C2). Planning only; no product code; `faber-runtime/`
untouched; the DDPP1 gate proof is a PLAN, not a run.*
