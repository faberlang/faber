#!/usr/bin/env python3
"""Render faber/docs/EBNF_MATRIX.md and docs/CONVERSIO_MATRIX.md from radix
measurement JSON.

The measurement JSON is produced by the private radix repo at the end of its
test ladder (`scripta/emit-compat-json.py`, wired into the stage-4 measurement
gates, covered by `--full` / `--release` / `--e2e`) and committed at radix
release. This renderer is pure presentation: it reads the JSON from the sibling
radix checkout and emits the public matrices. It never runs cargo or the
exempla harness.

Usage:
  python3 scripta/render-matrices.py            # render both matrices in place
  python3 scripta/render-matrices.py --check    # render to temp; fail if stale

Env:
  RADIX_ROOT   sibling radix checkout (default ../radix)
  MATRICES_OUT render output root (default faber/docs)
"""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
RADIX = Path(os.environ.get("RADIX_ROOT", ROOT.parent / "radix"))
COMPAT = RADIX / "corpus" / "measurement" / "compat"
CONVERSIO = RADIX / "corpus" / "measurement" / "conversio"
DEFAULT_OUT = Path(os.environ.get("MATRICES_OUT", ROOT / "docs"))

EBNF_OVERRIDES = ROOT / "scripta" / "ebnf-matrix-overrides.toml"
CONVERSIO_OVERRIDES = ROOT / "scripta" / "conversio-matrix-overrides.toml"

HIR_TARGETS = ["rust", "go", "ts", "faber"]
MIR_TARGETS = [
    "llvm-text",
    "wasm-text",
    "wasm",
    "metal-text",
    "wgsl-text",
    "sexp-struct",
    "sexp",
    "scena",
]

CONV_HIR_TARGETS = ["rust", "ts", "go", "faber"]
CONV_MIR_TARGETS = ["llvm-text", "wasm-text", "wasm", "wgsl-text", "sexp-struct", "sexp"]

# Family universe in stable matrix order (matches ConversioTypeFamily::ALL).
FAMILIES = [
    "numerus", "fractus", "bivalens", "textus", "ascii", "octeti",
    "regex", "json", "valor", "instans", "modulus",
    "lista", "tabula", "copia", "intervallum",
    "tensor", "vector", "matrix", "sparsa",
    "cursor", "promissum",
]

TIER_GLYPH = {
    "dedicated": "✓",
    "fallback": "◐",
    "rejected": "✕",
    "not-emitted": "—",
}


# --- planned overlay (curated ○ cells) --------------------------------------

def load_planned_overlay(path: Path) -> dict[tuple[str, str], str]:
    """(term, target) -> reason, for curated ○ entries. Empty if file absent."""
    if not path.exists():
        return {}
    text = path.read_text(encoding="utf-8")
    in_planned = False
    out: dict[tuple[str, str], str] = {}
    for line in text.splitlines():
        s = line.strip()
        if s.startswith("[") and s.endswith("]"):
            in_planned = s == "[planned]"
            continue
        if not in_planned:
            continue
        m = re.match(r'^"([^"]+)"\."([^"]+)"\s*=\s*"([^"]*)"', s)
        if m:
            out[(m.group(1), m.group(2))] = m.group(3)
    return out


def glyph(capable: int, denom: int, term: str, target: str, planned: dict) -> str:
    if denom == 0:
        return "—"
    if capable == denom:
        return "✓"
    if capable == 0:
        return "○" if (term, target) in planned else "✕"
    return "◐"


def slug_anchor(term: str) -> str:
    """Return the stable Latin slug used by grammar matrix row anchors."""
    return re.sub(r"[^a-z0-9]+", "-", term.lower()).strip("-")


# --- compat JSON loading -----------------------------------------------------

def load_compat(targets: list[str]) -> dict[str, dict]:
    data: dict[str, dict] = {}
    for t in targets:
        path = COMPAT / f"{t}.json"
        if not path.exists():
            raise SystemExit(f"error: missing measurement JSON {path}")
        data[t] = json.loads(path.read_text(encoding="utf-8"))
    return data


def render_ebnf_matrix(planned: dict) -> list[str]:
    hir = load_compat(HIR_TARGETS)
    mir = load_compat(MIR_TARGETS)

    def lane_summary(targets: list[str], data: dict) -> list[str]:
        out = ["| target | capable | analyzable | % |", "|---|---|---|---|"]
        for t in targets:
            cap = data[t]["summary"]["capable"]
            ana = data[t]["summary"]["analyzable"]
            pct = f"{100*cap/ana:.0f}%" if ana else "—"
            out.append(f"| {t} | {cap} | {ana} | {pct} |")
        return out

    def term_rows(
        lane: dict, rule_filter: str, targets: list[str]
    ) -> list[str]:
        """Term rows across all targets of the lane for one rule group."""
        by_term: dict[str, list] = {}
        order: list[str] = []
        for t in targets:
            for entry in lane[t]["terms"]:
                if entry["rule"] != rule_filter:
                    continue
                key = entry["term"]
                if key not in by_term:
                    by_term[key] = [None] * len(targets)
                    order.append(key)
                by_term[key][targets.index(t)] = (
                    entry["capable"],
                    entry["analyzable"],
                )
        if not order:
            return [f"| _(no {rule_filter} terms)_ |" + " |" * len(targets)]
        rows = []
        for term in order:
            glyphs = []
            for t, counts in zip(targets, by_term[term]):
                capable, denom = counts if counts else (0, 0)
                glyphs.append(glyph(capable, denom, term, t, planned))
            anchor = slug_anchor(term)
            rows.append(
                f'| <a id="{anchor}"></a>`{term}` | '
                + " | ".join(glyphs)
                + " |"
            )
        return rows

    def lane_section(
        title: str, rule: str, lane: dict, targets: list[str]
    ) -> list[str]:
        out = [f"## {title}", "", f"### {rule}", ""]
        out.append("| term | " + " | ".join(targets) + " |")
        out.append("|" + "---|" * (len(targets) + 1))
        out.extend(term_rows(lane, rule, targets))
        out.append("")
        return out

    lines = []
    lines.append("# EBNF Target Support Matrix")
    lines.append("")
    lines.append(
        "**Rendered** by `faber/scripta/render-matrices.py` from "
        "`radix/corpus/measurement/compat/*.json` (emitted by the private radix "
        "ladder) — **do not hand-edit**."
    )
    lines.append(
        "**Measurement**: `emit_hir_target_matrix` + `emit_mir_target_matrix` "
        "(in-process radix harness, no external toolchains)."
    )
    lines.append("**Join**: `corpus/index.toml` terms → exempla.")
    lines.append("")
    lines.append(
        "This is the **official generated** grammar×target support matrix. It reports"
    )
    lines.append(
        "**lowerability** — can target X lower grammar production Y — across every term in"
    )
    lines.append(
        "the exempla corpus. Runtime semantics (erase/warn/defer policy verbs), per-target"
    )
    lines.append(
        "contracts, and pipeline routing are covered on the"
    )
    lines.append("[target compatibility](https://faberlang.dev/en-US/toolchain/target-matrix.html)")
    lines.append("and [Compiling and targets](https://faberlang.dev/en-US/toolchain/compiling.html)")
    lines.append("pages of the documentation site.")
    lines.append("")
    lines.append("## Legend")
    lines.append("")
    lines.append("| Glyph | Meaning |")
    lines.append("|---|---|")
    lines.append("| ✓ | fully supported — all analyzable exempla for the term lower |")
    lines.append("| ◐ | partial — some exempla lower, some have a measured gap |")
    lines.append("| ○ | planned — not yet lowering; curated overlay (`scripta/ebnf-matrix-overrides.toml`) |")
    lines.append("| ✕ | not supported — no exempla lower; default-truth, measured gap is real |")
    lines.append("| — | not measured — no analyzable exempla for this term on this lane |")
    lines.append("")
    lines.append("> A ✓ means the corpus exempla exercising this term lower to the target. It does")
    lines.append("> **not** guarantee identical runtime semantics. Some targets *erase* or *warn* on")
    lines.append("> certain constructs (e.g. Go erases borrow modes `de`/`in`/`ex`) — those still")
    lines.append("> render ✓ here because they lower. See the policy doc for that nuance.")
    lines.append("")
    lines.append("## Corpus-wide summary (all registered terms)")
    lines.append("")
    lines.append("**Application lane (HIR → emitted source languages)**")
    lines.append("")
    lines.extend(lane_summary(HIR_TARGETS, hir))
    lines.append("")
    lines.append("**Systems lane (MIR → device/IR artifacts)**")
    lines.append("")
    lines.extend(lane_summary(MIR_TARGETS, mir))
    lines.append("")
    lines.extend(lane_section("Keywords — application lane", "keyword", hir, HIR_TARGETS))
    lines.extend(lane_section("Operators — application lane", "operator-group", hir, HIR_TARGETS))
    lines.extend(lane_section("Keywords — systems lane", "keyword", mir, MIR_TARGETS))
    lines.extend(lane_section("Operators — systems lane", "operator-group", mir, MIR_TARGETS))
    lines.extend(lane_section("Types, intrinsics & meta", "existing-home", hir, HIR_TARGETS))
    lines.append("## Regeneration")
    lines.append("")
    lines.append(
        "The measurement JSON is regenerated by the private radix ladder "
        "(`./scripta/test --full` / `--release`, stage-4 measurement gates) via "
        "`scripta/emit-compat-json.py` and committed at radix release. Render:"
    )
    lines.append("")
    lines.append("```bash")
    lines.append("python3 scripta/render-matrices.py          # render in place")
    lines.append("python3 scripta/render-matrices.py --check  # fail if committed docs are stale")
    lines.append("```")
    lines.append("")
    lines.append("Rerun whenever the codegen, MIR lowering, or exempla corpus changes.")
    lines.append("")
    return lines


# --- conversio rendering -----------------------------------------------------

def load_conversio(targets: list[str]) -> dict[str, dict]:
    data: dict[str, dict] = {}
    for t in targets:
        path = CONVERSIO / f"{t}.json"
        if not path.exists():
            raise SystemExit(f"error: missing measurement JSON {path}")
        data[t] = json.loads(path.read_text(encoding="utf-8"))
    return data


def load_disagreements() -> list[dict]:
    path = CONVERSIO / "disagreements.json"
    if not path.exists():
        return []
    return json.loads(path.read_text(encoding="utf-8"))["rows"]


def render_grid(title: str, target: str, data: dict) -> list[str]:
    out = [f"### {title} (`{target}`)", ""]
    header = "| src ╲ tgt | " + " | ".join(f"`{f}`" for f in FAMILIES) + " |"
    out.append(header)
    out.append("|" + "---|" * (len(FAMILIES) + 1))
    cells = {(c["src"], c["tgt"]): c for c in data[target]["cells"]}
    for src in FAMILIES:
        row = []
        for tgt in FAMILIES:
            cell = cells.get((src, tgt), {})
            tier = cell.get("tier", "not-emitted")
            glyph_ = TIER_GLYPH.get(tier, "?")
            if cell.get("measured"):
                glyph_ = f"**{glyph_}**"
            row.append(glyph_)
        out.append(f"| `{src}` | " + " | ".join(row) + " |")
    out.append("")
    return out


def render_conversio_matrix() -> list[str]:
    hir = load_conversio(CONV_HIR_TARGETS)
    mir = load_conversio(CONV_MIR_TARGETS)
    disagreements = load_disagreements()

    total = len(FAMILIES) * len(FAMILIES)
    backed = sum(1 for c in hir[CONV_HIR_TARGETS[0]]["cells"] if c["measured"])

    def summary_table(targets: list[str], data: dict) -> list[str]:
        out = ["| target | dedicated | fallback | rejected | not emitted |"]
        out.append("|---|---|---|---|---|")
        for t in targets:
            s = data[t]["summary"]
            out.append(
                f"| {t} | {s['dedicated']} | {s['fallback']} | {s['rejected']} | {s['not-emitted']} |"
            )
        return out

    lines = []
    lines.append("# Conversio Target Coverage Matrix")
    lines.append("")
    lines.append(
        "**Rendered** by `faber/scripta/render-matrices.py` from "
        "`radix/corpus/measurement/conversio/*.json` (emitted by the private "
        "radix ladder) — **do not hand-edit**."
    )
    lines.append(
        "**Measurement**: `emit_conversio_target_matrix` over the type-family "
        "cartesian product. Fixture-backed cells are **measured** (real "
        "frontend + real MIR probe + real HIR emit-arm detection, no external "
        "toolchains); the rest are classifier predictions."
    )
    lines.append("")
    lines.append(
        "This is the **official generated** conversio (`↦`) coverage matrix. It "
        "reports, per (type-family × type-family × target), whether a conversion "
        "has a **dedicated lowering** (✓), is accepted but **only via the "
        "unspecialized fallback** (◐ — not guaranteed to compile), is "
        "**semantically rejected** (✕), or is **not emitted by the target** (—)."
    )
    lines.append("")
    lines.append("## Legend")
    lines.append("")
    lines.append("| Glyph | Tier | Meaning |")
    lines.append("|---|---|---|")
    lines.append("| ✓ | dedicated | the target has a dedicated lowering arm for this pair |")
    lines.append("| ◐ | fallback | semantic-accepted, but only the unspecialized fallback (may not compile) |")
    lines.append("| ✕ | rejected | the typechecker denies the pair before any backend |")
    lines.append("| — | not emitted | the target cannot lower this pair |")
    lines.append("")
    lines.append(
        "> Tier precedence per cell: `✕ rejected > — not emitted > ✓ dedicated > "
        "◐ fallback`. `✕` is target-independent (it mirrors the semantic `↦` "
        "policy). The `◐` tier is specific to HIR backends with an unspecialized "
        "fallback (notably Rust's `emit_unspecialized_conversio_target`); MIR "
        "targets lower `↦` to a runtime intrinsic the runtime owns and have no "
        "`◐` tier."
    )
    lines.append("")
    lines.append("## Provenance")
    lines.append("")
    lines.append(
        f"**{backed}** of {total} cells are **fixture-backed and measured** (an "
        f"authored `examples/conversio-matrix/<src>/<tgt>.fab` exists and the "
        "harness measured its real frontend / MIR / emit-arm verdict) and "
        "rendered in **bold**. Plain glyphs are classifier predictions (no "
        "fixture yet)."
    )
    lines.append("")
    lines.append(
        "> Honesty note: measured verdicts come from the real compiler pipeline "
        "(`✕` from `analyze_source` with the issue code captured, MIR `✓`/`—` "
        "from `classify_mir_coverage` on the lowered fixture, HIR `✓`/`◐` from "
        "emit-arm detection in each backend). The read-only classifier remains "
        "as a cross-check oracle; cells where the measured verdict disagrees "
        "with the prediction are listed under Measured divergences."
    )
    lines.append("")
    lines.append("## Corpus-wide summary")
    lines.append("")
    lines.append(
        f"Universe: {len(FAMILIES)} families × {len(FAMILIES)} families = {total} "
        f"pairs per target. Fixture-backed: {backed}/{total} cells."
    )
    lines.append("")
    lines.append("**Application lane (HIR → emitted source languages)**")
    lines.append("")
    lines.extend(summary_table(CONV_HIR_TARGETS, hir))
    lines.append("")
    lines.append("**Systems lane (MIR → device/IR artifacts)**")
    lines.append("")
    lines.extend(summary_table(CONV_MIR_TARGETS, mir))
    lines.append("")
    lines.append("## Application lane")
    lines.append("")
    for t in CONV_HIR_TARGETS:
        lines.extend(render_grid("HIR backend", t, hir))
    lines.append("## Systems lane")
    lines.append("")
    for t in CONV_MIR_TARGETS:
        lines.extend(render_grid("MIR target", t, mir))
    lines.append("## Measured divergences (predicted vs measured)")
    lines.append("")
    if disagreements:
        lines.append(
            "Fixture-backed cells where the measured verdict disagrees with the "
            "hand classifier. The classifier is kept as a cross-check oracle; "
            "each row is either genuine drift to fix (follow-on goals) or a "
            "documented divergence."
        )
        lines.append("")
        transitions: dict[str, dict[str, int]] = {}
        for row in disagreements:
            bucket = transitions.setdefault(row["target"], {})
            key = f"{TIER_GLYPH.get(row['predicted'], '?')} → {TIER_GLYPH.get(row['measured'], '?')}"
            bucket[key] = bucket.get(key, 0) + 1
        lines.append("| target | transitions (predicted → measured) |")
        lines.append("|---|---|")
        for t in CONV_HIR_TARGETS + CONV_MIR_TARGETS:
            bucket = transitions.get(t, {})
            if not bucket:
                continue
            summary = ", ".join(f"{count}× {key}" for key, count in sorted(bucket.items()))
            lines.append(f"| {t} | {summary} |")
        lines.append("")
        lines.append("<details>")
        lines.append("<summary>Full per-cell disagreement list</summary>")
        lines.append("")
        lines.append("| src | tgt | target | predicted | measured | detail |")
        lines.append("|---|---|---|---|---|---|")
        for row in disagreements:
            lines.append(
                f"| `{row['src']}` | `{row['tgt']}` | `{row['target']}` | "
                f"{TIER_GLYPH.get(row['predicted'], '?')} | "
                f"{TIER_GLYPH.get(row['measured'], '?')} | `{row['detail']}` |"
            )
        lines.append("")
        lines.append("</details>")
    else:
        lines.append(
            "None — every measured fixture-backed cell agrees with the hand "
            "classifier."
        )
    lines.append("")
    lines.append("## Regeneration")
    lines.append("")
    lines.append(
        "The measurement JSON is regenerated by the private radix ladder "
        "(`./scripta/test --full` / `--release`, stage-4 measurement gates) via "
        "`scripta/emit-compat-json.py` and committed at radix release. Render:"
    )
    lines.append("")
    lines.append("```bash")
    lines.append("python3 scripta/render-matrices.py          # render in place")
    lines.append("python3 scripta/render-matrices.py --check  # fail if committed docs are stale")
    lines.append("```")
    lines.append("")
    lines.append(
        "Rerun whenever the conversio codegen, semantic `↦` policy, or MIR "
        "lowering changes."
    )
    lines.append("")
    lines.append("## Family-level granularity notes")
    lines.append("")
    lines.append(
        "- Each aggregate family (`tensor`, `vector`, `matrix`, `sparsa`, "
        "`intervallum`) collapses shape/element variance into one cell."
    )
    lines.append(
        "- `valor ↦ fractus<f32>` (sized) is semantically rejected; `valor ↦ "
        "fractus` (unsized) is dedicated. The `fractus` family cell reports the "
        "unconditional (unsized) verdict."
    )
    lines.append(
        "- `tensor ↦ tensor` reports dedicated when element widths unify; shape "
        "or element mismatch is a conditional rejection, not a cell verdict."
    )
    lines.append("")
    return lines


def main() -> int:
    check = "--check" in sys.argv[1:]
    planned = load_planned_overlay(EBNF_OVERRIDES)

    rendered = {
        "EBNF_MATRIX.md": render_ebnf_matrix(planned),
        "CONVERSIO_MATRIX.md": render_conversio_matrix(),
    }

    if check:
        import tempfile

        with tempfile.TemporaryDirectory() as tmp:
            tmp_root = Path(tmp)
            for name, lines in rendered.items():
                (tmp_root / name).write_text("\n".join(lines), encoding="utf-8")
                committed = DEFAULT_OUT / name
                if not committed.exists():
                    print(f"error: {committed} missing (render --check)", file=sys.stderr)
                    return 1
                import difflib

                diff = list(
                    difflib.unified_diff(
                        committed.read_text(encoding="utf-8").splitlines(),
                        (tmp_root / name).read_text(encoding="utf-8").splitlines(),
                        fromfile=str(committed),
                        tofile=f"<rendered {name}>",
                    )
                )
                if diff:
                    print(f"error: {committed} is stale; rerun render-matrices.py", file=sys.stderr)
                    print("\n".join(diff[:40]), file=sys.stderr)
                    return 1
        print("matrices fresh")
        return 0

    for name, lines in rendered.items():
        out = DEFAULT_OUT / name
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text("\n".join(lines), encoding="utf-8")
        print(f"wrote {out} ({out.stat().st_size} bytes)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
