//! est-RHS classification tests (type-algebra U1R).
//!
//! `est <name>` routes to a `TypeCheck` node only when the name declares a
//! type visible to the parser: the primitive spellings plus — since U1R —
//! identifiers this file declares as nominal types (`genus`, `implendum`,
//! `ordo`, `discretio`, `typus`), order-independently through one pre-parse
//! token scan. A name that declares no type keeps `Binary(Is)`
//! value-equality classification, which is why an imported type stays a
//! value test at this layer (open row 8's documented miss).

use crate::{parse, ParseResult};
use radix_lexer::lex;
use radix_syntax::{BinOp, ExprKind, StmtKind, TypeExprKind};
fn parse_source(source: &str) -> ParseResult {
    parse(lex(source))
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
                ExprKind::Binary(binary) if matches!(binary.op, BinOp::Is | BinOp::IsNot) => {
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

fn assert_value_equality(source: &str) {
    let result = parse_source(source);
    assert!(result.success(), "{:?}", result.errors);
    assert_eq!(est_rhs_kind(&result), Some("value_is"), "{source}");
}

#[test]
fn est_declared_genus_classifies_as_type_check() {
    assert_type_check(concat!(
        "genus Errata {\n    textus nuntius\n}\n\n",
        "functio f(Errata ∪ nihil r) → bivalens { redde r est Errata }",
    ));
}

#[test]
fn est_declared_type_classifies_before_its_declaration() {
    // The declared-type scan is order-independent: a body referencing a
    // genus declared later in the file still routes to `TypeCheck`.
    assert_type_check(concat!(
        "functio f(Posterius ∪ nihil r) → bivalens { redde r est Posterius }\n\n",
        "genus Posterius {\n    textus nuntius\n}",
    ));
}

#[test]
fn est_declared_implendum_ordo_typus_classify() {
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
fn est_value_name_stays_value_equality() {
    // `aliud` declares no type, so the test stays `Binary(Is)` value
    // equality — the classification must not guess from casing.
    assert_value_equality("functio f(numerus x, numerus aliud) → bivalens { redde x est aliud }");
}

#[test]
fn est_primitive_stays_type_check() {
    assert_type_check("functio f(numerus ∪ textus u) → bivalens { redde u est numerus }");
}

#[test]
fn est_nihil_stays_binary_value_form() {
    // `est nihil` keeps the nn-u1 binary form: the null check refines
    // through the literal probe, not a TypeCheck node.
    let result = parse_source("functio f(textus ∪ nihil x) → bivalens { redde x est nihil }");
    assert!(result.success(), "{:?}", result.errors);
    assert_eq!(est_rhs_kind(&result), Some("value_is"));
}

#[test]
fn declared_type_name_is_not_claimed_in_other_positions() {
    // The scan only feeds the est classifier: an ordinary construction use
    // of a declared type name, and the return annotation's named type,
    // parse unchanged.
    let result = parse_source(concat!(
        "genus Punctum {\n    numerus x\n}\n\n",
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
    // literal — the declared name is consumed by the ordinary type grammar,
    // not the est classifier.
    assert!(matches!(value.kind, ExprKind::Verte(_)));
}
