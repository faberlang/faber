# Faber

Public Faber project and package tool (`faber` binary).

Builds, checks, runs, tests, formats, and interprets Faber packages. The
compiler engine lives in the private **Radix** workspace; this repository is
the user-facing product surface.

## Local development layout

```text
faberlang/
  faber/           this repo (public CLI)
  faber-runtime/   public Rust runtime types (`use faber::…`)
  radix/           private compiler (formatter is `radix::forma`)
  norma/           public standard library source
  examples/        public application examples
  cista/           public package-store CLI/lib
```

Path dependencies (not published):

- `../radix/crates/radix` — compiler library (includes `radix::forma` formatter and MIR stepper)
- `../radix/crates/hygiene-ratchet` — hygiene tests only (dev-dependency)

Generated packages from `faber build` depend on sibling **`faber-runtime`**
(package name `faber-runtime`, crate name `faber`), not on this CLI crate.

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
