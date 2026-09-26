//! Coverage for top-level statics and their compile-time evaluation
//! (DECISIONS D5.7, D5.8, D5.9, D8.4).
//!
//! Contract:
//! - a `generis` static's initializer is folded to a literal: arithmetic,
//!   comparison, `textus` concatenation, and references to other statics in
//!   dependency order, so every backend sees the constant;
//! - collection literals of constants are accepted and left structural;
//! - a non-constant initializer is SEM060 with a named issue;
//! - a cycle between statics is SEM007 `static_cycle`;
//! - arithmetic overflow and division by zero are compile errors;
//! - a legacy top-level `fixum`/`varia` is the migration warning WARN030
//!   `top_level_binding` (an error once the library sweep lands).

use crate::test_support::{analyze_source, assert_rejects_with_issue, assert_warns_with_issue};
use radix_hir::{
    HirConst, HirExpression, HirExpressionKind, HirItemKind, HirLiteral, HirStatic, HirUnOp,
};

/// Analyze `source`, assert success, and return the named static's HIR const.
fn static_const(source: &str, name: &str) -> (HirConst, radix_lexer::Interner) {
    let fixture = analyze_source(source);
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
    let hir = fixture.result.hir.as_ref().expect("hir");
    let konst = hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Constant(konst) if fixture.interner.resolve(konst.name) == name => {
                Some(konst.clone())
            }
            _ => None,
        })
        .expect("static item");
    assert!(konst.statica.is_some(), "{name} should be a static");
    (konst, fixture.interner)
}

fn int_value(expr: &HirExpression) -> i128 {
    match &expr.kind {
        HirExpressionKind::Literal(HirLiteral::Int(v)) => i128::from(*v),
        HirExpressionKind::Unary(HirUnOp::Neg, inner) => match &inner.kind {
            HirExpressionKind::Literal(HirLiteral::Int(v)) => -i128::from(*v),
            other => panic!("expected negated int literal, got {other:?}"),
        },
        other => panic!("expected folded int literal, got {other:?}"),
    }
}

#[test]
fn arithmetic_initializer_folds_to_literal() {
    let (konst, _) = static_const("generis numerus LIMES = 4 * 1024\n", "LIMES");
    assert_eq!(int_value(&konst.value), 4096);
}

#[test]
fn folding_keeps_the_authored_initializer_for_reemission() {
    let (konst, _) = static_const("generis numerus LIMES = 4 * 1024\n", "LIMES");
    let Some(HirStatic {
        authored: Some(authored),
    }) = &konst.statica
    else {
        panic!("folded static keeps its authored initializer");
    };
    assert!(matches!(authored.kind, HirExpressionKind::Binary(..)));
}

#[test]
fn literal_initializer_is_not_rewritten() {
    let (konst, _) = static_const("generis numerus N = 7\n", "N");
    assert_eq!(int_value(&konst.value), 7);
    assert_eq!(konst.statica, Some(HirStatic { authored: None }));
}

#[test]
fn negative_results_fold_to_negated_literals() {
    let (konst, _) = static_const("generis numerus N = 3 - 10\n", "N");
    assert_eq!(int_value(&konst.value), -7);
}

#[test]
fn integer_division_and_remainder_truncate() {
    let (div, _) = static_const("generis numerus D = 7 / 2\n", "D");
    assert_eq!(int_value(&div.value), 3);
    let (rem, _) = static_const("generis numerus R = (0 - 7) % 3\n", "R");
    assert_eq!(int_value(&rem.value), -1);
}

#[test]
fn references_fold_in_dependency_order() {
    // `B` is declared before the static it depends on.
    let source = "generis numerus B = A * 2 + 1\ngeneris numerus A = 4\n";
    let (konst, _) = static_const(source, "B");
    assert_eq!(int_value(&konst.value), 9);
}

#[test]
fn fractus_arithmetic_folds() {
    let (konst, _) = static_const("generis fractus F = 1.5 * 2.0\n", "F");
    assert!(
        matches!(konst.value.kind, HirExpressionKind::Literal(HirLiteral::Float(v)) if (v - 3.0).abs() < f64::EPSILON),
        "got {:?}",
        konst.value.kind
    );
}

#[test]
fn comparison_folds_to_bivalens() {
    let (konst, _) = static_const(
        "generis numerus A = 3\ngeneris bivalens MAGNUS = A > 2\n",
        "MAGNUS",
    );
    assert!(matches!(
        konst.value.kind,
        HirExpressionKind::Literal(HirLiteral::Bool(true))
    ));
}

#[test]
fn textus_concatenation_folds() {
    let (konst, interner) = static_const(
        "generis textus PRAE = \"ab\"\ngeneris textus NOMEN = PRAE + \"cd\"\n",
        "NOMEN",
    );
    let HirExpressionKind::Literal(HirLiteral::String(sym)) = konst.value.kind else {
        panic!("expected folded textus literal, got {:?}", konst.value.kind);
    };
    assert_eq!(interner.resolve(sym), "abcd");
}

#[test]
fn collection_literal_of_constants_is_accepted() {
    let (konst, _) = static_const(
        "generis numerus N = 2\ngeneris lista<numerus> XS = [1, N * 3, 4]\n",
        "XS",
    );
    let HirExpressionKind::Array(elements) = &konst.value.kind else {
        panic!("collection stays structural, got {:?}", konst.value.kind);
    };
    let radix_hir::HirArrayElement::Expr(second) = &elements[1] else {
        panic!("expected expression element");
    };
    assert_eq!(int_value(second), 6);
}

#[test]
fn call_initializer_is_not_constant() {
    assert_rejects_with_issue(
        "functio f() → numerus { redde 1 }\ngeneris numerus N = f()\n",
        "static_initializer_not_constant",
    );
}

#[test]
fn reference_to_top_level_fixum_is_not_constant() {
    assert_rejects_with_issue(
        "fixum numerus A ← 1\ngeneris numerus N = A + 1\n",
        "static_initializer_not_constant",
    );
}

#[test]
fn static_cycle_is_rejected() {
    assert_rejects_with_issue(
        "generis numerus A = B + 1\ngeneris numerus B = A + 1\n",
        "static_cycle",
    );
}

#[test]
fn self_reference_is_a_cycle() {
    assert_rejects_with_issue("generis numerus A = A + 1\n", "static_cycle");
}

#[test]
fn overflow_is_rejected() {
    assert_rejects_with_issue(
        "generis numerus N = 9223372036854775807 + 1\n",
        "static_arithmetic_overflow",
    );
}

#[test]
fn division_by_zero_is_rejected() {
    assert_rejects_with_issue("generis numerus N = 1 / 0\n", "static_division_by_zero");
}

#[test]
fn statics_are_usable_from_functions() {
    let fixture = analyze_source(
        "generis numerus LIMES = 4 * 1024\nfunctio f() → numerus { redde LIMES + 1 }\n",
    );
    assert!(fixture.is_ok(), "diagnostics: {:?}", fixture.diagnostics);
}

#[test]
fn statics_are_immutable() {
    assert_rejects_with_issue(
        "generis numerus LIMES = 4\nfunctio f() → vacuum { LIMES ← 5 }\n",
        "assignment_to_constant",
    );
}

#[test]
fn top_level_fixum_warns_with_migration_issue() {
    assert_warns_with_issue("fixum numerus N ← 4\n", "top_level_binding");
}

#[test]
fn top_level_varia_warns_with_migration_issue() {
    assert_warns_with_issue("varia numerus N ← 4\n", "top_level_binding");
}

#[test]
fn top_level_static_does_not_warn() {
    let fixture = analyze_source("generis numerus N = 4\n");
    assert!(
        fixture.diagnostics.is_empty(),
        "diagnostics: {:?}",
        fixture.diagnostics
    );
}

#[test]
fn failed_dependency_declared_later_still_reports_its_own_error() {
    // `B` evaluates `A` first as a dependency; `A` must still carry its own
    // diagnostic when the loop reaches it.
    assert_rejects_with_issue(
        "functio f() → numerus { redde 1 }\ngeneris numerus B = A + 1\ngeneris numerus A = f()\n",
        "static_initializer_not_constant",
    );
}
