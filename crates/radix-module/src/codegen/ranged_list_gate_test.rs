//! Need: a list length range `lista<T, lo‥hi>` is applied by the
//! Rust target, the Faber source emitter and the MIR runner. Every other
//! emitter would read the cell as a plain list and drop the range, so each
//! rejects a module that names one with the named
//! `ranged_list_target_unsupported` diagnostic before emission.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const SOURCE: &str = "functio f(lista<numerus, 1…∞> a) → lista<numerus, 1…∞> { redde a }\nincipit {\n}\n";

fn issues(target: Target) -> Vec<String> {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(target)
            .with_dev_locale_root(),
    );
    let result = crate::driver::compile(&session, "<test>", SOURCE, ModuleCompileRequest::default());
    result
        .diagnostics
        .iter()
        .flat_map(|diagnostic| {
            diagnostic
                .args
                .iter()
                .filter(|arg| arg.name == "issue")
                .map(|arg| arg.value.clone())
        })
        .filter(|issue| issue == "ranged_list_target_unsupported")
        .collect()
}

#[cfg(feature = "hir-go")]
#[test]
fn go_emitter_refuses_a_list_length_range() {
    assert_eq!(issues(Target::HirGo), vec!["ranged_list_target_unsupported".to_owned()]);
}

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_emitter_refuses_a_list_length_range() {
    assert_eq!(issues(Target::HirTypeScript), vec!["ranged_list_target_unsupported".to_owned()]);
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_emitter_refuses_a_list_length_range() {
    assert_eq!(issues(Target::HirSwift), vec!["ranged_list_target_unsupported".to_owned()]);
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_emitter_keeps_a_list_length_range() {
    assert_eq!(issues(Target::HirRust), Vec::<String>::new());
}

#[cfg(feature = "hir-faber")]
#[test]
fn faber_emitter_keeps_a_list_length_range() {
    assert_eq!(issues(Target::HirFaber), Vec::<String>::new());
}
