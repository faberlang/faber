//! Clean-break contract for the retired allocator surface (DECISIONS D8.2).
//!
//! `cura` (allocator scope statement) and `curata` (allocator-requirement
//! function modifier) were removed. Where either word appears in its old
//! grammar position the parser reports the named migration diagnostic
//! `allocator_surface_removed`, not a generic parse error. Elsewhere both
//! spellings are ordinary identifiers.

use crate::ParseErrorKind;
use crate::test_support::{ParseExpectation, assert_parse_error_matches, parse_ok};

#[test]
fn cura_statement_reports_allocator_surface_removed() {
    assert_parse_error_matches(
        r#"cura "arena" fixum _ alloc {}"#,
        ParseExpectation::Args(&[("issue", "allocator_surface_removed"), ("keyword", "cura")]),
    );
    assert_parse_error_matches(
        r#"cura "arena" fixum _ alloc {}"#,
        ParseExpectation::Kind(ParseErrorKind::Unexpected),
    );
}

#[test]
fn nested_cura_statement_reports_allocator_surface_removed() {
    assert_parse_error_matches(
        "functio f() → numerus {\n    cura \"page\" fixum _ res { redde 1 } cape err { redde 0 }\n    redde 3\n}",
        ParseExpectation::Args(&[("issue", "allocator_surface_removed"), ("keyword", "cura")]),
    );
}

#[test]
fn curata_modifier_reports_allocator_surface_removed() {
    assert_parse_error_matches(
        r"functio greet(textus name) curata alloc ut a → textus { redde name }",
        ParseExpectation::Args(&[
            ("issue", "allocator_surface_removed"),
            ("keyword", "curata"),
        ]),
    );
}

#[test]
fn cura_and_curata_are_ordinary_identifiers_elsewhere() {
    let bindings = parse_ok("sit cura ← 5\nsit curata ← cura\n");
    assert_eq!(bindings.program.expect("program").statements.len(), 2);
    let function = parse_ok("functio cura(numerus curata) → numerus { redde curata }\n");
    assert_eq!(function.program.expect("program").statements.len(), 1);
}
