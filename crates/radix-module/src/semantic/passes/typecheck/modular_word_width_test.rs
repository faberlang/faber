//! R0 red width-family contract: `modulus<W>` must admit exactly the unsigned
//! widths u8/u16/u32/u64 under one width-parametric contract. The u32 rows are
//! the shipped regression floor and stay green; the u8/u16/u64 rows fail until
//! R3 replaces the `lower_modular_word` u32 allowlist with the unsigned-width
//! gate.

// Unconditional: `analyze_diagnostics` below runs in every feature set and is
// not target-specific (previously cfg'd, which broke reduced-feature builds).
use crate::Compiler;
use crate::driver::AnalyzeRequest;
use crate::{Config, Target};

#[cfg(feature = "hir-rust")]
fn compile(source: &str) -> crate::CompileResult {
    Compiler::new(
        Config::default()
            .with_target(Target::HirRust)
            .with_latin_locale(),
    )
    .compile_str("test.fab", source)
}

#[cfg(feature = "hir-faber")]
fn compile_for_target(source: &str, target: Target) -> crate::CompileResult {
    Compiler::new(Config::default().with_target(target).with_latin_locale()).compile_str("test.fab", source)
}

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
        diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error),
        "expected failure, got: {diagnostics:?}"
    );
    assert!(
        diagnostics.iter().any(|diagnostic| diagnostic
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == issue)),
        "expected diagnostic issue {issue:?}, got: {diagnostics:?}",
    );
}

/// One row of the shared modular width vector table. `max` is 2^W - 1 and
/// `max_plus_one` is the first out-of-range literal for the width.
struct ModularWidthRow {
    width: &'static str,
    max: &'static str,
    max_plus_one: &'static str,
    #[cfg(feature = "hir-rust")]
    rust_wrapping_add: &'static str,
}

const MODULAR_WIDTH_ROWS: &[ModularWidthRow] = &[
    ModularWidthRow {
        width: "u8",
        max: "255",
        max_plus_one: "256",
        #[cfg(feature = "hir-rust")]
        rust_wrapping_add: "u8::wrapping_add",
    },
    ModularWidthRow {
        width: "u16",
        max: "65535",
        max_plus_one: "65536",
        #[cfg(feature = "hir-rust")]
        rust_wrapping_add: "u16::wrapping_add",
    },
    ModularWidthRow {
        width: "u32",
        max: "4294967295",
        max_plus_one: "4294967296",
        #[cfg(feature = "hir-rust")]
        rust_wrapping_add: "u32::wrapping_add",
    },
    ModularWidthRow {
        width: "u64",
        max: "18446744073709551615",
        // 2^64 cannot be spelled as a decimal literal under the u64 carrier;
        // the u64 out-of-range proof is the lexer-level unsigned_integer gate.
        max_plus_one: "",
        #[cfg(feature = "hir-rust")]
        rust_wrapping_add: "u64::wrapping_add",
    },
];

#[test]
fn modular_word_admits_every_unsigned_width() {
    for row in MODULAR_WIDTH_ROWS {
        let source = format!(
            r"
functio add(modulus<{w}> x, modulus<{w}> y) → modulus<{w}> {{
    redde x + y
}}
",
            w = row.width
        );
        analysis_accepts(&source);
    }
}

#[test]
fn modular_word_width_rows_reject_decimal_and_float_widths() {
    // These rows are the isolation floor and must stay green in every stage.
    analysis_rejects_with_issue(
        r"
functio keep(modulus<d64> x) → modulus<d64> {
    redde x
}
",
        "unsupported_modular_word_width",
    );
    analysis_rejects_with_issue(
        r"
functio keep(modulus<f32> x) → modulus<f32> {
    redde x
}
",
        "modular_word_unsigned_width_required",
    );
}

#[test]
fn modular_word_accepts_max_literal_per_width() {
    for row in MODULAR_WIDTH_ROWS {
        let source = format!(
            r"
incipit {{
    sit modulus<{w}> max ← {max}
    nota max
}}
",
            w = row.width,
            max = row.max
        );
        analysis_accepts(&source);
    }
}

#[test]
fn modular_word_reduces_max_plus_one_literal_per_width() {
    // d11-6 U4 (ruling 21, amended): a constant stored with `←` into a
    // `wrapping` slot follows the slot's policy and reduces. This inverts the
    // pre-U4 pin `modular_word_literal_out_of_range` for `←` positions; an `=`
    // position (`generis`) still refuses a constant that does not fit.
    for row in MODULAR_WIDTH_ROWS {
        if row.max_plus_one.is_empty() {
            continue;
        }
        let source = format!(
            r"
incipit {{
    sit modulus<{w}> tooBig ← {max_plus_one}
    nota tooBig
}}
",
            w = row.width,
            max_plus_one = row.max_plus_one
        );
        analysis_accepts(&source);
        let generis = format!(
            "fixum modulus<{w}> TOO_BIG = {max_plus_one}\nincipit {{\n    nota TOO_BIG\n}}\n",
            w = row.width,
            max_plus_one = row.max_plus_one
        );
        analysis_rejects_with_issue(&generis, "store_constant_out_of_range");
    }
}

#[cfg(feature = "hir-rust")]
#[test]
fn modular_word_rust_codegen_uses_width_typed_wrapping_ops() {
    for row in MODULAR_WIDTH_ROWS {
        let source = format!(
            r"
functio add(modulus<{w}> x, modulus<{w}> y) → modulus<{w}> {{
    redde x + y
}}
",
            w = row.width
        );
        let result = compile(&source);
        assert!(result.success(), "modulus<{}> diagnostics: {:?}", row.width, result.diagnostics);
        let Some(crate::Output::Rust(output)) = result.output else {
            panic!("expected Rust output")
        };
        assert!(
            output.code.contains(row.rust_wrapping_add),
            "modulus<{}> must emit {}:\n{}",
            row.width,
            row.rust_wrapping_add,
            output.code
        );
    }
}

#[cfg(feature = "hir-faber")]
#[test]
fn modular_word_faber_codegen_roundtrips_every_width() {
    for row in MODULAR_WIDTH_ROWS {
        let source = format!(
            r"
functio add(modulus<{w}> x, modulus<{w}> y) → modulus<{w}> {{
    redde x + y
}}
",
            w = row.width
        );
        let result = compile_for_target(&source, Target::HirFaber);
        assert!(result.success(), "modulus<{}> diagnostics: {:?}", row.width, result.diagnostics);
        let Some(crate::Output::Faber(output)) = result.output else {
            panic!("expected Faber output")
        };
        let spelling = format!("modulus<{}>", row.width);
        assert!(
            output.code.contains(&spelling),
            "Faber output must preserve {spelling}:\n{}",
            output.code
        );
    }
}

#[test]
fn modular_word_cross_width_arithmetic_rejects_in_both_directions() {
    // Width isolation: same family, different widths still requires an
    // explicit conversion. u8 rows go red with admission in R3; the u32/u64
    // direction proves isolation as soon as u64 lands.
    for (left, right) in [("u8", "u16"), ("u32", "u64"), ("u64", "u32")] {
        let source = format!(
            r"
functio add(modulus<{left}> x, modulus<{right}> y) → modulus<{left}> {{
    redde x + y
}}
",
        );
        let diagnostics = analyze_diagnostics(&source);
        assert!(
            diagnostics
                .iter()
                .any(radix_diagnostics::Diagnostic::is_error),
            "modulus<{left}> + modulus<{right}> must reject: {diagnostics:?}"
        );
    }
}

#[test]
fn modular_word_admits_every_signed_width() {
    // D11.8: signed wrapping is the ring Z/2^W with signed representatives.
    for width in ["i8", "i16", "i32", "i64"] {
        let source = format!(
            r"
functio step(modulus<{width}> x, modulus<{width}> y) → modulus<{width}> {{
    redde -x / y + x % y - (x ⇒ 1) + ¬y
}}
"
        );
        analysis_accepts(&source);
    }
}

#[test]
fn signed_modular_word_stores_reduce_constants_and_conversions() {
    analysis_accepts(
        r"
incipit {
    sit modulus<i8> wrapped ← 200
    sit modulus<i8> low ← -128
    sit numerus wide ← 300
    sit modulus<i8> converted ← wide ↦ modulus<i8>
    nota wrapped + low + converted
}
",
    );
}

#[test]
fn signed_modular_word_keeps_family_and_width_isolation() {
    let diagnostics = analyze_diagnostics(
        r"
functio bad(modulus<i8> x, i8 y) → modulus<i8> {
    redde x + y
}
",
    );
    assert!(
        diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error)
    );
    let diagnostics = analyze_diagnostics(
        r"
functio bad(modulus<i8> x, modulus<u8> y) → modulus<i8> {
    redde x + y
}
",
    );
    assert!(
        diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error)
    );
}
