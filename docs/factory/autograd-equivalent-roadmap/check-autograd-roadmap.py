#!/usr/bin/env python3
"""Validate the autograd-equivalent roadmap deck guardrails."""

from __future__ import annotations

import re
import sys
from pathlib import Path


REQUIRED_TERMS = [
    "finite-difference oracle",
    "broadcast reductions",
    "view/alias policy",
    "matmul/linear VJP",
    "manual update oracle",
    "optimizer/session boundary",
    "loss-trace oracle",
    "no public PyTorch replacement",
    "no `torch.nn` parity matrix",
    "no generated AIR autodiff yet",
    "no GPU training loop",
]

UNIT_PATTERN = re.compile(r"\| A[1-6] ")


def fail(message: str) -> None:
    print(f"autograd roadmap check failed: {message}", file=sys.stderr)
    raise SystemExit(1)


def main() -> None:
    deck = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(
        "docs/factory/autograd-equivalent-roadmap/deck.md"
    )
    if len(sys.argv) > 2:
        fail("usage: check-autograd-roadmap.py [deck]")
    text = deck.read_text(encoding="utf-8")

    for term in REQUIRED_TERMS:
        if term not in text:
            fail(f"missing required term: {term}")

    units = UNIT_PATTERN.findall(text)
    if not (3 <= len(units) <= 6):
        fail(f"expected 3-6 implementable units, found {len(units)}")

    for heading in ("## Slide 3 - Shipped Evidence To Start From", "## Slide 6 - Next Implementable Units", "## Slide 9 - Stop Conditions"):
        if heading not in text:
            fail(f"missing heading: {heading}")

    if "PyTorch parity claim" in text and "no public PyTorch replacement" not in text:
        fail("PyTorch mention lacks explicit non-claim")

    print(f"ok: {deck}")


if __name__ == "__main__":
    main()
