//! Format golden corpus harness (faber-owned).
//!
//! Reads every `corpus-product/format/*.fab` fixture and formats it through
//! the exact CLI path FR-2 will install: [`super::format::format_session`] +
//! [`radix::module::driver::reemit_faber`] +
//! [`super::format::formatted_source_for_write`]. Output must match
//! `*.format.expected`. Each fixture is idempotent, byte-stable, and
//! re-parses under its own locale pack.
//!
//! Gated on `hir-faber` because the canonical HIR emitter is.

#![cfg(feature = "hir-faber")]

use super::format::{format_session, formatted_source_for_write};
use radix::module::driver::faber_format::test_gate::{
    assert_canonical_idempotent_with_session, lex_parse_with_pack,
};
use radix::module::driver::reemit_faber;
use radix::module::lexer::TokenKind;
use radix::module::syntax::StmtKind;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

fn corpus_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../corpus-product/format")
}

/// Fixture sources in the corpus (one `*.fab` per pinned expectation).
fn fixture_sources() -> Vec<PathBuf> {
    let mut fixtures = fs::read_dir(corpus_dir())
        .expect("read corpus-product/format")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("fab"))
        .collect::<Vec<_>>();
    fixtures.sort();
    fixtures
}

/// Format a fixture through the exact CLI canonical pipeline.
fn canonical_pipeline(
    path: &Path,
    source: &str,
) -> Result<(radix::module::driver::Session, String, String), String> {
    let session =
        format_session(path, None, source).map_err(|err| format!("format session: {err:?}"))?;
    let name = path.display().to_string();
    let result = reemit_faber(
        &session,
        session
            .config
            .locale_pack
            .as_ref()
            .expect("format emission locale pack"),
        &name,
        source,
    );
    if !result.success() {
        return Err(format!(
            "canonical format failed on {name}: {:?}",
            result.diagnostics
        ));
    }
    let body = result.output.expect("canonical output").code;
    let written = formatted_source_for_write(path, source, &body)
        .map_err(|err| format!("format write pipeline: {err}"))?;
    Ok((session, body, written))
}

fn fixture_stem(path: &Path) -> &str {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .expect("utf8 fixture stem")
}

fn is_en_source(source: &str) -> bool {
    let Some(after_open) = source.strip_prefix("+++\n") else {
        return false;
    };
    let front = after_open.split("+++").next().unwrap_or("");
    front.lines().any(|line| line.trim() == "locale = \"en\"")
}

#[derive(Debug, PartialEq, Eq)]
struct SurfaceCensus {
    annotations: usize,
    declarations: BTreeMap<&'static str, usize>,
    annotated_declarations: BTreeMap<&'static str, usize>,
    operators: BTreeMap<&'static str, usize>,
    token_kinds: BTreeMap<String, usize>,
    comments: usize,
}

fn increment<K: Ord>(counts: &mut BTreeMap<K, usize>, key: K) {
    *counts.entry(key).or_default() += 1;
}

fn declaration_kind(kind: &StmtKind) -> Option<&'static str> {
    match kind {
        StmtKind::Var(_) => Some("constant"),
        StmtKind::Func(_) => Some("function"),
        StmtKind::Class(_) => Some("struct"),
        StmtKind::Interface(_) => Some("interface"),
        StmtKind::TypeAlias(_) => Some("type-alias"),
        StmtKind::Schema(_) => Some("schema"),
        StmtKind::Enum(_) => Some("enum"),
        StmtKind::Union(_) => Some("union"),
        StmtKind::Import(_) => Some("import"),
        _ => None,
    }
}

fn operator_kind(kind: &TokenKind) -> Option<&'static str> {
    match kind {
        TokenKind::Plus => Some("plus"),
        TokenKind::Minus => Some("minus"),
        TokenKind::Star => Some("star"),
        TokenKind::Slash => Some("slash"),
        TokenKind::Percent => Some("percent"),
        TokenKind::BitwiseAnd => Some("bitwise-and"),
        TokenKind::BitwiseOr => Some("bitwise-or"),
        TokenKind::BitwiseXor => Some("bitwise-xor"),
        TokenKind::BitwiseNot => Some("bitwise-not"),
        TokenKind::BitwiseShl => Some("shift-left"),
        TokenKind::BitwiseShr => Some("shift-right"),
        TokenKind::Transfer => Some("transfer"),
        TokenKind::MiddleDot => Some("middle-dot"),
        TokenKind::Cross => Some("cross"),
        TokenKind::CircledTimes => Some("circled-times"),
        TokenKind::CircledDot => Some("circled-dot"),
        TokenKind::CircledDivide => Some("circled-divide"),
        TokenKind::Maximum => Some("maximum"),
        TokenKind::Minimum => Some("minimum"),
        TokenKind::Transpose => Some("transpose"),
        TokenKind::Nabla => Some("nabla"),
        TokenKind::Eq => Some("equals"),
        TokenKind::Assign => Some("assign"),
        TokenKind::ConversioAssign => Some("convert-assign"),
        TokenKind::EqEq | TokenKind::EqEqEq => Some("strict-equals"),
        TokenKind::ApproxEq => Some("promoted-equals"),
        TokenKind::ApproxNotEq => Some("promoted-not-equals"),
        TokenKind::FuzzyEq => Some("fuzzy-equals"),
        TokenKind::FuzzyNotEq => Some("fuzzy-not-equals"),
        TokenKind::BangEq | TokenKind::BangEqEq => Some("strict-not-equals"),
        TokenKind::Lt | TokenKind::LtAscii => Some("less-than"),
        TokenKind::Gt | TokenKind::GtAscii => Some("greater-than"),
        TokenKind::LtEq => Some("less-than-equals"),
        TokenKind::GtEq => Some("greater-than-equals"),
        TokenKind::PostInc => Some("increment"),
        TokenKind::PostDec => Some("decrement"),
        TokenKind::DotDot => Some("exclusive-range"),
        TokenKind::Ellipsis => Some("inclusive-range"),
        TokenKind::Arrow => Some("return-arrow"),
        TokenKind::ExitArrow => Some("exit-arrow"),
        _ => None,
    }
}

fn normalized_token_kind(kind: &TokenKind, spelling: &str) -> Option<String> {
    if matches!(spelling, "ergo" | "then") {
        return None;
    }
    if matches!(
        kind,
        TokenKind::LineComment(_)
            | TokenKind::Newline
            | TokenKind::Eof
            | TokenKind::LBrace
            | TokenKind::RBrace
            | TokenKind::LParen
            | TokenKind::RParen
            | TokenKind::Comma
            | TokenKind::Semicolon
            | TokenKind::Ergo
    ) {
        return None;
    }
    if let Some(operator) = operator_kind(kind) {
        return Some(format!("operator:{operator}"));
    }
    let debug = format!("{kind:?}");
    Some(
        debug
            .split(['(', '{'])
            .next()
            .expect("token debug variant")
            .trim()
            .to_owned(),
    )
}

fn surface_census(
    name: &str,
    source: &str,
    pack: &radix::module::locale::LocalePack,
) -> Result<SurfaceCensus, String> {
    let peeled = radix::module::driver::peel_raw_source(name, source)
        .map_err(|err| format!("frontmatter: {err}"))?;
    let lex = radix::module::lexer::lex_with_locale_pack(peeled.body, pack);
    if !lex.success() {
        return Err(format!("lex errors: {:?}", lex.errors));
    }

    let mut annotations = 0;
    let mut operators = BTreeMap::new();
    let mut token_kinds = BTreeMap::new();
    let mut comments = 0;
    for token in &lex.tokens {
        if matches!(token.kind, TokenKind::At) {
            annotations += 1;
        }
        if matches!(token.kind, TokenKind::LineComment(_)) {
            comments += 1;
        }
        if let Some(operator) = operator_kind(&token.kind) {
            increment(&mut operators, operator);
        }
        let spelling = peeled
            .body
            .get(token.span.start as usize..token.span.end as usize)
            .unwrap_or("");
        if let Some(kind) = normalized_token_kind(&token.kind, spelling) {
            increment(&mut token_kinds, kind);
        }
    }

    let parsed = radix::module::parser::parse(lex);
    if !parsed.success() {
        return Err(format!("parse errors: {:?}", parsed.errors));
    }
    let program = parsed.program.ok_or("parse succeeded without program")?;
    let mut declarations = BTreeMap::new();
    let mut annotated_declarations = BTreeMap::new();
    for statement in &program.statements {
        if let Some(kind) = declaration_kind(&statement.kind) {
            increment(&mut declarations, kind);
            if !statement.annotations.is_empty() {
                increment(&mut annotated_declarations, kind);
            }
        }
    }

    Ok(SurfaceCensus {
        annotations,
        declarations,
        annotated_declarations,
        operators,
        token_kinds,
        comments,
    })
}

/// The golden corpus is byte-exact against the canonical HIR pin, idempotent
/// under it, and re-parses under each fixture's own locale pack.
#[test]
fn format_corpus_is_byte_exact_idempotent_and_reparses() {
    let fixtures = fixture_sources();
    assert!(
        fixtures.len() >= 12,
        "corpus should cover the la shapes plus ≥3 en fixtures, found {}",
        fixtures.len()
    );

    let mut en_fixtures = 0usize;
    let mut failures = Vec::new();
    for fixture in &fixtures {
        let stem = fixture_stem(fixture);
        let expected_path = corpus_dir().join(format!("{stem}.format.expected"));
        let source = fs::read_to_string(fixture).expect("read fixture source");
        if is_en_source(&source) {
            en_fixtures += 1;
        }

        let (session, body, formatted) = match canonical_pipeline(fixture, &source) {
            Ok(result) => result,
            Err(detail) => {
                failures.push(format!("{stem}: {detail}"));
                continue;
            }
        };
        let expected = match fs::read_to_string(&expected_path) {
            Ok(expected) => expected,
            Err(err) => {
                failures.push(format!("{stem}: missing expectation: {err}"));
                continue;
            }
        };
        if formatted != expected {
            failures.push(format!(
                "{stem}: canonical output must be byte-identical to the pinned .format.expected\n===== expected\n{expected}===== actual\n{formatted}"
            ));
        }

        match canonical_pipeline(fixture, &formatted) {
            Ok((_, _, reformatted)) if reformatted == formatted => {}
            Ok((_, _, _)) => {
                failures.push(format!("{stem}: format(format(x)) must equal format(x)"));
            }
            Err(detail) => failures.push(format!("{stem} second pass: {detail}")),
        }

        let pack = session
            .config
            .locale_pack
            .as_ref()
            .expect("format corpus session locale pack");
        if let Err(detail) = lex_parse_with_pack(&body, Some(pack)) {
            failures.push(format!("{stem} re-parse under own locale: {detail}"));
        }

        let census_name = fixture.display().to_string();
        match (
            surface_census(&census_name, &source, pack),
            surface_census(&census_name, &body, pack),
        ) {
            (Ok(input), Ok(output)) => {
                let diagnostic = format!(
                    "input comments={}, output comments={}",
                    input.comments, output.comments
                );
                if input.annotations != output.annotations {
                    failures.push(format!(
                        "{stem}: annotation census changed: input={}, output={} ({diagnostic})",
                        input.annotations, output.annotations
                    ));
                }
                if input.declarations != output.declarations {
                    failures.push(format!(
                        "{stem}: top-level declaration census changed: input={:?}, output={:?} ({diagnostic})",
                        input.declarations, output.declarations
                    ));
                }
                if input.annotated_declarations != output.annotated_declarations {
                    failures.push(format!(
                        "{stem}: annotated top-level declaration census changed: input={:?}, output={:?} ({diagnostic})",
                        input.annotated_declarations, output.annotated_declarations
                    ));
                }
                if input.operators != output.operators {
                    failures.push(format!(
                        "{stem}: operator census changed: input={:?}, output={:?} ({diagnostic})",
                        input.operators, output.operators
                    ));
                }
                if input.token_kinds != output.token_kinds {
                    failures.push(format!(
                        "{stem}: non-trivia token-kind census changed: input={:?}, output={:?} ({diagnostic})",
                        input.token_kinds, output.token_kinds
                    ));
                }
            }
            (Err(detail), _) => failures.push(format!("{stem} input census: {detail}")),
            (_, Err(detail)) => failures.push(format!("{stem} output census: {detail}")),
        }

        let name = fixture.display().to_string();
        if let Err(detail) = assert_canonical_idempotent_with_session(&session, &name, &source) {
            failures.push(format!("{stem} canonical test_gate idempotent: {detail}"));
        }

        if is_en_source(&source)
            && (body.contains("numerus")
                || body.contains("fractus")
                || body.contains("textus")
                || body.contains("octeti"))
        {
            failures.push(format!(
                "{stem}: en format must not relatinize types:\n{body}"
            ));
        }
        if stem == "en-types"
            && !(body.contains("int")
                && body.contains("float")
                && body.contains("string")
                && body.contains("bytes"))
        {
            failures.push(format!(
                "{stem}: primitive en type spellings must stay English:\n{body}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "format corpus failures:\n{}",
        failures.join("\n")
    );
    assert!(
        en_fixtures >= 3,
        "corpus must pin ≥3 en-source fixtures, found {en_fixtures}"
    );
}

/// D-A: no leftover policy pin filenames under the format corpus.
#[test]
fn format_corpus_has_no_policy_pin_filenames() {
    let leftover: Vec<_> = fs::read_dir(corpus_dir())
        .expect("read corpus-product/format")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.contains("normalise-v1") || name.contains("pretty-v1"))
        .collect();
    assert!(
        leftover.is_empty(),
        "corpus-product/format must not keep policy pin filenames: {leftover:?}"
    );
}
