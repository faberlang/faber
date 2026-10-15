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
    sit iuncta<gx: f32, numerus> p ← iuncta<f32, numerus> [a, b]
    redde p.gx
}
";
    let _result = analyze(source);
}

#[test]
fn iuncta_label_member_lowers_to_the_same_index_as_bracket() {
    let labeled = r"
functio first(f32 a, numerus b) → f32 {
    sit iuncta<gx: f32, numerus> p ← iuncta<f32, numerus> [a, b]
    redde p.gx
}
";
    let indexed = r"
functio first(f32 a, numerus b) → f32 {
    sit iuncta<gx: f32, numerus> p ← iuncta<f32, numerus> [a, b]
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
    sit iuncta<gx: f32, numerus> p ← iuncta<f32, numerus> [a, b]
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
    sit iuncta<gx: f32, numerus> p ← iuncta<f32, numerus> [a, b]
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
    sit iuncta<f32, numerus> p ← iuncta<f32, numerus> [a, b]
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
    sit iuncta<gx: f32, numerus> labeled ← iuncta<f32, numerus> [a, b]
    sit iuncta<f32, numerus> plain ← labeled
    redde labeled ≡ plain
}
";
    let _result = analyze(source);
}

#[test]
fn iuncta_keyword_label_member_access_typechecks() {
    let source = r"
functio first(f32 a) → f32 {
    sit iuncta<fixum: f32> p ← iuncta<f32> [a]
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

fn function_locals(source: &str) -> Vec<(String, HirExpressionKind)> {
    let unit = analyze(source);
    let hir = unit.hir.as_ref().expect("hir");
    let item = hir.items.first().expect("function item");
    let HirItemKind::Function(func) = &item.kind else {
        panic!("expected function item");
    };
    func.body
        .as_ref()
        .expect("function body")
        .statements
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            HirStatementKind::Local(local) => Some((
                unit.interner.resolve(local.name).to_owned(),
                local.init.as_ref()?.kind.clone(),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn iuncta_label_destructuring_lowers_fields_by_position() {
    let locals = function_locals(
        r"
functio split(iuncta<gx: f32, gw: numerus> pair) → f32 {
    fixum {gw, gx} ← pair
    redde gx
}
",
    );
    assert!(matches!(
        locals.iter().find(|(name, _)| name == "gx").map(|(_, kind)| kind),
        Some(HirExpressionKind::Index(_, index))
            if matches!(index.kind, HirExpressionKind::Literal(HirLiteral::Int(0)))
    ));
    assert!(matches!(
        locals.iter().find(|(name, _)| name == "gw").map(|(_, kind)| kind),
        Some(HirExpressionKind::Index(_, index))
            if matches!(index.kind, HirExpressionKind::Literal(HirLiteral::Int(1)))
    ));
}

#[test]
fn iuncta_label_destructuring_allows_partial_binding() {
    let _locals = function_locals(
        r"
functio first(iuncta<gx: f32, gw: numerus> pair) → f32 {
    fixum {gx} ← pair
    redde gx
}
",
    );
}

#[test]
fn iuncta_label_destructuring_rest_is_the_unbound_subtuple() {
    let locals = function_locals(
        r"
functio second(iuncta<gx: f32, gw: numerus> pair) → numerus {
    fixum {gx, ceteri tail} ← pair
    redde tail[0]
}
",
    );
    let (_, tail) = locals
        .iter()
        .find(|(name, _)| name == "tail")
        .expect("tail local");
    let HirExpressionKind::Tuple(items, None) = tail else {
        panic!("expected tail to be a tuple, got {tail:?}");
    };
    assert_eq!(items.len(), 1);
    assert!(matches!(
        items[0].kind,
        HirExpressionKind::Index(_, ref index)
            if matches!(index.kind, HirExpressionKind::Literal(HirLiteral::Int(1)))
    ));
}

#[test]
fn iuncta_label_destructuring_unknown_label_is_rejected() {
    test_support::assert_rejects_with_issue(
        r"
functio bad(iuncta<gx: f32, gw: numerus> pair) → f32 {
    fixum {other} ← pair
    redde other
}
",
        "unknown_tuple_element_label",
    );
}

#[test]
fn iuncta_label_destructuring_unlabeled_tuple_is_rejected() {
    test_support::assert_rejects_with_issue(
        r"
functio bad(iuncta<f32, numerus> pair) → f32 {
    fixum {gx} ← pair
    redde gx
}
",
        "unknown_tuple_element_label",
    );
}

#[test]
fn iuncta_label_destructuring_mixed_pattern_stays_rejected() {
    let result = test_support::analyze_source(
        r"
functio bad(iuncta<gx: f32, gw: numerus> pair) → f32 {
    fixum [gx, {gw}] ← pair
    redde gx
}
",
    );
    assert!(!result.is_ok(), "expected mixed pattern rejection");
}
