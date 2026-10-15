//! D11.1 `saturatus<W>` (en `saturating<W>`) typing contract: every integer
//! width is admitted, literals adapt to the width and must fit it, the family
//! never mixes with `numerus` or `modulus` without `↦`, and bit-pattern
//! operators fail closed. Arithmetic is exact (ruling 3); a store clamps.

use crate::Compiler;
use crate::driver::AnalyzeRequest;
use crate::{Config, Target};

fn analyze_diagnostics(source: &str) -> Vec<crate::Diagnostic> {
    let session = crate::driver::Session::new(
        Config::default()
            .with_target(Target::HirRust)
            .with_latin_locale(),
    );
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
fn saturating_word_rejects_decimal_and_missing_widths() {
    analysis_rejects_with_issue(
        r"
functio keep(saturatus<d64> x) → saturatus<d64> {
    redde x
}
",
        "unsupported_saturating_word_width",
    );
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
    sit saturatus<{width}> top ← {max}
    nota top + 1
    nota 1 + top
}}
"
        ));
        analysis_accepts(&format!(
            r"
incipit {{
    sit saturatus<{width}> tooBig ← {max_plus_one}
    nota tooBig
}}
"
        ));
        analysis_rejects_with_issue(
            &format!("fixum saturatus<{width}> TOO_BIG = {max_plus_one}\nincipit {{\n    nota TOO_BIG\n}}\n"),
            "store_constant_out_of_range",
        );
    }
}

#[test]
fn negated_minimum_constant_is_judged_by_its_exact_value() {
    // lw2 (ruling 12): `-128` is one exact constant that fits `i8`, though the
    // magnitude 128 alone does not. One below the minimum clamps in a
    // saturating slot and is a compile error in a trapping slot.
    for (width, min, min_minus_one) in [
        ("i8", "-128", "-129"),
        ("i16", "-32768", "-32769"),
        ("i32", "-2147483648", "-2147483649"),
        ("i64", "-9223372036854775808", "-9223372036854775809"),
    ] {
        for keyword in ["sit", "varia"] {
            analysis_accepts(&format!(
                "incipit {{\n    {keyword} saturatus<{width}> low ← {min}\n    nota low\n}}\n"
            ));
        }
        analysis_accepts(&format!("incipit {{\n    sit {width} low ← {min}\n    nota low\n}}\n"));
        analysis_accepts(&format!(
            "incipit {{\n    sit saturatus<{width}> below ← {min_minus_one}\n    nota below\n}}\n"
        ));
        analysis_rejects_with_issue(
            &format!("incipit {{\n    sit {width} below ← {min_minus_one}\n    nota below\n}}\n"),
            "store_constant_out_of_range",
        );
    }
    analysis_accepts("incipit {\n    sit numerus low ← -9223372036854775808\n    nota low\n}\n");
}

#[test]
fn saturating_word_operand_constant_is_exact_and_only_the_store_clamps() {
    // d11-6 ruling 3: `low - 300 + 100` is exact (-190) and clamps once, so a
    // constant beside a saturating operand may exceed the word's range.
    analysis_accepts(
        r"
incipit {
    sit saturatus<u8> low ← 10
    sit saturatus<u8> a ← low - 300 + 100
    sit saturatus<u8> b ← low + 300
    nota a
    nota b
}
",
    );
}

#[test]
fn saturating_word_signed_negative_literal_adapts() {
    analysis_accepts(
        r"
incipit {
    sit saturatus<i16> low ← -32767
    nota low * -2
}
",
    );
}

#[test]
fn saturating_word_rejects_mixed_family_arithmetic() {
    analysis_rejects_with_issue(
        r"
functio mix(saturatus<u8> x, u8 y) → saturatus<u8> {
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
functio mix(i32 x, saturatus<i32> y) → i32 {
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
functio keep(u8 x) → saturatus<u8> {
    redde x
}
",
    );
    assert!(
        diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error),
        "u8 must not flow into saturatus<u8> without ↦: {diagnostics:?}"
    );
}

#[test]
fn saturating_word_converts_to_and_from_integers_with_conversio() {
    analysis_accepts(
        r"
functio round(numerus x) → numerus {
    sit saturatus<u8> level ← x ↦ saturatus<u8>
    sit saturatus<i16> wide ← level ↦ saturatus<i16>
    redde wide ↦ numerus
}
",
    );
}

#[test]
fn saturating_word_bit_pattern_operators_are_exact() {
    // Ruling 19: `∧ ∨ ⊻ ¬ ⇐ ⇒` are exact over integers; the store clamps.
    analysis_accepts(
        r"
incipit {
    varia saturatus<u8> x ← 200
    sit saturatus<u8> a ← x ∧ x
    sit saturatus<u8> b ← x ∨ 256
    sit saturatus<u8> c ← x ⊻ x
    sit saturatus<u8> d ← ¬x
    sit saturatus<u8> e ← x ⇐ 1
    sit saturatus<u8> f ← x ⇒ 1
    nota a + b + c + d + e + f
}
",
    );
    // A `saturating` operand never meets a trapping integer variable.
    analysis_rejects_with_issue(
        r"
functio bits(saturatus<u8> x, numerus n) → numerus {
    redde x ∧ n
}
",
        "saturating_word_family_mismatch",
    );
}

#[test]
fn saturating_word_unsigned_negation_is_exact_and_inc_dec_clamps() {
    // Ruling 3/19: `-x` is exact (a negative value); the store clamps it.
    analysis_accepts(
        r"
functio flip(saturatus<u8> x) → saturatus<u8> {
    redde -x
}
",
    );
    // `level ↑` is `level ← level + 1`: a store into the slot, which clamps.
    analysis_accepts(
        r"
incipit {
    varia saturatus<u8> level ← 1
    level ↑
    level ↓
    nota level
}
",
    );
}

#[test]
fn saturating_word_expression_is_exact_and_an_inferred_slot_keeps_the_word() {
    // Ruling 3: `s + 10` is an exact integer, stored into an integer slot
    // as-is; an inferred slot keeps `saturating<W>` (ruling 12).
    analysis_accepts(
        r"
incipit {
    sit saturatus<u8> s ← 250
    sit numerus wide ← s + 10
    sit inferred ← s + 10
    sit saturatus<u8> same ← inferred
    nota wide
    nota same
}
",
    );
    analysis_rejects_with_issue(
        r"
incipit {
    sit saturatus<u8> s ← 250
    sit u8 narrow ← s + 10
    sit saturatus<u16> other ← narrow
    nota other
}
",
        "expression_type_mismatch",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn saturating_word_rust_codegen_computes_a_single_operation_exactly() {
    // Ruling 3: even one operation is exact `i128` arithmetic; the return
    // store is the only clamp. No native `saturating_*` method is emitted.
    let result = Compiler::new(
        Config::default()
            .with_target(Target::HirRust)
            .with_latin_locale(),
    )
    .compile_str(
        "test.fab",
        r"
functio add(saturatus<i16> x, saturatus<i16> y) → saturatus<i16> {
    redde x + y
}
functio wide(saturatus<i16> x, saturatus<i16> y) → numerus {
    redde x + y
}
",
    );
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected Rust output")
    };
    assert!(
        output
            .code
            .contains(".clamp(i128::from(i16::MIN), i128::from(i16::MAX))"),
        "missing the store clamp:\n{}",
        output.code
    );
    for per_step in ["saturating_add", "saturating_sub", "saturating_mul"] {
        assert!(
            !output.code.contains(per_step),
            "per-operation {per_step} must not appear:\n{}",
            output.code
        );
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn saturating_word_rust_codegen_clamps_a_chain_once_at_its_root() {
    // d11-6 ruling 3 (U9): a chain is exact `i128` arithmetic with one clamp,
    // never a saturating method per step. This replaces the pre-U9 pin of
    // `i16::saturating_{add,sub,mul,neg}` on `-((x + y) - (x * y))`.
    let result = Compiler::new(
        Config::default()
            .with_target(Target::HirRust)
            .with_latin_locale(),
    )
    .compile_str(
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
    assert!(
        output
            .code
            .contains(".clamp(i128::from(i16::MIN), i128::from(i16::MAX))"),
        "missing the single store clamp:\n{}",
        output.code
    );
    for per_step in ["saturating_add", "saturating_sub", "saturating_mul", "saturating_neg"] {
        assert!(
            !output.code.contains(per_step),
            "per-step {per_step} must not appear:\n{}",
            output.code
        );
    }
}
