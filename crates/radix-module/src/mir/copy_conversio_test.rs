//! A `copy`-bound local converted with `↦` / `↤` (F-41): the expression reads
//! typed under the `copy` access marker while the MIR local carries the
//! payload, so the conversion source type must be the peeled payload.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run(source: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let mut host = BufferHost::default();
    match run_source(&session, "copy-conversio.fab", source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Frontend(errors)) => panic!("fixture frontend errors: {errors:?}"),
        Err(RunSourceError::Mir(errors)) => panic!("fixture MIR errors: {errors:?}"),
        Err(RunSourceError::Stepper(errors)) => panic!("fixture runner errors: {errors:?}"),
    }
}

fn en(body: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n{body}")
}

#[test]
fn copy_list_returns_as_bytes() {
    let source = en("fn mk(list<u8> table) → bytes {
    var list<u8> b ← copy table
    b.append(9)
    return b ↦ bytes
}
main {
    const bytes o ← mk([1, 2, 3])
    print o.length()
}
");
    assert_eq!(run(&source), vec!["4"]);
}

#[test]
fn copy_list_converts_to_bytes_with_arrow_down() {
    let source = en("main {
    const list<u8> t ← [1, 2, 3]
    var list<u8> b ← copy t
    const bytes o ↤ b
    print o.length()
}
");
    assert_eq!(run(&source), vec!["3"]);
}

#[test]
fn copy_list_converts_to_wider_list() {
    let source = en("main {
    const list<i32> t ← [1, 2, 3]
    var list<i32> b ← copy t
    const list<i64> o ← b ↦ list<i64>
    print o
}
");
    assert_eq!(run(&source), vec!["[1, 2, 3]"]);
}

#[test]
fn plain_list_converts_to_bytes() {
    let source = en("fn mk(list<u8> table) → bytes {
    const list<u8> b ← table
    return b ↦ bytes
}
main {
    const bytes o ← mk([1, 2, 3])
    print o.length()
}
");
    assert_eq!(run(&source), vec!["3"]);
}

#[test]
fn plain_list_converts_to_wider_list() {
    let source = en("main {
    var list<i32> t ← [1, 2, 3]
    var list<i32> b ← t
    const list<i64> o ← b ↦ list<i64>
    print o
}
");
    assert_eq!(run(&source), vec!["[1, 2, 3]"]);
}
