# Deck: Autograd-Equivalent Roadmap

**Status**: roadmap — PyTorch-near autograd-equivalent milestone scoped to evidence and next units; no public PyTorch parity claim
**Created**: 2026-07-14
**Refreshed**: 2026-07-14
**Target workspace**: `/home/ianzepp/work/faberlang`
**Factory artifact dir**: `faber/docs/factory/autograd-equivalent-roadmap/`
**Primary surfaces**: Faber factory planning, `faber-runtime` dense tensor
autograd scaffold, Radix AIR route, examples GPU workload oracles, future Faber
session boundary.

---

## Slide 1 - Ask

Estimate how close Faber is to a minimal autograd-equivalent proof and define
the smallest honest PyTorch-near milestone.

The milestone is not "PyTorch parity." It is a narrow proof that Faber can own
a reverse-mode gradient path for a dense scalar-loss tensor graph, validate it
against a CPU oracle, and preserve enough session evidence to plan the next
training-loop step.

## Slide 2 - Guardrail

This roadmap is evidence and sequencing only.

Non-claims:

- no public PyTorch replacement;
- no `torch.nn` parity matrix;
- no generated AIR autodiff yet;
- no optimizer/session API support;
- no GPU training loop;
- no CUDA/WebGPU/Metal gradient execution;
- no sparse, packed, quantized, or model-format autograd.

## Slide 3 - Shipped Evidence To Start From

| Required evidence | Current shipped fact | Source |
| --- | --- | --- |
| Finite-difference oracle | Runtime-local central-difference checks cover scalar, same-shape vector, broadcast-bias, and dense linear training-step gradients. | `faber-runtime/docs/factory/autograd-substrate-inventory.md`; `faber-runtime/src/autograd_reference_test.rs` |
| Broadcast reductions | The internal tape reduces broadcast gradients for add/sub/mul and has a broadcast-bias oracle. | `faber-runtime/src/autograd.rs`; `faber-runtime/src/autograd_reference_test.rs` |
| view/alias policy | `sectio` views alias in the Rust tensor carrier; autograd rejects view leaves until scatter-add policy exists. | `faber-runtime/docs/factory/autograd-substrate-inventory.md`; `faber-runtime/src/autograd.rs` |
| Matmul/linear VJP | Dense rank-2 matmul exists; the internal autograd scaffold computes matmul VJPs with a private transpose helper. | `faber-runtime/docs/factory/autograd-substrate-inventory.md`; `faber-runtime/src/autograd.rs` |
| Manual update oracle | Test-only `LinearTrainingSession` applies manual `param -= learning_rate * grad` updates to weight and bias. | `faber-runtime/src/autograd_reference_test.rs` |
| optimizer/session boundary | Faber inference/session docs define contract-only session and oracle manifest boundaries, not runtime execution. | `faber/docs/factory/inference-session-boundary/goal.md` |
| loss-trace oracle | A two-step linear session loss trace matches the finite-difference trace and strictly decreases. | `faber-runtime/src/autograd_reference_test.rs` |

## Slide 4 - Current Proximity Read

Faber is closer to a minimal autograd-equivalent proof than to a full ML stack:

- dense `Tensor<f32>` reverse-mode scaffolding already exists in runtime-local
  code;
- finite-difference oracles already validate the seed gradient family;
- broadcast and rank-2 matmul VJP evidence cover the small linear model shape;
- session/update/loss-trace evidence exists as a test-only oracle;
- AIR is architecture-captured as the compiler-owned pure-functional autodiff
  detour, but generated AIR gradients are not implemented.

The honest milestone should therefore be a CPU reference gradient proof first,
then a generated/AIR proof, then a session/training proof. GPU training remains
outside the first milestone.

## Slide 5 - Smallest Honest Milestone

Minimal autograd-equivalent proof:

> Given a materialized dense `Tensor<f32>` scalar-loss graph using add/sub/mul,
> broadcast add/sub/mul, rank-2 matmul, and `summa`, produce reverse-mode
> gradients that match a CPU finite-difference oracle for a single linear layer
> or tiny MLP, then replay one or two manual parameter updates and verify the
> loss trace decreases.

Boundary:

- inputs are contiguous materialized dense tensors;
- no `sectio` view leaves;
- no mutation after graph capture;
- no sparse or packed tensors;
- no public optimizer API;
- no GPU/device execution.

## Slide 6 - Next Implementable Units

| Unit | Done when | Dependencies | Non-claims |
| --- | --- | --- | --- |
| A1 Runtime autograd inventory ratchet | A code-owned/runtime-owned check confirms the shipped dense proof surface: finite-difference oracle, broadcast reductions, view rejection, matmul VJP, manual update, and loss-trace rows. | Existing `faber-runtime` autograd scaffold and inventory. | Not public autograd; not generated AIR; not PyTorch parity. |
| A2 AIR eligibility and purity slice | A Radix/Faber evidence packet names the source subset for differentiable functions and fail-closed exclusions: mutation, aliasing views, opaque effects, unsupported tensor ops. | Radix AIR lane routing, AIR purity policy, tensor type facts. | Not AIR lowering; not optimizer/session support. |
| A3 Generated gradient oracle for scalar linear loss | A generated or compiler-owned path produces the scalar rung-3 gradient and matches the finite-difference oracle for `loss(x, weight, target) = (x * weight - target)^2`. | A1, A2, examples rung-3 oracle. | Not broad reverse-mode; not GPU; not `torch.autograd`. |
| A4 Dense linear layer / tiny MLP gradient proof | Reverse-mode gradients for `summa((XW + b - target)^2)` match CPU finite differences for input, weight, and bias. | A3, matmul/linear VJP, broadcast reduction policy. | Not arbitrary module graphs; not public `torch.nn` parity. |
| A5 Manual update and loss-trace replay | A bounded session/oracle runner applies manual weight/bias updates and verifies the two-step loss trace against the finite-difference session oracle. | A4, Faber session CLI contract, manual update oracle. | Not optimizer API, checkpointing, dataloader, or training loop product. |
| A6 GPU/workload handoff map | Examples/Radix/Faber docs map how the CPU proof feeds GPU workload rung 3/4 without claiming device gradients. | A3-A5, examples GPU workload oracle contracts. | Not CUDA/WebGPU/Metal training; not performance evidence. |

## Slide 7 - Recommended First Packet

File A1 first unless a runtime owner has already closed it.

Reason:

- it converts "we seem close" into a stable evidence gate;
- it keeps the current runtime proof separate from generated/AIR claims;
- it gives AIR and session work a small, testable target instead of a
  PyTorch-sized surface.

Suggested want:

`[P1][runtime][autograd] code-owned dense autograd proof inventory ratchet`

Done when: a runtime-local matrix/check ties finite-difference, broadcast
reduction, view rejection, matmul VJP, manual update, and loss-trace evidence
to exact tests and preserves explicit non-claims.

## Slide 8 - Executive Decision Points

1. Treat "autograd-equivalent" as the PyTorch-near milestone, but define it as
   dense scalar-loss reverse mode plus oracle-checked linear/tiny-MLP proof.
2. Keep AIR as the compiler-owned generated-gradient path; the runtime scaffold
   remains reference evidence until AIR produces gradients.
3. Keep session/optimizer work as oracle replay first, product API later.
4. Keep GPU training outside the first milestone; cite workload rungs only as
   downstream acceptance targets.

## Slide 9 - Stop Conditions

Stop and re-scope if a follow-up tries to:

- claim public PyTorch or `torch.nn` parity;
- route around AIR by making runtime-local tape the compiler story;
- allow views or mutation without an alias/scatter-add policy;
- claim optimizer/session support from the manual update oracle;
- claim GPU training from examples rung 3/4 oracles;
- include sparse, packed, quantized, or model-format gradients in the first
  milestone.
