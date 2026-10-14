//! F9 ruling 28 and unit pot2: float `potentia` is IEEE `pow` on every source
//! backend, and integer `potentia` traps on a negative exponent.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const FLOAT_POW: &str = r"incipit {
    fixum fractus b ← -8.0
    fixum fractus r ← b.potentia(0.5)
    nota r
    nota 2.0.potentia(3.0)
}";

const FLOAT_LITERAL_POW: &str = r"incipit {
    fixum fractus r ← 2.0.potentia(3.0)
    nota r
}";

fn compile(target: Target, source: &str) -> crate::Output {
    let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
    let result = crate::driver::compile(&session, "<test>", source, ModuleCompileRequest::default());
    assert!(result.success(), "compilation failed: {:?}", result.diagnostics);
    result.output.expect("output")
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_float_potentia_is_foundation_pow() {
    let crate::Output::Swift(out) = compile(Target::HirSwift, FLOAT_POW) else {
        panic!("expected Swift output");
    };
    assert!(out.code.contains("pow(l_b, 0.5)"), "{}", out.code);
    assert!(out.code.contains("pow(2.0, 3.0)"), "{}", out.code);
    assert!(!out.code.contains(".potentia("), "{}", out.code);
    assert!(out.code.contains("import Foundation"), "pow needs Foundation:\n{}", out.code);
}

#[cfg(feature = "hir-python")]
#[test]
fn python_float_potentia_is_the_ieee_pow_helper() {
    let crate::Output::Python(out) = compile(Target::HirPython, FLOAT_POW) else {
        panic!("expected Python output");
    };
    assert!(out.code.contains("_faber_fpow(b, 0.5)"), "{}", out.code);
    assert!(!out.code.contains(".potentia("), "{}", out.code);
}

#[cfg(feature = "hir-go")]
#[test]
fn go_float_display_never_suffixes_a_non_finite_value() {
    let crate::Output::Go(out) = compile(Target::HirGo, FLOAT_POW) else {
        panic!("expected Go output");
    };
    let nan = out.code.find("s == \"NaN\"").expect("NaN guard");
    let suffix = out
        .code
        .find("if !strings.Contains(s, \".\")")
        .expect("whole-float branch");
    assert!(nan < suffix, "the guard precedes the whole-float suffix:\n{}", out.code);
    assert!(out.code.contains("s == \"+Inf\"") && out.code.contains("s == \"-Inf\""));
}

const INTEGER_POW: &str = r"functio minus() → numerus {
    redde -1
}

incipit {
    fixum numerus base ← 2
    nota base.potentia(minus())
}";

#[cfg(feature = "mir-sexp")]
#[test]
fn sexp_integer_potentia_traps_through_the_panic_intrinsic() {
    let crate::Output::Sexp(out) = compile(Target::MirSexp, INTEGER_POW) else {
        panic!("expected sexp output");
    };
    assert!(
        out.code
            .contains("numerus potentia failed: negative exponent"),
        "the negative-exponent trap carries its message:\n{}",
        out.code
    );
}

#[cfg(feature = "mir-llvm")]
#[test]
fn llvm_integer_potentia_carries_the_trap_message() {
    let crate::Output::LlvmText(out) = compile(Target::MirLlvm, INTEGER_POW) else {
        panic!("expected LLVM output");
    };
    assert!(
        out.code
            .contains("numerus potentia failed: negative exponent"),
        "{}",
        out.code
    );
}

#[cfg(feature = "mir-wasm")]
#[test]
fn wasm_integer_potentia_carries_the_trap_message() {
    let crate::Output::WasmText(out) = compile(Target::MirWasm, INTEGER_POW) else {
        panic!("expected wasm output");
    };
    assert!(
        out.code
            .contains("numerus potentia failed: negative exponent"),
        "{}",
        out.code
    );
}

#[cfg(feature = "hir-ts")]
#[test]
fn ts_float_literal_receiver_is_parenthesized() {
    let crate::Output::TypeScript(out) = compile(Target::HirTypeScript, FLOAT_LITERAL_POW) else {
        panic!("expected TypeScript output");
    };
    assert!(
        !out.code.contains("2.potentia(") && !out.code.contains("2.0.potentia("),
        "a numeric literal receiver needs parentheses:\n{}",
        out.code
    );
}
