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

REJECTED_FORMATS = {
    "gguf",
    "safetensors",
    "transformer",
    "quantized-kernel",
    "gpu-runtime",
}

REQUIRED_FAILURES = {
    "unsupported_format",
    "missing_non_claim",
    "unchecked_path",
    "unknown_runtime_requirement",
    "runtime_execution_requested",
}

EXPECTED_ALLOWED_TARGETS = ("fmir-text", "fmir", "fmir-bin", "scena", "rust")


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
    for required in ("faber session", "--model-manifest", "--"):
        if required not in shape:
            fail(f"command.shape must include {required}")
    if string(command.get("model_manifest_flag"), "command.model_manifest_flag") != "--model-manifest":
        fail("command.model_manifest_flag must be --model-manifest")

    accepted = set(string_list(inputs.get("accepted_manifest_formats"), "inputs.accepted_manifest_formats"))
    if accepted != {"oracle"}:
        fail("inputs.accepted_manifest_formats must be exactly ['oracle']")
    rejected = set(string_list(inputs.get("rejected_manifest_formats"), "inputs.rejected_manifest_formats"))
    missing_rejections = sorted(REJECTED_FORMATS - rejected)
    if missing_rejections:
        fail(f"inputs.rejected_manifest_formats missing {missing_rejections}")
    if set(string_list(inputs.get("session_args"), "inputs.session_args")) != {"prompt"}:
        fail("inputs.session_args must describe only prompt forwarding")

    if "check-model-artifact-oracle.py" not in string(admission.get("manifest_checker"), "admission.manifest_checker"):
        fail("admission.manifest_checker must use the oracle manifest checker")
    requirements = set(string_list(admission.get("required_runtime_requirements"), "admission.required_runtime_requirements"))
    if requirements != {"oracle-fixture"}:
        fail("admission.required_runtime_requirements must be exactly ['oracle-fixture']")
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
    for field in ("loads_model_bytes", "loads_tokenizer", "executes_runtime", "downloads_models"):
        if bool_value(admission.get(field), f"admission.{field}"):
            fail(f"admission.{field} must stay false")

    allowed_events = set(string_list(stdout.get("allowed_event_kinds"), "stdout.allowed_event_kinds"))
    if not {"artifact", "diagnostic", "oracle-request", "oracle-result"}.issubset(allowed_events):
        fail("stdout.allowed_event_kinds missing contract events")
    forbidden_events = set(string_list(stdout.get("forbidden_event_kinds"), "stdout.forbidden_event_kinds"))
    if not {"token", "logit", "gpu-kernel", "model-loaded"}.issubset(forbidden_events):
        fail("stdout.forbidden_event_kinds missing runtime event exclusions")
    if "diagnostics" not in string(stderr.get("contract"), "stderr.contract"):
        fail("stderr.contract must be diagnostics-only")

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
