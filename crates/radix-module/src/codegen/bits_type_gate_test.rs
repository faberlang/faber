//! Need: the raw-bits types `b8 b16 b32 b64` are implemented only by
//! the Faber source emitter and the MIR runner. Every other emitter rejects a
//! module that names one with the named `bits_type_unsupported_on_target`
//! diagnostic before emission, instead of printing a placeholder identifier
//! or a plain integer carrier.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const SOURCE: &str = "functio f(b8 a) → b8 { redde a }\nincipit {\n}\n";

fn diagnostics(target: Target) -> Vec<(String, String)> {
    let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
    let result = crate::driver::compile(&session, "<test>", SOURCE, ModuleCompileRequest::default());
    result
        .diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "bits_type_unsupported_on_target")
        })
        .map(|diagnostic| {
            let value = |name: &str| {
                diagnostic
                    .args
                    .iter()
                    .find(|arg| arg.name == name)
                    .map(|arg| arg.value.clone())
                    .unwrap_or_default()
            };
            (value("type"), value("target"))
        })
        .collect()
}

fn assert_rejected(target: Target, spelled: &str) {
    let found = diagnostics(target);
    assert_eq!(
        found,
        vec![("b8".to_owned(), spelled.to_owned())],
        "{target:?} did not reject b8 with the named diagnostic"
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_rejects_bits_before_emission() {
    assert_rejected(Target::HirRust, "HirRust");
}

#[cfg(feature = "hir-go")]
#[test]
fn go_rejects_bits_before_emission() {
    assert_rejected(Target::HirGo, "HirGo");
}

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_rejects_bits_before_emission() {
    assert_rejected(Target::HirTypeScript, "HirTypeScript");
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_rejects_bits_before_emission() {
    assert_rejected(Target::HirSwift, "HirSwift");
}

#[cfg(feature = "mir-llvm")]
#[test]
fn llvm_rejects_bits_before_emission() {
    assert_rejected(Target::MirLlvm, "MirLlvm");
}

#[cfg(feature = "mir-wasm")]
#[test]
fn wasm_rejects_bits_before_emission() {
    assert_rejected(Target::MirWasm, "MirWasm");
}

#[cfg(feature = "mir-sexp")]
#[test]
fn sexp_rejects_bits_before_emission() {
    assert_rejected(Target::MirSexp, "MirSexp");
}

#[cfg(feature = "hir-faber")]
#[test]
fn faber_source_emitter_keeps_bits() {
    assert_eq!(diagnostics(Target::HirFaber), Vec::<(String, String)>::new());
}
