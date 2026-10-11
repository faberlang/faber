//! MIR lowering of the labelled record literal (`record<a: T, ...> { a = v, ... }`).
//!
//! The literal lowers to one `Record` aggregate with named operands in written
//! order, so the runner reads fields, prints, passes and returns it like any
//! other aggregate.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, dump_analyzed_unit, run_source};
use crate::program::analyze_prepared_program_name;

fn english(source: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\n{source}")
}

fn run(source: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let mut host = BufferHost::default();
    run_source(&session, "record-literal.fab", &english(source), &mut host)
        .unwrap_or_else(|error| panic!("the record program must run: {error:?}"));
    host.stdout_lines
}

#[test]
fn labelled_record_literal_reads_its_fields_on_the_runner() {
    let out = run(r"
main {
    const record<loss: f32, steps: int> r ← record<loss: f32, steps: int> { loss = 3.0, steps = 4 }
    print r.steps
    print r.loss
}
");
    assert_eq!(out, ["4", "3.0"]);
}

#[test]
fn labelled_record_literal_lowers_to_one_record_aggregate() {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let unit = analyze_prepared_program_name(
        &session,
        "record-literal-dump.fab",
        &english(
            "main {\n    const record<loss: f32, steps: int> r ← record<loss: f32, steps: int> { loss = 3.0, steps = 4 }\n    print r.steps\n}\n",
        ),
    )
    .expect("analyzes");
    let dump = dump_analyzed_unit(unit.application_view()).expect("MIR lowering succeeds");
    assert_eq!(dump.matches("construct record").count(), 1, "{dump}");
}

#[test]
fn record_passes_to_and_returns_from_functions() {
    let out = run(r"
fn bump(record<loss: f32, steps: int> r) → record<loss: f32, steps: int> {
    return record<loss: f32, steps: int> { loss = r.loss, steps = r.steps + 1 }
}
fn steps_of(record<loss: f32, steps: int> r) → int {
    return r.steps
}
main {
    const record<loss: f32, steps: int> r ← record<loss: f32, steps: int> { loss = 1.5, steps = 4 }
    const record<loss: f32, steps: int> s ← bump(r)
    print steps_of(s)
    print s.loss
}
");
    assert_eq!(out, ["5", "1.5"]);
}

#[test]
fn nested_record_literal_runs() {
    let out = run(r"
main {
    const record<id: int, inner: record<a: int, b: int>> r ← record<id: int, inner: record<a: int, b: int>> { id = 1, inner = record<a: int, b: int> { a = 2, b = 3 } }
    print r.id
    print r.inner.b
}
");
    assert_eq!(out, ["1", "3"]);
}

#[test]
fn record_with_a_list_field_runs() {
    let out = run(r"
main {
    const record<name: string, xs: list<int>> r ← record<name: string, xs: list<int>> { name = 'a', xs = [1, 2, 3] }
    print r.xs[2]
    print r.name
}
");
    assert_eq!(out, ["3", "a"]);
}

#[test]
fn whole_record_prints_in_written_order() {
    let out = run(r"
main {
    const record<loss: f32, steps: int> r ← record<loss: f32, steps: int> { loss = 3.0, steps = 4 }
    print r
}
");
    assert_eq!(out.len(), 1, "{out:?}");
    assert!(out[0].contains("loss") && out[0].contains("steps"), "{out:?}");
}
