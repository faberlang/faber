# Compatibility measurement JSON

Measured data behind [`grammar/EBNF_MATRIX.md`](../../grammar/EBNF_MATRIX.md)
and [`grammar/CONVERSIO_MATRIX.md`](../../grammar/CONVERSIO_MATRIX.md). **Do
not hand-edit.** [`scripta/render-matrices.py`](../../scripta/render-matrices.py)
renders both matrices from these files.

- `compat/<target>.json` — per-target grammar×target rows (terms with rule,
  capable and analyzable counts, plus a lane summary) for the EBNF matrix.
- `conversio/<target>.json` — per-target conversio (`↦`) cells (source, target,
  tier, measured flag, plus a lane summary).
- `conversio/disagreements.json` — measured-versus-predicted disagreement rows.
- `conversio-decimal/<target>.json` — the same cells for the decimal families.

## Where the numbers come from

The `compat/` files measure every corpus exemplar against each target column
(the lowerability classifier of the HIR lanes, the MIR emitters, and the
in-process runner). Which columns can be re-measured depends on whether the
column's backend ships in this repository:

| columns | where they are measured |
| --- | --- |
| `faber`, `runner` | **here.** Their backends ship in this repository, and `crates/radix/examples/compat_measure.rs` measures them. |
| `rust`, `go`, `ts`, `llvm-text`, `wasm-text`, `wasm`, `metal-text`, `wgsl-text`, `sexp-struct`, `sexp` | **in the private development repository only.** Those backends are built there, and the results are committed here at release. This repository cannot recompute them. |

The measured corpus is the exempla this repository ships. Exempla that only
the private repository carries are left out of the join there too, so every
checkout measures the same set.

The `conversio*/` files are a frozen record: the fixtures they measured no
longer live in this repository, and nothing regenerates them today.

## Checking

```bash
./scripta/check-compat-json-freshness       # regenerate faber and runner; fail on any byte of drift
python3 scripta/emit-compat-json.py         # rewrite those columns' JSON after a measured change
python3 scripta/render-matrices.py          # render both matrices in place
python3 scripta/render-matrices.py --check  # fail if the committed matrices are stale
```

The freshness check builds and runs the measurement example (a few minutes cold,
well under a minute warm), then requires the regenerated `faber.json` and
`runner.json` to equal the committed files byte for byte. A drifted column is
reported by term, with the class-aware diff of
`scripta/compare-compat-json.py`. A change that moves a measured column
regenerates its JSON and re-renders the matrices in the same commit.

The scripts need Python 3.11 or newer; the measurement also needs a Rust
toolchain. Without the private backends the checker skips the other columns, so
a green run here says nothing about them; it prints the columns it checked.
