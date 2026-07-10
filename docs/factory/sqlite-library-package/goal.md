# Goal: SQLite Library Package

**Status**: Stage 1 API/fixture contract complete — Phase 4 verification
available; implementation blocked on the Phase 3 backend library build graph
**Created**: 2026-07-09
**Refreshed**: 2026-07-10
**Target workspace**: `/Users/ianzepp/work/faberlang`
**Factory artifact dir**: `faber/docs/factory/sqlite-library-package/`
**Primary surfaces**: unified `faber.toml` library packages, target binding
manifests, Rust shim packaging, `valor` row conversion, application consumers
such as ViviLite.

---

## Summary

Provide SQLite to Faber applications as a Faber-oriented library package, not as
a compiler builtin and not as a new permanent `cista.toml` package shape. The
package should expose a Faber source API such as `sqlite:sqlite` and implement
that API through target-specific Rust bindings over `rusqlite`. Unified
manifests can now describe and verify that library/binding contract; the open
backend library build graph still must link it into an application build.

The first consumer is expected to be `examples` ViviLite, which can begin with
file-backed scaffolding but eventually needs SQLite to read and write the same
local mailspace storage as regular Vivi.

## Problem

Several useful application-lane examples need SQLite-compatible local state:
ViviLite, transcript/index tools, package caches, and future project-local
agent stores. Faber should not reimplement SQLite, shell out to `sqlite3` as the
main API, or make SQLite a hidden compiler/runtime feature.

The current package direction is the sibling
[`unified-package-manifest`](../unified-package-manifest/goal.md) goal:
`faber.toml` becomes the package authority for applications, source libraries,
backend-compiled libraries, and native-binding facades. A SQLite package must
follow that direction instead of reviving a separate long-term `cista.toml`
facade design.

## Goals

- Define a Faber-facing SQLite API package with provider `sqlite`.
- Use the shipped unified `faber.toml` library metadata for the package.
- Use target-specific Rust binding manifests for `rusqlite` implementation
  verification through the shipped Phase 4 contract.
- Route application build/linkage through unified manifest Phase 3 or an
  explicitly proven equivalent build path.
- Keep SQLite outside `norma` for v1; it is a concrete native dependency, not a
  backend-agnostic standard-library primitive.
- Return query rows as `valor` objects for the first contract.
- Support parameterized SQL with `lista<valor>` parameters.
- Provide enough read/query capability for ViviLite to validate against regular
  Vivi local mailspace storage.
- Add write/transaction support only after read parity and error conversion are
  stable.

## Non-goals

- Reimplementing SQLite in Faber.
- Making SQLite a compiler builtin or implicit dependency of every Faber
  program.
- Adding a permanent `cista.toml`-first package shape for SQLite.
- Reintroducing `@ externa` / `@ subsidia` source annotations.
- Designing a full ORM or typed query builder in v1.
- Supporting non-Rust backend bindings before the Rust binding model is proven.
- Claiming ViviLite write compatibility until normal Vivi can read the mutated
  storage.

## Ground Truth Researched

| Source | Evidence |
| --- | --- |
| [`unified-package-manifest/goal.md`](../unified-package-manifest/goal.md) | `faber.toml` is the package authority; Phases 1–2 and Phase 4 verification are complete, while Phase 3 backend library linkage remains open. |
| [`../README.md`](../README.md) | Public `faber` repo owns package product surface. |
| `../../../../cista/docs/factory/cista-package-store/goal.md` | Current Cista work separates store concerns from Faber package authority and warns against conflating with unified manifest. |
| `../../../../examples/cista-lab/` | Existing lab proves interface/source plus target binding ideas but uses the old split-manifest staging shape. |
| `~/work/ianzepp/vivarium/src/mailspace*.rs` | Vivi local mailspace compatibility ultimately depends on `.vivi/mail.sqlite` semantics. |

## Reference Packet

- `docs/factory/unified-package-manifest/goal.md`
- `docs/factory/sqlite-library-package/stage-1-api-fixture-contract.md`
- `../../../../cista/docs/factory/cista-package-store/goal.md`
- `../../../../examples/cista-lab/source/mathesis/`
- `../../../../examples/docs/factory/vivilite/goal.md`
- `../../../../norma/src/valor.fab`
- `../../../../norma/src/json.fab`
- `../../../../radix/docs/design/target-capability-matrix.md`

## Constraints And Invariants

- `faber.toml` is the package authority; do not create a competing durable
  manifest format for the SQLite package.
- Faber source declares the API contract; target binding manifests declare
  implementation linkage.
- SQLite v1 is path-based. Do not require opaque native connection handles for
  the first usable API.
- Query outputs are `valor` rows so application code can inspect them without a
  generated row type system.
- SQL errors must return through the alternate-exit channel (`⇥ textus`) or a
  later structured error genus; they must not crash generated Rust.
- Parameter binding must be explicit. Do not concatenate user strings into SQL
  in examples as the normal pattern.
- Read compatibility comes before write compatibility.
- ViviLite must not bypass this package by shelling out to `sqlite3` for its
  main compatibility path.

## Architecture Direction

Proposed package shape after unified manifests:

```text
sqlite/
  faber.toml
  src/
    sqlite.fab
  bindings/
    rust.toml
  rust/
    Cargo.toml
    src/lib.rs
```

Faber API sketch:

```fab
importa ex "sqlite:sqlite" privata sqlite

genus SQLiteEffect {
    numerus rows_changed
    numerus last_insert_id
}

functio exsequi(textus via, textus sql, lista<valor> params) → SQLiteEffect ⇥ textus
functio quaere(textus via, textus sql, lista<valor> params) → lista<valor> ⇥ textus
functio scalar(textus via, textus sql, lista<valor> params) → valor ∪ nihil ⇥ textus
```

The bodyless function declarations above are the current Phase 4 form for
"body supplied by target binding." Detailed delivery research must still prove
the facade and shim through the application build path.

Rust target:

- wraps `rusqlite`;
- maps `valor` scalars to SQLite bind values;
- maps result rows to `valor` objects keyed by column name;
- exposes explicit error strings or a later structured SQLite error genus;
- keeps transaction support out of v1 unless ViviLite write parity requires it.

## Dependency Order

| Dependency | Required for | Notes |
| --- | --- | --- |
| Unified package manifest Phase 1 | Record SQLite as a library package | Complete. |
| Unified package manifest Phase 2 | Install/resolve provider roots by manifest | Complete. |
| Unified package manifest Phase 4 | Verify bodyless declarations, binding rows, shim source, dependencies, and Rust ABI | Complete for `faber verify-library`; it does not provide application linkage. |
| Unified package manifest Phase 3 | Link generated/backend library artifacts into application Cargo graphs | Open; current implementation gate for a usable SQLite consumer. |
| SQLite read API | ViviLite regular-Vivi read parity | Allows `board --json` and list/show validation against `.vivi/mail.sqlite`. |
| SQLite write API | ViviLite write compatibility | Only after read parity and storage semantics are understood. |

Phase 4 verifies the native binding contract in an isolated Rust probe. The
current application Cargo generator does not consume a library package's shim
and target dependencies, so Phase 3 or an explicitly proven equivalent path is
the remaining build/linkage prerequisite.

## Implementation Shape

### Stage 0 - Hold Until Backend Library Linkage Is Ready

Status: Phase 1–2 and Phase 4 gates satisfied; Phase 3 linkage gate open.

Do not implement SQLite through the old split `cista.toml` staging shape. Keep
this goal parked for implementation until the Phase 3 build graph is delivered
or detailed research proves and records an equivalent application linkage path.

### Stage 1 - API And Fixture Contract

Status: complete. See
[`stage-1-api-fixture-contract.md`](stage-1-api-fixture-contract.md).

Draft the Faber source facade and fixture database contract:

- decide exact module path (`sqlite:sqlite`);
- define `SQLiteEffect` and read API signatures;
- create fixture SQL/database expectations for a tiny message/task table;
- validate that `valor` can represent the needed scalar and row shapes.

This stage may proceed before the Rust binding implementation if it is kept as
docs/interfaces only.

### Stage 2 - Rust Binding Prototype

After application linkage for verified target bindings is available:

- add Rust shim over `rusqlite`;
- bind `exsequi`, `quaere`, and `scalar`;
- add generated package build proof;
- use parameter binding, not string interpolation.

### Stage 3 - ViviLite Read Consumer

Use the SQLite package from ViviLite to read a regular Vivi fixture mailspace
and match selected `vivi` JSON outputs:

- `mailspace status --json`;
- `task list --json`;
- `need list --json`;
- `want list --json`;
- `board --json`.

### Stage 4 - Write Compatibility

Add mutation surfaces only after read parity:

- transaction helper or explicit `exsequi` batches;
- insert message/index rows in the regular Vivi-compatible shape;
- regular `vivi` must read messages or work-item moves created by ViviLite.

## Acceptance Criteria

- A Faber package can declare a dependency on provider `sqlite` through unified
  `faber.toml` package metadata.
- Building a Rust application that imports `sqlite:sqlite` links the Rust
  SQLite shim through the selected binding manifest.
- `sqlite.quaere` returns deterministic `lista<valor>` rows from a fixture
  database.
- `sqlite.exsequi` applies parameterized writes and returns affected row
  metadata.
- SQL errors are surfaced as Faber alternate exits or structured diagnostics,
  not Rust panics.
- ViviLite can use the package for read parity against a regular Vivi fixture
  mailspace.

## Validation

Initial planning/interface validation:

```bash
timeout 120 cargo test --lib manifest
timeout 120 cargo test --lib binding
timeout 120 cargo run -- check <sqlite-package-fixture>
```

Consumer validation after ViviLite integration:

```bash
vivi board --project <fixture> --for codex --json > expected.json
faber run ../examples/vivilite -- board --project <fixture> --for codex --json > actual.json
```

Compare JSON semantically rather than by raw field order.

## Open Questions

- Should SQLite package source live in a new sibling repo, under `examples` as
  a lab first, or under a future public package namespace?
- Should first errors stay `⇥ textus`, or define `SQLiteError` immediately?
- Is path-based access enough for v1, or does write compatibility require an
  explicit transaction/batch API?
- Which subset of Vivi `.vivi/mail.sqlite` is stable enough to treat as a
  compatibility target?

Resolved prerequisite facts: Phase 4 uses bodyless Faber function declarations
for target-supplied implementations and keys bindings as
`provider:module/path.function`. These are current contracts, not open design
questions for the SQLite delivery.

## Stop Conditions

- Stop if implementation starts from a durable `cista.toml` package shape
  instead of unified `faber.toml`.
- Stop if the plan invents a second source annotation or binding-key form instead
  of using the shipped bodyless-declaration and target-manifest contract.
- Stop if generated Rust would build by shelling out to `sqlite3` rather than
  linking an intentional Rust target dependency.
- Stop if ViviLite write compatibility would mutate real project mailspaces
  without a fixture-first parity gate.
