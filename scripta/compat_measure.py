"""Shared code for the grammar x target compatibility measurement.

`scripta/emit-compat-json.py` writes `corpus/measurement/compat/<column>.json`
from per-exemplum verdicts. This module holds what every producer of those
verdicts shares: the column lists, the term join, the row arithmetic and the
JSON writer, plus the producer for the columns this repository can measure
itself.

A column is measurable here when its backend ships in this repository. Today
that is `faber` (the HIR application lane's Faber source emitter) and `runner`
(the MIR lane's in-process runner); `crates/radix/examples/compat_measure.rs`
produces their verdicts. The other columns need backends this repository does
not ship; a checkout that builds them measures them with its own producer.

A verdict map is `{corpus-relative path: {column: verdict}}` where a verdict is
`capable`, `gap`, `n/a` (the MIR lane never reached the classifier) or `no-hir`
(the HIR lane never reached the classifier). Only `capable` and `gap` count
toward a term's analyzable total.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Callable

ROOT = Path(__file__).resolve().parents[1]
CORPUS = ROOT / "corpus"

HIR_TARGETS = ["rust", "go", "ts", "faber"]
MIR_TARGETS = [
    "llvm-text",
    "wasm-text",
    "wasm",
    "metal-text",
    "wgsl-text",
    "sexp-struct",
    "sexp",
    "runner",
]
ALL_TARGETS = HIR_TARGETS + MIR_TARGETS
LANE_OF = {**{t: "hir" for t in HIR_TARGETS}, **{t: "mir" for t in MIR_TARGETS}}

# Columns whose backends ship in this repository, so any checkout can measure
# them. Every other column is measured only where its backend is built.
PUBLIC_TARGETS = ["faber", "runner"]

GENERATED_BY = "scripta/emit-compat-json.py"

# Environment: a captured `compat_measure` stdout replaces the cargo run.
MEASURE_LOG_ENV = "COMPAT_MEASURE_LOG"

MEASURE_CMD = [
    "cargo", "run", "-q", "-p", "radix", "--example", "compat_measure", "--",
]

VERDICTS = {"capable", "gap", "n/a", "no-hir"}


def load_terms(
    excluded: Callable[[str], bool] | None = None,
) -> list[tuple[str, str, list[str]]]:
    """[(term, kind, [corpus-rel-path, ...])] sorted by term, from exempla frontmatter.

    A term is declared by the one exemplum that lists it under `explains`. Its
    paths are that exemplum plus every exemplum whose own `term` is the same.

    `excluded` names corpus-relative paths that are left out of the join. A
    checkout that carries exempla the public repository does not ship passes
    the predicate that hides them, so every checkout measures the same corpus.
    """
    by_term: dict[str, set[str]] = {}
    declared: dict[str, tuple[str, str]] = {}
    for path in sorted(CORPUS.rglob("*.fab")):
        rel = path.relative_to(CORPUS).as_posix()
        if excluded is not None and excluded(rel):
            continue
        text = path.read_text(encoding="utf-8")
        if not text.startswith("+++\n"):
            continue
        kind = "unspecified"
        explains: list[str] = []
        for line in text[4 : text.find("\n+++", 4)].splitlines():
            if line.startswith("term = "):
                by_term.setdefault(json.loads(line[len("term = ") :]), set()).add(rel)
            elif line.startswith("kind = "):
                kind = json.loads(line[len("kind = ") :])
            elif line.startswith("explains = "):
                explains = json.loads(line[len("explains = ") :])
        for term in explains:
            declared[term] = (kind, rel)
    return [
        (term, kind, sorted(by_term.get(term, set()) | {primary}))
        for term, (kind, primary) in sorted(declared.items())
    ]


def target_rows(
    terms: list[tuple[str, str, list[str]]],
    verdicts: dict[str, dict[str, str]],
    target: str,
) -> tuple[list[dict], dict[str, int]]:
    """Per-term (capable, analyzable) rows for one target, plus the tally."""
    rows = []
    capable = 0
    analyzable = 0
    for term, rule, paths in terms:
        cap = 0
        ana = 0
        for p in paths:
            v = verdicts.get(p, {}).get(target)
            if v is None or v == "n/a" or v == "no-hir":
                continue
            ana += 1
            if v == "capable":
                cap += 1
        rows.append({"term": term, "rule": rule, "capable": cap, "analyzable": ana})
        capable += cap
        analyzable += ana
    return rows, {"capable": capable, "analyzable": analyzable}


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(
        json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8"
    )
    print(f"wrote {path}", file=sys.stderr)


def write_compat(
    out_root: Path,
    targets: list[str],
    terms: list[tuple[str, str, list[str]]],
    verdicts: dict[str, dict[str, str]],
) -> None:
    """Write `compat/<target>.json` for each target from the verdict map."""
    for target in targets:
        rows, summary = target_rows(terms, verdicts, target)
        write_json(
            out_root / "compat" / f"{target}.json",
            {
                "schemaVersion": 1,
                "artifact": "faber-compat-measurement",
                "target": target,
                "lane": LANE_OF[target],
                "generatedBy": GENERATED_BY,
                "summary": summary,
                "terms": rows,
            },
        )


def effective(verdict: str | None) -> str | None:
    """The part of a verdict that counts: `capable`, `gap`, or nothing."""
    return verdict if verdict in ("capable", "gap") else None


def disagreements(
    left: dict[str, dict[str, str]],
    right: dict[str, dict[str, str]],
    targets: list[str],
) -> list[str]:
    """Rows where two producers disagree on a column, as `path [column]: a != b`."""
    rows = []
    for rel in sorted(set(left) | set(right)):
        for target in targets:
            a = effective(left.get(rel, {}).get(target))
            b = effective(right.get(rel, {}).get(target))
            if a != b:
                rows.append(f"{rel} [{target}]: {a} != {b}")
    return rows


def merged(
    *maps: dict[str, dict[str, str]],
) -> dict[str, dict[str, str]]:
    """Union of verdict maps; later maps win per (path, column)."""
    out: dict[str, dict[str, str]] = {}
    for verdicts in maps:
        for rel, cells in verdicts.items():
            out.setdefault(rel, {}).update(cells)
    return out


def parse_measure_output(text: str, targets: list[str]) -> dict[str, dict[str, str]]:
    """Parse `compat_measure` stdout: `<path>\\t<column>\\t<verdict>[:<why>]` lines.

    Fails on a malformed line, an unrequested column, or a column that does not
    cover every measured path: a partial run must never read as a measurement.
    """
    verdicts: dict[str, dict[str, str]] = {}
    for number, line in enumerate(text.splitlines(), start=1):
        parts = line.split("\t")
        if len(parts) != 3:
            raise ValueError(f"measurement line {number} is not path/column/verdict: {line!r}")
        rel, target, cell = parts
        verdict = cell.split(":", 1)[0]
        if target not in targets:
            raise ValueError(f"measurement line {number} names unrequested column {target!r}")
        if verdict not in VERDICTS:
            raise ValueError(f"measurement line {number} has unknown verdict {cell!r}")
        slot = verdicts.setdefault(rel, {})
        if target in slot:
            raise ValueError(f"measurement line {number} repeats {rel} [{target}]")
        slot[target] = verdict
    if not verdicts:
        raise ValueError("measurement is empty: no exempla were measured")
    for rel, slot in sorted(verdicts.items()):
        missing = [t for t in targets if t not in slot]
        if missing:
            raise ValueError(f"measurement is incomplete: {rel} has no {', '.join(missing)} verdict")
    return verdicts


def measure_public(targets: list[str]) -> dict[str, dict[str, str]]:
    """Verdicts for columns this repository measures itself (`PUBLIC_TARGETS`)."""
    unknown = [t for t in targets if t not in PUBLIC_TARGETS]
    if unknown:
        raise ValueError(f"not measured by this repository's own backends: {', '.join(unknown)}")
    cached = os.environ.get(MEASURE_LOG_ENV)
    if cached:
        text = Path(cached).read_text(encoding="utf-8")
    else:
        command = [*MEASURE_CMD, *targets]
        print(f"running: {' '.join(command)}", file=sys.stderr)
        try:
            text = subprocess.run(
                command, cwd=ROOT, text=True, check=True,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE,
            ).stdout
        except subprocess.CalledProcessError as error:
            # Surface the tool's stderr so a failing measurement is visible, not
            # silent (the freshness gate prints the emit script's stderr on failure).
            stderr = (error.stderr or "").strip()
            if stderr:
                print(stderr, file=sys.stderr)
            raise
    return parse_measure_output(text, targets)
