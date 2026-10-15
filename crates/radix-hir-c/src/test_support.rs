//! Shared helpers for the C target tests.
//!
//! Each case compiles a Faber source through the module compiler with the `c`
//! target (a dev-dependency on `radix-module`) and checks the program text or
//! the named rejection. No test here runs a C compiler.

use crate::error::UNSUPPORTED_ISSUE;
use radix_module::codegen::Target;
use radix_module::driver::{Config, ModuleCompileRequest, Session, compile};
use radix_module::{CompileResult, Output};

pub(crate) fn compile_c(name: &str, source: &str) -> CompileResult {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirC)
            .with_dev_locale_root(),
    );
    compile(&session, name, source, ModuleCompileRequest::default())
}

pub(crate) fn program(name: &str, source: &str) -> String {
    match compile_c(name, source).output {
        Some(Output::C(out)) => out.code,
        _ => panic!("expected a C program for {name}"),
    }
}

/// The `construct` fact of the named rejection for `source`; asserts the
/// `issue` and `target` facts on the way.
pub(crate) fn rejected_construct(name: &str, source: &str) -> String {
    let result = compile_c(name, source);
    assert!(result.output.is_none(), "{name} must not emit a program");
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code == Some("CODEGEN001"))
        .unwrap_or_else(|| panic!("expected CODEGEN001, got {:?}", result.diagnostics));
    let arg = |wanted: &str| {
        diagnostic
            .args
            .iter()
            .find(|arg| arg.name == wanted)
            .map_or_else(
                || panic!("missing diagnostic arg {wanted}"),
                |arg| arg.value.clone(),
            )
    };
    assert_eq!(arg("issue"), UNSUPPORTED_ISSUE);
    assert_eq!(arg("target"), "c");
    arg("construct")
}

pub(crate) fn inline(body: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n{body}\n")
}

pub(crate) fn emitted(body: &str) -> String {
    program("case.fab", &inline(body))
}

pub(crate) fn rejected(body: &str) -> String {
    rejected_construct("case.fab", &inline(body))
}
