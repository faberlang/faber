//! The unified membership operator (`∈` / `∉`): the right operand's static type
//! picks range or collection, and the checker records the resolved operator in
//! the HIR.
//!
//! Tests assert diagnostic code, issue and args only, never prose. Acceptance is
//! checked on the analysis path: `∉` has no emitter until unit 3b.

use super::test_support::{analyze_ok_la, assert_analysis_accepts_la as assert_accepts};
use crate::hir::{HirBinOp, HirExpressionKind, HirStatementKind};
use crate::{Compiler, Config, Target};

fn compile(source: &str) -> crate::CompileResult {
    Compiler::new(Config::default().with_target(Target::HirRust).with_latin_locale()).compile_str("test.fab", source)
}

/// The resolved operator of every `fixum _ ← <binary>` local in the entry block.
fn resolved_ops(source: &str) -> Vec<HirBinOp> {
    let unit = analyze_ok_la(source);
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
fn retired_keyword_spellings_fail_at_parse_with_the_migration_diagnostic() {
    // `intra` / `inter` are not operators any more; `∈` is the one spelling.
    for source in [
        "incipit { fixum numerus x ← 5\n fixum lista<numerus> xs ← [1]\n nota x inter xs }",
        "incipit { fixum numerus x ← 5\n nota x intra 0‥10 }",
    ] {
        rejects(source, "PARSE001", "membership_keyword_retired");
    }
}

#[test]
fn text_right_operand_is_rejected_in_both_polarities() {
    // OD-1: membership never applies to text; the text method does.
    for source in [
        "incipit { fixum textus t ← \"abc\"\n nota \"a\" ∈ t }",
        "incipit { fixum textus t ← \"abc\"\n nota \"a\" ∉ t }",
        "incipit { nota \"a\" ∈ \"abc\" }",
    ] {
        rejects(source, "SEM010", "membership_over_text");
    }
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

/// The `found` arg of the first `SEM010:membership_operand_unresolved` error.
fn unresolved_found(source: &str) -> Option<String> {
    compile(source)
        .diagnostics
        .iter()
        .filter(|diag| diag.is_error() && diag.issue() == Some("membership_operand_unresolved"))
        .find_map(|diag| {
            diag.args
                .iter()
                .find(|arg| arg.name == "found")
                .map(|arg| arg.value.clone())
        })
}

#[test]
fn empty_list_right_operand_is_unresolved_in_both_polarities() {
    for source in ["incipit { nota 1 ∈ [] }", "incipit { nota 1 ∉ [] }"] {
        rejects(source, "SEM010", "membership_operand_unresolved");
        assert!(unresolved_found(source).is_some(), "no found arg: {source}");
    }
}

#[test]
fn unresolved_container_element_is_reported_once_at_the_right_operand() {
    let source = "incipit { fixum _ xs ← []\n nota 1 ∈ xs }";
    let found = issues(source);
    let count = found
        .iter()
        .filter(|(_, issue)| issue.as_deref() == Some("membership_operand_unresolved"))
        .count();
    assert_eq!(count, 1, "{found:?}");
}

#[test]
fn unannotated_hole_right_operand_is_unresolved() {
    rejects("incipit { varia _ y\n nota 1 ∈ y }", "SEM010", "membership_operand_unresolved");
}

#[test]
fn resolved_empty_collections_are_not_unresolved() {
    assert_accepts("incipit { fixum lista<numerus> xs ← []\n nota 1 ∈ xs }");
    assert_accepts("incipit { varia copia<numerus> xs ← vacua\n nota 1 ∉ xs }");
}

#[test]
fn generic_right_operand_is_rejected_by_the_collection_rule_not_this_one() {
    let source = "functio quaere<T>(T t) → bivalens { redde 1 ∈ t }\nincipit { }";
    let found = issues(source);
    assert!(
        found.contains(&(Some("SEM010"), Some("membership_requires_collection".to_owned()))),
        "{found:?}"
    );
    assert_eq!(unresolved_found(source), None);
}

#[test]
fn unresolved_left_operand_still_binds_the_bound_type() {
    for source in [
        "incipit { fixum lista<numerus> xs ← [1]\n varia _ y\n nota y ∈ xs }",
        "incipit { varia _ y\n nota y ∈ 0‥10 }",
    ] {
        assert_eq!(unresolved_found(source), None, "{source}");
        let found = issues(source);
        assert!(
            found.iter().all(|(_, issue)| !matches!(
                issue.as_deref(),
                Some("membership_operand_type_mismatch" | "cannot_infer_expression_type")
            )),
            "{source}: {found:?}"
        );
    }
}
