//! Optionals: a presence-flag struct for scalars, `NULL` for pointers.

use crate::test_support::emitted;

#[test]
fn an_optional_scalar_is_a_flag_and_a_value() {
    let code = emitted(
        "main {\n    const int ∪ none a ← 5\n    const int ∪ none b ← null\n    print a, b\n}",
    );
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
        "main {\n    const string ∪ none s ← null\n    if s is none {\n        print \"none\"\n    }\n}",
    );
    assert!(code.contains("const char *const l_s = NULL;"), "{code}");
    assert!(code.contains("if (l_s == NULL) {"), "{code}");
}

#[test]
fn coalesce_runs_its_fallback_only_for_none() {
    let code = emitted("main {\n    const int ∪ none a ← null\n    print a coalesce 9\n}");
    assert!(
        code.contains("faber_c0.has ? faber_c0.value : INT64_C(9)"),
        "{code}"
    );
}

#[test]
fn unwrap_aborts_on_none() {
    let code = emitted(
        "class Box {\n    var int n\n}\nmain {\n    const Box ∪ none b ← null\n    print b!.n\n}",
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
