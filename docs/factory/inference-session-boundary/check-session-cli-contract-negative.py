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


def with_field(document: dict[str, object], table: str, field: str, value: object) -> dict[str, object]:
    mutated = copy.deepcopy(document)
    table_value = mutated[table]
    if not isinstance(table_value, dict):
        raise AssertionError(f"fixture table {table} is not a table")
    table_value[field] = value
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


def check_exact_contract_fields(root: pathlib.Path) -> None:
    checker = load_checker(root)
    with open(root / "session-cli-contract.toml", "rb") as handle:
        baseline = tomllib.load(handle)

    checker.validate(baseline)

    cases = [
        (
            "package root argument",
            with_field(baseline, "command", "package_root_arg", "<pkg>"),
            "command.package_root_arg must be <package-root>",
        ),
        (
            "separator",
            with_field(baseline, "command", "separator", "---"),
            "command.separator must be --",
        ),
        (
            "package root input contract",
            with_field(baseline, "inputs", "package_root", "optional"),
            "inputs.package_root must be required-relative-or-absolute-path",
        ),
        (
            "model manifest identity",
            with_field(baseline, "inputs", "model_manifest", "unchecked-any-file"),
            "inputs.model_manifest must be oracle-only-model-artifact-manifest",
        ),
        (
            "existing package root requirement",
            with_field(baseline, "admission", "requires_existing_package_root", False),
            "admission.requires_existing_package_root must be true",
        ),
        (
            "stdout contract",
            with_field(baseline, "stdout", "contract", "free-form session text"),
            "stdout.contract must be line-delimited session events",
        ),
        (
            "stderr contract",
            with_field(baseline, "stderr", "contract", "warnings and diagnostics"),
            "stderr.contract must be diagnostics only",
        ),
        (
            "stderr required prefix",
            with_field(baseline, "stderr", "required_failure_prefix", "session:"),
            "stderr.required_failure_prefix must be faber session:",
        ),
    ]

    for name, document, expected_error in cases:
        try:
            assert_rejects(checker, document, expected_error)
        except AssertionError as error:
            raise AssertionError(f"{name}: {error}") from error


def check_runtime_requirement_edges(root: pathlib.Path) -> None:
    checker = load_checker(root)
    with open(root / "session-cli-contract.toml", "rb") as handle:
        baseline = tomllib.load(handle)

    checker.validate(baseline)

    cases = [
        (
            "missing runtime requirement",
            ["oracle-fixture"],
            "admission.required_runtime_requirements missing ['fmir-cli-args']",
        ),
        (
            "duplicate runtime requirement",
            ["fmir-cli-args", "oracle-fixture", "oracle-fixture"],
            "admission.required_runtime_requirements contains duplicates ['oracle-fixture']",
        ),
        (
            "unknown runtime requirement",
            ["fmir-cli-args", "oracle-fixture", "external-provider"],
            "admission.required_runtime_requirements contains unknown entries ['external-provider']",
        ),
        (
            "non-canonical runtime requirement order",
            ["oracle-fixture", "fmir-cli-args"],
            "admission.required_runtime_requirements must be exactly ['fmir-cli-args', 'oracle-fixture']",
        ),
    ]

    for name, requirements, expected_error in cases:
        try:
            assert_rejects(
                checker,
                with_field(baseline, "admission", "required_runtime_requirements", requirements),
                expected_error,
            )
        except AssertionError as error:
            raise AssertionError(f"{name}: {error}") from error


def check_required_failure_rows(root: pathlib.Path) -> None:
    checker = load_checker(root)
    with open(root / "session-cli-contract.toml", "rb") as handle:
        baseline = tomllib.load(handle)

    checker.validate(baseline)

    cases = [
        (
            "unsupported format",
            with_field(baseline, "failures", "unsupported_format", "may continue"),
            "failures.unsupported_format must reject before package execution",
        ),
        (
            "missing non-claim",
            with_field(baseline, "failures", "missing_non_claim", "may continue"),
            "failures.missing_non_claim must reject before package execution",
        ),
        (
            "unchecked path",
            with_field(baseline, "failures", "unchecked_path", "may continue"),
            "failures.unchecked_path must reject before package execution",
        ),
        (
            "unknown runtime requirement",
            with_field(baseline, "failures", "unknown_runtime_requirement", "may continue"),
            "failures.unknown_runtime_requirement must reject before package execution",
        ),
        (
            "runtime execution requested",
            with_field(baseline, "failures", "runtime_execution_requested", "may continue"),
            "failures.runtime_execution_requested must reject before package execution",
        ),
    ]

    for name, document, expected_error in cases:
        try:
            assert_rejects(checker, document, expected_error)
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


def check_required_failure_fixture(root: pathlib.Path) -> None:
    checker = load_checker(root)
    with open(root / "session-cli-contract.toml", "rb") as handle:
        baseline = tomllib.load(handle)

    mutated = with_field(
        baseline,
        "failures",
        "runtime_execution_requested",
        "may continue into package execution",
    )
    try:
        assert_rejects(
            checker,
            mutated,
            "failures.runtime_execution_requested must reject before package execution",
        )
    except AssertionError as error:
        raise AssertionError(f"required failure fixture: {error}") from error


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


def check_required_failure_self_test(root: pathlib.Path) -> None:
    check_required_failure_fixture(root)


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent
    if len(sys.argv) > 2 or (
        len(sys.argv) == 2
        and sys.argv[1] not in {"--self-test", "--self-test-required-failure"}
    ):
        print(
            "usage: check-session-cli-contract-negative.py [--self-test|--self-test-required-failure]",
            file=sys.stderr,
        )
        return 1
    if len(sys.argv) == 2 and sys.argv[1] == "--self-test":
        check_exact_contract_fields(root)
        check_runtime_requirement_edges(root)
        check_unexpected_pass_regression(root)
        check_required_failure_self_test(root)
        print("ok: session CLI contract negative checker self-test")
        return 0
    if len(sys.argv) == 2:
        check_required_failure_self_test(root)
        print("ok: session CLI contract required-failure self-test")
        return 0
    check_allowed_target_edges(root)
    check_exact_contract_fields(root)
    check_runtime_requirement_edges(root)
    check_required_failure_rows(root)
    check_required_failure_fixture(root)
    check_unsupported_target_fixture(root)
    print("ok: session CLI contract negative fixtures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
