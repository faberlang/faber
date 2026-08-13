# Compatibility Measurement JSON

Generated measurement data for the public compatibility matrices. **Do not
hand-edit.** These files are the raw output of the in-process exempla harness,
serialized per target by [`scripta/emit-compat-json.py`](../../scripta/emit-compat-json.py).

## What this is

The public faber repository renders
[`docs/EBNF_MATRIX.md`](../../../faber/docs/EBNF_MATRIX.md) and
[`docs/CONVERSIO_MATRIX.md`](../../../faber/docs/CONVERSIO_MATRIX.md) from
these files via `faber/scripta/render-matrices.py` — it never re-runs the
exempla build. Radix owns the measurement; the public repo owns the
presentation.

- `compat/<target>.json` — per-target grammar×target rows (terms with rule,
  capable, analyzable counts + lane summary) for the EBNF support matrix.
- `conversio/<target>.json` — per-target conversio (`↦`) cells (src, tgt,
  tier, measured flag) + lane summary.
- `conversio/disagreements.json` — measured-vs-predicted disagreement rows.

## Regeneration

Regenerated at the **end of the test ladder**: `./scripta/test --full`,
`--release`, or `--stage 4a` run the measurement gates, which include
`scripta/check-compat-json-freshness`. That gate regenerates to a temp root
and fails if the committed files are stale — the fix is to run
`./scripta/emit-compat-json.py` and commit the regenerated JSON.

## Committing

The regenerated JSON is a **known artifact committed as part of a release**
(see the Radix release protocol in `AGENTS.md`). Between releases the files
stay byte-stable; a non-release change that makes them stale fails the ladder
rather than silently drifting.
