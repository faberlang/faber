# GPU Execution Architecture

**Status**: accepted direction; delivery is tracked and implementation is partial
**Date**: 2026-08-23
**Amended**: 2026-08-24 — the first proof now pairs BF16 and F32 GEMV
**Authority**: cross-repository GPU execution contract for Faber, Gradus,
Radix, and Hosts
**Delivery tracking**: private Radix campaign
`radix/docs/factory/gpu-execution-architecture/CAMPAIGN.md`

This document defines where GPU semantics, compilation, and physical execution
belong. It also defines the first bounded performance proof for that
architecture. It is not a claim that the current implementation already
conforms. This document remains the design authority; the Radix campaign named
above is the delivery ledger. Neither document claims that naming a backend,
model format, or kernel family delivers it.

## Core invariant

```text
Gradus Faber source
    -> Radix analysis, specialization, and target lowering
    -> device artifact plus explicit execution descriptor
    -> Hosts admission, binding, residency, launch, synchronization, and readback
    -> physical device
```

Gradus defines computation over logical devices. Radix turns that computation
into validated target artifacts and execution facts. Hosts binds those facts
to admitted physical resources and performs the effects that touch the device.

No layer may silently take over another layer's authority to make a path work.
In particular, a GPU route must not substitute a CPU implementation, infer
missing model semantics from buffer sizes, or move tensors through host memory
without an explicit placement operation.

## Ownership

| Concern | Authority | Contract |
| --- | --- | --- |
| Model, tensor, operator, training, and inference semantics | Gradus | Defines what the computation means. |
| Model-format interpretation | Gradus | Admits GGUF, Safetensors, MLX, or future formats into normalized model and tensor facts. |
| ML kernel source | Gradus | Authored in Faber; independent of physical device identity and model container. |
| Logical devices, placement, sharding, and communication intent | Gradus | Declares the program-level topology and semantic ownership of values. |
| Language semantics, type checking, optimization, and specialization | Radix | Validates the Faber program and derives target-neutral execution facts. |
| Device artifact and compiled execution descriptor | Radix | Lowers the same Gradus source to MSL, PTX, WGSL, or another target artifact plus explicit binds, grids, lifetimes, and dependencies. |
| Physical discovery and capability reporting | Hosts | Enumerates actual devices and reports their usable capabilities. |
| Virtual-partition admission and budget enforcement | Hosts | Creates machine-local admitted partitions over physical capacity and fails closed on overcommit. |
| Logical-to-physical binding | Hosts | Binds a compiled logical plan to admitted virtual partitions and physical devices. |
| Buffers, pipelines, queues, residency, launch, synchronization, and readback | Hosts | Performs physical effects and preserves the compiled execution contract. |
| Deployment and request scheduling policy | Product or Hosts coordinator | Selects allowed devices, concurrency, and tenant policy; it does not change kernel semantics. |

Radix may carry compiler representations of Gradus concepts. Hosts may carry
runtime representations of compiled facts. Those representations do not move
semantic authority out of Gradus.

## Kernel authorship

Every computational kernel that implements Gradus ML semantics is authored in
Faber under Gradus.

This is a target-state ruling, not a pilot question. Migration may be sequenced
by kernel family, but a pilot determines which language, lowering, or host gaps
must be closed. It does not decide whether the kernel remains permanently as a
hand-written Rust, MSL, or PTX body elsewhere.

The boundary does not remove Rust from the system:

- Radix uses Rust to implement parsing, semantic analysis, optimization,
  lowering, instruction selection, and artifact serialization.
- Hosts uses Rust to implement device discovery, admission, resource
  management, queues, launch, synchronization, and readback.
- A target-specific Radix lowering may select a native instruction or sequence
  for a Faber operation. That lowering is not an independent semantic kernel.
- Hosts may contain physical runtime primitives with no Gradus tensor
  semantics, such as device allocation or queue operations. Such primitives
  must not become alternate implementations of ML operators.
- CPU and hand-written device bodies may remain temporarily as test oracles.
  They are not production fallbacks or parallel semantic authorities.

### Migration rule

For each existing ML kernel family:

1. Freeze the current correct body and numerical fixtures as migration
   oracles.
2. Author the replacement in Gradus Faber.
3. Close every required Faber, Radix, or Hosts gap in its owning layer.
4. Prove numerical correctness and inspect the emitted target operations.
5. Measure the new body under the same execution contract as its comparator.
6. Switch compiled plans to the Gradus-authored body.
7. Delete the former production body. Retain a reference implementation only
   when a test still needs an independent oracle.

The transition is additive only until the replacement is proven. There is no
indefinite dual-authority compatibility tier.

## Model formats and tensor representations

Four axes must remain separate:

| Axis | Examples | Kernel visibility |
| --- | --- | --- |
| Model container | GGUF, Safetensors, MLX, future Faber packing | Invisible after admission |
| Model architecture | SmolLM2, Qwen, dense transformer, MoE | Visible through Gradus model semantics |
| Tensor representation | F32, F16, BF16, Q8_0, Q4_0, K-quants | Visible in typed tensor layout and kernel selection |
| Device target | Metal, CUDA, WebGPU | Chosen by Radix lowering and Hosts binding |

A Q8_0 kernel necessarily understands Q8_0 block layout. It must not know
whether those blocks came from a GGUF file or another container. A future
packing format may map to an existing tensor representation or introduce a new
typed representation, but it must not require container-specific branches in
the kernel.

Model loading and conversion are measured separately from steady-state
execution. Weights are admitted once, bound once, and remain device-resident
for the timed session unless an explicit placement plan says otherwise.

## Logical, virtual, and physical devices

The terms are not synonyms.

### Logical device

A logical device is a program-level placement identity. Gradus uses logical
devices to express where semantic values live, how a model is sharded or
replicated, and which transfers or collectives are required. Logical device
identity is portable and contains no physical device identifier.

Radix lowers the logical topology into target-neutral partitions and an
execution graph. The compiler representation may include launches, transfers,
barriers, collectives, buffer lifetimes, and resource demands, but it still
contains no binding to a particular installed card.

### Virtual device partition

A virtual device partition is a machine-local Hosts admission object. It binds
exactly one physical device, draws from that device's safe capacity, and
enforces a declared memory budget. Many virtual partitions may bind the same
physical device.

Virtual partitions are software admission boundaries. They do not claim
hardware isolation, CUDA MIG, MPS isolation, or independent physical capacity.
Hardware isolation may be added later without changing Gradus kernels.

### Physical device

A physical device is an actual Metal, CUDA, WebGPU, or future device discovered
and managed by Hosts. Physical identity, driver state, queues, allocations,
failure, and teardown never enter Gradus kernel semantics.

### Binding shapes

The same logical execution graph can be bound as:

- one logical partition to one physical device;
- eight logical partitions through eight admitted virtual partitions on one
  physical device;
- eight logical partitions across two physical devices; or
- eight logical partitions one-to-one across eight physical devices.

The kernels do not change between those bindings.

A model larger than one card is not solved by virtual admission alone. Gradus
must express the model's sharding and communication semantics. Radix must
compile the per-partition programs and communication graph. Hosts must bind and
execute that graph on the chosen physical topology.

## Compiled execution contract

Radix emits both the target artifact and the facts Hosts needs to execute it.
The descriptor must carry, rather than invite Hosts to rediscover:

- tensor dtypes, shapes, strides, and representation layouts;
- buffer identities, offsets, bounds, lifetimes, and residency classes;
- kernel entry identity and target capability requirements;
- dispatch geometry and the relationship between logical outputs and physical
  threads;
- operation dependencies, barriers, and legal synchronization points;
- logical partition identity, transfers, and collectives;
- resource demand used for virtual-partition admission; and
- declared outputs and exact readback ranges.

Hosts validates these facts against physical capabilities and fails closed on
inconsistency. It does not derive head dimensions, cursor semantics, dispatch
coverage, or model topology from coincidental resource extents. It does not
silently select a CPU body when a declared GPU body cannot run.

For a resident inference session:

- weights and persistent KV storage are prepared once;
- pipelines and binding plans are cached by complete identities;
- intermediate tensors stay on device;
- a subview operation transfers only its declared range;
- device-side dependencies do not force host-visible waits;
- CPU/GPU synchronization is explicit and measured; and
- readback is limited to the outputs the caller actually consumes.

## First-principles performance proof

The first proof deliberately removes quantization and MoE complexity. Its job
is to determine whether the basic kernel, compiler, and execution architecture
is competitive and, when it is not, to identify the owning layer.

### Fixed comparison

| Axis | First proof |
| --- | --- |
| Model | SmolLM2-360M-Instruct |
| Source artifact | Official BF16 Safetensors at one pinned Hub revision |
| Derived artifact | F32 GGUF produced by exact BF16-to-F32 value expansion from that revision |
| Kernel source | Two separate Gradus Faber bodies implementing one logical GEMV |
| Initial operation | Model-shaped dense decode GEMV with F32 input, accumulation, and output |
| Weight representations | Resident BF16 and resident expanded F32 |
| Initial target | Metal on burgus |
| Kernel oracle | Independent CPU F32 reference over the same represented values and input |
| Model comparator | llama.cpp on the derived F32 artifact and same physical device |

The pinned source is the official model's BF16 Safetensors artifact. Every
source BF16 value is exactly representable in F32, so the derived F32 artifact
preserves the represented values without adding precision that the source did
not contain. This proves F32 execution over the official values; it is not a
claim that the model was trained or published with original-F32 precision.
Expanding an integer-quantized model into F32 remains an invalid substitute.

The two kernels are deliberately separate in the first slice. One reads BF16
weights and converts each value for F32 accumulation. The other reads the
already-expanded F32 weights. They use the same F32 input and output contract.
Shared abstractions are extracted only after both bodies execute, so the second
implementation reveals the real tensor-view, descriptor, residency, dispatch,
measurement, and numerical seams.

### Benchmark ladder

1. **Kernel:** deterministic model-shaped paired GEMV cases with inputs and
   weights already resident. Compare the BF16-weight/F32-accumulate body and
   the F32-weight/F32-accumulate body against the independent CPU F32 oracle
   and against each other. Record numerical output, device time, effective
   bandwidth, dispatch geometry, and submission/synchronization overhead.
2. **Transformer block:** repeat one SmolLM2 block with resident tensors. This
   introduces normalization, Q/K/V and output projections, RoPE, attention,
   residuals, and the feed-forward network while excluding tokenizer,
   sampling, and server behavior.
3. **Full decode:** run the same F32 model through both systems with matched
   prompt tokens, generation length, KV representation, attention settings,
   warmup, and repetitions.

Each rung records:

- numerical comparison and the accepted tolerance;
- GPU time per operation and per token;
- host submission and orchestration time;
- dispatch, command-buffer, and host-visible wait counts;
- host-to-device and device-to-host bytes;
- pipeline creation, binding, and cache-reuse counts; and
- end-to-end prompt-processing and token-generation throughput where the rung
  supports those metrics.

Model acquisition, conversion, loading, tokenization, and sampling are outside
the steady-state timing window and are reported separately.

### Done condition

The first proof is complete when:

1. Both Gradus-authored kernels consume the same represented weight values.
2. Both kernels are numerically correct against the independent CPU F32 oracle,
   and their cross-kernel difference is recorded.
3. Radix produces distinct device artifacts and complete execution descriptors.
4. Hosts runs both artifacts without a CPU substitute in the timed path.
5. Measurements separate loading, conversion, residency, GPU work, host work,
   synchronization, and transfers.
6. The resident F32 transformer-block rung passes its numerical and execution
   gates.
7. The full F32 decode rung and pinned llama.cpp comparator consume the same
   derived artifact under the matched model-level comparison contract; that
   receipt is not presented as an isolated GEMV oracle.
8. Every material gap from llama.cpp is attributed to a named layer with
   evidence and a concrete next change.

The first measurement does not have to reach parity. An unexplained aggregate
number is not completion; an honest, reproducible decomposition is.

After the F32 architecture is understood, the comparison advances in this
order: F16, Q8_0, Q4_0, K-quants, mixed-representation model files, and MoE.
Those are compatibility requirements for the architecture, not deliverables
inside the F32 proof.

## Gap protocol

Failure to express, compile, or execute a Gradus kernel is an architecture
signal. It is handled before performance work continues.

| Gap | Owning response |
| --- | --- |
| Faber cannot express a stable, target-neutral operation | Settle its semantics, add the language or typed intrinsic surface, and implement Radix validation and lowering. |
| Faber expresses the operation but a target cannot lower it | Add the Radix backend lowering and target capability proof. |
| The artifact is complete but cannot be admitted, bound, or executed | Add the missing Hosts capability without moving model semantics into Hosts. |
| Placement, sharding, or communication meaning is absent | Define it in Gradus, represent and compile it in Radix, then bind and execute it in Hosts. |
| A GPU route needs a CPU or hand-written body to continue | Keep that body as an explicit oracle, stop the production path, and resolve the owning gap. |

Every gap record names the missing semantic fact, the owning repository, the
smallest proof that closes it, and whether it blocks the current benchmark
rung. No escape hatch becomes a permanent fallback merely because it made one
test pass.

## Superseded directions

This document supersedes earlier statements, wherever they appear, that:

- the remaining inference performance gap is known not to be architectural;
- the production ML kernel library belongs in Hosts or an independent Rust
  kernel crate;
- a Q8_0 authorship pilot decides whether kernels migrate to Gradus; or
- a host may infer missing execution facts or silently fall back to CPU work.

Earlier performance reviews, hand-written kernels, and implementation records
remain useful evidence and migration oracles. They are not the target
architecture.

## Scope boundaries

This architecture is intended to support Metal, CUDA, WebGPU, multiple model
containers, multiple tensor representations, dense and MoE models, and
multi-device execution. The first proof implements none of those merely by
naming them.

The paired BF16/F32 kernel slice does not extend BF16 through the transformer
block or full-decode rungs. Those rungs remain F32 until a later campaign stage
explicitly advances another representation.

The first proof does not include:

- F16, integer-quantized, or MoE kernel delivery;
- CUDA or WebGPU parity;
- hardware isolation for virtual partitions;
- a multi-tenant scheduler or inference server;
- a custom model-packing format;
- complete migration of every existing kernel; or
- an advance promise of llama.cpp performance parity.

Those follow as bounded work after the F32 evidence identifies which parts of
the architecture already work and which must change.
