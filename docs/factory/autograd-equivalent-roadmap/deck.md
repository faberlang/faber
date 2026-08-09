# Deck: Autograd-Equivalent Roadmap

**Status**: active — milestone met on compiler/CPU path; remaining next units re-derived on the same lane (evidence record refreshed 2026-08-08).
**Created**: 2026-07-14
**Refreshed**: 2026-07-31
**Target workspace**: `/Users/ianzepp/work/faberlang`
**Factory artifact dir**: `faber/docs/factory/autograd-equivalent-roadmap/`
**Primary surfaces**: Faber factory planning, `faber-runtime` dense tensor autograd scaffold (test-only oracle, deprecated tape path), Radix AIR→MIR reverse-AD transform (`radix/docs/factory/mir-autograd/`), examples GPU workload oracles and training exempla, future Faber session boundary.

---

## Slide 1 - Ask

> Estimate how close Faber is to a minimal autograd-equivalent proof and define
> the smallest honest PyTorch-near milestone.

**Answered 2026-07-25.** Faber now owns a reverse-mode gradient path for a
dense scalar-loss tensor graph: a composed Faber function annotated
`@ radix backward` emits a correct multi-op backward companion through AIR → MIR
(the `mir-autograd` campaign, closed 2026-07-25), validated against the
runtime-tape finite-difference oracle. Session evidence exists as training
exempla (linear+MSE, MLP, BERT-tiny fragment, cross-entropy), and the GPU
workload floor is the downstream device-lane acceptance target.

The milestone was never "PyTorch parity." It was a narrow proof that Faber can
own a reverse-mode gradient path, validate it against a CPU oracle, and preserve
enough session evidence to plan the next training-loop step. All three clauses
now hold; the device lane (GPU gradient execution) does not.

## Slide 2 - Guardrail

This roadmap is evidence and sequencing only.

Non-claims (as written 2026-07-14; stale items struck and annotated):

- no public PyTorch replacement;
- no `torch.nn` parity matrix;
- ~~no generated AIR autodiff yet~~ — **stale since 2026-07-25**: generated
  AIR/MIR autodiff exists for all 15 AIR tensor ops (see
  `radix/docs/factory/mir-autograd/mir-autograd-closeout.md`);
- no optimizer/session API support — still true; `norma` has concrete
  `sgd_step` overloads (`[4]`, `[2,2]`, `[1,2]`, scalar `[]`), not a product
  optimizer API;
- no GPU training loop — still true (device lane);
- no CUDA/WebGPU/Metal gradient execution — still true; GPU workload rungs 3–4
  stop at `DeviceStagingFailed`; the WebGPU sibling-lane `OutputChecked` proof
  (`ed77b5d`) is forward matmul+add, not gradient execution;
- no sparse, packed, quantized, or model-format autograd — still true.

## Slide 3 - Shipped Evidence To Start From

The 07-14 rows below remain valid as the **runtime-tape substrate**. The tape is
now formally governed by `faber-runtime/docs/factory/view-alias-autograd-policy.md`
(active product law; **deprecated tape path**): it stays the test-only oracle,
never the product path.

| Required evidence | Current shipped fact | Source |
| --- | --- | --- |
| Finite-difference oracle | Runtime-local central-difference checks cover scalar, same-shape vector, broadcast-bias, and dense linear training-step gradients. | `faber-runtime/docs/factory/autograd-substrate-inventory.md`; `faber-runtime/src/autograd_reference_test.rs` |
| Broadcast reductions | The internal tape reduces broadcast gradients for add/sub/mul and has a broadcast-bias oracle. | `faber-runtime/src/autograd.rs`; `faber-runtime/src/autograd_reference_test.rs` |
| view/alias policy | Tape has tape-owned `sectio` scatter-add **and** fail-closed rejection of raw aliased view leaves; compiler lane rejects views via AIR purity. | `faber-runtime/docs/factory/view-alias-autograd-policy.md`; `faber-runtime/docs/factory/autograd-substrate-inventory.md` |
| matmul/linear VJP | Dense rank-2 matmul exists; the runtime autograd scaffold computes matmul VJPs with a private transpose helper. | `faber-runtime/docs/factory/autograd-substrate-inventory.md`; `faber-runtime/src/autograd.rs` |
| Manual update oracle | Test-only `TestOnlySgdSession` applies manual `param -= learning_rate * grad` updates to weight and bias. | `faber-runtime/src/autograd_reference_test.rs` |
| optimizer/session boundary | Faber inference/session docs define contract-only session and oracle manifest boundaries, not runtime execution. | `faber/docs/factory/inference-session-boundary/goal.md` (proposed, contract-only) |
| loss-trace oracle | A two-step linear session loss trace matches the finite-difference trace and strictly decreases. | `faber-runtime/src/autograd_reference_test.rs` |

**Shipped since the 07-14 write — compiler-side evidence (the product path):**

| Required evidence | Shipped fact | Source |
| --- | --- | --- |
| Compiler-owned reverse-mode transform | `@ radix backward` on a composed Faber function emits a multi-op backward companion through AIR → MIR; all 15 AIR tensor ops differentiable (Add, Sub, Mul, Div, Neg, Sum, Mean, MatMul, Exp, Log, Sqrt, GELU, Tanh, LayerNorm, Softmax); Fill intentionally rejected. | `radix/docs/factory/mir-autograd/mir-autograd-closeout.md` (closed 2026-07-25); `crates/radix/src/air/reverse_ad.rs` |
| Expression `ad` MIR lowering | `ad` routes lower to sermo MIR (`lower_ad_expr` → `MirIntrinsic::SermoOpen`); all five GPU workload rungs now reach `MirLowered`. | `crates/radix/src/mir/lower/runtime.rs:1875` (`1f41facfb`, 2026-07-09) |
| Generated-gradient oracle | Generated companions match the runtime-tape finite-difference oracle across Stage A–F tests. | `a459c2cdd` (A+B) … `ae99fccba` (F5) |
| Training-loop loss trace | Linear+MSE loop compiles forward+backward+SGD through MIR; ≥8-step loss trace matches FD oracle and strictly decreases. Two-layer MLP (linear→GELU→linear→MSE) landed with per-step oracle parity. | `examples/training/linear-regression/`, `examples/training/mlp/` (`4931c3a`), `faber-runtime/tests/compiler_generated_training_test.rs` (`800422f`, per-step parity `e4389fd`) |
| Transformer-scale fragment | BERT-tiny training fragment Units 1–2 landed (gates C0–C6 PASS); largest generated companion to date (~25 gradients, 8-step loop). | `examples/training/bert-tiny-fragment/` (`f3c52fd`), `faber-runtime/tests/compiler_generated_bert_tiny_test.rs` (`88dbb57`); U3 `996bfae0c` tip-ACCEPT, loss 1.634→1.471 within FD tolerance |
| Cross-entropy loss | `CruxEntropia` VJP walk + oracle + session exemplum landed and accepted. | radix `b32ce3742`/`8d0913de8`, examples `2b99f4c`, faber `961af03` |
| GPU workload floor (first measurement) | All rungs `MirLowered`; rung 0 stops `LaunchContractFailed`, rungs 1–4 `DeviceStagingFailed`; output floors pinned 0, ceiling 5; rung-1 WebGPU sibling-lane `OutputChecked` claim under reconciliation. | `radix/docs/factory/gpu-workload-floor/baseline-ledger.md` (remeasured 2026-07-22); `rung-remeasure-reconcile-delivery.md` (2026-07-31) |
| PyTorch-equivalence ladder | Strategy deck complete; ladder stages 6–7 (AIR autograd, toy training) now satisfied on the compiler/CPU path; stage 5 (device execution floor) remains open. | `radix/docs/factory/pytorch-equivalence-ladder/goal.md` (complete 2026-07-14) |
| Tensor semantics foundation | MIR-first tensor semantics contract and 13-stage tensor systems timeline complete. | `radix/docs/factory/tensor-semantics-contract/goal.md` (2026-07-08); `radix/docs/factory/tensor-systems-timeline/CAMPAIGN.md` (2026-07-08) |

## Slide 4 - Current Proximity Read

The 07-14 read ("closer to a minimal proof than to a full ML stack", with AIR
gradients unimplemented) is **superseded** on the compiler/CPU path:

- generated AIR→MIR reverse-mode gradients exist and match the FD oracle
  (15-op set, `mir-autograd` complete 2026-07-25);
- training-loop evidence exists at three scales: linear+MSE (Stage E), MLP
  (two-layer, GELU), BERT-tiny fragment (transformer-scale companion);
- control-flow AD (G.1) and interprocedural AD (H.1) landed and were
  Mind-accepted; the fusion pass (differentiate-before-fuse, ADR `ee3c00a3a`)
  landed after them; G.2 (control-flow emit) is deferred indefinitely;
- the runtime tape remains the test-only oracle and is formally deprecated as
  a product path.

What has **not** moved since 07-14: the device lane. GPU workload rungs 3–4
(backward pass, training loop) still stop at `DeviceStagingFailed`; the
differentiated kernel's device path and the placement gate (Gate 3) are
unproven. The honest remaining milestone is therefore the **device path for
rung 3/4**, not more compiler autograd.

## Slide 5 - Smallest Honest Milestone

> Given a materialized dense `Tensor<f32>` scalar-loss graph using add/sub/mul,
> broadcast add/sub/mul, rank-2 matmul, and `summa`, produce reverse-mode
> gradients that match a CPU finite-difference oracle for a single linear layer
> or tiny MLP, then replay one or two manual parameter updates and verify the
> loss trace decreases.

**MET 2026-07-25 on the compiler/CPU path.** The generated-companion gradient
set now exceeds the milestone: all 15 AIR tensor ops differentiable, linear+MSE
and two-layer MLP oracle-checked, BERT-tiny fragment oracle-checked at 8 steps.
The manual-update and loss-trace clauses are satisfied by Stage E3 (≥8 SGD
steps, strictly decreasing, FD-matched).

Boundary (unchanged, except where noted):

- inputs are contiguous materialized dense tensors — unchanged;
- no `sectio` view leaves — enforced by AIR purity on the compiler path;
  the tape has tape-owned `sectio` scatter-add and rejects raw aliased views;
- no mutation after graph capture — unchanged (AIR purity);
- no sparse or packed tensors — unchanged;
- no public optimizer API — unchanged (concrete `sgd_step` overloads only);
- no GPU/device execution — **unchanged; this is the remaining gap**.

## Slide 6 - Next Implementable Units

### 07-14 unit plan — verdicts (all resolved or superseded)

| Unit | Done when (07-14) | Verdict 2026-07-31 | Evidence |
| --- | --- | --- | --- |
| A1 Runtime autograd inventory ratchet | Code-owned check ties finite-difference oracle, broadcast reductions, view rejection, matmul VJP, manual update, loss-trace rows to tests. | **Superseded** — never filed; the tape is now deprecated product law and the compiler-generated path eclipses it. Oracle-survival regression remains `cargo test -p faber-runtime autograd_reference`. | `faber-runtime/docs/factory/view-alias-autograd-policy.md` |
| A2 AIR eligibility and purity slice | Evidence packet names the source subset for differentiable functions and fail-closed exclusions. | **Landed** — closed the same day (2026-07-14). | `radix/docs/factory/air-eligibility-purity-slice/goal.md` |
| A3 Generated gradient oracle for scalar linear loss | Generated path produces the scalar rung-3 gradient matching FD for `(x*weight-target)^2`. | **Landed** — Stage A+B; rung-3 scalar target covered by the FD oracle; E4 rung-4 alignment closed SEM011. | `a459c2cdd` (Stage A+B); `f5e42e6`/`35ce29a` (E4) |
| A4 Dense linear layer / tiny MLP gradient proof | Gradients for `summa((XW + b - target)^2)` match CPU FD for input, weight, bias. | **Landed** — Stage E2 linear+MSE; MLP exemplum exceeds the target. | `c84bf4b`/`42ebd4d`/`0d01947`/`ebf6d84` (E2); `4931c3a`+`800422f` (MLP) |
| A5 Manual update and loss-trace replay | Bounded runner applies manual weight/bias updates and verifies two-step loss trace vs FD session oracle. | **Landed** — Stage E3 ≥8 SGD steps, strictly decreasing, FD-matched; per-step oracle parity closed for MLP. | `9ccd986` (E3); `e4389fd` (MLP per-step parity) |
| A6 GPU/workload handoff map | Examples/Radix/Faber docs map how the CPU proof feeds GPU workload rung 3/4 without claiming device gradients. | **Landed in substance** — `gpu-workload-floor` pins rungs and cites the autodiff gate (now landed); `pytorch-equivalence-ladder` stages 5–7 are the acceptance targets. Rung-1 reconciliation and rungs 3–4 device paths remain downstream. | `radix/docs/factory/gpu-workload-floor/goal.md`; `baseline-ledger.md` |

### Re-derived next units (2026-07-31) — evidence and sequencing only

| Unit | Done when | Dependencies | Non-claims |
| --- | --- | --- | --- |
| B1 BERT-tiny fragment ship-complete | BERT U3 (`996bfae0c`) re-verified and Mind-accepted at tip (fix the `for_function` API residual first — `996bfae0c` alone does not compile); 6 gates C0–C6 green; loss trace FD-matched over 8 steps. | `for_function` API residual (radix), tip-level ship. | Not full BERT model training; not a real model/dataset pipeline; no device execution. |
| B2 GPU workload floor re-measurement + rung-1 reconciliation | Harness re-run against the moved corpus; rung-1 `OutputChecked` WebGPU claim adjudicated under Tier D discipline; ledger paths/status updated. **Already in flight** (delivery spec 2026-07-31); expected outcome: no ratchet of the softmax floor (workload-shape/device-route mismatch), with the proof recorded as WebGPU sibling-lane evidence. | `faber/crates/exempla/src/exempla_e2e/gpu_workload.rs`; `examples/gpu-workload/`. | Not CUDA launch; not placement; no ladder-floor movement without the ladder's own `.ref.json` workload reaching `OutputChecked` on the CUDA route. |
| B3 Runtime-tape oracle numeric boundary tests | Explicit boundary tests for LayerNorm VJP at near-zero variance (denormal) and GELU VJP at tanh saturation, comparing compiler-generated gradients against the tape FD oracle. Test-only. | None — additive `#[test]`s in `reverse_ad_test.rs` / `autograd_reference_test.rs`. | No new ops, no transform changes, no tolerance weakening. |
| B4 Session/oracle-replay boundary evidence packet | A Faber-owned packet maps the training-loop evidence (linear+MSE, MLP, BERT-tiny, cross-entropy loss traces) onto the session/oracle manifest contract from `inference-session-boundary`, preserving explicit non-claims. | B1; `faber/docs/factory/inference-session-boundary/goal.md`. | Not a runtime session; not model loading; not an optimizer product API. |
| B5 Rung 3/4 device-path acceptance map | Keep rungs 3–4 pinned at `DeviceStagingFailed`; record the differentiated-kernel device path and placement gate (Gate 3) as the binding producer gaps, routing fixes to `cuda-kernel-emit`/placement, not to this lane. | B2; `mir-autograd` autodiff gate (landed). | No device gradient implementation here; no CUDA/WebGPU/Metal training claim; no performance evidence. |

Non-lane items (tracked elsewhere, not this deck's units): G.2 control-flow emit
(deferred indefinitely, `radix/mir-autograd/structural-ad-roadmap.md` invariant 5);
fusion pass (landed, Mind-accepted); optimizer shape generalization E5 (low
residual, implementation in `norma`); safetensors/GGUF model formats and
device-side backward (G-A-06, deferred Class 1 DR) — all out of this lane.

## Slide 7 - Recommended First Packet

File **B1** first: the BERT-tiny fragment ship-complete is the last outstanding
north-star evidence item (transformer-scale companion, FD-oracle-checked), and
its blocker is named (`for_function` API residual, `996bfae0c` not
tip-compilable alone). **B2 is already in flight** (delivery spec 2026-07-31) —
do not refile it. **B3** is the smallest independent unit if B1 blocks on the
`for_function` residual.

Reason:

- it converts the 07-14 "session evidence" clause into a finished,
  Mind-accepted proof at transformer scale;
- it keeps the compiler-generated path (product) separate from the runtime
  tape (deprecated oracle);
- it does not touch the device lane, which remains producer-gated.

Suggested want:

`[P1][examples][training] BERT-tiny fragment ship-complete (U3 + for_function residual)`

Done when: BERT U3 re-verified and Mind-accepted at tip; 6 gates C0–C6 green;
loss trace FD-matched; deck Slide 3/5 evidence updated. Do **not** file A1 —
superseded (Slide 6).

## Slide 8 - Executive Decision Points

1. "Autograd-equivalent" was defined as dense scalar-loss reverse mode plus
   oracle-checked linear/tiny-MLP proof. That bar is **met**; extend the same
   definition to the BERT-tiny fragment (B1) and then to the device lane
   (rungs 3–4), not to `torch.nn` breadth.
2. AIR is the compiler-owned generated-gradient path and it now **produces
   gradients** (no longer a future detour). The runtime tape remains reference
   evidence only — deprecated product law.
3. Session/optimizer work stays oracle replay first, product API later
   (B4). `sgd_step` concrete overloads are not a public optimizer API.
4. GPU training stays outside this lane; the workload rungs are downstream
   acceptance targets. The device path for rung 3/4 is the honest next
   milestone for the device lane, owned by `cuda-kernel-emit`/placement
   producer gates.

## Slide 9 - Stop Conditions

Stop and re-scope if a follow-up tries to:

- claim public PyTorch or `torch.nn` parity;
- route around AIR by making the runtime-local tape the compiler story (the
  tape is deprecated; the compiler path is the product);
- allow views or mutation without an alias/scatter-add policy;
- claim optimizer/session support from the manual update oracle;
- claim GPU training from examples rung 3/4 oracles or from the WebGPU
  sibling-lane matmul+add proof (not gradient execution);
- include sparse, packed, quantized, or model-format gradients in the first
  milestone;
- file the deferred G.2 (control-flow emit) as a requirement of this lane, or
  treat the fusion pass as autograd evidence.

---

## Slide 10 - Stale Claims Annotated

Specific 07-14 claims and their disposition (history is preserved, not rewritten):

| Claim (as written 07-14) | Where | Disposition |
| --- | --- | --- |
| "no generated AIR autodiff yet" | Slide 2 | **Stale** since 2026-07-25 — `mir-autograd` landed generated AIR/MIR autodiff (all 15 ops). |
| "generated AIR gradients are not implemented" | Slide 4 | **Stale** since 2026-07-25 — the reverse-AD transform is the product path. |
| "The honest milestone should therefore be a CPU reference gradient proof first, then a generated/AIR proof, then a session/training proof." | Slide 4 | The deck's own sequencing **came true**; the first two stages and the training-loop slice are done (Stage A–F + Stage E). Now read as history, not plan. |
| "autograd rejects view leaves until scatter-add policy exists" | Slide 3 | **Superseded** — the tape has tape-owned `sectio` scatter-add plus fail-closed rejection of raw aliased views; compiler lane rejects views via AIR purity (`view-alias-autograd-policy.md`). |
| A1–A6 "Next Implementable Units" | Slide 6 | All resolved or superseded by 2026-07-31 (per-row verdicts in Slide 6). A2 was actually closed the same day the deck was written. |
| "File A1 first unless a runtime owner has already closed it" | Slide 7 | **Superseded** — A1 never filed; tape deprecated as product path; B1 replaces it as first packet. |
| "Target workspace: `/home/ianzepp/work/faberlang`" | Header | **Corrected** — machine-local path is `/Users/ianzepp/work/faberlang`. |
| The 07-14 rows of Slide 3 describing the runtime tape as the current evidence base | Slide 3 | Still factually valid, but no longer the *product* evidence base — tape is deprecated oracle. Annotated in place. |
