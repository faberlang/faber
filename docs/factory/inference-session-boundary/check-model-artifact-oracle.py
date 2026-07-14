#!/usr/bin/env python3
"""Validate the Faber inference model artifact oracle manifest.

This check intentionally validates only static handoff metadata. It must not
open model/tokenizer artifacts, load model formats, run tokenizers, or execute
an inference runtime.
"""

from __future__ import annotations

import contextlib
import copy
import io
import re
import sys
import tomllib
from pathlib import Path, PurePosixPath


REQUIRED_NON_CLAIMS = {
    "no public inference support",
    "no llama.cpp equivalence",
    "no GGUF runtime",
    "no safetensors runtime",
    "no transformer runtime",
    "no tokenizer runtime",
    "no quantized-kernel runtime",
    "no GPU runtime",
    "no model download",
    "no model loading",
    "no runtime execution",
}

REJECTED_FORMATS = {
    "gguf",
    "safetensors",
    "transformer",
    "quantized-kernel",
    "gpu-runtime",
}

EXPECTED_ALLOWED_FORMATS = ("oracle",)
EXPECTED_REJECTED_FORMATS = (
    "gguf",
    "safetensors",
    "transformer",
    "quantized-kernel",
    "gpu-runtime",
)

EXPECTED_ARGV_CONTRACT = ("prompt",)

ALLOWED_RUNTIME_REQUIREMENTS = {
    "fmir-cli-args",
    "oracle-fixture",
}

REQUIRED_EVIDENCE_CHECKS = {
    "static metadata only",
    "relative contained artifact paths",
    "checksum syntax only",
    "oracle format only",
    "explicit non-claims",
}

SHA256 = re.compile(r"^[0-9a-f]{64}$")


def fail(message: str) -> None:
    print(f"model artifact oracle check failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def table(document: dict[str, object], name: str) -> dict[str, object]:
    value = document.get(name)
    if not isinstance(value, dict):
        fail(f"missing table [{name}]")
    return value


def string(value: object, field: str) -> str:
    if not isinstance(value, str) or not value:
        fail(f"{field} must be a non-empty string")
    return value


def string_list(value: object, field: str) -> list[str]:
    if not isinstance(value, list) or not all(isinstance(item, str) for item in value):
        fail(f"{field} must be a string array")
    return value


def exact_string_list(value: object, field: str, expected: tuple[str, ...]) -> list[str]:
    items = string_list(value, field)
    if items != list(expected):
        fail(f"{field} must be exactly {list(expected)}")
    return items


def contained_relative_path(value: object, field: str) -> None:
    path = PurePosixPath(string(value, field))
    if path.is_absolute():
        fail(f"{field} must be relative")
    if ".." in path.parts:
        fail(f"{field} must not contain '..'")
    if not path.parts:
        fail(f"{field} must name a contained artifact path")
    if path.parts[0] != "fixtures":
        fail(f"{field} must stay under fixtures/")


def checksum(value: object, field: str) -> None:
    digest = string(value, field)
    if not SHA256.fullmatch(digest):
        fail(f"{field} must be lowercase hex sha256 syntax")


def validate(document: dict[str, object]) -> None:
    model = table(document, "model")
    tokenizer = table(document, "tokenizer")
    session = table(document, "session")
    runtime = table(document, "runtime")
    policy = table(document, "policy")
    evidence = table(document, "evidence")

    string(model.get("id"), "model.id")
    if string(model.get("format"), "model.format") != "oracle":
        fail("model.format must remain oracle-only")
    contained_relative_path(model.get("path"), "model.path")
    checksum(model.get("sha256"), "model.sha256")

    contained_relative_path(tokenizer.get("path"), "tokenizer.path")
    checksum(tokenizer.get("sha256"), "tokenizer.sha256")

    string(session.get("entry"), "session.entry")
    target = string(session.get("target"), "session.target")
    if target not in {"fmir-text", "fmir", "fmir-bin", "scena", "rust"}:
        fail(f"session.target is not a Faber package target: {target}")
    argv_contract = string_list(session.get("argv_contract"), "session.argv_contract")
    if argv_contract != list(EXPECTED_ARGV_CONTRACT):
        fail(f"session.argv_contract must be exactly {list(EXPECTED_ARGV_CONTRACT)}")

    requirement_list = string_list(runtime.get("requirements"), "runtime.requirements")
    duplicates = sorted(
        {
            requirement
            for requirement in requirement_list
            if requirement_list.count(requirement) > 1
        }
    )
    if duplicates:
        fail(f"runtime.requirements contains duplicates {duplicates}")
    requirements = set(requirement_list)
    unknown_requirements = sorted(requirements - ALLOWED_RUNTIME_REQUIREMENTS)
    if unknown_requirements:
        fail(f"runtime.requirements contains unknown entries {unknown_requirements}")
    missing_requirements = sorted(ALLOWED_RUNTIME_REQUIREMENTS - requirements)
    if missing_requirements:
        fail(f"runtime.requirements missing {missing_requirements}")
    if requirements & REJECTED_FORMATS:
        fail("runtime.requirements must not name real model/runtime formats")

    allowed = exact_string_list(policy.get("allowed_formats"), "policy.allowed_formats", EXPECTED_ALLOWED_FORMATS)
    rejected = string_list(policy.get("rejected_formats"), "policy.rejected_formats")
    duplicate_rejections = sorted(
        {
            format_name
            for format_name in rejected
            if rejected.count(format_name) > 1
        }
    )
    if duplicate_rejections:
        fail(f"policy.rejected_formats contains duplicates {duplicate_rejections}")
    if set(allowed) & set(rejected):
        fail("policy.allowed_formats and policy.rejected_formats overlap")
    unknown_rejections = sorted(set(rejected) - set(EXPECTED_REJECTED_FORMATS))
    if unknown_rejections:
        fail(f"policy.rejected_formats contains unknown entries {unknown_rejections}")
    missing_rejections = [format_name for format_name in EXPECTED_REJECTED_FORMATS if format_name not in rejected]
    if missing_rejections:
        fail(f"policy.rejected_formats missing {missing_rejections}")
    if rejected != list(EXPECTED_REJECTED_FORMATS):
        fail(f"policy.rejected_formats must be exactly {list(EXPECTED_REJECTED_FORMATS)}")
    for field in ("model_loading", "tokenizer_loading", "runtime_execution"):
        if string(policy.get(field), f"policy.{field}") != "disabled":
            fail(f"policy.{field} must be disabled")

    checks = set(string_list(evidence.get("checks"), "evidence.checks"))
    missing_checks = sorted(REQUIRED_EVIDENCE_CHECKS - checks)
    if missing_checks:
        fail(f"evidence.checks missing {missing_checks}")
    unknown_checks = sorted(checks - REQUIRED_EVIDENCE_CHECKS)
    if unknown_checks:
        fail(f"evidence.checks contains unknown entries {unknown_checks}")
    non_claims = set(string_list(evidence.get("non_claims"), "evidence.non_claims"))
    missing_non_claims = sorted(REQUIRED_NON_CLAIMS - non_claims)
    if missing_non_claims:
        fail(f"evidence.non_claims missing {missing_non_claims}")


def assert_rejects(document: dict[str, object], expected_error: str) -> None:
    stderr = io.StringIO()
    try:
        with contextlib.redirect_stderr(stderr):
            validate(document)
    except SystemExit as error:
        if error.code != 1:
            raise AssertionError(f"expected exit 1, got {error.code}")
    else:
        raise AssertionError(f"expected rejection containing {expected_error!r}")
    error_text = stderr.getvalue()
    if expected_error not in error_text:
        raise AssertionError(f"expected {expected_error!r} in {error_text!r}")


def with_mutation(
    document: dict[str, object],
    table_name: str,
    key: str,
    value: object,
) -> dict[str, object]:
    mutated = copy.deepcopy(document)
    table_value = mutated[table_name]
    if not isinstance(table_value, dict):
        raise AssertionError(f"fixture table {table_name} is not a table")
    table_value[key] = value
    return mutated


def self_test() -> None:
    root = Path(__file__).resolve().parent
    with open(root / "model-artifact-oracle.toml", "rb") as handle:
        baseline = tomllib.load(handle)

    validate(baseline)
    cases = [
        (
            "unknown runtime requirement",
            with_mutation(
                baseline,
                "runtime",
                "requirements",
                ["fmir-cli-args", "oracle-fixture", "external-provider"],
            ),
            "runtime.requirements contains unknown entries ['external-provider']",
        ),
        (
            "missing runtime requirement",
            with_mutation(baseline, "runtime", "requirements", ["oracle-fixture"]),
            "runtime.requirements missing ['fmir-cli-args']",
        ),
        (
            "oracle overlap in rejected formats",
            with_mutation(
                baseline,
                "policy",
                "rejected_formats",
                ["oracle", "gguf", "safetensors", "transformer", "quantized-kernel", "gpu-runtime"],
            ),
            "policy.allowed_formats and policy.rejected_formats overlap",
        ),
        (
            "unknown rejected format",
            with_mutation(
                baseline,
                "policy",
                "rejected_formats",
                ["gguf", "safetensors", "transformer", "quantized-kernel", "gpu-runtime", "image"],
            ),
            "policy.rejected_formats contains unknown entries ['image']",
        ),
        (
            "duplicate rejected format",
            with_mutation(
                baseline,
                "policy",
                "rejected_formats",
                ["gguf", "gguf", "transformer", "quantized-kernel", "gpu-runtime"],
            ),
            "policy.rejected_formats contains duplicates ['gguf']",
        ),
        (
            "missing rejected format",
            with_mutation(
                baseline,
                "policy",
                "rejected_formats",
                ["gguf", "safetensors", "transformer", "quantized-kernel"],
            ),
            "policy.rejected_formats missing ['gpu-runtime']",
        ),
        (
            "order drift in rejected formats",
            with_mutation(
                baseline,
                "policy",
                "rejected_formats",
                ["gpu-runtime", "quantized-kernel", "transformer", "safetensors", "gguf"],
            ),
            "policy.rejected_formats must be exactly ['gguf', 'safetensors', 'transformer', 'quantized-kernel', 'gpu-runtime']",
        ),
        (
            "empty argv contract",
            with_mutation(baseline, "session", "argv_contract", []),
            "session.argv_contract must be exactly ['prompt']",
        ),
        (
            "extra argv contract",
            with_mutation(baseline, "session", "argv_contract", ["prompt", "temperature"]),
            "session.argv_contract must be exactly ['prompt']",
        ),
        (
            "weakened evidence checks",
            with_mutation(baseline, "evidence", "checks", ["static metadata only"]),
            "evidence.checks missing",
        ),
    ]

    for name, document, expected_error in cases:
        try:
            assert_rejects(document, expected_error)
        except AssertionError as error:
            raise AssertionError(f"{name}: {error}") from error


def main() -> None:
    if len(sys.argv) == 2 and sys.argv[1] == "--self-test":
        self_test()
        print("ok: model artifact oracle negative self-test")
        return
    manifest = (
        sys.argv[1]
        if len(sys.argv) > 1
        else "docs/factory/inference-session-boundary/model-artifact-oracle.toml"
    )
    if len(sys.argv) > 2:
        fail("usage: check-model-artifact-oracle.py [manifest]")
    with open(manifest, "rb") as handle:
        validate(tomllib.load(handle))
    print(f"ok: {manifest}")


if __name__ == "__main__":
    main()
