//! decimal-widths D1 probes: annotated decimal literals carry exact digit
//! text through typing, and the d32/d64 marker law holds at the surface.

use crate::test_support::analyze_source;

#[test]
fn annotated_d32_literal_typechecks() {
    let fixture = analyze_source("incipit { fixum numerus<d32> a ← 4.2 }");
    assert!(
        fixture.success(),
        "numerus<d32> a ← 4.2 must typecheck; diagnostics: {:?}",
        fixture.diagnostics
    );
}

#[test]
fn annotated_d64_literal_typechecks() {
    let fixture = analyze_source("incipit { fixum numerus<d64> b ← 4.2 }");
    assert!(fixture.success(), "numerus<d64> must accept 4.2");
}

#[test]
fn d32_literal_out_of_range_rejects() {
    // i32×10⁻⁷ carrier cannot hold 1e20.
    let fixture = analyze_source("incipit { fixum numerus<d32> a ← 1e20 }");
    assert!(fixture.is_err(), "1e20 exceeds the d32 scaled carrier");
}

#[test]
fn bare_decimal_stays_fractus() {
    // Ruling: bare 4.2 stays fractus — it must not flow into bare numerus.
    let fixture = analyze_source("incipit { fixum numerus a ← 4.2 }");
    assert!(fixture.is_err(), "bare 4.2 keeps fractus inference");
}

#[test]
fn d_marker_on_fractus_rejects() {
    let fixture = analyze_source("incipit { fixum fractus<d32> a ← 4.2 }");
    assert!(fixture.is_err(), "d-markers are numerus-only");
}

#[test]
fn f_marker_on_numerus_still_rejects() {
    let fixture = analyze_source("incipit { fixum numerus<f32> a ← 1 }");
    assert!(fixture.is_err(), "FloatOnNumerus stays rejected");
}
