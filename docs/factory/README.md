# Factory documentation (faber)

Open factory tracks for the public **`faber` CLI** and package product surface.

Relocated from private Radix on 2026-07-08. Compiler-only factory work remains
under `faberlang/radix/docs/factory/`.

## Layout (current)

```text
faberlang/
  faber/            this repo (CLI) — factory docs live here
  faber-runtime/    generated Rust runtime (`use faber::…`)
  radix/            private compiler (`radix::forma`, MIR stepper, exempla)
  norma/            public `norma:*` source
  cista/            package store
  examples/         application packages
```

Common commands from this repo:

```bash
cargo test
cargo run -- check <path>
cargo run -- run <path>
# compiler gates:
cargo test --manifest-path ../radix/Cargo.toml -p radix -- <filter>
```

Each `goal.md` / `CAMPAIGN.md` owns its **Status** line.

## Open campaigns

| Campaign | Status | Entry |
| --- | --- | --- |
| Aer Purgatus code-smell remediation | active — Goals 1–2 complete; Goal 3 selected for factory | [`aer-purgatus/CAMPAIGN.md`](aer-purgatus/CAMPAIGN.md) |
| Tabular data access (census, SQLite, ViviLite) | proposed — not selected | [`tabular-data-access/CAMPAIGN.md`](tabular-data-access/CAMPAIGN.md) |

## Open goals

| Goal | Status | Entry |
| ---- | ------ | ----- |
| SQLite library package | Stage 1 API/fixture contract complete - blocked on unified manifest target bindings | [`sqlite-library-package/goal.md`](sqlite-library-package/goal.md) |
| Unified package manifest | active - Phases 1-2 complete; Phases 3-4 open | [`unified-package-manifest/goal.md`](unified-package-manifest/goal.md) |
