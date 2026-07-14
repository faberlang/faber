# Campaign: Tabular Data Access — Census, SQLite, ViviLite

**Status**: proposed — prerequisite facts refreshed; not selected for factory
**Date**: 2026-07-10
**Refreshed**: 2026-07-10
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

**Not part of aer-purgatus.** Aer Purgatus completed on 2026-07-10 without
pulling census or SQLite into its scope. Its ViviLite JSON work stayed on the
file-backed `.vivilite/` lane.

**Board:** want `688cd65` remains open and records the decision to defer
SQLite/census for ViviLite until this campaign is selected. Its stated
done-when (opening a separate campaign) is now satisfied; closing or promoting
the want remains a separate operator action.

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
- **Clean break from aer-purgatus scope.** The completed remediation campaign
  did not adopt census or SQLite as prerequisites.

## Implementation Workflow

1. Operator selects this campaign. Aer Purgatus is complete and is no longer a
   sequencing gate.
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
| 2 — SQLite library shim (`sqlite:sqlite`) | Stages 1–3 complete; Stage 4 write compatibility partially complete | [`sqlite-library-package/goal.md`](../sqlite-library-package/goal.md) | Continue only the remaining Stage 4 sent-copy parity and mutation-command scope when selected |
| 3 — ViviLite SQL integration | Stage 0–1 file-backed complete; SQLite read lane present; write lane partially complete | [`examples/docs/factory/vivilite/goal.md`](../../../../examples/docs/factory/vivilite/goal.md) | Finish sent-copy parity and remaining mutation commands through fixture-first gates |

Delivery specs for Goals 1–3 are **not authored yet**. Link existing factory
goals above; lower to `goal-*-delivery.md` files in this directory when the
campaign is selected.

## Prerequisite And Status Check — 2026-07-14

| Surface | Verified state | Campaign implication |
| --- | --- | --- |
| Aer Purgatus | Complete; Goals 1–4 and residual queue closed | No longer a sequencing prerequisite |
| Census types | Proposed/parked; no `schema`, typed `series<S>`, or `census<S>` implementation landed | Goal 1 still begins with delivery/spec lock work |
| Unified manifest Phases 1–2 | Complete | Library manifests, install, and provider-root resolution are available |
| Unified manifest Phase 4 | Binding-manifest verification complete | Bodyless Faber declarations, binding keys, shim presence, target dependencies, and Rust ABI probes can be verified |
| Unified manifest Phase 3 | Complete for the Rust native-binding path used by the SQLite packet | The old application-linkage gate is historical evidence, not a current blocker for the shipped SQLite packet |
| SQLite package | Stage 1 API contract, Stage 2 Rust binding prototype, and Stage 3 ViviLite read consumer complete; Stage 4 write compatibility partially complete | Goal 2 remains active only for sent-copy parity and remaining mutation commands |
| ViviLite | Stage 0–1 file-backed package and tests landed; SQLite read lane present; write lane partially complete | Goal 3 remains fixture-gated for write parity and remaining mutation commands |
| Vivi want `688cd65` | Open; stated done-when is satisfied by this campaign artifact | Preserve as sequencing evidence; operator may close/promote separately |

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

Unified package manifest Phase 4 verification is complete and is no longer a
blocker. The Rust native-binding application linkage path is also proven for
the SQLite packet, so the former Phase 3 build-graph gate is no longer the Goal
2 blocker. Remaining work is the Stage 4 write-compatibility scope recorded in
[`sqlite-library-package/goal.md`](../sqlite-library-package/goal.md).

## Related Artifacts

| Artifact | Role |
| --- | --- |
| [`sqlite-library-package/stage-1-api-fixture-contract.md`](../sqlite-library-package/stage-1-api-fixture-contract.md) | API + valor map + ViviLite oracle contract |
| [`unified-package-manifest/goal.md`](../unified-package-manifest/goal.md) | Authoritative Phase 1–4 package prerequisite state |
| [`radix/docs/factory/census-types/plan.md`](../../../../radix/docs/factory/census-types/plan.md) | Census stage graph |
| [`norma/src/arca.fab`](../../../../norma/src/arca.fab) | Long-term async DB device (parallel API vocabulary) |
| Vivi want `688cd65` | Sequencing policy vs aer-purgatus |
