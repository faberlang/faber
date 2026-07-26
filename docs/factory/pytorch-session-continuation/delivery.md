# Delivery: PyTorch-Session Training Workflow — Next Bounded Units

**Status**: delivery spec for goal `faber/docs/factory/pytorch-session-continuation/goal.md`
**Created**: 2026-07-26
**Goal verdict**: READY
**Consumer**: factory (mid-tier implementer Hand)
**Unit count**: 3

---

## Interpreted Unit

After the autograd-equivalent roadmap A1–A5 closeout (generated backward
companions for linear, MLP, and BERT-tiny training with 8-step inline SGD),
the north-star still lacks:

1. Cross-entropy loss (the dominant classification loss in PyTorch workflows).
   Softmax is forward-only; its VJP is explicitly deferred to "cross-entropy goal."
2. A reusable training session pattern. All 3 training exempla copy-paste the
   same inline SGD pattern across every parameter.
3. A single cold-start document that maps current shipped evidence to next gates.

This delivery lowers those three gaps into bounded, path-disjoint implementable
units. All units stay within the existing dense `Tensor<f32>` proof boundary and
avoid the reverse_ad path in `radix/crates/radix-air/`.

## Normalized Spec

| Unit | ID | What | Paths |
|------|-----|------|-------|
| Cross-entropy loss with softmax VJP | PSC-1 | Runtime-local analytical VJP + FD oracle | `faber-runtime/` |
| Training session exemplum | PSC-2 | Examples-owned composable training loop | `examples/training/` |
| North-star evidence gate doc | PSC-3 | Cold-start evidence map for future Hands | `faber/docs/factory/` |

All three are path-disjoint from each other and from reverse_ad. PSC-2
logically depends on PSC-1 for a second loss function but can ship with
MSE-only in parallel.

## Repo-Aware Baseline

Current shipped state (verified 2026-07-26):

| Surface | State |
|--------|-------|
| `faber-runtime/src/tensor.rs` | `softmax()` — forward-only, rank-1 and rank-2 row-wise. No `crux_entropia`. |
| `faber-runtime/src/autograd.rs` | `AutogradTape` with VJP for add/sub/mul/divide/matmul/neg/forma/permute/sectio/media/layernorm/gelu/relu/sqrt. No `CruxEntropia` op. |
| `faber-runtime/src/autograd_reference_test.rs` | FD oracle for all existing autograd ops. No cross-entropy test. |
| `faber-runtime/docs/factory/autograd-substrate-inventory.md` | Line 76: "Softmax forward-only (VJP deferred to cross-entropy goal)." |
| `norma/src/optimizer.fab` | Shape-specific `sgd_step` for `[4]`, `[2,2]`, `[1,2]`, `[]`. Line 12: "Shape generics blocked by radix tip." |
| `examples/training/linear-regression/` | 2×2 linear+MSE, 8-step inline SGD. ~10 lines of SGD per parameter. |
| `examples/training/mlp/` | Two-layer MLP with GELU, 4 trainable params, 8-step inline SGD. |
| `examples/training/bert-tiny-fragment/` | Single-layer BERT-tiny, 22 trainable params, 16 differentiable ops, 8-step inline SGD. |
| `faber/docs/factory/autograd-equivalent-roadmap/deck.md` | A1–A6 roadmap; A1–A5 substantially closed. |
| `faber/docs/factory/pytorch-session-continuation/` | New directory (this delivery). |

## Ordered Unit Graph

```
PSC-3 (docs) ── independent, parallel with PSC-1
PSC-1 (cross-entropy) ── independent, first implement
PSC-2 (session exemplum) ── after PSC-1 (logical dep), or parallel with MSE-only
```

No unit depends on another unit's write scope. PSC-2's logical dependency on
PSC-1 is for the cross-entropy loss function; if PSC-1 is delayed, PSC-2 can
ship with MSE as the sole loss and add cross-entropy in a follow-up.

---

## PSC-1: Cross-Entropy Loss with Softmax VJP

| Field | Value |
|-------|-------|
| **id** | PSC-1 |
| **outcome** | `Tensor<f32>::crux_entropia` forward + `AutogradOp::CruxEntropia` analytical VJP + finite-difference oracle in `faber-runtime`. Softmax VJP is no longer deferred. |
| **write_scope** | `faber-runtime/src/tensor.rs`, `faber-runtime/src/autograd.rs`, `faber-runtime/src/autograd_reference_test.rs`, `faber-runtime/docs/factory/autograd-substrate-inventory.md` |
| **read_scope** | `faber-runtime/src/tensor.rs` (existing softmax), `faber-runtime/src/autograd.rs` (existing VJP patterns), `radix/docs/design/domain-primitive-policy.md` (domain validation policy) |
| **done_when** | (a) `Tensor<f32>::crux_entropia(logits, targets)` exists — forward: softmax(logits) → -sum(targets * log(softmax + ε)) / N. Domain validation: non-empty, all finite, targets in [0,1], ε=1e-7. (b) `AutogradOp::CruxEntropia` variant in `autograd.rs` — backward computes `grad * (softmax - targets) / N` with broadcast reduction. (c) `cargo test -p faber-runtime crux_entropia` passes — forward returns correct scalar loss for a known logits/targets pair. (d) `cargo test -p faber-runtime autograd_reference_test` passes — FD oracle matches analytical VJP for at least one rank-2 logits case (e.g. 3-class, 2-batch). (e) `autograd-substrate-inventory.md` updated: cross-entropy row added, Softmax VJP deferred text removed. |
| **validation** | `cargo test -p faber-runtime crux_entropia`; `cargo test -p faber-runtime autograd_reference_test` |
| **depends_on** | none |
| **non_goals** | No host ABI symbol. No generated AIR backward. No sparse/packed/quantized gradient. No `norma:loss` package. No one-hot index convenience (probability distribution targets only). |
| **risk** | **low** — well-known analytical VJP (`dL/dlogits = softmax - targets`). Follows existing quotient-rule + broadcast-reduction pattern. FD oracle pattern exists for all other ops. |

### PSC-1 Implementation Notes

**Forward (`Tensor<f32>::crux_entropia`):**

```rust
// Pseudocode — follow existing tensor.rs patterns
pub fn crux_entropia(&self, targets: &Tensor<f32>) -> Result<f32, TensorError> {
    // Domain validation: non-empty, finite inputs, targets in [0,1]
    // 1. Compute softmax over last axis
    // 2. Compute -sum(targets * ln(softmax + 1e-7)) / N
    // 3. Return scalar loss
}
```

**Autograd (`AutogradOp::CruxEntropia`):**

```rust
// Pseudocode — follow existing autograd.rs VJP patterns
AutogradOp::CruxEntropia { logits_id, targets_id, saved_softmax }
// backward: let n = logits.shape.last(); grad * (saved_softmax - targets) / n
// broadcast-reduce if targets is broadcast-compatible
```

**FD oracle test:**

```rust
// Pseudocode — follow existing autograd_reference_test.rs patterns
// Fixed logits: [[2.0, 1.0, 0.0], [1.0, 2.0, 1.0]]  (2-batch, 3-class)
// Fixed targets: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]  (one-hot)
// Forward loss: compare against hand-computed cross-entropy
// FD oracle: perturb each logit element, compare autograd gradient
```

**Inventory update (`autograd-substrate-inventory.md`):**

- Line 76: Change "Softmax forward-only (VJP deferred to cross-entropy goal)" to "Softmax VJP integrated via cross-entropy loss (`crux_entropia`)."
- Add cross-entropy row to the existing substrate table (between gelu and softmax): "Cross-entropy loss | `Tensor<f32>::crux_entropia` with analytical VJP `softmax - targets`; FD oracle verified. | `src/tensor.rs`; `src/autograd.rs`; `src/autograd_reference_test.rs`"

---

## PSC-2: Reusable Training Session Exemplum

| Field | Value |
|-------|-------|
| **id** | PSC-2 |
| **outcome** | An `examples/training/session-exemplum/` package that demonstrates a composable training loop: define model + loss + optimizer step → loop with loss trace. Reduces copy-paste across training exempla. |
| **write_scope** | `examples/training/session-exemplum/` (new directory: `faber.toml`, `src/train.fab`, `README.md`) |
| **read_scope** | `examples/training/linear-regression/`, `examples/training/mlp/`, `norma/src/optimizer.fab` |
| **done_when** | (a) `examples/training/session-exemplum/` directory exists with `faber.toml`, `src/train.fab`, and `README.md`. (b) `src/train.fab` compiles and runs: `faber run -t fmir examples/training/session-exemplum/` produces a loss trace. (c) The training loop shows a composable pattern: model function, loss function, optimizer step, loop body. Inline SGD is acceptable; the pattern is what matters. (d) `README.md` documents the pattern clearly: what each component is, how to swap model/loss/optimizer, explicit non-claims (not a product library, not generic tensor-shape SGD). |
| **validation** | `faber run -t fmir examples/training/session-exemplum/` |
| **depends_on** | PSC-1 (logical — for cross-entropy as second loss). Can ship with MSE-only if PSC-1 is delayed; add cross-entropy section in follow-up. |
| **non_goals** | No `norma:training` package. No generic tensor-shape SGD. No dataloader abstraction. No checkpointing. No product optimizer API. No `norma:loss` package. |
| **risk** | **low** — extracts existing pattern from 3 training exempla. No new runtime code. Only risk is that `faber run -t fmir` may surface MIR limitations on the specific model shape chosen. |

### PSC-2 Implementation Notes

**Recommended model** for the exemplum: reuse the 2×2 linear+MSE from
`linear-regression/` as it's the simplest existing training proof. This
keeps the exemplum focused on the **pattern**, not the model complexity.

**Pattern structure:**

```text
# session-exemplum/src/train.fab

# 1. Model: forward + backward companion (copy from linear-regression or define 2×2 linear)
#    @ radix lane "air"
#    @ radix backward "model_backward"
#    functio model_loss(input, weight, bias, target) → f32 { ... }

# 2. Training step: compose model, loss, optimizer
#    functio training_step(params..., lr) → (loss, updated_params...) {
#        loss = model_loss(...)
#        grads = model_backward(..., nil(), 1.0)
#        // inline SGD per param (document as the current best practice)
#        return (loss, updated_params...)
#    }

# 3. Loop: 8 steps with loss trace
#    incipit {
#        for step in 0..8 {
#            (loss, params...) = training_step(params..., lr)
#            loss_trace.append(loss)
#        }
#        nota loss_trace
#    }
```

**README.md sections:**

1. Pattern overview: model + loss + optimizer = training step
2. How to swap the model (define new `@ radix backward` function)
3. How to swap the loss (MSE vs cross-entropy)
4. How the optimizer step works (inline SGD, reference to `norma:optimizer`)
5. Explicit non-claims
6. Validation command

---

## PSC-3: North-Star Evidence Gate Document

| Field | Value |
|-------|-------|
| **id** | PSC-3 |
| **outcome** | A single durable document (`north-star-evidence-gate.md`) that maps current shipped evidence to the next north-star gates for future Hands. |
| **write_scope** | `faber/docs/factory/pytorch-session-continuation/north-star-evidence-gate.md` |
| **read_scope** | `faber/docs/factory/autograd-equivalent-roadmap/deck.md`, `faber-runtime/docs/factory/autograd-substrate-inventory.md`, `examples/training/`, `examples/gpu-workload/README.md` |
| **done_when** | (a) Document exists at the path above. (b) Contains: shipped evidence table (at least 6 rows), next gates table (at least 4 rows), explicit non-claims section. (c) Self-check script passes: all required terms present. |
| **validation** | `python3 -c "text = open('faber/docs/factory/pytorch-session-continuation/north-star-evidence-gate.md').read(); assert all(t in text for t in ['cross-entropy', 'session', 'non-claim', 'dense Tensor<f32>']); print('ok')"` |
| **depends_on** | none |
| **non_goals** | No product code. No new runtime facts. No Hand tasking. |
| **risk** | **low** — documentation only. All evidence already exists in shipped code/docs. |

### PSC-3 Implementation Notes

**Required sections:**

1. **Current North-Star Position** — 2–3 sentence summary of where we are
2. **Shipped Evidence Table** — 6+ rows: generated backward companions, 3 training exempla, BERT-tiny 22-param proof, 16 differentiable ops, softmax forward, layernorm VJP, gelu VJP, SGD shape-specific in norma, runtime autograd inventory, view-alias policy
3. **Next Gates Table** — 4+ rows mapping gap → status → estimated unit size. Include: cross-entropy loss (PSC-1), training session pattern (PSC-2), dataloader abstraction, checkpointing, generic tensor-shape SGD (blocked on Radix), optimizer library, inference session runtime
4. **Explicit Non-Claims** — public PyTorch parity, `torch.nn` parity, GPU training, CUDA/WebGPU/Metal gradient execution, sparse/packed/quantized gradients, model serialization, tokenizer runtime, LLM inference
5. **Cold-Start Path** — which file to read first, which test to run, which exemplum to study

---

## Checkpoints And Gates

| Gate | After | Check |
|------|-------|-------|
| PSC-1 complete | Cross-entropy VJP landed | `cargo test -p faber-runtime crux_entropia` green; inventory doc updated |
| PSC-2 complete | Session exemplum landed | `faber run -t fmir examples/training/session-exemplum/` produces loss trace |
| PSC-3 complete | Evidence gate doc landed | Self-check script passes |
| Theme close | All 3 units landed | Goal acceptance criteria met; `autograd-substrate-inventory.md` no longer says "VJP deferred" |

## Validation Summary

```bash
# PSC-1 — cross-entropy loss
cargo test -p faber-runtime crux_entropia
cargo test -p faber-runtime autograd_reference_test

# PSC-2 — session exemplum
faber run -t fmir examples/training/session-exemplum/

# PSC-3 — evidence gate doc
python3 -c "
text = open('faber/docs/factory/pytorch-session-continuation/north-star-evidence-gate.md').read()
for term in ['cross-entropy', 'session', 'non-claim', 'dense Tensor<f32>']:
    assert term in text, f'missing: {term}'
print('ok')
"
```

## Open Questions For Mind

1. **Should PSC-2 use cross-entropy or MSE?** If PSC-1 lands first, the exemplum can demonstrate both losses. If PSC-1 is delayed, ship PSC-2 with MSE and add cross-entropy in follow-up. **Recommendation:** ship PSC-1 first (smallest unit), then PSC-2 with both losses.

2. **Does PSC-2 need a `norma:optimizer` generalization?** The `norma/src/optimizer.fab` has shape-specific SGD. A generic version is blocked on Radix shape generics. PSC-2 should inline SGD and document the blocker. **Recommendation:** inline SGD, note blocker in README.

3. **Should PSC-3 be filed as a separate Hand task?** PSC-3 is documentation only and very small. It could be folded into the implementing Hand's closeout or filed as a standalone unit. **Recommendation:** file as standalone — it's path-disjoint and can be done by any Hand while PSC-1/PSC-2 are in flight.
