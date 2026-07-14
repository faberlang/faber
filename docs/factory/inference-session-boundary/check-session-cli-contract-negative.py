#!/usr/bin/env python3
"""Validate negative fixtures for the Faber session CLI contract checker."""

from __future__ import annotations

import pathlib
import subprocess
import sys


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent
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
        return 1
    print("ok: session CLI contract negative fixtures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
