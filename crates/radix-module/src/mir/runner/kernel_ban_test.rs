//! Focused execution-policy proof for the device annotation ban.

use super::{run_source, BufferHost, RunSourceError};
use crate::codegen::Target;
use crate::driver::{Config, Session};
use radix_mir_runner::RUNNER_KERNEL_REFUSED_ISSUE;

#[test]
fn kernel_function_called_from_incipit_is_refused_by_runner() {
    let source = r#"+++
locale = "en"
+++

@ kernel
fn marked() → void {
    print "must not execute"
}

main {
    marked()
}
"#;
    let mut host = BufferHost::default();
    let error = run_source(
        &Session::new(Config::default().with_target(Target::HirRust)),
        "kernel-ban.fab",
        source,
        &mut host,
    )
    .expect_err("the MIR runner must refuse a call to a @ kernel function");

    let RunSourceError::Stepper(errors) = error else {
        panic!("kernel execution should fail in the runner: {error:?}");
    };
    assert_eq!(errors.len(), 1, "the refusal should be the only runner error");
    assert_eq!(errors[0].issue, RUNNER_KERNEL_REFUSED_ISSUE);
    assert!(host.stdout_lines.is_empty(), "refused code must not execute");
}
