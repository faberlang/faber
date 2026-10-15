//! decimal-widths D1 probes: annotated decimal literals carry exact digit
//! text through typing, and the d64 marker law holds at the surface.

use crate::test_support::{self, analyze_source_la};

#[test]
fn annotated_d64_literal_typechecks() {
    let fixture = analyze_source_la("incipit { sit d64 b ← 4.2 }");
    assert!(fixture.success(), "d64 must accept 4.2");
}

#[test]
fn explicit_integer_to_decimal_conversion_still_accepts() {
    test_support::assert_accepts_la("incipit { sit d64 e ← 5 ↦ d64 }");
}

#[test]
fn d64_literal_out_of_range_rejects() {
    // The i64 scaled carrier cannot hold 1e20.
    let fixture = analyze_source_la("incipit { sit d64 a ← 1e20 }");
    assert!(fixture.is_err(), "1e20 exceeds the d64 scaled carrier");
}

#[test]
fn bare_decimal_stays_fractus() {
    // Ruling: bare 4.2 stays fractus — it must not flow into bare numerus.
    let fixture = analyze_source_la("incipit { sit numerus a ← 4.2 }");
    assert!(fixture.is_err(), "bare 4.2 keeps fractus inference");
}

#[test]
fn d_marker_on_fractus_rejects() {
    // N7d: the wrapped spelling is retired whole, so a wrong-family wrapper is
    // the wrapper diagnostic; the bare `d64` is its own (integer-family) type.
    test_support::assert_rejects_with_issue_la(
        "incipit { sit fractus<d64> a ← 4.2 }",
        "numeric_wrapper_retired",
    );
}

#[test]
fn f_marker_on_numerus_still_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit numerus<f32> a ← 1 }",
        "numeric_wrapper_retired",
    );
}

// --- d32 was removed (D11.7): a dedicated diagnostic points at d64 ---

#[test]
fn wrapped_numerus_d32_is_the_retired_wrapper_not_the_d32_removal() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit numerus<d32> a ← 4.2 }",
        "numeric_wrapper_retired",
    );
}

#[test]
fn bare_d32_type_is_removed() {
    test_support::assert_rejects_with_issue_la(
        "functio f(d32 a) → d32 { redde a }",
        "removed_decimal_width_d32",
    );
}

#[test]
fn removed_d32_help_points_at_d64() {
    let fixture = analyze_source_la("incipit { sit d32 a ← 4.2 }");
    let text = format!("{:?}", fixture.diagnostics);
    assert!(text.contains("removed_decimal_width_d32"), "{text}");
}

// --- D11.7: scale 8; a written literal is never rounded (ruling 20) ---

#[test]
fn d64_literal_accepts_eight_fraction_digits_and_the_range_edge() {
    test_support::assert_accepts_la("incipit { sit d64 a ← 0.12345678 }");
    test_support::assert_accepts_la("incipit { sit d64 a ← 92233720368.54775807 }");
}

#[test]
fn d64_literal_with_nine_fraction_digits_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit d64 a ← 0.123456789 }",
        "decimal_literal_out_of_range",
    );
}

#[test]
fn d64_literal_past_the_range_edge_rejects() {
    test_support::assert_rejects_with_issue_la(
        "incipit { sit d64 a ← 92233720368.54775808 }",
        "decimal_literal_out_of_range",
    );
}
