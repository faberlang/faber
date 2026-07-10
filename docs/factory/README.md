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
| Tabular data access (census, SQLite, ViviLite) | proposed — prerequisite facts refreshed; not selected | [`tabular-data-access/CAMPAIGN.md`](tabular-data-access/CAMPAIGN.md) |

## Completed campaigns

| Campaign | Status | Entry |
| --- | --- | --- |
| Aer Purgatus code-smell remediation | complete — Goals 1–4 complete; residual queue clear | [`aer-purgatus/CAMPAIGN.md`](aer-purgatus/CAMPAIGN.md) |

## Open goals

| Goal | Status | Entry |
| ---- | ------ | ----- |
| SQLite library package | Stage 1 contract complete — Phase 4 verification + G4 Rust linkage path available; Stage 2 product open | [`sqlite-library-package/goal.md`](sqlite-library-package/goal.md) |
| Unified package manifest | active — Phases 1–4 Rust native-binding path complete (G4); Go/TS product assembly later | [`unified-package-manifest/goal.md`](unified-package-manifest/goal.md) |
