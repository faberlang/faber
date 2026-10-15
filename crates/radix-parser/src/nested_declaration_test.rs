//! Parser tests for the top-level declaration rule (DECISIONS D22.1).
//!
//! Contract: a `functio` or a type declaration (`genus`, `implendum`,
//! `ordo`, `discretio`, `typus`, `schema`) inside a statement block fails at
//! parse time with PARSE010 `declaration_not_top_level`. The same declaration
//! at top level parses, and a `functio` inside a `genus` body is a method and
//! stays legal.

use crate::ParseErrorKind;
use crate::test_support::{has_arg, parse_ok_la, parse_program_la};

/// Each declaration form the rule covers, spelled as a complete top-level item.
const DECLARATIONS: &[&str] = &[
    "functio g() → numerus { redde 1 }",
    "genus Punctum { varia numerus x }",
    "implendum Mensura { functio area() → fractus }",
    "ordo Color { Ruber, Viridis }",
    "discretio Signum { Unus, Duo }",
    "typus Identitas = numerus",
    "schema Persona { columna textus nomen }",
];

/// Assert `source` fails with exactly one `declaration_not_top_level` error.
fn assert_rejected_as_nested(source: &str) {
    let result = parse_program_la(source);
    let nested = result
        .errors
        .iter()
        .filter(|err| has_arg(err, "issue", "declaration_not_top_level"))
        .collect::<Vec<_>>();
    assert_eq!(
        nested.len(),
        1,
        "expected one declaration_not_top_level error for {source:?}, got {:?}",
        result.errors
    );
    assert_eq!(nested[0].kind, ParseErrorKind::DeclarationInvalid);
}

#[test]
fn every_covered_declaration_parses_at_top_level() {
    for decl in DECLARATIONS {
        let _ = parse_ok_la(decl);
    }
}

#[test]
fn declarations_inside_a_function_body_are_rejected() {
    for decl in DECLARATIONS {
        assert_rejected_as_nested(&format!(
            "functio f() → numerus {{\n    {decl}\n    redde 1\n}}"
        ));
    }
}

#[test]
fn declarations_inside_incipit_are_rejected() {
    for decl in DECLARATIONS {
        assert_rejected_as_nested(&format!("incipit {{\n    {decl}\n}}"));
    }
}

#[test]
fn declarations_inside_a_nested_block_are_rejected() {
    assert_rejected_as_nested(
        "functio f(bivalens b) → vacuum {\n    si b {\n        functio g() → vacuum { tacet }\n    }\n}",
    );
}

#[test]
fn annotated_nested_function_is_rejected() {
    assert_rejected_as_nested(
        "incipit {\n    @ radix typus T\n    functio g(T x) → T { redde x }\n}",
    );
}

#[test]
fn method_inside_genus_stays_legal() {
    let _ = parse_ok_la(
        "genus Punctum {\n    varia numerus x\n    functio duplex() → numerus { redde 2 }\n}",
    );
}

#[test]
fn statements_after_a_rejected_declaration_still_parse() {
    let result = parse_program_la(
        "incipit {\n    functio g() → numerus { redde 1 }\n    sit numerus n ← 2\n}",
    );
    assert_eq!(result.errors.len(), 1, "errors: {:?}", result.errors);
}
