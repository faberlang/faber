//! Sink-into law on the MIR runner: `xs ⇜ v` appends to a list keeping
//! duplicates, and is a no-op on a set that already holds the item. The runner
//! is pinned here directly rather than through the corpus suite.
//!
//! The runner tests these built-in collection sinks without declaring or
//! importing the user-defined sinking contract.

use super::{BufferHost, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirRust))
}

fn program(body: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\nmain {{\n{body}\n}}\n")
}

fn run_program(name: &str, source: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    run_source(&session(), name, source, &mut host)
        .unwrap_or_else(|error| panic!("{name} should run on the MIR runner: {error:?}"));
    host.stdout_lines
}

fn run_lines(name: &str, body: &str) -> Vec<String> {
    run_program(name, &program(body))
}

#[test]
fn sink_into_a_list_appends_and_keeps_duplicates() {
    let lines = run_lines(
        "sink-into-list.fab",
        "mut list<int> xs ← []\nxs ⇜ 3\nxs ⇜ 3\nprint xs.length()\nprint xs[0]\nprint xs[1]",
    );
    assert_eq!(lines, ["2", "3", "3"]);
}

#[test]
fn sink_into_a_bounded_list_runs_without_a_contract_declaration() {
    let lines = run_lines(
        "sink-into-bounded-list.fab",
        "mut list<int, size 2> xs ← []\nxs ⇜ 3\nprint xs.length()\nprint xs[0]",
    );
    assert_eq!(lines, ["1", "3"]);
}

#[test]
fn sink_into_a_set_is_a_no_op_when_present() {
    let lines = run_lines(
        "sink-into-set.fab",
        "mut set<int> seen ← []\nseen ⇜ 3\nseen ⇜ 3\nprint seen.length()",
    );
    assert_eq!(lines, ["1"]);
}

/// The operator form dies at capacity: the run stops on the failed write and
/// nothing after it runs. The method form in the twin test below reports the
/// same overflow through the declared error channel instead.
#[test]
fn sink_into_a_full_bounded_list_dies_at_capacity() {
    let mut host = BufferHost::default();
    let source = program("mut list<int, size 2> xs ← [1, 2]\nxs ⇜ 3\nprint \"unreachable\"");
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        run_source(&session(), "sink-into-full-bounded-list.fab", &source, &mut host)
    }));
    assert!(outcome.is_err(), "a full bounded list must die rather than return");
    assert!(
        host.stdout_lines.is_empty(),
        "the statement after the failed write never runs, got {:?}",
        host.stdout_lines
    );
    assert!(host.last_abort.is_some(), "the dying write aborts through the host");
}

#[test]
fn the_sink_method_reports_a_full_bounded_list() {
    const REPORTING: &str = r#"fn grow(mut list<int, size 2> xs) → void ⇥ string {
    xs.sink(3)
}

main {
    mut list<int, size 2> xs ← [1, 2]
    do {
        grow(mut xs)
        print "no overflow"
    } catch err {
        print "reported"
    }
    print xs.length()
}
"#;
    let source = format!("+++\nlocale = \"en\"\n+++\n{REPORTING}");
    let lines = run_program("full-bounded-list-reports.fab", &source);
    assert_eq!(lines, ["reported", "2"]);
}
