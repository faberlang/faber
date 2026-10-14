//! Need (unit L1): the ledger type `tabulae<K, V>` has no carrier on
//! any emitter yet. Every emitter rejects a module that names one with the
//! named `ledger_type_unsupported_on_target` diagnostic before emission; the
//! Faber source emitter keeps it.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const SOURCE: &str = "functio f(tabulae<textus, numerus> a) → tabulae<textus, numerus> { redde a }\nincipit {\n}\n";

fn issues(target: Target) -> Vec<String> {
    let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
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
        .filter(|issue| issue == "ledger_type_unsupported_on_target")
        .collect()
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_emitter_refuses_a_ledger_type() {
    assert_eq!(issues(Target::HirRust), vec!["ledger_type_unsupported_on_target".to_owned()]);
}

#[cfg(feature = "hir-go")]
#[test]
fn go_emitter_refuses_a_ledger_type() {
    assert_eq!(issues(Target::HirGo), vec!["ledger_type_unsupported_on_target".to_owned()]);
}

#[cfg(feature = "hir-ts")]
#[test]
fn typescript_emitter_refuses_a_ledger_type() {
    assert_eq!(
        issues(Target::HirTypeScript),
        vec!["ledger_type_unsupported_on_target".to_owned()]
    );
}

#[cfg(feature = "hir-swift")]
#[test]
fn swift_emitter_refuses_a_ledger_type() {
    assert_eq!(issues(Target::HirSwift), vec!["ledger_type_unsupported_on_target".to_owned()]);
}

#[cfg(feature = "hir-faber")]
#[test]
fn faber_emitter_keeps_a_ledger_type() {
    assert_eq!(issues(Target::HirFaber), Vec::<String>::new());
}

/// The en spelling `ledger<K, V>` parses and type-checks; an operation on a
/// ledger value that is not admitted by need is refused by name.
#[cfg(feature = "hir-faber")]
fn en_compile(body: &str) -> (bool, Vec<String>) {
    let source = format!("+++\nlocale = \"en\"\n+++\n{body}");
    let session = Session::new(
        Config::default()
            .with_target(Target::HirFaber)
            .with_dev_locale_root(),
    );
    let result = crate::driver::compile(&session, "<test>", &source, ModuleCompileRequest::default());
    let issues = result
        .diagnostics
        .iter()
        .flat_map(|diagnostic| {
            diagnostic
                .args
                .iter()
                .filter(|arg| arg.name == "issue")
                .map(|arg| arg.value.clone())
        })
        .collect();
    (result.success(), issues)
}

#[cfg(feature = "hir-faber")]
#[test]
fn en_ledger_spelling_resolves_as_a_parameter_and_a_return_type() {
    let (success, issues) = en_compile("fn f(ledger<string, int> l) → ledger<string, int> {\n    return l\n}\n");
    assert!(success, "issues: {issues:?}");
}

#[cfg(feature = "hir-faber")]
#[test]
fn en_ledger_print_is_refused_by_name() {
    let (_, issues) = en_compile("fn f(ledger<string, int> l) {\n    print l\n}\n");
    assert!(
        issues
            .iter()
            .any(|issue| issue == "ledger_operation_not_implemented"),
        "issues: {issues:?}"
    );
}
