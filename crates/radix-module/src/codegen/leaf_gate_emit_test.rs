//! N3: a missing leaf fails at compile time with `leaf_unsupported_on_target`.

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const INLINE_FS: &str = r#"
incipit {
    sit textus body ← ad 'solum:lege' ("x") ↦ textus
    nota body
}
"#;

fn compile(target: Target, source: &str) -> crate::CompileResult {
    let session = Session::new(Config::default().with_target(target).with_dev_locale_root());
    crate::driver::compile(&session, "leaf-gate.fab", source, ModuleCompileRequest::default())
}

fn issue_values(result: &crate::CompileResult) -> Vec<String> {
    result
        .diagnostics
        .iter()
        .filter_map(|diag| diag.issue().map(str::to_owned))
        .collect()
}

#[cfg(feature = "hir-rust")]
#[test]
fn rust_admits_inline_fs_leaf() {
    let result = compile(Target::HirRust, INLINE_FS);
    assert!(result.success(), "rust must declare fs: {:?}", issue_values(&result));
}

#[test]
fn go_rejects_inline_fs_leaf() {
    let result = compile(Target::HirGo, INLINE_FS);
    assert!(!result.success(), "go must refuse fs");
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.issue() == Some("leaf_unsupported_on_target")),
        "expected leaf_unsupported_on_target, got {:?}",
        issue_values(&result)
    );
}
