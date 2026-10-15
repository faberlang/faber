//! D11.8 `exactus<W>` (en `trapping<W>`) policy word, and the bare markers
//! that mean the same default-policy type: `trapping<u8>` is `u8` (one type
//! identity, not a new family), integers and `d64` are admitted, and a float
//! width is the trapping float cell (N7e), its own type and its own family.

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
    let u8 same ← b
    let trapping<i64> y ← x
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
fn policy_float_cell_resolves_for_every_float_width_in_both_locales() {
    for width in ["f16", "bf16", "f32", "f64"] {
        assert_accepts(&latin(&format!(
            r"
functio keep(exactus<{width}> x, saturatus<{width}> y) → exactus<{width}> {{
    redde x
}}
"
        )));
        assert_accepts(&english(&format!(
            r"
fn keep(trapping<{width}> x, saturating<{width}> y) → saturating<{width}> {{
    return y
}}
"
        )));
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
    assert_accepts(&english("fn keep(trapping<f32> x) → int { return 0 }"));
}

#[test]
fn policy_word_is_legal_wherever_a_type_goes() {
    assert_accepts(&english(
        r"
fn positions(trapping<u8> p, list<trapping<u16>> xs, trapping<d64> money) → trapping<u8> {
    let trapping<u8> local ← p
    mut list<trapping<i32>> ys ← empty
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
    let u8 x ← a
    let f32 y ← d
    let list<f16> hs ← empty
    let tensor<f32, [2, 2]> t ← empty
    return y
}
fn pair((u8, i32) → f64 g, u8 ∪ none maybe) → (u8, i32) → f64 {
    return g
}
fn same<T>(T a) → T {
    return a
}
main {
    let u8 w ← same<u8>(4)
    print w
}
",
    ));
    assert_accepts(&latin(
        r"
functio omnes(u8 a, i32 b, u64 c, f32 d, f64 e, d64 g, f16 h, bf16 i) → f32 {
    sit u8 x ← a
    sit f32 y ← d
    sit lista<f16> hs ← vacua
    sit tensor<f32, [2, 2]> t ← vacua
    redde y
}
functio par((u8, i32) → f64 g, u8 ∪ nihil forsan) → (u8, i32) → f64 {
    redde g
}
",
    ));
}

// N7E-2: the float cells (`trapping<W>` and `saturating<W>` over a float width)
// type-check. Every assertion names a diagnostic code or issue, never prose.

#[test]
fn policy_float_untyped_constants_adopt_the_cell() {
    assert_accepts(&english(
        r"
main {
    mut trapping<f32> x ← 1.5
    print x + 1
    x ← x + 2.5
    let saturating<f32> a ← 1.5
    print a + a * a
    print -a
    print a ≺ 2
}
",
    ));
}

#[test]
fn policy_float_typed_mixes_are_rejected_with_the_mixing_issue() {
    for body in [
        "let trapping<f32> t ← 1.5\n let f32 b ← 2.5\n print t + b",
        "let f32 b ← 2.5\n let saturating<f32> s ← 1.5\n print b * s",
        "let saturating<f32> s ← 1.5\n let trapping<f32> t ← 1.5\n print s + t",
        "let trapping<f32> t ← 1.5\n let trapping<f64> u ← 1.5\n print t - u",
        "let saturating<f32> s ← 1.5\n let saturating<f64> u ← 1.5\n print s ≺ u",
    ] {
        let diagnostics = english(&format!("main {{\n{body}\n}}\n"));
        assert_issue(&diagnostics, "policy_float_family_mismatch");
    }
}

#[test]
fn policy_float_bare_float_into_a_cell_slot_needs_an_authored_conversion() {
    for (slot, source) in [
        ("trapping<f32>", "f32"),
        ("saturating<f32>", "f32"),
        ("trapping<f32>", "saturating<f32>"),
        ("f32", "trapping<f32>"),
    ] {
        let diagnostics = english(&format!(
            "fn pick({source} v) → {slot} {{\n let {slot} out ← v\n return out\n}}\n"
        ));
        assert_issue(&diagnostics, "expression_type_mismatch");
    }
}

#[test]
fn policy_float_conversion_into_and_out_of_a_cell_is_typed() {
    assert_accepts(&english(
        r"
main {
    let f32 b ← 2.5
    let trapping<f32> t ← b ↦ trapping<f32>
    let f64 w ← t ↦ f64
    let saturating<f64> s ← w ↦ saturating<f64>
    let saturating<f32> s2 ← t ↦ saturating<f32>
    let int n ← 3
    let trapping<f32> t2 ← n ↦ trapping<f32>
    let f64 total ← w + (s ↦ f64) + (s2 ↦ f64) + (t2 ↦ f64)
    print total
}
",
    ));
}

#[test]
fn policy_float_math_method_on_a_cell_names_the_interim_issue() {
    for method in ["abs", "floor"] {
        let diagnostics = english(&format!("main {{\n let saturating<f32> s ← 1.5\n print s.{method}()\n}}\n"));
        assert_issue(&diagnostics, "policy_float_math_not_built");
    }
    // A name the bare float does not have stays an unknown method.
    let unknown = english("main {\n let saturating<f32> s ← 1.5\n print s.nope()\n}\n");
    assert_issue(&unknown, "unknown_method");
    assert!(!has_issue(&unknown, "policy_float_math_not_built"));
}

#[test]
fn policy_float_written_constants_are_range_checked_at_the_cell_width() {
    assert_issue(
        &english("main {\n let trapping<f32> t ← 3.5e38\n print t\n}\n"),
        "float_constant_out_of_range",
    );
    assert_issue(
        &english("main {\n let saturating<f16> s ← 70000\n print s\n}\n"),
        "float_constant_out_of_range",
    );
    assert_accepts(&english("main {\n let saturating<f64> s ← 3.5e38\n print s\n}\n"));
}

#[test]
fn policy_float_ascription_takes_a_written_constant_and_nothing_else() {
    assert_accepts(&english("main {\n let trapping<f32> t ← 2.5 ∷ trapping<f32>\n print t\n}\n"));
    assert_issue(
        &english("main {\n let f32 b ← 2.5\n let trapping<f32> t ← b ∷ trapping<f32>\n print t\n}\n"),
        "expression_type_mismatch",
    );
}

#[test]
fn policy_float_leaves_decimal_and_wrapping_rejections_in_place() {
    assert_issue(
        &english("fn keep(saturating<d64> x) → int { return 0 }"),
        "unsupported_saturating_word_width",
    );
    assert_issue(
        &english("fn keep(wrapping<f32> x) → int { return 0 }"),
        "modular_word_unsigned_width_required",
    );
}
