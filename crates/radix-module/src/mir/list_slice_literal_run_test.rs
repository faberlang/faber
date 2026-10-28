//! Runner proof for a list slice `xs.slice(lo, hi)` whose receiver is a list
//! LITERAL. A standalone non-empty list literal types as the exact-length
//! witness `list<T, size N>` (`Type::BoundedArray`); the validator's slice branch
//! must accept that receiver and require the unbounded list result.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, run_source};

fn run(source: &str) -> (Result<(), String>, Vec<String>) {
    let source = format!("+++\nlocale = \"en\"\n+++\n{source}");
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let mut host = BufferHost::default();
    let result = run_source(&session, "test.fab", &source, &mut host).map_err(|error| format!("{error:?}"));
    (result, host.stdout_lines)
}

#[test]
fn list_literal_slice_runs() {
    let (result, lines) = run("main {
    print [1, 2, 3, 4].slice(1, 3)
    let list<int> w ← [1, 2, 3, 4].slice(1, 3)
    print w
}
");
    result.expect("a list literal receiver slices");
    assert_eq!(lines, vec!["[2, 3]", "[2, 3]"]);
}
