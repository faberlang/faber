//! Parser tests for the retired module-level static (st1 R1, R21).
//!
//! Contract: a statement-initial `generis`/`static` followed by a type is the
//! retired declaration. It is parsed whole (so recovery resumes after it) and
//! reported as PARSE010 `static_decl_retired` with args `keyword` (the
//! spelling as written, i.e. the pack's `generis` row) and `name`, at top
//! level and inside blocks alike. `generis` followed by anything else stays
//! an ordinary identifier, and `generis` as a genus member keeps its field
//! meaning. The module constant is `const T X = e` / la `fixum T X = e`.

use crate::test_support::{has_arg, parse_ok, parse_program};
use crate::{ParseErrorKind, ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::StmtKind;

/// English surface for the words these tests use: `static` is the pack's
/// `generis` row, `const` the `fixum` row, `int` the `numerus` type.
struct EnStaticPack;

impl KeywordPack for EnStaticPack {
    fn pack_id(&self) -> &'static str {
        "en-static-test"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "static" => Some("generis"),
            "const" => Some("fixum"),
            "let" => Some("sit"),
            "fn" => Some("functio"),
            _ => None,
        }
    }

    fn type_latin_key(&self, text: &str) -> Option<&str> {
        (text == "int").then_some("numerus")
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_en(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &EnStaticPack))
}

/// The single `static_decl_retired` error, asserted for `keyword` and `name`.
fn assert_retired(result: &ParseResult, keyword: &str, name: &str) {
    let matching = result
        .errors
        .iter()
        .filter(|err| has_arg(err, "issue", "static_decl_retired"))
        .collect::<Vec<_>>();
    assert_eq!(
        matching.len(),
        1,
        "expected one static_decl_retired error, got {:?}",
        result.errors
    );
    assert_eq!(matching[0].kind, ParseErrorKind::DeclarationInvalid);
    assert!(has_arg(matching[0], "keyword", keyword), "{matching:?}");
    assert!(has_arg(matching[0], "name", name), "{matching:?}");
}

#[test]
fn latin_top_level_static_is_retired() {
    let result = parse_program("generis numerus LIMES = 4 * 1024");
    assert_retired(&result, "generis", "LIMES");
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
}

#[test]
fn english_top_level_static_reports_the_pack_spelling() {
    let result = parse_en("static int LIMES = 4096");
    assert_retired(&result, "static", "LIMES");
}

#[test]
fn annotated_static_is_retired() {
    let result = parse_program("@ publica\ngeneris textus NOMEN = \"x\"");
    assert_retired(&result, "generis", "NOMEN");
}

#[test]
fn retired_static_with_arrow_initializer_is_still_one_retirement_error() {
    // The retirement answer replaces the old per-shape errors: whatever the
    // old spelling got wrong, the user is told the declaration is gone.
    let result = parse_program("generis numerus L ← 4");
    assert_retired(&result, "generis", "L");
    assert_eq!(result.errors.len(), 1, "{:?}", result.errors);
}

#[test]
fn retired_static_inside_a_block_is_the_same_diagnostic() {
    let result =
        parse_program("functio f() → numerus {\n    generis numerus L = 4\n    redde 1\n}");
    assert_retired(&result, "generis", "L");
    assert!(
        !result
            .errors
            .iter()
            .any(|err| has_arg(err, "issue", "static_not_top_level")),
        "{:?}",
        result.errors
    );
}

#[test]
fn recovery_resumes_after_the_retired_declaration() {
    let result = parse_program(
        "generis numerus A = 1\nfixum numerus B = 2\nfunctio f() → vacuum { tacet }\nfixum numerus C = 3",
    );
    assert_retired(&result, "generis", "A");
    let program = result.program.as_ref().expect("program");
    assert_eq!(program.statements.len(), 4, "{:?}", result.errors);
    assert!(matches!(program.statements[2].kind, StmtKind::Func(_)));
}

#[test]
fn generis_before_a_non_type_stays_an_identifier() {
    let result = parse_program("generis ← 1");
    assert!(
        !result
            .errors
            .iter()
            .any(|err| has_arg(err, "issue", "static_decl_retired")),
        "{:?}",
        result.errors
    );
}

#[test]
fn module_constant_is_the_replacement() {
    let _ = parse_ok("fixum numerus LIMES = 4 * 1024");
    let result = parse_en("const int LIMES = 4096");
    assert!(result.success(), "{:?}", result.errors);
}

#[test]
fn static_genus_field_keeps_member_meaning() {
    let _ = parse_ok("genus Capsa {\n    generis numerus MAXIMUM = 8\n    varia numerus n\n}");
}
