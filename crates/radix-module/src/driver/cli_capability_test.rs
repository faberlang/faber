//! CLI-entry capability gate: `Target::lowers_cli_entry` drives the
//! `cli_codegen_rust_only` (CODEGEN002) refusal in the driver.

use super::compile;
use crate::codegen::Target;
use crate::diagnostics::Diagnostic;
use crate::driver::ModuleCompileRequest;
use crate::test_support::test_session_la;

const CLI_SOURCE: &str = r#"
@ cli "tool"
incipit argumenta args { }
"#;

fn compile_cli(target: Target) -> Vec<Diagnostic> {
    let session = test_session_la(target);
    compile(&session, "cli.fab", CLI_SOURCE, ModuleCompileRequest::default()).diagnostics
}

fn refused_by_capability_gate(diagnostics: &[Diagnostic]) -> bool {
    diagnostics
        .iter()
        .any(|d| d.code == Some("CODEGEN002") && d.issue() == Some("cli_codegen_rust_only"))
}

#[test]
fn cli_capability_gate_refuses_incapable_targets_by_name() {
    for target in [Target::HirPython, Target::HirSwift] {
        if !target.is_available() {
            continue;
        }
        assert!(!target.lowers_cli_entry(), "{target} must not declare CLI entry lowering");
        let diagnostics = compile_cli(target);
        assert!(
            refused_by_capability_gate(&diagnostics),
            "{target} must be refused with CODEGEN002 cli_codegen_rust_only: {diagnostics:?}"
        );
        let refusal = diagnostics
            .iter()
            .find(|d| d.code == Some("CODEGEN002"))
            .expect("refusal diagnostic present");
        assert!(
            refusal
                .args
                .iter()
                .any(|arg| arg.name == "target" && arg.value == target.as_str()),
            "refusal must name {target}: {refusal:?}"
        );
    }
}

#[test]
fn cli_capability_gate_admits_capable_targets() {
    for target in [Target::HirRust, Target::HirGo, Target::HirTypeScript] {
        if !target.is_available() {
            continue;
        }
        assert!(target.lowers_cli_entry(), "{target} must declare CLI entry lowering");
        let diagnostics = compile_cli(target);
        assert!(
            !refused_by_capability_gate(&diagnostics),
            "{target} must not be refused by the capability gate: {diagnostics:?}"
        );
    }
}
