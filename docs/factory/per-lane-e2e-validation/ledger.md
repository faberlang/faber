# Per-Lane E2E Validation — Unit Ledger

Goal: `docs/factory/per-lane-e2e-validation/goal.md` · Delivery: `docs/factory/per-lane-e2e-validation/delivery.md`
**Status**: active — EL-1 delivered (hand-6, task 80428876); EL-2–EL-6 pending tasking. Lowered 2026-08-10 by planner-4; goal-check READY.

## Unit rows

| Unit | Status | Hand | Receipt (commit/handle) | Notes |
| --- | --- | --- | --- | --- |
| EL-1 | done | hand-6 | hand-6 / 80428876 | exempla pass-through + cfg gates (rust proof lane) |
| EL-2 | pending | — | — | gate radix-mir-llvm + radix-mir-fmir behind dep: (radix + faber; serialized) |
| EL-3 | pending | — | — | failable_facts_parts leaf ownership (radix codegen; serialized) |
| EL-4 | pending | — | — | lane-scoped expectations + diff-derived lane selection |
| EL-5 | pending | — | — | nightly per-lane grid on pharos |
| EL-6 | pending | — | — | release protocol rewrite + one-command rehearsal |

Status values: `pending` · `tasked` · `in progress` · `done` · `deferred` (one machine-parseable leading clause; audit-recognized per `radix/scripta/audit-factory-goal-status.py`).
