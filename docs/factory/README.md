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
