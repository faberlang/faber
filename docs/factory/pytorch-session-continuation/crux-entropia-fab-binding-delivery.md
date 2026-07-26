# Delivery: Crux Entropia FAB Binding

**Kind**: delivery lowering
**Planner**: planner-2
**Assignment**: 23f35a39
**Goal**: `faber/docs/factory/pytorch-session-continuation/crux-entropia-fab-binding-goal.md`
**Goal verdict**: READY
**Created**: 2026-07-26
**Consumer**: factory Hands (mid-tier implementer)
**Unit count**: 3

---

## Interpreted Unit

PSC-1 (commit `bfba771`) shipped cross-entropy loss at the Rust runtime layer:
`Tensor<f32>::crux_entropia` forward pass, `AutogradOp::CruxEntropia` analytical
VJP, and finite-difference oracle. The runtime is complete and tested. What
remains is the compiler pipeline that connects Faber source code
(`x.crux_entropia(targets)`) to that runtime — the "FAB binding."

This delivery lowers the binding into three path-disjoint implementable units.
All three follow the established tensor-op binding pattern used by Gelu,
Softmax, and LayerNorm. Unit 2 (reverse_ad VJP) is deferred until hand-5
completes BERT diagnostic task 0237b130, which holds single-writer lock on
`reverse_ad.rs`.

## Normalized Spec

Wire the crux-entropia tensor op through the full compiler pipeline:

```text
FAB source (.crux_entropia)
  → Intrinsic registry ("crux_entropia" → AirTensorOp::CruxEntropia)
    → AIR node (AirTensorOp::CruxEntropia variant)
      → MIR lowering (MirCollectionOp::TensorCruxEntropia)
        → MIR stepper / LLVM / Wasm dispatch (runtime crux_entropia call)
          → Eligibility ledger (AirTensorOp + method name entry)
            → Reverse AD VJP walk (BLOCKED — hand-5 lock)
              → Exemplum wiring (train.fab uncomment)
```

The VJP is `(softmax(prediction) - targets) / N` — analytical formula already
verified by the runtime finite-difference oracle in
`faber-runtime/src/autograd_reference_test.rs`.

## Repo-Aware Baseline

All units work from the current main tips:

| Repo | Tip evidence |
|------|-------------|
| `faber-runtime` | `bfba771` — crux_entropia shipped |
| `radix` | `831263b4c` — tip-honesty v24; reverse_ad.rs under hand-5 single-writer |
| `faber` | `0f2ca28` — PSC-3 evidence gate |
| `examples/training` | session-exemplum `train.fab` has commented crux_entropia call |

**Hard constraint**: `radix/crates/radix/src/air/reverse_ad.rs` is single-writer
locked by hand-5 (task 0237b130, BERT C4/C5 diagnostic). Unit 2 must not start
until hand-5 completes and commits. Units 1 and 3 are path-disjoint from
`reverse_ad.rs` and from each other.

## Ordered Unit Graph

### Unit 1: Compiler Pipeline — id `ce-fab-binding-u1`

| Field | Value |
|-------|-------|
| **outcome** | `.crux_entropia(targets)` compiles via FAB → FMIR; forward pass runs on MIR stepper and produces correct cross-entropy loss |
| **write_scope** | `radix/crates/radix/src/intrinsics/registry.rs`, `radix/crates/radix/src/air/nodes.rs`, `radix/crates/radix/src/air/to_mir.rs`, `radix/crates/radix/src/air/generated_differentiable_eligibility.rs`, `radix/crates/radix/src/air/generated_differentiable_eligibility_test.rs`, `radix/crates/radix-mir/src/nodes.rs`, `radix/crates/radix-mir/src/validate.rs`, `radix/crates/radix-mir/src/dump.rs`, `radix/crates/radix-mir-stepper/src/runtime.rs`, `radix/crates/radix-mir-llvm/src/symbols.rs`, `radix/crates/radix-mir-wasm/src/import_names.rs` |
| **forbidden** | `radix/crates/radix/src/air/reverse_ad.rs`, `radix/crates/radix/src/air/reverse_ad_test.rs` — hand-5 single-writer lock |
| **read_scope** | Full radix workspace; `faber-runtime/src/tensor.rs` (crux_entropia signature); `faber-runtime/src/autograd.rs` (AutogradOp::CruxEntropia) |
| **done_when** | (a) `AirTensorOp::CruxEntropia` variant exists in `air/nodes.rs` with method name `"crux_entropia"` and string constant `TENSOR_CRUX_ENTROPIA`; round-trip test in `nodes_test.rs` passes. (b) `MirCollectionOp::TensorCruxEntropia` variant exists in `radix-mir/src/nodes.rs`. (c) Intrinsic registry maps `"crux_entropia"` to `collection TensorCruxEntropia` in `intrinsics/registry.rs`. (d) `air/to_mir.rs` lowers `AirTensorOp::CruxEntropia` to `MirCollectionOp::TensorCruxEntropia`. (e) MIR stepper (`runtime.rs`) dispatches `TensorCruxEntropia` to runtime `tensor_crux_entropia` call. (f) LLVM symbols and Wasm import names map `TensorCruxEntropia` to `"tensor_crux_entropia"`. (g) MIR validate accepts structurally valid `TensorCruxEntropia` calls (2 args: logits + targets). (h) MIR dump formats `TensorCruxEntropia` correctly. (i) `generated_differentiable_eligibility.rs`: `AirTensorOp::CruxEntropia` added to `ALLOWED_TENSOR_OPS` and `ALLOWED_TENSOR_METHODS` with `"crux_entropia"` method name. Test `allowed_tensor_ops_match_air_v1_method_vocabulary_and_mir_floor` updated and passes. (j) `cargo test -p radix-mir-stepper tensor_crux_entropia` passes — forward-only test with known logits/targets produces correct scalar loss matching the runtime FD oracle. |
| **validation** | `cargo test -p radix nodes_test`; `cargo test -p radix generated_differentiable_eligibility`; `cargo test -p radix-mir-stepper tensor_crux_entropia`; `cargo test -p radix-mir` (validate); manual: compile a `.fab` file with `x.crux_entropia(targets)`, verify FMIR output contains `TensorCruxEntropia` op |
| **depends_on** | none |
| **non_goals** | Reverse AD VJP (Unit 2); exemplum wiring (Unit 3); forward-only at this unit — gradients will fail gracefully (eligibility ledger marks CruxEntropia as deferred-VJP like Softmax until Unit 2) |
| **risk** | low — mechanical pattern-follow; 11 files but all trivial additions following Gelu/Softmax precedent; no reverse_ad interaction |

#### Unit 1 Detailed Touchpoints

| File | Change |
|------|--------|
| `intrinsics/registry.rs` | Add `("crux_entropia", collection TensorCruxEntropia)` entry |
| `air/nodes.rs` | Add `CruxEntropia` variant to `AirTensorOp` enum; add `TENSOR_CRUX_ENTROPIA: &str = "crux_entropia"` constant; add match arms in `from_method_name`, `method_name`, `from_str`, and `Display` |
| `air/nodes_test.rs` | Add `(TENSOR_CRUX_ENTROPIA, AirTensorOp::CruxEntropia)` to round-trip test |
| `air/to_mir.rs` | Add match arm: `AirTensorOp::CruxEntropia => MirCollectionOp::TensorCruxEntropia` (2-arg collection op path, same as Gelu/Softmax pattern) |
| `air/generated_differentiable_eligibility.rs` | Add `AirTensorOp::CruxEntropia` to `ALLOWED_TENSOR_OPS`; add `TENSOR_CRUX_ENTROPIA` to `ALLOWED_TENSOR_METHODS` |
| `air/generated_differentiable_eligibility_test.rs` | Add `AirTensorOp::CruxEntropia` and `"crux_entropia"` to expected arrays in `allowed_tensor_ops_match_air_v1_method_vocabulary_and_mir_floor` |
| `radix-mir/src/nodes.rs` | Add `TensorCruxEntropia` variant to `MirCollectionOp` enum |
| `radix-mir/src/validate.rs` | Add `MirCollectionOp::TensorCruxEntropia` to validation match arms (2-arg collection op, single-tensor return like Softmax) |
| `radix-mir/src/dump.rs` | Add `MirCollectionOp::TensorCruxEntropia => "tensor_crux_entropia"` |
| `radix-mir-stepper/src/runtime.rs` | Add `MirCollectionOp::TensorCruxEntropia => self.tensor_crux_entropia(args, span)` dispatch; implement `tensor_crux_entropia` method calling the runtime `crux_entropia` |
| `radix-mir-llvm/src/symbols.rs` | Add `MirCollectionOp::TensorCruxEntropia => "tensor_crux_entropia"` |
| `radix-mir-wasm/src/import_names.rs` | Add `MirCollectionOp::TensorCruxEntropia => "tensor_crux_entropia"` |

---

### Unit 2: Reverse AD VJP Walk — id `ce-fab-binding-u2`

| Field | Value |
|-------|-------|
| **outcome** | Compiler-generated reverse AD produces correct gradient companion for functions containing `crux_entropia`. The VJP walk emits `(softmax(prediction) - targets) / N` in AIR. |
| **write_scope** | `radix/crates/radix/src/air/reverse_ad.rs`, `radix/crates/radix/src/air/reverse_ad_test.rs` |
| **forbidden** | All other radix files (already done in Unit 1); product code changes in faber-runtime, examples, or faber CLI |
| **read_scope** | Full radix workspace; `faber-runtime/src/autograd_reference_test.rs` (FD oracle for formula verification); Unit 1 changes |
| **done_when** | (a) `reverse_ad.rs` handles `AirTensorOp::CruxEntropia` in the differentiable-op gating arm (same as `Mean`, `Gelu`, `Softmax`). (b) `build_vjp_expr` returns the correct VJP expression: `(softmax(prediction_replay) - targets_replay) / N`. The VJP uses the forward input (logits, args[0]) to compute softmax, NOT the forward output (scalar loss). (c) Caller-side gradient accumulation handles the two-input case (logits gradient → accumulate; targets gradient → nil/skip). (d) `reverse_ad_test.rs`: new oracle test `test_crux_entropia_vjp_oracle_matches_tape` verifies the generated VJP matches the runtime FD oracle. Pattern: `sum(crux_entropia(logits, targets))` to make scalar output → AIR transform → compare VJP against finite-difference. (e) `cargo test -p radix reverse_ad` passes — all existing tests + new crux-entropia test green. |
| **validation** | `cargo test -p radix reverse_ad` (all tests including new oracle) |
| **depends_on** | Unit 1 (needs `AirTensorOp::CruxEntropia` to exist); **hand-5 completing task 0237b130** (BERT C4/C5 diagnostic — single-writer lock on `reverse_ad.rs`) |
| **non_goals** | Changing how other ops' VJP walks work; adding crux-entropia to fusion.rs (fusion handles ops already in the eligibility ledger — CruxEntropia will be eligible after this unit, fusion can be a follow-on if needed); WASM/WebGPU gradient execution (CPU reference proof only) |
| **risk** | medium — reverse_ad.rs is a complex file; VJP formula is known and verified but the walk integration has nuance: forward output is a scalar loss but VJP needs the forward logits input to compute softmax; similar to Mean (scalar output, VJP needs element count N); higher risk than Unit 1's mechanical additions |

#### Unit 2 VJP Design Notes

The crux-entropia VJP is not a simple elementwise gradient. Key design points:

1. **Forward inputs**: `(logits: tensor<f32, [B,C]>, targets: tensor<f32, [B,C]>)`
2. **Forward output**: scalar `f32` loss
3. **VJP for logits**: `upstream * (softmax(logits) - targets) / N` where N = batch size (first dimension)
4. **VJP for targets**: not needed (targets are a training data constant, not a parameter)

The walk must:
- Replay the logits input (not the scalar loss output) to compute softmax
- Subtract targets from softmax
- Divide by N (batch size — element count of logits' first dimension)
- Multiply by upstream scalar
- Only accumulate gradient for args[0] (logits); args[1] (targets) gets nil

This pattern is closest to `Mean` (which also needs element count N from the
input type) combined with `LayerNorm` (which needs the forward input for the
VJP computation, not just the output).

---

### Unit 3: Exemplum Wiring — id `ce-fab-binding-u3`

| Field | Value |
|-------|-------|
| **outcome** | Session exemplum `train.fab` uses cross-entropy loss; README documents the working binding; north-star evidence gate reflects complete FAB binding |
| **write_scope** | `examples/training/session-exemplum/src/train.fab`, `examples/training/session-exemplum/README.md`, `faber/docs/factory/pytorch-session-continuation/north-star-evidence-gate.md` |
| **forbidden** | Radix compiler code (already done in U1/U2); faber-runtime (already shipped); other exempla (linear-regression, mlp, bert-tiny-fragment — don't modify their loss functions) |
| **read_scope** | `faber-runtime/src/tensor.rs` (crux_entropia docs for README); Unit 1+2 radix changes; current train.fab |
| **done_when** | (a) `train.fab` line 34-35: `# cross-entropy` comment block uncommented; MSE lines 53-55 replaced with `redde prediction.crux_entropia(target)`. Keep the existing model (2×2 linear) — only swap the loss. (b) `README.md` lines 87-93: "FAB binding pending" note removed; cross-entropy section rewritten as "available and working." (c) `north-star-evidence-gate.md` Gate 1 (Cross-entropy loss) status changed from "Ready (PSC-1)" to "**Shipped (PSC-1 + FAB binding)**" with evidence: runtime bfba771 + radix binding SHA. (d) `faber run -t fmir examples/training/session-exemplum/` produces 8-element decreasing loss trace with cross-entropy loss. |
| **validation** | `faber run -t fmir examples/training/session-exemplum/` — loss trace decreases monotonically; no compile errors; cross-entropy loss value is reasonable (not NaN, not zero) |
| **depends_on** | Unit 1 (needs compiler to accept `.crux_entropia`); Unit 2 (needs `@ radix backward` to generate correct gradients — without gradients, the training loop can't update parameters) |
| **non_goals** | Changing the model architecture; adding a second loss exemplum; creating `norma:loss` package; touching linear-regression/mlp/bert-tiny exempla |
| **risk** | low — simple file edits; primary risk is that the cross-entropy loss surface may need different hyperparameters (learning rate) than MSE to converge; if loss doesn't decrease, adjust lr from 0.01 and note the finding |

---

## Checkpoints and Gates

| Gate | After | Check |
|------|-------|-------|
| Forward compiles | Unit 1 | `cargo test -p radix-mir-stepper tensor_crux_entropia` green; manual `.fab` compile |
| Gradients work | Unit 2 | `cargo test -p radix reverse_ad` green with new oracle test |
| Exemplum trains | Unit 3 | `faber run -t fmir examples/training/session-exemplum/` produces decreasing loss |

## Validation Summary

```bash
# Unit 1
cargo test -p radix nodes_test
cargo test -p radix generated_differentiable_eligibility
cargo test -p radix-mir-stepper tensor_crux_entropia
cargo test -p radix-mir  # validate module

# Unit 2 (deferred — hand-5 must complete first)
cargo test -p radix reverse_ad

# Unit 3 (deferred — requires U1 + U2)
faber run -t fmir examples/training/session-exemplum/
```

## Open Questions for Mind

1. **Unit 2 scheduling**: Does Mind want planner-2 to file a need when hand-5
   completes 0237b130, or should Unit 2 be pre-filed as a task with "BLOCKED"
   status?

2. **Fusion eligibility**: After Unit 2, CruxEntropia is differentiable in
   isolated functions. Fusion of crux-entropia into larger companions is
   follow-on work — not in scope of this delivery. OK to defer?

3. **Learning rate for cross-entropy exemplum**: MSE loss uses lr=0.01 and
   converges in 8 steps. Cross-entropy may need a different lr. The Hand
   should adjust if the loss trace doesn't decrease — OK to change
   hyperparameters as a Unit 3 implementation detail?
