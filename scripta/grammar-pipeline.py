#!/usr/bin/env python3
"""Grammar pipeline: markdown-audit of EBNF.md and dialect-1 validation.

GSD-1 skeleton. `--audit-md` parses fenced `ebnf` blocks in the current
`faber/docs/EBNF.md` (and locale packs) and emits the mechanical sin list:
orphans, undefined terminals, keyword-table gaps, comma variance, locale
drift. `--validate` parses a dialect-1 `.fg` file and checks closure plus
reachability from `@start` (appendix smoke-check).

Advisory only. Not wired into stage 1 until GSD-6.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from collections import Counter, deque
from dataclasses import dataclass, field
from pathlib import Path

SCRIPT = Path(__file__).resolve()
RADIX = SCRIPT.parents[1]

PROD_HEAD_RE = re.compile(
    r"^([A-Za-z_][A-Za-z0-9_]*)\s*(?:::=|:=)\s*(.*)$"
)
IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
LIT_RE = re.compile(r"'([^']*)'")
FENCE_RE = re.compile(r"```ebnf\n(.*?)```", re.S)
COMMA_VAR_RE = re.compile(r"','\s*\?")
LATIN_KW_RE = re.compile(r"^[a-z][a-z0-9_]*$")
TERMINAL_NAME_RE = re.compile(r"^[A-Z][A-Z0-9_]*$")
DIRECTIVE_RE = re.compile(r"^@([A-Za-z_][A-Za-z0-9_-]*)(?:\s+(.*))?$")
LEADING_LATIN_RE = re.compile(r"[a-z][a-z0-9_]*")

# Lexical / frontmatter terminals verified in the GSD-1 delivery baseline.
VERIFIED_UNDEFINED_TERMINALS = (
    "IDENTIFIER",
    "NUMBER",
    "NATURAL",
    "STRING",
    "WIDTH_MARKER",
    "FRONTMATTER_DELIMITER",
    "NEWLINE",
    "TOML_LINES",
)
VERIFIED_ORPHANS = (
    "returnAwaitStmt",
    "awaitDiscardStmt",
    "yieldStmt",
)
VERIFIED_KEYWORD_GAPS = (
    "ceteri",
    "ceterum",
    "ut",
    "omnia",
    "ex",
    "de",
    "per",
    "ab",
)

SCHEMA = "grammar-pipeline-audit/v1"


@dataclass
class Production:
    name: str
    line: int
    rhs: str
    path: str = ""


@dataclass
class Grammar:
    path: str
    productions: list[Production] = field(default_factory=list)
    start: str | None = None
    grammar_n: int | None = None
    by_name: dict[str, Production] = field(default_factory=dict)


def discover_container_root(start: Path | None = None) -> Path | None:
    here = start if start is not None else RADIX
    for directory in [here, *here.parents]:
        if (directory / "worktrees").is_dir() and (directory / "radix").is_dir():
            if directory.parent.name == "worktrees":
                continue
            return directory
    return None


def default_ebnf_path() -> Path:
    packet = RADIX.parent / "faber" / "docs" / "EBNF.md"
    if packet.is_file():
        return packet
    container = discover_container_root(RADIX)
    if container is not None:
        candidate = container / "faber" / "docs" / "EBNF.md"
        if candidate.is_file():
            return candidate
    raise FileNotFoundError(
        "cannot locate faber/docs/EBNF.md beside radix or in the container"
    )


def default_locale_dir() -> Path | None:
    container = discover_container_root(RADIX)
    if container is None:
        return None
    directory = container / "faberlang.dev" / "generator" / "grammar"
    return directory if directory.is_dir() else None


def display_path(path: Path) -> str:
    container = discover_container_root(RADIX)
    resolved = path.resolve()
    if container is not None:
        try:
            return resolved.relative_to(container.resolve()).as_posix()
        except ValueError:
            pass
    try:
        return resolved.relative_to(RADIX.parent.resolve()).as_posix()
    except ValueError:
        return path.as_posix()


def strip_ebnf_noise(text: str) -> str:
    """Drop # line comments and (* *) ISO comments; keep quoted literals intact."""
    out: list[str] = []
    i = 0
    n = len(text)
    while i < n:
        char = text[i]
        if char == "'":
            j = i + 1
            while j < n and text[j] != "'":
                j += 1
            out.append(text[i : min(j + 1, n)])
            i = j + 1 if j < n else n
            continue
        if char == "#" and (i == 0 or text[i - 1] != "'"):
            while i < n and text[i] != "\n":
                i += 1
            continue
        if char == "(" and i + 1 < n and text[i + 1] == "*":
            end = text.find("*)", i + 2)
            if end < 0:
                break
            out.append(" ")
            i = end + 2
            continue
        out.append(char)
        i += 1
    return "".join(out)


def rhs_literals(rhs: str) -> list[str]:
    return LIT_RE.findall(rhs)


def rhs_symbols(rhs: str) -> list[str]:
    stripped = LIT_RE.sub(" ", rhs)
    return IDENT_RE.findall(stripped)


def first_latin_literal(rhs: str) -> str | None:
    for literal in rhs_literals(rhs):
        if LATIN_KW_RE.fullmatch(literal):
            return literal
    return None


def parse_production_block(text: str, *, path: str, start_line: int = 1) -> list[Production]:
    productions: list[Production] = []
    index: dict[str, int] = {}
    current: Production | None = None
    for offset, raw_line in enumerate(text.splitlines()):
        line_no = start_line + offset
        cleaned = strip_ebnf_noise(raw_line)
        if not cleaned.strip():
            continue
        match = PROD_HEAD_RE.match(cleaned)
        if match:
            name, rhs = match.group(1), match.group(2)
            if name in index:
                productions[index[name]].rhs += " " + rhs
                current = productions[index[name]]
            else:
                production = Production(name=name, line=line_no, rhs=rhs, path=path)
                index[name] = len(productions)
                productions.append(production)
                current = production
            continue
        if current is None:
            continue
        stripped = cleaned.strip()
        if stripped.startswith("|") or cleaned[:1].isspace():
            current.rhs += " " + stripped
    return productions


def grammar_from_productions(
    productions: list[Production],
    *,
    path: str,
    start: str | None = None,
    grammar_n: int | None = None,
) -> Grammar:
    by_name = {prod.name: prod for prod in productions}
    if start is None and productions:
        start = productions[0].name
    return Grammar(
        path=path,
        productions=productions,
        start=start,
        grammar_n=grammar_n,
        by_name=by_name,
    )


def extract_fenced_ebnf(markdown: str) -> list[tuple[int, str]]:
    blocks: list[tuple[int, str]] = []
    for match in FENCE_RE.finditer(markdown):
        start_line = markdown[: match.start()].count("\n") + 2
        blocks.append((start_line, match.group(1)))
    return blocks


def parse_markdown_grammar(text: str, *, path: str) -> Grammar:
    productions: list[Production] = []
    for start_line, body in extract_fenced_ebnf(text):
        productions.extend(
            parse_production_block(body, path=path, start_line=start_line)
        )
    return grammar_from_productions(productions, path=path)


def parse_dialect(text: str, *, path: str) -> Grammar:
    start: str | None = None
    grammar_n: int | None = None
    kept: list[str] = []
    original_lines = text.splitlines()
    for raw in original_lines:
        cleaned = strip_ebnf_noise(raw).rstrip()
        stripped = cleaned.strip()
        if not stripped:
            kept.append("")
            continue
        directive = DIRECTIVE_RE.match(stripped)
        if directive and not PROD_HEAD_RE.match(stripped):
            name = directive.group(1)
            rest = (directive.group(2) or "").strip()
            if name == "start" and rest:
                start = rest.split()[0]
            elif name == "grammar" and rest:
                try:
                    grammar_n = int(rest.split()[0])
                except ValueError:
                    grammar_n = None
            kept.append("")
            continue
        kept.append(cleaned)
    productions = parse_production_block(
        "\n".join(kept), path=path, start_line=1
    )
    return grammar_from_productions(
        productions, path=path, start=start, grammar_n=grammar_n
    )


def reference_counts(grammar: Grammar) -> Counter[str]:
    counts: Counter[str] = Counter()
    for production in grammar.productions:
        counts.update(rhs_symbols(production.rhs))
    return counts


def reachable_names(grammar: Grammar) -> set[str]:
    if not grammar.start:
        return set()
    seen: set[str] = set()
    queue: deque[str] = deque([grammar.start])
    while queue:
        name = queue.popleft()
        if name in seen:
            continue
        seen.add(name)
        production = grammar.by_name.get(name)
        if production is None:
            continue
        for symbol in rhs_symbols(production.rhs):
            if symbol not in seen:
                queue.append(symbol)
    return seen


def orphan_productions(grammar: Grammar) -> list[Production]:
    reachable = reachable_names(grammar)
    return [prod for prod in grammar.productions if prod.name not in reachable]


def undefined_symbols(grammar: Grammar) -> list[tuple[str, int]]:
    counts = reference_counts(grammar)
    defined = set(grammar.by_name)
    return sorted(
        (name, counts[name])
        for name in counts
        if name not in defined
    )


def undefined_terminals(grammar: Grammar) -> list[tuple[str, int]]:
    return [
        (name, refs)
        for name, refs in undefined_symbols(grammar)
        if TERMINAL_NAME_RE.fullmatch(name)
    ]


def parse_keyword_table(markdown: str) -> set[str]:
    if "## Keyword Reference" not in markdown:
        return set()
    section = markdown.split("## Keyword Reference", 1)[1]
    body = section.split("\n## ", 1)[0]
    covered: set[str] = set()
    for span in re.findall(r"`([^`]+)`", body):
        covered.add(span)
        match = LEADING_LATIN_RE.search(span)
        if match:
            covered.add(match.group(0))
    return covered


def keyword_literals(grammar: Grammar) -> Counter[str]:
    counts: Counter[str] = Counter()
    for production in grammar.productions:
        for literal in rhs_literals(production.rhs):
            if LATIN_KW_RE.fullmatch(literal):
                counts[literal] += 1
    return counts


def keyword_gaps(grammar: Grammar, covered: set[str]) -> list[tuple[str, int]]:
    counts = keyword_literals(grammar)
    return sorted(
        (literal, counts[literal])
        for literal in counts
        if literal not in covered
    )


def comma_variance_rows(
    text: str, *, path: str, start_line: int = 1
) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    current: str | None = None
    for offset, raw_line in enumerate(text.splitlines()):
        line_no = start_line + offset
        cleaned = strip_ebnf_noise(raw_line)
        match = PROD_HEAD_RE.match(cleaned)
        if match:
            current = match.group(1)
        if not COMMA_VAR_RE.search(cleaned):
            continue
        rows.append(
            {
                "path": path,
                "line": line_no,
                "production": current,
                "snippet": raw_line.strip(),
            }
        )
    return rows


def markdown_comma_variance(text: str, *, path: str) -> list[dict[str, object]]:
    rows: list[dict[str, object]] = []
    for start_line, body in extract_fenced_ebnf(text):
        rows.extend(comma_variance_rows(body, path=path, start_line=start_line))
    return rows


def first_symbol_line(text: str, symbol: str) -> int | None:
    pattern = re.compile(rf"\b{re.escape(symbol)}\b")
    for start_line, body in extract_fenced_ebnf(text):
        for offset, raw_line in enumerate(body.splitlines()):
            if pattern.search(strip_ebnf_noise(raw_line)):
                return start_line + offset
    return None


def locale_drift_rows(text: str, *, path: str) -> list[dict[str, object]]:
    grammar = parse_markdown_grammar(text, path=path)
    rows: list[dict[str, object]] = []
    statement = grammar.by_name.get("statement")
    if statement is not None:
        refs = set(rhs_symbols(statement.rhs))
        if "adStmt" in refs:
            rows.append(
                {
                    "path": path,
                    "code": "adStmt",
                    "line": first_symbol_line(text, "adStmt") or statement.line,
                    "message": "statement still lists retired adStmt",
                }
            )
        pre_layer = "statementCore" not in refs or "annotation" not in refs
        if pre_layer:
            rows.append(
                {
                    "path": path,
                    "code": "pre_layer_statement",
                    "line": statement.line,
                    "message": (
                        "statement predates the annotation* statementCore layer"
                    ),
                }
            )
    frontmatter = grammar.by_name.get("frontmatter")
    if frontmatter is not None:
        literals = set(rhs_literals(frontmatter.rhs))
        symbols = set(rhs_symbols(frontmatter.rhs))
        canonical = (
            "FRONTMATTER_DELIMITER" in symbols and "TOML_LINES" in symbols
        )
        divergent = "+++" in literals or "tomlBody" in symbols
        if divergent and not canonical:
            rows.append(
                {
                    "path": path,
                    "code": "divergent_frontmatter",
                    "line": frontmatter.line,
                    "message": (
                        "frontmatter spells +++ / tomlBody; canonical uses "
                        "FRONTMATTER_DELIMITER / TOML_LINES"
                    ),
                }
            )
    return rows


def iter_locale_files(locale_dir: Path | None) -> list[Path]:
    if locale_dir is None or not locale_dir.is_dir():
        return []
    return sorted(locale_dir.glob("EBNF.*.md"))


def audit_markdown(
    ebnf_text: str,
    *,
    ebnf_path: Path,
    locale_files: list[tuple[Path, str]] | None = None,
) -> dict[str, object]:
    path = display_path(ebnf_path)
    grammar = parse_markdown_grammar(ebnf_text, path=path)
    covered = parse_keyword_table(ebnf_text)

    orphans = []
    for production in orphan_productions(grammar):
        orphans.append(
            {
                "name": production.name,
                "line": production.line,
                "keyword": first_latin_literal(production.rhs),
            }
        )

    terminals = [
        {"name": name, "refs": refs}
        for name, refs in undefined_terminals(grammar)
    ]
    gaps = [
        {"literal": literal, "refs": refs}
        for literal, refs in keyword_gaps(grammar, covered)
    ]
    commas = markdown_comma_variance(ebnf_text, path=path)
    locale_drift: list[dict[str, object]] = []
    locale_paths: list[str] = []

    for locale_path, locale_text in locale_files or []:
        locale_display = display_path(locale_path)
        locale_paths.append(locale_display)
        locale_drift.extend(locale_drift_rows(locale_text, path=locale_display))
        commas.extend(markdown_comma_variance(locale_text, path=locale_display))

    payload = {
        "schema": SCHEMA,
        "mode": "audit-md",
        "start": grammar.start,
        "sources": {"ebnf": path, "locales": locale_paths},
        "orphans": orphans,
        "undefined_terminals": terminals,
        "keyword_gaps": gaps,
        "comma_variance": commas,
        "locale_drift": locale_drift,
        "counts": {
            "orphans": len(orphans),
            "undefined_terminals": len(terminals),
            "keyword_gaps": len(gaps),
            "comma_variance": len(commas),
            "locale_drift": len(locale_drift),
        },
    }
    return payload


def load_locale_files(locale_dir: Path | None) -> list[tuple[Path, str]]:
    loaded: list[tuple[Path, str]] = []
    for path in iter_locale_files(locale_dir):
        loaded.append((path, path.read_text(encoding="utf-8")))
    return loaded


def validate_dialect(grammar: Grammar) -> dict[str, object]:
    undefined = undefined_symbols(grammar)
    orphans = orphan_productions(grammar)
    start_missing = grammar.start is None or grammar.start not in grammar.by_name
    ok = not undefined and not orphans and not start_missing
    return {
        "schema": SCHEMA,
        "mode": "validate",
        "ok": ok,
        "path": grammar.path,
        "start": grammar.start,
        "grammar": grammar.grammar_n,
        "productions": len(grammar.productions),
        "undefined": [{"name": name, "refs": refs} for name, refs in undefined],
        "orphans": [
            {"name": prod.name, "line": prod.line} for prod in orphans
        ],
        "start_missing": start_missing,
    }


def print_audit_text(payload: dict[str, object]) -> None:
    sources = payload["sources"]
    print("=== grammar markdown-audit ===")
    print(f"source: {sources['ebnf']}")
    locales = sources["locales"]
    if locales:
        print(f"locales: {len(locales)}")
    print(f"start: {payload['start']}")
    print()

    print("--- orphans (defined, not reachable from start) ---")
    orphans = payload["orphans"]
    if orphans:
        for row in orphans:
            keyword = row.get("keyword")
            suffix = f" ('{keyword}')" if keyword else ""
            print(f"  - {row['name']}:{row['line']}{suffix}")
    else:
        print("  (none)")
    print()

    print("--- undefined terminals ---")
    terminals = payload["undefined_terminals"]
    if terminals:
        for row in terminals:
            print(f"  - {row['name']} ({row['refs']} refs)")
    else:
        print("  (none)")
    print()

    print("--- keyword-table gaps ---")
    gaps = payload["keyword_gaps"]
    if gaps:
        for row in gaps:
            print(f"  - {row['literal']} ({row['refs']} refs)")
    else:
        print("  (none)")
    print()

    print("--- comma variance ---")
    commas = payload["comma_variance"]
    ebnf_path = sources["ebnf"]
    canonical = [row for row in commas if row["path"] == ebnf_path]
    locale_commas = [row for row in commas if row["path"] != ebnf_path]
    if canonical:
        for row in canonical:
            print(
                f"  - {row['path']}:{row['line']} "
                f"{row.get('production') or '?'}  {row['snippet']}"
            )
    else:
        print("  canonical: (none — post-CS-2 base is lawful)")
    if locale_commas:
        for row in locale_commas:
            print(
                f"  - {row['path']}:{row['line']} "
                f"{row.get('production') or '?'}  {row['snippet']}"
            )
    print()

    print("--- locale drift ---")
    drift = payload["locale_drift"]
    if drift:
        for row in drift:
            print(f"  - {row['path']}:{row['line']} [{row['code']}] {row['message']}")
    else:
        print("  (none)")
    print()

    counts = payload["counts"]
    print(
        "summary: "
        f"{counts['orphans']} orphans, "
        f"{counts['undefined_terminals']} undefined terminals, "
        f"{counts['keyword_gaps']} keyword gaps, "
        f"{counts['comma_variance']} comma-variance rows, "
        f"{counts['locale_drift']} locale-drift rows"
    )


def print_validate_text(payload: dict[str, object]) -> None:
    status = "OK" if payload["ok"] else "FAIL"
    print(
        f"=== dialect validate: {status} ===\n"
        f"path: {payload['path']}\n"
        f"start: {payload['start']}\n"
        f"productions: {payload['productions']}"
    )
    if payload["start_missing"]:
        print("  - @start symbol is missing or undefined")
    if payload["undefined"]:
        print("--- undefined ---")
        for row in payload["undefined"]:
            print(f"  - {row['name']} ({row['refs']} refs)")
    if payload["orphans"]:
        print("--- orphans ---")
        for row in payload["orphans"]:
            print(f"  - {row['name']}:{row['line']}")
    if payload["ok"]:
        print("closure and reachability: pass")


def dumps_findings(payload: dict[str, object]) -> str:
    return json.dumps(payload, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def default_dialect_path() -> Path:
    return SCRIPT.with_name("fixtures") / "grammar" / "dialect-1.fg"


def default_findings_path() -> Path:
    return SCRIPT.with_name("grammar-pipeline-findings.json")


def build_arg_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument(
        "--audit-md",
        action="store_true",
        help="enumerate sins in the current Markdown EBNF (default)",
    )
    mode.add_argument(
        "--validate",
        action="store_true",
        help="validate a dialect-1 .fg file (see --dialect)",
    )
    parser.add_argument(
        "--dialect",
        type=Path,
        default=None,
        help="dialect-1 path for --validate (default: scripta/fixtures/grammar/dialect-1.fg)",
    )
    parser.add_argument(
        "--ebnf",
        type=Path,
        default=None,
        help="Markdown grammar path (default: sibling faber/docs/EBNF.md)",
    )
    parser.add_argument(
        "--locale-dir",
        type=Path,
        default=None,
        help="directory of EBNF.{locale}.md packs (default: faberlang.dev/generator/grammar)",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="emit machine-readable JSON on stdout",
    )
    parser.add_argument(
        "--write-findings",
        type=Path,
        nargs="?",
        const="__default__",
        metavar="PATH",
        help="write the audit JSON (default: scripta/grammar-pipeline-findings.json)",
    )
    return parser


def run_audit_md(args: argparse.Namespace) -> tuple[dict[str, object], int]:
    ebnf_path = args.ebnf if args.ebnf is not None else default_ebnf_path()
    ebnf_path = ebnf_path.resolve()
    locale_dir = args.locale_dir
    if locale_dir is None and args.ebnf is None:
        locale_dir = default_locale_dir()
    ebnf_text = ebnf_path.read_text(encoding="utf-8")
    payload = audit_markdown(
        ebnf_text,
        ebnf_path=ebnf_path,
        locale_files=load_locale_files(locale_dir),
    )
    write_path = args.write_findings
    if write_path is not None:
        if write_path == Path("__default__") or write_path == "__default__":
            write_path = default_findings_path()
        write_path = Path(write_path)
        write_path.write_text(dumps_findings(payload), encoding="utf-8")
    return payload, 0


def run_validate(args: argparse.Namespace) -> tuple[dict[str, object], int]:
    path = args.dialect if args.dialect is not None else default_dialect_path()
    text = path.read_text(encoding="utf-8")
    grammar = parse_dialect(text, path=display_path(path))
    payload = validate_dialect(grammar)
    return payload, 0 if payload["ok"] else 1


def main(argv: list[str] | None = None) -> int:
    parser = build_arg_parser()
    args = parser.parse_args(argv)
    if args.validate:
        payload, code = run_validate(args)
        if args.json:
            sys.stdout.write(dumps_findings(payload))
        else:
            print_validate_text(payload)
        return code

    payload, code = run_audit_md(args)
    if args.json:
        sys.stdout.write(dumps_findings(payload))
    else:
        print_audit_text(payload)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
