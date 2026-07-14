#!/usr/bin/env python3
"""Validate negative fixtures for the Faber session CLI contract checker."""

from __future__ import annotations

import contextlib
import copy
import importlib.util
import io
import pathlib
import subprocess
import sys
import tomllib


def load_checker(root: pathlib.Path):
    sys.dont_write_bytecode = True
    checker_path = root / "check-session-cli-contract.py"
    spec = importlib.util.spec_from_file_location("session_cli_contract_checker", checker_path)
    if spec is None or spec.loader is None:
        raise RuntimeError(f"could not load {checker_path}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def assert_rejects(checker, document: dict[str, object], expected_error: str) -> None:
    stderr = io.StringIO()
    try:
        with contextlib.redirect_stderr(stderr):
            checker.validate(document)
    except SystemExit as error:
        if error.code != 1:
            raise AssertionError(f"expected exit 1, got {error.code}")
    else:
        raise AssertionError(f"expected rejection containing {expected_error!r}")
    error_text = stderr.getvalue()
    if expected_error not in error_text:
        raise AssertionError(f"expected {expected_error!r} in {error_text!r}")


def with_allowed_targets(document: dict[str, object], allowed_targets: list[str]) -> dict[str, object]:
    mutated = copy.deepcopy(document)
    mutated["admission"]["allowed_targets"] = allowed_targets
    return mutated


def check_allowed_target_edges(root: pathlib.Path) -> None:
    checker = load_checker(root)
    with open(root / "session-cli-contract.toml", "rb") as handle:
        baseline = tomllib.load(handle)

    checker.validate(baseline)

    cases = [
        (
            "missing target",
            ["fmir-text", "fmir", "fmir-bin", "scena"],
            "admission.allowed_targets missing ['rust']",
        ),
        (
            "duplicate target",
            ["fmir-text", "fmir", "fmir-bin", "scena", "rust", "rust"],
            "admission.allowed_targets contains duplicates ['rust']",
        ),
        (
            "unknown target",
            ["fmir-text", "fmir", "fmir-bin", "scena", "rust", "unsupported-target"],
            "admission.allowed_targets contains unknown targets ['unsupported-target']",
        ),
        (
            "non-canonical order",
            ["fmir-text", "fmir-bin", "fmir", "scena", "rust"],
            "admission.allowed_targets must be exactly ['fmir-text', 'fmir', 'fmir-bin', 'scena', 'rust']",
        ),
    ]

    for name, allowed_targets, expected_error in cases:
        try:
            assert_rejects(checker, with_allowed_targets(baseline, allowed_targets), expected_error)
        except AssertionError as error:
            raise AssertionError(f"{name}: {error}") from error


def check_unsupported_target_fixture(root: pathlib.Path) -> None:
    checker = root / "check-session-cli-contract.py"
    fixture = root / "session-cli-contract-unsupported-target.toml"
    result = subprocess.run(
        ["python3", str(checker), str(fixture)],
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        timeout=30,
    )
    if result.returncode == 0:
        print("FAIL unsupported target fixture unexpectedly passed", file=sys.stderr)
        print(result.stdout, file=sys.stderr)
        return 1
    if "admission.allowed_targets contains unknown targets ['unsupported-target']" not in result.stderr:
        print("FAIL unsupported target fixture failed for the wrong reason", file=sys.stderr)
        print(result.stderr, file=sys.stderr)
        raise AssertionError("unsupported target fixture failed for the wrong reason")


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent
    check_allowed_target_edges(root)
    check_unsupported_target_fixture(root)
    print("ok: session CLI contract negative fixtures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
