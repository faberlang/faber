#!/usr/bin/env python3
"""Validate the Faber inference session CLI contract artifact.

This checker validates a future command contract only. It must not run `faber`,
load model/tokenizer artifacts, or execute an inference runtime.
"""

from __future__ import annotations

import sys
import tomllib


REQUIRED_NON_CLAIMS = {
    "no public inference support",
    "no llama.cpp equivalence",
    "no GGUF runtime",
    "no safetensors runtime",
    "no transformer runtime",
    "no tokenizer runtime",
    "no quantized-kernel runtime",
    "no GPU runtime",
    "no model loading",
    "no runtime execution",
    "no implemented faber session command",
}

REQUIRED_FAILURES = {
    "unsupported_format",
    "missing_non_claim",
    "unchecked_path",
    "unknown_runtime_requirement",
    "runtime_execution_requested",
}

EXPECTED_ALLOWED_TARGETS = ("fmir-text", "fmir", "fmir-bin", "scena", "rust")
EXPECTED_PACKAGE_ROOT_ARG = "<package-root>"
EXPECTED_PACKAGE_ROOT_CONTRACT = "required-relative-or-absolute-path"
EXPECTED_SEPARATOR = "--"
EXPECTED_STDOUT_CONTRACT = "line-delimited session events"
EXPECTED_STDERR_CONTRACT = "diagnostics only"
EXPECTED_FAILURE_PREFIX = "faber session:"
EXPECTED_MODEL_MANIFEST = "oracle-only-model-artifact-manifest"
EXPECTED_ORACLE_RUNTIME_REQUIREMENTS = ("fmir-cli-args", "oracle-fixture")
EXPECTED_SESSION_ARGS = ("prompt",)
EXPECTED_ACCEPTED_MANIFEST_FORMATS = ("oracle",)
EXPECTED_REJECTED_MANIFEST_FORMATS = (
    "gguf",
    "safetensors",
    "transformer",
    "quantized-kernel",
    "gpu-runtime",
)
EXPECTED_ALLOWED_STDOUT_EVENTS = (
    "artifact",
    "diagnostic",
    "oracle-request",
    "oracle-result",
)
EXPECTED_FORBIDDEN_STDOUT_EVENTS = (
    "token",
    "logit",
    "gpu-kernel",
    "model-loaded",
)


def fail(message: str) -> None:
    print(f"session CLI contract check failed: {message}", file=sys.stderr)
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


def bool_value(value: object, field: str) -> bool:
    if not isinstance(value, bool):
        fail(f"{field} must be boolean")
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


def validate(document: dict[str, object]) -> None:
    command = table(document, "command")
    inputs = table(document, "inputs")
    admission = table(document, "admission")
    stdout = table(document, "stdout")
    stderr = table(document, "stderr")
    failures = table(document, "failures")
    evidence = table(document, "evidence")

    if string(command.get("name"), "command.name") != "faber session":
        fail("command.name must remain faber session")
    if string(command.get("status"), "command.status") != "contract-only":
        fail("command.status must remain contract-only")
    shape = string(command.get("shape"), "command.shape")
    for required in (
        "faber session",
        EXPECTED_PACKAGE_ROOT_ARG,
        "--model-manifest",
        "<manifest>",
        EXPECTED_SEPARATOR,
        "<session-args>",
    ):
        if required not in shape:
            fail(f"command.shape must include {required}")
    if string(command.get("package_root_arg"), "command.package_root_arg") != EXPECTED_PACKAGE_ROOT_ARG:
        fail(f"command.package_root_arg must be {EXPECTED_PACKAGE_ROOT_ARG}")
    if string(command.get("model_manifest_flag"), "command.model_manifest_flag") != "--model-manifest":
        fail("command.model_manifest_flag must be --model-manifest")
    if string(command.get("separator"), "command.separator") != EXPECTED_SEPARATOR:
        fail(f"command.separator must be {EXPECTED_SEPARATOR}")

    if string(inputs.get("package_root"), "inputs.package_root") != EXPECTED_PACKAGE_ROOT_CONTRACT:
        fail(f"inputs.package_root must be {EXPECTED_PACKAGE_ROOT_CONTRACT}")
    if string(inputs.get("model_manifest"), "inputs.model_manifest") != EXPECTED_MODEL_MANIFEST:
        fail(f"inputs.model_manifest must be {EXPECTED_MODEL_MANIFEST}")
    session_args = exact_string_list(
        inputs.get("session_args"),
        "inputs.session_args",
        EXPECTED_SESSION_ARGS,
    )
    accepted_manifest_formats = exact_string_list(
        inputs.get("accepted_manifest_formats"),
        "inputs.accepted_manifest_formats",
        EXPECTED_ACCEPTED_MANIFEST_FORMATS,
    )
    rejected_manifest_formats = string_list(
        inputs.get("rejected_manifest_formats"),
        "inputs.rejected_manifest_formats",
    )
    manifest_overlap = sorted(set(accepted_manifest_formats) & set(rejected_manifest_formats))
    if manifest_overlap:
        fail(f"inputs accepted/rejected manifest formats overlap {manifest_overlap}")
    if set(rejected_manifest_formats) != set(EXPECTED_REJECTED_MANIFEST_FORMATS):
        fail(
            f"inputs.rejected_manifest_formats must be exactly "
            f"{list(EXPECTED_REJECTED_MANIFEST_FORMATS)}"
        )
    if rejected_manifest_formats != list(EXPECTED_REJECTED_MANIFEST_FORMATS):
        fail(
            f"inputs.rejected_manifest_formats must be exactly "
            f"{list(EXPECTED_REJECTED_MANIFEST_FORMATS)}"
        )

    if "check-model-artifact-oracle.py" not in string(admission.get("manifest_checker"), "admission.manifest_checker"):
        fail("admission.manifest_checker must use the oracle manifest checker")
    requirement_list = string_list(
        admission.get("required_runtime_requirements"),
        "admission.required_runtime_requirements",
    )
    duplicates = sorted(
        {requirement for requirement in requirement_list if requirement_list.count(requirement) > 1}
    )
    if duplicates:
        fail(f"admission.required_runtime_requirements contains duplicates {duplicates}")
    requirements = set(requirement_list)
    expected_requirements = set(EXPECTED_ORACLE_RUNTIME_REQUIREMENTS)
    unknown_requirements = sorted(requirements - expected_requirements)
    if unknown_requirements:
        fail(f"admission.required_runtime_requirements contains unknown entries {unknown_requirements}")
    missing_requirements = [requirement for requirement in EXPECTED_ORACLE_RUNTIME_REQUIREMENTS if requirement not in requirements]
    if missing_requirements:
        fail(f"admission.required_runtime_requirements missing {missing_requirements}")
    if requirement_list != list(EXPECTED_ORACLE_RUNTIME_REQUIREMENTS):
        fail(f"admission.required_runtime_requirements must be exactly {list(EXPECTED_ORACLE_RUNTIME_REQUIREMENTS)}")
    allowed_targets = string_list(admission.get("allowed_targets"), "admission.allowed_targets")
    if not allowed_targets:
        fail("admission.allowed_targets must not be empty")
    for target in allowed_targets:
        if not target:
            fail("admission.allowed_targets entries must be non-empty")
    duplicates = sorted({target for target in allowed_targets if allowed_targets.count(target) > 1})
    if duplicates:
        fail(f"admission.allowed_targets contains duplicates {duplicates}")
    expected_targets = set(EXPECTED_ALLOWED_TARGETS)
    actual_targets = set(allowed_targets)
    unknown_targets = sorted(actual_targets - expected_targets)
    if unknown_targets:
        fail(f"admission.allowed_targets contains unknown targets {unknown_targets}")
    missing_targets = [target for target in EXPECTED_ALLOWED_TARGETS if target not in actual_targets]
    if missing_targets:
        fail(f"admission.allowed_targets missing {missing_targets}")
    if allowed_targets != list(EXPECTED_ALLOWED_TARGETS):
        fail(f"admission.allowed_targets must be exactly {list(EXPECTED_ALLOWED_TARGETS)}")
    if not bool_value(admission.get("requires_manifest_static_validation"), "admission.requires_manifest_static_validation"):
        fail("manifest static validation must be required")
    if not bool_value(admission.get("requires_existing_package_root"), "admission.requires_existing_package_root"):
        fail("admission.requires_existing_package_root must be true")
    for field in ("loads_model_bytes", "loads_tokenizer", "executes_runtime", "downloads_models"):
        if bool_value(admission.get(field), f"admission.{field}"):
            fail(f"admission.{field} must stay false")

    if string(stdout.get("contract"), "stdout.contract") != EXPECTED_STDOUT_CONTRACT:
        fail(f"stdout.contract must be {EXPECTED_STDOUT_CONTRACT}")
    allowed_events = string_list(stdout.get("allowed_event_kinds"), "stdout.allowed_event_kinds")
    forbidden_events = string_list(stdout.get("forbidden_event_kinds"), "stdout.forbidden_event_kinds")
    stdout_overlap = sorted(set(allowed_events) & set(forbidden_events))
    if stdout_overlap:
        fail(f"stdout allowed/forbidden event kinds overlap {stdout_overlap}")
    if allowed_events != list(EXPECTED_ALLOWED_STDOUT_EVENTS):
        fail(
            f"stdout.allowed_event_kinds must be exactly "
            f"{list(EXPECTED_ALLOWED_STDOUT_EVENTS)}"
        )
    if forbidden_events != list(EXPECTED_FORBIDDEN_STDOUT_EVENTS):
        fail(
            f"stdout.forbidden_event_kinds must be exactly "
            f"{list(EXPECTED_FORBIDDEN_STDOUT_EVENTS)}"
        )
    if string(stderr.get("contract"), "stderr.contract") != EXPECTED_STDERR_CONTRACT:
        fail(f"stderr.contract must be {EXPECTED_STDERR_CONTRACT}")
    if string(stderr.get("required_failure_prefix"), "stderr.required_failure_prefix") != EXPECTED_FAILURE_PREFIX:
        fail(f"stderr.required_failure_prefix must be {EXPECTED_FAILURE_PREFIX}")

    missing_failures = sorted(REQUIRED_FAILURES - set(failures.keys()))
    if missing_failures:
        fail(f"failures missing {missing_failures}")
    for key in REQUIRED_FAILURES:
        if "reject before package execution" not in string(failures.get(key), f"failures.{key}"):
            fail(f"failures.{key} must reject before package execution")

    checks = set(string_list(evidence.get("checks"), "evidence.checks"))
    if "contract metadata only" not in checks:
        fail("evidence.checks must include contract metadata only")
    non_claims = set(string_list(evidence.get("non_claims"), "evidence.non_claims"))
    missing_non_claims = sorted(REQUIRED_NON_CLAIMS - non_claims)
    if missing_non_claims:
        fail(f"evidence.non_claims missing {missing_non_claims}")


def main() -> None:
    contract = (
        sys.argv[1]
        if len(sys.argv) > 1
        else "docs/factory/inference-session-boundary/session-cli-contract.toml"
    )
    if len(sys.argv) > 2:
        fail("usage: check-session-cli-contract.py [contract]")
    with open(contract, "rb") as handle:
        validate(tomllib.load(handle))
    print(f"ok: {contract}")


if __name__ == "__main__":
    main()
