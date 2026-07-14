#!/usr/bin/env python3
"""Validate the Faber inference model artifact oracle manifest.

This check intentionally validates only static handoff metadata. It must not
open model/tokenizer artifacts, load model formats, run tokenizers, or execute
an inference runtime.
"""

from __future__ import annotations

import re
import sys
import tomllib
from pathlib import PurePosixPath


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
    string_list(session.get("argv_contract"), "session.argv_contract")

    requirements = set(string_list(runtime.get("requirements"), "runtime.requirements"))
    if "oracle-fixture" not in requirements:
        fail("runtime.requirements must include oracle-fixture")
    if requirements & REJECTED_FORMATS:
        fail("runtime.requirements must not name real model/runtime formats")

    allowed = set(string_list(policy.get("allowed_formats"), "policy.allowed_formats"))
    if allowed != {"oracle"}:
        fail("policy.allowed_formats must be exactly ['oracle']")
    rejected = set(string_list(policy.get("rejected_formats"), "policy.rejected_formats"))
    missing_rejections = sorted(REJECTED_FORMATS - rejected)
    if missing_rejections:
        fail(f"policy.rejected_formats missing {missing_rejections}")
    for field in ("model_loading", "tokenizer_loading", "runtime_execution"):
        if string(policy.get(field), f"policy.{field}") != "disabled":
            fail(f"policy.{field} must be disabled")

    checks = set(string_list(evidence.get("checks"), "evidence.checks"))
    if "static metadata only" not in checks:
        fail("evidence.checks must include static metadata only")
    non_claims = set(string_list(evidence.get("non_claims"), "evidence.non_claims"))
    missing_non_claims = sorted(REQUIRED_NON_CLAIMS - non_claims)
    if missing_non_claims:
        fail(f"evidence.non_claims missing {missing_non_claims}")


def main() -> None:
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
