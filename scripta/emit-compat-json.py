#!/usr/bin/env python3
"""Emit measurement JSON for the public compatibility matrices.

Measures the exempla corpus against the matrix columns and writes one file per
column under corpus/measurement/:

  compat/<column>.json          EBNF matrix rows for one column (hir/mir lanes)

grammar/EBNF_MATRIX.md is rendered from these files by
scripta/render-matrices.py. This script is the measurement side only; no
markdown is emitted here.

Which columns it can emit depends on the checkout. A column is measurable
where its backend is built:

  faber, runner    ship in this repository; measured by
                   crates/radix/examples/compat_measure.rs (compat_measure.py)
  the other ten    need backends this repository does not ship. A checkout
                   that builds them adds scripta/compat_extra.py, which
                   measures every column; without it this script reports them
                   as not measured here and leaves their committed JSON alone

Usage:
  ./scripta/emit-compat-json.py                    # every column measurable here
  ./scripta/emit-compat-json.py --targets faber    # a subset of those columns
  ./scripta/emit-compat-json.py --list             # print the measurable columns

Env:
  COMPAT_OUT           output root (default corpus/measurement)
  COMPAT_MEASURE_LOG   captured compat_measure output (skip cargo)

The freshness gate (scripta/check-compat-json-freshness) regenerates the
measurable columns to a temp root and diffs them against the committed JSON.
Commit the regenerated JSON in the same change when the measured baseline is
meant to move.
"""

from __future__ import annotations

import argparse
import importlib.util
import os
import sys
from pathlib import Path

import compat_measure as cm

ROOT = cm.ROOT
DEFAULT_OUT = ROOT / "corpus" / "measurement"
EXTRA_PRODUCER = ROOT / "scripta" / "compat_extra.py"


def load_extra_producer():
    """The checkout's own producer for the remaining columns, if it has one."""
    if not EXTRA_PRODUCER.is_file():
        return None
    spec = importlib.util.spec_from_file_location("compat_extra", EXTRA_PRODUCER)
    module = importlib.util.module_from_spec(spec)
    sys.modules["compat_extra"] = module
    spec.loader.exec_module(module)
    return module


def measurable(extra) -> list[str]:
    return list(extra.TARGETS) if extra else list(cm.PUBLIC_TARGETS)


def measure(targets: list[str], extra) -> dict[str, dict[str, str]]:
    """Verdicts for the requested columns, from whichever producers apply."""
    public = [t for t in targets if t in cm.PUBLIC_TARGETS]
    verdicts = cm.measure_public(public) if public else {}
    if extra and any(t not in cm.PUBLIC_TARGETS for t in targets):
        harness = extra.measure()
        if public:
            rows = cm.disagreements(harness, verdicts, public)
            if rows:
                shown = "\n  ".join(rows[:20])
                raise SystemExit(
                    f"error: the extra producer and the in-repository measurement disagree on "
                    f"{len(rows)} verdicts (first {min(len(rows), 20)}):\n  {shown}"
                )
        verdicts = cm.merged(harness, verdicts)
    return verdicts


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--targets", help="comma-separated columns (default: all measurable here)")
    parser.add_argument("--list", action="store_true", help="print the measurable columns and exit")
    args = parser.parse_args(argv)

    extra = load_extra_producer()
    available = measurable(extra)
    if args.list:
        print("\n".join(available))
        return 0

    targets = args.targets.split(",") if args.targets else available
    unknown = [t for t in targets if t not in cm.ALL_TARGETS]
    if unknown:
        print(f"error: unknown column(s): {', '.join(unknown)}", file=sys.stderr)
        return 2
    unavailable = [t for t in targets if t not in available]
    if unavailable:
        print(
            f"error: not measurable in this checkout: {', '.join(unavailable)} "
            f"(this checkout does not build their backends; "
            f"measurable here: {', '.join(available)})",
            file=sys.stderr,
        )
        return 2

    terms = cm.load_terms(extra.corpus_excluded if extra else None)
    verdicts = measure(targets, extra)
    out_root = Path(os.environ.get("COMPAT_OUT", DEFAULT_OUT))
    cm.write_compat(out_root, targets, terms, verdicts)
    skipped = [t for t in cm.ALL_TARGETS if t not in targets]
    print(f"measurement JSON written under {out_root}: {', '.join(targets)}", file=sys.stderr)
    if skipped:
        print(f"not measured in this run: {', '.join(skipped)}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
