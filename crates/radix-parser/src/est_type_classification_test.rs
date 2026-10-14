//! `est` / `non est` right-hand-side shape (DECISIONS D23.1, D23.5).
//!
//! The right-hand side is always a type: the parser builds a `TypeCheck` node
//! for every `est` / `non est` and never guesses type-vs-value. A name that
//! turns out to be a value is rejected in semantic analysis, where the imported
//! file's interface is available.

use crate::ParseResult;
use crate::test_support::parse_program_la;
use radix_syntax::{ExprKind, StmtKind, TypeExprKind};

fn parse_source(source: &str) -> ParseResult {
    parse_program_la(source)
}

/// The RHS classification of the single `redde <test>` in the source.
fn est_rhs_kind(result: &ParseResult) -> Option<&'static str> {
    let program = result.program.as_ref()?;
    for stmt in &program.statements {
        let StmtKind::Func(func) = &stmt.kind else {
            continue;
        };
        let body = func.body.as_ref()?;
        for stmt in &body.statements {
            let StmtKind::Redde(redde) = &stmt.kind else {
                continue;
            };
            let value = redde.value.as_deref()?;
            return match &value.kind {
                ExprKind::TypeCheck(_) => Some("type_check"),
                ExprKind::Binary(binary)
                    if matches!(
                        binary.op,
                        radix_syntax::BinOp::Is | radix_syntax::BinOp::IsNot
                    ) =>
                {
                    Some("value_is")
                }
                _ => None,
            };
        }
    }
    None
}

fn assert_type_check(source: &str) {
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);
    assert_eq!(est_rhs_kind(&result), Some("type_check"), "{source}");
}

#[test]
fn est_declared_genus_is_a_type_test() {
    assert_type_check(concat!(
        "genus Errata {\n    varia textus nuntius\n}\n\n",
        "functio f(Errata ∪ nihil r) → bivalens { redde r est Errata }",
    ));
}

#[test]
fn est_declared_type_before_its_declaration_is_a_type_test() {
    // Order-independent: a body referencing a genus declared later in the
    // file still builds a type test.
    assert_type_check(concat!(
        "functio f(Posterius ∪ nihil r) → bivalens { redde r est Posterius }\n\n",
        "genus Posterius {\n    varia textus nuntius\n}",
    ));
}

#[test]
fn est_declared_implendum_ordo_typus_are_type_tests() {
    assert_type_check(concat!(
        "implendum Salutat {\n    functio saluta() → textus\n}\n\n",
        "functio f(Salutat ∪ nihil r) → bivalens { redde r est Salutat }",
    ));
    assert_type_check(concat!(
        "ordo Color {\n    Ruber, Caeruleus\n}\n\n",
        "functio f(Color ∪ nihil r) → bivalens { redde r est Color }",
    ));
    assert_type_check(
        "typus Par = numerus ∪ textus\n\nfunctio f(Par ∪ nihil r) → bivalens { redde r est Par }",
    );
}

#[test]
fn est_primitive_is_a_type_test() {
    assert_type_check("functio f(numerus ∪ textus u) → bivalens { redde u est numerus }");
}

#[test]
fn est_nihil_is_a_type_test_on_the_null_type() {
    // `nihil` is the null type, so the right-hand side is a type; nil
    // narrowing is preserved through the TypeCheck path (D23.1).
    let result = parse_source("functio f(textus ∪ nihil x) → bivalens { redde x est nihil }");
    assert!(result.success(), "{:?}", result.errors);
    assert_eq!(est_rhs_kind(&result), Some("type_check"));
    let program = result.program.as_ref().unwrap();
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let StmtKind::Redde(redde) = &func.body.as_ref().unwrap().statements[0].kind else {
        panic!("expected redde");
    };
    let ExprKind::TypeCheck(check) = &redde.value.as_deref().unwrap().kind else {
        panic!("expected TypeCheck");
    };
    assert!(matches!(check.ty.kind, TypeExprKind::Named(..)));
}

#[test]
fn non_est_is_a_negative_type_test() {
    let result = parse_source(
        "genus Errata {\n    varia textus nuntius\n}\n\n\
         functio f(Errata ∪ nihil r) → bivalens { redde r non est Errata }",
    );
    assert!(result.success(), "{:?}", result.errors);
    let program = result.program.as_ref().unwrap();
    let StmtKind::Func(func) = &program.statements[1].kind else {
        panic!("expected function");
    };
    let StmtKind::Redde(redde) = &func.body.as_ref().unwrap().statements[0].kind else {
        panic!("expected redde");
    };
    let ExprKind::TypeCheck(check) = &redde.value.as_deref().unwrap().kind else {
        panic!("expected TypeCheck");
    };
    assert!(!check.positive);
}

#[test]
fn est_value_name_is_still_parsed_as_a_type() {
    // The parser does not guess: an identifier that names no type still parses
    // as a type. The semantic pass rejects the value target by name (D23.1).
    assert_type_check("functio f(numerus x, numerus aliud) → bivalens { redde x est aliud }");
}

#[test]
fn est_qualified_imported_name_is_a_type_test() {
    // D18.1 interim routing sent `x est aux.Felis` down the value path; D23.5
    // removes the lookahead, so the qualified name parses as a type.
    assert_type_check(
        "importa ex \"./aux\" aux\n\n\
         functio f(aux.Felis ∪ nihil x) → bivalens { redde x est aux.Felis }",
    );
}

#[test]
fn est_value_literal_is_an_error() {
    // A literal is not type syntax; the right-hand side must be a type.
    let result = parse_source("incipit { si 1 est 1 { nota \"ok\" } }");
    assert!(!result.success(), "literal RHS must not parse as a type");
}

#[test]
fn declared_type_name_is_not_claimed_in_other_positions() {
    // Ordinary construction use of a declared type name, and the return
    // annotation's named type, parse unchanged.
    let result = parse_source(concat!(
        "genus Punctum {\n    varia numerus x\n}\n\n",
        "functio f() → Punctum { redde Punctum { x = 1 } }",
    ));
    assert!(result.success(), "{:?}", result.errors);
    let program = result.program.as_ref().unwrap();
    let StmtKind::Func(func) = &program.statements[1].kind else {
        panic!("expected function");
    };
    let ret = func.ret.as_ref().expect("return annotation");
    assert!(matches!(ret.kind, TypeExprKind::Named(..)));
    let body = func.body.as_ref().unwrap();
    let StmtKind::Redde(redde) = &body.statements[0].kind else {
        panic!("expected redde");
    };
    let value = redde.value.as_deref().expect("redde value");
    // Typed construction lowers as a `verte` conversion of the object
    // literal — the declared name is consumed by the ordinary type grammar.
    assert!(matches!(value.kind, ExprKind::Verte(_)));
}
