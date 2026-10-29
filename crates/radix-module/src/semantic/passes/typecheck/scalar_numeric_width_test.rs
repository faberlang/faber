//! Regression coverage for scalar numeric width inference, arithmetic, and codegen emission.
//!
//! These tests lock the typecheck rules for fractus widths (f16, f32, f64, bf16),
//! numerus widths (i8, i16, i32, i64, u8, u16, u32, u64), modular word arithmetic,
//! cross-family rejection, explicit widening/narrowing conversio, and target-level
//! unsupported-width diagnostics.

use crate::codegen::Target;
use crate::semantic::passes::typecheck::test_support;

#[cfg(feature = "hir-rust")]
#[test]
fn fractus_f32_arithmetic_preserves_width() {
    test_support::assert_accepts_la(
        r"
functio add(f32 x, f32 y) → f32 {
    redde x + y
}
",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn fractus_f32_arithmetic_shapes_float_literal() {
    test_support::assert_accepts_la(
        r"
functio addOne(f32 x) → f32 {
    redde x + 1.0
}
",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn fractus_f64_arithmetic_preserves_width() {
    test_support::assert_accepts_la(
        r"
functio add(f64 x, f64 y) → f64 {
    redde x + y
}
",
    );
}

#[test]
fn fractus_f16_is_known_to_semantic_analysis() {
    test_support::assert_analysis_accepts_la(
        r"
functio keep(f16 x) → f16 {
    redde x
}
",
    );
}

#[test]
fn standalone_f16_is_fractus_width_sugar() {
    test_support::assert_analysis_accepts_la(
        r"
functio keep(f16 x) → f16 {
    redde x
}
",
    );
}

#[test]
fn wrapped_numerus_f16_is_the_retired_wrapper() {
    // N7d: the wrapped spelling is rejected whole, a float marker included.
    test_support::assert_analysis_rejects_with_issue_la(
        r"
functio bad(numerus<f16> x) → numerus<f16> {
    redde x
}
",
        "numeric_wrapper_retired",
    );
}

#[test]
fn bf16_is_admitted_as_a_whole_width_marker() {
    test_support::assert_analysis_accepts_la(
        r"
functio keep(bf16 x) → bf16 {
    redde x
}
",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_codegen_rejects_f16_until_runtime_representation_exists() {
    test_support::assert_rejects_with_issue_la(
        r"
functio keep(f16 x) → f16 {
    redde x
}
",
        "rust_target_fractus_f16_unsupported",
    );
}

/// Inverted (d11-6 U3): mixed float widths join by the lossless lattice
/// instead of requiring identical operands.
#[test]
fn fractus_f16_with_f32_arithmetic_joins_at_f32() {
    test_support::assert_analysis_accepts_la(
        r"
functio add(f16 x, f32 y) → f32 {
    redde x + y
}
",
    );
}

/// Inverted (d11-6 U3): the sum of two `i32` values is `i64` (range rule),
/// not `i32`; storing it back into `i32` is the checked-store rule (U4).
#[test]
fn numerus_i32_arithmetic_grows_to_i64() {
    test_support::assert_analysis_accepts_la(
        r"
functio add(i32 x, i32 y) → i64 {
    redde x + y
}
",
    );
}

/// Inverted (d11-6 U3): an integer constant joins by value, it is not shaped
/// to the peer's width.
#[test]
fn numerus_i32_arithmetic_joins_int_constant_by_value() {
    test_support::assert_analysis_accepts_la(
        r"
functio addOne(i32 x) → i64 {
    redde x + 1
}
",
    );
}

/// Inverted (d11-6 ruling 20): an exactly representable integer constant
/// beside a float operand is accepted.
#[test]
fn arithmetic_accepts_integer_constant_beside_float() {
    test_support::assert_analysis_accepts_la(
        r"
functio addOne(f32 x) → f32 {
    redde x + 1
}
",
    );
}

/// A typed integer beside a float crosses number families (ruling 16).
#[test]
fn arithmetic_rejects_typed_integer_beside_float() {
    test_support::assert_rejects_with_issue_la(
        r"
functio add(f32 x, i32 n) → f32 {
    redde x + n
}
",
        "numeric_family_crossing_requires_conversio",
    );
}

/// Inverted (d11-6 ruling 1): mixed float widths join by the lattice.
#[test]
fn arithmetic_joins_mixed_fractus_widths() {
    test_support::assert_analysis_accepts_la(
        r"
functio add(f32 x, f64 y) → f64 {
    redde x + y
}
",
    );
}

/// Inverted (d11-6 ruling 12): mixed integer widths are range-typed, not
/// rejected.
#[test]
fn arithmetic_joins_mixed_numerus_widths() {
    test_support::assert_analysis_accepts_la(
        r"
functio add(i32 x, i64 y) → i64 {
    redde x + y
}
",
    );
}

/// Inverted (d11-6 U3): `u8 + u8` is `u16` (range rule), not `u8`.
#[test]
fn numerus_u8_arithmetic_grows_to_u16() {
    test_support::assert_analysis_accepts_la(
        r"
functio add(u8 x, u8 y) → u16 {
    redde x + y
}
",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn modular_word_u32_is_a_distinct_numeric_family() {
    test_support::assert_accepts_la(
        r"
functio add(modulus<u32> x, modulus<u32> y) → modulus<u32> {
    redde x + y
}
",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn modular_word_u8_arithmetic_preserves_width() {
    test_support::assert_accepts_la(
        r"
functio add(modulus<u8> x, modulus<u8> y) → modulus<u8> {
    redde x + y
}
",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn modular_word_u64_arithmetic_preserves_width() {
    test_support::assert_accepts_la(
        r"
functio add(modulus<u64> x, modulus<u64> y) → modulus<u64> {
    redde x + y
}
",
    );
}

#[test]
fn modular_word_zero_literal_accepts() {
    test_support::assert_analysis_accepts_la(
        r"
incipit {
    sit modulus<u32> zero ← 0
    nota zero
}
",
    );
}

#[test]
fn modular_word_rejects_mixed_numerus_arithmetic() {
    test_support::assert_analysis_rejects_with_issue_la(
        r"
functio add(modulus<u32> x, u32 y) → modulus<u32> {
    redde x + y
}
",
        "modular_word_family_mismatch",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn modular_word_compares_with_numerus_by_value() {
    // d11-6 rulings 12, 22: a comparison stores nothing, so a word beside a
    // plain integer or a constant compares exact values.
    test_support::assert_accepts_la(
        r"
functio compare(modulus<u32> x, u32 y) → bivalens {
    redde x ≻ y
}
functio constant(modulus<u8> a) → bivalens {
    redde a ≻ 100 et a ≅ 200 et 3 ≺ a et a ≇ 7
}
",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn saturating_word_compares_with_numerus_and_constants_by_value() {
    test_support::assert_accepts_la(
        r"
functio compare(saturatus<u8> s, numerus n) → bivalens {
    redde s ≺ 251 et s ≅ n et s ≺ n et n ≥ s
}
",
    );
}

#[test]
fn word_equality_stays_exact_type() {
    test_support::assert_analysis_rejects_with_issue_la(
        r"
functio same(modulus<u8> a, numerus n) → bivalens {
    redde a ≡ n
}
",
        "equality_exact_types_required",
    );
}

#[test]
fn modular_word_rejects_numerus_to_modulus_assignment() {
    test_support::assert_analysis_rejects_with_issue_la(
        r"
incipit {
    sit u32 checked ← 1
    sit modulus<u32> word ← checked
    nota word
}
",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn modular_word_rejects_modulus_to_numerus_assignment() {
    test_support::assert_analysis_rejects_with_issue_la(
        r"
incipit {
    sit modulus<u32> word ← 1
    sit u32 checked ← word
    nota checked
}
",
        "initializer_annotation_mismatch",
    );
}

#[test]
fn modular_word_method_rejects_numerus_argument() {
    test_support::assert_analysis_rejects_with_issue_la(
        r"
incipit {
    sit modulus<u32> word ← 4294967295
    sit u32 checked ← 1
    nota word.addita(checked)
}
",
        "modular_word_family_mismatch",
    );
}

#[test]
fn numerus_method_rejects_modulus_argument() {
    test_support::assert_analysis_rejects_with_issue_la(
        r"
incipit {
    sit modulus<u32> word ← 4294967295
    sit u32 checked ← 1
    nota checked.addita(word)
}
",
        "modular_word_family_mismatch",
    );
}

#[test]
fn modular_word_rejects_unsupported_widths() {
    test_support::assert_analysis_rejects_with_issue_la(
        r"
functio keep(modulus<d64> x) → modulus<d64> {
    redde x
}
",
        "unsupported_modular_word_width",
    );
}

#[test]
fn modular_word_reduces_out_of_range_literals_at_the_store() {
    // d11-6 U4 (ruling 21, amended): `redde` is a `←`-style store, so the
    // constant reduces; this inverts the pre-U4 pin
    // `modular_word_literal_out_of_range` for stores.
    test_support::assert_analysis_accepts_la(
        r"
functio keep(modulus<u32> x) → modulus<u32> {
    redde 4294967296
}
",
    );
}

#[test]
fn modular_word_verte_rejects_out_of_range_literal() {
    test_support::assert_analysis_rejects_with_issue_la(
        r"
incipit {
    nota 4294967296 ∷ modulus<u32>
}
",
        "modular_word_literal_out_of_range",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_codegen_uses_modular_wrapping_operation() {
    let result = test_support::compile_and_get_result_la(
        r"
functio add(modulus<u32> x, modulus<u32> y) → modulus<u32> {
    redde x + y
}
",
    );
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected Rust output")
    };
    assert!(output.code.contains("u32::wrapping_add"), "{}", output.code);
}

#[cfg(feature = "hir-faber")]
#[test]
fn faber_codegen_roundtrips_modular_word_type() {
    let result = test_support::compile_for_target_and_get_result_la(
        r"
functio add(modulus<u32> x, modulus<u32> y) → modulus<u32> {
    redde x + y
}
",
        Target::HirFaber,
    );
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let Some(crate::Output::Faber(output)) = result.output else {
        panic!("expected Faber output")
    };
    assert!(output.code.contains("modulus<u32>"), "{}", output.code);
}

#[cfg(feature = "hir-rust")]
#[test]
fn same_width_modular_and_numerus_conversion_is_explicitly_available() {
    test_support::assert_accepts_la(
        r"
functio toNumerus(modulus<u32> value) → u32 {
    redde value ↦ u32
}

functio toModulus(u32 value) → modulus<u32> {
    redde value ↦ modulus<u32>
}
",
    );
}

#[cfg(all(feature = "hir-go", feature = "mir-llvm", feature = "mir-wasm"))]
#[test]
fn go_llvm_and_wasm_accept_modular_words_at_emit() {
    let source = r"
functio add(modulus<u32> x, modulus<u32> y) → modulus<u32> {
    redde x + y
}
";
    // TypeScript was de-gated in the S7-U6 sweep (TS4-1) and Go lifted its
    // wrapping gate in U6g (0d498f28c). LLVM text and Wasm text lift the
    // modular-word gate at their emit boundaries: `modulus<W>` lowers as an
    // unsigned integral family there.
    for target in [Target::HirGo, Target::MirLlvm, Target::MirWasm] {
        let result = test_support::compile_for_target_and_get_result_la(source, target);
        assert!(result.success(), "{target:?} now emits modular words: {:?}", result.diagnostics);
    }
}

#[cfg(all(feature = "mir-metal", feature = "mir-wgsl"))]
#[test]
fn metal_and_wgsl_emit_u32_modular_word_kernels() {
    // K15 implements the 32-bit modular word on both device leaves, so the
    // old modular-word rejection no longer applies to `modulus<u32>`.
    let source = r"
@ nucleum
@ publica
functio wordAdd(modulus<u32> value) → modulus<u32> {
    redde value + 1
}
";
    for target in [Target::MirMetal, Target::MirWgsl] {
        let result = test_support::compile_for_target_and_get_result_la(source, target);
        assert!(
            result.success(),
            "{target:?} modulus<u32> kernel diagnostics: {:?}",
            result.diagnostics
        );
    }
}

#[cfg(all(feature = "hir-go", feature = "mir-llvm", feature = "mir-wasm"))]
#[test]
fn go_wasm_and_llvm_accept_every_modular_width() {
    const WIDTHS: [&str; 4] = ["u8", "u16", "u32", "u64"];
    // Go lifted its gate in U6g (0d498f28c); Wasm text and LLVM text lower the
    // whole modular-word family (Stage 3B B1 / llvm-host-parity emit boundary)
    // as unsigned integral scalars.
    for target in [Target::HirGo, Target::MirWasm, Target::MirLlvm] {
        for width in WIDTHS {
            let source = format!(
                "functio add(modulus<{width}> x, modulus<{width}> y) → modulus<{width}> {{\n    redde x + y\n}}\n"
            );
            let result = test_support::compile_for_target_and_get_result_la(&source, target);
            assert!(
                result.success(),
                "{target:?} modulus<{width}> now emits: {:?}",
                result.diagnostics
            );
        }
    }
}

#[cfg(all(feature = "mir-metal", feature = "mir-wgsl"))]
#[test]
fn metal_and_wgsl_reject_unimplemented_modular_widths_with_stable_issue() {
    // After the kernel-profile widening (need, 2026-10-16), the
    // sub-32-bit modular words (modulus<u8>/modulus<u16>, Wrapping policy)
    // are gate-admitted on the device leaves and fail later at the emitter
    // carrier seam. Ruling 2026-10-28 admits u64 at check as well: it is a
    // per-target capability fact: WGSL refuses it by name in its requirement
    // row, and Metal's emitter refuses the wide scalar layout (never a check
    // diagnostic).
    const WIDTHS: [&str; 1] = ["u64"];
    for width in WIDTHS {
        let source = format!(
            "@ nucleum\n@ publica\nfunctio wordAdd(modulus<{width}> value) → modulus<{width}> {{\n    redde value + 1\n}}\n"
        );
        for (target, issue) in [
            (Target::MirMetal, "mir_metal_text_unsupported"),
            (Target::MirWgsl, "kernel_target_requirements_not_met"),
        ] {
            let result = test_support::compile_for_target_and_get_result_la(&source, target);
            assert!(!result.success(), "{target:?} unexpectedly accepted a modulus<{width}> kernel");
            assert!(
                result.diagnostics.iter().any(|diagnostic| diagnostic
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == issue)),
                "{target:?} modulus<{width}> diagnostics: {:?}",
                result.diagnostics
            );
        }
    }
}

#[cfg(feature = "mir-sexp")]
#[test]
fn sexp_accepts_every_modular_width() {
    // ss5 (6a53b353e) lifted the sexp modular/saturating gate and implemented
    // the words in radix-mir-sexp; every width must now emit.
    for (width, max) in [
        ("u8", "255"),
        ("u16", "65535"),
        ("u32", "4294967295"),
        ("u64", "18446744073709551615"),
    ] {
        let source = format!("incipit {{\n    sit modulus<{width}> max ← {max}\n    nota max + 1\n}}\n");
        let result = test_support::compile_for_target_and_get_result_la(&source, Target::MirSexp);
        assert!(result.success(), "Sexp modulus<{width}> now emits: {:?}", result.diagnostics);
    }
}

#[cfg(feature = "hir-go")]
#[test]
fn go_accepts_local_modular_words_before_emit() {
    let source = r"
incipit {
    sit modulus<u32> max ← 4294967295
    sit modulus<u32> one ← 1
    nota max + one
}
";
    // U6g (0d498f28c) lifted the Go gate; the same local modular-word program
    // must now emit there, as it does for TypeScript below.
    let result = test_support::compile_for_target_and_get_result_la(source, Target::HirGo);
    assert!(result.success(), "Go diagnostics: {:?}", result.diagnostics);
}

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_accepts_local_modular_words_before_emit() {
    // TS4-1 sibling: the S7-U6 sweep de-gated the TypeScript emit path, so
    // the same local modular-word program must now emit successfully there.
    let source = r"
incipit {
    sit modulus<u32> max ← 4294967295
    sit modulus<u32> one ← 1
    nota max + one
}
";
    let result = test_support::compile_for_target_and_get_result_la(source, Target::HirTypeScript);
    assert!(
        result.success(),
        "TypeScript now emits local modular words (TS4-1): {:?}",
        result.diagnostics
    );
}

#[cfg(feature = "mir-sexp")]
#[test]
fn sexp_accepts_modular_words_at_emit() {
    let result = test_support::compile_for_target_and_get_result_la(
        r"
incipit {
    sit modulus<u32> max ← 4294967295
    nota max + 1
}
",
        Target::MirSexp,
    );
    assert!(result.success(), "Sexp now emits modular words: {:?}", result.diagnostics);
}

#[test]
fn lista_preserves_modular_word_element_type() {
    test_support::assert_analysis_accepts_la(
        r"
incipit {
    sit lista<modulus<u32>> values ← []
}
",
    );
}

#[test]
fn octeti_is_not_numeric_arithmetic() {
    // A byte list is `lista<b8>`: the list `+` is refused by the
    // numeric-list operator law, and an element is a raw-bits `b8` that takes
    // no arithmetic either (bits are not numbers).
    test_support::assert_rejects_with_issue_la(
        r"
incipit {
    sit lista<b8> left ← |01|
    sit lista<b8> right ← |02|
    sit _ both ← left + right
}
",
        "numeric_lista_operator_law",
    );
    test_support::assert_rejects_with_issue_la(
        r"
incipit {
    sit b8 left ← 1
    sit b8 right ← 2
    sit _ both ← left + right
}
",
        "bits_arithmetic_refused",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn assignment_widening_remains_assimilative() {
    test_support::assert_accepts_la(
        r"
functio widen(i16 x) → i32 {
    redde x
}
",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn explicit_lista_target_allows_sized_numeric_widening() {
    test_support::assert_accepts_la(
        r"
incipit {
    sit f32 single ← 1.0 ∷ f32
    sit lista<f64> xs ← [single] ∷ lista<f64>
}
",
    );
}

#[test]
fn explicit_lista_target_warns_only_for_sized_numeric_narrowing() {
    let source = r"
incipit {
    sit f64 wide ← 1.0 ∷ f64
    sit lista<f32> xs ← [wide] ∷ lista<f32>
}
";
    let diagnostics = test_support::analyze_and_get_diagnostics_la(source);
    assert!(
        !diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error),
        "expected no errors for sized numeric narrowing, got: {diagnostics:?}",
    );
    let warning_count = diagnostics.iter().filter(|d| !d.is_error()).count();
    assert!(
        warning_count > 0,
        "expected at least one warning for sized numeric narrowing, but no diagnostics were emitted"
    );
}
