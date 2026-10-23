//! Sink-into law on the MIR runner: `xs ⇜ v` appends to a list keeping
//! duplicates, and is a no-op on a set that already holds the item. The runner
//! is pinned here directly rather than through the corpus suite.
//!
//! The compiler recognizes the sinking contract by its `@ radix contract
//! "sinking"` role, never by name, so the fixture declares the same interface
//! `norma:sink` declares. The bodies above `main` stay out of the tests.

use super::{BufferHost, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};

/// The sinking contract, as `norma:sink` declares it, plus a local fault type.
const SINKING: &str = r#"@ radix contract "sinking"
interface Sink<T> {
    fn sink(T item) → void ⇥ Oops
    fn flush() → void ⇥ Oops
}

union Oops {
    Broken { string path }
}

"#;

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirRust))
}

fn program(body: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n{SINKING}main {{\n{body}\n}}\n")
}

fn run_lines(name: &str, body: &str) -> Vec<String> {
    let mut host = BufferHost::default();
    run_source(&session(), name, &program(body), &mut host)
        .unwrap_or_else(|error| panic!("{name} should run on the MIR runner: {error:?}"));
    host.stdout_lines
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
fn sink_into_a_set_is_a_no_op_when_present() {
    let lines = run_lines(
        "sink-into-set.fab",
        "mut set<int> seen ← []\nseen ⇜ 3\nseen ⇜ 3\nprint seen.length()",
    );
    assert_eq!(lines, ["1"]);
}
