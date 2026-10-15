//! Runner proofs for by-reference closure capture (operator ruling
//! 2026-09-24): an unmarked closure shares the enclosing scope. Writes through
//! a captured `var` are visible outside, outer writes after capture are
//! visible inside, and loop bindings stay fresh per iteration.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

fn run_en(body: &str) -> Vec<String> {
    let source = format!("+++\nlocale = \"en\"\n+++\n{body}");
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_dev_locale_root(),
    );
    let mut host = BufferHost::default();
    match run_source(&session, "test.fab", &source, &mut host) {
        Ok(()) => host.stdout_lines,
        Err(RunSourceError::Stepper(errors)) => panic!("runner failed: {errors:?}"),
        Err(RunSourceError::Frontend(diagnostics)) => panic!("frontend failed: {diagnostics:?}"),
        Err(RunSourceError::Mir(error)) => panic!("MIR lowering failed: {error:?}"),
    }
}

#[test]
fn outer_write_after_capture_is_visible_inside_closure() {
    let out = run_en("main {\n    mut int n ← 1\n    let () → int f ← () → int ∴ n\n    n ← 5\n    print f()\n}\n");
    assert_eq!(out, ["5"]);
}

#[test]
fn closure_write_to_captured_var_is_visible_outside() {
    let out = run_en(
        "main {\n    mut int n ← 0\n    let () → void bump ← () → void ∴ do { n ← n + 1 }\n    bump()\n    bump()\n    print n\n}\n",
    );
    assert_eq!(out, ["2"]);
}

#[test]
fn escaping_counter_keeps_its_own_shared_state() {
    let out = run_en(
        "fn counter() → () → int {\n    mut int c ← 0\n    return () → int ∴ do { c ← c + 1\n return c }\n}\nmain {\n    let () → int k ← counter()\n    let () → int j ← counter()\n    print k()\n    print k()\n    print j()\n    print k()\n}\n",
    );
    assert_eq!(out, ["1", "2", "1", "3"]);
}

#[test]
fn loop_bindings_are_fresh_per_iteration() {
    let out = run_en(
        "main {\n    mut list<() → int> fs ← empty\n    for range 0‥3 let i {\n        fs.append(() → int ∴ i * 10)\n    }\n    for from fs let f { print f() }\n}\n",
    );
    assert_eq!(out, ["0", "10", "20"]);
}

#[test]
fn loop_body_var_captured_and_mutated_is_fresh_per_iteration() {
    let out = run_en(
        "main {\n    mut list<() → int> fs ← empty\n    for range 0‥3 let i {\n        mut int acc ← i\n        fs.append(() → int ∴ do { acc ← acc + 100\n return acc })\n    }\n    for from fs let f { print f() }\n    for from fs let f { print f() }\n}\n",
    );
    assert_eq!(out, ["100", "101", "102", "200", "201", "202"]);
}

#[test]
fn nested_closures_share_one_binding() {
    let out = run_en(
        "main {\n    mut int n ← 0\n    let () → void outer ← () → void ∴ do {\n        let () → void inner ← () → void ∴ do { n ← n + 10 }\n        inner()\n        n ← n + 1\n    }\n    outer()\n    outer()\n    print n\n}\n",
    );
    assert_eq!(out, ["22"]);
}

#[test]
fn two_closures_share_one_captured_var() {
    let out = run_en(
        "main {\n    mut int n ← 0\n    let () → void inc ← () → void ∴ do { n ← n + 1 }\n    let () → int get ← () → int ∴ n\n    inc()\n    inc()\n    print get()\n    n ← 40\n    inc()\n    print get()\n}\n",
    );
    assert_eq!(out, ["2", "41"]);
}

#[test]
fn escaping_adder_captures_parameter() {
    let out = run_en(
        "fn apply((int) → int f, int x) → int { return f(x) }\nfn adder(int k) → (int) → int { return int x → int ∴ x + k }\nmain {\n    print apply(int x → int ∴ x * 3, 4)\n    let (int) → int a5 ← adder(5)\n    print a5(1)\n    print apply(adder(7), 1)\n}\n",
    );
    assert_eq!(out, ["12", "6", "8"]);
}

fn dump_en(body: &str) -> String {
    let source = format!("+++\nlocale = \"en\"\n+++\n{body}");
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_dev_locale_root(),
    );
    let unit =
        match crate::driver::analyze_source(&session, "test.fab", &source, crate::driver::AnalyzeRequest::default()) {
            Ok(unit) => unit,
            Err(diagnostics) => panic!("analysis failed: {diagnostics:?}"),
        };
    match crate::mir::dump_analyzed_unit(unit.application_view()) {
        Ok(dump) => dump,
        Err(errors) => panic!("MIR lowering failed: {errors:?}"),
    }
}

#[test]
fn never_rebound_capture_copies_without_a_shared_cell() {
    // A `var` that is captured but never rebound is observably a value:
    // capture stays a plain copy and no cell is allocated.
    let dump = dump_en("main {\n    mut int n ← 1\n    let () → int f ← () → int ∴ n\n    print f()\n}\n");
    assert!(!dump.contains("shared_cell"), "{dump}");
    assert!(!dump.contains(".cell"), "{dump}");
}

#[test]
fn rebound_capture_is_stored_in_a_shared_cell() {
    let dump = dump_en("main {\n    mut int n ← 1\n    let () → int f ← () → int ∴ n\n    n ← 5\n    print f()\n}\n");
    assert!(dump.contains("construct shared_cell"), "{dump}");
    assert!(dump.contains(".cell"), "{dump}");
}

#[test]
fn param_less_vacuum_closure_is_not_the_entry() {
    // Entry-shaped synthetic closures (no params, `void` return) must not
    // be selected as `incipit`; only the unreferenced entry is.
    let out = run_en(
        "main {\n    let () → void hello ← () → void ∴ do { print \"inner\" }\n    print \"outer\"\n    hello()\n}\n",
    );
    assert_eq!(out, ["outer", "inner"]);
}

#[test]
fn shared_text_rebinding_reads_before_it_stores() {
    let out = run_en(
        "main {\n    mut string s ← \"a\"\n    let () → void grow ← () → void ∴ do { s ← s + \"b\" }\n    grow()\n    grow()\n    print s\n}\n",
    );
    assert_eq!(out, ["abb"]);
}

#[test]
fn increment_through_a_captured_var() {
    let out = run_en(
        "main {\n    mut int n ← 0\n    let () → void tick ← () → void ∴ do { n↑ }\n    tick()\n    tick()\n    tick()\n    print n\n}\n",
    );
    assert_eq!(out, ["3"]);
}

#[test]
fn captured_list_mutated_after_capture_is_visible_inside() {
    let out = run_en(
        "main {\n    mut list<int> xs ← [1]\n    let () → int f ← () → int ∴ xs.length()\n    xs.append(2)\n    xs.append(3)\n    print f()\n}\n",
    );
    assert_eq!(out, ["3"]);
}
