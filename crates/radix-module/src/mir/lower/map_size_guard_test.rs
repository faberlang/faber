//! Map size guard (need, ruling O1(3)): a loop over a map whose body
//! holds a bracket store on that same map compares the map's size with its
//! size at loop entry at every step and traps on a difference. A loop without
//! such a store carries no check.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session};
use crate::mir::{
    BufferHost, MirIntrinsic, MirLoweringOptions, MirStatementKind, RunSourceError, lower_analyzed_unit, run_source,
};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn session() -> Session {
    Session::new(Config::default().with_target(Target::HirRust))
}

fn program(body: &str) -> String {
    format!("+++\nlocale = \"en\"\n+++\nmain {{\n{body}\n}}\n")
}

/// Number of `Panic` runtime calls in the lowered program: the loop's size
/// guard is the only panic site a plain map loop can contain.
fn panic_calls(name: &str, source: &str) -> usize {
    let unit = crate::driver::analyze_source(&session(), name, source, AnalyzeRequest::default())
        .unwrap_or_else(|error| panic!("{name} should analyze: {error:?}"));
    let lowered = lower_analyzed_unit(unit.application_view(), MirLoweringOptions::default())
        .unwrap_or_else(|error| panic!("{name} should lower: {error:?}"));
    lowered
        .validated
        .program()
        .functions
        .iter()
        .flat_map(|function| &function.blocks)
        .flat_map(|block| &block.statements)
        .filter(|statement| {
            matches!(
                &statement.kind,
                MirStatementKind::RuntimeCall { call, .. } if call.intrinsic == MirIntrinsic::Panic
            )
        })
        .count()
}

#[test]
fn loop_with_bracket_store_on_the_same_map_carries_one_size_guard() {
    let source = program("mut map<string, int> m ← { \"a\": 1 }\nfor from m at [k] let v {\nm[k] ← v + 1\n}");
    assert_eq!(panic_calls("map-guard-store.fab", &source), 1);
}

#[test]
fn loop_without_a_bracket_store_carries_no_size_guard() {
    let source = program(
        "mut map<string, int> m ← { \"a\": 1 }\nmut int total ← 0\nfor from m let v {\ntotal ← total + v\n}\nprint total",
    );
    assert_eq!(panic_calls("map-guard-none.fab", &source), 0);
}

#[test]
fn loop_storing_into_a_different_map_carries_no_size_guard() {
    let source = program(
        "mut map<string, int> m ← { \"a\": 1 }\nmut map<string, int> n ← empty\nfor from m at [k] let v {\nn[k] ← v\n}\nprint n[\"a\"]",
    );
    assert_eq!(panic_calls("map-guard-other.fab", &source), 0);
}

#[test]
fn update_only_bracket_store_loop_runs_to_completion() {
    let source = program(
        "mut map<string, int> m ← { \"a\": 1, \"b\": 2 }\nfor from m at [k] let v {\nm[k] ← v * 10\n}\nprint m[\"a\"]\nprint m[\"b\"]",
    );
    let mut host = BufferHost::default();
    run_source(&session(), "map-guard-update.fab", &source, &mut host)
        .unwrap_or_else(|error| panic!("update-only loop should run: {error:?}"));
    assert_eq!(host.stdout_lines, ["10", "20"]);
}

#[test]
fn adding_bracket_store_loop_traps_with_the_size_message() {
    let source = program(
        "mut map<string, int> m ← { \"a\": 1 }\nfor from m let v {\nm[\"b\"] ← v\nprint \"stored\"\n}\nprint \"unreached\"",
    );
    let mut host = BufferHost::default();
    let outcome: Result<Result<(), RunSourceError>, _> = catch_unwind(AssertUnwindSafe(|| {
        run_source(&session(), "map-guard-trap.fab", &source, &mut host)
    }));
    assert!(outcome.is_err(), "an adding store must trap, not return");
    assert_eq!(host.stdout_lines, ["stored"]);
    assert_eq!(host.last_abort.as_deref(), Some("map changed size during iteration"));
}
