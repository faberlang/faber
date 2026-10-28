use crate::script::source_declares_cli_entry;
use crate::script::source_declares_non_kernel_import;
use crate::script::{
    BufferHost, RunSourceError, interpret_source, print_run_source_error, run_named, run_source,
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
        code: "runner_unsupported",
        args: Vec::new(),
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
fn bounded_lista_for_ref_runs_indices_end_to_end() {
    let mut host = BufferHost::default();
    let status = run_source(
        r#"
main {
    let list<int, size 3> numeri ← [10, 20, 30]
    for ref numeri let index {
        print index
    }
}
"#,
        &mut host,
    )
    .expect("bounded lista for ref run succeeds");
    assert_eq!(status, ExitCode::SUCCESS);
    assert_eq!(
        host.stdout_lines,
        vec!["0".to_owned(), "1".to_owned(), "2".to_owned()]
    );
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
fn process_read_env_uses_the_shared_leaf() {
    unsafe { std::env::set_var("FABER_LEAF_ENV", "via-leaf") };
    let mut host = BufferHost::default();
    let status = run_source(
        r#"
import from "norma:process" process

main {
    print process.read_env("FABER_LEAF_ENV")
}
"#,
        &mut host,
    )
    .expect("norma:process read_env uses faber-leaf");
    unsafe { std::env::remove_var("FABER_LEAF_ENV") };
    assert_eq!(status, ExitCode::SUCCESS);
    assert_eq!(host.stdout_lines, vec!["via-leaf".to_owned()]);
}

#[test]
fn run_source_returns_explicit_exit_code() {
    let mut host = BufferHost::default();
    let status = run_source(
        r#"
import from "norma:process" process

main {
    process.exit_process(7)
}
"#,
        &mut host,
    )
    .expect("processus:exi returns through Host::exit");
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
    assert!(
        errors
            .iter()
            .any(|error| error.issue == "runner_no_entry_function_in_mir_program")
    );
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

// VEC-5b: a run-time index on a `vector<T, size N>` is a checked lane select. The
// loop runs the in-range lanes; an index past the width ends the run with the
// same runner failure an out-of-range `lista` index produces (the
// checked-index law), after the lines printed before the trap.

fn runner_failure_issue(source: &str) -> (Vec<String>, String) {
    let mut host = BufferHost::default();
    let error = run_source(source, &mut host).expect_err("the run ends on the index trap");
    let RunSourceError::Stepper(errors) = error else {
        panic!("expected a runner failure, got a frontend failure");
    };
    let issue = errors.first().expect("one runner failure").issue.clone();
    (host.stdout_lines, issue)
}

#[test]
fn vector_runtime_index_sums_the_lanes_then_traps_like_a_lista() {
    let (printed, vector_issue) = runner_failure_issue(
        r"
fn lane(vector<f32, size 3> v, int i) → f32 {
    return v[i]
}

main {
    let vector<f32, size 3> v ← [1.0, 2.0, 3.0] ↦ vector<f32, size 3>
    mut f32 sum ← 0.0
    mut int i ← 0
    while i ≺ 3 {
        sum ← sum + lane(v, i)
        i ← i + 1
    }
    print sum
    print lane(v, 3)
}
",
    );
    assert_eq!(printed, vec!["6.0".to_owned()]);

    let (_, lista_issue) = runner_failure_issue(
        r"
fn item(list<f32> xs, int i) → f32 {
    return xs[i]
}

main {
    let list<f32> xs ← [1.0, 2.0, 3.0]
    print item(xs, 3)
}
",
    );
    assert_eq!(vector_issue, lista_issue);
}

#[test]
fn source_declares_non_kernel_import_classifies_imports() {
    assert!(source_declares_non_kernel_import(
        "<source>",
        "import from \"norma:process\" process\n\nmain { print 1 }\n"
    ));
    assert!(source_declares_non_kernel_import(
        "<source>",
        "import from \"./thing\" thing\n\nmain { print 1 }\n"
    ));
    assert!(!source_declares_non_kernel_import(
        "<source>",
        "import from \"faber:processus\" processus\n\nmain { print 1 }\n"
    ));
    assert!(!source_declares_non_kernel_import(
        "<source>",
        "main { print 1 }\n"
    ));
}

#[test]
fn source_declares_cli_entry_classifies_sources() {
    // Sugared `@ cli "tool"` on the en surface.
    assert!(source_declares_cli_entry(
        "<source>",
        "\n@ cli \"tool\"\nincipit {}\n"
    ));
    // Braced form on the pack-less (Latin) surface.
    assert!(source_declares_cli_entry(
        "<source>",
        "\n@ cli { nomen = \"tool\" }\nincipit {}\n"
    ));
    // No `@ cli` root: not a CLI program.
    assert!(!source_declares_cli_entry("<source>", "\nincipit {}\n"));
    // A comment-only mention is not an annotation.
    assert!(!source_declares_cli_entry(
        "<source>",
        "\n# @ cli mention\nincipit {}\n"
    ));
}

#[test]
fn run_named_binds_cli_operands_for_single_file_source() {
    const SOURCE: &str = "+++\nlocale = \"en\"\n+++\n\n@ cli \"greeter\"\n@ operand string who\nmain args args {\n  print args.who\n}\n";
    let mut host = BufferHost::with_argumenta(vec!["Anna".to_owned()]);
    let status =
        run_named("cli-tool.fab", SOURCE, &mut host).expect("cli single-file run succeeds");
    assert_eq!(status, ExitCode::SUCCESS);
    assert_eq!(host.stdout_lines, vec!["Anna".to_owned()]);
}
