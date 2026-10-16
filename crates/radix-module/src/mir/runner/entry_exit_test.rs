//! Proof: a single-source run applies the entry `exit` clause on the plain
//! runner route (`faber run <file>` with no `@ cli` and no non-kernel import).
//! Ruling Q-T8: `exit <expr>` on an entry header is lowered on every route, so
//! `main exit 3 { ... }` must reach `Host::exit` with 3 even though the unit is
//! not a CLI package. A body trap still wins: the trap surfaces as a runner
//! error and its own code reaches the host before the entry tail runs.

use super::{BufferHost, RunSourceError, RunnerError, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn run_host(name: &str, source: &str, host: &mut BufferHost) -> Result<(), Vec<RunnerError>> {
    match run_source(
        &Session::new(Config::default().with_target(Target::HirRust)),
        name,
        source,
        host,
    ) {
        Ok(()) => Ok(()),
        Err(RunSourceError::Stepper(errors)) => Err(errors),
        Err(error) => panic!("{name} should reach the MIR runner: {error:?}"),
    }
}

/// English reader surface, matching the frontmatter shape of the route-serve
/// fixtures (`main`/`let`/`print`/`exit` are the `en` spellings).
fn program(body: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n{body}\n")
}

#[test]
fn entry_exit_clause_reaches_the_process_host() {
    let mut host = BufferHost::default();
    let source = program("main exit 3 { print \"hi\" }");
    let panic = catch_unwind(AssertUnwindSafe(|| {
        let _ = run_host("entry-exit.fab", &source, &mut host);
    }))
    .expect_err("main exit 3 must reach Host::exit");
    let message = panic
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| panic.downcast_ref::<&str>().map(|text| (*text).to_owned()))
        .unwrap_or_default();
    assert_eq!(message, "runner exit: 3");
    assert!(
        host.stdout_lines.iter().any(|line| line.contains("hi")),
        "the body must run before the entry exit: {:?}",
        host.stdout_lines
    );
}

#[test]
fn entry_exit_does_not_override_a_body_trap() {
    let mut host = BufferHost::default();
    let source = program("main exit 3 {\n  let int x ← 0\n  print 100 / x\n}");
    let errors = run_host("entry-exit-trap.fab", &source, &mut host)
        .expect_err("a body trap must surface as a runner error, not an entry exit");
    assert!(!errors.is_empty(), "a division by zero must produce a runner error");
    assert_eq!(
        errors[0].code, "runner_internal",
        "the trap keeps its own identity: {:?}",
        errors[0]
    );
}
