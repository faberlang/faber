//! End-to-end cross-crate integration tests.
//!
//! These tests validate that a Faber source program passes through the full
//! compilation pipeline (parse → analyze → MIR lower → backend emit) and
//! produces correct output for each backend.
//!
//! Backend toolchain checks (e.g. `llvm-as`, `naga`) are handled by the
//! `backend_smoke` module's smoke helpers; these tests assert structural
//! properties of the emitted output that do not require external tools.

#[cfg(feature = "mir-llvm")]
use radix_module::LlvmTextOutput;
#[cfg(feature = "mir-metal")]
use radix_module::MetalTextOutput;
#[cfg(feature = "mir-sexp")]
use radix_module::SexpOutput;
#[cfg(feature = "mir-wasm")]
use radix_module::WasmTextOutput;
#[cfg(feature = "mir-wgsl")]
use radix_module::WgslTextOutput;
#[cfg(any(
    feature = "mir-llvm",
    feature = "mir-metal",
    feature = "mir-wasm",
    feature = "mir-wgsl",
    feature = "mir-sexp"
))]
use radix_module::driver::{Config, ModuleCompileRequest, Session, compile};
#[cfg(any(
    feature = "mir-llvm",
    feature = "mir-metal",
    feature = "mir-wasm",
    feature = "mir-wgsl",
    feature = "mir-sexp"
))]
use radix_module::{Output, Target};

/// Simple Faber program: a function that adds two integers.
#[cfg(any(feature = "mir-llvm", feature = "mir-wasm", feature = "mir-sexp"))]
const ADD_SOURCE: &str = r"
functio adde(numerus a, numerus b) → numerus {
    redde a + b
}
";

/// A minimal Faber program with an incipit block (entry point).
#[cfg(feature = "mir-llvm")]
const INCIPIT_SOURCE: &str = r"
incipit {
    nota 42
}
";

/// A Faber kernel function for GPU backends (requires @ nucleum annotation).
/// GPU kernels need either a single buffer parameter, or named input*/output* parameters.
#[cfg(any(feature = "mir-wgsl", feature = "mir-metal"))]
const KERNEL_SOURCE: &str = r"
@ nucleum
@ publica
functio add_one(f32 x) → f32 {
    redde x + 1.0
}
";

// ── LLVM backend (lane: mir-llvm) ───────────────────────────────────────

#[cfg(feature = "mir-llvm")]
#[test]
fn llvm_text_emits_nonempty_output() {
    let session = Session::new(Config::default().with_target(Target::MirLlvm).with_latin_locale());
    let result = compile(&session, "smoke.fab", ADD_SOURCE, ModuleCompileRequest::default());
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);

    let Output::LlvmText(LlvmTextOutput { code }) = result.output.expect("LLVM output missing") else {
        panic!("expected LlvmText output");
    };
    assert!(!code.is_empty(), "LLVM text output is empty");
}

#[cfg(feature = "mir-llvm")]
#[test]
fn llvm_text_contains_function_definition() {
    let session = Session::new(Config::default().with_target(Target::MirLlvm).with_latin_locale());
    let result = compile(&session, "smoke.fab", ADD_SOURCE, ModuleCompileRequest::default());
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);

    let Output::LlvmText(LlvmTextOutput { code }) = result.output.expect("LLVM output missing") else {
        panic!("expected LlvmText output");
    };
    assert!(
        code.contains("define"),
        "LLVM output should contain a function definition:\n{code}"
    );
    assert!(code.contains("add"), "LLVM output should contain an add instruction:\n{code}");
}

#[cfg(feature = "mir-llvm")]
#[test]
fn llvm_text_emits_nonempty_for_incipit() {
    let session = Session::new(Config::default().with_target(Target::MirLlvm).with_latin_locale());
    let result = compile(&session, "smoke.fab", INCIPIT_SOURCE, ModuleCompileRequest::default());
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);

    let Output::LlvmText(LlvmTextOutput { code }) = result.output.expect("LLVM output missing") else {
        panic!("expected LlvmText output");
    };
    assert!(!code.is_empty(), "LLVM text output is empty for incipit program");
    assert!(
        code.contains("define"),
        "LLVM output should contain a function definition:\n{code}"
    );
}

// ── WGSL backend (lane: mir-wgsl) ────────────────────────────────────────

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_emits_nonempty_output() {
    let session = Session::new(Config::default().with_target(Target::MirWgsl).with_latin_locale());
    let result = compile(&session, "smoke.fab", KERNEL_SOURCE, ModuleCompileRequest::default());
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);

    let Output::WgslText(WgslTextOutput { code, .. }) = result.output.expect("WGSL output missing") else {
        panic!("expected WgslText output");
    };
    assert!(!code.is_empty(), "WGSL text output is empty");
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_contains_fn_declaration() {
    let session = Session::new(Config::default().with_target(Target::MirWgsl).with_latin_locale());
    let result = compile(&session, "smoke.fab", KERNEL_SOURCE, ModuleCompileRequest::default());
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);

    let Output::WgslText(WgslTextOutput { code, .. }) = result.output.expect("WGSL output missing") else {
        panic!("expected WgslText output");
    };
    assert!(
        code.contains("fn "),
        "WGSL output should contain a function declaration:\n{code}"
    );
}

#[cfg(feature = "mir-wgsl")]
#[test]
fn wgsl_text_includes_reflection() {
    let session = Session::new(Config::default().with_target(Target::MirWgsl).with_latin_locale());
    let result = compile(&session, "smoke.fab", KERNEL_SOURCE, ModuleCompileRequest::default());
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);

    let Output::WgslText(WgslTextOutput { reflection, .. }) = result.output.expect("WGSL output missing") else {
        panic!("expected WgslText output");
    };
    assert!(reflection.is_some(), "WGSL output should include a GPU reflection sidecar");
}

// ── Metal backend (lane: mir-metal) ───────────────────────────────────────

#[cfg(feature = "mir-metal")]
#[test]
fn metal_text_emits_nonempty_output() {
    let session = Session::new(Config::default().with_target(Target::MirMetal).with_latin_locale());
    let result = compile(&session, "smoke.fab", KERNEL_SOURCE, ModuleCompileRequest::default());
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);

    let Output::MetalText(MetalTextOutput { code, .. }) = result.output.expect("Metal output missing") else {
        panic!("expected MetalText output");
    };
    assert!(!code.is_empty(), "Metal text output is empty");
}

#[cfg(feature = "mir-metal")]
#[test]
fn metal_text_contains_kernel_declaration() {
    let session = Session::new(Config::default().with_target(Target::MirMetal).with_latin_locale());
    let result = compile(&session, "smoke.fab", KERNEL_SOURCE, ModuleCompileRequest::default());
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);

    let Output::MetalText(MetalTextOutput { code, .. }) = result.output.expect("Metal output missing") else {
        panic!("expected MetalText output");
    };
    assert!(
        code.contains("kernel void"),
        "Metal output should contain a kernel declaration:\n{code}"
    );
}

// ── Wasm backend (lane: mir-wasm) ────────────────────────────────────────

#[cfg(feature = "mir-wasm")]
#[test]
fn wasm_text_emits_nonempty_output() {
    let session = Session::new(Config::default().with_target(Target::MirWasm).with_latin_locale());
    let result = compile(&session, "smoke.fab", ADD_SOURCE, ModuleCompileRequest::default());
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);

    let Output::WasmText(WasmTextOutput { code }) = result.output.expect("WasmText output missing") else {
        panic!("expected WasmText output");
    };
    assert!(!code.is_empty(), "Wasm text output is empty");
}

#[cfg(feature = "mir-wasm")]
#[test]
fn wasm_text_contains_module() {
    let session = Session::new(Config::default().with_target(Target::MirWasm).with_latin_locale());
    let result = compile(&session, "smoke.fab", ADD_SOURCE, ModuleCompileRequest::default());
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);

    let Output::WasmText(WasmTextOutput { code }) = result.output.expect("WasmText output missing") else {
        panic!("expected WasmText output");
    };
    assert!(
        code.contains("(module"),
        "Wasm text output should contain a module declaration:\n{code}"
    );
}

// ── S-expression backend (lane: mir-sexp) ─────────────────────────────────

#[cfg(feature = "mir-sexp")]
#[test]
fn sexp_text_emits_nonempty_output() {
    let session = Session::new(Config::default().with_target(Target::MirSexp).with_latin_locale());
    let result = compile(&session, "smoke.fab", ADD_SOURCE, ModuleCompileRequest::default());
    assert!(result.success(), "compile failed: {:?}", result.diagnostics);

    let Output::Sexp(SexpOutput { code }) = result.output.expect("Sexp output missing") else {
        panic!("expected Sexp output");
    };
    assert!(!code.is_empty(), "S-expression output is empty");
    assert!(
        code.contains("#lang racket"),
        "S-expression output should contain a Racket language declaration:\n{code}"
    );
}
