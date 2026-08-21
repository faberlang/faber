#!/usr/bin/env python3
"""Grammar pipeline: validate, render, emit, and drift-check the Faber grammar.

`--audit-md` retains the GSD-1 Markdown sin report for historical and locale
analysis. `--validate` checks dialect closure and reachability. GSD-6's
`--render` and `--emit` produce the identity EBNF and one-way grammar.jsonl;
`--check` is the fail-closed normative-triple gate used by stage 1.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import sys
import tomllib
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
TRIPLE_SCHEMA = "faber-grammar/v1"
SOURCE_REGION_RE = re.compile(r"^@(rewrite|lexicon|keywords|glyphs)\s*$")
METADATA_RE = re.compile(r"^@([A-Za-z_][A-Za-z0-9_-]*)(?:\s+(.*))?$")
KEYWORD_ROW_RE = re.compile(r"^# \| \*?([^|]+?)\s*\|\s*([^|]+?)\s*\|\s*([^|]+?)\s*\|\s*$")


@dataclass
class Production:
    name: str
    line: int
    rhs: str
    path: str = ""
    region: str = "grammar"
    metadata: dict[str, list[str]] = field(default_factory=dict)


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


def parse_source(text: str, *, path: str) -> Grammar:
    """Parse a dialect source while retaining regions and downward metadata."""
    start: str | None = None
    grammar_n: int | None = None
    region = "preamble"
    productions: list[Production] = []
    by_name: dict[str, Production] = {}
    pending: dict[str, list[str]] = {}
    current: Production | None = None

    for line_no, raw in enumerate(text.splitlines(), start=1):
        cleaned = strip_ebnf_noise(raw).rstrip()
        stripped = cleaned.strip()
        if not stripped:
            continue

        region_match = SOURCE_REGION_RE.match(stripped)
        if region_match:
            region = region_match.group(1)
            current = None
            pending = {}
            continue

        directive = METADATA_RE.match(stripped)
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
            elif name not in {"rewrite", "lexicon", "keywords", "glyphs"}:
                pending.setdefault(name, []).append(rest)
            continue

        match = PROD_HEAD_RE.match(cleaned)
        if match:
            name, rhs = match.group(1), match.group(2)
            if name in by_name:
                production = by_name[name]
                production.rhs = (production.rhs + " " + rhs).strip()
                production.metadata.update(
                    {key: list(values) for key, values in pending.items()}
                )
            else:
                production = Production(
                    name=name,
                    line=line_no,
                    rhs=rhs,
                    path=path,
                    region=region,
                    metadata={key: list(values) for key, values in pending.items()},
                )
                productions.append(production)
                by_name[name] = production
            current = production
            pending = {}
            continue

        if current is not None and (raw[:1].isspace() or stripped.startswith("|")):
            current.rhs = (current.rhs + " " + stripped).strip()

    return Grammar(
        path=path,
        productions=productions,
        start=start,
        grammar_n=grammar_n,
        by_name=by_name,
    )


def parse_dialect(text: str, *, path: str) -> Grammar:
    return parse_source(text, path=path)


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


def default_source_path() -> Path:
    return default_ebnf_path().parent / "grammar" / "source.fg"


def default_sidecar_path() -> Path:
    return default_source_path().with_name("sidecar.en.toml")


def default_jsonl_path() -> Path:
    return default_source_path().with_name("grammar.jsonl")


def production_anchor(name: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", name.lower()).strip("-")


def source_keyword_rows(source_text: str, grammar: Grammar) -> list[tuple[str, str, str]]:
    """Return one derived row per quoted Latin literal in the source."""
    known: dict[str, tuple[str, str]] = {}
    in_keywords = False
    for raw in source_text.splitlines():
        stripped = raw.strip()
        if stripped == "@keywords":
            in_keywords = True
            continue
        if in_keywords and stripped.startswith("@lexicon"):
            break
        if not in_keywords:
            continue
        match = KEYWORD_ROW_RE.match(raw)
        if not match:
            continue
        category, spellings, meaning = (part.strip() for part in match.groups())
        if category.lower() in {"category", "---"}:
            continue
        for literal in re.findall(r"`([^`]+)`", spellings):
            for word in re.findall(r"[a-z][a-z0-9_]*", literal):
                known.setdefault(word, (category, meaning))

    rows: list[tuple[str, str, str]] = []
    literals = sorted(keyword_literals(grammar))
    for literal in literals:
        category, meaning = known.get(
            literal, ("Grammar", "keyword literal derived from the production")
        )
        rows.append((category, literal, meaning))
    return rows


def source_lexicon_rows(source_text: str) -> list[tuple[str, str, str]]:
    rows: list[tuple[str, str, str]] = []
    in_lexicon = False
    status = "live"
    comments: list[str] = []
    for raw in source_text.splitlines():
        stripped = raw.strip()
        if stripped == "@lexicon":
            in_lexicon = True
            continue
        if in_lexicon and stripped == "@rewrite":
            break
        if not in_lexicon:
            continue
        if stripped.startswith("@status"):
            status = stripped.split(None, 1)[1] if " " in stripped else "live"
            continue
        match = PROD_HEAD_RE.match(stripped)
        if match:
            rows.append((match.group(1), status, " ".join(comments).strip()))
            status = "live"
            comments = []
            continue
        if stripped.startswith("#"):
            comments.append(stripped[1:].strip())
    return rows


def load_sidecar(path: Path) -> dict[str, object]:
    if not path.is_file():
        raise FileNotFoundError(f"missing grammar sidecar: {path}")
    with path.open("rb") as handle:
        data = tomllib.load(handle)
    productions = data.get("productions", {})
    if not isinstance(productions, dict):
        raise ValueError(f"{path}: productions must be a TOML table")
    return data


def canonical_source_grammar(source_text: str, source_path: Path) -> Grammar:
    grammar = parse_source(source_text, path=display_path(source_path))
    payload = validate_dialect(grammar)
    if not payload["ok"]:
        raise ValueError(
            "source grammar is not closed/reachable: "
            + json.dumps(payload, sort_keys=True)
        )
    return grammar


def jsonl_records(
    source_text: str, source_path: Path, sidecar_text: str
) -> list[dict[str, object]]:
    grammar = canonical_source_grammar(source_text, source_path)
    sidecar_hash = "sha256:" + hashlib.sha256(sidecar_text.encode("utf-8")).hexdigest()
    keyword_counts = keyword_literals(grammar)
    records: list[dict[str, object]] = []
    for production in grammar.productions:
        formerly = production.metadata.get("formerly", [])
        record = {
            "schema": TRIPLE_SCHEMA,
            "grammar": grammar.grammar_n,
            "id": production.name,
            "anchor": production_anchor(production.name),
            "region": production.region,
            "line": production.line,
            "rhs": " ".join(production.rhs.split()),
            "status": (production.metadata.get("status") or ["live"])[-1],
            "formerly": formerly,
            "keywords": sorted(
                literal
                for literal in rhs_literals(production.rhs)
                if literal in keyword_counts
            ),
            "sidecar": sidecar_hash,
        }
        records.append(record)
    return records


def dumps_jsonl(records: list[dict[str, object]]) -> str:
    return "".join(
        json.dumps(record, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
        + "\n"
        for record in records
    )


def grammar_fence(grammar: Grammar) -> str:
    lines = ["```ebnf"]
    # Markdown audit infers its start from the first production. Render the
    # rewrite grammar before the lexical appendix so the identity starts at
    # @start (`fab_file`) while retaining every lexical definition.
    ordered = [
        production
        for production in grammar.productions
        if production.region == "rewrite"
    ] + [
        production
        for production in grammar.productions
        if production.region != "rewrite"
    ]
    for index, production in enumerate(ordered, start=1):
        if production.metadata.get("formerly"):
            aliases = ", ".join(production.metadata["formerly"])
            lines.append(f"# formerly: {aliases}")
        rhs = " ".join(production.rhs.split())
        lines.append(f"# [{index:03d}] {production.name}")
        lines.append(f"{production.name} ::= {rhs}".rstrip())
    lines.append("```")
    return "\n".join(lines)


def render_identity(
    source_text: str, source_path: Path, sidecar_text: str
) -> str:
    grammar = canonical_source_grammar(source_text, source_path)
    sidecar = tomllib.loads(sidecar_text)
    document = sidecar.get("document", {})
    if not isinstance(document, dict):
        document = {}
    intro = str(document.get("intro", ""))
    prose = str(document.get("prose_markdown", "")).strip()
    lexicon = source_lexicon_rows(source_text)
    keywords = source_keyword_rows(source_text, grammar)
    comma_rows = comma_variance_rows(source_text, path=display_path(source_path))

    lines = [
        "# Faber Language Specification",
        "",
        "This file is generated from `docs/grammar/source.fg`; hand edits fail the",
        "stage-1 normative-triple gate. Production IDs are the grammar's stable",
        "snake_case spine and their anchors are derived from those IDs.",
        "",
        "## Grammar {#grammar}",
        "",
    ]
    if intro:
        lines.extend([intro.strip(), ""])
    lines.extend([grammar_fence(grammar), ""])
    lines.extend(
        [
            "## Production Index {#production-index}",
            "",
            "| ID | Anchor | Status | Former names |",
            "|---|---|---|---|",
        ]
    )
    for production in grammar.productions:
        status = (production.metadata.get("status") or ["live"])[-1]
        former = ", ".join(production.metadata.get("formerly", [])) or "—"
        anchor = production_anchor(production.name)
        lines.append(
            f"| [`{production.name}`](#{anchor}) | `#{anchor}` | {status} | {former} |"
        )
    lines.extend(["", "## Lexicon Appendix {#lexicon}", ""])
    lines.extend(
        [
            "The lexical tier is descriptive and remains owned by the live lexer and",
            "driver. `capture-pending` rows intentionally carry no invented token shape.",
            "",
            "| Terminal | Status | Capture notes |",
            "|---|---|---|",
        ]
    )
    for name, status, note in lexicon:
        lines.append(f"| `{name}` | `{status}` | {note or '—'} |")
    lines.extend(["", "## Keyword Reference {#keyword-reference}", ""])
    lines.extend(
        [
            "This table is derived from the quoted Latin literals in the source",
            "productions. It is not a second keyword authority.",
            "",
            "| Category | Faber | Meaning |",
            "|---|---|---|",
        ]
    )
    for category, literal, meaning in keywords:
        lines.append(f"| {category} | `{literal}` | {meaning} |")
    lines.extend(["", "## Comma Separator Table {#comma-separator-law}", ""])
    lines.extend(
        [
            "Optional commas are forbidden. The source currently has no `','?`",
            "positions; every comma-bearing production is either required or absent.",
            "",
            "| Production | Source row |",
            "|---|---|",
        ]
    )
    if comma_rows:
        for row in comma_rows:
            lines.append(
                f"| `{row.get('production') or 'unknown'}` | `{row['snippet']}` |"
            )
    else:
        lines.append("| — | no optional comma positions |")

    lines.extend(["", "## Normative Language Notes {#normative-language-notes}", ""])
    if prose:
        lines.extend(prose.splitlines())
    else:
        lines.append("Normative details are maintained in `docs/grammar/sidecar.en.toml`.")
    lines.append("")
    return "\n".join(lines).rstrip() + "\n"


def generated_artifacts(
    source_path: Path, sidecar_path: Path
) -> tuple[str, str]:
    source_text = source_path.read_text(encoding="utf-8")
    sidecar_text = sidecar_path.read_text(encoding="utf-8")
    return (
        render_identity(source_text, source_path, sidecar_text),
        dumps_jsonl(jsonl_records(source_text, source_path, sidecar_text)),
    )


def check_triple(
    source_path: Path, sidecar_path: Path, ebnf_path: Path, jsonl_path: Path
) -> tuple[bool, dict[str, object]]:
    expected_ebnf, expected_jsonl = generated_artifacts(source_path, sidecar_path)
    actual_ebnf = ebnf_path.read_text(encoding="utf-8") if ebnf_path.is_file() else ""
    actual_jsonl = jsonl_path.read_text(encoding="utf-8") if jsonl_path.is_file() else ""
    mismatches: list[str] = []
    if actual_ebnf != expected_ebnf:
        mismatches.append(display_path(ebnf_path))
    if actual_jsonl != expected_jsonl:
        mismatches.append(display_path(jsonl_path))
    payload = {
        "schema": TRIPLE_SCHEMA,
        "ok": not mismatches,
        "source": display_path(source_path),
        "render": display_path(ebnf_path),
        "jsonl": display_path(jsonl_path),
        "mismatches": mismatches,
        "productions": len(expected_jsonl.splitlines()),
    }
    return not mismatches, payload


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
        "--render",
        action="store_true",
        help="render the canonical identity EBNF.md from source.fg",
    )
    parser.add_argument(
        "--emit",
        action="store_true",
        help="emit grammar.jsonl from source.fg",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="fail when source, identity render, or grammar.jsonl drift",
    )
    parser.add_argument(
        "--source",
        type=Path,
        default=None,
        help="source.fg path for renderer modes",
    )
    parser.add_argument(
        "--sidecar",
        type=Path,
        default=None,
        help="sidecar.en.toml path for renderer modes",
    )
    parser.add_argument(
        "--jsonl",
        type=Path,
        default=None,
        help="grammar.jsonl path for --emit/--check",
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=None,
        help="render output path for --render (default: sibling faber/docs/EBNF.md)",
    )
    parser.add_argument(
        "--jsonl-output",
        type=Path,
        default=None,
        help="emit output path (default: docs/grammar/grammar.jsonl)",
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


def run_render(args: argparse.Namespace) -> tuple[str, Path]:
    source_path = (args.source or default_source_path()).resolve()
    sidecar_path = (args.sidecar or default_sidecar_path()).resolve()
    output = (args.output or default_ebnf_path()).resolve()
    rendered, _ = generated_artifacts(source_path, sidecar_path)
    output.write_text(rendered, encoding="utf-8")
    return rendered, output


def run_emit(args: argparse.Namespace) -> tuple[str, Path]:
    source_path = (args.source or default_source_path()).resolve()
    sidecar_path = (args.sidecar or default_sidecar_path()).resolve()
    output = (args.jsonl_output or args.jsonl or default_jsonl_path()).resolve()
    _, emitted = generated_artifacts(source_path, sidecar_path)
    output.write_text(emitted, encoding="utf-8")
    return emitted, output


def run_check(args: argparse.Namespace) -> tuple[dict[str, object], int]:
    source_path = (args.source or default_source_path()).resolve()
    sidecar_path = (args.sidecar or default_sidecar_path()).resolve()
    ebnf_path = (args.ebnf or default_ebnf_path()).resolve()
    jsonl_path = (args.jsonl or default_jsonl_path()).resolve()
    try:
        ok, payload = check_triple(source_path, sidecar_path, ebnf_path, jsonl_path)
    except (OSError, ValueError, tomllib.TOMLDecodeError) as exc:
        payload = {
            "schema": TRIPLE_SCHEMA,
            "ok": False,
            "source": display_path(source_path),
            "render": display_path(ebnf_path),
            "jsonl": display_path(jsonl_path),
            "mismatches": [str(exc)],
        }
        ok = False
    return payload, 0 if ok else 1


def main(argv: list[str] | None = None) -> int:
    parser = build_arg_parser()
    args = parser.parse_args(argv)
    if args.check:
        payload, code = run_check(args)
        if args.json:
            sys.stdout.write(dumps_findings(payload))
        else:
            status = "OK" if payload.get("ok") else "FAIL"
            print(f"=== grammar triple: {status} ===")
            if payload.get("ok"):
                print(f"productions: {payload.get('productions', '?')}")
                print("source, identity render, and grammar.jsonl agree")
            else:
                for mismatch in payload.get("mismatches", []):
                    print(f"  - drift: {mismatch}")
        return code

    if args.render or args.emit:
        try:
            if args.render:
                _, output = run_render(args)
                print(f"rendered {display_path(output)}")
            if args.emit:
                _, output = run_emit(args)
                print(f"emitted {display_path(output)}")
            return 0
        except (OSError, ValueError, tomllib.TOMLDecodeError) as exc:
            print(f"grammar pipeline: {exc}", file=sys.stderr)
            return 1

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
