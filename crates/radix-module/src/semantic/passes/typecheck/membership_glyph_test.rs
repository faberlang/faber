//! The unified membership operator (`∈` / `∉`): the right operand's static type
//! picks range or collection, and the checker records the resolved operator in
//! the HIR.
//!
//! Tests assert diagnostic code, issue and args only, never prose. Acceptance is
//! checked on the analysis path: `∉` has no emitter until unit 3b.

use super::test_support::{analyze_ok, assert_analysis_accepts as assert_accepts};
use crate::hir::{HirBinOp, HirExpressionKind, HirStatementKind};
use crate::{Compiler, Config, Target};

fn compile(source: &str) -> crate::CompileResult {
    Compiler::new(Config::default().with_target(Target::HirRust)).compile_str("test.fab", source)
}

/// The resolved operator of every `fixum _ ← <binary>` local in the entry block.
fn resolved_ops(source: &str) -> Vec<HirBinOp> {
    let unit = analyze_ok(source);
    let entry = unit.hir.entry.as_ref().expect("incipit");
    entry
        .statements
        .iter()
        .filter_map(|stmt| match &stmt.kind {
            HirStatementKind::Local(local) => match local.init.as_ref().map(|init| &init.kind) {
                Some(HirExpressionKind::Binary(op, ..)) => Some(*op),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

fn issues(source: &str) -> Vec<(Option<&'static str>, Option<String>)> {
    compile(source)
        .diagnostics
        .iter()
        .filter(|diag| diag.is_error())
        .map(|diag| (diag.code, diag.issue().map(str::to_owned)))
        .collect()
}

fn rejects(source: &str, code: &'static str, issue: &str) {
    let found = issues(source);
    assert!(
        found.contains(&(Some(code), Some(issue.to_owned()))),
        "expected {code}:{issue}, got {found:?}"
    );
}

#[test]
fn range_right_operand_resolves_to_in_range() {
    let ops = resolved_ops("incipit { fixum numerus x ← 5\n fixum bivalens r ← x ∈ 0‥10 }");
    assert_eq!(ops, [HirBinOp::InRange]);
}

#[test]
fn range_right_operand_negated_resolves_to_not_in_range() {
    let ops = resolved_ops("incipit { fixum numerus x ← 5\n fixum bivalens r ← x ∉ 0‥10 }");
    assert_eq!(ops, [HirBinOp::NotInRange]);
}

#[test]
fn collection_right_operand_resolves_to_between() {
    let ops =
        resolved_ops("incipit { fixum numerus x ← 5\n fixum lista<numerus> xs ← [1, 2]\n fixum bivalens r ← x ∈ xs }");
    assert_eq!(ops, [HirBinOp::Between]);
}

#[test]
fn collection_right_operand_negated_resolves_to_not_between() {
    let ops =
        resolved_ops("incipit { fixum numerus x ← 5\n fixum lista<numerus> xs ← [1, 2]\n fixum bivalens r ← x ∉ xs }");
    assert_eq!(ops, [HirBinOp::NotBetween]);
}

#[test]
fn range_held_in_a_binding_resolves_by_its_type() {
    let ops = resolved_ops(
        "incipit { fixum numerus x ← 5\n fixum intervallum<numerus> fines ← 0‥10\n fixum bivalens r ← x ∈ fines }",
    );
    assert_eq!(ops, [HirBinOp::InRange]);
}

#[test]
fn keyword_forms_resolve_through_the_same_path() {
    // `inter` over a range now takes the range rule; `intra` is unchanged.
    let ops = resolved_ops(
        "incipit { fixum numerus x ← 5\n fixum lista<numerus> xs ← [1]\n fixum bivalens a ← x inter 0‥10\n fixum bivalens b ← x intra 0‥10\n fixum bivalens c ← x inter xs }",
    );
    assert_eq!(ops, [HirBinOp::InRange, HirBinOp::InRange, HirBinOp::Between]);
}

#[test]
fn scalar_right_operand_is_rejected_with_its_type() {
    rejects(
        "incipit { fixum numerus x ← 5\n nota x ∈ 5 }",
        "SEM010",
        "membership_requires_collection",
    );
    rejects(
        "incipit { fixum numerus x ← 5\n nota x ∉ 5 }",
        "SEM010",
        "membership_requires_collection",
    );
}

#[test]
fn collection_shapes_accept() {
    assert_accepts("incipit { fixum lista<numerus> xs ← [1, 2]\n nota 1 ∈ xs\n nota 3 ∉ xs }");
    assert_accepts("incipit { varia copia<numerus> xs ← vacua\n nota 1 ∈ xs }");
    assert_accepts("incipit { fixum tabula<textus, numerus> m ← {\"a\": 1}\n nota \"a\" ∈ m\n nota \"b\" ∉ m }");
}

#[test]
fn range_accepts_numeric_left_and_rejects_text_left() {
    assert_accepts("incipit { fixum numerus n ← 5\n nota n ∈ 0‥10\n nota n ∉ 0‥10 }");
    rejects("incipit { nota \"text\" ∈ 0‥3 }", "SEM011", "range_operand_must_be_numeric");
}

#[test]
fn float_left_is_not_a_range_bound() {
    rejects(
        "incipit { fixum fractus f ← 5.0\n nota f ∈ 0‥10 }",
        "SEM011",
        "range_operand_must_be_numeric",
    );
}

#[test]
fn enum_value_set_checks() {
    assert_accepts(
        "ordo Color { rubrum, viridis, caeruleus }\nincipit { fixum Color c ← Color.rubrum\n nota c ∈ [Color.rubrum, Color.viridis]\n nota c ∉ [Color.viridis, Color.caeruleus] }",
    );
}

#[test]
fn mixed_value_set_fails_common_type() {
    rejects(
        "ordo Color { rubrum, viridis }\nincipit { fixum Color c ← Color.rubrum\n nota c ∈ [Color.rubrum, 3] }",
        "SEM010",
        "common_type_mismatch",
    );
}
