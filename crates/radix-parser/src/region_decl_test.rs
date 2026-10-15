//! `regio` (en `module`) file-name declaration (DECISIONS D7.7, F8).
//!
//! `regio NAME` is optional, at most one, and only the first declaration of
//! the file body. Anywhere else at top level it is a named error; as an
//! ordinary word it stays an identifier (contextual keyword).

use crate::test_support::{ParseExpectation, assert_parse_error_matches, parse_ok};
use crate::{ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::{StmtKind, Trivia};

struct EnModulePack;

impl KeywordPack for EnModulePack {
    fn pack_id(&self) -> &'static str {
        "en"
    }

    fn keyword_token_kind(&self, text: &str) -> Option<TokenKind> {
        match text {
            "import" => Some(TokenKind::Importa),
            "from" => Some(TokenKind::Ex),
            "const" => Some(TokenKind::Fixum),
            _ => None,
        }
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "module" => Some("regio"),
            "import" => Some("importa"),
            "from" => Some("ex"),
            "const" => Some("fixum"),
            "let" => Some("sit"),
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
    parse(lex_with_locale_pack(source, &EnModulePack))
}

fn region_name(result: &ParseResult) -> Option<String> {
    let program = result.program.as_ref()?;
    let region = program.region.as_ref()?;
    Some(result.interner.resolve(region.name.name).to_owned())
}

#[test]
fn leading_regio_names_the_file() {
    let result = parse_ok("regio geometria\n\nimporta ex \"norma:chorda\" chorda\n");
    assert_eq!(region_name(&result).as_deref(), Some("geometria"));
    let program = result.program.expect("program");
    assert_eq!(program.statements.len(), 1);
    assert!(matches!(program.statements[0].kind, StmtKind::Import(_)));
}

#[test]
fn file_without_regio_has_no_region() {
    let result = parse_ok("importa ex \"norma:chorda\" chorda\n");
    assert_eq!(region_name(&result), None);
}

#[test]
fn en_module_spelling_names_the_file() {
    let result = parse_en("module geometry\nimport from \"norma:chorda\" chorda\n");
    assert!(result.success(), "{:?}", result.errors);
    assert_eq!(region_name(&result).as_deref(), Some("geometry"));
}

#[test]
fn en_pack_does_not_claim_latin_regio() {
    // Locale surfaces are sealed: under the en pack `regio` is an identifier,
    // so `regio geometria` is not a file-name declaration.
    let result = parse_en("regio geometria\n");
    assert_eq!(region_name(&result), None);
}

#[test]
fn regio_leading_comment_block_rides_the_declaration() {
    let source = "# Geometria plana.\n# Puncta et lineae.\nregio geometria\n";
    let result = parse_ok(source);
    let program = result.program.expect("program");
    let region = program.region.expect("region");
    let comments = region
        .leading_trivia
        .iter()
        .filter(|trivia| matches!(trivia, Trivia::CommentLine(_)))
        .count();
    assert_eq!(comments, 2);
}

#[test]
fn leading_comment_without_regio_stays_on_first_statement() {
    let result = parse_ok("# nota\nsit numerus x ← 1\n");
    let program = result.program.expect("program");
    assert!(program.region.is_none());
    assert!(
        program.statements[0]
            .leading_trivia
            .iter()
            .any(|trivia| matches!(trivia, Trivia::CommentLine(_)))
    );
}

#[test]
fn regio_after_import_is_misplaced() {
    assert_parse_error_matches(
        "importa ex \"norma:chorda\" chorda\nregio geometria\n",
        ParseExpectation::Args(&[("issue", "module_declaration_misplaced")]),
    );
}

#[test]
fn regio_after_declaration_is_misplaced() {
    assert_parse_error_matches(
        "sit numerus x ← 1\nregio geometria\n",
        ParseExpectation::Args(&[("issue", "module_declaration_misplaced")]),
    );
}

#[test]
fn second_regio_is_duplicate() {
    assert_parse_error_matches(
        "regio geometria\nregio alia\n",
        ParseExpectation::Args(&[("issue", "module_declaration_duplicate")]),
    );
}

#[test]
fn regio_word_stays_an_identifier_elsewhere() {
    let result = parse_ok(
        "sit numerus regio ← 1\nfunctio f(numerus regio) → numerus {\n    redde regio\n}\n",
    );
    assert_eq!(region_name(&result), None);
    assert_eq!(result.program.expect("program").statements.len(), 2);
}

#[test]
fn en_module_word_stays_an_identifier_elsewhere() {
    let result = parse_en("let int module ← 1\nlet int other ← module\n");
    assert!(result.success(), "{:?}", result.errors);
    assert_eq!(region_name(&result), None);
    assert_eq!(result.program.expect("program").statements.len(), 2);
}

#[test]
fn back_to_back_default_imports_are_two_imports() {
    // A default-named import must not swallow the next line's `importa` as
    // its binding name.
    let result = parse_ok("importa ex \"./a/geo\"\nimporta ex \"./b/linea\"\n");
    let program = result.program.expect("program");
    assert_eq!(program.statements.len(), 2);
    assert!(
        program
            .statements
            .iter()
            .all(|stmt| matches!(stmt.kind, StmtKind::Import(_)))
    );
}
