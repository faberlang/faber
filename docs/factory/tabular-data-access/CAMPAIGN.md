# Campaign: Typed Relational Data Access

**Status**: planned — architecture refreshed around partial schemas, typed filter plans, and PostgreSQL/SQLite execution; implementation not selected
**Created**: 2026-07-10
**Refreshed**: 2026-08-11
**Mode**: routing artifact — stages lower to delivery before implementation
**Control-plane repo**: `/Users/ianzepp/work/faberlang/faber`
**Working repos**: `radix`, `faber`, `faber-runtime`, `hosts`, `norma`, `examples`

## Purpose

Give Faber applications typed, partial views over relational data without
making them own the complete database schema, migrations, storage, or SQL
dialect.

The central product path is:

```text
PostgreSQL / SQLite relation
    + schema I                  partial input contract
    + filtrum<I,O>              typed relational plan
    + schema O                  visible result contract, O ⊆ I
    ↓ provider execution and admission
census<O>                       finite result
cursor<series<O>>               streaming result
series<O> ∪ nihil               optional single result
```

The authoritative language architecture is sibling
[`radix/docs/factory/census-types/goal.md`](../../../../radix/docs/factory/census-types/goal.md).

## Why this campaign still exists

The original campaign assumed Census must land before SQLite and ViviLite.
History went another way:

- the SQLite package proved a dynamic `valor`-row floor;
- ViviLite proved concrete SQLite read/write application paths;
- typed Census did not land;
- the dynamic floor exposed the missing architectural layer: a typed,
  serializable filter/view plan between a source relation and a result schema.

The delivered dynamic work is now evidence and infrastructure for typed views,
not unfinished work that must be replayed in its original order.

## Architectural vocabulary

| Surface | Meaning | Owner |
| --- | --- | --- |
| `schema S` | Application-owned partial relational heading; open to extra source columns | `radix` |
| `filtrum<I,O>` | Inert typed plan: source, aliases, predicate, order, window, provider requirements | `radix` + provider-neutral runtime encoding |
| `series<S>` | One row admitted through `S` | `faber-runtime` |
| `census<S>` | Finite admitted relational result with bag semantics | `faber-runtime` |
| `cursor<series<S>>` | Resource-bound streaming rows | `norma:arca` + database host/provider |
| database catalog/execution | Relation metadata, parameter binding, transactions, SQL | PostgreSQL/SQLite host providers |
| application packaging | Provider selection, dependency assembly, user CLI workflow | `faber` |

The working plan noun is `filtrum`, not noun `filtra`. `filtra` already belongs
to Faber collection morphology as the imperative partner of copy-returning
`filtrata`. The final lexical decision belongs to the Census design lock.

## Campaign tracks

### Track 1 — Typed relational language contract

Authority:
[`radix/docs/factory/census-types/`](../../../../radix/docs/factory/census-types/goal.md)

Deliver:

- partial/open schemas;
- input/output schema distinction;
- closed typed predicate AST;
- checked output projection;
- provider capability requirements;
- Series/Census result types;
- deterministic saved-plan encoding;
- structured admission errors.

### Track 2 — PostgreSQL and SQLite providers

Deliver two renderers/adapters over one provider-neutral plan:

- parameterized SQL with identifiers separately admitted;
- catalog/result metadata validation;
- PostgreSQL type-identity admission;
- SQLite per-cell storage-class admission;
- finite, optional-single, and explicit streaming postures;
- fail-closed provider capability differences;
- host-owned tenant, ACL, and deletion policy injection.

The existing SQLite package is input evidence. It is not the final owner of a
second filter AST or competing typed-row system.

### Track 3 — Saved filters

Use the proven shape from the Monk API reference implementation:

```text
source/model + projection + predicate + order + limit + offset
```

Faber saves a versioned provider-neutral `filtrum<I,O>`, not generated SQL.
Loading revalidates source/schema identities and provider requirements before
execution.

Reference implementation:
`/Users/ianzepp/work/ianzepp/monk-api/src/lib/filter*.ts` and
`planning/FILTER_REWORK.md`.

### Track 4 — Application proof

Prove one shared logical plan against PostgreSQL and SQLite fixtures. The proof
must include:

- extra source columns;
- an input-only predicate column;
- explicit aliases;
- nullable and required cells;
- deterministic ordering;
- finite, single, and streaming results;
- saved-plan round trip and revalidation;
- exact failure evidence for missing, duplicate, null, incompatible, and
  unsupported-provider cases.

ViviLite is a candidate consumer because it already exercises SQLite rows, but
it is not mandatory. A smaller dedicated package is preferred if it gives a
cleaner cross-provider oracle.

## Current evidence

| Surface | Verified/documented state | Campaign use |
| --- | --- | --- |
| Census types | No live `schema`, typed `series<S>`, `census<S>`, or relational plan type | Begin with Census S0/S1 design and evidence freeze. |
| Bare `series` | Dynamic `tabula<textus, valor>` lowering remains live | Preserve as the untyped interop floor. |
| SQLite package | Dynamic `valor` query rows and parameterized read/write paths are documented as delivered in substantial part | Reuse fixtures and packaging evidence; reverify exact open closeout scope before execution. |
| ViviLite read | SQLite read delivery records board/status/list proofs | Candidate typed-view consumer and oracle source. |
| ViviLite write | Stage-3 delivery says complete while top-level goal/campaign prose contains older partial-state claims | Treat status as doc drift until its owning repo reconciles it; do not block Census design on it. |
| `norma:arca` | Dynamic bare-Series API vocabulary; typed adapters explicitly out of scope today | Preserve vocabulary; add typed finite/stream adapters only after the shared contract is frozen. |
| Monk Filter | PostgreSQL/SQLite query descriptor and saved filters; planned typed-AST rework | Architectural reference for plan shape and dialect separation. |

## Dependency and stage order

```text
T0 Census evidence + design lock
    ↓
T1 schema + filtrum semantics
    ↓
T2 runtime values + saved-plan encoding
    ↓
T3 PostgreSQL provider ──┐
                        ├─→ T5 cross-provider application proof
T4 SQLite provider ─────┘                 ↓
                                      T6 closeout
```

The existing dynamic SQLite/ViviLite work is a predecessor evidence base, not
a stage that waits behind T1. PostgreSQL and SQLite implementations may proceed
in parallel only after the provider-neutral plan and admission contracts are
frozen.

| Stage | Deliverable | Authority | Gate |
| --- | --- | --- | --- |
| T0 | Evidence, fixtures, representation and naming decisions | Census S0 | All design locks decided or explicitly block implementation |
| T1 | Partial schemas and typed `filtrum<I,O>` | Census S1–S3 | Typed AST; `O ⊆ I`; no provider SQL in semantic layer |
| T2 | Series/Census runtime, errors, saved-plan encoding | Census S4 | Deterministic round trip and owned finite results |
| T3 | PostgreSQL vertical | Census S5 | Parameterized execution and precise admission failures |
| T4 | SQLite vertical | Census S6 | Per-cell validation and fail-closed capability handling |
| T5 | Same-plan application proof | Census S7 | Semantic parity over shared capability intersection |
| T6 | Docs/corpus/locale/product closeout | Census S8 | Integrated lane ladder and reproducible receipts |

## Non-goals

- ORM, migrations, DDL ownership, or schema synchronization.
- Automatic SQL generation from arbitrary Faber closures.
- Raw SQL as the portable filter representation.
- Joins, subqueries, or common-table expressions in v1.
- Aggregation in the first vertical slice; it needs a distinct typed output
  schema.
- Automatic writes through Series or Census.
- Filter-based update/delete without a separate authorization, identity,
  transaction, and affected-row contract.
- Silent PostgreSQL/SQLite semantic approximation.
- `corpus` / `censet<S>` as a v1 prerequisite.
- Mutating live Vivi mailspaces during fixtures or acceptance.

## Ownership boundaries

- Radix owns semantics, typed plan lowering, and portable serialization. It
  does not open databases or run SQL.
- Database hosts/providers own catalog access, parameter binding, execution,
  transactions, policy injection, and dialect capability truth.
- `faber-runtime` owns provider-neutral runtime values and admission errors,
  not database drivers.
- Norma owns public database verbs and finite/stream posture, not a second
  schema or filter system.
- Faber owns package/build workflow, not compiler semantics or provider effects.
- Examples own disposable proof packages and fixtures, not live data.

## Stop conditions

- Stop if a Faber schema begins to claim complete database ownership.
- Stop if Census becomes only a renamed list.
- Stop if `select` strings can disagree with the declared result schema.
- Stop if hidden predicate/order fields have no typed input contract.
- Stop if provider-specific SQL enters the portable plan identity.
- Stop if ACL/tenant/deletion policy becomes editable author filter data.
- Stop if SQLite approximates unsupported PostgreSQL semantics silently.
- Stop if a read view implies mutation authority.

## Next action

Lower Census S0 from
[`plan.md`](../../../../radix/docs/factory/census-types/plan.md). The packet must
freeze exact live owner paths, PostgreSQL/SQLite fixtures, source-column mapping,
runtime representation, cursor ownership, saved-plan versioning, and the final
plan noun before any grammar or implementation unit is filed.
