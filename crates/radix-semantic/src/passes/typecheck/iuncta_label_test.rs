//! Pins for identity-erased `iuncta` labels and `.gx` member access.

use crate::test_support;
use radix_hir::{HirExpressionKind, HirItemKind, HirLiteral, HirStatementKind};

fn analyze(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_ok(source)
}

fn returned_expr_kind(source: &str) -> HirExpressionKind {
    let unit = analyze(source);
    assert!(unit.is_ok(), "diagnostics: {:?}", unit.diagnostics);
    let hir = unit.hir.as_ref().expect("hir");
    let item = hir.items.first().expect("function item");
    let HirItemKind::Function(func) = &item.kind else {
        panic!("expected function item");
    };
    let body = func.body.as_ref().expect("function body");
    body.statements
        .iter()
        .rev()
        .find_map(|stmt| match &stmt.kind {
            HirStatementKind::Redde(Some(expr)) => Some(expr.kind.clone()),
            _ => None,
        })
        .expect("return expression")
}

fn is_constant_index(kind: &HirExpressionKind, index: u64) -> bool {
    matches!(
        kind,
        HirExpressionKind::Index(_, idx)
            if matches!(idx.kind, HirExpressionKind::Literal(HirLiteral::Int(n)) if n == index)
    )
}

#[test]
fn iuncta_label_member_access_typechecks() {
    let source = r"
functio first(f32 a, numerus b) → f32 {
    fixum iuncta<gx: f32, numerus> p ← iuncta<f32, numerus> [a, b]
    redde p.gx
}
";
    let _result = analyze(source);
}

#[test]
fn iuncta_label_member_lowers_to_the_same_index_as_bracket() {
    let labeled = r"
functio first(f32 a, numerus b) → f32 {
    fixum iuncta<gx: f32, numerus> p ← iuncta<f32, numerus> [a, b]
    redde p.gx
}
";
    let indexed = r"
functio first(f32 a, numerus b) → f32 {
    fixum iuncta<gx: f32, numerus> p ← iuncta<f32, numerus> [a, b]
    redde p[0]
}
";
    let labeled_kind = returned_expr_kind(labeled);
    let indexed_kind = returned_expr_kind(indexed);
    assert!(
        is_constant_index(&labeled_kind, 0),
        "expected p.gx to lower to Index of literal 0, got {labeled_kind:?}"
    );
    assert!(
        is_constant_index(&indexed_kind, 0),
        "expected p[0] to lower to Index of literal 0, got {indexed_kind:?}"
    );
    match (&labeled_kind, &indexed_kind) {
        (HirExpressionKind::Index(_, labeled_idx), HirExpressionKind::Index(_, indexed_idx)) => {
            assert_eq!(labeled_idx.kind, indexed_idx.kind);
        }
        _ => panic!("both returns must be Index nodes"),
    }
}

#[test]
fn iuncta_label_bracket_access_stays_legal() {
    let source = r"
functio first(f32 a, numerus b) → f32 {
    fixum iuncta<gx: f32, numerus> p ← iuncta<f32, numerus> [a, b]
    redde p[0]
}
";
    let _result = analyze(source);
}

#[test]
fn iuncta_unknown_label_member_is_rejected() {
    test_support::assert_rejects_with_issue(
        r"
functio bad(f32 a, numerus b) → f32 {
    fixum iuncta<gx: f32, numerus> p ← iuncta<f32, numerus> [a, b]
    redde p.other
}
",
        "unknown_tuple_element_label",
    );
}

#[test]
fn iuncta_unlabeled_member_access_is_rejected() {
    test_support::assert_rejects_with_issue(
        r"
functio bad(f32 a, numerus b) → f32 {
    fixum iuncta<f32, numerus> p ← iuncta<f32, numerus> [a, b]
    redde p.gx
}
",
        "unknown_tuple_element_label",
    );
}

#[test]
fn iuncta_labeled_and_unlabeled_types_unify() {
    let source = r"
functio same(f32 a, numerus b) → bivalens {
    fixum iuncta<gx: f32, numerus> labeled ← iuncta<f32, numerus> [a, b]
    fixum iuncta<f32, numerus> plain ← labeled
    redde labeled ≡ plain
}
";
    let _result = analyze(source);
}

#[test]
fn iuncta_keyword_label_member_access_typechecks() {
    let source = r"
functio first(f32 a) → f32 {
    fixum iuncta<fixum: f32> p ← iuncta<f32> [a]
    redde p.fixum
}
";
    let _result = analyze(source);
}

#[test]
fn iuncta_param_label_member_access_typechecks() {
    let source = r"
functio first(iuncta<gx: f32, numerus> p) → f32 {
    redde p.gx
}
";
    let _result = analyze(source);
}
