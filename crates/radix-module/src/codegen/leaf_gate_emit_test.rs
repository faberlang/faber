//! N3: a missing leaf fails at compile time with `leaf_unsupported_on_target`.
//!
//! Every case here compiles for a target leaf (`hir-rust`/`hir-go`), so the
//! whole module compiles out of builds without those leaves (the public cut).
#![cfg(any(feature = "hir-rust", feature = "hir-go"))]

use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};

const INLINE_FS: &str = r#"
incipit {
    sit textus body ← ad 'solum:lege' ("x") ↦ textus
    nota body
}
"#;

fn compile(target: Target, source: &str) -> crate::CompileResult {
    let session = Session::new(
        Config::default()
            .with_latin_locale()
            .with_target(target)
            .with_dev_locale_root(),
    );
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

#[cfg(feature = "hir-go")]
#[test]
fn go_admits_inline_clock_leaf() {
    const SOURCE: &str = r#"
incipit {
    sit numerus nunc ← ad 'tempus:monotonicum' ↦ numerus
    nota nunc
}
"#;
    let result = compile(Target::HirGo, SOURCE);
    assert!(result.success(), "go must declare clock: {:?}", issue_values(&result));
}

#[cfg(feature = "hir-go")]
#[test]
fn go_admits_inline_entropy_leaf() {
    const SOURCE: &str = r#"
incipit {
    sit numerus sors ← ad 'aleator:sortire' ([0, 10]) ↦ numerus
    nota sors
}
"#;
    let result = compile(Target::HirGo, SOURCE);
    assert!(result.success(), "go must declare entropy: {:?}", issue_values(&result));
}

#[cfg(feature = "hir-go")]
#[test]
fn go_admits_inline_stdio_leaf() {
    const SOURCE: &str = r#"
incipit {
    sit textus linea ← ad 'consolum:lege' ↦ textus
    nota linea
}
"#;
    let result = compile(Target::HirGo, SOURCE);
    assert!(result.success(), "go must declare stdio: {:?}", issue_values(&result));
}

#[cfg(feature = "hir-go")]
#[test]
fn go_admits_inline_process_leaf() {
    const SOURCE: &str = r#"
incipit {
    sit numerus identitas ← ad 'processus:identitas' ↦ numerus
    nota identitas
}
"#;
    let result = compile(Target::HirGo, SOURCE);
    assert!(result.success(), "go must declare process: {:?}", issue_values(&result));
}

#[cfg(feature = "hir-go")]
#[test]
fn go_declares_inline_fs_leaf() {
    let result = compile(Target::HirGo, INLINE_FS);
    assert!(result.success(), "go must declare fs: {:?}", issue_values(&result));
}
