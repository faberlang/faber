# Delivery: DDPP1 — Generic product build plan and artifact materialization

**Goal ref**: `faber/docs/factory/direct-device-product-pipeline/CAMPAIGN.md` — DDPP1 stage (status `planned — after DDPP0 and DDCP2 contract`, posture, sources, gate; §Product Build Pipeline; §Artifact identity and payload encoding; §LLVM native materialization; §Batching And Split Policy) + the DDPP0 phase closeout Notes ("DDPP1 post-DDCP2 route identified", campaign line ~584–587) + the DDPP0 gate record ("DDPP1's post-DDCP2 delivery route, owner, fixture, and done oracle are identified").
**Status**: lowered 2026-08-09 — READY for admission by Mind; goal-check READY before Hands file units.
**Entry gate**: **MET** — DDCP2 contract landed (radix `d17535cc4`, `ddcp2-closeout.md`; the same-compatibility-boundary contract handed the faber B4–B7 consumers to this route; DDCP1 delivered `8c9cff241` with the fire-8 materializer residual + CTO-5 input recorded). This lowering follows the campaign's recorded post-DDCP2 route.
**Repo**: faber (implementation: package/build + feature-isolation surfaces). `faber-runtime/` and `hosts/` are **read-only** here; no radix write (radix-lane defects escalate, see §Escalation Path).
**Planning-only**: no product code is written by this lowering; this document is the delivery spec implementing Hands run from.

## Phase Intent

DDPP1 turns the accepted raw-byte `DeviceArtifact` packet (DDCP2) into a **generic product build plan and artifact materialization** path: one analyzed package produces an inspectable build-plan result (host artifact + optional `DeviceProgram` + versioned `DeviceArtifact[]` + call/submission-region facts) with feature availability and host/device pair support validated **before toolchain work**, the Metal/CUDA text assumptions replaced by the accepted packet, and no device session started by planning or inspection. It also executes the **C2 feature-isolation gate proof** (specified as a plan by DDPP0-U7; run here, not re-planned) and closes the **CTO fire-8 materializer seam** (faber `build_semantics` populates and enforces `DeviceLegalityFacts`). Planning/materialization is T1–T2 evidence only; physical execution proofs remain auditor/operator gates (DDPP2+).

## Interpreted Scope

Deliver, for the campaign DDPP1 gate:

1. **Raw-byte artifact packet materialization**: the faber device section/materialization path consumes the DDCP2 `DeviceArtifact` packet shape — backend id, format id, materialization stage (`compiler-input` | `finalized-binary`), target id + required features, ABI/schema version, canonical raw bytes, entrypoint map, reflection/requirements, `content_sha256` + `packet_sha256` (with `compiler_input_packet_sha256` parent provenance on finalized rows). Metal/CUDA text assumptions are replaced by the accepted packet; **no emitted-target text is reparsed** to recover facts.
2. **Canonical text/binary round-trip**: text and binary payloads round-trip with hashes over **canonical decoded bytes** (`§RoundTripFixture` spellings; hashes never over the transport spelling); hashes survive faber materialization unchanged.
3. **Build-plan result + pair validation before toolchain**: one inspectable `CompiledPackage`/build-plan result for host + device artifacts; **absent target features and unsupported host/device pairs fail before build**; existing CPU-only Rust, FHIR/FMIR, and LLVM products remain **deliberately routed or explicitly rejected**; build planning imports **no Hosts driver implementation**.
4. **Feature isolation (C2) executed**: `cargo check -p faber --no-default-features --features hir-rust` excludes GPU emitters (`radix-mir-metal`, `radix-mir-llvm`), physical Hosts leaves (`faber-host-macos-arm64`, `faber-host-wasm`), and device runtime (`faber`/`faber-runtime`, `package/device/*`, `host_factory.rs`, `prefill_run.rs`); `faber targets` reports matching capability truth; the generated-Rust support crate (`faber-hir-rust`) transitively pulls **no** device or Hosts.
5. **Materializer seam + enforcement gate (CTO fire-8)**: faber `build_semantics` populates `DeviceLegalityFacts` from MIR-body inspection and the device/AIR legality gate is enforced in faber materialization (device `ad`/Sermo/unresolved host effects reject with the named structured diagnostic; CPU host `ad` remains valid) — mirroring the DDCP1-U6 radix-side gate. Owner: **hand-3**.
6. **Namespaces and manifests extended without a device session**: manifests and inspection output carry the packet identity rows and the frozen artifact namespace; no session is started by build planning or inspection.

**Forbidden scope**: any radix write (schema/emit defects escalate, never silently patched); any `faber-runtime/` or `hosts/` edit; any FNV artifact-provenance path kept as a compatibility fallback (B4–B7 clean break); any emitted-text parsing to reconstruct artifact facts; any device session / driver code in build planning; any `cargo build`, whole-workspace nextest, full-profile suites during development (Cargo discipline); foreign dirt (see Repo-Aware Baseline) is never touched.

## Normalized Spec

Locked decisions (folded routing inputs; defaults recorded, not left to Hands):

1. **U1 lands as a repair slice (folded NOTE).** The B4–B7 faber consumer migration / compile-restore is being **executed concurrently by hand-3** (Vivi task `60b340c1`; faber was left broken by the radix-side DDCP2 migration). This delivery **treats U1 as landing now**: it cites the **frozen `ddpp0-contract.md` §FnvRemoval contract + B4–B7 rows** (never pre-repair line numbers), U1's done-oracle is the **compile-restore + migration-matrix spellings**, and the implementing hand re-verifies against the landed state ("summaries are claims"). Mind does **not** re-file U1 as new work.
2. **Feature-isolation gate proof (C2) is RUN here, not planned again.** The exact commands and expected exclusions from `ddpp0-feature-isolation.md` are the C2 gate contract; the proof executes at this phase's gate (Proof 1 = small-build `cargo check`; Proof 2 = `faber targets` capability truth; Proof 3 = support-crate transitive non-pull). No DDPP1 unit re-specifies the proof.
3. **Feature-gate wiring (C2 contract → faber).** Each GPU emitter / physical Hosts leaf / device-runtime dependency becomes `optional = true`, enabled **exclusively** by its feature (directly or through `full-targets`): `radix-mir-metal` → `mir-metal`, `radix-mir-llvm` → `mir-llvm`, `faber-runtime` (package `faber`) → new **`device-runtime`**, `faber-host-macos-arm64` → new **`host-macos-arm64`**, `faber-host-wasm` → new **`host-wasm`**. Device-runtime modules (`package/device/*`, `package/host_factory.rs`, `package/device/prefill_run.rs`) compile only under `device-runtime`; the named caller surfaces compile the device-selection plumbing out without it. `default = ["full-targets"]` is unchanged and `full-targets` includes the new host/device-runtime features. Exclusion is **compile-level**, never a runtime unreachable-path claim. Capability truth: `faber targets` reports only capabilities the build actually compiled (C1).
4. **`radix-mir-fmir` gating** (ddpp0-feature-isolation residual, not frozen): whether a *fully* minimal build also gates the FMIR device-schema crate is a **DDPP1 implementation decision** (U2). Default: keep `radix-mir-fmir` unconditional unless the C2 exclusion proof is unreachable without gating it; if gated, it rides `mir-fmir`. The C2 exclusion set itself is unchanged.
5. **Artifact-namespace freeze (CTO-5) for this route.** Before any further fixture freezes: no new generated-Rust fixture or materialized manifest binds an **unqualified `faber::` name** to a symbol the decomposition will rename; generated-Rust fixtures use the qualified target-specific support-crate spelling; the faber build-plan target naming aligns with the radix emit contract (`target-emit-contract/goal.md` — a target name identifies a backend or artifact family, not a lifecycle stage; `faber build` owns materialization/linking, `radix emit` stops at the immediate artifact). U4 enforces this as a validation check; the decision is frozen here.
6. **`faber::` alias ambiguity (CTO-6) is a next-wave planning input, not DDPP1 work.** The `faber` package (`faber-runtime`) and the generated-Rust `faber::` reference in generated code coexist with the faber CLI crate; once the support crate moves to a Faber-owned target-specific name, unqualified `faber::` in generated fixtures becomes ambiguous. Folded as a **routed residual** to DDCP3/DDPP3 planning (see §Residuals); DDPP1 honors the CTO-5 qualified-name rule in its own fixtures.
7. **Evidence tiers.** Every DDPP1 claim is labeled (C5): build-plan/packet identity = T1–T2 (compiler emission + materialization receipts); the C2 gate proof and round-trip fixtures = T2. No T3/T4 physical-execution or performance claims are made by DDPP1.
8. **Same-compatibility-boundary contract (DDCP2 §5.2 / ddcp2-inventory §4.2).** The faber consumers consume the versioned, `deny_unknown_fields`-admitted, `content_sha256`-keyed surface at the same coordinated boundary; the typed `DeviceArtifact` rows, ABI/schema version inputs, `HostDeviceCall` typed bindings, and declared observation cadence are the boundary fields. Nothing is reconstructed from emitted text.
9. **NGAB2 is a DDPP3 child packet, not a DDPP1 competitor.** The scoped NGAB2 delivery (composite build + embedded artifact assembly) re-files under **DDPP3** per `§ChildRouting` (NGAB1–NGAB4 → DDPP3); DDPP1 owns the generic build-plan/packet-materialization surface. DDPP1 write scopes on shared files are the generic-packet hunks; NGAB2's composite-embedding hunks land later and consume the landed DDPP1 state.

## Repo-Aware Baseline

Verified 2026-08-09 against live faber HEAD `efa6a02de` (main) and radix HEAD `d17535cc4d` (DDCP2 closeout, the entry gate). Grounding facts — the C2/dep/module rows below are **frozen contract rows** (ddpp0-feature-isolation.md), not pre-repair line numbers:

- **faber `Cargo.toml`**: `default = ["full-targets"]`; `hir-rust = ["radix/hir-rust", "dep:faber-hir-rust"]`; the five gated deps are **plain `[dependencies]` today** — `radix-mir-metal` (Metal MSL emitter), `radix-mir-llvm` (CUDA NVVM/PTX emitter), `faber = { package = "faber-runtime" }` (device-runtime surface), `faber-host-macos-arm64` (native Metal/CUDA host leaf), `faber-host-wasm` (wasm product host). `radix-mir-fmir` is unconditional (the FMIR device-schema/text crate; outside the C2 exclusion set — decision #4).
- **Unconditional modules today**: `src/package/mod.rs` `mod device;` (whole `src/package/device/` subtree), `mod host_factory;`, `package/device/prefill_run.rs`; callers referencing the device surface that must compile out: `src/cli/mod.rs`, `src/commands/run.rs`, `src/package/mir/mod.rs` + `mir/routes.rs`, `src/package/manifest.rs`.
- **B4–B7 sites (frozen FnvRemoval rows; U1 landing now)**: `device/section.rs` (B4 Metal provenance + B5 CUDA PTX provenance), `device/run.rs` (B6 A9 receipt `fnv64:` line), `host_factory.rs` (B7 `BackendDiscoveryReceipt.artifact_hash`), plus the compile-restore of `device/wire.rs`, `device/program.rs`, `device/device_test.rs`, `device/prefill_run_test.rs`, `mir/mod.rs` (FmirDeviceSelection `Unknown(_)` arm, fail-closed; `FmirImageError::ArtifactDigestMismatch`), `mir/routes.rs`. **Source-identity FNV (S1 `mir/image.rs`, S2 `mir/bin_runner.rs`) is a separate surface and is out of scope** (§FnvRemoval "Separate surface").
- **`faber targets`** (`src/commands/targets.rs`): today a static `FABER_TARGET_ROWS` table with per-row availability from `target.is_available()` (radix feature cfg) + `target_capabilities_for_surface`. DDPP1 reworks it to report **compiled** capabilities only (decision #3).
- **Materializer seam (CTO fire-8)**: faber's `build_semantics` (`src/package/device/prefill_run.rs`) and the device-program semantic construction (`src/package/device/program.rs` `semantic_facts`) do **not** populate `DeviceLegalityFacts`; radix-mir carries the typed `DeviceLegalityFacts`/`DeviceEffectFact` + `DeviceFunctionHostileEffect` gate (DDCP1-U6, `crates/radix-mir/src/device_semantics.rs`). DDPP1 wires the faber seam to that surface and enforces it.
- **Build-plan surface**: `src/package/compile.rs` (`AnalyzedPackage`, `PackageCompileResult`), `src/package/artifact_plan.rs` (target-neutral artifact planning), `src/package/cmd.rs` (`cmd_build` routing), `src/package/manifest.rs` (build manifest), `src/package/llvm.rs`/`llvm_host.rs` (LLVM lane; **read-reference only** here — the composite build/embedding is NGAB2/DDPP3).
- **Core-support** (`core-support-manifest.txt`, `build.rs`, `src/core_support/`): unchanged by DDPP1; the support-archive identity row (§IdentityDomains row 6) and the no-last-good-archive rule stay in force (DDPP0-U8 / DDPP8 gate surface).
- **Foreign dirt (never touch, Class B / generated)**: faber working tree carries hand-3's in-flight U1 repair-slice edits (`src/package/device/*`, `src/package/mir/*`, `src/commands/run.rs`) and hand-1 FMIR e2e-hardening `src/package/mir/{driver,link,lower}.rs` + `lane_test.rs`; `docs/factory/README.md` (generated; regenerated at closeout); `docs/factory/browser-wasm-product/` (unrelated untracked goal). None of these are DDPP1 delivery write files except where a unit's write scope names them as hunks on the **landed** U1 state.
- **NGAB2 scoped delivery** (`native-gpu-application-bundle/ngab2-delivery.md`) names `llvm_host.rs`, `build.rs`, `device/section.rs`, `mir/image.rs`, `device/run.rs` as its eventual write surface — **entry-gated, re-files under DDPP3** (decision #9). DDPP1 does not pre-empt its composite-embedding hunks.

**Authority order**: live source/tests and live `faber targets` → accepted artifact schemas + hardware receipts → this phase's frozen contracts (DDPP0 + DDCP2 + NGAB0-R1) → campaign prose.

## Stage Graph (unit graph)

```text
U1 B4–B7 consumer migration / compile-restore  [landing now — hand-3 repair slice 60b340c1]
  └─ U2 C2 feature isolation + gate proof Proof 1/3        [Wave A; needs U1 landed]
       ├─ U3 `faber targets` capability truth (Proof 2)    [Wave B; needs U2]
       │    └─ U4 build-plan result + pair validation +
       │         inspection/manifest + CTO-5 namespace      [Wave B/C; needs U2 + U3]
       │              └─ U5 raw-byte packet materialization + round-trip  [Wave C; needs U4 + U1]
       └─ U6 materializer seam + DeviceLegalityFacts gate (CTO fire-8)    [Wave C; needs U1; owner hand-3]
  └─ all ── U7 phase closeout (gate)                        [Gate; needs all]
```

Dependency edges: `U1 → U2 → U3 → U4 → U5` (serial spine); `U1 → U6` (parallel with U3–U5 on disjoint files); `U7` needs all. **Waves**: Wave A = U1 (landing) + U2; Wave B = U3 + U4; Wave C = U5 + U6; Gate = U7. No unit overlaps another's write scope; the three shared files (`mir/mod.rs`, `manifest.rs`, `device/section.rs`+`wire.rs`) are **hunk-disjoint and strictly serialized** (see Lane Notes).

## Implementation Work (units)

### DDPP1-U1 — B4–B7 faber consumer migration / compile-restore [landing now]

| Field | Content |
| --- | --- |
| **outcome** | The faber device consumers compile again at the DDCP2 packet boundary with FNV backend-artifact provenance removed (B4–B7), receipts spelled `sha256:`, `FmirDeviceSelection::Unknown(_)` handled fail-closed, and the migration-matrix spellings honored. **Executed concurrently by hand-3 as a repair slice (Vivi `60b340c1`); this delivery records the done-oracle and treats U1 as landing.** |
| **write_scope** | `radix/crates/faber/src/package/device/section.rs`, `radix/crates/faber/src/package/device/run.rs`, `radix/crates/faber/src/package/host_factory.rs` (B4–B7); compile-restore hunks in `radix/crates/faber/src/package/device/wire.rs`, `radix/crates/faber/src/package/device/program.rs`, `radix/crates/faber/src/package/device/device_test.rs`, `radix/crates/faber/src/package/device/prefill_run_test.rs`, `radix/crates/faber/src/package/mir/mod.rs` (FmirDeviceSelection `Unknown(_)` arm + `FmirImageError::ArtifactDigestMismatch`), `radix/crates/faber/src/package/mir/routes.rs`. Hunks only on the landed slice; **no S1/S2 source-identity FNV sites** (`radix/crates/faber/src/package/mir/image.rs`, `radix/crates/faber/src/package/mir/bin_runner.rs`). |
| **read_scope** | `ddpp0-contract.md` §FnvRemoval (frozen B4–B7 rows); `ddcp2-closeout.md` + `ddcp2-inventory.md` §4.2 (same-compatibility-boundary contract); `radix-mir-fmir/src/schema/{device,hash,admit}.rs` (landed packet shapes); `ddcp2-closeout.md` §3 (emitter `sha256:` spellings). |
| **done_when** | (a) `cargo check -p faber` green (compile-restore) — the exact in-flight task's verification; (b) B4–B7 migrated per the frozen contract: `content_sha256` over canonical decoded bytes replaces FNV artifact provenance; no FNV artifact-provenance path remains in the write scope (grep `fnv64:` artifact sites); A9/Discovery receipts spell `sha256:{hex}`; `FmirDeviceSelection::Unknown(_)` → explicit fail-closed error naming the rule (never silently remapped to auto/metal/cuda); `FmirImageError::ArtifactDigestMismatch` matched to the landed variant; (c) `git diff --check` clean. |
| **validation** | `cargo check -p faber` (one-shot start + one-shot end per the repair-slice task; lock-contended → note, no loop); `grep -n 'content_sha256' src/package/device/section.rs src/package/device/run.rs src/package/host_factory.rs`; `grep -rn 'fnv64:' src/package/device/` (no artifact-provenance hits; S1/S2 excluded); `grep -n 'Unknown' src/package/mir/mod.rs` (fail-closed arm); `git diff --check`. |
| **depends_on** | none (phase root; DDCP2 landed). **Parallel children**: none — the compatibility boundary lands first; every later unit consumes the landed state. |
| **non_goals** | No FNV fallback/compat translation; no S1/S2 source-identity changes; no `Cargo.toml` feature wiring (U2); no packet-shape extension beyond compile-restore (U5); no `DeviceLegalityFacts` wiring (U6); no `CAMPAIGN.md`/contract edits. |
| **risk** | faber Cargo lock contention (radix hands building) — bounded: wait once, note, no loop; ambiguous receipt-format intent → record + escalate in the unit report, never silently guess. |
| **est_work_tokens** | 4k–8k (slice in flight; re-verify + residual close). **tool_latency**: medium (one `cargo check -p faber` × 2). |
| **test_owner** | hand-3 (repair-slice owner); tests = `device/device_test.rs`, `device/prefill_run_test.rs`, `mir` route tests touched by the slice. |

### DDPP1-U2 — C2 feature isolation + gate proof Proof 1/Proof 3

| Field | Content |
| --- | --- |
| **outcome** | The five gated deps are `optional` behind their features; device-runtime modules compile only under `device-runtime`; caller surfaces compile the device-selection plumbing out; `full-targets` unchanged and includes the new features; **Proof 1 and Proof 3 of the C2 gate are RUN and green** (small build excludes GPU emitters / Hosts leaves / device runtime; support crate non-pull). |
| **write_scope** | `radix/crates/faber/Cargo.toml` (optional deps + `device-runtime`/`host-macos-arm64`/`host-wasm` features + `full-targets` wiring); `radix/crates/faber/src/package/mod.rs` (module gates for `device`, `host_factory`); `radix/crates/faber/src/package/device/mod.rs` (submodule gates incl. `prefill_run`); caller compile-out hunks in `radix/crates/faber/src/cli/mod.rs`, `radix/crates/faber/src/commands/run.rs`, `radix/crates/faber/src/package/manifest.rs`, `radix/crates/faber/src/package/mir/mod.rs` + `radix/crates/faber/src/package/mir/routes.rs` (feature-gated route-selection plumbing); tests proving the small build's exclusion. |
| **read_scope** | `ddpp0-feature-isolation.md` (frozen feature set, dependency-gate table, module-gate list, Proof 1/3 commands); `ddpp0-contract.md` §GeneratedRustSupport (support crate non-pull); landed U1 state (`device/` compiles before gating). |
| **done_when** | (a) `cargo check -p faber --no-default-features --features hir-rust` exits 0 and the build graph contains **no** `radix-mir-metal`, `radix-mir-llvm`, `faber-host-macos-arm64`, `faber-host-wasm`, `faber` (faber-runtime), and no `package/device/*` / `host_factory.rs` / `prefill_run.rs` compilation (Proof 1 — exact command + expected exclusion list, verified via `cargo tree` or equivalent grep over the build graph); (b) `cargo tree -p faber-hir-rust` transitive closure contains no device-runtime or Hosts leaf crate (Proof 3); (c) default build surface unchanged (`default = ["full-targets"]`, new features in `full-targets`); (d) `git diff --check` clean. |
| **validation** | the two exact Proof commands above (RUN at this unit's boundary); `grep -n 'optional' Cargo.toml` (five gated rows); `grep -n 'device-runtime\|host-macos-arm64\|host-wasm' Cargo.toml`; `grep -n 'cfg(feature' src/package/mod.rs src/package/device/mod.rs`; `git diff --check`. Narrow in-loop: `cargo check -p faber --no-default-features --features hir-rust` only (Cargo discipline). |
| **depends_on** | U1 (landed compile state). **Parallel children**: none within the unit (single-file coherence on `Cargo.toml` + module gates); runs ahead of U3/U4. |
| **non_goals** | No gating of `radix-mir-fmir` unless the proof requires it (decision #4); no `faber targets` row-policy change (U3); no build-plan/manifest extension (U4); no packet materialization (U5); no legality seam (U6). |
| **risk** | Feature-wiring subtleties (a caller pulling the device surface through a non-obvious path) → the exclusion proof must stay honest; a *fully* minimal build may additionally need `radix-mir-fmir` gated (decision #4, recorded). Cargo lock contention → bounded. |
| **est_work_tokens** | 10k–16k. **tool_latency**: medium (one small-build `cargo check` per in-loop step). |
| **test_owner** | U2 implementing hand; named test owner for the exclusion proof = the faber lib `cli`/`package` tests exercising the `hir-rust`-only feature set (new or extended small-build test file). |

### DDPP1-U3 — `faber targets` capability truth (Proof 2)

| Field | Content |
| --- | --- |
| **outcome** | `faber targets` reports **capability truth** for the compiled build: the `rust` host lane present under `hir-rust`; **no Metal/CUDA device rows, no native/wasm host-leaf rows, no device-runtime rows for capabilities the build did not compile** (C1); target rows stay honest per row. Proof 2 RUN and green. |
| **write_scope** | `radix/crates/faber/src/commands/targets.rs` (capability-driven row set + device/host-leaf/device-runtime capability rows keyed to the compiled features); `radix/crates/faber/src/commands/` tests + `radix/crates/faber/src/cli_test.rs` assertions. |
| **read_scope** | `ddpp0-feature-isolation.md` Proof 2 (exact expected surface); `ddpp0-contract.md` §SelectionPolicy (capability truth, not a shipping promise; pair matrix); landed U2 feature set (`Cargo.toml`). |
| **done_when** | `faber targets` run against the small build (`--no-default-features --features hir-rust`) reports the `rust` host lane present and **no** Metal/CUDA device rows, **no** native/wasm host-leaf rows, **no** device-runtime rows; run against the default build reports the full honest capability surface (each row's availability reflects what was compiled); no row claims a capability the build did not compile; `git diff --check` clean. |
| **validation** | `cargo check -p faber --no-default-features --features hir-rust && ./target/debug/faber targets` (or the small-build test harness) — assert no device/host-leaf/runtime rows; default-build `faber targets` assert full surface; grep the row table for capability keys. |
| **depends_on** | U2 (features exist before truth is meaningful). **Parallel children**: none (small, one file); parallel lane with U4. |
| **non_goals** | No shipping-promise/claim changes; no T3/T4 claims; no new backend capability rows beyond what faber compiles; no `target_capabilities_for_surface` radix edits. |
| **risk** | Row-set policy ambiguity (static-table vs filtered) — decision #6 default: compiled-only capability rows for device/host-leaf/runtime surfaces; emit-target rows keep honest per-row availability. |
| **est_work_tokens** | 4k–8k. **tool_latency**: medium (small-build check + CLI run). |
| **test_owner** | U3 implementing hand; named test owner = `src/cli_test.rs` + a `targets` test (capability truth under both small and default builds). |

### DDPP1-U4 — Build-plan result + pair validation + inspection/manifest + CTO-5 namespace

| Field | Content |
| --- | --- |
| **outcome** | One inspectable `CompiledPackage`/build-plan result per analyzed package: host artifact plan + optional `DeviceProgram` + `DeviceArtifact[]` + call/submission-region facts (per §ProductShape), produced without starting a device session. **Absent target features and unsupported host/device pairs fail before build.** CPU-only Rust, FHIR/FMIR, and LLVM products remain **deliberately routed or explicitly rejected**. Build planning imports **no Hosts driver implementation** and reparses **no emitted target text**. CTO-5 artifact-namespace freeze enforced in the build-plan/manifest surface. |
| **write_scope** | `radix/crates/faber/src/package/compile.rs` (build-plan result construction), `radix/crates/faber/src/package/artifact_plan.rs` (plan result shape/extensions), `radix/crates/faber/src/package/cmd.rs` (build routing + inspection output), `radix/crates/faber/src/package/manifest.rs` (build-manifest hunks carrying packet identity rows + namespace), new build-plan tests (`radix/crates/faber/src/package/compile_test.rs` / `radix/crates/faber/src/package/artifact_plan_test.rs`). |
| **read_scope** | `ddpp0-contract.md` §ProductShape / §SelectionPolicy / §IdentityDomains (plan shape, pair matrix, identity domains); radix `target-emit-contract/goal.md` (emit boundary); `src/package/llvm.rs`/`llvm_host.rs` (LLVM lane — read-reference; composite embedding is NGAB2/DDPP3); DDCP2 packet identity fields. |
| **done_when** | (a) a build-plan result exists and is inspectable for host + device artifacts (host lane + `DeviceProgram` + `DeviceArtifact[]` + call facts; identity rows present); (b) an unsupported host/device pair and an absent target feature each **fail before any toolchain work** with a stable structured diagnostic naming the violated rule (no CPU fallback, no unrelated target deps acquired); (c) existing CPU-only Rust / FHIR / FMIR / LLVM product paths remain deliberately routed (regression tests) or are explicitly rejected with a named diagnostic; (d) the build-plan module imports **no** Hosts driver crate (`faber_host_macos_arm64`, `faber-host-wasm`) and contains **no** emitted-target-text parsing (grep/test gate); (e) build manifest + inspection output carry the packet identity rows and use the **qualified support-crate namespace** (CTO-5: no unqualified `faber::` binding in new materialized fixtures/manifests); (f) `git diff --check` clean. |
| **validation** | `grep -n 'faber_host_macos_arm64\|faber-host-wasm' src/package/compile.rs src/package/artifact_plan.rs` (absent); `grep -rn 'faber::' docs/factory/.../evidence` / new materialized fixtures (qualified names only); pair-validation negative tests (unsupported pair, absent feature); CPU-only routing regression tests; `git diff --check`. |
| **depends_on** | U2 (feature gates + capability truth), U3 (targets truth feeds pair validation). **Parallel children**: none (build-plan coherence); serial after U2/U3. |
| **non_goals** | No device session/driver code; no composite native embedding (NGAB2/DDPP3); no LLVM support-archive rebuild; no `faber targets` row changes (U3); no packet materialization proof (U5); no legality seam (U6). |
| **risk** | Pair-matrix and routing regressions — the CPU-only lanes are the campaign's ordinary product surface and must stay green; CTO-5 namespace rule touching generated fixtures must not break existing CPU-only generated-Rust paths. |
| **est_work_tokens** | 12k–20k. **tool_latency**: low–medium (greps + tests; no toolchain runs — toolchain invocation stays out of DDPP1). |
| **test_owner** | U4 implementing hand; named test owner = `compile_test.rs` / `artifact_plan_test.rs` (build-plan shape + pair validation + routing regressions). |

### DDPP1-U5 — Raw-byte packet materialization + canonical round-trip

| Field | Content |
| --- | --- |
| **outcome** | The faber device section/materialization path consumes the DDCP2 `DeviceArtifact` packet (typed backend/format/materialization-stage/target/version, canonical raw bytes, entrypoint map, reflection, `content_sha256`/`packet_sha256` + `compiler_input_packet_sha256` on finalized rows) instead of Metal/CUDA text assumptions; **text and binary payloads round-trip with hashes over canonical decoded bytes**; no emitted-target text is parsed to recover facts. |
| **write_scope** | `radix/crates/faber/src/package/device/section.rs` + `radix/crates/faber/src/package/device/wire.rs` (post-U1 packet-consumption hunks — hunk-disjoint from U1's B4–B7 hunks, serialized after U1), new `radix/crates/faber/src/package/device/packet_materialization_test.rs`, evidence fixture (`docs/factory/direct-device-product-pipeline/evidence/ddpp1-roundtrip.md` or `.txt` capturing the worked spellings + declared digests). |
| **read_scope** | `ddpp0-contract.md` §CanonicalEncoding + §RoundTripFixture (worked `binary:` spelling + declared digests, `shasum`-verifiable); DDCP2 `device.rs`/`hash.rs`/`admit.rs` packet shapes + fixture spellings; `ddcp2-closeout.md` §2 gate 1 evidence. |
| **done_when** | (a) a canonical text payload and a binary payload (the `binary:<tag>:<len>:<base64>` spelling) round-trip through faber materialization with identical canonical decoded bytes and identical `content_sha256`/`packet_sha256` (hashes over decoded bytes, never the transport spelling — verified with `shasum` against the §RoundTripFixture declared digests); (b) the section consumes the typed packet fields (backend/format/materialization-stage/target/entrypoint map) — no text-blob guessing, no parsing of emitted MSL/PTX/WGSL/LLVM text to recover facts; (c) finalized packets preserve `compiler_input_packet_sha256` parent provenance; (d) `git diff --check` clean. |
| **validation** | `printf '<canonical bytes>' | shasum -a 256` matches the fixture's declared `content_sha256`; round-trip tests in `device/packet_materialization_test.rs` (encode → materialize → admit → decode, identical digests); grep for emitted-text parsing in the section path (none); `git diff --check`. |
| **depends_on** | U4 (pair validation before materialization), U1 (landed packet boundary). **Parallel children**: none (section coherence); serial after U1 + U4. |
| **non_goals** | No physical execution / device session (T3+); no composite embedding of the artifact into a native executable (NGAB2/DDPP3); no change to `DeviceProgram` semantics (DDCP0 §SemanticProgram frozen); no AMD/WGSL leaf behavior (DDCP5/DDCP6). |
| **risk** | Encoding-spelling drift between radix codec and faber materialization — the §RoundTripFixture `shasum` check is the guard; a binary payload arriving in a non-canonical spelling must fail closed, never be silently re-encoded. |
| **est_work_tokens** | 8k–14k. **tool_latency**: low (hashing + tests). |
| **test_owner** | U5 implementing hand; named test owner = `device/packet_materialization_test.rs` (round-trip + hash-over-decoded-bytes + no-reparse proofs). |

### DDPP1-U6 — Materializer seam + DeviceLegalityFacts enforcement gate (CTO fire-8)

| Field | Content |
| --- | --- |
| **outcome** | Faber's device-program materializer (`build_semantics` / the semantic construction in `device/program.rs`) **populates `DeviceLegalityFacts` from MIR-body inspection** and enforces the device/AIR legality gate in faber materialization: a device function containing `ad`/Sermo/unresolved host effects rejects with the named structured diagnostic (`DeviceFunctionHostileEffect`-class) while the equivalent CPU host `ad` path remains valid. Closes the DDCP1 closeout §5.1 residual. |
| **write_scope** | `radix/crates/faber/src/package/device/program.rs` (legality-fact seam hunks on the landed U1 state), `radix/crates/faber/src/package/device/prefill_run.rs` (`build_semantics` seam), legality enforcement tests (`radix/crates/faber/src/package/device/program` legality test file — positive CPU `ad` + negative device `ad` in the same file set, never traded). |
| **read_scope** | `ddcp1-closeout.md` §5.1 (the residual); `radix-mir/src/device_semantics.rs` (typed `DeviceLegalityFacts`/`DeviceEffectFact`/`DeviceFunctionHostileEffect` surface, DDCP1-U6); DDCP1 fixture evidence (`ddcp1_fixtures_test.rs` fixture 7 — the same-ladder hard-gate precedent). |
| **done_when** | (a) faber's materializer populates `DeviceLegalityFacts` (and the carried effect-boundary facts) from the MIR body during device-program materialization; (b) a device function containing `ad`/Sermo/unresolved host effect **rejects during faber materialization** with a stable structured diagnostic naming the violated rule; (c) the equivalent CPU function with host `ad` through Sermo **runs unchanged** — proven in the same test file set (positive + negative together); (d) `git diff --check` clean. |
| **validation** | legality test file: negative device-`ad` case + positive CPU-`ad` case (same ladder); `grep -n 'DeviceLegalityFacts\|DeviceFunctionHostileEffect' src/package/device/program.rs src/package/device/prefill_run.rs`; narrow check per Cargo discipline. |
| **depends_on** | U1 (landed program.rs/prefill_run.rs compile state). **Parallel children**: none; runs parallel with U3–U5 (disjoint files), after U1. |
| **non_goals** | No radix-mir changes (legality surface is radix-owned; defects escalate); no enforcement in Hosts (device-leaf physical behavior is DDPP2); no autograd/tape work; no change to CPU `ad` semantics. |
| **risk** | The seam must mirror the radix-side gate without re-deciding semantics — if a materialization case is genuinely ambiguous, record + escalate, never weaken the gate (truth over safety); "summaries are claims" — re-verify the landed radix surface before wiring. |
| **est_work_tokens** | 8k–14k. **tool_latency**: medium (faber lib tests). |
| **test_owner** | **hand-3** (CTO fire-8 owner per the routing input); named test owner = the new legality test file. |

### DDPP1-U7 — Phase closeout (gate)

| Field | Content |
| --- | --- |
| **outcome** | DDPP1 closes: campaign §DDPP1 status updated (machine-parseable), the DDPP1 gate checklist recorded with per-unit evidence, README regenerated, goal-status audit 0 findings, residuals routed, DDPP2 route identified. |
| **write_scope** | `faber/docs/factory/direct-device-product-pipeline/CAMPAIGN.md` (status line only), `faber/docs/factory/direct-device-product-pipeline/ddpp1-closeout.md` (create), `faber/docs/factory/README.md` (regenerated, never hand-edited). |
| **read_scope** | all DDPP1 unit validations; campaign §DDPP1 gate; ddpp0-closeout route record; DDCP2/DDCP1 closeouts. |
| **done_when** | the campaign DDPP1 gate checklist recorded with PASSED evidence per item (raw-byte packet round-trip with hashes over canonical bytes; absent features + unsupported pairs fail before build; CPU-only Rust/FHIR/FMIR/LLVM remain routed or explicitly rejected; build planning imports no Hosts driver impl + reparses no emitted text; C2 Proof 1/2/3 green); CTO fire-8 seam closed; CTO-5 namespace freeze recorded; campaign status line set (leading clause `delivered`); README regenerated; `check-factory-goal-status` exit 0; `git diff --check` clean. |
| **validation** | `grep -n '^\*\*Status\*\*' CAMPAIGN.md`; `python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check`; `./scripta/check-factory-goal-status`; `git diff --check`. |
| **depends_on** | U1–U6. **Parallel children**: none (gate). |
| **non_goals** | No release/version bump (docs + code surface only; release checkpoint = `defer-release` — DDPP1 is not a release boundary, evidence tier T1–T2); no physical-device proofs. |
| **est_work_tokens** | 3k–5k. **tool_latency**: medium (two python scripts; no cargo). |
| **test_owner** | closeout record itself (gate checklist; README freshness; audit 0). |

## Parallelism + Lane Notes

- **Wave A — compatibility boundary (serial spine)**: U1 lands first (repair slice in flight); U2 gates after the device surface compiles.
- **Wave B — capability + plan**: U3 and U4 serial on the spine (U3 → U4); U3 is a small distinct file and may run parallel with U4's read-only prep, but U4's pair validation consumes U3's truth.
- **Wave C — materialization + legality (parallel, disjoint files)**: U5 (`device/section.rs`, `device/wire.rs`, new test + evidence) and U6 (`device/program.rs`, `device/prefill_run.rs`, legality tests) are **file-disjoint** and run in parallel after U1/U4 and U1 respectively. U5 and U6 never touch the same file.
- **Shared-file discipline (the four shared files, all hunk-disjoint + strictly serialized)**:
  1. `package/mir/mod.rs` + `mir/routes.rs` — U1 (landed compile-restore hunks: `Unknown(_)` arm, `ArtifactDigestMismatch`, type renames) **before** U2 (feature-gated route-selection compile-out hunks).
  2. `package/manifest.rs` — U2 (caller compile-out hunks) **before** U4 (build-manifest packet-identity/namespace hunks).
  3. `device/section.rs` + `device/wire.rs` — U1 (B4–B7 FNV→`content_sha256` hunks) **before** U5 (packet-shape consumption + round-trip hunks).
  4. `device/program.rs` — U1 (compile-restore hunks on the landed slice) **before** U6 (legality-fact seam hunks).
  No two units write the same file concurrently; each handoff is a landed commit boundary, and the downstream unit re-verifies against the landed state ("summaries are claims").
- **NGAB2/DDPP3 coordination**: DDPP1 owns the generic packet/build-plan surface (decision #9); NGAB2's composite-embedding write files re-file under DDPP3 and re-base on the landed DDPP1 state. No DDPP1 unit claims the composite embedding.
- **MIR Swarm / paired DDCP**: no shared MIR writes in this delivery; radix-lane defects escalate per §Escalation Path. DDCP1/DDCP2 closeouts are consumed read-only.
- **No cargo in this lowering's validation**; the commands above are the implementing units' contracts, run under the Cargo discipline (narrow in-loop checks, exactly one closeout run per unit boundary). Physical/GPU/e2e/release runs are auditor/operator gates only.

## Checkpoints And Gates

| Gate | Content | Owner |
| --- | --- | --- |
| **SG1** (after U1 lands) | faber compiles at the DDCP2 boundary; B4–B7 migration-matrix spellings re-verified against the landed commit (no FNV artifact path; `sha256:` receipts; fail-closed `Unknown`) | Mind accepts the landed repair slice |
| **SG2** (after U2) | C2 Proof 1 + Proof 3 green — small build excludes GPU emitters / Hosts leaves / device runtime; `faber-hir-rust` transitively pulls no device/Hosts; `full-targets` unchanged | Mind accepts U2 |
| **SG3** (after U3) | `faber targets` reports capability truth (Proof 2) under both small and default builds | Mind accepts U3 |
| **SG4** (after U4) | build-plan result inspectable; absent features/unsupported pairs fail before build; CPU-only/FHIR/FMIR/LLVM routed or rejected; no Hosts driver import; no emitted-text reparse; CTO-5 namespace enforced | Mind accepts U4 |
| **SG5** (after U5) | text/binary round-trip with hashes over canonical decoded bytes; packet identity survives materialization; no emitted-text reparse in the section path | Mind accepts U5 |
| **SG6** (after U6) | `DeviceLegalityFacts` populated + enforcement gate green (device `ad`/Sermo rejected; CPU `ad` valid — same ladder) | Mind accepts U6 (owner hand-3) |
| **Phase gate (U7)** | campaign §DDPP1 gate checklist all PASSED; README regenerated; `check-factory-goal-status` 0 findings; residuals routed; DDPP2 route identified | Mind closes DDPP1 |
| **Release** | `defer-release` — no version bump; DDPP1 is a T1–T2 code/planning boundary, not a release checkpoint | — |

**Batching / Split Decision**: discovery-first then batch (campaign posture). The compatibility boundary (U1) and feature isolation (U2) land first as the unblocking wave; the build-plan (U4) + packet materialization (U5) batch after the isolation proof is green; the legality seam (U6) is an independent parallel lane. Splits are named on concrete boundaries: C2 gate (U2/U3), build-plan vs packet-materialization (U4 vs U5), legality seam (U6).

## Validation

```text
U1:  cargo check -p faber (start/end); grep content_sha256|Unknown in the B4-B7 write scope;
     grep -rn 'fnv64:' src/package/device/ (no artifact hits); git diff --check
U2:  cargo check -p faber --no-default-features --features hir-rust (Proof 1); cargo tree -p faber-hir-rust (Proof 3);
     grep optional|device-runtime|host-macos-arm64|host-wasm Cargo.toml; grep 'cfg(feature' src/package/mod.rs device/mod.rs
U3:  small-build faber targets (no device/host-leaf/runtime rows); default-build faber targets (full honest surface); git diff --check
U4:  grep no Hosts driver imports in build-plan; pair-validation negative tests; CPU-only routing regressions;
     CTO-5 qualified-namespace check on new materialized fixtures; git diff --check
U5:  shasum -a 256 round-trip fixture matches declared content_sha256; packet_materialization_test (round-trip + no-reparse); git diff --check
U6:  legality test file (device-ad negative + CPU-ad positive same ladder); grep DeviceLegalityFacts|DeviceFunctionHostileEffect; git diff --check
U7:  grep '^\*\*Status\*\*' CAMPAIGN.md; generate-factory-readme.py --check; check-factory-goal-status (0 findings); git diff --check
```

No cargo in **this lowering's** validation (planning artifact). Implementing units run narrow checks in-loop and exactly one closeout at their boundary under the Cargo discipline.

## Escalation Path (radix-lane defects, `fix:<id>` discipline)

- A faber unit that hits a **radix-lane defect** (landed schema shape inconsistent with the boundary contract, emitter contract gap, missing typed surface, DDCP2/DDCP1 regressions) records it as a **named defect with a `fix:<id>` marker** and routes it to the radix lane (paired DDCP campaign / Mind), **not** a silent faber-side patch. Every applied workaround carries the `fix:<id>` marker at the site (anti-fossilization, CTO-6 discipline) so the defect cannot fossilize.
- The delivery's implementing hand never weakens a gate to stay green; an ambiguous materialization/legality/encoding case is **recorded + escalated** in the unit report with the default chosen, never silently guessed.
- Defects that change frozen contract fields (identity domains, encoding, packet shape, legality semantics) route through the campaign Open Questions path — no implementation unit decides them.

## Open Questions (for Mind; defaults recorded, none blocks)

1. **Generated-Rust support crate final name/home (campaign OQ1)** — default recorded in §GeneratedRustSupport: Faber-owned support crate, target-specific name (today `faber-hir-rust`); Proof 3 names the crate; the **operator gate at DDPP1** decides before further fixture freezes (CTO-5). DDPP1 honors the current `faber-hir-rust` name unless the operator moves it.
2. **`radix-mir-fmir` gating in a fully minimal build** — DDPP1 implementation decision (recorded residual); default: unconditional unless the C2 exclusion proof requires gating; if gated, rides `mir-fmir`.
3. **`faber targets` row-set policy** — default: compiled-only capability rows for device/host-leaf/runtime surfaces, honest per-row availability for emit targets (decision #6); no shipping-promise rows.
4. **NGAB2/DDPP3 re-file timing** — default: NGAB2 stays entry-gated and re-files under DDPP3 on the landed DDPP1 packet surface; no parallel NGAB2 implementation during DDPP1.
5. **CTO-6 `faber::` alias resolution** — routed to DDCP3/DDPP3 next-wave planning; DDPP1 only enforces the CTO-5 qualified-name rule in its own fixtures.

## Residuals Routed (not DDPP1 work)

- **CTO-6 `faber::` alias ambiguity** → DDCP3/DDPP3 planning input (decision #6) — recorded, not resolved here.
- **NGAB2 composite build + embedded artifact assembly** → DDPP3 child packet (decision #9) — its scoped delivery re-bases on the landed DDPP1 packet/build-plan surface.
- **Physical device execution, residency + performance receipts (T3/T4)** → DDPP2+ / named auditor/operator gates; no DDPP1 claim reaches T3.
- **MSL-vs-metallib embedding, PTX arch set, AMD first-leaf API** → NGAB0 operator gates / DDPP5 (recorded in §SelectionPolicy; unchanged).
- **`faber-runtime` decomposition + DDPP8 deletion gate surface** → DDPP8 (recorded in §DeletionRule); DDPP1's C2 gating is the first compile-level isolation step, not a deletion.
- **Core-support/CI/release implications** → DDPP0-U8 record + DDPP8 gate surface; unchanged by DDPP1.

---

*Planning artifact only. No product code was written; `faber-runtime/` and `hosts/` untouched; no cargo executed by this lowering. Hands implement from this spec; Mind files units (U1 is landing as hand-3's repair slice `60b340c1` and is not re-filed); Heads are advisory only. The DDPP1 gate's C2 proof is RUN by U2/U3, the fire-8 seam closes in U6 (owner hand-3), and the phase closes at U7.*
