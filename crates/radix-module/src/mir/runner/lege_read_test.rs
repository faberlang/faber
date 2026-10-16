//! Q-E5 (ruled 2026-10-15): `read` returns all of stdin to EOF; `read line`
//! one line without its terminator.
//!
//! These tests push the whole chain through `run_source`: the HIR read flag
//! selects `ReadLine` or `ReadAll` during MIR lowering, and the runner host
//! renders the seeded stdin accordingly.
//!
//! The read-line spelling that reaches `HirExpressionKind::ReadLine(true)` on
//! this checkout is the standalone `lineam` builtin. The two-word `lege lineam`
//! surface currently consumes `lege` as a bare read and leaves `lineam` as a
//! separate expression statement (a radix-parser gap outside this unit's write
//! scope); `lineam` alone is the spelling the parser maps to the line flag.

use super::{BufferHost, run_source};
use crate::codegen::Target;
use crate::driver::{Config, Session};

fn run_lines(name: &str, source: &str, stdin: &[&str]) -> Vec<String> {
    let mut host = BufferHost::default().with_stdin_lines(stdin.iter().copied());
    run_source(
        &Session::new(Config::default().with_target(Target::HirRust)),
        name,
        source,
        &mut host,
    )
    .unwrap_or_else(|error| panic!("{name} must run: {error:?}"));
    host.stdout_lines
}

#[test]
fn lege_read_line_yields_one_line_at_a_time() {
    let source = r#"+++
locale = "la"
+++
incipit {
  sit textus ∪ nihil a ← lineam
  sit textus ∪ nihil b ← lineam
  nota a
  nota b
}
"#;
    assert_eq!(run_lines("lege-read-line.fab", source, &["one", "two"]), ["one", "two"]);
}

#[test]
fn lege_read_all_stdin_to_eof() {
    let source = r#"+++
locale = "la"
+++
incipit {
  sit textus ∪ nihil x ← lege
  nota x
}
"#;
    assert_eq!(run_lines("lege-read-all.fab", source, &["one", "two"]), ["one\ntwo\n"]);
}
