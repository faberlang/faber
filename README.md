# Faber

Public Faber project and package tool (`faber` binary).

Builds, checks, runs, tests, formats, and interprets Faber packages. The
compiler engine lives in the private **Radix** workspace; this repository is
the user-facing product surface.

## Local development layout

```text
faberlang/
  faber/      this repo (public CLI)
  radix/      private compiler + runtime crates
  norma/      public standard library source
  examples/   public application examples
```

Path dependencies (not published):

- `../radix/crates/radix` — compiler library
- `../radix/crates/forma` — formatter
- `../radix/crates/faber-runtime` — generated Rust runtime (`use faber::…`)

## Build

```bash
cargo build --release
./target/release/faber --help
```

End users of released binaries do not need the Radix tree. Building this crate
from source requires a sibling Radix checkout.

## Commands

- `faber check` / `build` / `run` / `test`
- `faber script` — MIR interpret (no Cargo)
- `faber format`, `faber explain`, `faber targets`

See `faber --help` and after-help text for the full surface.
