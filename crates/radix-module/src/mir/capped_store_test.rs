//! H1 (ruling 17b): an `int` store is checked whatever shape the value has.
//!
//! `big + 1` sits at the 64-bit cap: its type is `i64` but its exact value is
//! `2^63`. Nested arithmetic, a conditional, a call argument, a return, a list
//! element, a field and `append` all still land that value in an `i64` slot, so
//! each one traps with the store text. The probes are the audit's p18a..p31.

use crate::codegen::Target;
use crate::driver::{Config, Session};
use crate::mir::{BufferHost, RunSourceError, run_source};

const BIG: &str = "fixum numerus big ← 9223372036854775807\n";

fn run(prelude: &str, body: &str) -> (Vec<String>, Vec<(String, String)>) {
    let session = Session::new(
        Config::default()
            .with_target(Target::HirTypeScript)
            .with_latin_locale(),
    );
    let source = format!("{prelude}incipit {{\n{BIG}{body}\n}}\n");
    let mut host = BufferHost::default();
    let errors = match run_source(&session, "capped.fab", &source, &mut host) {
        Ok(()) => Vec::new(),
        Err(RunSourceError::Stepper(errors)) => errors.into_iter().map(|e| (e.issue, e.message)).collect(),
        Err(other) => panic!("{body}: {other:?}"),
    };
    (host.stdout_lines, errors)
}

/// The store traps with the ruled text at `position`, before anything prints.
fn assert_store_trap(prelude: &str, body: &str, position: &str) {
    let (out, errors) = run(prelude, body);
    assert!(out.is_empty(), "{body}: printed {out:?}");
    let (issue, message) = errors
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no trap: {body}"));
    assert_eq!(issue, "implicit_store_out_of_range", "{body}: {message}");
    assert!(
        message.contains("9223372036854775808 does not fit in `i64`") && message.contains(position),
        "{body}: {message}"
    );
}

#[test]
fn nested_sum_traps_at_the_declaration() {
    // p18a
    assert_store_trap("", "fixum numerus r ← big + 1 + 0\nnota r", "(declaration of `r`)");
}

#[test]
fn nested_product_traps_at_the_assignment() {
    // p18b
    assert_store_trap("", "varia numerus r ← 0\nr ← (big + 1) * 1\nnota r", "(assignment to `r`)");
}

#[test]
fn conditional_branch_traps_at_the_declaration() {
    // p18c
    assert_store_trap(
        "",
        "fixum bivalens t ← verum\nfixum numerus r ← t ✓ big + 1 ✗ 0\nnota r",
        "(declaration of `r`)",
    );
}

#[test]
fn nested_sum_traps_at_the_argument() {
    // p18d
    assert_store_trap(
        "functio ident(numerus x) → numerus { redde x }\n",
        "nota ident(big + 1 + 0)",
        "(argument",
    );
}

#[test]
fn nested_sum_traps_at_the_return() {
    // p18e
    assert_store_trap(
        "functio over(numerus x) → numerus { redde x + 1 + 0 }\n",
        "nota over(big)",
        "(return)",
    );
}

#[test]
fn negated_minimum_plus_zero_traps() {
    // p18f
    assert_store_trap(
        "",
        "fixum numerus mn ← -9223372036854775807 - 1\nfixum numerus r ← -mn + 0\nnota r",
        "(declaration of `r`)",
    );
}

#[test]
fn list_element_store_traps() {
    // p23: the corrupt value never reaches the slot, so no later read prints.
    assert_store_trap(
        "",
        "varia lista<numerus> xs ← [0]\nxs[0] ← big + 1 + 0\nnota xs[0]",
        "(assignment",
    );
}

#[test]
fn field_store_traps() {
    // p26
    assert_store_trap(
        "genus Holder { varia numerus n }\n",
        "varia Holder h ← Holder { n = 0 }\nh.n ← big + 1 + 0\nnota h.n",
        "(assignment to `n`)",
    );
}

#[test]
fn append_argument_traps() {
    // p31
    assert_store_trap(
        "",
        "varia lista<numerus> xs ← []\nxs.appende(big + 1 + 0)\nnota xs[0]",
        "(argument",
    );
}

#[test]
fn inferred_slot_traps_at_the_declaration() {
    // ss6 / p22: `_` takes `i64`, so the declaration is the checked store.
    assert_store_trap("", "fixum _ v ← big + 1\nnota v", "(declaration of `v`");
    assert_store_trap("", "fixum _ v ← big + 1 + 0\nnota v", "(declaration of `v`");
}

#[test]
fn in_range_nested_values_still_store() {
    // Controls: nothing here reaches past `i64::MAX`.
    let (out, errors) = run("", "fixum numerus r ← 5 + 1 + 0\nnota r");
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(out, ["6"]);
    let (out, errors) = run("", "fixum numerus r ← big - 1 + 1\nnota r");
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(out, ["9223372036854775807"]);
    let (out, errors) = run("", "fixum bivalens t ← falsum\nfixum numerus r ← t ✓ big + 1 ✗ 7\nnota r");
    assert!(errors.is_empty(), "{errors:?}");
    assert_eq!(out, ["7"]);
}
