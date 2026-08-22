use crate::script::{
    interpret_source, print_run_source_error, run_named, run_source, BufferHost, RunSourceError,
};
use std::process::ExitCode;

#[test]
fn interpret_source_runs_one_liner_fixture() {
    let mut host = BufferHost::default();
    interpret_source(
        "test.fab",
        r#"
main {
    print "salve"
}
"#,
        &mut host,
    )
    .expect("one-liner fixture runs");
    assert_eq!(host.stdout_lines, vec!["salve".to_owned()]);
}

#[test]
fn print_run_source_error_accepts_runner_failures() {
    let error = RunSourceError::Stepper(vec![radix::module::mir::RunnerError {
        message: "FaberScript unsupported: async".to_owned(),
        issue: "runner_unsupported".to_owned(),
        span: radix::module::lexer::Span::default(),
    }]);
    print_run_source_error(&error);
}

#[test]
fn run_source_executes_salve_fixture() {
    let mut host = BufferHost::default();
    let status = run_source(
        r#"
main {
    print "salve"
}
"#,
        &mut host,
    )
    .expect("embed run succeeds");
    assert_eq!(status, ExitCode::SUCCESS);
    assert_eq!(host.stdout_lines, vec!["salve".to_owned()]);
}

#[test]
fn run_named_uses_diagnostic_identity() {
    let mut host = BufferHost::default();
    let status = run_named(
        "embed.fab",
        r"
main {
    print 1
}
",
        &mut host,
    )
    .expect("named embed run succeeds");
    assert_eq!(status, ExitCode::SUCCESS);
    assert_eq!(host.stdout_lines, vec!["1".to_owned()]);
}

#[test]
fn trap_host_forwards_process_env_reads() {
    let mut host = BufferHost::default().with_env("FABER_TRAP_ENV", "via-trap");
    let status = run_source(
        r#"
import from "faber:processus" processus

main {
    print processus.read_env("FABER_TRAP_ENV")
}
"#,
        &mut host,
    )
    .expect("trap host forwards env_get to inner host");
    assert_eq!(status, ExitCode::SUCCESS);
    assert_eq!(host.stdout_lines, vec!["via-trap".to_owned()]);
}

#[test]
fn trap_host_forwards_virtual_cwd() {
    let mut host = BufferHost::default().with_cwd("/trap/sandbox");
    let status = run_source(
        r#"
import from "faber:processus" processus

main {
    print processus.cwd()
}
"#,
        &mut host,
    )
    .expect("trap host forwards cwd to inner host");
    assert_eq!(status, ExitCode::SUCCESS);
    assert_eq!(host.stdout_lines, vec!["/trap/sandbox".to_owned()]);
}

#[test]
fn run_source_returns_explicit_exit_code() {
    let mut host = BufferHost::default();
    let status = run_source(
        r#"
import from "faber:processus" processus

main {
    processus.exit_process(7)
}
"#,
        &mut host,
    )
    .expect("exi returns through trap host");
    assert_eq!(status, ExitCode::from(7));
}

#[test]
fn run_source_surfaces_frontend_errors() {
    let mut host = BufferHost::default();
    let error = run_source("not faber", &mut host).expect_err("invalid source fails");
    assert!(matches!(error, RunSourceError::Frontend(_)));
}

#[test]
fn empty_source_returns_runner_no_entry_error() {
    let mut host = BufferHost::default();
    let error = run_source("", &mut host).expect_err("empty source has no entry");
    let RunSourceError::Stepper(errors) = error else {
        panic!("expected runner error for empty source");
    };
    assert!(errors
        .iter()
        .any(|error| error.issue == "runner_no_entry_function_in_mir_program"));
}

#[test]
fn print_run_source_error_accepts_frontend_failures() {
    let mut host = BufferHost::default();
    let error = run_source("not faber", &mut host).expect_err("invalid source fails");
    // Must not panic.
    print_run_source_error(&error);
}

#[test]
fn run_source_surfaces_runner_errors() {
    let mut host = BufferHost::default();
    let error = run_source("@ future\nfn bad() { }\n", &mut host).expect_err("invalid async fails");
    assert!(matches!(error, RunSourceError::Stepper(_)));
}
