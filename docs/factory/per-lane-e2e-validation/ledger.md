# Per-Lane E2E Validation — Unit Ledger

Goal: `docs/factory/per-lane-e2e-validation/goal.md` · Delivery: `docs/factory/per-lane-e2e-validation/delivery.md`
**Status**: active — EL-1 delivered (hand-6, task 80428876); EL-2 reverted + escalated (facade re-export surface load-bearing, task c1a7692e); EL-3–EL-6 pending tasking. Lowered 2026-08-10 by planner-4; goal-check READY.

## Unit rows

| Unit | Status | Hand | Receipt (commit/handle) | Notes |
| --- | --- | --- | --- | --- |
| EL-1 | done | hand-6 | hand-6 / 80428876 | exempla pass-through + cfg gates (rust proof lane) |
| EL-2 | deferred | hand-6 | hand-6 / c1a7692e | gate reverted + escalated: rust-only radix check fails at the facade re-export surface (`cli_descriptor.rs:44`, `mir/llvm_text/mod.rs:7`, `driver/mod.rs:698` — unconditional `radix_mir_llvm` sites); faber `mod mir` FMIR consumers are the whole module incl. the source-built run-image struct, not just route/selection surfaces. Re-scope proposal in the c1a7692e escalation reply to mind. |
| EL-3 | pending | — | — | failable_facts_parts leaf ownership (radix codegen; serialized) |
| EL-4 | pending | — | — | lane-scoped expectations + diff-derived lane selection |
| EL-5 | pending | — | — | nightly per-lane grid on pharos |
| EL-6 | pending | — | — | release protocol rewrite + one-command rehearsal |

Status values: `pending` · `tasked` · `in progress` · `done` · `deferred` (one machine-parseable leading clause; audit-recognized per `radix/scripta/audit-factory-goal-status.py`).
