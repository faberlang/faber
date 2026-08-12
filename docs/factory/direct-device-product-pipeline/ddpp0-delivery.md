# Delivery: DDPP0 — Joint product, ownership, and performance contract

**Goal ref**: `faber/docs/factory/direct-device-product-pipeline/CAMPAIGN.md` — DDPP0 stage (status `planned — selected`), Gate, Freeze list, §Artifact identity and payload encoding, §Prepared-session shape policy, §`faber-runtime` Decomposition Target, §Stage Dependency Graph, Open Questions, Stop Conditions
**Status**: lowered 2026-08-08 — ready for admission by Mind
**Repo**: faber (control plane). Write scope spans **faber** docs (the DDPP0 artifact set + the NGAB0 packet/NGAB1/NGAB2 amendment paths). `faber-runtime` is **read-only inventory** in this phase — no repo edits, no moves.
**Paired contract**: Radix DDCP0 delivery (`radix/docs/factory/direct-device-compilation-pipeline/`). Field-by-field agreement between the DDPP0 and DDCP0 contracts is this phase's **hardest gate**.

## Phase Intent

DDPP0 is **discovery-first and planning-only**: freeze the joint product shape, identity domains, prepared-region performance contract, ownership/decomposition destinations, child-campaign routing, and the NGAB0 major revision before any implementation stage chooses incompatible artifact, submission, crate, or migration shapes. No product code, no cargo builds, no runtime moves. Every unit produces a contract/decision/inventory artifact with a hard `done_when` and a grep-or-script proof.

## Interpreted Scope

Freeze, per the campaign DDPP0 stage list:

1. `HostArtifact + DeviceProgram + DeviceArtifact[] + call facts` product shape.
2. Backend/format/target/version/hash identity (six identity domains, SHA-256).
3. Release single-backend default + optional fat-product selection policy (C1).
4. Prepared-submission and explicit-observation performance invariants.
5. **One host call → one prepared submission region** containing one or more kernels — the NGAB0 major revision, paired field-by-field with DDCP0's amendment (H1).
6. Prepared-region regime/shape identity, bounded dynamic fields, cache keys, bounds checks, cache-miss behavior.
7. CPU/device partition ownership.
8. Generated-Rust support destination + final runtime deletion rule.
9. Exact child-campaign routing (NGAB1–4 → DDPP3, NGAB5 → DDPP7, NGAB6–7 → DDPP8).
10. Per-module/runtime-import and per-route destinations (faber `device`/`dequant`/`gguf`/`prefill`/`Valor` imports, `src/package/dispatch.rs`, `src/package/cargo.rs`, `faber-runtime/hosts/llvm`, every Hosts `Cargo.toml` path dep, core-support/release manifest schemas + examples, Triga engine/hello-voxel/graphics-MIR routes).
11. SHA-256 digest domains, canonical encoding, legacy FNV removal.
12. Explicit major-revision updates to NGAB0 (Partition, ABI, Manifest, Verification, FrozenVsReserved, Versioning, U4/U10, U11 fixture) + NGAB1 U1/U4 + NGAB2 receipts under the joint PML/NGAB packet authority.
13. LLVM support-archive ABI version + content identity, no last-good reuse, fail-closed rebuild.
14. Exact product features + dependency/module gates for a small Rust-only build (C2).
15. Clean-install/core-support/CI/release implications (C3/C4/C5).

## Normalized Spec

Artifact set — `ddpp0-contract.md` (main contract; sections frozen by exactly one unit) plus supporting artifacts:

| § / artifact | Content | Unit |
| --- | --- | --- |
| §ProductShape + §IdentityDomains + §CanonicalEncoding + §FnvRemoval + §RoundTripFixture | product shape; six identity domains; canonical UTF-8/text + binary/base64 encoding; FNV removal default; spec-only round-trip fixture with worked example | U2 |
| §PerformanceInvariants + §PreparedRegion + §SelectionPolicy + §EvidenceTiers | prepared submission/observation invariants; region regime identity + bounds/cache/miss; release selection policy (C1); evidence-tier labels (C5) | U3 |
| §PartitionOwnership + §GeneratedRustSupport + §DeletionRule + §ChildRouting | partition ownership; generated-Rust support destination; deletion rule + DDPP8 gate surface; child routing | U4 |
| `evidence/ddpp0-snapshot.md` | SHAs + dirty-state classification + baseline role-match (C0 root) | U1 |
| `ddpp0-runtime-inventory.md` | per-module/import/route destinations + deletion receipts; TR7 consumer (C3/C4) | U5 |
| `ddpp0-feature-isolation.md` | exact feature/dependency/module gates + DDPP1 gate proof **plan** (C2) | U7 |
| `ddpp0-support-archive.md` | LLVM archive ABI/content identity + clean-install/core-support/CI/release + migration note (C5) | U8 |
| NGAB0 packet + `ngab1-delivery.md` + `ngab2-delivery.md` amendment files | one-prepared-region major revision; NGAB1-U1/U4 retarget (H1) | U6 |

## Repo-Aware Baseline (verified 2026-08-08; grounding, not implementation)

- **faber `Cargo.toml`**: `default = ["full-targets"]`; `hir-rust = ["radix/hir-rust", "dep:faber-hir-rust"]`; **unconditional** GPU/runtime/Hosts deps today: `radix-mir-metal`, `radix-mir-llvm`, `radix-mir-fmir`, `faber-runtime` (package `faber`), `faber-host-macos-arm64`, `faber-host-wasm`. Feature isolation is contract-only here (U7); the proof runs at DDPP1.
- **Faber runtime-import sites**: `faber::device::DeviceSelection`/`DeviceBackend` in `src/cli/mod.rs:383-387`, `src/commands/run.rs:14`, `src/package/host_factory.rs:30`, `src/package/device/mod.rs:48`, `src/package/mir/{mod,routes}.rs`, `src/package/manifest.rs:373-377`; `faber::dequant::dequant_tensor` + `faber::gguf::admit_file` + `faber::prefill::*` in `src/package/device/prefill_run.rs:62-65`; `faber::Valor` in generated-code tests + host registration via `src/package/cargo.rs:127-138` (materialized `support.faber_runtime()`, path-links `faber-runtime`); `src/package/dispatch.rs:43-46` routes covered by `faber-runtime` `BuiltinRuntimeDispatch`/`builtin_route_frames` (keep-in-sync with `faber-runtime/src/frame.rs`).
- **`faber-runtime`** (`10d48ea47435` snapshot): `src/` carries ~45 modules (device, device_identity, device_set, discovery, fake_device, partition, transport, session, frame, http, gguf, dequant, prefill, kv_cache, decoder_ops, greedy_run, cpu_oracle, autograd, tensor/sparsa/packed_numeric, valor/textus/json/instans/intervallum/display/failable/ascii/arena, host_abi, bound_plan, capability, policy, repack_plan, execution_transaction, tokenizer, …); `hosts/llvm/` = `faber-host-llvm` (rlib+staticlib, dep `faber-runtime`); `docs/factory/autograd-substrate-inventory.md` is the existing substrate inventory U5 must reconcile.
- **`faber-runtime/hosts/llvm` consumer**: `radix/crates/faber/src/package/llvm_host.rs:147-172, 655-661` builds `libfaber_host_llvm.a` from `../faber-runtime` and links it; NGAB0/DDPP3 treat the archive as a support archive (U8).
- **Hosts path deps**: `hosts/macos-arm64/Cargo.toml:10` `faber = { package = "faber-runtime", path = "../../faber-runtime" }` + `host-kernel`, `aleator`, `consolum`, `processus`, `solum`, `tempus`, `libloading`, cfg-gated `metal`; `hosts/wasm/Cargo.toml` = `radix-host-abi` only (no faber-runtime). `hosts/AGENTS.md` in scope.
- **Core-support**: `radix/crates/faber/core-support-manifest.txt` roots `faber-runtime`, `radix/crates/radix-runtime-contract`, `hosts/crates/{host-kernel,host-native,aleator,http,consolum,processus,solum,tempus}`; `faber/build.rs` assembles `core-support.tar.zst` + `.sha256` + files sha256.
- **TR7** (`radix/docs/factory/gpu-training-lowering/stage-7-delivery.md`): TR7-U1 pins companion revisions incl. faber-runtime; the training RC rides faber-runtime via core-support. Must be a consumer row in U5 (C3).
- **PML/NGAB packet authority** (`ngab0-composite-contract.md` §Versioning): joint PML-Mind + NGAB-Mind authority, operator binding; **revisable through PML1/NGAB1**; NGAB0 operator gates: `llvm-host` identity retained+extended, MSL source-first, PTX arch set (U6 folds the revision, U9 cross-references).
- **Tooling**: `faber/scripta/check-factory-goal-status` exists (NGAB0-U10 landed); README generator `../radix/scripta/generate-factory-readme.py --factory-root docs/factory`.

**Authority order**: live source/tests and live `faber targets` → accepted artifact schemas + hardware receipts → this phase's frozen contracts → campaign prose.

## Unit Graph

```text
U1 snapshot/baseline
  └─ U2 contract core (product shape + identity + encoding)
       ├─ U3 performance/selection (serial in ddpp0-contract.md)
       │    └─ U4 ownership/deletion/routing (serial)
       ├─ U5 runtime inventory [lane B; needs U4 destination authority]
       ├─ U6 NGAB0 amendment [lane C; needs U2 + DDCP0 agreement]
       ├─ U7 feature isolation [lane D; needs U2]
       └─ U8 support archive [lane E; needs U2]
  └─ all ── U9 phase closeout (needs all + DDCP0 agreement + operator gates)
```

## Units

### DDPP0-U1 — Snapshot refresh + foreign-dirt classification + baseline
- **title**: Refresh campaign source snapshot and classify dirty state.
- **done_when**: `evidence/ddpp0-snapshot.md` records current HEAD SHAs + dirty-state classification for faber, radix, hosts, faber-runtime, gradus, triga, examples; confirms role-match grep proofs for `src/package/device/section.rs`, `src/package/device/run.rs`, `src/package/device/prefill_run.rs`, `src/package/llvm_host.rs`, `src/package/dispatch.rs`, `src/package/cargo.rs`, `radix/crates/faber/Cargo.toml` (features block), `core-support-manifest.txt`, `hosts/{macos-arm64,wasm}/Cargo.toml`, `faber-runtime/hosts/llvm/Cargo.toml`, `faber-runtime/src/lib.rs` module list; marks every Ground-Truth-table claim confirmed/drifted/superseded with a reference; classifies the NGAB0 packet/`ngab1-delivery.md`/`ngab2-delivery.md` amendment paths as in-scope dirty for DDPP0-U6 and excludes all other foreign dirt from DDPP0 write sets.
- **write_scope**: `faber/docs/factory/direct-device-product-pipeline/evidence/ddpp0-snapshot.md` (create); no other repo edits.
- **validation**: `git -C <repo> rev-parse HEAD` for the seven repos (record, no compare gate); `test -f` on the listed files; `grep -n 'hir-rust' radix/crates/faber/Cargo.toml`; `grep -n 'drifted\|superseded' evidence/ddpp0-snapshot.md`.
- **est_work_tokens**: 3k–6k. **tool_latency**: medium (seven `git rev-parse` calls, no build).
- **dependencies**: none.
- **parallel_children_considered**: none (phase root).

### DDPP0-U2 — Product shape + identity domains + canonical encoding + FNV removal
- **title**: Freeze the joint product shape, versioned SHA-256 identity domains, canonical encoding, and legacy FNV removal.
- **done_when**: `ddpp0-contract.md` §ProductShape + §IdentityDomains + §CanonicalEncoding + §FnvRemoval + §RoundTripFixture frozen: product shape = one `HostArtifact` + optional target-neutral `DeviceProgram` + zero or more versioned `DeviceArtifact[]` + host/device call and submission-region facts + effect/capability requirements; the six-domain identity table (semantic `device_identity_hash` | artifact `content_sha256` | `packet_sha256` | `execution_descriptor_hash` | distributed logical/bound-plan hashes | support-archive ABI/content identity) with authority-and-inputs + migration rule per campaign §Artifact identity; `content_sha256` over canonical raw payload bytes only; `packet_sha256` over schema version + backend + format + materialization stage + target identity + `content_sha256` + entrypoint map + canonical reflection/requirements; canonical UTF-8 text; binary payloads in `fmir-text` use an explicit binary encoding tag + decoded byte length + canonical unpadded base64; hashes cover decoded bytes, never the transport spelling; **FNV removal default recorded** (removed in the coordinated schema migration; an external-contract exemption requires recorded evidence, none presumed); a **spec-only** cross-FMIR/Faber/NGAB/Hosts round-trip fixture with a worked canonical-encoding example (declared canonical bytes → base64 spelling → declared `content_sha256`) whose hash is verifiable by `shasum` (schema fixture; no runnable code).
- **write_scope**: `faber/docs/factory/direct-device-product-pipeline/ddpp0-contract.md` (create; §ProductShape, §IdentityDomains, §CanonicalEncoding, §FnvRemoval, §RoundTripFixture).
- **validation**: `grep -n '^## .*ProductShape'` and `grep -n '^## .*IdentityDomains'` present; `grep -n 'content_sha256' ddpp0-contract.md`; `grep -n 'packet_sha256' ddpp0-contract.md`; `grep -n 'FNV' ddpp0-contract.md`; fixture: `printf '<declared bytes>' | shasum -a 256` equals the fixture's declared `content_sha256`; `git diff --check`.
- **est_work_tokens**: 7k–12k. **tool_latency**: low.
- **dependencies**: U1.
- **parallel_children_considered**: none (contract root; single-file coherence — see lane notes).

### DDPP0-U3 — Prepared-region performance + regime identity + selection policy + evidence tiers
- **title**: Freeze prepared-submission/explicit-observation invariants, prepared-region regime/bounds/cache/miss policy, release selection policy (C1), and evidence-tier labels (C5).
- **done_when**: `ddpp0-contract.md` §PerformanceInvariants + §PreparedRegion + §SelectionPolicy + §EvidenceTiers frozen: prepared submission = backend loads modules, resolves functions/pipelines, allocates persistent state, and prepares native argument/submission layouts **once**; the hot path enqueues a prepared region and synchronizes/readbacks only at an explicit observation, cancellation, dependency, or product boundary; **one host call → one prepared submission region** containing one or more kernels; region has a compiler-owned regime/shape-class identity; invocation carries only bounded numeric dynamic fields (active prompt length, dispatch extent) validated against compiled bounds **without name lookup or map construction**; cache keys = (artifact identity, region identity, shape class); cache miss prepares outside the hot loop or **fails closed** — it never interprets a kernel; §SelectionPolicy records C1 with defaults: v1 release = **single-backend default** (one backend leaf at build; fat binaries deferred), host×device matrix = **capability truth, not shipping promise** (every product cell needs its own residency+performance receipt), PTX-vs-cubin default = **PTX** (driver JIT per TR7 Stage-0 contract; cubin deferred), MSL-vs-metallib default = **MSL source first** (NGAB0 precedent; metallib reserved), AMD stays **`amd` + HSA-native** (no HIP or CUDA-translation identity; first-leaf API default HSA/ROCr with operator gate at DDPP5), fat binaries deferred; §EvidenceTiers records the four labels — **compiler emission / materialization / physical execution / performance** — and the rule that every product claim carries a tier label + named receipt.
- **write_scope**: `ddpp0-contract.md` (add §PerformanceInvariants, §PreparedRegion, §SelectionPolicy, §EvidenceTiers).
- **validation**: `grep -n 'prepared region\|prepared-region' ddpp0-contract.md`; `grep -n 'shape class\|regime' ddpp0-contract.md`; `grep -n 'bounded dynamic' ddpp0-contract.md`; `grep -n 'cache miss' ddpp0-contract.md`; `grep -n 'single-backend\|capability truth' ddpp0-contract.md`; `grep -n 'HSA' ddpp0-contract.md`; `grep -n 'evidence tier' ddpp0-contract.md`; `git diff --check`.
- **est_work_tokens**: 7k–12k. **tool_latency**: low.
- **dependencies**: U2.
- **parallel_children_considered**: none (serial in file; see lane notes).

### DDPP0-U4 — Partition ownership + generated-Rust support destination + deletion rule + child routing
- **title**: Freeze CPU/device partition ownership, the generated-Rust support destination + final runtime deletion rule, and exact child-campaign routing.
- **done_when**: `ddpp0-contract.md` §PartitionOwnership + §GeneratedRustSupport + §DeletionRule + §ChildRouting frozen: owner-per-surface rows (faber product/assembly/UX; radix compiler facts; hosts device leaves + effects/sessions; gradus ML semantics — no device handle; triga graphics source facts; generated-Rust support = **Rust-target support only**, no device session behavior); generated-Rust support destination default recorded (Faber-owned support crate, target-specific name — campaign OQ1) with the rule it **must not transitively pull device or Hosts** (C2); final deletion rule = `faber-runtime` deleted only after every listed consumer migrates, **no forwarding crate / route alias / dual authority**, renaming ≠ decomposition; the DDPP8 support-ABI/product-version gate surface list recorded (Cargo.toml, Cargo.lock, `core-support-manifest.txt`, `build.rs`, generated Cargo manifests, CI sibling checkouts, release notes, stale-archive fallback); the **"no universal runtime owner"** wording requirement (C5) recorded; §ChildRouting: NGAB0 stays an accepted historical record with amended call granularity; NGAB1–NGAB4 → DDPP3 child packets, NGAB5 → DDPP7 capstone, NGAB6–7 → DDPP8; MIR Swarm routes shared MIR writes; superseded historical clauses (target-build-pipelines GPU/LLVM packaging clauses → DDPP1; inference-session-boundary ownership superseded).
- **write_scope**: `ddpp0-contract.md` (add §PartitionOwnership, §GeneratedRustSupport, §DeletionRule, §ChildRouting).
- **validation**: `grep -n 'no universal runtime owner' ddpp0-contract.md`; `grep -n 'forwarding' ddpp0-contract.md`; `grep -n 'core-support-manifest' ddpp0-contract.md`; `grep -n 'NGAB5' ddpp0-contract.md`; `grep -n 'DDPP7' ddpp0-contract.md`; `grep -n 'DDPP8' ddpp0-contract.md`; `git diff --check`.
- **est_work_tokens**: 6k–10k. **tool_latency**: low.
- **dependencies**: U3.
- **parallel_children_considered**: none (serial in file).

### DDPP0-U5 — faber-runtime consumer + import inventory (C3/C4)
- **title**: Inventory every faber-runtime consumer, module, and import; assign exactly one destination + a deletion receipt each; TR7 included.
- **done_when**: `ddpp0-runtime-inventory.md` frozen: module-by-module + import-by-import inventory over `faber-runtime/src/` (each of the ~45 modules) and `faber-runtime/hosts/llvm`, each with **exactly one destination** per the campaign Decomposition Target + a **deletion receipt**; Faber-side import sites named with their destination — `faber::device` (`cli/mod.rs`, `commands/run.rs`, `package/host_factory.rs`, `package/device/*`, `package/mir/{mod,routes}.rs`, `package/manifest.rs`), `faber::dequant`/`faber::gguf`/`faber::prefill` (`package/device/prefill_run.rs`), `faber::Valor` + generated host registration (`package/cargo.rs`, `package/dispatch.rs` ↔ `faber-runtime/src/frame.rs` `builtin_route_frames`); `faber-runtime/hosts/llvm` (`package/llvm_host.rs`); every Hosts `Cargo.toml` path dep (`hosts/macos-arm64` → faber-runtime + host-kernel/aleator/consolum/processus/solum/tempus; `hosts/wasm` → radix-host-abi); `hosts/AGENTS.md`; core-support/release manifest schemas + examples (`core-support-manifest.txt`, `release-manifest.yaml` + schema, generated Cargo manifests); Triga engine / hello-voxel / graphics-MIR routes (grep-discovered, per campaign §Decomposition Target); **TR7 training RC as a consumer** (rides faber-runtime via core-support; immutable pinned receipts — C3); **PML2 GI1 admission migration named as a DDPP8 prerequisite** and the **PML0 capsule faber-runtime carriage reference reconciled** (C4); the existing `faber-runtime/docs/factory/autograd-substrate-inventory.md` reconciled (autograd/cpu oracles → repo-owned test/oracle fixtures); ordinary `ad` preserved and GPU submission statically excluded from Sermo/`Valor`/`HostDispatch`/route selection. **faber-runtime repo read-only.**
- **write_scope**: `faber/docs/factory/direct-device-product-pipeline/ddpp0-runtime-inventory.md` (create). No edits in `faber-runtime/`.
- **validation**: `test -f ddpp0-runtime-inventory.md`; `grep -n 'TR7' ddpp0-runtime-inventory.md`; `grep -n 'PML2' ddpp0-runtime-inventory.md`; `grep -n 'prefill_run' ddpp0-runtime-inventory.md`; `grep -n 'deletion receipt' ddpp0-runtime-inventory.md`; `grep -c '^| ' ddpp0-runtime-inventory.md` ≥ the row count declared in the doc's own summary line; `git diff --check`.
- **est_work_tokens**: 10k–15k. **tool_latency**: medium (multi-repo grep discovery, no build).
- **dependencies**: U1 (baseline), U4 (destination authority).
- **parallel_children_considered**: independent of U3/U6/U7/U8 (distinct files); serializes only after U4's destination authority.

### DDPP0-U6 — NGAB0 major packet revision + NGAB1/NGAB2 retarget (H1 paired amendment)
- **title**: Amend NGAB0 (one-call → one-prepared-region; `artifact_id` → `content_sha256` role) and retarget NGAB1-U1/U4 + NGAB2 receipts under the joint PML/NGAB packet authority; record the field-by-field DDCP0 agreement checklist.
- **done_when**: the NGAB0 packet's §Partition, §Abi, §Manifest, §Verification, §FrozenVsReserved, §Versioning amended to **one prepared-region granularity** (one host call → one prepared submission region containing one or more kernels; the one-kernel fixture remains the minimal case, not the ABI limit) and §Manifest `artifact_id` re-roled as `content_sha256` with `packet_sha256` admission identity; `ngab0-receipt-schema.md` (U10) gains prepared-region + `content_sha256`/`packet_sha256` fields; `ngab0-fixture-contract.md` (U11) retargeted so the one-kernel fixture **proves the minimal one-region case**; `ngab1-delivery.md` U1 done-when + U4 updated to the prepared-region granularity; `ngab2-delivery.md` materialization/verification receipts updated; the revision recorded in §Versioning as a **MAJOR revision** under the joint PML/NGAB authority; the **field-by-field agreement checklist with DDCP0's amendment unit** recorded (region granularity, identity domains, encoding, version authority) — every agreed field cites the DDCP0 contract field name and none contradicts; unresolved fields are marked `PENDING-AGREEMENT` and block the U9 phase gate, not this unit's write.
- **write_scope**: `faber/docs/factory/native-gpu-application-bundle/{ngab0-composite-contract.md, ngab0-receipt-schema.md, ngab0-fixture-contract.md, ngab1-delivery.md, ngab2-delivery.md}`.
- **validation**: `grep -n 'prepared region\|prepared-region' ngab0-composite-contract.md`; `grep -n 'content_sha256' ngab0-composite-contract.md ngab0-receipt-schema.md`; `grep -n 'minimal one-region' ngab0-fixture-contract.md`; `grep -n 'MAJOR' ngab0-composite-contract.md`; `grep -n 'DDCP0' ngab0-composite-contract.md`; `grep -n 'PENDING-AGREEMENT\|field-by-field' <amendment marker>`; `git diff --check`.
- **est_work_tokens**: 7k–12k. **tool_latency**: low.
- **dependencies**: U2 (identity domains), plus the DDCP0 amendment unit's frozen field list (radix delivery spec; in flight — record `PENDING-AGREEMENT` where not yet landed).
- **parallel_children_considered**: separate files from the DDPP contract (lane C). **NGAB1–NGAB4 must not start implementation until this lands** (H1 hold lifts here).

### DDPP0-U7 — Feature-isolation contract + DDPP1 gate proof plan (C2)
- **title**: Freeze exact product features + dependency/module gates for a small Rust-only build; specify the DDPP1 gate proof **plan** (plan, not run).
- **done_when**: `ddpp0-feature-isolation.md` frozen: current-state table (unconditional deps: `radix-mir-metal`, `radix-mir-llvm`, `faber-runtime`, `faber-host-macos-arm64`, `faber-host-wasm`; unconditional modules: `package/device/*`, `package/host_factory.rs`, `package/device/prefill_run.rs`); the exact product feature set for a small Rust-only build (default-features off + `hir-rust`-only target features); the dependency-gate table (each GPU emitter / physical Hosts leaf / device-runtime dependency → optional feature + gate rule); the module-gate list (device-runtime modules excluded without their feature); the **DDPP1 gate proof PLAN** — the exact command `cargo check -p faber --no-default-features --features hir-rust` and the expected exclusion list (GPU emitters, physical Hosts leaves, device runtime) plus `faber targets` reporting matching capability truth — specified as commands to be run at DDPP1, **not here**; the generated-Rust support crate non-pull rule (no transitive device/Hosts) restated as a DDPP1 gate.
- **write_scope**: `faber/docs/factory/direct-device-product-pipeline/ddpp0-feature-isolation.md` (create).
- **validation**: `test -f ddpp0-feature-isolation.md`; `grep -n 'hir-rust' ddpp0-feature-isolation.md`; `grep -n 'no-default-features' ddpp0-feature-isolation.md`; `grep -n 'DDPP1 gate proof' ddpp0-feature-isolation.md`; `grep -n 'transitively' ddpp0-feature-isolation.md`; `git diff --check`.
- **est_work_tokens**: 6k–10k. **tool_latency**: low.
- **dependencies**: U2 (product shape feeds the gate list).
- **parallel_children_considered**: none (small, one file); parallel lane with U3–U6/U8.

### DDPP0-U8 — LLVM support-archive ABI/content identity + clean-install/core-support/CI/release
- **title**: Freeze the LLVM support-archive ABI version + content identity, no last-good reuse, fail-closed rebuild; record clean-install/core-support/CI/release implications + the C5 migration note.
- **done_when**: `ddpp0-support-archive.md` frozen: the `faber-host-llvm` archive (`faber-runtime/hosts/llvm`) carries an explicit **ABI version** + **SHA-256 content receipt** over canonical archive bytes; faber rebuilds it from the selected support sources; **stale last-good archive reuse forbidden** (no silent `llvm_host.rs` reuse); rebuild failure or identity mismatch **fails closed**; support-archive identity is a distinct domain in §IdentityDomains (cross-referenced); clean-install/core-support/CI/release implications recorded — `core-support-manifest.txt` entries, faber `build.rs` core-support assembly + `.sha256`, CI sibling checkouts, release-manifest schema + examples, generated Cargo manifests, and the DDPP8 deletion-gate references; the **C5 migration-note requirement** recorded: before DDPP8 release work a migration note covers deletion, support surfaces, feature/toolchain changes, `core-support-manifest`, sibling checkouts, no facade.
- **write_scope**: `faber/docs/factory/direct-device-product-pipeline/ddpp0-support-archive.md` (create).
- **validation**: `test -f ddpp0-support-archive.md`; `grep -n 'last-good\|last good' ddpp0-support-archive.md`; `grep -n 'fails closed\|fail-closed' ddpp0-support-archive.md`; `grep -n 'ABI version' ddpp0-support-archive.md`; `grep -n 'migration note' ddpp0-support-archive.md`; `git diff --check`.
- **est_work_tokens**: 4k–7k. **tool_latency**: low.
- **dependencies**: U2 (support-archive identity domain row).
- **parallel_children_considered**: none (small, one file).

### DDPP0-U9 — Phase closeout
- **title**: Close DDPP0: status update, decisions folded, gates green, README regenerated, DDPP1 route identified.
- **done_when**: campaign `**Status**` line + DDPP0 stage status updated (machine-parseable per factory vocabulary); the **DDCP0 field-by-field agreement gate is closed** (no `PENDING-AGREEMENT` fields) **or** the blocker is recorded as a routed need; every campaign freeze item is listed as satisfied with a cited artifact + section; campaign Open Questions folded — OQ1 generated-Rust support crate name (default recorded), OQ3 PTX-vs-cubin, OQ4 MSL-vs-metallib, OQ5 first AMD API (all recorded with defaults in §SelectionPolicy; operator gate at U12-equivalent DDPP boundaries) — or explicitly deferred with recorded defaults; NGAB0 operator gates cross-referenced (U6 amendment did not silently change them); **DDPP1's post-DDCP2 route identified** (delivery route, owner, fixture, done oracle per campaign gate); faber docs README regenerated; `git diff --check` clean; `check-factory-goal-status` exit 0.
- **write_scope**: `faber/docs/factory/direct-device-product-pipeline/CAMPAIGN.md` (status lines + DDPP0 stage Notes), `faber/docs/factory/README.md` (regenerated, never hand-edited).
- **validation**: `grep -n '^\*\*Status\*\*' CAMPAIGN.md`; `python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check`; `scripta/check-factory-goal-status`; `git diff --check -- docs/factory/direct-device-product-pipeline docs/factory/README.md`.
- **est_work_tokens**: 3k–6k. **tool_latency**: medium (two python scripts, no cargo).
- **dependencies**: U1–U8, plus the DDCP0 agreement gate + operator answers (folded or deferred with recorded defaults).
- **parallel_children_considered**: none (gate).

## Parallelism + Lane Notes

- **Lane A (contract, serial)**: U1 → U2 → U3 → U4 — single-file coherence on `ddpp0-contract.md`; parallel section files were considered and rejected (NGAB0 precedent).
- **Lane B (inventory)**: U5 after U1+U4 — distinct file, parallel with lanes A/C/D/E.
- **Lane C (NGAB0 amendment)**: U6 after U2 — distinct files (NGAB0 packet paths), parallel with lane A. **Hot-path caution**: U6 edits `native-gpu-application-bundle/` while NGAB1 is held — the amendment is what un-holds NGAB1; no NGAB1–NGAB4 implementation starts until U6 lands.
- **Lane D (feature isolation)**: U7 after U2 — distinct file.
- **Lane E (support archive)**: U8 after U2 — distinct file.
- **Cross-campaign**: DDPP0 runs in parallel with DDCP0 lowering (its contract delivery is U6's agreement partner), the ongoing wave (PML1, NGAB1-hold, GI3, TR7), and MD work. No shared code hot paths are touched; `faber-runtime/` is read-only; no unit runs cargo.
- **Overlap rule**: no unit writes radix/hosts/gradus/triga source, no `src/package/*` code, no `faber-runtime/` edits.

## Checkpoints And Gates

- **C0 (root)**: U1 — snapshot + dirt classification committed.
- **Contract core**: after U2 — product shape, identity domains, encoding, FNV removal frozen.
- **Prepared-region**: after U3 — performance invariants + regime/bounds/cache/miss + selection policy + evidence tiers frozen.
- **Inventory gate**: after U5 — every current runtime import and route has exactly one destination + a deletion receipt; ordinary `ad` preserved; GPU submission statically excluded.
- **Amendment gate (hardest)**: U6 — field-by-field agreement with DDCP0 on region granularity, identity domains, encoding, version authority; `PENDING-AGREEMENT` fields block U9.
- **Packet gate**: after U4+U7+U8 — all contract sections + supporting artifacts present, cross-references resolve, no dangling decision gates except the recorded operator gates.
- **Phase gate**: U9 — freeze list closed, README fresh, audit exit 0.
- **Release**: no version bump (docs + contract only).

## Validation

Phase-level commands (unit-level proofs in each unit; **no cargo build/test per Cargo discipline** — the DDPP1 gate proof is a PLAN recorded by U7, not a run):

```bash
cd faber
grep -n '^## ' docs/factory/direct-device-product-pipeline/ddpp0-contract.md        # all § present
grep -n 'content_sha256' docs/factory/direct-device-product-pipeline/ddpp0-contract.md
grep -n 'prepared region\|prepared-region' docs/factory/native-gpu-application-bundle/ngab0-composite-contract.md
test -f docs/factory/direct-device-product-pipeline/ddpp0-runtime-inventory.md
grep -n 'TR7' docs/factory/direct-device-product-pipeline/ddpp0-runtime-inventory.md
python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check
scripta/check-factory-goal-status
git diff --check -- docs/factory/direct-device-product-pipeline docs/factory/native-gpu-application-bundle docs/factory/README.md
```

## Open Questions (routed to DDPP0/planner; operator gate; defaults safe to proceed)

1. **Generated-Rust support crate name/home** (campaign OQ1) — default: Faber-owned support crate, target-specific name; §GeneratedRustSupport records the default; operator gate at DDPP1.
2. **PTX-vs-cubin** (campaign OQ3) — default: PTX (driver JIT per TR7 Stage-0 contract); cubin deferred; §SelectionPolicy.
3. **MSL-vs-metallib** (campaign OQ4) — default: MSL source first, metallib reserved; §SelectionPolicy; NGAB0 operator gate cross-referenced.
4. **First AMD leaf API** (campaign OQ5) — default: HSA/ROCr native; identity stays `amd` + HSA-native; operator gate at DDPP5.
5. **Stable output layout** (campaign OQ6) — deferred to DDPP1 by design; DDPP0 names only the layout requirement classes.
6. **DDCP0 agreement checklist** — tracked in U6; `PENDING-AGREEMENT` fields are routed to the paired planner; blocks U9, not U6's write.

## Council Dispositions Folded (H1 + C1–C5)

| Item | Mandate | Landed in |
| --- | --- | --- |
| **H1** | NGAB1 HOLD — DDPP0 lands the NGAB0 major packet revision first (one-call → one-prepared-region; `artifact_id` → `content_sha256`), then NGAB1-U1 is retargeted so the one-kernel fixture proves the minimal one-region case; field-by-field agreement with DDCP0 is a hard gate; NGAB1–4 → DDPP3, NGAB5 → DDPP7, NGAB6–7 → DDPP8 | U6 (amendment + retarget + agreement checklist); U4 §ChildRouting |
| **C1** (cpo) | v1 single-backend default release; host×device matrix = capability truth, not shipping promise — every product cell needs its own residency+performance receipt; PTX-vs-cubin, MSL-vs-metallib, AMD stays `amd`+HSA-native, fat binaries deferred — resolved with recorded defaults, not left to Hands | U3 §SelectionPolicy |
| **C2** (cpo) | Feature isolation is the developer-story contract; DDPP1 gate proof plan specified (`cargo check -p faber --no-default-features --features hir-rust` excludes GPU emitters/physical Hosts leaves/device runtime; `faber targets` matches truth); generated-Rust support crate must not transitively pull device/Hosts | U7 (`ddpp0-feature-isolation.md`, proof is a plan); U4 §GeneratedRustSupport |
| **C3** (cxo/ceo) | TR7 in the faber-runtime consumer inventory (training RC rides faber-runtime via core-support; immutable pinned receipts); DDPP8 support-ABI/product-version gate (Cargo.toml, core-support-manifest, build.rs, generated manifests, CI checkout, stale-archive fallback) | U5 (TR7 consumer row); U4 §DeletionRule gate surface; U8 |
| **C4** (ceo/cso/cto) | faber-runtime is not a rename — delete only after every consumer migrates; PML2 executes the GI1 admission migration before DDPP8; no forwarding crate; PML0 capsule faber-runtime carriage reference reconciled | U5 (PML2 prerequisite + PML0 carriage row); U4 §DeletionRule |
| **C5** (cmo) | Say "no universal runtime owner" (not "no runtime"); label every claim by evidence tier (compiler emission vs materialization vs physical execution vs performance); migration note before DDPP8 release work | U4 §DeletionRule (wording); U3 §EvidenceTiers (labels); U8 (migration note) |

---

*Planning artifact only. No product code was written; `faber-runtime/` untouched. Hands implement from this spec at DDPP1+; Mind files units; Heads are advisory only.*
