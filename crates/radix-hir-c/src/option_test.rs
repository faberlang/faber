//! Optionals: a presence-flag struct for scalars, `NULL` for pointers.

use crate::test_support::emitted;

#[test]
fn an_optional_scalar_is_a_flag_and_a_value() {
    let code =
        emitted("main {\n    let int ∪ none a ← 5\n    let int ∪ none b ← null\n    print a, b\n}");
    assert!(
        code.contains("const faber_opt_i64 l_a = ((faber_opt_i64){1, INT64_C(5)});"),
        "{code}"
    );
    assert!(
        code.contains("const faber_opt_i64 l_b = ((faber_opt_i64){0});"),
        "{code}"
    );
    assert!(
        code.contains("faber_c0.has ? faber_int_text(faber_a, faber_c0.value) : \"none\""),
        "{code}"
    );
}

#[test]
fn an_optional_text_is_a_nullable_pointer() {
    let code = emitted(
        "main {\n    let string ∪ none s ← null\n    if s is none {\n        print \"none\"\n    }\n}",
    );
    assert!(code.contains("const char *const l_s = NULL;"), "{code}");
    assert!(code.contains("if (l_s == NULL) {"), "{code}");
}

#[test]
fn coalesce_runs_its_fallback_only_for_none() {
    let code = emitted("main {\n    let int ∪ none a ← null\n    print a coalesce 9\n}");
    assert!(
        code.contains("faber_c0.has ? faber_c0.value : INT64_C(9)"),
        "{code}"
    );
}

#[test]
fn unwrap_aborts_on_none() {
    let code = emitted(
        "class Box {\n    var int n\n}\nmain {\n    let Box ∪ none b ← null\n    print b!.n\n}",
    );
    assert!(code.contains("faber_abort(\"unwrap of nihil\");"), "{code}");
}

#[test]
fn an_optional_result_returns_the_struct() {
    let code = emitted(
        "fn find(int n) → int ∪ none {\n    if n ≻ 0 {\n        return n\n    }\n    return null\n}\nmain {\n    print find(1)\n}",
    );
    assert!(code.contains("return ((faber_opt_i64){1, p_n});"), "{code}");
    assert!(code.contains("return ((faber_opt_i64){0});"), "{code}");
}

#[test]
fn a_creo_hook_runs_on_the_finished_object() {
    let code = crate::test_support::emitted(
        "class Box {\n    var int n = 0\n    fn creo() → void {\n        if self.n ≻ 9 {\n            self.n ← 9\n        }\n    }\n}\nmain {\n    let Box b ← Box { n = 50 }\n    print b.n\n}",
    );
    let set = code.find("->m_n = INT64_C(50);").expect("field set");
    let hook = code.find("g_Box_creo(faber_a, faber_t").expect("hook call");
    assert!(set < hook, "{code}");
}

#[test]
fn a_sponte_parameter_is_the_optional_of_its_declared_type() {
    // The bare `optional` slot: the C storage is the optional, a body read
    // keeps the none (the runner holds the omitted nil for coalesce), and an
    // omitted call passes none.
    let code = emitted(
        "fn pick(int x optional) → int {\n    return x coalesce 3\n}\nmain {\n    print pick()\n    print pick(9)\n}",
    );
    assert!(code.contains("faber_opt_i64 p_x"), "{code}");
    assert!(
        code.contains("faber_c0.has ? faber_c0.value : INT64_C(3)"),
        "{code}"
    );
    assert!(code.contains("f_pick(((faber_opt_i64){0}))"), "{code}");
    assert!(
        code.contains("f_pick(((faber_opt_i64){1, INT64_C(9)}))"),
        "{code}"
    );
}

#[test]
fn an_optional_coalesce_parameter_keeps_its_declared_type() {
    // `optional coalesce <expr>`: the call site runs the default, so the
    // parameter is the plain declared type and the body adds without a none.
    let code = emitted(
        "fn page(int n optional coalesce 1) → int {\n    return n + 1\n}\nmain {\n    print page()\n    print page(7)\n}",
    );
    assert!(code.contains("f_page(int64_t p_n)"), "{code}");
    assert!(code.contains("f_page(INT64_C(1))"), "{code}");
    assert!(code.contains("f_page(INT64_C(7))"), "{code}");
}

#[test]
fn an_assertion_index_reads_the_element() {
    let code =
        emitted("main {\n    let list<string, 3> xs ← [\"a\", \"b\", \"c\"]\n    print xs![0]\n}");
    assert!(code.contains("faber_list_text_at("), "{code}");
}

#[test]
fn a_sponte_parameter_that_is_ignored_is_marked_used() {
    let code = emitted(
        "fn greet(string nomen, string titulus optional) → string {\n    return nomen\n}\nmain {\n    print greet(\"a\")\n}",
    );
    assert!(code.contains("(void)p_titulus;"), "{code}");
}

#[test]
fn a_bare_none_prints_the_none_word() {
    let code = emitted("main {\n    let none nothing ← null\n    print nothing\n}");
    assert!(code.contains("\"none\""), "{code}");
}
