# Goal: PyTorch-Session Training Workflow — Next Bounded Units

**Status**: proposed — goal-forge for next 1–3 bounded implementable units after autograd-equivalent roadmap A1–A5 closeout
**Created**: 2026-07-26
**Target workspace**: `/Users/ianzepp/work/faberlang`
**Factory artifact dir**: `faber/docs/factory/pytorch-session-continuation/`
**Primary surfaces**: `faber-runtime` dense tensor autograd, `examples/training/`, `norma/src/optimizer.fab`, `faber-runtime/docs/factory/`

---

## Summary

After the autograd-equivalent roadmap units A1–A5 shipped generated-backward
training for linear regression, two-layer MLP, and a 22-param BERT-tiny
fragment, the north-star training pipeline works but remains **inline** —
every example manually writes the forward pass and manually inlines
`param -= lr * grad` for each parameter. The next bounded step adds one
critical missing loss function (cross-entropy with softmax VJP), extracts
the inline SGD pattern into a reusable training session exemplum, and
updates the north-star evidence gate document.

This is not a public PyTorch replacement, not `torch.nn` parity, not a
production optimizer library, and not a GPU training loop.

## Problem

The north-star has generated-backward proof for MSE-based training on dense
`Tensor<f32>` graphs with up to 22 trainable params and 16 differentiable
AIR ops, but:

1. **No cross-entropy loss.** Softmax is forward-only (VJP deferred to
   "cross-entropy goal" in `faber-runtime/docs/factory/autograd-substrate-inventory.md`
   line 76, `radix/docs/factory/mir-swarm/delivery/d-p-09-activation-expansion.md`
   line 19). Classification-style training (the dominant PyTorch use case)
   cannot be demonstrated.

2. **No reusable training pattern.** All training exempla inline the forward
   pass and inline SGD updates. `norma/src/optimizer.fab` has shape-specific
   `sgd_step` functions for `[4]`, `[2,2]`, `[1,2]`, and `[]` shapes, but
   no generic tensor-shape SGD exists (blocked by Radix shape generic
   resolution, per `norma/src/optimizer.fab` line 12). Training loops are
   copy-paste across `linear-regression/`, `mlp/`, and `bert-tiny-fragment/`.

3. **North-star gaps are scattered.** Evidence is spread across the autograd
   roadmap deck, substrate inventory, GPU workload README, and three training
   exempla. No single document maps current shipped evidence to the next gates
   for a Hand to pick up.

## Goals

1. **Cross-entropy loss with softmax VJP.** Add `Tensor<f32>::crux_entropia`
   and `AutogradOp::CruxEntropia` with analytical VJP to the runtime-local
   dense autograd scaffold. Prove with finite-difference oracle. The VJP
   combines the softmax Jacobian `diag(s) - s @ s^T` with the cross-entropy
   gradient simplification (logits - one-hot targets).

2. **Reusable training session exemplum.** Create an examples-owned training
   session pattern that composes a model (forward + backward companion), loss
   function, and optimizer step into a reusable loop. Not a product library —
   an exemplum that shows the pattern and reduces copy-paste across training
   examples.

3. **North-star evidence gate document.** Create a single durable document
   that maps current shipped evidence (dense autograd proof, generated
   backward companions, 3 training exempla, 16 differentiable ops) to the
   next gates (cross-entropy, session abstraction, dataloader, checkpointing).
   This gives future Hands a cold-start map.

## Non-Goals

- No public PyTorch or `torch.nn` parity claim.
- No generated AIR autodiff for cross-entropy (runtime-local VJP only).
- No host ABI gradient handle for cross-entropy.
- No generic tensor-shape optimizer (blocked on Radix shape generics).
- No production optimizer library, scheduler, or checkpointing.
- No dataloader, batching, or dataset abstraction.
- No GPU/device training loop.
- No model serialization or safetensors save.
- No LLM inference, tokenizer execution, or embedding layer.
- No sparse, packed, or quantized gradient rules.

## Ground Truth Researched

| Claim | Evidence |
| --- | --- |
| Softmax is forward-only, VJP deferred | `autograd-substrate-inventory.md` line 76: "Softmax forward-only (VJP deferred to cross-entropy goal)"; `d-p-09-activation-expansion.md` line 19: "No VJP in this delivery" |
| Cross-entropy not implemented | `grep -i "cross.entropy\|crux_entropia" faber-runtime/src/` returns zero hits |
| Generated backward companions work | `linear-regression/src/train.fab`, `mlp/src/train.fab`, `bert-tiny-fragment/src/train.fab` all use `@ radix backward` and inline SGD |
| BERT-tiny fragment trains 22 params | `bert-tiny-fragment/src/train.fab` — 8-step loop, 16 of 18 differentiable AIR ops, 22 trainable params, 528 floats |
| SGD is shape-specific in norma | `norma/src/optimizer.fab` — `sgd_step` for `[4]`, `sgd_step_2x2` for `[2,2]`, `sgd_step_1x2` for `[1,2]`, `sgd_step_0d` for `[]`; line 12: "Shape generics blocked by radix tip" |
| Inference session boundary exists | `faber/docs/factory/inference-session-boundary/goal.md` — proposed, contract-only; covers inference, not training sessions |
| All 3 training exempla inline SGD | Each `train.fab` has ~10+ lines of inline `seed.crea(lr, shape)` + `gw.multiplica(lr_fill)` + `weight.subtrahe(scaled)` per parameter |
| Autograd roadmap A1-A5 substantially closed | `autograd-equivalent-roadmap/deck.md` Slide 3 evidence matches shipped code; A3-A5 training exempla exist |

## Reference Packet

| Path | What it proves |
| --- | --- |
| `faber-runtime/src/tensor.rs` | `Tensor<f32>::softmax()` — forward-only, rank-1 and rank-2 row-wise |
| `faber-runtime/src/autograd.rs` | `AutogradTape` — existing VJP for add/sub/mul/divide/matmul/neg/forma/permute/sectio/media/layernorm/gelu/relu/sqrt |
| `faber-runtime/src/autograd_reference_test.rs` | Finite-difference oracle for all existing autograd ops including layernorm, gelu, relu, sqrt |
| `faber-runtime/docs/factory/autograd-substrate-inventory.md` | Code-owned inventory of shipped dense autograd proof surface |
| `faber-runtime/docs/factory/view-alias-autograd-policy.md` | Canonical view/alias autograd policy |
| `faber/docs/factory/autograd-equivalent-roadmap/deck.md` | Roadmap: A1–A6 units, current proximity read, smallest honest milestone |
| `faber/docs/factory/inference-session-boundary/goal.md` | Proposed inference session boundary (contract-only) |
| `norma/src/optimizer.fab` | Shape-specific SGD step functions |
| `examples/training/linear-regression/src/train.fab` | 2×2 linear+MSE, 8-step inline SGD, generated backward companion |
| `examples/training/mlp/src/train.fab` | Two-layer MLP with GELU, 8-step inline SGD, generated backward companion |
| `examples/training/bert-tiny-fragment/src/train.fab` | Single-layer BERT-tiny, 22 params, 16 ops, 8-step inline SGD |
| `examples/training/rung-4-autograd.fab` | Scalar regression with `@ radix backward` |
| `examples/gpu-workload/README.md` | GPU workload rungs, rung-4 toy-train contract |
| `radix/docs/factory/mir-swarm/delivery/d-p-09-activation-expansion.md` | Softmax VJP deferral rationale |

## Constraints And Invariants

- **Dense `Tensor<f32>` only.** Cross-entropy stays in the same proof boundary as existing autograd: contiguous materialized tensors, no sparse, no packed.
- **No host ABI export.** Cross-entropy stays runtime-local like existing division, neg, layernorm, gelu VJPs.
- **No generated AIR backward for cross-entropy.** The VJP is analytical (combines softmax Jacobian with CE simplification), not an AIR transform candidate in this packet.
- **Session exemplum is examples-owned.** Not a `norma:training` package. Uses existing `norma:optimizer` shape-specific SGD or re-inlines.
- **Path-disjoint from reverse_ad.** No changes to `radix/crates/radix-air/`, `radix/crates/radix-mir/` transform passes, or AIR reverse-mode generation.
- **Follow existing autograd patterns.** Cross-entropy VJP follows the same quotient-rule, broadcast-reduction, and finite-difference patterns as existing ops.

## Architecture Direction

**Cross-entropy loss:**

The VJP for cross-entropy with softmax has a well-known simplification:
`dL/dlogits = softmax(logits) - targets` (when targets are one-hot).
This avoids computing the full softmax Jacobian. The runtime-local
implementation:

1. `Tensor<f32>::crux_entropia(logits, targets)` — forward: softmax → -sum(targets * log(softmax + ε)) / N. Domain validation: non-empty, finite inputs, targets in [0,1] range (one-hot relaxed to probability distribution for oracle compatibility). ε defaults to 1e-7.
2. `AutogradOp::CruxEntropia` — records saved softmax outputs. Backward: `grad * (softmax - targets) / N`. Broadcast-reduce if targets are broadcast-compatible with logits.
3. Finite-difference oracle: fixed logits and targets, compare autograd gradient against central-difference perturbation of logits.

**Training session exemplum:**

Extract the common pattern from the three training exempla into a documented,
reusable shape stored in `examples/training/`:

```text
examples/training/
  session-exemplum/
    README.md          — pattern documentation
    src/
      train.fab        — exemplum using the pattern
    faber.toml
```

The exemplum shows: define a model (forward + backward companion), define a
loss, compose into a training step function, loop with loss trace. It reuses
`norma:optimizer` shape-specific SGD or demonstrates the inline pattern as
documentation. It does not claim generic tensor-shape SGD.

**North-star evidence gate document:**

A single document (`north-star-evidence-gate.md`) under this goal's factory
directory that answers "what is proven, what is next, what is explicitly out
of scope." Follows the same structure as the autograd roadmap deck but
narrower: current evidence table, next 3–5 gates table, explicit non-claims.

## Supporting Skills

- `$factory` — implementation phase for PSC-1 (runtime), PSC-2 (examples)
- `$polish` — per-file refinement after implementation
- `$auditor` — cross-entropy VJP correctness review

## Implementation Shape

The first milestone is PSC-1 (cross-entropy loss). It is the smallest
self-contained unit, has a well-known analytical VJP, and unblocks
classification-style training demonstrations.

PSC-2 (session exemplum) depends on PSC-1 for a second loss function to
demonstrate composability, but can ship with MSE only if PSC-1 is delayed.

PSC-3 (evidence gate) is documentation only and has no code dependencies.

## Release Posture

No user-visible release. All units are internal evidence: runtime-local
autograd VJP (PSC-1), examples-owned exemplum (PSC-2), and documentation
(PSC-3). No CLI surface, no host ABI, no package publish.

## Exit Strategy

Stop and re-scope if:
- A unit tries to add host ABI symbols for cross-entropy.
- A unit tries to create a `norma:training` or `norma:loss` package.
- A unit tries to add generated AIR backward for cross-entropy.
- The session exemplum grows into a product optimizer library.
- A unit claims public PyTorch or `torch.nn` parity.
- Cross-entropy VJP drifts into sparse/packed/quantized gradient rules.

## Acceptance Criteria

1. **PSC-1:** `cargo test -p faber-runtime cross_entropy` passes. Finite-difference oracle matches analytical VJP for at least one rank-2 logits case.
2. **PSC-2:** `faber run -t fmir examples/training/session-exemplum/` produces a loss trace from a composable training loop. The exemplum's README documents the pattern clearly enough for a mid-tier model.
3. **PSC-3:** `north-star-evidence-gate.md` exists with current evidence table, next gates table, and explicit non-claims.

## Validation

```bash
# PSC-1
cargo test -p faber-runtime cross_entropy
cargo test -p faber-runtime autograd_reference_test

# PSC-2
faber run -t fmir examples/training/session-exemplum/

# PSC-3
python3 -c "
import sys
text = open('faber/docs/factory/pytorch-session-continuation/north-star-evidence-gate.md').read()
for term in ['cross-entropy', 'session', 'non-claim', 'dense Tensor<f32>']:
    assert term in text, f'missing: {term}'
print('ok')
"
```

## Open Questions

1. **Should cross-entropy targets be one-hot indices or probability distributions?** Probability distributions are more general and compatible with label smoothing. One-hot indices are simpler for Hands. **Default:** probability distributions (compatible with existing tensor ops, easier to verify with FD oracle). Hand can implement one-hot index convenience on top.

2. **Should the session exemplum use `norma:optimizer` or inline SGD?** `norma:optimizer` has shape-specific SGD but no generic version. Inlining SGD demonstrates the pattern more honestly. **Default:** inline SGD in the exemplum, with a comment pointing to `norma:optimizer` for shape-specific reuse.

3. **Does the evidence gate document belong in `faber/docs/factory/` or `faber-runtime/docs/factory/`?** The autograd roadmap is in `faber/docs/factory/`. The substrate inventory is in `faber-runtime/docs/factory/`. **Default:** co-locate with this goal in `faber/docs/factory/pytorch-session-continuation/` and cross-reference the runtime inventory.

## Stop Conditions

- Stop if cross-entropy VJP requires changes to `radix/crates/radix-air/` (reverse_ad path).
- Stop if the session exemplum tries to import modules not yet available in FMIR targets.
- Stop if any unit claims public PyTorch equivalence or `torch.nn` parity.
- Stop if generic tensor-shape SGD is attempted before Radix shape generics land.
