//! `own` call arguments: whole local bindings only, and the name is dead after.
//!
//! Operator rulings 2026-10-08 (R-05, R-06). Assertions are issue slugs only.

use crate::test_support::{analyze_source_with_config, issue};

const DEFS: &str = "class Box {\n    mut list<int> xs\n}\n\
fn eat(own list<int> xs) → void { print xs.length() }\n\
fn dup(copy list<int> xs) → void { print xs.length() }\n\
fn peek(list<int> xs) → void { print xs.length() }\n\
fn make() → list<int> { return [7, 8, 9] }\n\
class Sink {\n    fn take(own list<int> xs) → void { print xs.length() }\n}\n\
const list<int> K = [1, 2]\n";

fn issues(source: &str) -> Vec<String> {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| issue(d).unwrap_or("<no issue slug>").to_owned())
        .collect()
}

fn main_body(body: &str) -> Vec<String> {
    issues(&format!("{DEFS}main {{\n{body}\n}}\n"))
}

fn func(sig_and_body: &str) -> Vec<String> {
    issues(&format!("{DEFS}{sig_and_body}\nmain {{ }}\n"))
}

const LOCAL: &str = "    mut list<int> a ← [1, 2, 3]\n";

#[test]
fn whole_local_fresh_and_exclusive_parameters_are_accepted() {
    assert!(main_body(&format!("{LOCAL}    eat(own a)")).is_empty());
    assert!(main_body("    eat(own [1, 2, 3])").is_empty());
    assert!(main_body("    eat(own make())").is_empty());
    assert!(func("fn f(own list<int> p) → void { eat(own p) }").is_empty());
    assert!(func("fn f(copy list<int> p) → void { eat(own p) }").is_empty());
}

#[test]
fn field_and_element_arguments_are_not_local_bindings() {
    let field = "    let Box b ← Box { xs = [1] }\n    eat(own b.xs)";
    assert_eq!(main_body(field), ["own_argument_not_local_binding"]);
    let element = "    mut list<list<int>> a ← [[1], [2]]\n    eat(own a[0])";
    assert_eq!(main_body(element), ["own_argument_not_local_binding"]);
}

#[test]
fn copy_of_a_field_is_accepted_and_does_not_kill_the_name() {
    let body = "    let Box b ← Box { xs = [1] }\n    dup(copy b.xs)\n    dup(copy b.xs)\n    print b.xs.length()";
    assert!(main_body(body).is_empty());
    let local = format!("{LOCAL}    dup(copy a)\n    print a.length()");
    assert!(main_body(&local).is_empty());
}

#[test]
fn unmarked_parameter_cannot_be_given_away() {
    assert_eq!(
        func("fn f(list<int> p) → void { eat(own p) }"),
        ["own_argument_unmarked_parameter"]
    );
}

#[test]
fn module_constant_cannot_be_given_away() {
    assert_eq!(
        main_body("    eat(own K)"),
        ["own_argument_module_constant"]
    );
}

#[test]
fn loop_binder_cannot_be_given_away() {
    let body = "    let list<list<int>> ll ← [[1], [2]]\n    for from ll let x {\n        eat(own x)\n    }";
    assert_eq!(main_body(body), ["own_argument_loop_binder"]);
}

#[test]
fn method_arguments_follow_the_same_rules() {
    let ok = format!("    let Sink s ← Sink {{}}\n{LOCAL}    s.take(own a)");
    assert!(main_body(&ok).is_empty());
    let bad = "    let Sink s ← Sink {}\n    let Box b ← Box { xs = [1] }\n    s.take(own b.xs)";
    assert_eq!(main_body(bad), ["own_argument_not_local_binding"]);
}

#[test]
fn use_after_own_blames_the_call_for_an_unmarked_source() {
    let body = format!("{LOCAL}    eat(own a)\n    print a.length()");
    assert_eq!(main_body(&body), ["use_after_own"]);
}

#[test]
fn use_after_own_of_an_exclusive_source_is_use_after_move() {
    assert_eq!(
        func("fn f(own list<int> p) → void {\n    eat(own p)\n    print p.length()\n}"),
        ["use_after_move"]
    );
}

#[test]
fn a_different_name_after_the_move_is_fine() {
    let body = format!("{LOCAL}    mut list<int> c ← [4]\n    eat(own a)\n    print c.length()");
    assert!(main_body(&body).is_empty());
}

#[test]
fn giving_the_same_name_away_twice_is_use_after_own() {
    let body = format!("{LOCAL}    eat(own a)\n    eat(own a)");
    assert_eq!(main_body(&body), ["use_after_own"]);
}

#[test]
fn assigning_the_name_again_makes_it_live() {
    let body = format!("{LOCAL}    eat(own a)\n    a ← [5]\n    print a.length()");
    assert!(main_body(&body).is_empty());
}

#[test]
fn branches_do_not_kill_each_other() {
    let sibling = format!(
        "{LOCAL}    if a.length() ≻ 1 {{\n        eat(own a)\n    }} else {{\n        print a.length()\n    }}"
    );
    assert!(main_body(&sibling).is_empty());
    let joined = format!(
        "{LOCAL}    if a.length() ≻ 1 {{\n        eat(own a)\n    }}\n    print a.length()"
    );
    assert_eq!(main_body(&joined), ["use_after_own"]);
}

#[test]
fn a_diverging_branch_does_not_kill_the_name_after_the_join() {
    let f = "fn f(list<int> q) → void {\n    mut list<int> a ← [1]\n    if q.length() ≻ 1 {\n        eat(own a)\n        return\n    }\n    print a.length()\n}";
    assert!(func(f).is_empty());
}

#[test]
fn moving_an_outer_binding_in_a_while_body_is_moved_in_loop() {
    let body = format!(
        "{LOCAL}    mut bool c ← true\n    while c {{\n        eat(own a)\n        c ← false\n    }}"
    );
    assert_eq!(main_body(&body), ["moved_in_loop"]);
    let exclusive =
        "fn f(own list<int> p, bool c) → void {\n    while c {\n        eat(own p)\n    }\n}";
    assert_eq!(func(exclusive), ["moved_in_loop"]);
}

#[test]
fn a_value_created_in_the_loop_body_moves_once_per_iteration() {
    let body = "    mut bool c ← true\n    while c {\n        mut list<int> b ← [1]\n        eat(own b)\n        c ← false\n    }";
    assert!(main_body(body).is_empty());
}

#[test]
fn a_loop_that_reassigns_or_leaves_after_the_move_is_fine() {
    let reassign = format!(
        "{LOCAL}    mut bool c ← true\n    while c {{\n        eat(own a)\n        a ← [2]\n        c ← false\n    }}"
    );
    assert!(main_body(&reassign).is_empty());
    let leaves = format!(
        "{LOCAL}    mut bool c ← true\n    while c {{\n        eat(own a)\n        break\n    }}"
    );
    assert!(main_body(&leaves).is_empty());
}

#[test]
fn a_break_in_a_nested_loop_does_not_leave_the_outer_loop() {
    let body = format!(
        "{LOCAL}    mut bool c ← true\n    mut bool d ← true\n    while c {{\n        eat(own a)\n        while d {{\n            d ← false\n            break\n        }}\n        c ← false\n    }}"
    );
    assert_eq!(main_body(&body), ["moved_in_loop"]);
}

#[test]
fn a_return_inside_a_closure_does_not_leave_the_loop() {
    let body = format!(
        "{LOCAL}    mut bool c ← true\n    while c {{\n        eat(own a)\n        let () → int f ← () → int ∴ do {{\n            return 1\n        }}\n        print f()\n        c ← false\n    }}"
    );
    assert_eq!(main_body(&body), ["moved_in_loop"]);
}

#[test]
fn a_return_in_a_nested_loop_still_leaves_the_function() {
    let body = "fn f(own list<int> p, bool c, bool d) → void {\n    while c {\n        eat(own p)\n        while d {\n            return\n        }\n    }\n}";
    assert!(func(body).is_empty());
}

#[test]
fn a_closure_called_after_its_captured_name_was_given_away_is_rejected() {
    let body =
        format!("{LOCAL}    let () → int f ← () → int ∴ a.length()\n    eat(own a)\n    print f()");
    assert_eq!(main_body(&body), ["moved_name_captured_by_closure"]);
}

#[test]
fn a_closure_called_before_the_move_is_fine() {
    let body =
        format!("{LOCAL}    let () → int f ← () → int ∴ a.length()\n    print f()\n    eat(own a)");
    assert!(main_body(&body).is_empty());
}

#[test]
fn bind_form_own_from_unmarked_behaves_as_before() {
    let dead = "    let list<int> a ← [1, 2, 3]\n    let list<int> b ← own a\n    print b.length()";
    assert!(main_body(dead).is_empty());
    let live = "    let list<int> a ← [1, 2, 3]\n    let list<int> b ← own a\n    print a.length()\n    print b.length()";
    assert_eq!(main_body(live), ["unmarked_into_own"]);
}
