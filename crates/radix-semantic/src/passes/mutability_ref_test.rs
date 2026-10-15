//! MU-2 `ref` deep-read-only pins.
//!
//! Catalog-keyed mutating-receiver bans, projection assignment, pass into
//! `mut`/`own`, and genus-field transitivity. Unmarked code stays on the
//! MU-1 baseline tests.

use crate::test_support::{analyze_ok_la, assert_rejects_with_issue_la};
use crate::{
    ISSUE_ASSIGN_THROUGH_REF, ISSUE_MUTATING_RECEIVER_THROUGH_REF,
    ISSUE_REF_TO_MUT_OR_OWN_POSITION, SemanticMutability, Type,
};

#[test]
fn ref_list_read_methods_are_legal() {
    let _ = analyze_ok_la(
        r"
incipit {
    fixum de lista<numerus> xs ← [1, 2, 3]
    nota xs.longitudo()
}
",
    );
}

#[test]
fn ref_list_mutating_receiver_rejects() {
    assert_rejects_with_issue_la(
        r"
incipit {
    fixum de lista<numerus> xs ← [1, 2, 3]
    xs.appende(4)
}
",
        ISSUE_MUTATING_RECEIVER_THROUGH_REF,
    );
}

#[test]
fn unmarked_list_mutating_receiver_stays_legal() {
    let _ = analyze_ok_la(
        r"
incipit {
    varia lista<numerus> xs ← vacua
    xs.appende(4)
}
",
    );
}

#[test]
fn projection_assignment_through_ref_rejects() {
    assert_rejects_with_issue_la(
        r"
incipit {
    fixum de lista<numerus> xs ← [1, 2, 3]
    xs[0] ← 9
}
",
        ISSUE_ASSIGN_THROUGH_REF,
    );
}

#[test]
fn unmarked_projection_assignment_stays_legal() {
    let _ = analyze_ok_la(
        r"
incipit {
    varia lista<numerus> xs ← [1, 2, 3]
    xs[0] ← 9
}
",
    );
}

#[test]
fn ref_to_mut_position_rejects() {
    assert_rejects_with_issue_la(
        r"
functio poke(in lista<numerus> xs) → vacuum {
    xs.appende(4)
}
incipit {
    fixum de lista<numerus> a ← [1, 2, 3]
    poke(in a)
}
",
        ISSUE_REF_TO_MUT_OR_OWN_POSITION,
    );
}

#[test]
fn ref_to_own_position_rejects() {
    assert_rejects_with_issue_la(
        r"
functio take(own lista<numerus> xs) → vacuum {
    nota xs.longitudo()
}
incipit {
    fixum de lista<numerus> a ← [1, 2, 3]
    take(own a)
}
",
        ISSUE_REF_TO_MUT_OR_OWN_POSITION,
    );
}

#[test]
fn unmarked_into_mut_param_stays_legal() {
    let _ = analyze_ok_la(
        r"
functio poke(in lista<numerus> xs) → vacuum {
    xs.appende(4)
}
incipit {
    varia lista<numerus> a ← [1, 2, 3]
    poke(in a)
}
",
    );
}

#[test]
fn genus_field_projection_is_a_ref_view() {
    let result = analyze_ok_la(
        r"
genus Persona {
    varia lista<numerus> amici
}
incipit {
    fixum de Persona p ← Persona { amici = [1, 2] }
    nota p.amici
}
",
    );
    let entry = result.hir.as_ref().unwrap().entry.as_ref().unwrap();
    let field_ty = entry.statements.iter().find_map(|stmt| match &stmt.kind {
        radix_hir::HirStatementKind::Expr(expr) => field_expr_ty(expr),
        _ => None,
    });
    let field_ty = field_ty.expect("nota p.amici field type");
    assert!(
        matches!(
            result.types.get(field_ty),
            Type::Ref(SemanticMutability::Immutable, _)
        ),
        "field projection of a ref genus must be a ref view, got {:?}",
        result.types.get(field_ty)
    );
}

fn field_expr_ty(expr: &radix_hir::HirExpression) -> Option<crate::TypeId> {
    match &expr.kind {
        radix_hir::HirExpressionKind::Field(_, _) => expr.ty,
        radix_hir::HirExpressionKind::Call(_, _, args, _) => {
            args.iter().find_map(|arg| field_expr_ty(&arg.expr))
        }
        radix_hir::HirExpressionKind::Scribe(_, items) => items.iter().find_map(field_expr_ty),
        _ => None,
    }
}

#[test]
fn genus_field_mutating_receiver_rejects() {
    assert_rejects_with_issue_la(
        r"
genus Persona {
    varia lista<numerus> amici
}
incipit {
    fixum de Persona p ← Persona { amici = [1, 2] }
    p.amici.appende(3)
}
",
        ISSUE_MUTATING_RECEIVER_THROUGH_REF,
    );
}

#[test]
fn genus_field_projection_assignment_rejects() {
    assert_rejects_with_issue_la(
        r"
genus Persona {
    varia lista<numerus> amici
}
incipit {
    fixum de Persona p ← Persona { amici = [1, 2] }
    p.amici[0] ← 9
}
",
        ISSUE_ASSIGN_THROUGH_REF,
    );
}

#[test]
fn ref_param_field_write_has_no_de_parameter_report() {
    let src = "genus Punctum { varia numerus x }\nfunctio f(de Punctum p) → vacuum { p.x ← 1 }\nincipit { }";
    let result = crate::test_support::analyze_source_la(src);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(
        errors
            .iter()
            .any(|e| crate::test_support::issue(e) == Some(ISSUE_ASSIGN_THROUGH_REF)),
        "{errors:?}"
    );
    assert!(
        errors
            .iter()
            .all(|e| crate::test_support::issue(e) != Some("assign_to_de_parameter")),
        "one fault, one report: {errors:?}"
    );
}

#[test]
fn ref_param_whole_value_write_still_reports_de_parameter() {
    assert_rejects_with_issue_la(
        "functio f(de numerus n) → vacuum { n ← 1 }\nincipit { }",
        "assign_to_de_parameter",
    );
}
