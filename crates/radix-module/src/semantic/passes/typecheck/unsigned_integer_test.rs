//! R0 red carrier contract: unsigned integer magnitudes must survive analysis
//! through `u64::MAX`. `numerus<u64>` is a declared width today, but the
//! signed `i64` literal carrier caps source magnitudes at `i64::MAX`, so every
//! acceptance test here fails until R1 lands the unsigned magnitude carrier.

// Unconditional: `analyze_diagnostics` below runs in every feature set and is
// not hir-rust-specific (previously cfg'd, which broke reduced-feature builds).
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

fn analysis_rejects(source: &str) {
    let diagnostics = analyze_diagnostics(source);
    assert!(
        diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error),
        "expected failure, got: {diagnostics:?}"
    );
}

#[test]
fn unsigned_integer_numerus_u64_accepts_i64_max_plus_one() {
    analysis_accepts(
        r"
incipit {
    sit u64 sha ← 9223372036854775808
    nota sha
}
",
    );
}

#[test]
fn unsigned_integer_numerus_u64_accepts_u64_max() {
    analysis_accepts(
        r"
incipit {
    sit u64 max ← 18446744073709551615
    nota max
}
",
    );
}

#[test]
fn unsigned_integer_numerus_u64_accepts_radix_prefixed_u64_max() {
    analysis_accepts(
        r"
incipit {
    sit u64 max ← 0xffffffffffffffff
    nota max
}
",
    );
}

#[test]
fn unsigned_integer_numerus_u64_rejects_above_u64_max() {
    analysis_rejects(
        r"
incipit {
    sit u64 tooBig ← 18446744073709551616
    nota tooBig
}
",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn unsigned_integer_rust_codegen_preserves_u64_max_literal() {
    let result = compile(
        r"
incipit {
    sit u64 max ← 18446744073709551615
    nota max
}
",
    );
    assert!(result.success(), "diagnostics: {:?}", result.diagnostics);
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected Rust output")
    };
    assert!(
        output.code.contains("18446744073709551615"),
        "generated Rust must preserve the exact u64::MAX magnitude:\n{}",
        output.code
    );
}

#[test]
fn unsigned_integer_signed_numerus_range_unchanged() {
    // Signed boundary floor: i64::MAX remains accepted and plain numerus
    // overflow above i64::MAX remains rejected by literal range checks.
    analysis_accepts(
        r"
incipit {
    sit numerus max ← 9223372036854775807
    nota max
}
",
    );
    analysis_rejects(
        r"
incipit {
    sit numerus tooBig ← 9223372036854775808
    nota tooBig
}
",
    );
}

#[test]
fn unsigned_integer_u64_analysis_accepts_zero() {
    analysis_accepts(
        r"
incipit {
    sit u64 zero ← 0
    nota zero
}
",
    );
}
