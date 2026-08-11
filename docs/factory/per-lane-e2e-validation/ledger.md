# Per-Lane E2E Validation — Unit Ledger

Goal: `docs/factory/per-lane-e2e-validation/goal.md` · Delivery: `docs/factory/per-lane-e2e-validation/delivery.md`
**Status**: active — EL-1 delivered (hand-6, task 80428876); EL-2 reverted + escalated (facade re-export surface load-bearing, task c1a7692e); EL-2b delivered (hand-6, task addd5202); EL-3 delivered (hand-6, task 6ee15876); EL-4 delivered (hand-6, task e8aec8c4); EL-5/EL-6 pending tasking. Lowered 2026-08-10 by planner-4; goal-check READY.

## Unit rows

| Unit | Status | Hand | Receipt (commit/handle) | Notes |
| --- | --- | --- | --- | --- |
| EL-1 | done | hand-6 | hand-6 / 80428876 | exempla pass-through + cfg gates (rust proof lane) |
| EL-2 | deferred | hand-6 | hand-6 / c1a7692e | gate reverted + escalated: rust-only radix check fails at the facade re-export surface (`cli_descriptor.rs:44`, `mir/llvm_text/mod.rs:7`, `driver/mod.rs:698` — unconditional `radix_mir_llvm` sites); faber `mod mir` FMIR consumers are the whole module incl. the source-built run-image struct, not just route/selection surfaces. Re-scope proposal in the c1a7692e escalation reply to mind. |
| EL-2b | done | hand-6 | hand-6 / addd5202 | radix 721acd7f1 — `mir-llvm = ["dep:radix-mir-llvm"]` (+ `mir-amd` implies the dep + leaf `mir-amd`); `mod llvm_text` + `pub use` gated; CliAdapter plan/fields + FCLI-encoder split gated; `cuda_kernel_descriptor_json` + `--cuda-descriptor` path gated; driver `MirLlvm` emit arm fails closed like wasm/metal; `Target::MirLlvm(Host)` report `required_feature`/`is_available` = `mir-llvm`. FMIR/FHIR untouched (operator ruling). rust-only radix graph compiles no `radix-mir-llvm`; full-targets unchanged. |
| EL-3 | done | hand-6 | hand-6 / 6ee15876 | radix f9b977e5c — `failable_facts_parts` cfg-gated to `any(hir-go, hir-ts, hir-swift)` (dispatch-boundary conversion consumes monolith `FunctionFactTable`; go/ts/swift leaves never depend on `crate::semantic`, so no single leaf owns it); rust-only check clean with no dead-code finding; focused `codegen/failable_facts_test.rs` under the same gate (may_fail set + err_ty overrides preserved, synthetic entry facts excluded); full-targets byte-identical. |
| EL-4 | done | hand-6 | hand-6 / e8aec8c4 | lane-scoped expectations + diff-derived lane selection: per-lane expectation tables moved verbatim to `crates/exempla/src/exempla_e2e/expectations/<lane>.rs` (go/ts/wasm/rust/swift/sexp/llvm/mir/roundtrip; rust table empty by design — shared oracle classifications stay in `super::oracle`); `lane_selection.rs` maps a changed-crate path set to lanes (leaf crate → its lane; shared/harness/corpus → all); rust-table ownership test + selection unit tests (`radix-hir-go`→{go}, `radix-mir-llvm`→{llvm}, exempla-only→{all}) + dry-run green; `cargo test -p exempla --lib` green; grep guard clean (no lane table references another lane's table). |
| EL-5 | pending | — | — | nightly per-lane grid on pharos |
| EL-6 | pending | — | — | release protocol rewrite + one-command rehearsal |

Status values: `pending` · `tasked` · `in progress` · `done` · `deferred` (one machine-parseable leading clause; audit-recognized per `radix/scripta/audit-factory-goal-status.py`).
