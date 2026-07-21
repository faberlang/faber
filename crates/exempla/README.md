# exempla — Faber corpus e2e harness

Private-to-CI integration harness for the public language corpus. Lives in the
**faber** product repo because:

- single-file / compiler lanes use **radix** in-process;
- package lanes (`norma:*`, multi-file packages) use **faber_cli** package APIs;
- corpus sources live in public sibling **`examples/`** (and `norma/exempla/`).

This crate is **not** part of the private radix workspace. Radix must build from
a standalone checkout without this package.

## Layout

```text
faber/
  crates/exempla/     this crate
  Cargo.toml          workspace root (members: ., crates/exempla, hygiene-ratchet)
examples/corpus/      public .fab language reference (resolved at runtime)
```

## Run

From the faber repo root:

```bash
cargo test -p exempla --lib
cargo test -p exempla --lib mir_target_coverage_matrix -- --ignored --nocapture
cargo test -p exempla --test e2e_harness exempla_rust_e2e -- --ignored --nocapture
```

Radix `./scripta/test --full` forwards the matrix/parity subset here via
`--manifest-path ../faber/Cargo.toml`.

## Paths

See `src/paths.rs`. Overrides:

| Env | Meaning |
| --- | --- |
| `FABER_EXEMPLA_CORPUS` | `examples/corpus` root |
| `FABER_EXAMPLES_HOME` | `examples` repo root |
| `FABER_NORMA_EXEMPLA` | `norma/exempla` root |
