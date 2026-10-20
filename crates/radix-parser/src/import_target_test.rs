//! Parser tests for target-native imports: `import python "math" m` (la
//! `importa python "math" m`). The target word sits in the slot where `from`
//! (la `ex`) or `{` stands, is never translated, and an unknown word is PARSE050
//! `import_target_unknown` with a did-you-mean (args `target`, `suggestion`).
//! Tests identify diagnostics by issue and args, never by message text.

use crate::test_support::{has_arg, parse_ok_la, parse_program_la};
use crate::{ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::{ImportDecl, ImportKind, StmtKind};

/// English surface for the import words these tests use.
struct EnImportPack;

impl KeywordPack for EnImportPack {
    fn pack_id(&self) -> &'static str {
        "en-import-test"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "import" => Some("importa"),
            "from" => Some("ex"),
            "as" => Some("ut"),
            "const" => Some("fixum"),
            "let" => Some("sit"),
            _ => None,
        }
    }

    fn type_latin_key(&self, _text: &str) -> Option<&str> {
        None
    }

    fn keyword_suggestion(&self, _text: &str) -> Option<String> {
        None
    }
}

fn parse_en(source: &str) -> ParseResult {
    parse(lex_with_locale_pack(source, &EnImportPack))
}

fn imports(result: &ParseResult) -> Vec<&ImportDecl> {
    result
        .program
        .as_ref()
        .expect("program")
        .statements
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            StmtKind::Import(decl) => Some(decl),
            _ => None,
        })
        .collect()
}

fn target_word(result: &ParseResult, decl: &ImportDecl) -> Option<String> {
    decl.target
        .as_ref()
        .map(|word| result.interner.resolve(word.name).to_owned())
}

fn issue_error<'a>(result: &'a ParseResult, issue: &str) -> &'a crate::ParseError {
    result
        .errors
        .iter()
        .find(|err| has_arg(err, "issue", issue))
        .unwrap_or_else(|| panic!("expected {issue}: {:?}", result.errors))
}

#[test]
fn en_target_import_records_target_specifier_and_binding() {
    let result = parse_en("import python \"math\" m");
    assert!(result.success(), "{:?}", result.errors);
    let [decl] = imports(&result)[..] else {
        panic!("one import")
    };
    assert_eq!(target_word(&result, decl).as_deref(), Some("python"));
    assert_eq!(result.interner.resolve(decl.path), "math");
    assert!(matches!(
        &decl.kind,
        ImportKind::Named {
            alias: None,
            inferred: false,
            ..
        }
    ));
}

#[test]
fn en_go_target_import_resolves() {
    let result = parse_en("import go \"math\" m");
    assert!(result.success(), "{:?}", result.errors);
    let [decl] = imports(&result)[..] else {
        panic!("one import")
    };
    assert_eq!(target_word(&result, decl).as_deref(), Some("go"));
    assert_eq!(result.interner.resolve(decl.path), "math");
    assert!(matches!(
        &decl.kind,
        ImportKind::Named {
            alias: None,
            inferred: false,
            ..
        }
    ));
}

#[test]
fn la_target_import_uses_the_same_untranslated_word() {
    let result = parse_ok_la("importa python \"numpy\" np");
    let [decl] = imports(&result)[..] else {
        panic!("one import")
    };
    assert_eq!(target_word(&result, decl).as_deref(), Some("python"));
    assert_eq!(result.interner.resolve(decl.path), "numpy");
}

#[test]
fn specifier_is_copied_verbatim() {
    for specifier in ["node:fs", "@scope/pkg", "net/http", "os.path"] {
        let result = parse_en(&format!("import ts \"{specifier}\" x"));
        assert!(result.success(), "{specifier}: {:?}", result.errors);
        let [decl] = imports(&result)[..] else {
            panic!("one import")
        };
        assert_eq!(result.interner.resolve(decl.path), specifier);
    }
}

#[test]
fn ordinary_binding_forms_follow_the_specifier() {
    let wildcard = parse_en("import python \"os\" * as osmod");
    assert!(wildcard.success(), "{:?}", wildcard.errors);
    assert!(matches!(
        imports(&wildcard)[0].kind,
        ImportKind::Wildcard { .. }
    ));

    let selective = parse_en("import python \"math\" const sqrt, pi as p");
    assert!(selective.success(), "{:?}", selective.errors);
    let ImportKind::Selective { members } = &imports(&selective)[0].kind else {
        panic!("selective")
    };
    assert_eq!(members.len(), 2);
    assert!(members[1].alias.is_some());
}

#[test]
fn target_imports_stack_and_ordinary_imports_keep_no_target() {
    let result =
        parse_en("import python \"math\" m\nimport python \"os\" os\nimport from \"./a\" a");
    assert!(result.success(), "{:?}", result.errors);
    let all = imports(&result);
    assert_eq!(all.len(), 3);
    assert!(all[0].target.is_some() && all[1].target.is_some());
    assert!(all[2].target.is_none());
}

#[test]
fn misspelled_target_is_named_with_a_did_you_mean() {
    let result = parse_en("import pyton \"math\" m");
    let error = issue_error(&result, "import_target_unknown");
    assert!(has_arg(error, "target", "pyton"));
    assert!(has_arg(error, "suggestion", "python"));
}

#[test]
fn unknown_target_far_from_every_name_has_no_suggestion() {
    let result = parse_en("import zzzzzzzz \"math\" m");
    let error = issue_error(&result, "import_target_unknown");
    assert!(!error.args.iter().any(|arg| arg.name == "suggestion"));
}

#[test]
fn target_import_requires_an_explicit_binding() {
    let result = parse_en("import python \"math\"");
    issue_error(&result, "import_target_binding_required");
}

#[test]
fn target_word_is_contextual_to_the_slot() {
    // `python` is an ordinary identifier everywhere else.
    let result = parse_en("let _ python ← 1");
    assert!(
        !result
            .errors
            .iter()
            .any(|err| has_arg(err, "issue", "import_target_unknown")),
        "{:?}",
        result.errors
    );
}

#[test]
fn latin_misspelling_gets_the_same_diagnostic() {
    let result = parse_program_la("importa goo \"fmt\" f");
    let error = issue_error(&result, "import_target_unknown");
    assert!(has_arg(error, "suggestion", "go"));
}
