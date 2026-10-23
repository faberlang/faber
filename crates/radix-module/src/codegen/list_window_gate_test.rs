//! Need, ruling B10: the list window `xs[lo‥hi]` is executed only by
//! the MIR runner and re-printed by the Faber emitter. Every other emitter
//! rejects a module that takes one with the named
//! `list_window_unsupported_on_target` diagnostic instead of mis-emitting a
//! scalar read or failing with an unnamed error.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const SOURCE: &str = "incipit {\n    sit lista<numerus> xs ← [1, 2, 3]\n    nota xs[0‥2]\n}\n";

fn issues(target: Target) -> Vec<String> {
    let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
    let result = crate::driver::compile(&session, "<test>", SOURCE, ModuleCompileRequest::default());
    result
        .diagnostics
        .iter()
        .flat_map(|diagnostic| diagnostic.args.iter())
        .filter(|arg| arg.name == "issue")
        .map(|arg| arg.value.clone())
        .collect()
}

fn assert_rejected(target: Target) {
    assert_eq!(
        issues(target),
        vec!["list_window_unsupported_on_target".to_owned()],
        "{target:?} did not reject the list window by name"
    );
}

/// The Rust emitter copies the window of any list, so the gate admits it.
#[cfg(feature = "hir-rust")]
#[test]
fn rust_admits_the_list_window() {
    assert!(issues(Target::HirRust).is_empty());
}

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_admits_the_list_window() {
    assert!(issues(Target::HirTypeScript).is_empty());
}

#[cfg(feature = "hir-go")]
#[test]
fn go_admits_the_list_window() {
    assert!(issues(Target::HirGo).is_empty());
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_admits_the_list_window() {
    assert!(issues(Target::HirSwift).is_empty());
}

#[cfg(feature = "hir-python")]
#[test]
fn python_admits_the_list_window() {
    assert!(issues(Target::HirPython).is_empty());
}

#[cfg(feature = "mir-sexp")]
#[test]
fn sexp_rejects_the_list_window_before_emission() {
    assert_rejected(Target::MirSexp);
}

#[cfg(feature = "mir-llvm")]
#[test]
fn llvm_rejects_the_list_window_before_emission() {
    assert_rejected(Target::MirLlvm);
}

#[cfg(feature = "hir-faber")]
#[test]
fn faber_reemit_keeps_the_list_window() {
    assert!(issues(Target::HirFaber).is_empty());
}
