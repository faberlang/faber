use super::*;

fn arg<'a>(diag: &'a Diagnostic, name: &str) -> Option<&'a str> {
    diag.args
        .iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.as_str())
}

#[test]
fn diagnostic_carries_issue_code_target_and_help() {
    for target in [
        Target::HirRust,
        Target::HirTypeScript,
        Target::HirGo,
        Target::MirWasm,
        Target::MirLlvmHost,
        Target::MirMetal,
    ] {
        let diag = target_not_in_this_build(target);
        assert_eq!(diag.issue(), Some(TARGET_NOT_IN_THIS_BUILD));
        assert_eq!(diag.code, Some("PKG001"));
        assert_eq!(arg(&diag, "target"), Some(target.as_str()));
        assert!(diag.help.is_some());
    }
}

#[test]
fn ungated_stepper_is_always_compiled_in() {
    assert!(require_target_compiled_in(Target::MirStepper).is_ok());
}

#[test]
#[cfg(not(feature = "hir-go"))]
fn go_target_is_gated_without_feature() {
    let err = require_target_compiled_in(Target::HirGo).expect_err("hir-go is off");
    assert_eq!(err.issue(), Some(TARGET_NOT_IN_THIS_BUILD));
}

#[test]
#[cfg(feature = "hir-go")]
fn go_target_is_compiled_in_with_feature() {
    assert!(require_target_compiled_in(Target::HirGo).is_ok());
}

#[test]
#[cfg(not(feature = "hir-rust"))]
fn rust_target_is_gated_without_feature() {
    let err = require_target_compiled_in(Target::HirRust).expect_err("hir-rust is off");
    assert_eq!(err.issue(), Some(TARGET_NOT_IN_THIS_BUILD));
}

#[test]
#[cfg(feature = "hir-rust")]
fn rust_target_is_compiled_in_with_feature() {
    assert!(require_target_compiled_in(Target::HirRust).is_ok());
}

#[test]
fn experimental_target_help_differs_from_release_target_help() {
    let experimental = target_not_in_this_build(Target::HirPascal);
    let release = target_not_in_this_build(Target::HirGo);
    assert_ne!(experimental.help, release.help);
}
