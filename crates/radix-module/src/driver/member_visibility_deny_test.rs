//! D5.5 interim — redundant member `@ publica` (WARN028) is a warning by
//! default and an error under deny-all-warnings.

use super::compile;
use crate::CompileResult;
use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session, WarnPolicy};

const MEMBER_PUBLICA_SOURCE: &str = r#"
genus Demo {
    @ publica
    varia textus nomen
}
incipit {
    nota "salve"
}
"#;

fn compile_with_policy(policy: WarnPolicy) -> CompileResult {
    let config = Config::default()
        .with_target(Target::HirRust)
        .with_warn_policy(policy)
        .with_latin_locale();
    compile(
        &Session::new(config),
        "test.fab",
        MEMBER_PUBLICA_SOURCE,
        ModuleCompileRequest::default(),
    )
}

#[test]
fn member_publica_is_a_warning_by_default() {
    let result = compile_with_policy(WarnPolicy::default());
    assert!(result.success(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.code == Some("WARN028") && !diag.is_error()),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn member_publica_is_an_error_under_deny_all_warnings() {
    let result = compile_with_policy(WarnPolicy { deny_all_warnings: true, deny_codes: vec![] });
    assert!(!result.success(), "{:?}", result.diagnostics);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.code == Some("WARN028") && diag.is_error()),
        "{:?}",
        result.diagnostics
    );
}
