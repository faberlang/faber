#!/usr/bin/env python3
"""Validate negative fixtures for the Faber session CLI contract checker."""

from __future__ import annotations

import contextlib
import copy
import importlib.util
import io
import os
import pathlib
import subprocess
import sys
import tempfile
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


UNSUPPORTED_TARGET_FIXTURE_ENV = "FABER_SESSION_CLI_NEGATIVE_UNSUPPORTED_TARGET_FIXTURE"


def check_unsupported_target_fixture(root: pathlib.Path) -> None:
    checker = root / "check-session-cli-contract.py"
    fixture = pathlib.Path(
        os.environ.get(
            UNSUPPORTED_TARGET_FIXTURE_ENV,
            root / "session-cli-contract-unsupported-target.toml",
        )
    )
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
        raise AssertionError("unsupported target fixture unexpectedly passed")
    if "admission.allowed_targets contains unknown targets ['unsupported-target']" not in result.stderr:
        print("FAIL unsupported target fixture failed for the wrong reason", file=sys.stderr)
        print(result.stderr, file=sys.stderr)
        raise AssertionError("unsupported target fixture failed for the wrong reason")


def check_unexpected_pass_regression(root: pathlib.Path) -> None:
    with tempfile.TemporaryDirectory(prefix="faber-session-negative-") as temporary:
        passing_fixture = pathlib.Path(temporary) / "passing-session-cli-contract.toml"
        passing_fixture.write_bytes((root / "session-cli-contract.toml").read_bytes())
        env = os.environ.copy()
        env[UNSUPPORTED_TARGET_FIXTURE_ENV] = str(passing_fixture)
        result = subprocess.run(
            ["python3", str(root / "check-session-cli-contract-negative.py")],
            text=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
            env=env,
        )
    if result.returncode == 0:
        raise AssertionError("negative checker process passed when unsupported-target fixture passed")
    if "unsupported target fixture unexpectedly passed" not in result.stderr:
        raise AssertionError(
            "negative checker process failed without the unsupported-target false-pass diagnostic"
        )


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent
    if len(sys.argv) > 2 or (len(sys.argv) == 2 and sys.argv[1] != "--self-test"):
        print("usage: check-session-cli-contract-negative.py [--self-test]", file=sys.stderr)
        return 1
    if len(sys.argv) == 2:
        check_unexpected_pass_regression(root)
        print("ok: session CLI contract negative checker self-test")
        return 0
    check_allowed_target_edges(root)
    check_unsupported_target_fixture(root)
    print("ok: session CLI contract negative fixtures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
