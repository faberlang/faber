//! opt-lit emission pin: a list literal stored into a `lista<T> ∪ nihil` slot
//! emits as the typed list wrapped in the optional, never as a `[]any` that
//! the pointer-shaped Go optional cannot hold.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const SOURCE: &str = r"
functio crea() → lista<numerus> ∪ nihil { redde [1, 2, 3] }

functio vacat() → lista<numerus> ∪ nihil { redde vacua }

incipit {
    sit lista<numerus> ∪ nihil a ← [10, 20]
    nota a
}
";

fn emit(target: Target) -> String {
    let session = Session::new(Config::default().with_target(target).with_latin_locale());
    let result = crate::driver::compile(&session, "optional-literal.fab", SOURCE, ModuleCompileRequest::default());
    assert!(result.success(), "{target} compilation failed: {:?}", result.diagnostics);
    match result.output.expect("output") {
        crate::Output::Go(out) => out.code,
        crate::Output::Rust(out) => out.code,
        _ => panic!("unexpected output for {target}"),
    }
}

#[test]
fn go_wraps_an_optional_list_literal_as_a_typed_list() {
    let code = emit(Target::HirGo);
    assert!(code.contains("v := []int{1, 2, 3}; return &v"), "{code}");
    assert!(code.contains("v := []int{10, 20}; return &v"), "{code}");
    assert!(!code.contains("[]any"), "{code}");
}

#[test]
fn rust_wraps_an_optional_list_literal_in_some() {
    let code = emit(Target::HirRust);
    assert!(code.contains("Some(vec![1, 2, 3])"), "{code}");
}

#[test]
fn go_wraps_an_optional_vacua_as_a_typed_empty_list() {
    let code = emit(Target::HirGo);
    assert!(code.contains("v := make([]int, 0); return &v"), "{code}");
}

#[test]
fn rust_wraps_an_optional_vacua_in_some() {
    let code = emit(Target::HirRust);
    assert!(code.contains("Some(Vec::new())"), "{code}");
}
