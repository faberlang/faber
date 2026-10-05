//! Focused execution-policy proof for `@ kernel` functions: the default route
//! refuses them, the CPU route (host kernel lowering plus the `Execute`
//! runner policy) runs them. Both directions are pinned.

use super::{BufferHost, RunSourceError, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};
use radix_mir_runner::RUNNER_KERNEL_REFUSED_ISSUE;

const MARKED_KERNEL_CALLED_FROM_MAIN: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn marked() → void {
}

main {
    marked()
}
"#;

const VALUE_KERNEL_CALLED_FROM_MAIN: &str = r#"+++
locale = "en"
+++

@ kernel
@ public
fn bump(f32 x) → f32 {
    return x + 1.0
}

main {
    print bump(1.0)
}
"#;

#[test]
fn kernel_function_called_from_incipit_is_refused_by_runner() {
    let mut host = BufferHost::default();
    let error = run_source(
        &Session::new(Config::default().with_target(Target::HirRust)),
        "kernel-ban.fab",
        MARKED_KERNEL_CALLED_FROM_MAIN,
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

#[test]
fn kernel_function_called_from_incipit_runs_on_the_cpu_route() {
    let mut host = BufferHost::default();
    let session = Session::new(
        Config::default()
            .with_target(Target::HirRust)
            .with_host_kernels(),
    );
    run_source(&session, "kernel-cpu.fab", MARKED_KERNEL_CALLED_FROM_MAIN, &mut host)
        .expect("the CPU route must run a @ kernel function");

    let mut host = BufferHost::default();
    run_source(&session, "kernel-cpu-value.fab", VALUE_KERNEL_CALLED_FROM_MAIN, &mut host)
        .expect("the CPU route must run a value-returning @ kernel function");
    assert_eq!(host.stdout_lines, vec!["2.0".to_owned()], "the kernel's value is printed");

    // The same source on the default route stays refused: the route, not the
    // source, decides.
    let mut refused_host = BufferHost::default();
    let error = run_source(
        &Session::new(Config::default().with_target(Target::HirRust)),
        "kernel-default-value.fab",
        VALUE_KERNEL_CALLED_FROM_MAIN,
        &mut refused_host,
    )
    .expect_err("the default route refuses the same program");
    let RunSourceError::Stepper(errors) = error else {
        panic!("expected a runner refusal: {error:?}");
    };
    assert_eq!(errors[0].issue, RUNNER_KERNEL_REFUSED_ISSUE);
    assert!(refused_host.stdout_lines.is_empty());
}
