//! D11.8 `exactus<W>` (en `trapping<W>`) policy word, and the bare markers
//! that mean the same default-policy type: `trapping<u8>` is `u8` (one type
//! identity, not a new family), integers and `d64` are admitted, and a float
//! width rejects as "not implemented" (cell (e)), never as an unknown type.

use crate::driver::AnalyzeRequest;
use crate::{Config, Target};

fn pack(id: &str) -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path(id))
        .unwrap_or_else(|err| panic!("installed {id} pack should load: {err:?}"))
}

fn diagnostics_with(config: Config, source: &str) -> Vec<crate::Diagnostic> {
    let session = crate::driver::Session::new(config);
    match crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()) {
        Ok(unit) => unit.diagnostics,
        Err(diagnostics) => diagnostics,
    }
}

fn latin(source: &str) -> Vec<crate::Diagnostic> {
    diagnostics_with(Config::default().with_target(Target::HirRust), source)
}

fn english(source: &str) -> Vec<crate::Diagnostic> {
    diagnostics_with(Config::new().with_locale_pack(pack("en")), source)
}

fn assert_accepts(diagnostics: &[crate::Diagnostic]) {
    assert!(
        !diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error),
        "diagnostics: {diagnostics:?}"
    );
}

fn has_issue(diagnostics: &[crate::Diagnostic], issue: &str) -> bool {
    diagnostics.iter().any(|diagnostic| {
        diagnostic.is_error()
            && diagnostic
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == issue)
    })
}

fn assert_issue(diagnostics: &[crate::Diagnostic], issue: &str) {
    assert!(
        has_issue(diagnostics, issue),
        "expected error issue {issue:?}, got: {diagnostics:?}",
    );
}

#[test]
fn policy_word_is_the_bare_marker_type_for_every_integer_width_and_d64() {
    for width in ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "d64"] {
        // Assignable both ways with no conversion: one type identity.
        assert_accepts(&latin(&format!(
            r"
functio wide(exactus<{width}> x) → {width} {{
    redde x
}}
functio back({width} x) → exactus<{width}> {{
    redde x
}}
"
        )));
    }
}

#[test]
fn policy_word_spelled_out_equals_the_default_int_names() {
    // `trapping<i64>` means exactly `i64`, which is what bare `int` names.
    assert_accepts(&english(
        r"
fn total(trapping<i64> x, trapping<u8> b) → int {
    const u8 same ← b
    const trapping<i64> y ← x
    return y
}
",
    ));
}

#[test]
fn policy_word_mixes_with_the_bare_marker_in_arithmetic() {
    assert_accepts(&latin(
        r"
functio sum(exactus<u8> a, u8 b) → u8 {
    redde a + b
}
",
    ));
}

#[test]
fn policy_word_is_a_distinct_family_from_saturating() {
    // The policy word names the default family, so it still refuses to mix
    // implicitly with the other two.
    assert_issue(
        &latin(
            r"
functio mix(exactus<u8> a, saturatus<u8> b) → u8 {
    redde a + b
}
",
        ),
        "saturating_word_family_mismatch",
    );
}

#[test]
fn policy_word_on_a_float_width_is_not_implemented_not_unknown() {
    for width in ["f16", "bf16", "f32", "f64"] {
        let diagnostics = latin(&format!(
            r"
functio keep(exactus<{width}> x) → numerus {{
    redde 0
}}
"
        ));
        assert_issue(&diagnostics, "trapping_float_not_implemented");
        assert!(
            !has_issue(&diagnostics, "unknown_type"),
            "trapping<{width}> must not report an unknown type: {diagnostics:?}"
        );
    }
}

#[test]
fn policy_word_rejects_bad_arity_shape_and_width() {
    assert_issue(
        &latin("functio f(exactus x) → numerus { redde 0 }"),
        "trapping_word_width_required",
    );
    assert_issue(
        &latin("functio f(exactus<u8, u8> x) → numerus { redde 0 }"),
        "trapping_word_width_required",
    );
    assert_issue(
        &latin("functio f(exactus<lista<u8>> x) → numerus { redde 0 }"),
        "trapping_word_width_shape",
    );
    assert_issue(
        &latin("functio f(exactus<textus> x) → numerus { redde 0 }"),
        "unsupported_trapping_word_width",
    );
}

#[test]
fn policy_word_english_spelling_is_trapping() {
    assert_accepts(&english(
        r"
fn keep(trapping<u8> x) → trapping<u8> {
    return x
}
",
    ));
    assert_issue(
        &english("fn keep(trapping<f32> x) → int { return 0 }"),
        "trapping_float_not_implemented",
    );
}

#[test]
fn policy_word_is_legal_wherever_a_type_goes() {
    assert_accepts(&english(
        r"
fn positions(trapping<u8> p, list<trapping<u16>> xs, trapping<d64> money) → trapping<u8> {
    const trapping<u8> local ← p
    var list<trapping<i32>> ys ← empty
    ys.append(1)
    return local
}
",
    ));
}

#[test]
fn bare_markers_are_types_in_every_position_in_both_locales() {
    // Declarations, parameters, returns, generic args, collection elements,
    // function types, unions, and tensors, in the en and la surfaces.
    assert_accepts(&english(
        r"
fn every(u8 a, i32 b, u64 c, f32 d, f64 e, d64 g, f16 h, bf16 i) → f32 {
    const u8 x ← a
    const f32 y ← d
    const list<f16> hs ← empty
    const tensor<f32, [2, 2]> t ← empty
    return y
}
fn pair((u8, i32) → f64 g, u8 ∪ none maybe) → (u8, i32) → f64 {
    return g
}
fn same<T>(T a) → T {
    return a
}
main {
    const u8 w ← same<u8>(4)
    print w
}
",
    ));
    assert_accepts(&latin(
        r"
functio omnes(u8 a, i32 b, u64 c, f32 d, f64 e, d64 g, f16 h, bf16 i) → f32 {
    fixum u8 x ← a
    fixum f32 y ← d
    fixum lista<f16> hs ← vacua
    fixum tensor<f32, [2, 2]> t ← vacua
    redde y
}
functio par((u8, i32) → f64 g, u8 ∪ nihil forsan) → (u8, i32) → f64 {
    redde g
}
",
    ));
}
