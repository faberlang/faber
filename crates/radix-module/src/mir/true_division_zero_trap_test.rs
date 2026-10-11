//! `int ÷ int` lowers a discarded `1 / divisor` so a zero divisor traps
//! (ruling 19c). A nonzero constant divisor cannot trap, so it emits no such
//! temporary; any other divisor keeps it.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, Config, Session, analyze_source};

fn dump(body: &str) -> String {
    let source = format!("+++\nlocale = \"en\"\n+++\n{body}\n");
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let unit = analyze_source(&session, "true-div.fab", &source, AnalyzeRequest::default())
        .unwrap_or_else(|diagnostics| panic!("analysis succeeds: {diagnostics:?}"));
    crate::mir::dump_analyzed_unit(unit.application_view())
        .unwrap_or_else(|errors| panic!("MIR lowering succeeds: {errors:?}"))
}

fn has_trap_probe(dump: &str) -> bool {
    dump.lines().any(|line| line.contains("1 / "))
}

#[test]
fn constant_nonzero_divisor_emits_no_trap_probe() {
    let dump = dump("fn half(int n) → f64 {\n    return n ÷ 2\n}\nmain {\n    print half(7)\n}");
    assert!(!has_trap_probe(&dump), "{dump}");
}

#[test]
fn variable_divisor_keeps_the_trap_probe() {
    let dump = dump("fn over(int n, int d) → f64 {\n    return n ÷ d\n}\nmain {\n    print over(7, 2)\n}");
    assert!(has_trap_probe(&dump), "{dump}");
}

#[test]
fn constant_zero_divisor_keeps_the_trap_probe() {
    let dump = dump("fn zero(int n) → f64 {\n    return n ÷ 0\n}\nmain {\n    print zero(7)\n}");
    assert!(has_trap_probe(&dump), "{dump}");
}
