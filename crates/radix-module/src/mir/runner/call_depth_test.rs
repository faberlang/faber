//! Recursion depth in the MIR runner: a supported depth runs whatever the host
//! thread's stack size, and a deeper call stops with a runner diagnostic
//! instead of a native stack overflow abort.

use super::{BufferHost, RunSourceError, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};
use radix_mir_runner::{MAX_CALL_DEPTH, RUNNER_ISSUE_CALL_DEPTH_EXCEEDED};

fn recursion_source(depth: usize) -> String {
    format!(
        r#"+++
locale = "en"
+++
fn down(int n) → int {{
    if n ≤ 0 {{
        return 0
    }}
    return 1 + down(n - 1)
}}

main {{
    print down({depth})
}}
"#
    )
}

/// Run on a thread whose own stack is far too small for the recursion, so the
/// result shows the runner grows native stack itself.
fn run_on_small_stack(source: String) -> (Result<(), RunSourceError>, Vec<String>) {
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            let mut host = BufferHost::default();
            let result = run_source(
                &Session::new(Config::default().with_target(Target::HirRust)),
                "call-depth.fab",
                &source,
                &mut host,
            );
            (result, host.stdout_lines)
        })
        .expect("spawn small-stack thread")
        .join()
        .expect("the runner must not overflow the native stack")
}

#[test]
fn recursion_to_the_depth_limit_runs_on_a_small_thread_stack() {
    let depth = MAX_CALL_DEPTH - 2;
    let (result, stdout) = run_on_small_stack(recursion_source(depth));
    result.expect("recursion within the call depth limit runs");
    assert_eq!(stdout, vec![depth.to_string()]);
}

#[test]
fn recursion_past_the_depth_limit_stops_with_a_runner_diagnostic() {
    let (result, stdout) = run_on_small_stack(recursion_source(MAX_CALL_DEPTH + 1));
    let Err(RunSourceError::Stepper(errors)) = result else {
        panic!("recursion past the limit must fail in the runner: {result:?}");
    };
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].issue, RUNNER_ISSUE_CALL_DEPTH_EXCEEDED);
    assert!(stdout.is_empty());
}
