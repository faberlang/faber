# Campaign: Tabular Data Access — Census, SQLite, ViviLite

**Status**: proposed — not selected for factory
**Date**: 2026-07-10
**Mode**: routing artifact — does not implement code directly
**Control-plane repo**: `/Users/ianzepp/work/faberlang/faber`
**Working repos**: `radix`, `faber`, `faber-runtime`, `norma`, `examples`

## Summary

Coordinate typed tabular data in Faber and thin SQL access for application
packages. Three ordered goals:

1. **Census types** — compiler/runtime `schema`, `series<S>`, `census<S>` (bare
   `series` preserved for dynamic interop).
2. **SQLite library shim** — thin `sqlite:sqlite` package over `rusqlite`; API
   surface parallel to `norma:arca`; typed returns use census when available.
3. **ViviLite integration** — regular-Vivi `.vivi/mail.sqlite` read path (then
   write), oracle parity with `vivi`, file-backed lane retained.

**Not part of aer-purgatus.** That campaign may still migrate ViviLite JSON
output (Goal 3 M5) on the file-backed `.vivilite/` lane only — no census or
SQLite prerequisites there.

**Board:** want `688cd65` — defer SQLite/census for ViviLite until this
campaign is selected.

## Problem

Application packages need typed rows and SQL execution without duplicating
untyped `valor` walking, hand-rolled JSON, and incompatible sqlite vs Arca
surfaces. ViviLite Stage 0–1 proved CLI ergonomics on a file-backed floor;
regular Vivi storage compatibility and shared developer types belong here.

## Desired End State

- Apps declare column shapes with `schema`; query results materialize as
  `series<S>` / `census<S>` from both SQLite shim and (later) Arca adapters.
- `sqlite:sqlite` remains a thin sync binding — no ORM, no mailspace logic, no
  query DSL in the package.
- ViviLite reads fixture `.vivi/mail.sqlite` through the shim and matches
  selected `vivi --json` oracle outputs.
- Migration from sqlite package to `norma:arca` is manageable: connection +
  async + import path, not row-shape redesign.
- `norma:arca` stays the long-term stdlib authority via host gateway; this
  campaign does not replace Arca — it prepares typed rows both paths can share.

## Development Posture

- **Ordered delivery.** Goal 2 must not require ViviLite; Goal 3 must not start
  before Goal 2 read API is usable.
- **Thin sqlite package.** Rusqlite + valor map + `⇥ textus` errors only.
- **API overlap with Arca.** Shared stems (`quaer-`, `capi-`, `exsequ-`) and
  parameter/row contracts per sqlite Stage 1 contract.
- **Census-first typing.** Prefer `census<S>` returns for app-facing SQL APIs
  once Goal 1 reaches series/census shells.
- **Clean break from aer-purgatus scope.** Do not pull census or SQLite into the
  active four-goal remediation pass as prerequisites.

## Implementation Workflow

1. Operator selects this campaign (after aer-purgatus or in a dedicated season).
2. Flesh out per-goal delivery specs under this directory or linked factory dirs.
3. Execute each goal through `factory` in order; update Status here only.
4. Do not implement from this routing artifact alone.

## Scope Routing

| Surface | Owner | Campaign goal |
| --- | --- | --- |
| `schema`, `series<S>`, `census<S>`, EBNF, intrinsics | `radix` | Goal 1 |
| `faber::Series`, `faber::Census`, schema metadata | `faber-runtime` | Goal 1 |
| `sqlite:sqlite` package, `bindings/rust.toml`, rusqlite shim | `faber` (+ package tree TBD) | Goal 2 |
| `examples/vivilite`, fixture mailspaces, oracle tests | `examples` | Goal 3 |
| `norma:arca` gateway dispatch, sqlx host handler | `norma` / `faber-runtime` / gateway | **out of campaign** (follow-on) |

## Out of Campaign

- aer-purgatus Goals 1–4 and their factory runs
- Full `norma:arca` implementation and frame-gateway Stages 6–9
- ORM, query builders, SQLite DDL migrations bound to Faber `schema`
- Mutating live project `.vivi/` stores (fixture-only gates)
- census Stage 7 `corpus` / `censet<S>` unless explicitly added later

## Current State

| Goal | State | Factory entry | Next action |
| --- | --- | --- | --- |
| 1 — Census types (`schema`, `series<S>`, `census<S>`) | proposed — parked | [`radix/docs/factory/census-types/goal.md`](../../../../radix/docs/factory/census-types/goal.md) | Flesh delivery spec when campaign opens; implement compiler/runtime per census plan |
| 2 — SQLite library shim (`sqlite:sqlite`) | Stage 1 contract complete; Stage 2 not landed | [`sqlite-library-package/goal.md`](../sqlite-library-package/goal.md) | Thin rusqlite prototype; census-typed `quaere` when Goal 1 shells exist |
| 3 — ViviLite SQL integration | Stage 0–1 file-backed complete; Stage 2+ deferred | [`examples/docs/factory/vivilite/goal.md`](../../../../examples/docs/factory/vivilite/goal.md) | Read oracle via shim; typed board/status; keep `.vivilite/` lane |

Delivery specs for Goals 1–3 are **not authored yet**. Link existing factory
goals above; lower to `goal-*-delivery.md` files in this directory when the
campaign is selected.

## Dependency Order

```text
Goal 1 (census types)
    │
    ▼
Goal 2 (sqlite shim — dynamic rows minimum; census<S> returns when ready)
    │
    ▼
Goal 3 (ViviLite tie-in — read oracle, then write compatibility)
```

Unified package manifest Phases 3–4 remain prerequisites for Goal 2 binding
linkage (Phase 4 verification complete; Phase 3 build graph may still gate).

## Related Artifacts

| Artifact | Role |
| --- | --- |
| [`sqlite-library-package/stage-1-api-fixture-contract.md`](../sqlite-library-package/stage-1-api-fixture-contract.md) | API + valor map + ViviLite oracle contract |
| [`radix/docs/factory/census-types/plan.md`](../../../../radix/docs/factory/census-types/plan.md) | Census stage graph |
| [`norma/src/arca.fab`](../../../../norma/src/arca.fab) | Long-term async DB device (parallel API vocabulary) |
| Vivi want `688cd65` | Sequencing policy vs aer-purgatus |