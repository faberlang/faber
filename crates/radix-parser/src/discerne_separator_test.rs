//! Parser tests for the `discerne` header and arm separators (M1-1).
//!
//! Contract: `et` is the only separator between subjects and between arm
//! patterns; a comma in either position is a named migration diagnostic
//! (`discerne_subject_comma_retired`, `discerne_pattern_comma_retired`), a
//! subject is a name or member path (`discerne_subject_not_path` otherwise),
//! and the exhaustive marker is a keyword only where a subject path follows
//! it, so a binding named like the marker can be matched on. Latin sources
//! use `et`/`omnia`; the English cases map the same identities to
//! `and`/`all`. Assertions pin `issue` and structure, never prose.

use crate::test_support::{has_arg, parse_ok, parse_program, symbol_name};
use crate::{ParseResult, parse};
use radix_lexer::{KeywordPack, TokenKind, lex_with_locale_pack};
use radix_syntax::{Expr, ExprKind, Pattern, PatternBind, StmtKind};

/// English surface for the words these tests use.
struct EnDiscernePack;

impl KeywordPack for EnDiscernePack {
    fn pack_id(&self) -> &'static str {
        "en-discerne-test"
    }

    fn keyword_token_kind(&self, _text: &str) -> Option<TokenKind> {
        None
    }

    fn keyword_latin_key(&self, text: &str) -> Option<&str> {
        match text {
            "match" => Some("discerne"),
            "all" => Some("omnia"),
            "case" => Some("casu"),
            "and" => Some("et"),
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
    parse(lex_with_locale_pack(source, &EnDiscernePack))
}

fn issues(result: &ParseResult) -> Vec<String> {
    result
        .errors
        .iter()
        .filter_map(|err| {
            err.args
                .iter()
                .find(|arg| arg.name == "issue")
                .map(|arg| arg.value.clone())
        })
        .collect()
}

fn only_issue(result: &ParseResult, issue: &str) {
    assert_eq!(
        result.errors.len(),
        1,
        "expected exactly one error ({issue}), got {:?}",
        result.errors
    );
    assert!(has_arg(&result.errors[0], "issue", issue));
}

fn discerne_of(result: &ParseResult) -> &radix_syntax::DiscerneStmt {
    let program = result.program.as_ref().expect("program");
    let StmtKind::Discerne(stmt) = &program.statements[0].kind else {
        panic!("expected discerne statement");
    };
    stmt
}

fn subject_names<'a>(result: &'a ParseResult, subjects: &[Expr]) -> Vec<&'a str> {
    subjects
        .iter()
        .map(|subject| match &subject.kind {
            ExprKind::Ident(ident) => symbol_name(result, ident.name),
            other => panic!("expected an identifier subject, got {other:?}"),
        })
        .collect()
}

#[test]
fn et_separates_header_subjects_and_arm_patterns() {
    let result = parse_ok("discerne a et b { casu x et y { } casu _ et _ { } }");
    let stmt = discerne_of(&result);
    assert!(!stmt.exhaustive);
    assert_eq!(subject_names(&result, &stmt.subjects), ["a", "b"]);
    assert_eq!(stmt.arms.len(), 2);
    assert!(stmt.arms.iter().all(|arm| arm.patterns.len() == 2));
}

#[test]
fn header_comma_is_one_named_diagnostic_and_recovers_as_et() {
    let result = parse_program("discerne a, b { casu x et y { } }");
    only_issue(&result, "discerne_subject_comma_retired");
    let stmt = discerne_of(&result);
    assert_eq!(subject_names(&result, &stmt.subjects), ["a", "b"]);
    assert_eq!(stmt.arms[0].patterns.len(), 2);
}

#[test]
fn arm_comma_is_one_named_diagnostic_and_recovers_as_et() {
    let result = parse_program("discerne a et b { casu x, y { } }");
    only_issue(&result, "discerne_pattern_comma_retired");
    assert_eq!(discerne_of(&result).arms[0].patterns.len(), 2);
}

#[test]
fn each_comma_diagnostic_is_reported_once_per_position() {
    let result = parse_program("discerne a, b, c { casu x, y, z { } }");
    assert_eq!(
        issues(&result),
        [
            "discerne_subject_comma_retired",
            "discerne_pattern_comma_retired"
        ]
    );
    let stmt = discerne_of(&result);
    assert_eq!(stmt.subjects.len(), 3);
    assert_eq!(stmt.arms[0].patterns.len(), 3);
}

#[test]
fn comma_after_a_binding_list_stays_a_binding_comma() {
    let result = parse_ok("discerne a { casu Circulus sit r, Quadratum { } }");
    let patterns = &discerne_of(&result).arms[0].patterns;
    assert_eq!(patterns.len(), 1);
    let Pattern::Ident(_, Some(PatternBind::Bindings { bindings, .. })) = &patterns[0] else {
        panic!("expected a binding-list pattern, got {:?}", patterns[0]);
    };
    assert_eq!(bindings.len(), 2);
}

#[test]
fn comma_before_a_name_and_binding_introducer_is_the_retired_pattern_comma() {
    let result = parse_program("discerne a { casu Circulus sit r, Quadratum sit latus { } }");
    only_issue(&result, "discerne_pattern_comma_retired");
    let patterns = &discerne_of(&result).arms[0].patterns;
    assert_eq!(patterns.len(), 2);
    for pattern in patterns {
        let Pattern::Ident(_, Some(PatternBind::Bindings { bindings, .. })) = pattern else {
            panic!("expected a binding-list pattern, got {pattern:?}");
        };
        assert_eq!(bindings.len(), 1);
    }
}

#[test]
fn subjects_are_names_and_member_paths() {
    let result = parse_ok("discerne ego.status { casu _ { } }");
    let subjects = &discerne_of(&result).subjects;
    let ExprKind::Member(member) = &subjects[0].kind else {
        panic!("expected member subject");
    };
    assert!(matches!(member.object.kind, ExprKind::Ego(_)));

    let result = parse_ok("discerne a.b.c { casu _ { } }");
    let subjects = &discerne_of(&result).subjects;
    let ExprKind::Member(outer) = &subjects[0].kind else {
        panic!("expected member subject");
    };
    assert!(matches!(outer.object.kind, ExprKind::Member(_)));
    assert_eq!(symbol_name(&result, outer.member.name), "c");
}

#[test]
fn a_subject_that_is_not_a_path_is_one_named_diagnostic() {
    for header in ["f(x)", "5", "(a)", "a + b", "verum"] {
        let result = parse_program(&format!("discerne {header} {{ casu _ {{ }} }}"));
        only_issue(&result, "discerne_subject_not_path");
        assert_eq!(discerne_of(&result).subjects.len(), 1, "{header}");
    }
}

#[test]
fn a_non_path_subject_before_et_still_reports_once_without_a_cascade() {
    // Recovery reads the whole header as one expression (`et` is also the
    // logical operator there), so the report stays a single named error.
    let result = parse_program("discerne f(x) et b { casu _ et _ { } }");
    only_issue(&result, "discerne_subject_not_path");
}

#[test]
fn omnia_is_the_exhaustive_marker_only_before_a_subject_path() {
    let result = parse_ok("discerne omnia a et b { casu x et y { } }");
    let stmt = discerne_of(&result);
    assert!(stmt.exhaustive);
    assert_eq!(subject_names(&result, &stmt.subjects), ["a", "b"]);
}

#[test]
fn omnia_before_a_brace_or_separator_is_the_first_subject() {
    let result = parse_ok("discerne omnia { casu _ { } }");
    let stmt = discerne_of(&result);
    assert!(!stmt.exhaustive);
    assert_eq!(subject_names(&result, &stmt.subjects), ["omnia"]);

    let result = parse_ok("discerne omnia et b { casu x et y { } }");
    let stmt = discerne_of(&result);
    assert!(!stmt.exhaustive);
    assert_eq!(subject_names(&result, &stmt.subjects), ["omnia", "b"]);

    let result = parse_ok("discerne omnia.estado { casu _ { } }");
    let stmt = discerne_of(&result);
    assert!(!stmt.exhaustive);
    assert!(matches!(stmt.subjects[0].kind, ExprKind::Member(_)));
}

#[test]
fn english_all_binding_is_the_subject_and_the_keyword_is_kept_before_paths() {
    let result = parse_en("match all { case _ { } }");
    assert!(result.success(), "{:?}", result.errors);
    let stmt = discerne_of(&result);
    assert!(!stmt.exhaustive);
    assert_eq!(subject_names(&result, &stmt.subjects), ["all"]);

    let result = parse_en("match all all { case _ { } }");
    assert!(result.success(), "{:?}", result.errors);
    let stmt = discerne_of(&result);
    assert!(stmt.exhaustive);
    assert_eq!(subject_names(&result, &stmt.subjects), ["all"]);

    let result = parse_en("match all a and b { case x and y { } }");
    assert!(result.success(), "{:?}", result.errors);
    let stmt = discerne_of(&result);
    assert!(stmt.exhaustive);
    assert_eq!(subject_names(&result, &stmt.subjects), ["a", "b"]);

    let result = parse_en("match all and b { case x and y { } }");
    assert!(result.success(), "{:?}", result.errors);
    let stmt = discerne_of(&result);
    assert!(!stmt.exhaustive);
    assert_eq!(subject_names(&result, &stmt.subjects), ["all", "b"]);
}

#[test]
fn english_comma_is_the_named_migration_diagnostic() {
    let result = parse_en("match a, b { case x, y { } }");
    assert_eq!(
        issues(&result),
        [
            "discerne_subject_comma_retired",
            "discerne_pattern_comma_retired"
        ]
    );
}

#[test]
fn omnia_stays_a_legal_binding_name_elsewhere() {
    let result = parse_ok("sit _ omnia ← 1");
    assert_eq!(
        result.program.as_ref().expect("program").statements.len(),
        1
    );
}
