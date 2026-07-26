# North-Star Evidence Gate: PyTorch-Near Session Training

**Status:** Cold-start evidence map for future Hands
**Created:** 2026-07-26
**Consumer:** Factory Hands (PSC-1, PSC-2, subsequent training-loop units)
**Verification:** Self-check script in `faber/docs/factory/pytorch-session-continuation/delivery.md` §Validation Summary

---

## 1. Current North-Star Position

Faber's runtime reverse-mode autograd scaffold (dense `Tensor<f32>` tape at `faber-runtime/src/autograd.rs`) together with finite-difference oracles, three training exempla, and a BERT-tiny 22-parameter gradient proof forms a bounded CPU reference gradient surface. The north-star gap is a composable training **session** pattern that replaces copy-paste inline SGD, plus the dominant classification loss (**cross-entropy** with softmax VJP) needed before any PyTorch-near training workflow is credible.

---

## 2. Shipped Evidence Table

| # | Evidence | What it proves | Source |
|---|----------|---------------|--------|
| 1 | Generated backward companions (A3–A5) | Compiler-owned reverse-mode gradient for scalar linear loss `(x*weight - target)^2` through a dense `Tensor<f32>` graph with add/sub/mul/matmul/summa. Validated against CPU finite-difference oracle. | `faber-runtime/src/autograd_reference_test.rs`; `examples/training/rung-4-autograd.fab`; `faber/docs/factory/autograd-equivalent-roadmap/deck.md` §Slide 6 (A3–A5) |
| 2 | Linear regression training exemplum | 2×2 linear + MSE loss, 8-step inline SGD, loss trace decreases. Simplest training proof. | `examples/training/linear-regression/src/train.fab` |
| 3 | MLP training exemplum | Two-layer MLP with GELU activation, 4 trainable params, 8-step inline SGD. Proves multi-layer gradient flow. | `examples/training/mlp/src/train.fab` |
| 4 | BERT-tiny fragment training exemplum | Single-layer BERT-tiny, 22 trainable params, 16 differentiable ops, 8-step inline SGD. Largest differentiable graph compiled and run. | `examples/training/bert-tiny-fragment/src/train.fab` |
| 5 | 16 differentiable ops in autograd tape | Add, sub, mul, divide, matmul, neg, forma (reshape), permute, sectio (slice), media (mean), summa, layernorm, gelu, relu, sqrt, scala. All with analytical VJP and finite-difference oracle. | `faber-runtime/src/autograd.rs`; `faber-runtime/src/autograd_reference_test.rs` |
| 6 | Softmax forward (VJP deferred) | `Tensor<f32>::softmax()` with numerical stability, rank-1 and rank-2 row-wise. Forward-only — VJP explicitly deferred to **cross-entropy** goal. | `faber-runtime/src/tensor.rs`; `faber-runtime/docs/factory/autograd-substrate-inventory.md` line 76 |
| 7 | LayerNorm with analytical VJP | `Tensor<f32>::layernorm` with domain validation; `AutogradOp::LayerNorm` per Ba et al. 2016; finite-difference oracle verified. | `faber-runtime/src/autograd.rs`; `faber-runtime/src/autograd_reference_test.rs::test_autograd_layernorm_gradient` |
| 8 | GELU activation with analytical VJP | `Tensor<f32>::gelu()` with finite-input validation; `AutogradOp::Gelu` with inline `f32::tanh()`; FD oracle verified. | `faber-runtime/src/tensor.rs`; `faber-runtime/src/autograd.rs`; `faber-runtime/src/autograd_reference_test.rs::test_autograd_gelu_gradient` |
| 9 | Shape-specific SGD in `norma` | `norma/src/optimizer.fab` defines `sgd_step` for `[4]`, `[2,2]`, `[1,2]`, `[]` shapes. No generic tensor-shape SGD — blocked on Radix shape generics. | `norma/src/optimizer.fab` line 12 |
| 10 | Runtime autograd substrate inventory | Canonical inventory of all shipped autograd evidence, remaining blockers, and dense primitive gap matrix. Documents every op's FD oracle status. | `faber-runtime/docs/factory/autograd-substrate-inventory.md` |
| 11 | View/alias autograd policy | Fail-closed leaf rule (raw views rejected at tape boundary), tape-owned `sectio` scatter-add, AIR consistency statement. Eight boundary tests. | `faber-runtime/docs/factory/view-alias-autograd-policy.md`; `faber-runtime/src/autograd.rs` |
| 12 | GPU workload oracle contracts | Rung 3 (linear backward reference oracle) and rung 4 (toy train/session oracle) document future AIR gradient acceptance targets. No device gradient execution. | `examples/gpu-workload/README.md`; `examples/gpu-workload/rung-3-linear-backward.ref.json`; `examples/gpu-workload/rung-4-toy-train.ref.json` |

All evidence operates within the dense Tensor<f32> proof boundary: contiguous materialized tensors, no sparse/packed carriers, no device execution, no host ABI gradient handles.

**12 rows** — covers generated backward companions, all three training exempla, BERT-tiny proof, differentiable op count, softmax forward, layernorm VJP, gelu VJP, SGD shape-specific in norma, runtime autograd inventory, view-alias policy, and GPU workload contracts.

---

## 3. Next Gates Table

| # | Gate | Gap description | Status | Estimated unit size | Dependency |
|---|------|----------------|--------|-------------------|------------|
| 1 | **Cross-entropy loss with softmax VJP** | `Tensor<f32>::crux_entropia` forward + `AutogradOp::CruxEntropia` analytical VJP + FD oracle. Softmax VJP no longer deferred. | **Ready (PSC-1)** | `faber-runtime/` — ~80 lines Rust + test | None — independent |
| 2 | **Composable training session pattern** | Reusable training loop exemplum replacing copy-paste inline SGD. Model function + loss function + optimizer step + loop body. | **Ready (PSC-2)** | `examples/training/session-exemplum/` — ~3 files, ~100 lines Faber code | Logical: PSC-1 (can ship MSE-only if delayed) |
| 3 | **Dataloader abstraction** | Bounded iteration over training data: batch slicing, shuffle, epoch management. No product library. | **Not started** | `examples/` or `norma/` — medium | After session pattern (PSC-2) |
| 4 | **Checkpointing** | Save/restore parameter state between training runs. No model serialization format. | **Not started** | `examples/` or `norma/` — medium | After dataloader |
| 5 | **Generic tensor-shape SGD** | Replace shape-specific `sgd_step` with a shape-generic version. Blocked on Radix shape generics. | **Blocked** | `norma/src/optimizer.fab` — small edit | Radix shape generics milestone |
| 6 | **Optimizer library** | Beyond SGD: Adam, AdamW, momentum variants. No product optimizer API commitment. | **Not started** | `norma/` — large | After generic tensor-shape SGD |
| 7 | **Inference session runtime** | Model load, forward pass, output extraction. Links to existing inference-session-boundary docs. | **Not started** | `faber-runtime/` + `examples/` — large | After training session pattern |

**7 rows** — covers cross-entropy loss, training session pattern, dataloader, checkpointing, generic SGD, optimizer library, and inference session runtime.

---

## 4. Explicit Non-Claims

The following are **not** claimed by any shipped evidence in this gate document:

| non-claim | Rationale |
|-----------|-----------|
| Public PyTorch parity | No `torch.*` API surface, no `torch.nn.Module` hierarchy, no `torch.optim` equivalence. The dense `Tensor<f32>` tape is a reference proof, not a PyTorch replacement. |
| `torch.nn` parity | No `nn.Linear`, `nn.Dropout`, `nn.Transformer`, `nn.Embedding` or any other `torch.nn` module. GELU, LayerNorm, ReLU, and Sqrt are runtime-local primitives, not `nn` equivalents. |
| GPU training | All shipped gradients are CPU reference proofs. No CUDA, WebGPU, or Metal gradient execution. GPU workload oracle contracts (rungs 3–4) are acceptance targets, not shipped evidence. |
| CUDA / WebGPU / Metal gradient execution | Host gradient handle infrastructure exists for WebGPU (opaque `u32` buffer handles) but no device kernel computes a gradient. Wasm gradient handles are deferred. |
| Sparse / packed / quantized gradients | `Sparsa<T>` and `PackedU4Block` are bridge materialization surfaces only. No sparse or quantized gradient rules exist in the tape or AIR. |
| Model serialization | No GGUF, ONNX, safetensors, or custom model format support. No save/restore for trained parameters beyond test-only manual copy. |
| Tokenizer runtime | No tokenizer, vocabulary, or embedding table integration in the training loop. |
| LLM inference | BERT-tiny fragment is a gradient proof, not an inference runtime. No autoregressive generation, KV cache, or sampling. |
| Optimizer / session product API | The test-only `TestOnlySgdSession` oracle and inline SGD pattern are not a product optimizer or training session API. |
| Generated AIR backward for all ops | AIR-generated backward exists only for the A3–A5 linear loss seed. Ops like GELU (tanh), LayerNorm, and cross-entropy (when implemented) remain runtime-local only. |

---

## 5. Cold-Start Path

A future Hand landing on this document should read in this order:

1. **`faber/docs/factory/pytorch-session-continuation/delivery.md`** — The parent delivery spec. Describes all three units (PSC-1, PSC-2, PSC-3), their scopes, dependencies, and done-when criteria.

2. **`faber-runtime/docs/factory/autograd-substrate-inventory.md`** — Canonical inventory of every shipped autograd op, blocker, and gap. The dense primitive gap matrix (lines 80–90) ranks the next implementation candidates.

3. **`faber/docs/factory/autograd-equivalent-roadmap/deck.md`** — The A1–A6 roadmap that preceded this session-continuation work. Documents how A1–A5 were closed and why A6 (GPU handoff) remains future work.

4. **`faber-runtime/src/autograd_reference_test.rs`** — The finite-difference oracle file. Run `cargo test -p faber-runtime autograd_reference_test` to see every verified gradient proof.

5. **`examples/training/linear-regression/src/train.fab`** — The simplest training exemplum. ~50 lines of Faber code. Start here to understand the current inline-SGD pattern that PSC-2 replaces.

6. **`examples/training/bert-tiny-fragment/src/train.fab`** — The most complex training exemplum (22 params, 16 ops). Read after linear-regression to understand the scaling challenge.

7. **Run `python3 examples/gpu-workload/scripta/check-gpu-workload-contracts.py`** — Validates the GPU workload oracle contracts are in sync with shipped evidence.

### Quick-Start Commands

```bash
# Verify the finite-difference oracle covers all shipped ops
cargo test -p faber-runtime autograd_reference_test

# Compile and run the linear regression exemplum
faber run -t fmir examples/training/linear-regression/

# Compile and run the MLP exemplum
faber run -t fmir examples/training/mlp/

# Compile and run the BERT-tiny fragment
faber run -t fmir examples/training/bert-tiny-fragment/
```

### Where To Start Implementing

- **If you are implementing PSC-1 (cross-entropy):** read `faber-runtime/src/tensor.rs` (softmax forward pattern), then `faber-runtime/src/autograd.rs` (existing VJP pattern for layernorm/gelu), then `faber-runtime/src/autograd_reference_test.rs` (FD oracle pattern). Write scope: `faber-runtime/src/tensor.rs`, `faber-runtime/src/autograd.rs`, `faber-runtime/src/autograd_reference_test.rs`, `faber-runtime/docs/factory/autograd-substrate-inventory.md`.

- **If you are implementing PSC-2 (training session exemplum):** read `examples/training/linear-regression/src/train.fab` first, then `norma/src/optimizer.fab` for existing shape-specific SGD. Write scope: `examples/training/session-exemplum/`.
