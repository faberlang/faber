//! Control flow in value and statement position: conditionals, `elif` chains,
//! do-while, increment, panic and assert, and comparisons of text and booleans.

use crate::test_support::emitted;

#[test]
fn a_conditional_value_is_a_c_conditional() {
    let code = emitted(
        "main {\n    const int a ← 3\n    const int b ← 9\n    const int m ← a ≻ b ✓ a ✗ b\n    print m\n}",
    );
    assert!(code.contains("((l_a > l_b) ? l_a : l_b)"), "{code}");
}

#[test]
fn an_elif_chain_is_written_flat() {
    let code = emitted(
        "main {\n    const int h ← 14\n    if h ≺ 6 {\n        print \"a\"\n    }\n    elif h ≺ 12 {\n        print \"b\"\n    }\n    else {\n        print \"c\"\n    }\n}",
    );
    assert!(code.contains("} else if "), "{code}");
    assert_eq!(code.matches("} else {").count(), 1, "{code}");
}

#[test]
fn a_do_while_runs_continue_through_the_test() {
    let code = emitted(
        "main {\n    var int n ← 0\n    do {\n        n ↑\n        if n ≥ 2 { continue }\n        print n\n    } while n ≺ 4\n}",
    );
    assert!(code.contains("    do {\n"), "{code}");
    assert!(code.contains("} while (l_n < INT64_C(4));"), "{code}");
}

#[test]
fn increment_stores_through_the_assignment_check() {
    let code = emitted("main {\n    var int n ← 0\n    n ↑\n    n ↓\n}");
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
        "main {\n    const int x ← 1\n    assert x ≡ 1\n    assert x ≡ 1 panic \"one\"\n    panic \"boom\"\n}",
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
        "main {\n    print \"a\" ≺ \"b\"\n    print \"a\" ≡ \"a\"\n    const bool p ← true\n    print p ≢ false\n}",
    );
    assert!(code.contains("(strcmp(\"a\", \"b\") < 0)"), "{code}");
    assert!(code.contains("(strcmp(\"a\", \"a\") == 0)"), "{code}");
    assert!(code.contains("((l_p) != (0))"), "{code}");
}

#[test]
fn a_switch_is_an_if_chain_over_once_evaluated_subjects() {
    let code = emitted(
        "main {\n    const int code ← 404\n    switch code {\n        case 200 { print \"ok\" }\n        case 404 { print \"missing\" }\n        default { print \"other\" }\n    }\n}",
    );
    assert!(code.contains("int64_t faber_t0 = l_code;"), "{code}");
    assert!(code.contains("if (faber_t0 == INT64_C(200)) {"), "{code}");
    assert!(
        code.contains("} else if (faber_t0 == INT64_C(404)) {"),
        "{code}"
    );
    assert!(code.contains("} else {"), "{code}");
}

#[test]
fn a_text_switch_compares_with_strcmp() {
    let code = emitted(
        "main {\n    const string s ← \"a\"\n    switch s {\n        case \"a\" { print 1 }\n    }\n}",
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
    let code = emitted("main {\n    const int pending\n    pending ← 7\n    print pending\n}");
    assert!(code.contains("int64_t l_pending;"), "{code}");
    assert!(code.contains("l_pending = INT64_C(7);"), "{code}");
}
