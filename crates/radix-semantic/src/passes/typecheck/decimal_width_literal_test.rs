//! decimal-widths D1 probes: annotated decimal literals carry exact digit
//! text through typing, and the d64 marker law holds at the surface.

use crate::test_support::{self, analyze_source};

#[test]
fn annotated_d64_literal_typechecks() {
    let fixture = analyze_source("incipit { fixum numerus<d64> b ← 4.2 }");
    assert!(fixture.success(), "numerus<d64> must accept 4.2");
}

#[test]
fn decimal_addition_rejects_integer_literal() {
    test_support::assert_rejects_with_issue(
        "functio add(numerus<d64> a) → numerus<d64> { redde a + 1 }",
        "decimal_integer_literal_rejected",
    );
}

#[test]
fn decimal_division_rejects_integer_literal() {
    test_support::assert_rejects_with_issue(
        "functio dividi(numerus<d64> a) → numerus<d64> { redde a / 2 }",
        "decimal_integer_literal_rejected",
    );
}

#[test]
fn d64_binding_rejects_integer_literal() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum numerus<d64> e ← 5 }",
        "decimal_integer_literal_rejected",
    );
}

#[test]
fn explicit_integer_to_decimal_conversion_still_accepts() {
    test_support::assert_accepts("incipit { fixum numerus<d64> e ← 5 ↦ numerus<d64> }");
}

#[test]
fn d64_literal_out_of_range_rejects() {
    // The i64 scaled carrier cannot hold 1e20.
    let fixture = analyze_source("incipit { fixum numerus<d64> a ← 1e20 }");
    assert!(fixture.is_err(), "1e20 exceeds the d64 scaled carrier");
}

#[test]
fn bare_decimal_stays_fractus() {
    // Ruling: bare 4.2 stays fractus — it must not flow into bare numerus.
    let fixture = analyze_source("incipit { fixum numerus a ← 4.2 }");
    assert!(fixture.is_err(), "bare 4.2 keeps fractus inference");
}

#[test]
fn d_marker_on_fractus_rejects() {
    let fixture = analyze_source("incipit { fixum fractus<d64> a ← 4.2 }");
    assert!(fixture.is_err(), "d-markers are numerus-only");
}

#[test]
fn f_marker_on_numerus_still_rejects() {
    let fixture = analyze_source("incipit { fixum numerus<f32> a ← 1 }");
    assert!(fixture.is_err(), "FloatOnNumerus stays rejected");
}

// --- d32 was removed (D11.7): a dedicated diagnostic points at d64 ---

#[test]
fn numerus_d32_marker_is_removed() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum numerus<d32> a ← 4.2 }",
        "removed_decimal_width_d32",
    );
}

#[test]
fn bare_d32_type_is_removed() {
    test_support::assert_rejects_with_issue(
        "functio f(d32 a) → d32 { redde a }",
        "removed_decimal_width_d32",
    );
}

#[test]
fn removed_d32_help_points_at_d64() {
    let fixture = analyze_source("incipit { fixum numerus<d32> a ← 4.2 }");
    let text = format!("{:?}", fixture.diagnostics);
    assert!(text.contains("removed_decimal_width_d32"), "{text}");
}

// --- D11.7: scale 8; a written literal is never rounded (ruling 20) ---

#[test]
fn d64_literal_accepts_eight_fraction_digits_and_the_range_edge() {
    test_support::assert_accepts("incipit { fixum numerus<d64> a ← 0.12345678 }");
    test_support::assert_accepts("incipit { fixum numerus<d64> a ← 92233720368.54775807 }");
}

#[test]
fn d64_literal_with_nine_fraction_digits_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum numerus<d64> a ← 0.123456789 }",
        "decimal_literal_out_of_range",
    );
}

#[test]
fn d64_literal_past_the_range_edge_rejects() {
    test_support::assert_rejects_with_issue(
        "incipit { fixum numerus<d64> a ← 92233720368.54775808 }",
        "decimal_literal_out_of_range",
    );
}
