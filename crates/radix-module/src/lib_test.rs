//! Tests for the top-level Compiler API (`CompileResult`, `Compiler`,
//! `Config`).
//! Proves: success requires output + no errors; basic compile round-trips
//! produce expected output; multi-target configurations resolve.

use crate::{CompileResult, Compiler, Config, Diagnostic};
use std::path::Path;

#[test]
fn compile_result_success_requires_output_and_no_errors() {
    let ok = CompileResult {
        output: Some(crate::Output::Rust(crate::RustOutput { code: String::new() })),
        diagnostics: vec![Diagnostic::warning("non-fatal")],
    };
    assert!(ok.success());

    let no_output = CompileResult { output: None, diagnostics: Vec::new() };
    assert!(!no_output.success());

    let with_error = CompileResult {
        output: Some(crate::Output::TypeScript(crate::TypeScriptOutput { code: String::new() })),
        diagnostics: vec![Diagnostic::error("fatal")],
    };
    assert!(!with_error.success());
}

#[cfg(feature = "hir-rust")]
#[test]
fn compiler_compile_str_produces_output_for_valid_source() {
    let compiler = Compiler::new(Config::default().with_latin_locale());
    let result = compiler.compile_str("test.fab", "incipit {}");

    assert!(result.success());
    assert!(result.output.is_some());
}

#[test]
fn compiler_compile_reports_io_error_for_missing_file() {
    let compiler = Compiler::new(Config::default().with_latin_locale());
    let result = compiler.compile(Path::new("/definitely/missing/faber_test_input.fab"));

    assert!(result.output.is_none());
    assert!(
        result
            .diagnostics
            .iter()
            .any(radix_diagnostics::Diagnostic::is_error)
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.is_error() && d.file == "/definitely/missing/faber_test_input.fab")
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn compiler_emits_rust_runnable_cli_codegen_after_phase_03() {
    let compiler = Compiler::new(Config::default().with_latin_locale());
    let result = compiler.compile_str(
        "cli.fab",
        r#"
@ cli "tool"
incipit argumenta args {}
"#,
    );

    assert!(result.success());
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected Rust CLI output");
    };
    assert!(output.code.contains("parse_cli_args_or_exit"));
}

#[cfg(feature = "hir-rust")]
#[test]
fn shape_generic_tensor_reple_value_emits_filled_copy() {
    // E0308 regression: `param.reple(lr)` has a
    // SameAsReceiver semantic result, but the runtime mutator returns `()`.
    // A bound value must be a fresh filled copy, never `let x: Tensor = recv.reple(v);`.
    let compiler = Compiler::new(Config::default().with_latin_locale());
    let result = compiler.compile_str(
        "generic-shape.fab",
        r"functio sgd_step<magnitudo Figura>(tensor<f32, Figura> param, tensor<f32, Figura> grad, f32 lr) → tensor<f32, Figura> {
    fixum tensor<f32, Figura> lr_fill ← param.reple(lr)
    fixum tensor<f32, Figura> scaled ← grad ⊙ lr_fill
    redde param - scaled
}",
    );

    assert!(result.success(), "shape-generic reple must rust-emit: {:?}", result.diagnostics);
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected Rust output");
    };
    assert!(
        output.code.contains("crea(&p_param.magnitudines()"),
        "reple in value position must construct a fresh filled tensor, got:\n{}",
        output.code
    );
    assert!(
        !output.code.contains("= p_param.reple(") && !output.code.contains("p_param.clone()"),
        "reple value must never be a bare in-place call or an aliasing clone (E0308 / fill-aliasing), got:\n{}",
        output.code
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn tensor_reple_statement_stays_in_place() {
    // The dropped-value form of `reple`/`fill` is the documented in-place
    // mutator: `recv.reple(v);` mutates the receiver instead of filling a
    // discarded copy.
    let compiler = Compiler::new(Config::default().with_latin_locale());
    let result = compiler.compile_str(
        "shape-fill.fab",
        r"incipit {
    varia tensor<f32, [2, 3]> t ← vacua
    t.reple(4.0)
    nota t.longitudo()
}",
    );

    assert!(result.success(), "statement reple must rust-emit: {:?}", result.diagnostics);
    let Some(crate::Output::Rust(output)) = result.output else {
        panic!("expected Rust output");
    };
    assert!(
        // `reple` is fallible (an edge view is read-only), so the in-place call
        // carries its handled outcome: `faber::trap::ok_static(t.reple(4.0));`.
        output.code.contains("faber::trap::ok_static(l_t.reple(4.0))"),
        "statement reple must stay the in-place mutator, got:\n{}",
        output.code
    );
    assert!(
        !output.code.contains("l_t.clone()"),
        "statement reple must not clone the receiver, got:\n{}",
        output.code
    );
}
