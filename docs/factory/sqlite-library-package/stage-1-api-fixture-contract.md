# SQLite Package Stage 1 API And Fixture Contract

**Status**: Stage 1 contract complete — Phase 4 verification available;
implementation blocked on the Phase 3 backend library build graph.
**Created**: 2026-07-09
**Refreshed**: 2026-07-10
**Applies to**: Stage 2 Rust binding prototype and Stage 3 ViviLite read oracle.

## Invariant

`sqlite` is a Faber library package whose Faber source declares the public API
and whose Rust target manifest declares the `rusqlite` implementation. No
Faber source file may use durable `@ externa`, `@ subsidia`, or `cista.toml`
linkage for this package.

## Public Module And API

The provider is `sqlite`. The first public module path is exactly
`sqlite:sqlite`.

Consumers import the package as:

```fab
importa ex "sqlite:sqlite" privata sqlite
```

The Stage 2 source facade exposes these declarations:

```fab
genus SQLiteEffect {
    numerus rows_changed
    numerus last_insert_id
}

functio exsequi(textus via, textus sql, lista<valor> params) → SQLiteEffect ⇥ textus
functio quaere(textus via, textus sql, lista<valor> params) → lista<valor> ⇥ textus
functio scalar(textus via, textus sql, lista<valor> params) → valor ∪ nihil ⇥ textus
```

Contract:

- `via` is a filesystem path to the SQLite database.
- `sql` is passed to SQLite as a prepared statement.
- `params` bind positionally to SQLite placeholders in statement order.
- `exsequi` is for statements that do not return rows. It returns changed row
  count and last insert rowid.
- `quaere` returns every row as a `valor` tabula keyed by result column name.
- `scalar` returns the first column of the first row; zero rows returns `nihil`.
- SQL, path, bind, conversion, and SQLite engine errors return through
  `⇥ textus`.

`scalar` deliberately ignores additional rows and columns after the first cell
for v1. Callers that need shape validation should use `quaere`.

## Valor Mapping

SQLite parameter mapping:

| Faber `valor` | SQLite bind value |
| --- | --- |
| `nihil` | NULL |
| `bivalens` | INTEGER 0 or 1 |
| `numerus` | INTEGER i64 |
| `fractus` | REAL f64 |
| `textus` | TEXT |
| `octeti` | BLOB |
| `instans` | TEXT using its RFC3339 wire string |

Unsupported parameter values:

- `lista<valor>`
- `tabula<textus, valor>`

Unsupported parameters fail with `⇥ textus`; they are not stringified or encoded
as JSON implicitly.

SQLite result mapping:

| SQLite value | Faber `valor` |
| --- | --- |
| NULL | `nihil` |
| INTEGER | `numerus` |
| REAL | `fractus` |
| TEXT | `textus` |
| BLOB | `octeti` |

The binding does not infer booleans, JSON, dates, or `instans` from SQLite type
affinity. Callers may parse JSON with `norma:json` or interpret text timestamps
after reading rows.

Known `valor` gaps and follow-ups:

- SQLite INTEGER values outside signed i64 are not representable as `numerus`.
- SQLite decimal affinity has no exact decimal carrier; REAL maps to `fractus`.
- Duplicate result column names collapse in a `tabula`; queries must use unique
  aliases when duplicate labels matter.
- `tabula` key order is sorted by runtime map order, not original SELECT column
  order. Semantic comparisons must not depend on object field order.
- SQLite has no native boolean or datetime type. Boolean and instans handling is
  caller convention over INTEGER/TEXT.

These are not blockers for Stage 2 or ViviLite read parity because the required
Vivi fields are text, signed counts, nullable text, and row objects with unique
aliases.

## Tiny SQLite Fixture

Stage 2 must include a fixture database, created from SQL equivalent to:

```sql
CREATE TABLE identities (
  name TEXT PRIMARY KEY,
  address TEXT NOT NULL
);

CREATE TABLE work_items (
  handle TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK (kind IN ('task', 'need', 'want')),
  status TEXT NOT NULL CHECK (status IN ('open', 'done')),
  role TEXT NOT NULL,
  from_identity TEXT NOT NULL,
  to_identity TEXT NOT NULL,
  subject TEXT NOT NULL,
  body TEXT NOT NULL,
  priority INTEGER NOT NULL,
  created_at TEXT NOT NULL,
  done_at TEXT
);

INSERT INTO identities VALUES
  ('codex', 'codex@fixture.local'),
  ('reviewer', 'reviewer@fixture.local');

INSERT INTO work_items VALUES
  ('task-001', 'task', 'open', 'tasks', 'codex', 'codex',
   'SQLite package fixture', 'Prove row mapping.', 2,
   '2026-07-09T00:00:00Z', NULL),
  ('need-001', 'need', 'open', 'needs', 'reviewer', 'codex',
   'Choose binding key', 'Resolve binding manifest key grammar.', 1,
   '2026-07-09T00:01:00Z', NULL),
  ('want-001', 'want', 'done', 'done', 'codex', 'codex',
   'Later polish', 'Closed fixture item.', 3,
   '2026-07-09T00:02:00Z', '2026-07-09T00:03:00Z');
```

Required query proof:

```sql
SELECT
  handle,
  kind,
  status,
  role,
  from_identity AS "from",
  to_identity AS "to",
  subject,
  priority,
  done_at
FROM work_items
WHERE to_identity = ?
ORDER BY created_at, handle;
```

with params:

```fab
["codex"]
```

Expected first row shape:

```text
tabula {
  "done_at": nihil,
  "from": textus("codex"),
  "handle": textus("task-001"),
  "kind": textus("task"),
  "priority": numerus(2),
  "role": textus("tasks"),
  "status": textus("open"),
  "subject": textus("SQLite package fixture"),
  "to": textus("codex")
}
```

Required error cases:

- missing database path;
- invalid SQL syntax;
- bind arity mismatch;
- unsupported parameter shape (`lista` or `tabula`);
- write statement passed through `quaere` if SQLite reports no query columns;
- duplicate column labels documented by a test using aliases as the remedy.

## Stage 2 Binding Draft — Phase 4-Verified Surface

Candidate Stage 2 package layout; detailed delivery research must decide how
Phase 3 packages the Rust implementation crate:

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
  fixtures/
    work_items.sql
```

`faber.toml` shape using the verified Phase 4 fields:

```toml
[package]
name = "sqlite"
version = "0.1.0"
edition = "2026"

[library]
provider = "sqlite"

[paths]
source = "src"

[build]
kind = "lib"
targets = ["rust"]

[target.rust]
bindings = "bindings/rust.toml"

[target.rust.dependencies]
rusqlite = { version = "0.32", features = ["bundled"] }
```

Binding manifest shape using the verified `provider:module/path.function` key
grammar and shim module:

```toml
[functions."sqlite:sqlite.exsequi"]
symbol = "crate::shim::exsequi"

[functions."sqlite:sqlite.quaere"]
symbol = "crate::shim::quaere"

[functions."sqlite:sqlite.scalar"]
symbol = "crate::shim::scalar"

[shim]
path = "rust/src/lib.rs"
```

Stage 2 acceptance:

- `faber install <sqlite-package-path>` installs provider `sqlite`.
- `faber verify-library --target rust <sqlite-package-path>` validates all three
  bindings.
- A fixture consumer imports `sqlite:sqlite`, reads the SQL fixture with
  `quaere`, and compares semantic `valor` rows.
- `exsequi` inserts one row using bound params and returns
  `SQLiteEffect { rows_changed: 1, last_insert_id: <non-negative numerus> }`.
- `scalar` returns `nihil` for zero rows and the first selected cell for a
  matching row.
- The implementation links `rusqlite`; it does not shell out to `sqlite3`.

## ViviLite Stage 2 Oracle Contract

ViviLite Stage 2 must generate a temporary fixture mailspace with regular
`vivi`; it must not read or mutate `/Users/ianzepp/work/faberlang/.vivi` or any
other live project store.

Fixture setup outline:

```bash
fixture="$(mktemp -d)"
vivi mailspace init --project "$fixture"
vivi mailspace identity add codex --project "$fixture"
vivi mailspace identity add reviewer --project "$fixture"
vivi task send --project "$fixture" --from codex --to codex --subject "Task A" --body "body"
vivi need send --project "$fixture" --from reviewer --to codex --subject "Need A" --body "body"
vivi want send --project "$fixture" --from codex --to codex --subject "Want A" --body "body"
```

Oracle commands:

```bash
vivi mailspace status --project "$fixture" --json > expected-status.json
vivi task list --project "$fixture" --for codex --status open --json > expected-tasks.json
vivi need list --project "$fixture" --for codex --status open --json > expected-needs.json
vivi want list --project "$fixture" --for codex --json > expected-wants.json
vivi board --project "$fixture" --for codex --json > expected-board.json
```

ViviLite commands must read the same fixture through `sqlite:sqlite` and emit
matching JSON for the selected surfaces. Compare semantically, not by raw text:

- status: `name`, identity rows, and totals;
- work lists: `handle`, `kind`, `status`, `role`, `date`, `from`, `to`,
  `subject`, and `last_event` fields;
- board: `name`, `totals`, identity address, task/need/want item handles and
  subjects, and hidden want counts;
- object field order is ignored;
- extra internal SQLite columns are ignored unless they appear in public JSON.

ViviLite Stage 2 is read-only. Write compatibility belongs to ViviLite Stage 3.

## Blockers

- Unified package manifest Phase 4 is complete for verification. It defines the
  binding manifest, bodyless-declaration contract, missing-binding diagnostics,
  shim/dependency probe, and `faber verify-library` command.
- Unified package manifest Phase 3 remains the implementation blocker: a built
  application does not yet consume the verified library shim and target
  dependencies through a generated/backend library graph.
- Detailed delivery research must decide the SQLite package repository/path,
  prove the Phase 3 linkage route, and define the later dynamic-row-to-census
  adapter. This Stage 1 contract does not settle those questions.
