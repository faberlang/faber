//! Top-level statics reach every source backend as their folded constants
//! (DECISIONS D5.8, D5.9).
//!
//! Semantic analysis evaluates each `generis` static at compile time; the
//! backends read the folded initializer through the ordinary top-level
//! constant path, so no target evaluates the authored arithmetic.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const SOURCE: &str = "generis numerus LIMES = 4 * 1024\n\
generis numerus DUPLEX = LIMES * 2\n\
generis textus NOMEN = \"lim\" + \"es\"\n\
incipit {\n    nota LIMES\n    nota DUPLEX\n    nota NOMEN\n}\n";

fn emit(target: Target) -> String {
    let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
    let result = crate::driver::compile(&session, "<test>", SOURCE, ModuleCompileRequest::default());
    assert!(result.success(), "{target:?} compilation failed: {:?}", result.diagnostics);
    match result.output.expect("output") {
        crate::Output::Rust(out) => out.code,
        crate::Output::Go(out) => out.code,
        crate::Output::TypeScript(out) => out.code,
        crate::Output::Swift(out) => out.code,
        _ => panic!("unexpected output kind for {target:?}"),
    }
}

fn assert_folded(target: Target) {
    let code = emit(target);
    assert!(code.contains("4096"), "{target:?} lacks folded LIMES:\n{code}");
    assert!(code.contains("8192"), "{target:?} lacks folded DUPLEX:\n{code}");
    assert!(code.contains("\"limes\""), "{target:?} lacks folded NOMEN:\n{code}");
    assert!(!code.contains("1024"), "{target:?} evaluates authored arithmetic:\n{code}");
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_emits_folded_statics() {
    assert_folded(Target::HirRust);
}

#[cfg(feature = "hir-go")]
#[test]
fn go_emits_folded_statics() {
    assert_folded(Target::HirGo);
}

#[cfg(feature = "hir-ts")]
#[test]
fn ts_emits_folded_statics() {
    assert_folded(Target::HirTypeScript);
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_emits_folded_statics() {
    let code = emit(Target::HirSwift);
    assert!(code.contains("let LIMES: Int = 4096"), "swift static:\n{code}");
    assert_folded(Target::HirSwift);
}
