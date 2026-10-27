//! Top-level statics reach every source backend as their folded constants
//! (DECISIONS D5.8, D5.9).
//!
//! Semantic analysis evaluates each `generis` static at compile time; the
//! backends read the folded initializer through the ordinary top-level
//! constant path, so no target evaluates the authored arithmetic.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const SOURCE: &str = "fixum numerus LIMES = 4 * 1024\n\
fixum numerus DUPLEX = LIMES * 2\n\
fixum textus NOMEN = \"lim\" + \"es\"\n\
incipit {\n    nota LIMES\n    nota DUPLEX\n    nota NOMEN\n}\n";

fn emit(target: Target) -> String {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(target)
            .with_dev_locale_root(),
    );
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
    assert!(code.contains("8192"), "{target:?} lacks folded doubled limit:\n{code}");
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

/// A Rust const item cannot build a heap `Vec` (E0015): a static `octeti`
/// (a byte literal or a D8.10 file embed) is a `&'static [u8]`, and use sites
/// own it with `.to_vec()` like a const list.
#[cfg(feature = "hir-rust")]
#[test]
fn rust_emits_octeti_static_as_byte_slice() {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(Target::HirRust)
            .with_dev_locale_root(),
    );
    let source = "fixum lista<b8> SIGNUM = |de ad|\nincipit {\n    nota SIGNUM.longitudo()\n}\n";
    let result = crate::driver::compile(&session, "<test>", source, ModuleCompileRequest::default());
    assert!(result.success(), "compilation failed: {:?}", result.diagnostics);
    let crate::Output::Rust(out) = result.output.expect("output") else {
        panic!("expected rust output");
    };
    assert!(
        out.code
            .contains("pub const SIGNUM: &'static [u8] = &[0xde, 0xad];"),
        "octeti static:\n{}",
        out.code
    );
    assert!(!out.code.contains("vec![0xde"), "heap vec in const:\n{}", out.code);
}
