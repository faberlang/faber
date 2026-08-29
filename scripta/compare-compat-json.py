#!/usr/bin/env python3
"""Compare compatibility measurements without pinning reference totals.

A measurement change is classified by the capability it carries:

* new rows and larger capable/analyzable counts are additive and pass;
* a capable/analyzable row or count going down is regressive and fails;
* other valid measurement changes are recorded but do not turn a reference
  number into a gate.

The comparison is keyed by term or conversion cell, so output ordering and
summary totals cannot hide the exact row that regressed.
"""

from __future__ import annotations

import argparse
import json
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Any


@dataclass
class DiffResult:
    additions: list[str] = field(default_factory=list)
    regressions: list[str] = field(default_factory=list)
    changes: list[str] = field(default_factory=list)


def _json_files(root: Path) -> dict[str, Path]:
    if not root.is_dir():
        raise ValueError(f"measurement root is not a directory: {root}")
    return {
        path.relative_to(root).as_posix(): path
        for path in root.rglob("*.json")
    }


def _load(path: Path, rel: str, result: DiffResult) -> Any | None:
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        result.regressions.append(f"{rel}: candidate/baseline JSON is invalid: {error}")
        return None


def _identity_check(
    baseline: dict[str, Any], candidate: dict[str, Any], rel: str, result: DiffResult
) -> None:
    # These fields identify the measurement contract.  They are not numeric
    # pins, but changing or dropping them would make the comparison ambiguous.
    keys = ["schemaVersion", "artifact"]
    if baseline.get("artifact") != "faber-conversio-disagreements":
        keys.extend(("target", "lane"))
    for key in keys:
        if key not in baseline or key not in candidate:
            result.regressions.append(f"{rel}: missing measurement identity field {key}")
        elif baseline[key] != candidate[key]:
            result.regressions.append(
                f"{rel}: measurement identity {key} changed "
                f"from {baseline[key]!r} to {candidate[key]!r}"
            )


def _int_field(row: dict[str, Any], key: str, rel: str, label: str, result: DiffResult) -> int | None:
    value = row.get(key)
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        result.regressions.append(f"{rel} {label}: invalid non-negative integer {key}={value!r}")
        return None
    return value


def _compare_counter(
    baseline: dict[str, Any],
    candidate: dict[str, Any],
    key: str,
    rel: str,
    label: str,
    result: DiffResult,
    *,
    regressive: bool = True,
) -> None:
    old = _int_field(baseline, key, rel, label, result)
    new = _int_field(candidate, key, rel, label, result)
    if old is None or new is None or old == new:
        return
    detail = f"{rel} {label} {old} -> {new}"
    if new < old and regressive:
        result.regressions.append(detail)
    elif new < old:
        result.changes.append(detail)
    else:
        result.additions.append(detail)


def _unique_rows(
    payload: dict[str, Any], key: str, rel: str, result: DiffResult
) -> dict[str, dict[str, Any]]:
    rows = payload.get(key)
    if not isinstance(rows, list):
        result.regressions.append(f"{rel}: {key} is not an array")
        return {}
    indexed: dict[str, dict[str, Any]] = {}
    for index, row in enumerate(rows):
        if not isinstance(row, dict):
            result.regressions.append(f"{rel} {key}[{index}]: row is not an object")
            continue
        if key == "terms":
            identity = row.get("term")
            label = f"term={identity}"
        else:
            src = row.get("src")
            tgt = row.get("tgt")
            identity = f"{src}->{tgt}"
            label = f"cell={identity}"
        if not isinstance(identity, str):
            result.regressions.append(f"{rel} {key}[{index}]: missing row identity")
            continue
        if identity in indexed:
            result.regressions.append(f"{rel} {label}: duplicate row")
            continue
        indexed[identity] = row
    return indexed


def _compare_compat(
    baseline: dict[str, Any], candidate: dict[str, Any], rel: str, result: DiffResult
) -> None:
    old_summary = baseline.get("summary")
    new_summary = candidate.get("summary")
    if not isinstance(old_summary, dict) or not isinstance(new_summary, dict):
        result.regressions.append(f"{rel}: summary is not an object")
        return
    for key in ("capable", "analyzable"):
        _compare_counter(
            baseline=old_summary,
            candidate=new_summary,
            key=key,
            rel=rel,
            label=f"summary.{key}",
            result=result,
        )

    old_rows = _unique_rows(baseline, "terms", rel, result)
    new_rows = _unique_rows(candidate, "terms", rel, result)
    for term in sorted(set(old_rows) | set(new_rows)):
        old = old_rows.get(term)
        new = new_rows.get(term)
        label = f"term={term}"
        if old is None:
            new_capable = _int_field(new, "capable", rel, label, result)
            new_analyzable = _int_field(new, "analyzable", rel, label, result)
            if new_capable is not None and new_analyzable is not None:
                result.additions.append(
                    f"{rel} {label}: new row "
                    f"capable={new_capable} analyzable={new_analyzable}"
                )
            continue
        if new is None:
            old_capable = _int_field(old, "capable", rel, label, result)
            old_analyzable = _int_field(old, "analyzable", rel, label, result)
            if (old_capable is not None and old_capable > 0) or (
                old_analyzable is not None and old_analyzable > 0
            ):
                result.regressions.append(f"{rel} {label}: row removed")
            else:
                result.changes.append(f"{rel} {label}: unmeasured row removed")
            continue
        for key in ("capable", "analyzable"):
            old_value = _int_field(old, key, rel, label, result)
            new_value = _int_field(new, key, rel, label, result)
            if old_value is None or new_value is None or old_value == new_value:
                continue
            detail = f"{rel} {label} {key} {old_value} -> {new_value}"
            if new_value < old_value:
                result.regressions.append(detail)
            else:
                result.additions.append(detail)
        if old.get("rule") != new.get("rule"):
            result.changes.append(
                f"{rel} {label} rule {old.get('rule')!r} -> {new.get('rule')!r}"
            )


TIER_RANK = {
    "not-emitted": 0,
    "rejected": 1,
    "fallback": 2,
    "dedicated": 3,
}


def _compare_tier(
    old: dict[str, Any],
    new: dict[str, Any],
    rel: str,
    label: str,
    result: DiffResult,
) -> None:
    old_tier = old.get("tier")
    new_tier = new.get("tier")
    if old_tier not in TIER_RANK:
        result.regressions.append(f"{rel} {label}: unknown baseline tier {old_tier!r}")
        return
    if new_tier not in TIER_RANK:
        result.regressions.append(f"{rel} {label}: unknown candidate tier {new_tier!r}")
        return
    if TIER_RANK[new_tier] < TIER_RANK[old_tier]:
        result.regressions.append(
            f"{rel} {label} tier {old_tier} -> {new_tier}"
        )
    elif TIER_RANK[new_tier] > TIER_RANK[old_tier]:
        result.additions.append(
            f"{rel} {label} tier {old_tier} -> {new_tier}"
        )
    elif old_tier != new_tier:
        result.changes.append(f"{rel} {label} tier {old_tier} -> {new_tier}")

    old_measured = old.get("measured")
    new_measured = new.get("measured")
    if old_measured is True and new_measured is False:
        result.regressions.append(f"{rel} {label}: measured true -> false")
    elif old_measured is False and new_measured is True:
        result.additions.append(f"{rel} {label}: measured false -> true")


def _compare_conversion(
    baseline: dict[str, Any], candidate: dict[str, Any], rel: str, result: DiffResult
) -> None:
    old_summary = baseline.get("summary")
    new_summary = candidate.get("summary")
    if not isinstance(old_summary, dict) or not isinstance(new_summary, dict):
        result.regressions.append(f"{rel}: summary is not an object")
        return
    for key in TIER_RANK:
        _compare_counter(
            baseline=old_summary,
            candidate=new_summary,
            key=key,
            rel=rel,
            label=f"summary.{key}",
            result=result,
            regressive=False,
        )

    old_cells = _unique_rows(baseline, "cells", rel, result)
    new_cells = _unique_rows(candidate, "cells", rel, result)
    for cell in sorted(set(old_cells) | set(new_cells)):
        old = old_cells.get(cell)
        new = new_cells.get(cell)
        label = f"cell={cell}"
        if old is None:
            new_tier = new.get("tier")
            if new_tier not in TIER_RANK:
                result.regressions.append(f"{rel} {label}: unknown candidate tier {new_tier!r}")
            else:
                result.additions.append(f"{rel} {label}: new conversion row")
            continue
        if new is None:
            old_tier = old.get("tier")
            if old_tier not in TIER_RANK:
                result.regressions.append(f"{rel} {label}: unknown baseline tier {old_tier!r}")
            elif TIER_RANK[old_tier] > TIER_RANK["not-emitted"]:
                result.regressions.append(f"{rel} {label}: conversion row removed")
            else:
                result.changes.append(f"{rel} {label}: unmeasured conversion row removed")
            continue
        _compare_tier(old, new, rel, label, result)


def _compare_json(
    baseline: Any, candidate: Any, rel: str, result: DiffResult
) -> None:
    if not isinstance(baseline, dict) or not isinstance(candidate, dict):
        if baseline != candidate:
            result.changes.append(f"{rel}: non-object measurement changed")
        return
    _identity_check(baseline, candidate, rel, result)
    artifact = baseline.get("artifact")
    if artifact != candidate.get("artifact"):
        return
    if artifact == "faber-compat-measurement":
        _compare_compat(baseline, candidate, rel, result)
    elif artifact in {
        "faber-conversio-measurement",
        "faber-conversio-decimal-measurement",
    }:
        _compare_conversion(baseline, candidate, rel, result)
    elif baseline != candidate:
        # Disagreement rows and future measurement metadata are records, not
        # capability pins.  The typed artifact identity above still fails
        # closed if the file ceases to be the expected measurement contract.
        result.changes.append(f"{rel}: non-capability measurement content changed")


def compare_roots(baseline_root: Path, candidate_root: Path) -> DiffResult:
    result = DiffResult()
    baseline_files = _json_files(baseline_root)
    candidate_files = _json_files(candidate_root)
    for rel in sorted(set(baseline_files) | set(candidate_files)):
        baseline_path = baseline_files.get(rel)
        candidate_path = candidate_files.get(rel)
        if baseline_path is None:
            result.additions.append(f"{rel}: new measurement artifact")
            continue
        if candidate_path is None:
            result.regressions.append(f"{rel}: measurement artifact removed")
            continue
        baseline = _load(baseline_path, rel, result)
        candidate = _load(candidate_path, rel, result)
        if baseline is not None and candidate is not None:
            _compare_json(baseline, candidate, rel, result)
    return result


def format_report(result: DiffResult) -> str:
    if result.regressions:
        state = "FAIL (regressive delta)"
    elif result.additions:
        state = "PASS (additive-only)"
    elif result.changes:
        state = "PASS (non-capability changes recorded)"
    else:
        state = "PASS (fresh)"
    lines = [f"class-aware measurement diff: {state}"]
    for heading, rows in (
        ("recorded additive delta", result.additions),
        ("recorded non-capability change", result.changes),
        ("regression", result.regressions),
    ):
        lines.extend(f"  {heading}: {row}" for row in rows)
    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("baseline", type=Path)
    parser.add_argument("candidate", type=Path)
    args = parser.parse_args(argv)
    try:
        result = compare_roots(args.baseline, args.candidate)
    except (OSError, ValueError) as error:
        print(f"class-aware measurement diff: FAIL (comparison error): {error}", file=sys.stderr)
        return 1
    print(format_report(result), file=sys.stderr if result.regressions else sys.stdout)
    return 1 if result.regressions else 0


if __name__ == "__main__":
    raise SystemExit(main())
