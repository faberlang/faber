#!/usr/bin/env python3
"""Emit measurement JSON for the public compatibility matrices.

Runs the in-process exempla harness (emit_hir_target_matrix,
emit_mir_target_matrix) and writes per-target JSON under
radix/corpus/measurement/:

  compat/<target>.json          EBNF matrix rows for one target (hir/mir lanes)

The conversio/ and conversio-decimal/ measurement JSON is no longer emitted
here: its generator measured a fixture tree in the sibling examples repo, which
are not part of radix (single-repo-development U1, ruling Q6). The committed
files under corpus/measurement/conversio*/ are frozen records.

The public faber repository renders its matrices from this JSON
(faber/scripta/render-matrices.py); it never runs the exempla build itself.

This script is the measurement side only — no markdown is emitted here.

Usage:
  ./scripta/emit-compat-json.py

Env:
  COMPAT_OUT              output root (default radix/corpus/measurement)
  EBNF_MATRIX_HIR_LOG     captured emit_hir_target_matrix output (skip cargo)
  EBNF_MATRIX_MIR_LOG     captured emit_mir_target_matrix output (skip cargo)

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

CORPUS_ROOT = (ROOT / "corpus").as_posix()


def normalize_corpus_path(raw: str) -> str | None:
    """Reduce any exemplum path to a stable `corpus/<rel>` form or return None.

    Absolute paths are made relative to this script's own repository root, so
    the result does not depend on what the checkout directory is called.
    """
    raw = raw.replace("\\", "/").strip()
    if raw.startswith(CORPUS_ROOT + "/"):
        return "corpus/" + raw[len(CORPUS_ROOT) + 1:]
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
            if cell.startswith("runner panic:"):
                raise ValueError(
                    f"MIR target matrix contains a runner panic for {rel} [{target}]: {cell}"
                )
            if cell == "ok":
                slot[target] = "capable"
            elif cell == "n/a" or cell == "missing":
                slot[target] = "n/a"
            else:
                slot[target] = "gap"
    return verdicts


def load_terms() -> list[tuple[str, str, list[str]]]:
    """[(term, kind, [corpus-rel-path, ...])] sorted by term, from exempla frontmatter.

    A term is declared by the one exemplum that lists it under `explains`. Its
    paths are that exemplum plus every exemplum whose own `term` is the same.
    """
    corpus = ROOT / "corpus"
    by_term: dict[str, set[str]] = {}
    declared: dict[str, tuple[str, str]] = {}
    for path in sorted(corpus.rglob("*.fab")):
        text = path.read_text(encoding="utf-8")
        if not text.startswith("+++\n"):
            continue
        rel = path.relative_to(corpus).as_posix()
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


def main() -> int:
    out_root = Path(os.environ.get("COMPAT_OUT", DEFAULT_OUT))
    emit_compat(out_root)
    print(f"measurement JSON written under {out_root}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
