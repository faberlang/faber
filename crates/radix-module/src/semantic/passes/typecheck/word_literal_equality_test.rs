//! Need (operator ruling 2026-10-02): under `≡` an untyped constant
//! adopts the type of a policy cell (`saturating`, `wrapping`, `trapping`),
//! exactly as it does beside the bare width. A typed operand of another type
//! still needs `↦`.

use crate::driver::AnalyzeRequest;
use crate::{Config, Diagnostic};

const EXACT_EQ_ISSUE: &str = "equality_exact_types_required";

fn english(source: &str) -> Vec<Diagnostic> {
    let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .expect("installed en pack should load");
    let session = crate::driver::Session::new(Config::new().with_locale_pack(pack));
    match crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()) {
        Ok(unit) => unit.diagnostics,
        Err(diagnostics) => diagnostics,
    }
}

fn errors(diagnostics: &[Diagnostic]) -> Vec<&Diagnostic> {
    diagnostics.iter().filter(|d| d.is_error()).collect()
}

fn has_exact_eq_issue(diagnostics: &[Diagnostic]) -> bool {
    errors(diagnostics).iter().any(|diagnostic| {
        diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == EXACT_EQ_ISSUE)
    })
}

fn assert_accepts(source: &str) {
    let diagnostics = english(source);
    assert!(
        errors(&diagnostics).is_empty(),
        "expected accept, got: {diagnostics:?}\n{source}"
    );
}

fn assert_exact_eq_rejected(source: &str) {
    let diagnostics = english(source);
    assert!(
        has_exact_eq_issue(&diagnostics),
        "expected {EXACT_EQ_ISSUE}, got: {diagnostics:?}\n{source}"
    );
}

#[test]
fn integer_policy_cells_compare_against_a_literal_under_exact_equality() {
    for cell in [
        "u8",
        "saturating<u8>",
        "wrapping<u8>",
        "trapping<u8>",
        "saturating<i32>",
        "wrapping<i32>",
    ] {
        for body in ["x ≡ 250", "250 ≡ x", "x ≢ 250", "250 ≢ x"] {
            assert_accepts(&format!("fn probe({cell} x) → bool {{\n    return {body}\n}}\n"));
        }
    }
}

#[test]
fn float_policy_cells_compare_against_a_literal_under_exact_equality() {
    for cell in ["f64", "saturating<f64>", "trapping<f64>", "trapping<f32>"] {
        for body in ["x ≡ 2.5", "2.5 ≡ x", "x ≢ 2.5"] {
            assert_accepts(&format!("fn probe({cell} x) → bool {{\n    return {body}\n}}\n"));
        }
    }
}

#[test]
fn policy_cell_literal_equality_holds_in_an_if_condition() {
    assert_accepts("main {\n    let saturating<u8> x ← 250\n    if x ≡ 250 {\n        print \"eq\"\n    }\n}\n");
}

#[test]
fn a_typed_operand_of_another_type_still_needs_conversion() {
    assert_exact_eq_rejected("fn probe(saturating<u8> x, u8 y) → bool {\n    return x ≡ y\n}\n");
    assert_exact_eq_rejected("fn probe(wrapping<u8> x, wrapping<u16> y) → bool {\n    return x ≡ y\n}\n");
    assert_exact_eq_rejected("fn probe(wrapping<u8> x, saturating<u8> y) → bool {\n    return x ≡ y\n}\n");
}
