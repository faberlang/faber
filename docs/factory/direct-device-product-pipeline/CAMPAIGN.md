# Campaign: Direct Device Product Pipeline

**Status**: active — DDPP0 delivered (phase closeout 2026-08-08); DDPP1 next after DDCP2 contract
**Created**: 2026-08-08
**Mode**: routing artifact — draft/maintain; does not implement code directly
**Control-plane repo**: `/Users/ianzepp/work/faberlang/faber`
**Paired compiler campaign**:
[`radix/docs/factory/direct-device-compilation-pipeline`](../../../../radix/docs/factory/direct-device-compilation-pipeline/CAMPAIGN.md)
**Child product campaign**:
[`native-gpu-application-bundle`](../native-gpu-application-bundle/CAMPAIGN.md)
**Implementation repos**: `faber`, `hosts`, temporary migration work in
`faber-runtime`, and the generated-Rust support surface; consumes compiler
artifacts from `radix`, ML semantics from `gradus`, and graphics contracts from
`triga`
**Lowers to**: `delivery` then `factory`; cross-repo stages lower into separate
repo-owned delivery specs
**Campaign readiness**: **READY FOR DELIVERY — DDPP0 selected**

## Summary

Build one direct product pipeline from a checked Faber package to a host
artifact plus zero or more compiler-authored device artifacts or compiler
inputs. Faber materializes external-toolchain outputs, assembles, and links the
product. Backend-specific Hosts leaves prepare native modules,
pipelines, buffers, queues, and submission regions once. The running inference
or training loop calls the selected backend directly without `ad`, `Sermo`,
`Valor`, provider routing, emitted-text reconstruction, per-launch symbol
lookup, or per-kernel synchronization.

This campaign also removes `faber-runtime` as a universal architectural owner.
The old repository is decomposed into generated-Rust language support, Hosts
contracts and implementations, Gradus semantics, Radix ABI facts, and
test-only oracles. No replacement universal runtime is created.

## Problem

The workspace contains the ingredients for direct GPU execution, but the
product path is split and the ownership is blurred:

- Faber can build a CPU `llvm-host` executable.
- Faber can separately build an FMIR device section containing a target-neutral
  `DeviceProgram`, Metal MSL, and CUDA PTX.
- Hosts can execute real Metal and CUDA kernels.
- WebGPU browser code already performs direct command encoding and submission.
- `faber-runtime` contains generated-code carriers, concrete effect fallbacks,
  device selection and handles, multi-device planning, inference helpers,
  CPU oracles, an autograd oracle, and the LLVM host archive.
- The proof-oriented native device executor resolves resources and kernel
  functions during execution and synchronizes too often for an inference or
  training hot path.
- FMIR device artifacts are text-shaped and hard-code Metal/CUDA, which blocks
  binary AMDGPU code objects and keeps backend growth scattered across Faber,
  Radix, runtime, and Hosts.

Moving files alone would not fix this. The product needs a build-time artifact
contract and prepared backend execution path first. Runtime decomposition
follows the accepted direct path; it must not install forwarding shims that
preserve the old architecture.

## Desired End State

1. Faber builds a structured product from one analyzed package:

   ```text
   CompiledPackage
   ├── host artifact
   ├── optional target-neutral DeviceProgram
   ├── zero or more versioned device artifacts
   ├── host/device call and submission-region facts
   └── effect/capability requirements
   ```

2. Host and device selection are independent axes. Supported examples include:

   | Host lane | Device leaf | Product |
   | --- | --- | --- |
   | HIR -> Rust | Metal | native Rust application on Apple GPU |
   | HIR -> Rust | CUDA | native Rust application on NVIDIA GPU |
   | HIR -> Rust | AMD HSA | native Rust application on AMD GPU |
   | MIR -> LLVM | Metal | native LLVM CPU host plus embedded Metal artifact |
   | MIR -> LLVM | CUDA | native LLVM CPU host plus embedded PTX |
   | MIR -> LLVM | AMD HSA | native LLVM CPU host plus embedded AMDGPU code object |
   | MIR -> Wasm/browser product | WebGPU | Wasm/JS host plus WGSL and reflection |

3. Unsupported pairs reject during planning. They do not acquire unrelated
   target dependencies and do not fall back to CPU or another backend.
4. A release/native build selects one backend leaf by default. Optional fat
   products select once at startup and enter a backend-specialized execution
   loop; they do not branch or look up providers for each launch.
5. Backend preparation loads modules, resolves functions/pipelines, allocates
   persistent state, and prepares native argument/submission layouts once.
6. A compiled submission region enqueues several kernels and synchronizes only
   at an explicit observation, cancellation, dependency, or product boundary.
7. Model weights, logical KV state materialized as physical buffers,
   activations, gradients, parameters, optimizer state, and reusable workspace
   remain device resident according to compiler lifetime facts.
8. The LLVM native product embeds its selected device artifact and requires no
   separately located kernel file.
9. The WebGPU product consumes WGSL plus typed reflection, creates persistent
   browser resources, and crosses Wasm/JavaScript at submission-region
   boundaries rather than per kernel.
10. `faber-runtime` is deleted after all live consumers move. The remaining
    generated-Rust support is explicitly Rust-target support, not a universal
    runtime.

## Non-Negotiable Hot-Path Requirements

The optimized inference/training path must contain none of the following:

- `ad` route dispatch;
- `Sermo`, frame materialization, or `Valor` conversion;
- host provider-prefix lookup or capability-registry lookup per launch;
- serialization or deserialization of tensor/module payloads;
- parsing MSL, PTX, WGSL, LLVM text, or AMD code objects to recover compiler
  facts;
- runtime reconstruction of binding layouts from field names or positional
  guesses;
- kernel-name lookup after session preparation;
- implicit host/device copies at operation boundaries;
- an unconditional synchronization or readback after each kernel;
- a runtime autograd tape for compiler-generated backward programs;
- compatibility translation between CUDA, Metal, AMD, and WebGPU kernels.

The unavoidable platform boundary must be thin and concrete: Metal API,
CUDA Driver API, AMD HSA/HIP module API, or browser WebGPU. Abstraction is used
to produce and verify the build plan, not to interpret computation while it
runs.

## Development Posture

- **Clean break.** Internal workspace callers are not a compatibility contract.
  Migrations update producers and consumers together. No old-runtime facade,
  forwarding crate, route alias, or dual authority survives closeout.
- **Performance before file moves.** Prepared submission, residency, and
  synchronization behavior are proved before runtime ownership is moved.
- **Compiler facts are immutable inputs.** Faber and Hosts consume the Radix
  artifact packet; they do not infer device semantics.
- **Backend leaves are concrete.** Shared code may cover artifact identity,
  error transport, and lifecycle invariants. Driver calls, target metadata,
  memory rules, queueing, synchronization, and tuning remain backend-owned.
- **Feature gates must become real.** Faber currently exposes target features
  but still declares several GPU/runtime/Hosts dependencies and package modules
  unconditionally. DDPP0/DDPP1 must make a small Rust-only build exclude GPU
  emitters, physical Hosts leaves, and device runtime code.
- **No lowest common denominator.** Compute and graphics may share resource
  identities, artifact envelopes, hashing, and session lifecycle, but keep
  distinct execution contracts.
- **Evidence distinguishes proof and production.** Fake backends and CPU
  oracles validate sequencing and numerics; they do not prove physical GPU
  execution or performance.

## Implementation Workflow

This campaign is the product control plane. Stage prose is not filed directly
to implementation Hands.

1. DDPP0 passes goal-check and lowers to a Faber delivery spec paired with the
   Radix DDCP0 contract delivery.
2. Each cross-repo stage is split into repo-owned producer and consumer specs.
   Radix facts land before or at the coordinated boundary where Faber consumes
   them; Hosts and Gradus never guess unpublished contracts.
3. NGAB0 remains an accepted historical architecture record. DDPP0 must amend
   its one-host-call/one-kernel ABI before NGAB1. NGAB1-NGAB4 then become child
   packets for DDPP3, NGAB5 remains a native ML capstone, and NGAB6-NGAB7 feed
   DDPP8. They are not a parallel authority.
4. Shared MIR writes are routed through the Radix MIR Swarm. Hosts, Gradus,
   Triga, and temporary `faber-runtime` migration work stays in repo-local
   delivery/factory artifacts.
5. Every implementation unit names cwd, exact paths, feature/build request,
   fixture, expected receipt or diagnostic, source owner, and done oracle.
6. Physical GPU, browser, paid-provider, clean-install, and release proofs run
   only at named auditor/operator gates. Accepted results update this campaign
   and any child campaign whose status they change.

## Current State

| Authority or campaign | Current relationship | Next action under DDPP |
| --- | --- | --- |
| This DDPP campaign | planned; DDPP0 selected | goal-check and lower the joint product/ownership contract only |
| [Paired DDCP campaign](../../../../radix/docs/factory/direct-device-compilation-pipeline/CAMPAIGN.md) | compiler producer authority | reconcile shared schema, MIR routing, and backend policy in DDCP0/DDPP0 |
| [Native GPU Application Bundle](../native-gpu-application-bundle/CAMPAIGN.md) | NGAB0 accepted with a one-call/one-kernel ABI | amend call granularity before NGAB1; later stages become DDPP child packets |
| `faber-runtime` | live generated-Rust support plus mixed host/device/model/oracle ownership | inventory every import and consumer in DDPP0; move only after replacements pass |
| Faber feature isolation | not implemented: GPU/runtime/Hosts dependencies and modules are unconditional | DDPP0/DDPP1 define exact product features and prove a small Rust-only build excludes them |
| Hosts Metal/CUDA | real physical execution with proof-oriented session behavior | DDPP2 creates backend leaves with prepared multi-kernel submission |
| [Gradus production ML](../../../../gradus/docs/factory/production-ml-library/CAMPAIGN.md) | forward ML semantic owner; planned PML stages | gate DDPP7 inference and training on their distinct accepted stages |
| [WebGPU browser host](../../../../hosts/webgpu-browser/README.md) | direct compute/graphics command submission exists | preserve direct host execution and add persistent product sessions in DDPP6 |
| [Triga graphics MIR stages](../../../../triga/docs/factory/triga-threejs-80/goals/04-graphics-mir-shader-stages.md) | graphics source-to-MIR remains incomplete | route source/compiler gaps to Triga/Radix; do not hide them in Hosts |
| AMD native backend | no accepted compiler artifact or Hosts leaf | Radix emits AMDGPU compiler input; Faber materializes it; Hosts loads/runs it |

## Ownership Map

| Concern | Authority | Product consequence |
| --- | --- | --- |
| Faber source semantics | Radix frontend and language spec | Faber never rewrites semantics during packaging |
| `DeviceProgram`, value generations, dependencies, lifetimes, observations | Radix MIR | consumed as typed facts |
| Device compiler inputs/source artifacts, entrypoints, target requirements, hashes | Radix backend leaves | consumed without semantic reconstruction |
| Final artifacts requiring external compilers/linkers, plus input-to-output provenance | Faber | materialized once during build, then embedded unchanged |
| Host target selection, toolchains, materialization, linking, inspection | Faber | one product build plan |
| Backend probe, module load, physical allocation, queue/stream, launch, sync | Hosts backend leaves | direct prepared execution |
| `ad` effect providers | Hosts kernel/native/provider crates | separate from device execution |
| Generated Rust values and methods | Faber Rust support surface | linked only by HIR-Rust products |
| Model/tokenizer/forward/decode/logical KV/sampling semantics | Gradus | no device handles or drivers |
| Shader/graphics source contracts | Triga plus Radix shader facts | no WebGPU handles in public types |
| Browser adapter/device, pipelines, render passes, presentation | `hosts/webgpu-browser` | direct WebGPU product host |
| Serving, batching, HTTP API, deployment | future inference product | outside this campaign |

## Scope Routing

- Radix owns semantic and emitted compiler facts. Shared MIR work routes
  through MIR Swarm and the Faber MIR v1 roadmap.
- Faber owns target pairing, external toolchains, artifact materialization,
  linking, support selection, product manifests, and inspection.
- Hosts owns concrete effects and physical device/browser execution.
- Gradus owns inference and training semantics. Triga owns graphics source
  contracts. Neither owns driver handles or product linking.
- The temporary `faber-runtime` repository is in migration scope. It is not a
  forward owner and cannot become a containment facade.
- Serving, distributed scheduling, deployments, and provider purchasing remain
  outside this campaign.

## Dependency Rules

1. DDCP compiler contracts precede dependent Faber product consumers.
2. The NGAB0 call-granularity amendment precedes NGAB1 and DDPP2/DDPP3.
3. DDPP1 waits for DDCP2; DDPP0 only identifies its delivery route.
4. DDPP7 inference waits for accepted Gradus PML1/PML2/PML3/PML5 work.
   DDPP7 training waits for accepted PML1/PML3/PML4 work. These are device
   integration receipts, not the final Gradus PML7 release capstone; PML7 also
   waits for PML6 and its examples/release gates.
5. DDPP8 deletion waits for every listed consumer migration, an external
   consumer audit, and accepted native/browser capstones.
6. Repo-local deliveries name explicit owners and write sets. Cross-repo
   compatibility boundaries are landed in producer-before-consumer order.

## `faber-runtime` Decomposition Target

This table is routing authority for later inventory/delivery work. DDPP0 must
refresh exact module consumers before any move.

| Current family | Destination | Closeout rule |
| --- | --- | --- |
| `ascii`, `textus`, `valor`, `json`, `instans`, `intervallum`, display/failable/recovery helpers, CPU `tensor`/`sparsa` semantics needed by generated Rust | Faber-owned generated-Rust support crate | target-specific name and dependency; no device session behavior |
| Sermo/frame language carriers, generated-Rust client calls, and the Rust `HostDispatch` trait | Faber-owned generated-Rust support contract | Hosts depends on this narrow contract and installs an implementation; no concrete effect or GPU launch behavior lives here |
| `frame` built-in filesystem/process/console/time/random/HTTP implementations and `http` client effects | Hosts providers | one provider authority; no runtime fallback duplication |
| LLVM/C `ad` and host-call symbols/layouts | `radix-host-abi` contract plus Hosts-owned LLVM process support | one versioned ABI; ordinary host effects preserved |
| generated-language value/frame wire contracts | `radix-runtime-contract` | compiler-owned internal ABI facts; no Hosts implementation or driver code |
| `device` selection/build metadata | Radix artifact contract plus Faber build configuration | absent from generated language values |
| physical device handles, discovery, identity, topology, partition, health, transport, transactions, bound plans | Hosts or a later execution coordinator | no generated-Rust/runtime ownership |
| `gguf`, tokenizer, quantized model interpretation, logical decode/KV/sampling/prefill semantics | Gradus PML stages | old paths deleted after accepted Gradus consumer |
| backend-specific lowering/repacking | Radix leaf; physical upload/repacking in Hosts | no model semantics in Hosts |
| CPU logits/decode/greedy/autograd reference code | repo-owned test/oracle fixtures | not linked into production support |
| `faber-runtime/hosts/llvm` | Hosts-owned LLVM process support | linked by Faber `llvm-host`; no fake physical GPU claim |

The final deletion gate includes `Cargo.toml`, `Cargo.lock`,
`core-support-manifest.txt`, `build.rs`, generated Cargo manifests, CI sibling
checkouts, release notes, and all source imports. Renaming only the crate while
leaving the same ownership mix does not satisfy the campaign.

DDPP0's inventory is module-by-module and import-by-import. It must include
Faber's direct `faber::device`, `faber::dequant`, `faber::gguf`,
`faber::prefill`, and `faber::Valor` imports; `src/package/dispatch.rs` and
generated host registration in `src/package/cargo.rs`;
`faber-runtime/hosts/llvm`; every Hosts `Cargo.toml` path dependency;
`hosts/AGENTS.md`; core-support/release manifest schemas and examples; and the
active Triga engine, hello-voxel, and graphics-MIR routes. Every import and
route receives exactly one destination and a deletion receipt.

Ordinary `ad` remains the language's IO-bound host-effect seam:

```text
Faber `ad` -> Radix Sermo MIR -> generated-language/LLVM effect ABI
           -> Hosts dispatch/kernel -> concrete provider
```

GPU submission is separate and must be statically unreachable from Sermo,
`Valor`, `HostDispatch`, and route selection. Generated-Rust and LLVM fixtures
must preserve ordinary `ad` behavior while device fixtures prove the exclusion.

## Product Build Pipeline

The selected delivery specs must converge on this lifecycle:

```text
Faber package graph
  -> one Radix analysis/lowering result
       -> host artifact plan
       -> optional target-neutral DeviceProgram
       -> backend artifact packet(s)
       -> submission-region and host-call facts
  -> Faber validates requested host/device pair and enabled features
  -> Faber materializes target files and embeds device bytes
  -> Faber links one host support leaf and one selected device support leaf
  -> executable/browser product

startup
  -> verify artifact/ABI/target/capability identity
  -> open backend and load modules
  -> resolve functions or pipelines
  -> allocate and initialize persistent resources
  -> prepare backend-native submission regions

hot path
  -> update only declared inputs
  -> call backend-specific prepared submission symbol
  -> enqueue the region directly
  -> synchronize/read back only at declared observations

shutdown/error
  -> cancel or drain according to the accepted contract
  -> release resources in backend-owned order
  -> preserve the first structured failure
```

### LLVM native materialization

The composite product should be inspectable under a stable output directory,
with exact layout selected by DDPP1. Required artifact classes are:

```text
host LLVM modules
embedded device payload object/module
composite manifest
link manifest
host ABI/support archive
one selected device backend archive
final executable
```

The host LLVM module must omit physical device kernel bodies. It declares
direct backend-specific prepared-submission symbols. The linked backend leaf
loads the embedded MSL/metallib, PTX/cubin policy artifact, or AMDGPU code
object and resolves its native functions at startup.

For AMD, Radix normally emits validated AMDGPU LLVM IR/bitcode plus target and
kernel metadata. Faber invokes the pinned external LLVM/ROCm materializer and
linker to produce the final AMDGPU ELF code object. Hosts only validates,
loads, and executes the embedded final bytes. An in-process Radix materializer
would require an explicit DDCP0/DDPP0 boundary amendment and cannot be selected
by an implementation Hand.

### Artifact identity and payload encoding

DDPP0/DDCP0 replace the current FNV provenance hash with two versioned SHA-256
identities. `content_sha256` covers canonical raw payload bytes only.
`packet_sha256` covers schema version, backend, format, materialization stage,
target identity, `content_sha256`, entrypoint map, and canonical
reflection/requirements. NGAB0's current byte-only `artifact_id` is amended to
the `content_sha256` role; it is not confused with packet identity. Text
payloads are canonical UTF-8. Binary payloads in `fmir-text` use an explicit
binary encoding tag, decoded byte length, and canonical unpadded base64; hashes
cover decoded bytes, never the transport spelling. Legacy FNV backend-artifact
provenance is removed in the coordinated schema migration unless DDPP0 finds a
real external contract.

| Identity domain | Authority and inputs | Migration rule |
| --- | --- | --- |
| semantic `device_identity_hash` | Radix; target-neutral `DeviceProgram` semantics, excluding backend blobs | preserve as a separate domain; an algorithm/domain change needs its own major semantic-contract revision |
| artifact `content_sha256` | backend/materializer producer; finalized canonical payload bytes only | replaces only Faber's current FNV backend-artifact provenance and NGAB's byte-only `artifact_id` spelling |
| `packet_sha256` | Radix packet producer or Faber final-materialization producer; versioned artifact metadata plus `content_sha256` | new packet/admission identity; never becomes semantic program identity |
| `execution_descriptor_hash` | Radix typed call/region descriptor | preserve independently; artifact materialization cannot change its call semantics |
| distributed logical/bound-plan hashes | multi-device authority | outside this migration and unchanged by DDPP |
| support-archive ABI/content identity | Faber build plus Hosts-owned support source | versioned ABI plus SHA-256 content receipt; stale last-good archive reuse forbidden |

The NGAB call and identity changes use NGAB0's existing joint interface-packet
authority: the PML campaign Mind and NGAB campaign Mind act together, with the
operator as binding owner for disputes. Because call meaning and manifest
identity change, this is a recorded major packet revision. It updates NGAB0
Partition, ABI, Manifest, Verification, FrozenVsReserved, Versioning, U4/U10,
the U11 fixture, NGAB1 U1/U4 done-when, and NGAB2 materialization/verification
receipts. The one-kernel fixture remains the minimal case, not the ABI limit.
DDCP3/DDPP2 carry the accepted revision.

### Prepared-session shape policy

Prepared regions have a compiler-owned regime/shape-class identity. Native
module, function, pipeline, and argument layouts are fixed for that identity.
Invocation may carry only bounded numeric fields declared by the compiler,
such as active prompt length or dispatch extent; Hosts validates them against
compiled bounds without name lookup or map construction. A session may cache
several precomputed regions keyed by artifact identity, region identity, and
shape class for prefill/decode or other distinct regimes. Cache miss prepares
outside the hot loop or fails closed; it never interprets a kernel.

### Browser/WebGPU materialization

The browser product contains:

```text
CPU host JavaScript or Wasm
WGSL modules
typed compute and/or graphics reflection
binary/static resource payloads
browser product manifest
thin WebGPU host assets
```

WebGPU remains the mandatory browser platform API. Directness means no extra
Faber runtime/provider dispatch around command encoding and queue submission.

## Ground Truth Researched

| Fact | Live authority | Campaign treatment |
| --- | --- | --- |
| Faber exposes Radix `hir-*`/`mir-*` features but still has unconditional GPU/runtime/Hosts dependencies and modules | `Cargo.toml`, `src/package/mod.rs`, `AGENTS.md` | DDPP0/DDPP1 implement and prove product-side feature isolation |
| `llvm-host` emits and links CPU LLVM modules with a runtime archive | `src/package/llvm.rs`, `src/package/llvm_host.rs` | extend; do not replace with Rust codegen |
| Faber device packaging emits one typed program plus Metal/CUDA artifacts | `src/package/device/section.rs` | generalize into artifact packet |
| Current device run performs load/allocate/copy/launch/sync/readback/release | `src/package/device/run.rs` | proof baseline, not optimized hot path |
| Native Metal/CUDA sessions are in Hosts | `hosts/macos-arm64/src/{metal_host,cuda_host}.rs` | extract concrete backend leaves |
| Current native launch path synchronizes too often and resolves launch data during execution | Hosts launch/session source | first performance correction |
| Browser WebGPU already creates resources and submits command encoders directly | `hosts/webgpu-browser/public/src/backend/webgpu-runtime.js` | preserve and make sessions persistent |
| NGAB0 accepted one host call -> one kernel, conflicting with prepared multi-kernel regions | child NGAB contract and fixture | DDPP0/DDCP0 amend NGAB0 and NGAB1 before implementation |
| Gradus PML owns future inference semantics | Gradus production ML campaign and GI4 ownership amendment | hard dependency for ML capstones only |

Source snapshot refreshed during audit: Faber `a71c163e2bbb`, Radix
`749a17ab4ac4`, faber-runtime `10d48ea47435`, Hosts `e066ee0ae98a`,
Gradus `6d45e32fab45`, Triga `e6394b30f3ba`, examples `aad199ecf07c`.
DDPP0 must refresh revisions and dirty-state classification. This draft did
not alter foreign Radix Stage 7 planning, Gradus math/tensor sources and proof,
or examples oracle caches.

## Related Campaign Routing

| Artifact | Relationship | Rule |
| --- | --- | --- |
| [Direct Device Compilation Pipeline](../../../../radix/docs/factory/direct-device-compilation-pipeline/CAMPAIGN.md) | paired compiler authority | Radix contract stages precede dependent product stages |
| [MIR Swarm](../../../../radix/docs/factory/mir-swarm/CAMPAIGN.md) | shared MIR single-writer authority | DDCP implementation units touching shared MIR/LLVM/WGSL surfaces are filed through its member lanes |
| [Faber MIR v1](../../../../radix/docs/factory/faber-mir-v1/CAMPAIGN.md) | active release and GPU product-policy roadmap | DDPP0/DDCP0 reconcile Metal/CUDA/WebGPU posture before conflicting implementation |
| [Native GPU Application Bundle](../native-gpu-application-bundle/CAMPAIGN.md) | child LLVM composite vertical slice | DDPP0 amends NGAB0's one-call/one-kernel ABI; NGAB1-NGAB4 then lower only as DDPP3 child packets, NGAB5 is a DDPP7 capstone, and NGAB6-NGAB7 feed DDPP8 |
| [Target build pipelines](../target-build-pipelines/goal.md) | broader product-build predecessor | absorb its GPU/LLVM packaging clauses into DDPP1; keep unrelated target-build work independent |
| [Inference session boundary](../inference-session-boundary/goal.md) | historical session/artifact handoff | preserve valid metadata boundaries; supersede runtime/model/serving ownership through DDPP and Gradus |
| [Core-support runtime contract](../core-support-radix-runtime-contract/goal.md) | completed release/materialization invariant | DDPP8 explicitly amends manifests and support assembly when the old runtime is removed |
| [GPU training lowering](../../../../radix/docs/factory/gpu-training-lowering/CAMPAIGN.md) | Stages 0-3/3R/4/5/5A accepted; Stage 6 exit gate met but final acceptance remains audit/council-owned | consume each fact at its recorded evidence tier; Stage 7 release work aligns with DDPP |
| [GPU inference GGUF](../../../../radix/docs/factory/gpu-inference-gguf/CAMPAIGN.md) | GI0-GI2 accepted; GI3-1..5 landed and GI3-6/7/8 pending | consume each GI3 unit at its recorded tier; GI4+ behavior follows ownership amendment |
| [GPU inference multi-device](../../../../radix/docs/factory/gpu-inference-multi-device/CAMPAIGN.md) | downstream coordinator | single-device direct leaves precede physical multi-device binding; do not put coordination in the leaf ABI |
| [LLVM host parity](../../../../radix/docs/factory/llvm-host-parity/CAMPAIGN.md) | CPU host semantic dependency | route only blocking CPU/ABI gaps back; direct-device work does not wait for unrelated zero-gap parity |
| [Gradus production ML](../../../../gradus/docs/factory/production-ml-library/CAMPAIGN.md) | ML semantic dependency | inference needs PML1/PML2/PML3/PML5; training needs PML1/PML3/PML4; neither gates generic device infrastructure |
| [Triga graphics MIR stages](../../../../triga/docs/factory/triga-threejs-80/goals/04-graphics-mir-shader-stages.md) | graphics semantic/reflection dependency | graphics source facts remain Triga/Radix-owned; WebGPU product host remains Hosts-owned |
| RunPod/provider verification | optional paid hardware evidence | explicit authorization required; never a local implementation prerequisite |

## Batching And Split Policy

- **DDPP0-DDPP1: discovery-first.** Freeze one joint packet/build contract and
  prove one materialization pattern before batch conversion.
- **DDPP2: split-on-boundary.** Split Metal, CUDA, and later AMD by backend
  leaf because toolchains and failure domains differ. Share only an accepted
  lifecycle/ABI contract.
- **DDPP3-DDPP4: batch-by-default after one vertical slice.** Once one embedded
  native artifact and prepared submission symbol work, apply the same product
  pattern to the second native host lane/backend.
- **DDPP5-DDPP6: split-on-boundary.** Native AMD and browser WebGPU have
  different APIs, artifact formats, and validation environments.
- **DDPP7-DDPP8: split by owner and migration gate.** Gradus semantics, Hosts
  execution, Faber packaging, generated-Rust support, CI, and release changes
  land in repo-local units. Delete old owners only after named consumers pass.
- Never split artifact identity from its payload, entrypoint map, target
  requirements, and validation receipt.
- Never combine a generic native execution path with serving, deployment, or
  distributed scheduling because both happen to use a GPU.

## Campaign Path

### DDPP0 — Joint product, ownership, and performance contract

**Status**: delivered — DDPP0 phase closed (closeout 2026-08-08); DDPP1 next after DDCP2 contract
**Posture**: discovery-first
**Owner**: Faber planner, paired with the Radix DDCP0 planner
**Sources**: this campaign; paired DDCP0; accepted NGAB0 contract; current
Faber, Radix, Hosts, runtime, Gradus, and WebGPU source
**Why now**: later stages would otherwise choose incompatible artifact,
submission, crate, and migration shapes
**Lowers to**: one Faber-owned delivery spec paired with DDCP0; planning only

Freeze:

- the `HostArtifact + DeviceProgram + DeviceArtifact[] + call facts` product
  shape;
- backend/format/target/version/hash identity;
- release single-backend and optional fat-product selection policy;
- prepared-submission and explicit-observation performance invariants;
- one host call -> one prepared submission region containing one or more
  kernels, with the NGAB0 packet, fixture, and NGAB1 delivery amended to match;
- prepared-region regime/shape identity, bounded dynamic invocation fields,
  cache keys, bounds checks, and cache-miss behavior;
- CPU/device partition ownership;
- generated-Rust support destination and final runtime deletion rule;
- exact child-campaign routing and superseded historical clauses;
- exact per-module/runtime-import and per-route destinations, including the
  generated-language carrier contract, ordinary `ad` ABI, Hosts providers,
  LLVM support, device contracts, and inference/oracle code;
- SHA-256 digest domain, text/binary encoding, legacy FNV removal, and
  cross-FMIR/Faber/NGAB/Hosts round-trip fixtures;
- explicit major-revision updates to NGAB0 Partition, ABI, Manifest,
  Verification, FrozenVsReserved, Versioning, U4/U10, its U11 fixture, and
  NGAB1 U1/U4 plus NGAB2 materialization/verification receipts under the joint
  PML/NGAB packet authority;
- LLVM support-archive ABI version and content identity, removal of last-good
  archive reuse, and fail-closed rebuild behavior;
- exact product features and dependency/module gates for a small Rust-only
  build;
- clean-install/core-support/CI/release implications.

**Gate**:

- paired Faber and Radix contracts agree field-by-field and name one ABI/version
  authority;
- the amended NGAB0 packet, fixture, NGAB1 delivery, DDCP3, and DDPP2 all use
  the same prepared-region call granularity;
- NGAB manifest/verification/delivery receipts agree with DDCP/DDPP on
  semantic, content, packet, descriptor, and support-archive identity domains;
- no open decision can change artifact payload encoding, host/device ownership,
  submission granularity, or deletion order;
- DDCP1's implementation delivery is READY and DDPP1's post-DDCP2 delivery
  route, owner, fixture, and done oracle are identified;
- every current runtime import and route has exactly one destination, with
  ordinary `ad` preserved and GPU submission statically excluded;
- current foreign dirt is classified and excluded from packet write sets.

**Notes** (DDPP0-U9 phase closeout, 2026-08-08):

- **Units**: U1–U8 all landed (`evidence/ddpp0-snapshot.md`, `ddpp0-contract.md`
  U2–U4, `ddpp0-runtime-inventory.md` U5, NGAB0-R1 amendment + agreement
  checklist U6, `ddpp0-feature-isolation.md` U7, `ddpp0-support-archive.md`
  U8); commits `f4508b4` (U6, latest) and predecessors. No product code
  written; `faber-runtime/` untouched.
- **DDCP0 agreement gate — CLOSED.** The field-by-field agreement with the
  paired DDCP0 amendment is fully `LANDED`: every agreed field in
  `ddpp0-contract.md` §DDCP0Agreement checklist (region granularity, identity
  domains, canonical encoding, version authority, verification order) cites
  its DDCP0 contract field name, **no field is `PENDING-AGREEMENT`**, and
  §DDCP0Agreement §U9 gate records that nothing blocks this closeout on the
  agreement front. The regime/shape-class enumeration deferral is a recorded
  shared deferral (C5), not a disagreement.
- **Freeze items — satisfied** (each item from the Freeze list above, with
  artifact + section):
  1. Product shape `HostArtifact + DeviceProgram + DeviceArtifact[] + call
     facts` — `ddpp0-contract.md` §ProductShape.
  2. Backend/format/target/version/hash identity — §IdentityDomains (six
     domains, SHA-256).
  3. Release single-backend / fat-product selection policy (C1) — §SelectionPolicy.
  4. Prepared-submission / explicit-observation performance invariants —
     §PerformanceInvariants.
  5. One host call → one prepared submission region (NGAB0 packet, fixture,
     NGAB1 delivery amended) — §PreparedRegion + NGAB0-R1 (`native-gpu-application-bundle/ngab0-*` amendments) + §DDCP0Agreement.
  6. Prepared-region regime/shape identity, bounded dynamic fields, cache
     keys, bounds checks, cache-miss — §PreparedRegion.
  7. CPU/device partition ownership — §PartitionOwnership.
  8. Generated-Rust support destination + final runtime deletion rule —
     §GeneratedRustSupport + §DeletionRule.
  9. Exact child-campaign routing + superseded historical clauses — §ChildRouting.
  10. Per-module/runtime-import and per-route destinations — `ddpp0-runtime-inventory.md` (module-by-module, deletion receipts, TR7 row, PML2 prerequisite, PML0 carriage).
  11. SHA-256 digest domains, canonical text/binary encoding, legacy FNV
      removal, round-trip fixtures — §IdentityDomains + §CanonicalEncoding +
      §FnvRemoval + §RoundTripFixture (worked fixture verifiable by `shasum`).
  12. NGAB0 major-revision updates (Partition/ABI/Manifest/Verification/
      FrozenVsReserved/Versioning, U4/U10, U11 fixture, NGAB1 U1/U4, NGAB2
      receipts) under the joint PML/NGAB authority — NGAB0-R1 amendment
      (DDCP0-U4/U5 `1dc4513`/`36e8c2f`) + §DDCP0Agreement.
  13. LLVM support-archive ABI version + content identity, no last-good reuse,
      fail-closed rebuild — `ddpp0-support-archive.md` + §IdentityDomains row 6.
  14. Exact product features + dependency/module gates for a small Rust-only
      build (C2) — `ddpp0-feature-isolation.md` (DDPP1 gate proof is a PLAN).
  15. Clean-install/core-support/CI/release implications (C3/C4/C5) —
      `ddpp0-support-archive.md` + `ddpp0-runtime-inventory.md` +
      §DeletionRule DDPP8 gate surface.
- **Open Questions — folded or explicitly deferred** (defaults recorded, not
  left to Hands): OQ1 generated-Rust support crate name — default recorded in
  §GeneratedRustSupport (Faber-owned support crate, target-specific name);
  OQ2 fat binaries — deferred in §SelectionPolicy (not a v1 promise);
  OQ3 PTX-vs-cubin — default **PTX** (§SelectionPolicy backend artifact
  defaults); OQ4 MSL-vs-metallib — default **MSL source first**, metallib
  reserved (§SelectionPolicy); OQ5 first AMD API — default **HSA/ROCr**
  (identity stays `amd` + HSA-native; operator gate at DDPP5, §SelectionPolicy);
  OQ6 stable output layout — deferred to DDPP1 by design (layout requirement
  classes only, campaign §Product Build Pipeline + `ddpp0-delivery.md`); OQ7
  external consumers — the external-consumer audit is recorded in §DeletionRule
  rule 1 as a DDPP8 gate (deferred to the deletion stage). No open question can
  change payload encoding, ownership, submission granularity, or deletion order.
- **NGAB0 operator gates — cross-referenced, not silently changed.** §ChildRouting
  rule 1 + §DDCP0Agreement record that NGAB0's operator gates (llvm-host
  identity retained+extended, MSL source-first, PTX arch set) survive the U6
  amendment unchanged; operator decisions stay open at the U12-equivalent
  DDPP boundaries (Metal embedding → DDPP3-level, PTX arch set → NGAB0-U12
  gate, AMD first-leaf → DDPP5).
- **DDPP1 post-DDCP2 route identified.** Delivery route: Faber delivery then
  factory (campaign §DDPP1 "Lowers to"), filed after the DDCP2 contract lands
  (Dependency Rule 3). Owner: Faber package/build planning and implementation
  units (§DDPP1 Owner). Fixture: the §RoundTripFixture text/binary round-trip
  fixture + the DDPP1 gate proof from `ddpp0-feature-isolation.md`
  (`cargo check -p faber --no-default-features --features hir-rust` + `faber
  targets` capability truth). Done oracle: §DDPP1 Gate — text/binary payloads
  round-trip with hashes over canonical bytes; absent target features and
  unsupported host/device pairs fail before build; existing CPU-only Rust,
  FHIR/FMIR, and LLVM products remain deliberately routed or explicitly
  rejected; build planning imports no Hosts driver implementation.
- **Phase gate**: faber docs README regenerated; `check-factory-goal-status`
  exit 0; `git diff --check` clean.

### DDPP1 — Generic product build plan and artifact materialization

**Status**: planned — after DDPP0 and DDCP2 contract
**Posture**: discovery-first, then batch-by-default
**Owner**: Faber package/build planning and implementation units
**Sources**: `src/package/compile.rs`, `src/package/device/section.rs`,
`src/package/{llvm,llvm_host}.rs`, core-support assembler/materializer
**Lowers to**: Faber delivery then factory

Replace Metal/CUDA text assumptions with the accepted raw-byte artifact packet.
Produce an inspectable `CompiledPackage`/build-plan result for host plus device
artifacts. Validate feature availability and host/device pair support before
toolchain work. Extend manifests and inspection output without starting a
device session.

**Gate**:

- text and binary payloads round-trip with hashes over canonical bytes;
- absent target features and unsupported host/device pairs fail before build;
- existing CPU-only Rust, FHIR/FMIR, and LLVM products remain deliberately
  routed or explicitly rejected;
- build planning imports no Hosts driver implementation and reparses no
  emitted target text.
- `cargo check -p faber --no-default-features --features hir-rust` excludes
  GPU emitters, physical Hosts leaves, and device runtime modules, and `faber
  targets` reports matching capability truth.

### DDPP2 — Prepared native backend leaves

**Status**: planned — after DDPP0 and DDCP3 submission contract
**Posture**: split-on-boundary by Metal/CUDA backend
**Owner**: Hosts repo, split into Metal and CUDA leaf units
**Sources**: current Hosts `DeviceSession`, `ProgramSession`, Metal/CUDA drivers
**Lowers to**: separate Hosts repo delivery specs and factory phases

Extract or introduce narrow Metal and CUDA leaves that load modules, resolve
pipelines/functions, allocate persistent buffers, and prepare native submission
regions once. Remove per-launch kernel-name lookup, generic handle-map work,
and launch-local synchronization from the optimized path. Keep the current
proof executor only as a reference until the new path replaces its consumers.

**Gate**:

- a multi-kernel region submits on one Metal command buffer and one CUDA
  stream respectively;
- synchronization/readback occurs only at declared observations;
- module/function/pipeline preparation is measured once per session;
- no `ad`, frame, provider, `Valor`, or generic tensor path is reachable from
  the prepared launch symbol;
- failures preserve fail-closed backend identity and deterministic teardown.

### DDPP3 — LLVM composite native executable

**Status**: planned — after DDCP3, DDPP1, and DDPP2
**Posture**: route through NGAB1-NGAB4; batch after first backend
**Owner**: Faber product assembly plus Hosts leaf units through child NGAB packets
**Sources**: child NGAB campaign and delivery specs; `llvm-host` builder
**Lowers to**: continue NGAB delivery/factory, not a duplicate stage family

Build one LLVM CPU executable containing the selected device artifact, static
metadata, CPU host support, and one backend support leaf. The CPU module calls
backend-specific prepared-submission symbols. Prove CUDA and Metal with the
same target-neutral program and no external kernel sidecar.

The LLVM support archive carries an explicit ABI version and content digest.
Faber rebuilds it from the selected support sources and fails closed on a
rebuild failure or identity mismatch. It must not silently reuse a last-good
archive from `faber-runtime/hosts/llvm`.

**Gate**:

- one source package produces one inspectable executable;
- device kernels are absent from the native CPU module and present in the
  embedded backend artifact;
- startup verifies and prepares once; hot path submits directly;
- same-artifact hashes survive Faber materialization and host loading;
- support-archive ABI/content identity is recorded and stale archive fallback
  is unreachable;
- multi-kernel persistent-state proof passes on real Metal and CUDA hardware.

### DDPP4 — HIR-Rust host parity over the same device packet

**Status**: planned — after DDPP1-DDPP2 and accepted LLVM vertical slice
**Posture**: batch-by-default
**Owner**: Faber generated-package assembly plus Hosts leaf units
**Sources**: generated Rust package builder and minimal Rust support plan
**Lowers to**: Faber and Hosts repo-owned deliveries

Make the primary HIR-Rust application lane consume the same compiled device
packet and prepared backend leaves. Do not make HIR-Rust the canonical device
lowering path and do not add GPU behavior to the Rust carrier crate.

**Gate**:

- Rust-host and LLVM-host products use byte-identical device artifacts for the
  same package/backend/target request;
- both use the same prepared submission/observation contract;
- generated Rust carrier support contains no driver, device discovery,
  physical session, model, or inference implementation.

### DDPP5 — Native AMD product backend

**Status**: planned — after DDCP5 artifact emitter and DDPP2 leaf pattern
**Posture**: discovery-first for one provider-relevant target, then batch
**Owner**: Faber materialization/packaging and Hosts AMD leaf units
**Sources**: paired AMD compiler stage; Hosts backend pattern; provider
conformance plan
**Lowers to**: Faber packaging delivery plus Hosts AMD-HSA delivery

Invoke the pinned Faber-owned external materializer on Radix-produced AMDGPU
LLVM IR/bitcode and metadata. Package the resulting code object with explicit
target identity and link a direct AMD host leaf. Select one
infrastructure-relevant target row for the first proof. Do not translate PTX,
use WebGPU as native AMD execution, or treat HIP as the product backend
identity.

**Gate**:

- explicit AMD selection fails before launch on missing/incompatible artifact
  or hardware;
- the materialization receipt identifies compiler-input hash, pinned
  toolchain, target, final byte hash, and package embedding;
- code object is loaded from embedded canonical bytes;
- native functions and argument layouts resolve once;
- a persistent multi-kernel region runs with observation-only synchronization;
- hardware/provider execution requires a separately authorized receipt.

### DDPP6 — WebGPU compute and graphics product composition

**Status**: planned — after DDCP6 contract and DDPP1 artifact envelope
**Posture**: split-on-boundary between compute and graphics
**Owner**: Faber browser packaging and Hosts WebGPU units; compiler/source gaps remain Radix/Triga-owned
**Sources**: `hosts/webgpu-browser`, Faber browser product builder, Triga
shader/graphics goals
**Lowers to**: Faber and Hosts browser delivery specs; Triga/Radix gaps remain
in their owning campaigns

Use the same outer artifact envelope for WGSL and reflection while retaining
separate compute and graphics execution contracts. Prepare persistent browser
resources and cross Wasm/JavaScript at compiled submission/pass boundaries.

**Gate**:

- browser host consumes reflection rather than parsing WGSL or Triga names;
- compute chains use persistent buffers/pipelines and intentional readback;
- graphics uses typed stages, layouts, attachments, render passes, and
  presentation without being forced through the compute executor;
- direct WebGPU command encoding remains visible in the host hot path;
- current source-to-graphics-MIR gaps are reported, not hidden by host facts.

### DDPP7 — Gradus inference and training device integration proofs

**Status**: planned — after DDPP3/DDPP4 and workload-specific Gradus PML stages
**Posture**: batch-by-default by accepted compiled workload family
**Owner**: repo-local Gradus, Radix, Faber, and Hosts units coordinated by DDPP
**Sources**: Gradus production ML campaign; child NGAB5; accepted Radix
training/inference numeric policies
**Lowers to**: NGAB5 and repo-owned performance/evidence deliveries

Run compiler-generated forward/backward and inference regions through the same
direct device product path. Gradus supplies semantics; Radix supplies device
programs; Faber packages; Hosts executes. Serving remains out of scope.

Inference and training lower as distinct fixtures and gates. Inference waits
for accepted PML1/PML2/PML3/PML5 behavior. Training waits for accepted
PML1/PML3/PML4 optimizer, checkpoint, deterministic-seed, and training-layer
behavior. Acceptance of one workload family does not promote the other.
DDPP7 emits the native device-integration receipt consumed by Gradus PML7. It
does not claim the final Gradus release capstone, which also requires PML6,
examples, documentation, and release-owned gates.

**Gate**:

- model/parameter/KV/optimizer state remains resident across repeated steps;
- launch, transfer, synchronization, readback, and preparation counts are
  recorded from the real executable;
- no runtime tape or runtime model implementation owns the product path;
- correctness matches accepted independent oracles;
- performance evidence reports end-to-end tokens or training steps per second,
  not kernel-only timing.

### DDPP8 — Support decomposition, old-runtime deletion, qualification, release

**Status**: planned — after replacement consumers pass
**Posture**: split by repo owner and migration gate; clean-break closeout
**Owner**: repo-local migration owners; Faber controls product qualification and deletion gate
**Sources**: DDPP0 module inventory; core-support; CI/release workflows;
NGAB6-NGAB7
**Lowers to**: separate Faber, faber-runtime, Hosts, Gradus, Radix, Triga,
examples, and release delivery specs

Move each remaining live module to its accepted owner, delete obsolete
production oracles and fake physical-device claims, update generated package
dependencies and support archives, remove the old sibling checkout, and delete
the `faber-runtime` repository/package after an external-consumer audit.

**Gate**:

- no production manifest, source import, build script, support archive, CI
  checkout, host crate, or documentation authority depends on
  `faber-runtime`;
- `src/package/llvm_host.rs` contains no last-good/stale runtime-archive
  fallback and every linked support archive has verified ABI/content identity;
- no compatibility/forwarding crate preserves the old ownership surface;
- all supported host/device feature combinations are represented honestly in
  `faber targets` and release artifacts;
- clean-install native and browser capstones pass at the approved release
  boundary;
- release/version checkpoint is either completed or explicitly deferred with
  owner and reason.

## Stage Dependency Graph

```text
DDCP0 <-> DDPP0
  +-> DDCP1 ----+
  +-> DDCP2 ----+-> DDCP3
         |            |
         +-> DDPP1 ---+-> DDPP2 -> DDPP3 -> DDPP4

DDCP5 + DDPP1/DDPP2 ----------------------> DDPP5
DDCP6 + DDPP1 + Triga/WebGPU -------------> DDPP6
Gradus PML1/PML2/PML3/PML5 + DDPP3/DDPP4 -> DDPP7 inference
Gradus PML1/PML3/PML4 + DDPP3/DDPP4 ------> DDPP7 training
replacement consumers + DDPP7 ------------> DDPP8
DDPP7 + Gradus PML6/examples --------------> Gradus PML7 final capstone
```

DDCP numbering is authoritative in the paired Radix campaign. A downstream
session must refresh this graph if that campaign changes stage order.

## First Useful Milestones

1. One accepted paired artifact and prepared-submission contract.
2. One multi-kernel Metal/CUDA proof with no per-kernel synchronization.
3. One `llvm-host` executable containing and directly launching its device
   artifact.
4. Rust-host and LLVM-host parity over the same device bytes.
5. One native AMD code-object proof.
6. One persistent WebGPU compute chain and one typed graphics pass.
7. Gradus inference/training device-integration receipts with residency and
   performance evidence, ready for the later PML7 release capstone.
8. Deletion of the old universal runtime.

## Acceptance Criteria For This Campaign Artifact

This draft is ready for downstream lowering when:

- the paired Radix campaign exists and both select Stage 0;
- each stage names source authority, owner, gate, posture, and downstream
  lowering route;
- NGAB, Gradus, inference, training, multi-device, LLVM parity, and Triga
  overlaps have one explicit authority;
- the hot-path exclusions and runtime deletion target are unambiguous;
- the dependency graph prevents product work from inventing compiler facts;
- paid hardware, releases, repository deletion, and external-consumer impacts
  have stop conditions.

Campaign acceptance does not prove implementation. Each stage must be lowered
through `delivery`, then executed and verified through `factory` or the
repo-specific equivalent.

## Validation

Artifact-level checks for edits to this campaign:

```bash
cd /Users/ianzepp/work/faberlang/faber
../radix/scripta/generate-factory-readme.py --factory-root docs/factory
./scripta/check-factory-goal-status
../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check
git diff --check
```

Downstream delivery specs must name narrow checks for their touched repos.
Real Metal, CUDA, AMD, WebGPU, full product, release, and paid-provider runs are
auditor/operator gates, not campaign-draft validation.

## Open Questions Routed To DDPP0

1. Final name and home of the generated-Rust support crate.
2. Whether the public product supports fat native binaries or only explicit
   single-backend release builds in the first version.
3. PTX versus cubin policy and the permitted CUDA startup toolchain.
4. MSL source versus metallib policy for clean-install products.
5. First AMD product API leaf: HIP module API or lower-level HSA/ROCr.
6. Stable output layout and whether embedded payloads use generated LLVM,
   object sections, or another inspectable linker input.
7. External consumers, if any, that make deletion/renaming of the published
   `faber-runtime` crate a real compatibility event.

These questions must not be answered ad hoc by implementation Hands.

## Stop Conditions

Pause lowering or implementation when:

- the paired Radix and Faber packet schemas disagree;
- a stage requires reconstructing facts from emitted source or binary text;
- direct execution would pass through `ad`, Sermo, `Valor`, or provider routing;
- the proposed hot path requires per-kernel synchronization or undeclared
  readback;
- a generic abstraction erases backend-specific memory, queue, target, or
  graphics requirements;
- an owner would have to duplicate Gradus semantics, Radix compiler facts, or
  Hosts physical behavior;
- runtime code is moved before a replacement consumer and deletion gate exist;
- work would overwrite foreign dirt or cross repo-owned write sets;
- a paid provider, deployment, release, external repository deletion, or
  compatibility promise needs operator authorization.
