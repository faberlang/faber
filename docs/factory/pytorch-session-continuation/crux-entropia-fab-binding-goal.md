# Goal-Check: Crux Entropia FAB Binding

**Kind**: goal-check (READY — residual from PSC theme)
**Planner**: planner-2
**Assignment**: 23f35a39
**Created**: 2026-07-26
**Consumer**: delivery → factory Hands
**Goal artifact**: this document (goal-check on implicit PSC-residual goal)

---

## Verdict: READY

The crux-entropia FAB binding is a well-bounded implementable goal with clear
architecture, established patterns, and a single known blocking dependency
(reverse_ad single-writer lock by hand-5 on task 0237b130). Non-reverse-ad
units can proceed immediately.

---

## Summary

PSC-1 (commit `bfba771`) shipped `Tensor<f32>::crux_entropia` forward pass +
`AutogradOp::CruxEntropia` analytical VJP + finite-difference oracle in
`faber-runtime`. The runtime layer is complete. What remains is the compiler
pipeline to make `.crux_entropia(targets)` callable from Faber source code:

- No `AirTensorOp::CruxEntropia` variant exists in the radix compiler
- No `MirCollectionOp::TensorCruxEntropia` variant exists in MIR
- No intrinsic registry entry maps the FAB method name to an AIR/MIR op
- No MIR stepper/LLVM/Wasm codegen dispatches to the runtime `crux_entropia`
- No reverse_ad VJP walk exists for compiler-generated gradients
- `examples/training/session-exemplum/src/train.fab` has the call site
  commented out with note "FAB binding pending"

## Problem

Users cannot call `crux_entropia` from Faber code. The runtime exists (PSC-1)
but is unreachable from the language. The session exemplum documents the
intended cross-entropy call site but cannot exercise it.

## Desired End State

A Faber user writes:

```faber
fixum f32 loss ← prediction.crux_entropia(target)
```

and the compiler:

1. Resolves the call through the intrinsic registry
2. Lowers it to an AIR `CruxEntropia` tensor op
3. Emits a `MirCollectionOp::TensorCruxEntropia` MIR call
4. Dispatches to `Tensor<f32>::crux_entropia` at runtime (already shipped)
5. When used inside a `@ radix backward` function, the reverse_ad transform
   generates the correct VJP

## Goals

1. **FAB method surface**: `x.crux_entropia(targets)` compiles in Faber code
2. **Forward pass**: MIR stepper dispatches to runtime `crux_entropia` correctly
3. **Reverse AD**: VJP walk produces correct gradient companion (analytical:
   `(softmax(prediction) - targets) / N`)
4. **Exemplum**: session-exemplum `train.fab` uncomments the cross-entropy
   call site and trains successfully
5. **Evidence gate**: north-star evidence gate updated to reflect FAB binding
   complete

## Non-Goals

- Public cross-entropy API in `norma:loss` — not a product library surface
- Shape-generic cross-entropy — stays at current tensor-shape support level
- Torch cross-entropy parity (weight, ignore_index, label_smoothing, etc.)
- GPU/WebGPU gradient execution — CPU reference proof only
- Sparse/packed cross-entropy — dense `Tensor<f32>` proof boundary only
- Changing the session-exemplum model architecture — loss swap only

## Architecture Direction

Follow the established tensor-op binding pattern used by Gelu, Softmax, and
LayerNorm. The pipeline is:

```text
FAB source (.crux_entropia)
  → Intrinsic registry (method name → AIR op)
    → AIR (AirTensorOp::CruxEntropia)
      → MIR lowering (MirCollectionOp::TensorCruxEntropia)
        → MIR stepper / LLVM / Wasm (call runtime crux_entropia)
          → faber-runtime Tensor<f32>::crux_entropia (PSC-1, already shipped)
            → Reverse AD VJP walk (new, blocked by hand-5)
```

The VJP is `(softmax(prediction) - targets) / N` — already verified by the
runtime finite-difference oracle in `faber-runtime/src/autograd_reference_test.rs`.

## Ground Truth Researched

| Claim | Evidence |
|-------|----------|
| Runtime crux_entropia forward + VJP + FD oracle shipped | `faber-runtime/` commit `bfba771`; `src/tensor.rs` line 811; `src/autograd.rs` line 265; `src/autograd_reference_test.rs` line 1006 |
| No crux_entropia exists in radix compiler | `grep -r "crux_entropia" radix/` returns zero hits |
| No crux_entropia exists in faber CLI | `grep -r "crux_entropia" faber/ --include="*.rs"` returns zero hits |
| Binding pattern established by Gelu | `intrinsics/registry.rs` line 468 (`"gelu" → collection TensorGelu`); `air/nodes.rs` line 243 (`Gelu` variant); `mir/nodes.rs` line 824 (`TensorGelu`); stepper `runtime.rs` line 1611; LLVM `symbols.rs` line 158; Wasm `import_names.rs` line 359 |
| Train.fab has commented crux_entropia call | `examples/training/session-exemplum/src/train.fab` line 34-35 (comment); `README.md` line 87-93 ("FAB binding pending") |
| reverse_ad.rs is single-writer locked | Vivi task `0237b130` — hand-5 BERT C4/C5 Unit 1 diagnostic; write_scope includes `crates/radix/src/air/reverse_ad.rs` |
| Eligibility ledger is independent of reverse_ad.rs | `generated_differentiable_eligibility.rs` imports only `AirTensorOp` from `air/nodes.rs`; reverse_ad.rs has no import of eligibility module |
| VJP formula verified | `faber-runtime/src/autograd.rs`: backward computes `grad * (softmax - targets) / N`; FD oracle in `autograd_reference_test.rs::test_autograd_crux_entropia_gradient` |

## Constraints and Invariants

1. **Do not touch reverse_ad.rs while hand-5 has single-writer lock.** Unit 2
   (reverse_ad VJP) is deferred until hand-5 completes task 0237b130.
2. **Follow existing pattern exactly.** Copy the Gelu/Softmax binding pipeline
   — no novel architecture for a well-precedented op addition.
3. **Dense Tensor<f32> proof boundary.** No sparse, packed, or quantized
   cross-entropy. No device execution.
4. **Forward-first delivery.** Unit 1 ships forward pass only; Unit 2 adds
   gradients. This is the same pattern used for Softmax (forward shipped
   before VJP).
5. **Path-disjoint from hand-5.** Unit 1 write scope provably excludes
   `reverse_ad.rs` and `reverse_ad_test.rs`.

## Implementation Shape

Three path-disjoint units (see delivery spec for full field definitions):

| Unit | ID | What | Blocked? |
|------|----|------|----------|
| Compiler pipeline | `ce-fab-binding-u1` | Intrinsic registry, AIR node, MIR variant, stepper/LLVM/Wasm codegen, MIR validate/dump, eligibility ledger | No |
| Reverse AD VJP | `ce-fab-binding-u2` | VJP walk in reverse_ad.rs + reverse_ad test | Yes — hand-5 lock |
| Exemplum wiring | `ce-fab-binding-u3` | Uncomment train.fab, update README + evidence gate | After U1+U2 |

## Acceptance Criteria

1. `x.crux_entropia(targets)` compiles in a Faber package
2. FMIR output contains `TensorCruxEntropia` op
3. MIR stepper dispatches correctly to runtime `crux_entropia`
4. Forward pass computes correct cross-entropy loss (matches runtime FD oracle)
5. `@ radix backward` generates correct gradient companion (after U2)
6. `cargo test -p radix reverse_ad` passes with crux-entropia VJP oracle test (after U2)
7. Session exemplum trains with cross-entropy loss (after U3)
8. North-star evidence gate updated (after U3)

## Validation

```bash
# Unit 1: forward pass
cargo test -p radix-mir-stepper tensor_crux_entropia
cargo test -p radix generated_differentiable_eligibility
cargo test -p radix nodes_test  # AirTensorOp round-trip

# Unit 2: reverse AD (deferred)
cargo test -p radix reverse_ad

# Unit 3: exemplum (deferred)
faber run -t fmir examples/training/session-exemplum/
```

## Open Questions

None blocking. One scheduling note: Unit 2 depends on hand-5 completing
diagnostic task 0237b130. No architecture uncertainty.

## Stop Conditions

- If the binding pattern requires changes to more than the listed files,
  stop and report scope creep.
- If reverse_ad VJP requires architectural changes beyond the existing
  Mean/LayerNorm pattern, stop and escalate.
- If hand-5's diagnostic changes reverse_ad.rs in ways that conflict with
  Unit 2's planned changes, stop and re-lower.

---

## Goal-Check Checklist

| Category | Status | Note |
|----------|--------|------|
| Desired end state | ✅ Concrete | `x.crux_entropia(targets)` compiles and runs |
| Grounding | ✅ Evidence-mapped | Every claim cites a file path or commit SHA |
| Architecture decisions | ✅ Clear | Follow existing Gelu/Softmax pattern |
| Boundaries | ✅ Defined | Goals, non-goals, stop conditions |
| Acceptance criteria | ✅ Objective | 8 measurable criteria |
| Validation | ✅ Commands given | Unit-specific test commands |
| Implementation handoff | ✅ Path-named | 3 units with exact file lists |
| Open questions | ✅ None blocking | Single scheduling dependency named |
| Staleness | ✅ Current | All evidence verified against live code |

## Recommended Next Step

**Delivery lowering** — the goal is READY. Proceed to unit graph with
path-disjoint write scopes and explicit hand-5 dependency annotation for
Unit 2.
