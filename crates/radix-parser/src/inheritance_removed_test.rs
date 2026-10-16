//! Clean-break contract for removed class inheritance (DECISIONS D8.1).
//!
//! `abstractus` (abstract genus modifier) and `sub` (the `extends` clause)
//! were removed, together with bodyless abstract methods. Where either word
//! appears in its old grammar position the parser reports the named migration
//! diagnostic `inheritance_removed`, not a generic parse error. Elsewhere both
//! spellings are ordinary identifiers. Contracts (`implendum` + `implet`) stay.

use crate::ParseErrorKind;
use crate::test_support::{ParseExpectation, assert_parse_error_matches, parse_ok};

#[test]
fn abstractus_genus_reports_inheritance_removed() {
    assert_parse_error_matches(
        "abstractus genus Animal {\n    functio sonus() → textus\n}",
        ParseExpectation::Args(&[("issue", "inheritance_removed"), ("keyword", "abstractus")]),
    );
    assert_parse_error_matches(
        "abstractus genus Animal {}",
        ParseExpectation::Kind(ParseErrorKind::Unexpected),
    );
}

#[test]
fn sub_clause_reports_inheritance_removed() {
    assert_parse_error_matches(
        "genus Canis sub Animal {}",
        ParseExpectation::Args(&[("issue", "inheritance_removed"), ("keyword", "sub")]),
    );
    assert_parse_error_matches(
        "genus Arca<T> sub Capsa {}",
        ParseExpectation::Args(&[("issue", "inheritance_removed"), ("keyword", "sub")]),
    );
}

#[test]
fn bodyless_genus_method_requires_a_body() {
    assert_parse_error_matches(
        "genus Animal {\n    functio sonus() → textus\n}",
        ParseExpectation::Kind(ParseErrorKind::FunctionBodyAbsent),
    );
}

#[test]
fn implet_contract_still_parses() {
    let result = parse_ok(
        "implendum Sonans {\n    functio sonus() → textus\n}\ngenus Canis implet Sonans {\n    functio sonus() → textus { redde \"latrat\" }\n}\n",
    );
    assert_eq!(result.program.expect("program").statements.len(), 2);
}

#[test]
fn abstractus_and_sub_are_ordinary_identifiers_elsewhere() {
    let bindings = parse_ok("sit _ sub ← 5\nsit _ abstractus ← sub\n");
    assert_eq!(bindings.program.expect("program").statements.len(), 2);
    let function = parse_ok("functio sub(numerus abstractus) → numerus { redde abstractus }\n");
    assert_eq!(function.program.expect("program").statements.len(), 1);
}
