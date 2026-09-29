# Host Execution Architecture

**Status**: accepted direction; existing Hosts implementations are only partially conformant
**Date**: 2026-09-04
**Authority**: cross-repository ownership contract for Faber libraries, Radix,
and platform/browser hosts

> **Superseded in part — 2026-09-29.** The private Radix campaign
> `radix/docs/factory/gpu-reset/CAMPAIGN.md` amends this document in three
> places. Where they disagree, the campaign rulings win:
>
> 1. **The executor is an in-process runtime layer, not a separate Hosts
>    repository or child process.** The ownership role in this document stands:
>    the executor performs physical effects and never library semantics. The
>    `hosts` repository is being retired into small crates inside the Faber
>    product repository.
> 2. **The running host Faber program is the launch graph.** Radix still emits
>    each kernel artifact plus its per-dispatch facts (bindings, geometry,
>    extents). The sequence of dispatches comes from the program as the MIR
>    runner executes it, not from a precompiled whole-program descriptor. A
>    captured or precompiled graph may return later as an optimization.
> 3. **Protocol leaf exception.** A native effect leaf may wrap a standard
>    protocol that platforms ship natively (TLS, HTTP, WebSocket).
>    Library-specific composite behavior (Norma, Gradus, Triga, Tela semantics)
>    remains forbidden in the executor.
>
> Everything else here remains the design direction: library ownership of
> semantics, the forbidden-behavior list, kernels authored in Faber, the
> logical/virtual/physical device model, and resident-session rules. Delivery
> links and performance-proof sections are obsolete. A consolidated
> replacement will be written into the top-level `ARCHITECTURE.md` during
> gpu-reset Stage S1.

This document defines the boundary between portable library behavior and the
platform products that execute compiled Faber artifacts. It applies to every
library, not only Gradus or GPU inference.

## Core invariant

```text
library source and semantics
    -> Radix validation, specialization, and target lowering
    -> target artifact plus explicit execution descriptor
    -> Hosts capability admission, binding, execution, and observation
    -> operating system, browser, or physical device
```

Hosts owns execution mechanism. A library owns the meaning of its operations.
Radix owns the transformation from that meaning into an executable artifact.
No host may implement a library operation merely because that host is the
component that ultimately touches the operating system or device.

The boundary is a microkernel boundary: Hosts exposes a small set of physical
capabilities and executes complete supplied programs. It does not grow a
parallel implementation library for every consumer.

## Portability law

A library feature must not require an implementation in every operating-system
host. Adding a library operation changes the owning library and, when needed,
Radix lowering. Hosts changes only when the operation requires a genuinely new
physical capability or execution primitive.

This keeps the system additive rather than multiplicative:

```text
required: library semantics + target lowerings + platform executors
forbidden: library semantics x every platform executor
```

The first macOS implementation is one executor of a finite contract. It is not
the semantic reference that future Linux, Windows, browser, CUDA, Vulkan, or
other hosts must port.

Multiple operating-system hosts are not required to prove this architecture.
Every design and review must nevertheless ask whether a future platform could
implement the same execution contract without copying a library algorithm from
an existing host.

## Ownership

| Concern | Authority |
| --- | --- |
| Public operation meaning, algorithms, policies, and portable state | Owning library |
| ML tensor, inference, training, quantization, and kernel semantics | Gradus |
| Standard-library behavior above physical effects | Norma |
| Graphics, geometry, view, and rendering semantics | Triga or Tela |
| Language validation, specialization, optimization, fusion, and target lowering | Radix |
| Target artifacts and complete execution descriptors | Radix |
| Physical capability discovery and admission | Hosts |
| Buffers, handles, pipelines, queues, bindings, residency, transfers, synchronization, launch, readback, and teardown | Hosts |
| Product request, tenancy, deployment, and scheduling policy | Product or Hosts coordinator, without changing library semantics |

A host may carry runtime representations of compiled facts. Carrying a dtype,
layout identifier, entry identity, or capability requirement does not transfer
semantic authority to the host. Hosts treats those values as descriptor facts;
it does not use them to recreate an operator.

## Capability implementation versus library implementation

Hosts necessarily contains platform-specific code. The distinction is whether
that code implements a physical primitive or a portable library algorithm.

Allowed examples:

- open a file handle, write declared bytes, read a clock, or spawn a process;
- create a Metal, CUDA, WebGPU, or other backend pipeline from a supplied artifact;
- allocate a declared byte range and enforce its lifetime and residency class;
- bind declared offsets and launch geometry;
- submit work, synchronize at declared points, and return declared observations;
- reject an artifact whose required capabilities are unavailable.

Forbidden examples:

- implement QKV projection, RMS normalization, RoPE, attention, sampling, or training;
- decode Q4, Q8, or another packed representation as part of an ML operator;
- implement Norma, Triga, Tela, Gradus, or another library's composite behavior;
- synthesize MSL, PTX, WGSL, or another kernel body when Radix omitted it;
- select an alternate library algorithm or silently run a CPU body when a
  declared device path cannot execute;
- infer model axes, tensor meaning, cursor policy, or operation identity from
  names, buffer lengths, resource ordinals, or observed output;
- probe competing semantic interpretations until one matches expected values.

A host may compile target source supplied by Radix when the platform API uses
runtime compilation. It must not author or template that target source.

## Execution contract

The artifact bundle must provide everything the executor needs without asking
it to rediscover library meaning:

- artifact identity, target, entry identity, and capability requirements;
- buffer identities, byte ranges, offsets, bounds, and access modes;
- dtypes, physical layouts, shapes, and strides when the backend must validate them;
- lifetimes and residency classes;
- binding indices and target-specific resource metadata;
- launch geometry and coverage;
- dependency, barrier, transfer, and synchronization requirements;
- declared observations and exact readback ranges; and
- versioned failure behavior for unsupported or contradictory facts.

Hosts validates this contract against physical capabilities and fails closed.
Missing facts are an upstream library or Radix defect. They are never permission
for a host to reconstruct a plan, manufacture a body, or fall back to another
execution route.

## Cross-library examples

- Norma defines the semantics of its IO APIs. Hosts providers implement bounded
  filesystem, process, clock, network, and console capabilities.
- Triga and Tela define geometry, graphics, DOM, Canvas, and view behavior.
  Browser or GPU hosts expose the physical APIs and execute supplied artifacts.
- Gradus defines tensor, inference, training, packed-representation, and kernel
  semantics. Radix lowers them. Metal, CUDA, WebGPU, and future hosts execute
  the resulting artifacts without learning those algorithms.

The same rule applies to every future library.

## Review tests

Every proposed Hosts change must pass these questions:

1. Would a future Linux or Windows host have to copy this code?
2. Does the code change the mathematical or library-visible result for a fixed
   artifact, descriptor, and inputs?
3. Does it dispatch on a library operation, model architecture, or container
   format rather than a physical capability?
4. Is it compensating for a missing artifact or descriptor fact?
5. Could an unrelated new library execute through the same host without the
   host learning that library's vocabulary?

A `yes` to questions 1-4, or `no` to question 5, is an ownership failure unless
the change is demonstrably a new physical capability.

## Migration rule

Existing nonconforming code is migration debt, not precedent:

1. Freeze a numerical or behavioral oracle when one is genuinely needed.
2. Express the portable behavior in the owning library.
3. Close required language and compiler gaps in Radix.
4. Emit the complete target artifact and execution descriptor.
5. Prove the artifact through a generic Hosts execution path.
6. Delete the production host-side semantic implementation, selector, probe,
   and fallback in the same migration slice.
7. Keep an independent reference only in a clearly test-only surface that
   production dispatch cannot call.

There is no indefinite dual-authority compatibility tier. Existing Hosts code,
tests, factory documents, and commit history do not override this contract.

## Completion shape

Hosts is conformant when a platform product can execute a previously unknown
library artifact using only the generic execution contract, and adding a new
library operator requires no Hosts source change unless it introduces a new
physical capability.
