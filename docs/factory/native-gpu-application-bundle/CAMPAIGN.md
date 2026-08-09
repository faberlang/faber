# Campaign: Native GPU Application Bundle

**Status**: active — NGAB0 accepted (2026-08-08, NGAB0-U12 closeout); NGAB1–NGAB7 planned — after NGAB0
**Created**: 2026-08-08
**Mode**: routing artifact — draft/maintain; does not implement code directly
**Control-plane repo**: `/Users/ianzepp/work/faberlang/faber`
**Implementation repos**: `faber` for product build/run and artifact assembly;
`radix` for lowering, device-program facts, and GPU emission; `hosts` for
native device execution; `examples` for generic and LLM capstones
**Sibling campaign**:
[`gradus/docs/factory/production-ml-library`](../../../../gradus/docs/factory/production-ml-library/CAMPAIGN.md)
**Consumes**: live `llvm-host`, FMIR device sections, Metal/CUDA host sessions,
and accepted GPU-inference GI3 compiler evidence
**Lowers to**: `delivery` then `factory`
**Campaign readiness**: **NGAB0 ACCEPTED (2026-08-08) — NGAB1–NGAB7 planned**

## Summary

Make one Faber-written application compile through the LLVM host path into one
native executable that carries its compiler-produced GPU kernels and opens
the required Metal or CUDA host internally. The first LLM-shaped capstone will
load model data through Gradus, keep inference state alive, invoke embedded
kernels, and run without asking the user to separately compile, locate, or
launch a kernel artifact.

This campaign builds application infrastructure. It does not put an inference
server inside Faber, Radix, or Gradus. A later inference product repository
will consume the executable path.

## Problem

The two required halves already exist separately:

- `faber build/run --target llvm-host` produces and executes a native
  MIR-to-LLVM host binary.
- FMIR device images carry compiler-produced MSL and PTX and execute through
  ordinary `faber run --backend metal|cuda` host sessions.

There is no accepted product contract for one `llvm-host` executable that
contains those device artifacts, selects an admitted backend, opens a
persistent device session, and dispatches kernels while ordinary Faber host
code continues to own CLI, files, networking, and control flow. Treating the
RunPod kernel bundle as the application would also be wrong: a cloud GPU needs
the same executable, runtime/driver dependencies, model data, configuration,
and process lifecycle as a local machine.

## Desired End State

1. A Faber package can contain ordinary host functions and explicit device
   computation in one checked program.
2. Radix derives host LLVM modules and versioned device-program artifacts from
   one analyzed package without parsing emitted text to reconstruct facts.
3. Faber builds one inspectable native application artifact with embedded MSL,
   metallib where applicable, and/or PTX variants plus their manifests.
4. The executable verifies embedded artifact identity, chooses an admitted
   local backend, opens a persistent session, dispatches kernels, observes
   declared results, and tears down correctly on success or failure.
5. Unsupported hardware, driver, artifact version, architecture, dtype,
   quantization, or kernel capability fails closed without CPU fallback.
6. A generic host-plus-device fixture and an LLM-shaped Gradus consumer run
   through the same product command locally.
7. The CUDA artifact runs unchanged inside a pinned Linux/RunPod-compatible
   container. Provisioning and public service deployment remain separate.
8. Multi-device extension consumes the existing distributed campaign rather
   than hiding placement policy in this bundle format.

```text
Faber application source
  -> Radix analyzed package
       -> host MIR -> LLVM host modules
       -> device program -> MSL/metallib and NVVM/PTX
  -> Faber composite build and link manifest
  -> one native executable
  -> embedded-artifact verification and host session
  -> Metal or CUDA execution
```

## Development Posture

- **Composition, not reinvention.** `llvm-host` and device-image emission are
  live baselines. This campaign joins them; it does not recreate either path.
- **One executable means self-describing code, not embedded model weights.**
  Kernels and manifests travel in the application binary. Large model files
  remain explicit application inputs unless a product requests another mode.
- **Faber owns product workflow.** Build plans, external tool invocation,
  final layout, manifests, and `build/run` UX stay in Faber.
- **Radix owns compiler facts.** Host/device partitioning, validated MIR,
  kernel identity, resource semantics, and backend emission stay in Radix.
- **Hosts own effects.** Driver discovery, module loading, physical buffers,
  dispatch, synchronization, observation, and teardown stay in hosts.
- **Gradus stays device neutral.** The sibling library supplies model,
  tokenizer, transformer, decode, cache, and sampling semantics without
  receiving a backend handle.
- **Fail closed.** No selected GPU path silently falls back to Rust, CPU, a
  subprocess compiler, `llama.cpp`, or a separately installed kernel.

## Implementation Workflow

1. Lower each NGAB stage through `delivery` before implementation.
2. Route every unit to its owning repo; no cross-repo grab bag commit.
3. Prove the generic composite artifact before the LLM capstone.
4. Record exact compiler, Faber, host, Gradus, OS, driver, device, and artifact
   identities in cross-backend receipts.
5. Use the RunPod verification campaign only after local CUDA artifact
   identity and container prerequisites are frozen. Paid runs require explicit
   authorization.
6. Update this campaign and each affected factory status at stage boundaries.

## Scope Routing

### In campaign

- Host/device partition and call/entry contracts.
- Versioned composite application and embedded-device-artifact manifest.
- Faber build plan, LLVM host linking, resource embedding, and inspection.
- Metal and CUDA backend variants, capability admission, and selection.
- Native host bootstrap and persistent `ProgramSession` composition.
- Copy-in, dispatch, synchronization, observation, error, cancellation, and
  teardown semantics required by a native application.
- Local macOS/Metal and Linux/CUDA execution receipts.
- A container-portable CUDA artifact receipt on explicitly authorized
  hardware.
- Generic and Gradus-backed LLM-shaped capstones.

### Split out

- Model semantics, format parsing, tokenization, transformer composition,
  decode, logical KV cache, and sampling -> Gradus production ML campaign.
- HTTP/OpenAI API, request scheduling, continuous batching, authentication,
  observability, deployment, and autoscaling -> separate inference product.
- General LLVM-host corpus parity -> existing `llvm-host-parity` campaign.
- New decoder operation and kernel families -> existing/re-lowered GPU
  inference campaign, with Radix ownership only.
- Distributed placement, tensor/pipeline parallelism, collective transport,
  and failure transactions -> existing multi-device campaign.
- RunPod provisioning, billing controls, ingress, secrets, and teardown ->
  RunPod/deployment campaign.

## Batching And Split Policy

- **NGAB0-NGAB1: discovery-first.** Freeze artifact and call contracts, then
  prove one scalar host function calling one device kernel.
- **NGAB2-NGAB4: split-on-boundary.** Split by compiler facts, product
  packaging, host ABI, backend, or independent failure domain. Batch artifact
  families after the first accepted embedding and loader pattern.
- **NGAB5-NGAB7: batch-by-default.** Reuse the accepted product path for
  Gradus, portability, documentation, and release evidence.
- Never split version identity, kernel/resource binding, host/device call
  semantics, or same-artifact evidence across independent units.

## Ground Truth Researched

| Fact | Authority | Treatment |
| --- | --- | --- |
| `llvm-host` currently reports build/run/package yes | live `faber targets` and `src/package/llvm_host.rs` | Baseline; do not re-plan |
| `llvm-host` emits inspectable modules, link manifest, runtime identity, and native binary | `src/package/llvm_host.rs`, target capability matrix | Extend with a composite device section |
| Faber device images already carry MSL and PTX | `src/package/mir/image.rs`, `src/package/device/section.rs` | Reuse schema/provenance patterns |
| Device execution is currently selected by `faber run --backend` | `src/package/device/run.rs`, live target matrix | Compose into the application artifact |
| `DeviceProgram` is target-neutral and typed | Radix MIR/device program and Faber wire code | Compiler source of truth |
| GI3 proves selected LLM prefill kernels and GPU/oracle comparison | `radix/docs/factory/gpu-inference-gguf` | Consume evidence; do not inherit stale runtime ownership |
| RunPod currently proves infrastructure and small CUDA artifacts | `radix/docs/factory/runpod-gpu-verification` | Later portability receipt only |
| Gradus is device-neutral ML computation | sibling production ML campaign | LLM semantic dependency |

Authority order: live source/tests and live `faber targets`; accepted artifact
schemas and hardware receipts; this campaign's frozen contracts; historical
campaign prose.

Source snapshot used for this draft: Faber `26b503a0e3bb`, Radix
`a01543b06bfe`, hosts `e066ee0ae98a`, Gradus `29d26735d0d9`, Norma
`84f27dacd6f9`, and examples `aad199ecf07c`. NGAB0 must refresh these revisions,
record dirty state, and replace drifted claims before NGAB1 lowers.

## Related Campaign And Goal Dependency Ledger

This is the cross-repository handoff inventory for the later priority and
ordering session.

| Related artifact | Live state on 2026-08-08 | What this campaign uses or supplies | Stage edge | Routing disposition |
| --- | --- | --- | --- | --- |
| [`Gradus production ML library`](../../../../gradus/docs/factory/production-ml-library/CAMPAIGN.md) | proposed; PML0 selected | model formats, tokenizer, forward model, logical KV cache, decode, and sampling | PML0 <-> NGAB0; PML2/PML3/PML5 -> NGAB5 | Hard semantic dependency for LLM capstone; not for generic NGAB1-NGAB4 |
| [`gradus-ml-foundation`](../../../../gradus/docs/factory/gradus-ml-foundation/GOAL.md) | Horizon 0 architecture checkpoint complete | JAX-shaped/device-neutral Gradus architecture and nanoGPT history | predecessor to Gradus PML campaign | Consume through PML; do not use as parallel production control plane |
| [`gpu-training-lowering`](../../../../radix/docs/factory/gpu-training-lowering/CAMPAIGN.md) | active; accepted single-device Metal/CUDA training substrate through Stage 6 evidence | typed device programs, persistent sessions, emitters, cross-vendor receipts | supplies NGAB0-NGAB4 | Continue independently; reuse accepted contracts |
| [`gpu-inference-gguf`](../../../../radix/docs/factory/gpu-inference-gguf/CAMPAIGN.md) | active; GI3 compiler/prefill work in flight | pinned model/oracle contracts and selected LLM kernel evidence | supplies Gradus PML2/PML5 and NGAB5 | Consume GI0-GI3; re-lower GI4+ because model runtime moves to Gradus and serving moves to a product repo |
| [`gpu-inference-multi-device`](../../../../radix/docs/factory/gpu-inference-multi-device/CAMPAIGN.md) | active; MD3 partially accepted, real physical multi-device proof pending | distributed device graph, placement, transaction, and future inference binding | NGAB4 + Gradus PML5 -> later MD stages | Downstream; not a blocker for NGAB0-NGAB5 |
| [`llvm-host-parity`](../../../../radix/docs/factory/llvm-host-parity/CAMPAIGN.md) | in factory; broad semantic gaps remain, live `llvm-host` product path exists | host LLVM ABI, runtime archive, link manifest, build/run product | direct prerequisite to NGAB0-NGAB3 | Reuse current path; route only LLM-blocking carrier gaps back to its stage graph |
| [`mir-gpu`](../../../../radix/docs/factory/mir-gpu/CAMPAIGN.md) | active compiler architecture track | device-stage facts and GPU emitter ownership | prerequisite to NGAB1-NGAB2 | Continue as Radix implementation authority |
| [`mir-llvm`](../../../../radix/docs/factory/mir-llvm/CAMPAIGN.md) | parked historical authority | LLVM/NVVM architecture context | research input to NGAB1 | Do not revive unless live source lacks an owner |
| [`target-build-pipelines`](../target-build-pipelines/goal.md) | proposed; partially stale after live `llvm-host` delivery | Faber build-plan, artifact-layout, and fail-closed target concepts | NGAB0/NGAB2 reconciliation input | Rebaseline and absorb current parts rather than execute raw |
| [`inference-session-boundary`](../inference-session-boundary/goal.md) | proposed metadata/session boundary, no runtime | earlier CLI/session and model-handoff decisions | NGAB0 and later inference-product input | Split: artifact handoff may survive; serving/session product ownership moves out |
| [`mir-library-imports`](../mir-library-imports/goal.md) | implemented | linked `gradus:*` package calls through FMIR | prerequisite already satisfied for source-level capstones | Consume; no new work here |
| [`runpod-gpu-verification`](../../../../radix/docs/factory/runpod-gpu-verification/goal.md) | active; harness/card evidence exists, same-artifact run pending | controlled ephemeral CUDA execution and receipts | NGAB5 local CUDA -> NGAB6 | Downstream and paid-operation authorization-gated |
| [`gpu-workload-floor`](../../../../radix/docs/factory/gpu-workload-floor/goal.md) | active measurement track | workload rungs and honest capability floors | evidence input to NGAB4/NGAB7 | Continue as measurement authority |
| [`agent-native-device-runtime`](../../../../radix/docs/factory/agent-native-device-runtime/goal.md) | planned pre-implementation | alternative resident device-process proof | may consume NGAB3 later | Downstream research; not a prerequisite |
| [`pytorch-session-continuation`](../pytorch-session-continuation/goal.md) | proposed proof-era training continuation | older training gaps now owned by Gradus PML4 | no direct NGAB dependency | Re-lower into Gradus or retire |
| Inference product campaign *(not yet drafted)* | missing | separate application repository for CLI/server, public request and tuning API, scheduling, streaming, observability, and local operation | product shell may start after PML0/NGAB0; model execution depends on PML5 + NGAB5 | Required third campaign; NGAB supplies its binary path but does not implement it |
| RunPod deployment campaign *(not yet drafted)* | missing | container publication, provisioning, ingress, secrets, health, autoscaling, receipts, and teardown | consumes NGAB6 plus inference product | Required for an operated RunPod service, not portability proof |

### Cross-campaign ordering constraints

```text
PML0  <->  NGAB0                 shared interface freeze; run in parallel
  |          |
PML1       NGAB1 -> NGAB2 -> NGAB3
  |                         |
PML2 + PML3              NGAB4 generic composite proof
  |          \              |
PML4         PML5 --------> NGAB5 LLM executable
  |            |              |
PML6 ---------+-----------> NGAB6 portability
  \-----------------------> PML7 + NGAB7 closeout

PML5 + NGAB4 -> multi-device continuation (separate priority lane)
```

The generic compiler/package vertical slice NGAB0-NGAB4 can advance while
Gradus production work advances. NGAB5 is the convergence point. RunPod and
multi-device work follow the local single-device proof.

## Current State

| Track | State | Next action |
| --- | --- | --- |
| LLVM host executable | Live product path | Reuse unchanged where possible |
| Device artifacts | MSL/PTX carried by FMIR device image | NGAB0 define composite application envelope |
| Host/device composition | Separate run modes | NGAB1 freeze compiler and call boundary |
| Faber packaging | LLVM and device build plans separate | NGAB2 build one composite artifact |
| Native host | Persistent device sessions exist outside llvm-host app | NGAB3 compose bootstrap and lifecycle |
| Cross-vendor proof | Separate Metal/CUDA device receipts | NGAB4 same-source application receipts |
| LLM application | GPU prefill proof, no standalone binary | NGAB5 Gradus-backed executable capstone |
| Cloud portability | RunPod harness, not application packaging | NGAB6 same-artifact container receipt |
| Release truth | No composite support row | NGAB7 qualification and release decision |

## Campaign Path

### NGAB0 — Composite artifact and ownership contract

**Status**: accepted — NGAB0-U12 phase closeout (2026-08-08)
**Owner**: Faber; Radix owns compiler inputs, hosts own execution inputs, and
Gradus owns the paired semantic interface.
**Source**: this campaign, live Faber LLVM/device builders, Radix device schema,
hosts session contracts, and Gradus PML0 interface packet
**Gate**: accepted package graph; host/device partition; entry/call ABI;
versioned embedded-artifact manifest; resource identity; backend variants;
artifact layout; build/run UX; error taxonomy; ownership matrix; one generic
fixture; and stale-plan reconciliation.
**Required outputs**:
`docs/factory/native-gpu-application-bundle/ngab0-composite-contract.md`, the
Gradus `pml0-gradus-contract.md`, and a committed ownership amendment plus
migration map in `radix/docs/factory/gpu-inference-gguf/`. NGAB0 cannot close
while GI4+ remains a competing runtime/server control plane.
**Batch posture**: discovery-first.
**Lowers to**: `delivery` then `factory`.

**Notes** (NGAB0-U12 phase closeout, 2026-08-08):

- **Units**: U1–U11 all landed (snapshot, packet sections U2–U7, radix
  ownership amendment U8, claim register U9, receipt schema + audit
  entrypoint U10, fixture contract U11). Required outputs delivered:
  `ngab0-composite-contract.md` (frozen packet, U2–U7),
  `ngab0-receipt-schema.md` + `scripta/check-factory-goal-status` (U10),
  `ngab0-claim-register.md` (U9), `ngab0-fixture-contract.md` (U11),
  `evidence/ngab0-snapshot.md` (U1), and the committed gguf ownership
  amendment `radix/docs/factory/gpu-inference-gguf/gi4-ownership-amendment.md`
  + MD3I gate amendment (U8). The Gradus `pml0-gradus-contract.md` exchange is
  confirmed via the paired PML0 packet (U2 §OwnershipMatrix + §PackageGraph
  cite `pml0-gradus-contract.md`; U8 re-lowers the GI4+ authority under
  Gradus PML5 + NGAB composite session facts).
- **Operator open questions — DEFERRED with recorded defaults** (per
  `ngab0-delivery.md` §Open Questions; defaults recorded in
  `ngab0-composite-contract.md` §Admission "Operator decision gates"): (1)
  **llvm-host identity** — retain the stable user-facing selector `llvm-host`
  and extend its capability with the embedded-device section (broader identity
  is a packet change under the §Versioning procedure); (2) **Metal embedding**
  — MSL source first (matches current FMIR-carried MSL), `metallib` reserved
  until the operator decision closes; (3) **CUDA PTX arch set** — the admitted
  row's arch set, with `ptx_target` carried by the FMIR device section as the
  working target until operator-confirmed. **The phase does not claim the full
  gate while these dangle** — each default holds until the named gate closes
  (operator decision), and each is recorded in the packet §Admission +
  §FrozenVsReserved decision-gate list.
- **C7 gate**: the Faber-scoped audit entrypoint `scripta/check-factory-goal-status`
  (U10, thin wrapper over `radix/scripta/audit-factory-goal-status.py`
  `--factory-root docs/factory`) and the joint receipt schema
  `ngab0-receipt-schema.md` are referenced from the campaign Validation
  section below.

### NGAB1 — Radix host/device partition and callable device boundary

**Status**: accepted — NGAB1-U4 phase closeout (2026-08-09); NGAB2 next after DDCP1/DDCP2 per the DDPP3 absorption + paired DDCP contract
**Owner**: Radix.
**Source**: NGAB0, live analyzed-package/MIR lowering, `DeviceProgram`, FMIR
device schema, and accepted GI3 compiler contracts.
**Gate**: one analyzed package produces validated host MIR/LLVM and a typed
device program; host code invokes the device boundary through a versioned ABI;
resource, lifetime, mutation, and observation facts survive lowering; invalid
cross-boundary values fail at compile time.
**Overlap rule**: extend typed IR/schema before emitters; do not reconstruct
calls from LLVM, MSL, or PTX text.
**Batch posture**: one vertical slice, then batch compatible call shapes.
**Lowers to**: `delivery` then `factory`.

**Notes** (NGAB1-U4 phase closeout, 2026-08-09):

- **Units**: U1–U4 all landed (U1 `0bb5ebbd6`/`754a8e6`, U2 `3f8541bcb`/`93393cd`,
  U3 `3001fdd90`, U4 `cb92b4f11`/`4435068`). **Gate met**: one analyzed package
  produces validated host MIR/LLVM and a typed `DeviceProgram` — the NGAB0-U11
  fixture proves the minimal one-prepared-region vertical slice (U1); host→device
  calls go through the versioned call ABI and invalid cross-boundary values fail
  at compile time with typed diagnostics, proven by a negative fixture without a
  launch (U2); resource identity, lifetimes, mutation, and observation facts
  survive lowering (U3); compatible batch shapes submit as multiple kernels per
  prepared submission region — one host call → one region, never per-kernel ABI
  rows (U4, NGAB0-R1 granularity).
- **Routing**: NGAB1–NGAB4 implement as **DDPP3 child packets** per the DDPP0
  contract §ChildRouting and council H1 (the H1 NGAB1-HOLD was lifted by the
  NGAB0-R1 amendment). **NGAB2 is next after DDCP1/DDCP2** per the paired DDCP
  contract — DDCP1/DDCP2 have READY delivery-sized slices (`ddcp0-closeout.md`),
  and DDPP3's composite-build authority absorbs NGAB1–NGAB4.
- **Carried questions**: the NGAB0 operator gates (llvm-host identity, MSL
  source-first, PTX arch set) remain open as recorded in NGAB0 §Admission; U1–U4
  raised no new unresolved ABI or wire questions.

### NGAB2 — Faber composite build and embedded artifact assembly

**Status**: planned — after NGAB1
**Owner**: Faber.
**Source**: NGAB0/NGAB1, `src/package/llvm_host.rs`,
`src/package/device/section.rs`, and `src/package/mir/image.rs`.
**Gate**: `faber build` produces one native executable and an inspectable build
directory; the binary contains content-addressed admitted GPU artifacts and
manifest; debug/release toolchains are recorded; corrupt, missing, incoherent,
or unsupported variants fail before launch.
**Overlap rule**: Faber orchestrates existing Radix emitters and external
toolchains; it does not own kernel semantics.
**Batch posture**: split by executable/link manifest and backend artifact.
**Lowers to**: `delivery` then `factory`.

### NGAB3 — Native bootstrap and persistent device sessions

**Status**: planned — after NGAB2
**Owner**: hosts for physical lifecycle and Faber for executable integration.
**Source**: NGAB0-NGAB2, live host session/provider contracts, and
`src/package/device/run.rs`.
**Gate**: the executable discovers capabilities, verifies and loads its own
embedded variant, opens one persistent session, performs declared transfers
and launches, exposes observations to host Faber code, and releases resources
on normal exit, error, and cancellation.
**Overlap rule**: hosts own physical effects; generated/application runtime
holds only versioned logical handles and call state.
**Batch posture**: split by lifecycle and backend provider.
**Lowers to**: `delivery` then `factory`.

### NGAB4 — Cross-vendor generic application proof

**Status**: planned — after NGAB3
**Owner**: Faber product path; examples owns the fixture/receipts; Radix and
hosts retain their component boundaries.
**Source**: NGAB1-NGAB3 and accepted Metal/CUDA receipts from
gpu-training-lowering.
**Gate**: one Faber CLI application performs ordinary host work around at
least two kernel calls from the same source; local Metal and CUDA executions
match a CPU oracle; receipts prove embedded artifact provenance, persistent
session reuse, bounded transfers, and clean teardown.
**Batch posture**: batch-by-default across admitted backends.
**Lowers to**: `delivery` then `factory`.

### NGAB5 — Gradus-backed LLM executable capstone

**Status**: planned — after NGAB4 and Gradus PML2/PML3/PML5
**Owner**: Faber executable path and examples capstone; Gradus owns ML
semantics.
**Source**: NGAB0-NGAB4, Gradus PML2/PML3/PML5, and GI0-GI3
model/oracle/kernel evidence.
**Gate**: a Faber application binary loads an admitted external model,
tokenizes input, performs prefill and persistent autoregressive decode through
embedded kernels, maintains logical KV state through Gradus, accepts bounded
tuning parameters (model path, prompt, context length, prompt batch size,
maximum generated tokens, seed, temperature, top-k, top-p, min-p, repetition
penalty, and explicit backend/device selection), and produces oracle-matching
tokens; no server or
`llama.cpp` runtime dependency is involved.
**Overlap rule**: Gradus owns ML semantics; this campaign owns compilation,
embedding, launch, and executable evidence.
**Batch posture**: one admitted model/backend-neutral application, then backend
receipts.
**Lowers to**: `delivery` then `factory`.

### NGAB6 — Linux/CUDA container and RunPod portability

**Status**: planned — after NGAB5 local CUDA passes
**Owner**: Faber for the artifact; the Radix RunPod verification campaign owns
the paid evidence operation.
**Source**: accepted NGAB5 CUDA receipt and runpod-gpu-verification contracts.
**Gate**: the same release artifact or a declared target-triple rebuild runs in
a pinned minimal container with driver-only runtime prerequisites; a locally
replayed receipt passes before one explicitly authorized ephemeral RunPod run;
model/config inputs and teardown are recorded separately from the binary.
**Overlap rule**: this proves portability, not provisioning, ingress, serving,
autoscaling, or multi-node execution.
**Batch posture**: discovery-first locally, then one paid receipt.
**Lowers to**: `delivery` then `factory`, with an authorization stop before
external spend.

### NGAB7 — Qualification, documentation, and release checkpoint

**Status**: planned — final
**Owner**: Faber, with component evidence from Radix, hosts, Gradus, and
examples.
**Source**: accepted NGAB contracts and receipts plus live product support and
release protocols.
**Gate**: target discovery, CLI help, artifact inspection, ABI/schema versions,
support matrix, clean-install build, Metal/CUDA receipts, failure cases,
portability limits, and release notes agree; Faber version impact is decided
and unsupported claims remain false.
**Batch posture**: closeout only.
**Lowers to**: `delivery` then `factory`.

## Dependency Rules

1. NGAB0 and Gradus PML0 exchange one versioned interface packet before either
   campaign generalizes its public boundary.
2. NGAB1 precedes packaging. Faber cannot infer host/device facts from emitted
   text or application naming conventions.
3. NGAB2 precedes host loading. A host must not depend on loose developer-tree
   kernel paths.
4. NGAB4 proves general application composition before LLM complexity enters.
5. NGAB5 waits for admitted Gradus model, forward, decode, cache, and sampling
   contracts; it does not absorb them into Faber or Radix.
6. Existing `gpu-inference-gguf` GI3 compiler evidence is reusable. GI4-GI7
   ownership and product clauses must be re-lowered under the new Gradus and
   separate-application decision before further implementation.
7. Multi-device work consumes this single-device executable contract but does
   not block NGAB0-NGAB5.
8. No external RunPod mutation occurs without fresh operator authorization.

## First Useful Milestones

1. **Contract**: NGAB0 removes ambiguity about the binary and embedded assets.
2. **Real composite binary**: NGAB1-NGAB3 execute one embedded kernel from
   ordinary Faber host code.
3. **Portable application path**: NGAB4 proves the model on Metal and CUDA.
4. **LLM executable**: NGAB5 runs bounded generation from one native program.
5. **Cloud-capable artifact**: NGAB6 proves the CUDA application in a container
   and, when authorized, on RunPod.

## Acceptance Criteria

- [ ] Every stage has a source, gate, batching posture, owner, and lowering route.
- [ ] NGAB0 is selected and ready for a delivery spec.
- [ ] The campaign begins from live `llvm-host` and device artifact support.
- [ ] Compiler, product build, host execution, Gradus, and inference-product
      ownership remain distinct.
- [ ] The artifact is an application binary, not a kernel renamed as a product.
- [ ] Release/version review is explicit at NGAB7.
- [ ] External spend and deployment remain authorization-gated or split out.

## Validation

```bash
python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory
python3 ../radix/scripta/generate-factory-readme.py --factory-root docs/factory --check
git diff --check -- docs/factory/native-gpu-application-bundle docs/factory/README.md
```

The sibling README generator and the shared status-audit script
(`radix/scripta/audit-factory-goal-status.py`) accept `--factory-root`
(drift corrected by U1, `evidence/ngab0-snapshot.md` §5), so `--check` plus
the scoped audit entrypoint form the current Faber campaign-index gate.
NGAB0 must add or select a Faber-scoped audit entrypoint before claiming the
full status-audit gate. The Faber-scoped entrypoint is
`scripta/check-factory-goal-status` (NGAB0-U10, council C7): a thin wrapper
that invokes the shared `radix/scripta/audit-factory-goal-status.py` against
faber's own `docs/factory` root. The joint cross-repo receipt schema is frozen
in `native-gpu-application-bundle/ngab0-receipt-schema.md` (U10) — aligned
with the composite contract's §Manifest/§Verification (identity + digest +
dirty-state declarations + exact commands). The status-audit gate for this
campaign is the faber-scoped entrypoint exiting 0.

Implementation validation is named by each delivery. Cheap owner-repo checks
run first. Metal/CUDA hardware receipts, RunPod use, broad parity suites, and
release gates occur only at their named boundaries.

## Open Questions

- Should the stable user-facing build selector remain `llvm-host` with an
  embedded-device capability, or should NGAB0 define a broader application
  artifact identity while retaining `llvm-host` as its host compiler lane?
- Does Metal embed source, metallib, or both for the first admitted macOS row?
- Which CUDA PTX architecture set is the minimum portable NGAB6 bundle?
- Which separate repository will own the first inference CLI/server product?

## Stop Conditions

Pause and route a need when host/device partition is ambiguous; a kernel
requires untyped symbol guessing; an artifact needs an undeclared external
file; a backend would silently fall back; a Gradus API would gain device
state; model licensing or acquisition is not explicit; external GPU spend is
needed without authorization; or a stage starts implementing server,
deployment, or distributed placement behavior in Faber or Radix.
