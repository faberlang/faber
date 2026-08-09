# Residuals: stepper-exclusive `faber test`

**As of**: 2026-07-30 (stepper path landed)

## Shipped

- `faber test` executes proba on the MIR stepper only (no `invoke_cargo_test`,
  no `Target::Rust` emit, no generated test crate for the test path).
- Single-file `.fab` / `.proba` and simple package units with inventoriable
  proba cases run pass / fail / skip (`omitte` / `futurum`).
- Selection: `--name` / `--suite` / `--tag` / harness `--filter` / `.proba`
  `--include` / `--exclude`.
- Automated guards: radix `proba::` unit tests; faber `proba_stepper_test`;
  binary `cmd_test_source_has_no_cargo_or_rust_executor`.

## Fail-closed on real packages (expected)

| Consumer | Observation | Follow-on |
| --- | --- | --- |
| `triga` package / `math.proba` | Units with proba fail MIR lower (`method call before runtime/provider MIR lowering`, projection base) — cases named and failed, exit non-zero | Package MIR linking + stepper host/provider gaps for triga methods |
| `norma/src/mathesis.proba` as lone file | SEM type errors when analyzed without package/norma context | Run via package path with norma graph, or residual for single-file package context |
| `norma/exempla/caelum` | Discovery looks for `main.fab`; parse/package shape not test-ready as a package root | Exempla layout / package discovery residual |

**Policy**: no Rust emit fallback. These packages stay red on stepper until
capability/linking work lands; they are not silently skipped.

## Follow-on (out of this goal)

- Package-MIR linked proba runner (one program for multi-file helpers).
- Richer host I/O / provider surface so product packages can green on stepper.
- Corpus dual-purpose / scripta stage for proba (separate goals).
