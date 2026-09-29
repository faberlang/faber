//! D11.1 `saturatus<W>` (en `saturating<W>`) typing contract: every integer
//! width is admitted, literals adapt to the width and must fit it, the family
//! never mixes with `numerus` or `modulus` without `↦`, and bit-pattern
//! operators fail closed. Rust codegen clamps through `saturating_*`.

use crate::Compiler;
use crate::driver::AnalyzeRequest;
use crate::{Config, Target};

fn analyze_diagnostics(source: &str) -> Vec<crate::Diagnostic> {
    let session = crate::driver::Session::new(Config::default().with_target(Target::HirRust));
    match crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()) {
        Ok(unit) => unit.diagnostics,
        Err(diagnostics) => diagnostics,
    }
}

fn analysis_accepts(source: &str) {
    let diagnostics = analyze_diagnostics(source);
    assert!(
        !diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error),
        "diagnostics: {diagnostics:?}"
    );
}

fn analysis_rejects_with_issue(source: &str, issue: &str) {
    let diagnostics = analyze_diagnostics(source);
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic.is_error()
            && diagnostic
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == issue)),
        "expected error issue {issue:?}, got: {diagnostics:?}",
    );
}

/// `(width, max, max_plus_one)` for every admitted saturating width.
const SATURATING_WIDTH_ROWS: &[(&str, &str, &str)] = &[
    ("i8", "127", "128"),
    ("i16", "32767", "32768"),
    ("i32", "2147483647", "2147483648"),
    ("i64", "9223372036854775807", "9223372036854775808"),
    ("u8", "255", "256"),
    ("u16", "65535", "65536"),
    ("u32", "4294967295", "4294967296"),
];

#[test]
fn saturating_word_admits_every_integer_width() {
    for width in ["i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64"] {
        analysis_accepts(&format!(
            r"
functio add(saturatus<{width}> x, saturatus<{width}> y) → saturatus<{width}> {{
    redde x + y - x * y / y % y
}}
"
        ));
    }
}

#[test]
fn saturating_word_rejects_float_decimal_and_missing_widths() {
    for width in ["f32", "f64", "d32"] {
        analysis_rejects_with_issue(
            &format!(
                r"
functio keep(saturatus<{width}> x) → saturatus<{width}> {{
    redde x
}}
"
            ),
            "unsupported_saturating_word_width",
        );
    }
    analysis_rejects_with_issue(
        r"
functio keep(saturatus<u8, u8> x) → numerus {
    redde 0
}
",
        "saturating_word_width_required",
    );
}

#[test]
fn saturating_word_literal_adapts_and_clamps_at_the_store() {
    // d11-6 U4 (ruling 21, amended): a constant stored with `←` into a
    // `saturating` slot clamps. This inverts the pre-U4 pin
    // `saturating_word_literal_out_of_range` for `←` positions; an `=`
    // position (`generis`) still refuses a constant that does not fit.
    for (width, max, max_plus_one) in SATURATING_WIDTH_ROWS {
        analysis_accepts(&format!(
            r"
incipit {{
    fixum saturatus<{width}> top ← {max}
    nota top + 1
    nota 1 + top
}}
"
        ));
        analysis_accepts(&format!(
            r"
incipit {{
    fixum saturatus<{width}> tooBig ← {max_plus_one}
    nota tooBig
}}
"
        ));
        analysis_rejects_with_issue(
            &format!("generis saturatus<{width}> TOO_BIG = {max_plus_one}\nincipit {{\n    nota TOO_BIG\n}}\n"),
            "store_constant_out_of_range",
        );
    }
}

#[test]
fn saturating_word_signed_negative_literal_adapts() {
    analysis_accepts(
        r"
incipit {
    fixum saturatus<i16> low ← -32767
    nota low * -2
}
",
    );
}

#[test]
fn saturating_word_rejects_mixed_family_arithmetic() {
    analysis_rejects_with_issue(
        r"
functio mix(saturatus<u8> x, numerus<u8> y) → saturatus<u8> {
    redde x + y
}
",
        "saturating_word_family_mismatch",
    );
    analysis_rejects_with_issue(
        r"
functio mix(saturatus<u8> x, modulus<u8> y) → saturatus<u8> {
    redde x - y
}
",
        "saturating_word_family_mismatch",
    );
    analysis_rejects_with_issue(
        r"
functio mix(numerus<i32> x, saturatus<i32> y) → numerus<i32> {
    redde x * y
}
",
        "saturating_word_family_mismatch",
    );
}

#[test]
fn saturating_word_family_does_not_flow_implicitly() {
    let diagnostics = analyze_diagnostics(
        r"
functio keep(numerus<u8> x) → saturatus<u8> {
    redde x
}
",
    );
    assert!(
        diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error),
        "numerus<u8> must not flow into saturatus<u8> without ↦: {diagnostics:?}"
    );
}

#[test]
fn saturating_word_converts_to_and_from_integers_with_conversio() {
    analysis_accepts(
        r"
functio round(numerus x) → numerus {
    fixum saturatus<u8> level ← x ↦ saturatus<u8>
    fixum saturatus<i16> wide ← level ↦ saturatus<i16>
    redde wide ↦ numerus
}
",
    );
}

#[test]
fn saturating_word_rejects_bit_pattern_operators() {
    for body in ["x ∧ x", "x ⊻ x", "x ⇐ 1", "¬x"] {
        analysis_rejects_with_issue(
            &format!(
                r"
functio bits(saturatus<u8> x) → saturatus<u8> {{
    redde {body}
}}
"
            ),
            "saturating_word_operator_unsupported",
        );
    }
}

#[test]
fn saturating_word_rejects_unsigned_negation_and_inc_dec() {
    analysis_rejects_with_issue(
        r"
functio flip(saturatus<u8> x) → saturatus<u8> {
    redde -x
}
",
        "saturating_word_operator_unsupported",
    );
    analysis_rejects_with_issue(
        r"
incipit {
    varia saturatus<u8> level ← 1
    level ↑
    nota level
}
",
        "saturating_word_operator_unsupported",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn saturating_word_rust_codegen_clamps_with_saturating_methods() {
    let result = Compiler::new(Config::default().with_target(Target::HirRust)).compile_str(
        "test.fab",
        r"
functio mix(saturatus<i16> x, saturatus<i16> y) → saturatus<i16> {
    redde -((x + y) - (x * y))
}
",
    );
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected Rust output")
    };
    for needle in [
        "i16::saturating_add",
        "i16::saturating_sub",
        "i16::saturating_mul",
        "i16::saturating_neg",
    ] {
        assert!(output.code.contains(needle), "missing {needle}:\n{}", output.code);
    }
}
