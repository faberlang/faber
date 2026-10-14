//! Right-operand rule of collection membership (`∈` / `∉`).
//!
//! Tests assert diagnostic code, issue and args only, never prose.

use crate::{Compiler, Config, Target};

fn compile(source: &str) -> crate::CompileResult {
    Compiler::new(Config::default().with_target(Target::HirRust).with_latin_locale()).compile_str("test.fab", source)
}

fn accepts(source: &str) {
    let result = compile(source);
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
}

/// Assert exactly one error: `SEM010` with `issue` and, when given, the
/// right-operand type name in arg `found`.
fn rejects(source: &str, issue: &str, found: Option<&str>) {
    let result = compile(source);
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|diag| diag.is_error())
        .collect();
    assert_eq!(errors.len(), 1, "diagnostics: {:?}", result.diagnostics);
    let diag = errors[0];
    assert_eq!(diag.code, Some("SEM010"), "diagnostics: {:?}", result.diagnostics);
    assert_eq!(diag.issue(), Some(issue), "diagnostics: {:?}", result.diagnostics);
    if let Some(found) = found {
        let arg = diag
            .args
            .iter()
            .find(|arg| arg.name == "found")
            .map(|arg| arg.value.as_str());
        assert_eq!(arg, Some(found), "diagnostics: {:?}", result.diagnostics);
    }
}

#[test]
fn list_right_operand_accepts() {
    accepts("incipit { fixum lista<numerus> xs ← [1, 2]\n nota 1 ∈ xs }");
}

#[test]
fn set_right_operand_accepts() {
    accepts("incipit { varia copia<numerus> xs ← vacua\n nota 1 ∈ xs }");
}

#[test]
fn map_right_operand_tests_keys() {
    accepts("incipit { fixum tabula<textus, numerus> m ← {\"a\": 1}\n nota \"a\" ∈ m }");
}

#[test]
fn text_right_operand_is_rejected_for_a_text_left() {
    // OD-1: no membership operator over text; the text method is the spelling.
    rejects(
        "incipit { fixum textus c ← \"7\"\n nota c ∈ \"0123456789\" }",
        "membership_over_text",
        Some("textus"),
    );
}

#[test]
fn text_right_operand_is_rejected_whatever_the_left_operand() {
    rejects(
        "incipit { fixum numerus n ← 5\n fixum textus t ← \"abc\"\n nota n ∈ t }",
        "membership_over_text",
        Some("textus"),
    );
}

#[test]
fn negated_text_right_operand_is_rejected() {
    rejects(
        "incipit { fixum textus t ← \"abc\"\n nota \"a\" ∉ t }",
        "membership_over_text",
        Some("textus"),
    );
}

#[test]
fn ascii_right_operand_is_rejected() {
    rejects(
        "incipit { fixum ascii t ← 'abc'\n nota 'a' ∈ t }",
        "membership_over_text",
        Some("ascii"),
    );
}

#[test]
fn list_element_mismatch_keeps_its_issue() {
    rejects(
        "incipit { fixum lista<numerus> xs ← [1, 2]\n nota \"a\" ∈ xs }",
        "membership_operand_type_mismatch",
        None,
    );
}

#[test]
fn integer_right_operand_rejects() {
    rejects(
        "incipit { fixum numerus x ← 5\n nota x ∈ 5 }",
        "membership_requires_collection",
        Some("numerus"),
    );
}

#[test]
fn bool_right_operand_rejects() {
    rejects(
        "incipit { fixum bivalens b ← verum\n nota verum ∈ b }",
        "membership_requires_collection",
        Some("bivalens"),
    );
}

#[test]
fn nullable_list_right_operand_rejects() {
    rejects(
        "incipit { fixum lista<numerus> ∪ nihil m ← [1]\n nota 1 ∈ m }",
        "membership_requires_collection",
        None,
    );
}

#[test]
fn nullable_list_with_wrong_element_rejects_as_nullable() {
    rejects(
        "incipit { fixum lista<textus> ∪ nihil m ← [\"a\"]\n nota 3 ∈ m }",
        "membership_requires_collection",
        None,
    );
}

#[test]
fn tuple_right_operand_rejects() {
    rejects(
        "functio probe(iuncta<numerus, numerus> t) → bivalens { redde 1 ∈ t }",
        "membership_requires_collection",
        None,
    );
}

#[test]
fn generic_parameter_right_operand_rejects() {
    rejects(
        "functio probe<T>(T x) → bivalens { redde 1 ∈ x }",
        "membership_requires_collection",
        None,
    );
}
