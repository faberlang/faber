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

The `compat/` files are the output of an in-process harness that lowers every
corpus exemplar through each target backend. It runs in the private development
repository, which carries the non-public backends, and the results are committed
here at release. The `conversio*/` files are a frozen record: the fixtures they
measured no longer live in this repository, and nothing regenerates them today.

## Checking

```bash
python3 scripta/render-matrices.py          # render both matrices in place
python3 scripta/render-matrices.py --check  # fail if the committed matrices are stale
```

Both need Python 3.11 or newer.
