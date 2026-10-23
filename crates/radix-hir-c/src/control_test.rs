//! Control flow in value and statement position: conditionals, `elif` chains,
//! do-while, increment, panic and assert, and comparisons of text and booleans.

use crate::test_support::emitted;

#[test]
fn handled_block_runs_its_catch_arm() {
    let code = crate::test_support::program(
        "error_channel.fab",
        include_str!("../tests/fixtures/run/error_channel.fab"),
    );
    assert!(
        code.contains("faber_chan faber_c0 = {false, NULL};"),
        "{code}"
    );
    assert!(code.contains("if (faber_c0.set) {"), "{code}");
    assert!(code.contains("printf(\"%s\\n\", faber_join("), "{code}");
    assert!(code.contains("caught:"), "{code}");
    assert!(code.contains("(const char *)faber_c0.value"), "{code}");
}

#[test]
fn handled_block_carries_a_genus_error() {
    let code = crate::test_support::emitted(
        "union Fault { Missing { string path } }\nfn risky() → int ⇥ Fault {\n    return 1\n}\nmain {\n    do { print risky() } catch err { print \"caught\" }\n}",
    );
    assert!(code.contains("(g_Fault *)faber_c0.value"), "{code}");
}

#[test]
fn a_conditional_value_is_a_c_conditional() {
    let code = emitted(
        "main {\n    let int a ← 3\n    let int b ← 9\n    let int m ← a ≻ b ✓ a ✗ b\n    print m\n}",
    );
    assert!(code.contains("((l_a > l_b) ? l_a : l_b)"), "{code}");
}

#[test]
fn an_elif_chain_is_written_flat() {
    let code = emitted(
        "main {\n    let int h ← 14\n    if h ≺ 6 {\n        print \"a\"\n    }\n    elif h ≺ 12 {\n        print \"b\"\n    }\n    else {\n        print \"c\"\n    }\n}",
    );
    assert!(code.contains("} else if "), "{code}");
    assert_eq!(code.matches("} else {").count(), 1, "{code}");
}

#[test]
fn a_do_while_runs_continue_through_the_test() {
    let code = emitted(
        "main {\n    mut int n ← 0\n    do {\n        n ↑\n        if n ≥ 2 { continue }\n        print n\n    } while n ≺ 4\n}",
    );
    assert!(code.contains("    do {\n"), "{code}");
    assert!(code.contains("} while (l_n < INT64_C(4));"), "{code}");
}

#[test]
fn increment_stores_through_the_assignment_check() {
    let code = emitted("main {\n    mut int n ← 0\n    n ↑\n    n ↓\n}");
    assert!(
        code.contains("l_n = faber_store_i64(faber_add(l_n, 1), \"assignment to `n`\");"),
        "{code}"
    );
    assert!(
        code.contains("l_n = faber_store_i64(faber_sub(l_n, 1), \"assignment to `n`\");"),
        "{code}"
    );
}

#[test]
fn panic_and_assert_abort_with_the_message() {
    let code = emitted(
        "main {\n    let int x ← 1\n    assert x ≡ 1\n    assert x ≡ 1 panic \"one\"\n    panic \"boom\"\n}",
    );
    assert!(
        code.contains("faber_abort(\"assertion failed\");"),
        "{code}"
    );
    assert!(code.contains("faber_abort(\"one\");"), "{code}");
    assert!(code.contains("faber_abort(\"boom\");"), "{code}");
}

#[test]
fn text_compares_by_bytes_and_booleans_by_equality() {
    let code = emitted(
        "main {\n    print \"a\" ≺ \"b\"\n    print \"a\" ≡ \"a\"\n    let bool p ← true\n    print p ≢ false\n}",
    );
    assert!(code.contains("(strcmp(\"a\", \"b\") < 0)"), "{code}");
    assert!(code.contains("(strcmp(\"a\", \"a\") == 0)"), "{code}");
    assert!(code.contains("((l_p) != (0))"), "{code}");
}

#[test]
fn an_integer_switch_is_a_jump_table_over_once_evaluated_subjects() {
    let code = emitted(
        "main {\n    let int code ← 404\n    switch code {\n        case 200 { print \"ok\" }\n        case 404 { print \"missing\" }\n        default { print \"other\" }\n    }\n}",
    );
    assert!(code.contains("int64_t faber_t0 = l_code;"), "{code}");
    assert!(code.contains("switch (faber_t0) {"), "{code}");
    assert!(code.contains("case INT64_C(200): {"), "{code}");
    assert!(code.contains("case INT64_C(404): {"), "{code}");
    assert!(code.contains("default: {"), "{code}");
    assert!(code.contains("} break;"), "{code}");
}

#[test]
fn a_switch_with_a_break_keeps_the_if_chain() {
    // A Faber break must leave the loop around the dispatch; inside a C
    // `switch` it would leave the switch instead, so the arm that holds one
    // keeps the whole statement on the chain.
    let code = emitted(
        "main {\n    mut int n ← 0\n    while n ≺ 3 {\n        n ← n + 1\n        switch n {\n            case 2 { print \"two\" }\n            case 3 { break }\n            default { print \"other\" }\n        }\n    }\n    print n\n}",
    );
    assert!(!code.contains("switch ("), "{code}");
    assert!(
        code.contains("} else if (faber_t0 == INT64_C(3)) {"),
        "{code}"
    );
}

#[test]
fn a_text_switch_compares_with_strcmp() {
    let code = emitted(
        "main {\n    let string s ← \"a\"\n    switch s {\n        case \"a\" { print 1 }\n    }\n}",
    );
    assert!(code.contains("(strcmp(faber_t0, \"a\") == 0)"), "{code}");
}

#[test]
fn a_module_without_an_entry_fails_as_the_runner_does() {
    let code = emitted("fn f() → int {\n    return 1\n}");
    assert!(
        code.contains("faber_trap(\"no entry function in MIR program\");"),
        "{code}"
    );
}

#[test]
fn a_deferred_binding_is_declared_unwritten() {
    let code = emitted("main {\n    let int pending\n    pending ← 7\n    print pending\n}");
    assert!(code.contains("int64_t l_pending;"), "{code}");
    assert!(code.contains("l_pending = INT64_C(7);"), "{code}");
}

#[test]
fn a_thread_lane_binder_is_declared_metadata() {
    // `thread f` names a loop lane for later parallel lowering and is ignored
    // on the way to MIR, so the emitted loop is the plain single-source loop.
    let code = emitted(
        "main {\n    let list<int> xs ← [1, 2]\n    for from xs at [i] thread lane let v {\n        print i, v\n    }\n}",
    );
    assert!(code.contains("faber_i0 < faber_t0->len"), "{code}");
    assert!(!code.contains("lane"), "{code}");
}

#[test]
fn a_guard_section_is_early_exit_clauses() {
    // `guard { if <cond> { return x } }`: each clause is an `if` whose body
    // may return from the function; the ordinary body follows.
    let code = emitted(
        "fn clamp(int value, int low) → int {\n    guard {\n        if value ≺ low {\n            return low\n        }\n    }\n    return value\n}\nmain {\n    print clamp(5, 0)\n}",
    );
    assert!(code.contains("if (p_value < p_low) {"), "{code}");
    assert!(code.contains("return p_low;"), "{code}");
    assert!(code.contains("return p_value;"), "{code}");
}

#[test]
fn range_membership_is_the_two_bounds() {
    let code = emitted(
        "main {\n    const int age = 25\n    print age ∈ 0‥100\n    print age ∈ 18…65\n    print age ∉ 0‥25\n}",
    );
    assert!(
        code.contains("(INT64_C(0) <= l_age) && (l_age < INT64_C(100))"),
        "{code}"
    );
    assert!(
        code.contains("(INT64_C(18) <= l_age) && (l_age <= INT64_C(65))"),
        "{code}"
    );
    assert!(
        code.contains("(!((INT64_C(0) <= l_age) && (l_age < INT64_C(25))))"),
        "{code}"
    );
}
