#!/usr/bin/env python3
"""Emit measurement JSON for the public compatibility matrices.

Runs the in-process exempla harness (emit_hir_target_matrix,
emit_mir_target_matrix, emit_conversio_target_matrix) and writes per-target
JSON under radix/corpus/measurement/:

  compat/<target>.json          EBNF matrix rows for one target (hir/mir lanes)
  conversio/<target>.json       conversio cells for one target (hir/mir lanes)
  conversio/disagreements.json  measured-vs-predicted disagreements

The public faber repository renders its matrices from this JSON
(faber/scripta/render-matrices.py); it never runs the exempla build itself.

This script is the measurement side only — no markdown is emitted here.

Usage:
  ./scripta/emit-compat-json.py

Env:
  COMPAT_OUT              output root (default radix/corpus/measurement)
  EBNF_MATRIX_HIR_LOG     captured emit_hir_target_matrix output (skip cargo)
  EBNF_MATRIX_MIR_LOG     captured emit_mir_target_matrix output (skip cargo)
  CONVERSIO_MATRIX_LOG    captured emit_conversio_target_matrix output (skip cargo)
  CONVERSIO_MATRIX_FIXTURES  fixture tree root (default ../examples/conversio-matrix)

The freshness gate (scripta/check-compat-json-freshness, wired into
check-measurement-gates / stage 4a) regenerates to a temp root and runs the
class-aware diff. Equality with the committed record is the only state
reported fresh; additive drift fails by default (recorded only under an
explicit --accept) and capability regressions always fail. Commit the
regenerated JSON in the same change when the measured baseline is meant
to move.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUT = ROOT / "corpus" / "measurement"

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

CARGO_HIR = [
    "cargo", "test", "-p", "exempla", "--lib", "emit_hir_target_matrix",
    "--", "--ignored", "--nocapture",
]
# The MIR emitters live behind the exempla `mir-core` lane feature
# (crates/exempla/src/exempla_e2e/mod.rs); without it the tests compile to
# nothing and the measurement would silently be empty.
CARGO_MIR = [
    "cargo", "test", "-p", "exempla", "--lib", "emit_mir_target_matrix",
    "--features", "mir-core",
    "--", "--ignored", "--nocapture",
]
CARGO_CONVERSIO = [
    "cargo", "test", "-p", "exempla", "--lib", "emit_conversio_target_matrix",
    "--features", "mir-core",
    "--", "--ignored", "--nocapture",
]

# Conversio family universe in stable matrix order (ConversioTypeFamily::ALL).
FAMILIES = [
    "numerus", "fractus", "bivalens", "textus", "ascii", "octeti",
    "regex", "json", "valor", "instans", "modulus",
    "lista", "tabula", "copia", "intervallum",
    "tensor", "vector", "matrix", "sparsa",
    "cursor", "promissum",
]

CONV_HIR_TARGETS = ["rust", "ts", "go", "faber"]
CONV_MIR_TARGETS = ["llvm-text", "wasm-text", "wasm", "wgsl-text", "sexp-struct", "sexp"]


def run_or_read(log_env: str, cargo_cmd: list[str]) -> str:
    cached = os.environ.get(log_env)
    if cached:
        return Path(cached).read_text(encoding="utf-8")
    print(f"running: {' '.join(cargo_cmd)}", file=sys.stderr)
    try:
        return subprocess.check_output(
            cargo_cmd, cwd=ROOT, text=True, stderr=subprocess.PIPE
        )
    except subprocess.CalledProcessError as error:
        # Surface the harness stderr so a failing measurement is visible, not
        # silent (the freshness gate prints this script's stderr on failure).
        stderr = (error.stderr or "").strip()
        if stderr:
            print(stderr, file=sys.stderr)
        raise


# --- HIR/MIR matrix measurement parsing (same contract as the retired
# --- generate-ebnf-matrix.py; measurement only) -----------------------------

def normalize_corpus_path(raw: str) -> str | None:
    """Reduce any exemplum path to a stable `corpus/<rel>` form or return None."""
    raw = raw.replace("\\", "/").strip()
    for marker in (
        "/radix/corpus/",
        "radix/corpus/",
        "/examples/corpus/",
        "examples/corpus/",
    ):
        idx = raw.find(marker)
        if idx >= 0:
            return "corpus/" + raw[idx + len(marker):]
    if raw.endswith(".fab") and "/" in raw and not raw.startswith(("/", ".")):
        return f"corpus/{raw}"
    return None


def corpus_rel(path: str) -> str | None:
    norm = normalize_corpus_path(path)
    if not norm:
        return None
    if norm.startswith("corpus/"):
        return norm[len("corpus/"):]
    return norm


def parse_hir(text: str) -> dict[str, dict[str, str]]:
    """path -> {target: 'capable' | 'gap:<slugs>' | 'no-hir'}."""
    verdicts: dict[str, dict[str, str]] = {}
    for line in text.splitlines():
        parts = line.split("\t")
        if len(parts) < 3:
            continue
        raw_path, target, verdict = parts[0], parts[1], parts[2]
        if target not in HIR_TARGETS:
            continue
        rel = corpus_rel(raw_path)
        if not rel:
            continue
        slot = verdicts.setdefault(rel, {})
        if verdict == "no-hir":
            slot[target] = "no-hir"
        elif verdict == "capable":
            slot[target] = "capable"
        elif verdict.startswith("gaps:"):
            slot[target] = "gap"
    return verdicts


def parse_mir(text: str) -> dict[str, dict[str, str]]:
    """path -> {target: 'capable' | 'gap' | 'n/a'}."""
    verdicts: dict[str, dict[str, str]] = {}
    in_rows = False
    for line in text.splitlines():
        if line.strip() == "ROWS":
            in_rows = True
            continue
        if not in_rows:
            continue
        parts = line.split("|")
        if len(parts) < 3:
            continue
        rel = corpus_rel(parts[0])
        if not rel:
            continue
        cells = parts[2:]
        slot = verdicts.setdefault(rel, {})
        for target, cell in zip(MIR_TARGETS, cells):
            cell = cell.strip()
            if cell == "ok":
                slot[target] = "capable"
            elif cell == "n/a" or cell == "missing":
                slot[target] = "n/a"
            else:
                slot[target] = "gap"
    return verdicts


TERM_RE = re.compile(r'term\s*=\s*"([^"]+)"')
RULE_RE = re.compile(r'rule\s*=\s*"([^"]+)"')
PATHS_RE = re.compile(r'paths\s*=\s*\[(.*?)\]', re.S)
PATH_ELEM_RE = re.compile(r'"([^"]+)"')


def load_terms() -> list[tuple[str, str, list[str]]]:
    """[(term, rule, [corpus-rel-path, ...])] in corpus/index.toml order."""
    index = ROOT / "corpus" / "index.toml"
    text = index.read_text(encoding="utf-8")
    terms: list[tuple[str, str, list[str]]] = []
    for block in text.split("[[terms]]")[1:]:
        term_m = TERM_RE.search(block)
        rule_m = RULE_RE.search(block)
        paths_m = PATHS_RE.search(block)
        if not term_m:
            continue
        term = term_m.group(1)
        rule = rule_m.group(1) if rule_m else "unspecified"
        paths = PATH_ELEM_RE.findall(paths_m.group(1)) if paths_m else []
        terms.append((term, rule, paths))
    return terms


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


def emit_compat(out_root: Path) -> None:
    hir_text = run_or_read("EBNF_MATRIX_HIR_LOG", CARGO_HIR)
    mir_text = run_or_read("EBNF_MATRIX_MIR_LOG", CARGO_MIR)
    hir = parse_hir(hir_text)
    mir = parse_mir(mir_text)
    terms = load_terms()

    for target in HIR_TARGETS:
        rows, summary = target_rows(terms, hir, target)
        write_json(
            out_root / "compat" / f"{target}.json",
            {
                "schemaVersion": 1,
                "artifact": "faber-compat-measurement",
                "target": target,
                "lane": "hir",
                "generatedBy": "radix/scripta/emit-compat-json.py",
                "summary": summary,
                "terms": rows,
            },
        )
    for target in MIR_TARGETS:
        rows, summary = target_rows(terms, mir, target)
        write_json(
            out_root / "compat" / f"{target}.json",
            {
                "schemaVersion": 1,
                "artifact": "faber-compat-measurement",
                "target": target,
                "lane": "mir",
                "generatedBy": "radix/scripta/emit-compat-json.py",
                "summary": summary,
                "terms": rows,
            },
        )


# --- conversio measurement parsing ------------------------------------------

_FIXTURES_OVERRIDE = os.environ.get("CONVERSIO_MATRIX_FIXTURES")
FIXTURES_ROOT = (
    Path(_FIXTURES_OVERRIDE)
    if _FIXTURES_OVERRIDE
    else ROOT.parent / "examples" / "conversio-matrix"
)


def fixture_backed(src: str, tgt: str) -> bool:
    return (FIXTURES_ROOT / src / f"{tgt}.fab").is_file()


def parse_section(text: str, marker: str) -> list[str]:
    lines = text.splitlines()
    start = None
    for i, line in enumerate(lines):
        stripped = line.strip()
        if stripped == marker or stripped.endswith(f"... {marker}"):
            start = i
            break
    if start is None:
        return []
    rows = []
    for line in lines[start + 1:]:
        stripped = line.strip()
        if not stripped or stripped in ("ROWS", "MEASURED", "DISAGREEMENTS", "DECIMAL ROWS"):
            break
        rows.append(line)
    return rows


def parse_tsv(text: str) -> dict[tuple[str, str, str], str]:
    verdicts: dict[tuple[str, str, str], str] = {}
    for line in parse_section(text, "ROWS"):
        parts = line.split("\t")
        if len(parts) != 4:
            continue
        src, tgt, target, verdict = parts
        verdicts[(src, tgt, target)] = verdict
    return verdicts


def parse_disagreements(text: str) -> list[dict]:
    rows = []
    for line in parse_section(text, "DISAGREEMENTS"):
        parts = line.split("\t")
        if len(parts) != 6:
            continue
        src, tgt, target, predicted, measured, detail = parts
        rows.append(
            {
                "src": src,
                "tgt": tgt,
                "target": target,
                "predicted": predicted,
                "measured": measured,
                "detail": detail,
            }
        )
    return rows


# Decimal conversio rows (decimal-widths goal D6). Width-refined cells in the
# same stable order as the harness `DECIMAL_CELLS` table.
DECIMAL_CELL_IDS = [
    "d64-i64",
    "i64-d64",
    "d64-u32",
    "u32-d64",
    "fractus-d64",
    "d64-fractus",
    "textus-d64",
    "d64-textus",
]


def parse_decimal_rows(text: str) -> dict[tuple[str, str], str]:
    rows: dict[tuple[str, str], str] = {}
    for line in parse_section(text, "DECIMAL ROWS"):
        parts = line.split("\t")
        if len(parts) != 4:
            continue
        cell_id, target, verdict, _detail = parts
        rows[(cell_id, target)] = verdict
    return rows


def emit_conversio_decimal(out_root: Path) -> None:
    text = run_or_read("CONVERSIO_MATRIX_LOG", CARGO_CONVERSIO)
    rows = parse_decimal_rows(text)

    for target in CONV_HIR_TARGETS + CONV_MIR_TARGETS:
        lane = "hir" if target in CONV_HIR_TARGETS else "mir"
        summary = {"dedicated": 0, "fallback": 0, "rejected": 0, "not-emitted": 0}
        cells = []
        for cell_id in DECIMAL_CELL_IDS:
            src, tgt = cell_id.split("-", 1)
            verdict = rows.get((cell_id, target), "not-emitted")
            summary[verdict] = summary.get(verdict, 0) + 1
            cells.append(
                {
                    "src": src,
                    "tgt": tgt,
                    "tier": verdict,
                    "measured": True,
                }
            )
        write_json(
            out_root / "conversio-decimal" / f"{target}.json",
            {
                "schemaVersion": 1,
                "artifact": "faber-conversio-decimal-measurement",
                "target": target,
                "lane": lane,
                "generatedBy": "radix/scripta/emit-compat-json.py",
                "summary": summary,
                "cells": cells,
            },
        )


def emit_conversio(out_root: Path) -> None:
    text = run_or_read("CONVERSIO_MATRIX_LOG", CARGO_CONVERSIO)
    verdicts = parse_tsv(text)
    disagreements = parse_disagreements(text)

    for target in CONV_HIR_TARGETS + CONV_MIR_TARGETS:
        lane = "hir" if target in CONV_HIR_TARGETS else "mir"
        summary = {"dedicated": 0, "fallback": 0, "rejected": 0, "not-emitted": 0}
        cells = []
        for src in FAMILIES:
            for tgt in FAMILIES:
                verdict = verdicts.get((src, tgt, target), "not-emitted")
                summary[verdict] = summary.get(verdict, 0) + 1
                cells.append(
                    {
                        "src": src,
                        "tgt": tgt,
                        "tier": verdict,
                        "measured": fixture_backed(src, tgt),
                    }
                )
        write_json(
            out_root / "conversio" / f"{target}.json",
            {
                "schemaVersion": 1,
                "artifact": "faber-conversio-measurement",
                "target": target,
                "lane": lane,
                "generatedBy": "radix/scripta/emit-compat-json.py",
                "summary": summary,
                "cells": cells,
            },
        )

    write_json(
        out_root / "conversio" / "disagreements.json",
        {
            "schemaVersion": 1,
            "artifact": "faber-conversio-disagreements",
            "generatedBy": "radix/scripta/emit-compat-json.py",
            "rows": disagreements,
        },
    )


def main() -> int:
    out_root = Path(os.environ.get("COMPAT_OUT", DEFAULT_OUT))
    emit_compat(out_root)
    emit_conversio(out_root)
    emit_conversio_decimal(out_root)
    print(f"measurement JSON written under {out_root}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
