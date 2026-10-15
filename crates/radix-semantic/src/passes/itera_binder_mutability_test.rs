//! IB-1: the authored iteration-binder mutability (`varia` / `sit`) is
//! carried on every HIR loop node that introduces a binder: `Itera`,
//! `IteraLockstep`, `IteraRange` and `Reduction`.

use crate::test_support::{self, AnalyzedFixture};
use radix_hir::{HirExpressionKind, HirItemKind, HirStatementKind};

/// The binder-mutability flag of the first statement of the first function,
/// paired with the node name so a wrong-node match fails loudly.
fn first_loop_flag(unit: &AnalyzedFixture) -> (&'static str, bool) {
    let body = unit
        .hir
        .as_ref()
        .expect("hir")
        .items
        .iter()
        .find_map(|item| match &item.kind {
            HirItemKind::Function(function) => function.body.as_ref(),
            _ => None,
        })
        .expect("function body");
    let HirStatementKind::Expr(expr) = &body.statements[0].kind else {
        panic!(
            "expected expression statement, got: {:?}",
            body.statements[0]
        );
    };
    match &expr.kind {
        HirExpressionKind::Itera { mutable, .. } => ("Itera", *mutable),
        HirExpressionKind::IteraLockstep { mutable, .. } => ("IteraLockstep", *mutable),
        HirExpressionKind::IteraRange { mutable, .. } => ("IteraRange", *mutable),
        HirExpressionKind::Reduction { mutable, .. } => ("Reduction", *mutable),
        other => panic!("expected a loop node, got: {other:?}"),
    }
}

fn assert_flag(source: &str, node: &'static str, mutable: bool) {
    let unit = test_support::analyze_ok(source);
    assert_eq!(first_loop_flag(&unit), (node, mutable));
}

#[test]
fn itera_carries_binder_mutability() {
    for (modifier, mutable) in [("varia", true), ("sit", false)] {
        assert_flag(
            &format!(
                "functio f(lista<numerus> xs) → vacuum {{ itera ex xs {modifier} x {{ nota x }} }}"
            ),
            "Itera",
            mutable,
        );
    }
}

#[test]
fn itera_lockstep_carries_binder_mutability() {
    for (modifier, mutable) in [("varia", true), ("sit", false)] {
        assert_flag(
            &format!(
                "functio f(lista<numerus> xs, lista<numerus> ys) → vacuum {{ \
                 itera ex xs, ys {modifier} x, y {{ nota x }} }}"
            ),
            "IteraLockstep",
            mutable,
        );
    }
}

#[test]
fn itera_range_carries_binder_mutability() {
    for (modifier, mutable) in [("varia", true), ("sit", false)] {
        assert_flag(
            &format!(
                "functio f(numerus m, numerus n) → vacuum {{ \
                 itera ab 0‥m, 0‥n {modifier} i, j {{ nota i }} }}"
            ),
            "IteraRange",
            mutable,
        );
    }
}

#[test]
fn summa_carries_binder_mutability() {
    for (modifier, mutable) in [("varia", true), ("sit", false)] {
        assert_flag(
            &format!(
                "functio t(tensor<f32, [8]> a) → vacuum {{ \
                 reducta via summa ex a apud [i] {modifier} s {{ redde s }} }}"
            ),
            "Reduction",
            mutable,
        );
    }
}
