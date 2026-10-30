//! The single-source runner must honor compiler-owned contract environments.

use super::{BufferHost, RunSourceError, run_analyzed_with_kernel_lowering, run_source};
use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};
use crate::mir::KernelLowering;

const CONTRACT: &str = r#"+++
locale = "en"
+++
@ contract { target = "evm" }
class Arithmetic {
    @ public
    fn add(int a, int b) → int { return a + b }
}
main { print "ordinary-main" }
"#;

#[test]
fn single_source_run_refuses_contract_before_main_executes() {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let mut host = BufferHost::default();
    let error = run_source(&session, "contract-run.fab", CONTRACT, &mut host)
        .expect_err("an EVM requirement must stop interpreted execution");
    let RunSourceError::Frontend(diagnostics) = error else {
        panic!("contract environment is a frontend execution refusal: {error:?}");
    };
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == Some("PKG001")
                && diagnostic.issue() == Some("contract_target_mismatch")
                && diagnostic
                    .args
                    .iter()
                    .any(|arg| arg.name == "required" && arg.value == "evm")
                && diagnostic
                    .args
                    .iter()
                    .any(|arg| arg.name == "target" && arg.value == Target::MirStepper.as_str())
        }),
        "{diagnostics:?}"
    );
    assert!(host.stdout_lines.is_empty());
}

#[test]
fn analyzed_run_refuses_contract_before_lowering_or_main_execution() {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let mut unit = analyze_source(&session, "contract-analyzed.fab", CONTRACT, AnalyzeRequest::default())
        .expect("generic source inspection retains contract metadata");
    for kernel_lowering in [KernelLowering::Device, KernelLowering::Host] {
        let mut host = BufferHost::default();
        let diagnostics = run_analyzed_with_kernel_lowering(&mut unit, kernel_lowering, &mut host)
            .expect_err("an analyzed unit still requires its declared execution environment");
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "PKG001"
                    && diagnostic.issue == "contract_target_mismatch"
                    && diagnostic
                        .args
                        .iter()
                        .any(|(name, value)| name == "required" && value == "evm")
                    && diagnostic
                        .args
                        .iter()
                        .any(|(name, value)| name == "target" && value == Target::MirStepper.as_str())
            }),
            "{diagnostics:?}"
        );
        assert!(host.stdout_lines.is_empty());
    }
}
