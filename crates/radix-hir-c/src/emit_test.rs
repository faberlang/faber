//! Pinned-output tests for the scalar-and-print subset.

use crate::test_support::emitted;

const SCALARS: &str = include_str!("../tests/fixtures/run/scalars.c");
const C_EDGES: &str = include_str!("../tests/fixtures/run/c_edges.c");
const LOOPS: &str = include_str!("../tests/fixtures/run/loops.c");
const FUNCTIONS: &str = include_str!("../tests/fixtures/run/functions.c");
const NARROWING: &str = include_str!("../tests/fixtures/run/narrowing.c");

#[test]
fn scalars_program_is_pinned() {
    let source = include_str!("../tests/fixtures/run/scalars.fab");
    let source = crate::test_support::program("scalars.fab", source);
    assert_eq!(source, SCALARS);
}

#[test]
fn c_edges_program_is_pinned() {
    let source = include_str!("../tests/fixtures/run/c_edges.fab");
    let source = crate::test_support::program("c_edges.fab", source);
    assert_eq!(source, C_EDGES);
}

#[test]
fn print_of_literals_uses_printf() {
    let code = emitted("main {\n    print 1 + 2\n    print \"hi\"\n}");
    assert_eq!(
        code,
        "#include <inttypes.h>\n#include <stdio.h>\n\nint main(void) {\n    printf(\"%\" PRId64 \"\\n\", INT64_C(3));\n    printf(\"%s\\n\", \"hi\");\n    return 0;\n}\n"
    );
}

#[test]
fn a_program_that_prints_nothing_includes_nothing() {
    let code = emitted("main {\n}");
    assert_eq!(code, "int main(void) {\n    return 0;\n}\n");
}

#[test]
fn text_only_programs_do_not_include_inttypes() {
    let code = emitted("main {\n    print \"a\"\n}");
    assert!(!code.contains("inttypes"), "{code}");
    assert!(code.starts_with("#include <stdio.h>\n\n"), "{code}");
}

#[test]
fn constant_folding_is_exact_and_floors() {
    let code = emitted(
        "main {\n    const int neg ← 0 - 7\n    print 2 * 3 + 4\n    print neg\n    print neg / 2\n    print neg % 2\n    print 9223372036854775807\n}",
    );
    assert!(code.contains("INT64_C(10))"), "{code}");
    assert!(
        code.contains("const int64_t l_neg = -INT64_C(7);"),
        "{code}"
    );
    assert!(code.contains("\\n\", l_neg);"), "{code}");
    assert!(code.contains("-INT64_C(4))"), "{code}");
    assert!(code.contains("INT64_C(1))"), "{code}");
    assert!(code.contains("INT64_C(9223372036854775807))"), "{code}");
}

#[test]
fn the_minimum_integer_is_spelled_int64_min() {
    let code = emitted("main {\n    print 0 - 9223372036854775807 - 1\n}");
    assert!(code.contains("\\n\", INT64_MIN);"), "{code}");
}

#[test]
fn several_values_print_space_separated_on_one_line() {
    let code = emitted("main {\n    print 1, \"a\", 2\n}");
    assert!(
        code.contains(
            "printf(\"%\" PRId64 \" %s %\" PRId64 \"\\n\", INT64_C(1), \"a\", INT64_C(2));"
        ),
        "{code}"
    );
}

#[test]
fn percent_in_text_is_an_argument_not_a_format() {
    let code = emitted("main {\n    print \"100%s\"\n}");
    assert!(code.contains("printf(\"%s\\n\", \"100%s\");"), "{code}");
}

#[test]
fn text_escapes_are_c_escapes() {
    let code = emitted("main {\n    print \"a\\\"b\\\\c\\nd\"\n}");
    assert!(code.contains("\"a\\\"b\\\\c\\nd\""), "{code}");
}

#[test]
fn non_ascii_text_is_octal_bytes_so_hex_digits_after_it_survive() {
    // é is C3 A9; a `\x` escape would swallow the following `ab`.
    let code = emitted("main {\n    print \"h\u{e9}ab\"\n}");
    assert!(code.contains("\"h\\303\\251ab\""), "{code}");
}

#[test]
fn question_marks_cannot_form_trigraphs() {
    let code = emitted("main {\n    print \"what??!\"\n}");
    assert!(code.contains("\"what\\?\\?!\""), "{code}");
}

#[test]
fn locals_are_typed_const_and_read_back() {
    let code = emitted(
        "main {\n    const int n ← 40 + 2\n    const string s ← \"x\"\n    var int m ← 1\n    var string t ← \"y\"\n    print n, s, m, t\n}",
    );
    assert!(
        code.contains("    const int64_t l_n = INT64_C(42);\n"),
        "{code}"
    );
    assert!(
        code.contains("    const char *const l_s = \"x\";\n"),
        "{code}"
    );
    assert!(code.contains("    int64_t l_m = INT64_C(1);\n"), "{code}");
    assert!(code.contains("    const char *l_t = \"y\";\n"), "{code}");
    assert!(code.contains(", l_n, l_s, l_m, l_t);"), "{code}");
}

/// Names that are ordinary in Faber but reserved in C (keywords, header
/// objects and macros) take the shared `l_` prefix like every other local, so
/// the C keeps the source spelling after the prefix.
#[test]
fn c_words_do_not_collide_with_faber_names() {
    let code = emitted(
        "main {\n    const int char ← 1\n    const int goto ← 2\n    const int stdout ← 3\n    const int INT64_MAX ← 4\n    print char, goto, stdout, INT64_MAX\n}",
    );
    for decl in [
        "const int64_t l_char = INT64_C(1);",
        "const int64_t l_goto = INT64_C(2);",
        "const int64_t l_stdout = INT64_C(3);",
        "const int64_t l_INT64_MAX = INT64_C(4);",
    ] {
        assert!(code.contains(decl), "{decl} in {code}");
    }
    assert!(
        code.contains(", l_char, l_goto, l_stdout, l_INT64_MAX);"),
        "{code}"
    );
}

#[test]
fn no_allocation_or_free_is_emitted() {
    let code = emitted("main {\n    print \"a\"\n}");
    for banned in ["malloc(", "calloc(", "realloc(", "free("] {
        assert!(!code.contains(banned), "{code}");
    }
}

#[test]
fn loops_program_is_pinned() {
    let source = include_str!("../tests/fixtures/run/loops.fab");
    let source = crate::test_support::program("loops.fab", source);
    assert_eq!(source, LOOPS);
}

/// The case the target exists to get right: `n + 1` over a variable could
/// overflow `int64_t`, which is undefined behavior in C. It runs in the exact
/// helpers and is narrowed once, at the store, with the runner's wording.
#[test]
fn arithmetic_over_a_variable_is_exact_and_checked_at_the_store() {
    let code = emitted("main {\n    var int n ← 1\n    n ← n + 1\n    print n\n}");
    assert!(
        code.contains(
            "    l_n = (__extension__ ({ int faber_o = 0; int64_t faber_r = faber_add_o(l_n, INT64_C(1), &faber_o); faber_o ? faber_store_i64(faber_add(l_n, INT64_C(1)), \"assignment to `n`\") : faber_r; }));\n"
        ),
        "{code}"
    );
    assert!(!code.contains("l_n + "), "{code}");
}

#[test]
fn a_declaration_store_names_the_declaration() {
    let code = emitted("main {\n    var int a ← 2\n    const int b ← a * 3\n    print b\n}");
    assert!(
        code.contains("faber_store_i64(faber_mul(l_a, INT64_C(3)), \"declaration of `b`\")"),
        "{code}"
    );
}

#[test]
fn helpers_are_emitted_only_when_used() {
    let code = emitted("main {\n    var int n ← 1\n    n ← n + 1\n    print n\n}");
    for present in [
        "faber_i128",
        "faber_trap",
        "faber_cap",
        "faber_add",
        "faber_store_i64",
        "<stdlib.h>",
    ] {
        assert!(code.contains(present), "{present} missing in {code}");
    }
    for absent in ["faber_sub", "faber_mul", "faber_div", "faber_rem"] {
        assert!(!code.contains(absent), "{absent} present in {code}");
    }
    let plain = emitted("main {\n    print 1\n}");
    assert!(!plain.contains("faber_"), "{plain}");
}

#[test]
fn remainder_pulls_in_floor_division() {
    let code = emitted("main {\n    var int n ← 7\n    var int d ← 2\n    print n % d\n}");
    assert!(code.contains("static faber_i128 faber_div("), "{code}");
    assert!(code.contains("static faber_i128 faber_rem("), "{code}");
}

#[test]
fn an_unstored_exact_value_prints_through_the_text_helper() {
    let code = emitted("main {\n    var int n ← 1\n    print n + 1\n}");
    assert!(
        code.contains(
            "printf(\"%s\\n\", faber_i128_text(faber_add(l_n, INT64_C(1)), (char[41]){0}));"
        ),
        "{code}"
    );
}

#[test]
fn negation_of_a_variable_is_exact() {
    let code = emitted("main {\n    var int n ← 1\n    print -n\n}");
    assert!(code.contains("faber_sub(0, l_n)"), "{code}");
}

#[test]
fn control_flow_is_c_control_flow() {
    let code = emitted(
        "main {\n    var int i ← 0\n    while true {\n        i ← i + 1\n        if i ≡ 3 {\n            continue\n        } else {\n            print i\n        }\n        if i ≥ 5 and not (i ≡ 9) {\n            break\n        }\n    }\n}",
    );
    for line in [
        "    while (1) {\n",
        "        if (l_i == INT64_C(3)) {\n",
        "            continue;\n",
        "        } else {\n",
        "        if ((l_i >= INT64_C(5)) && (!(l_i == INT64_C(9)))) {\n",
        "            break;\n",
    ] {
        assert!(code.contains(line), "{line:?} missing in {code}");
    }
}

#[test]
fn functions_program_is_pinned() {
    let source = include_str!("../tests/fixtures/run/functions.fab");
    let source = crate::test_support::program("functions.fab", source);
    assert_eq!(source, FUNCTIONS);
}

#[test]
fn functions_get_prototypes_and_prefixed_names() {
    let code = emitted(
        "fn twice(int x) → int {\n    return x * 2\n}\nfn hello() {\n    print \"hi\"\n}\nmain {\n    hello()\n    print twice(4)\n}",
    );
    for line in [
        "static int64_t f_twice(int64_t);\n",
        "static void f_hello(void);\n",
        "static int64_t f_twice(int64_t p_x) {\n",
        "faber_mul_o(p_x, INT64_C(2), &faber_o); faber_o ? faber_store_i64(faber_mul(p_x, INT64_C(2)), \"return\") : faber_r;",
        "    f_hello();\n",
    ] {
        assert!(code.contains(line), "{line:?} missing in {code}");
    }
}

/// C leaves argument and operand order unspecified; Faber is left to right,
/// so operands that share an evaluation with a call are bound in order.
#[test]
fn calls_are_sequenced_left_to_right() {
    let code = emitted(
        "fn f(int x) → int {\n    print x\n    return x\n}\nmain {\n    print f(1) + f(2)\n}",
    );
    assert!(
        code.contains("int64_t faber_t0 = f_f(INT64_C(1)); int64_t faber_t1 = f_f(INT64_C(2)); faber_add(faber_t0, faber_t1);"),
        "{code}"
    );
}

#[test]
fn a_call_without_other_calls_is_not_sequenced() {
    let code = emitted(
        "fn f(int x) → int {\n    return x\n}\nmain {\n    var int n ← 1\n    print f(n)\n}",
    );
    assert!(!code.contains("faber_t"), "{code}");
}

#[test]
fn an_exact_argument_is_checked_like_a_store() {
    let code = emitted(
        "fn f(int x) → int {\n    return x\n}\nmain {\n    var int n ← 1\n    print f(n + 1)\n}",
    );
    assert!(
        code.contains("faber_o ? faber_store_i64(faber_add(l_n, INT64_C(1)), \"argument\")"),
        "{code}"
    );
}

#[test]
fn booleans_print_through_the_module_display_tokens() {
    let code = emitted("main {\n    var bool b ← 1 ≺ 2\n    print b\n}");
    assert!(code.contains("#include <stdbool.h>"), "{code}");
    assert!(
        code.contains("    bool l_b = (INT64_C(1) < INT64_C(2));\n"),
        "{code}"
    );
    assert!(
        code.contains("printf(\"%s\\n\", ((l_b) ? \"true\" : \"false\"));"),
        "{code}"
    );
}

#[test]
fn module_constants_are_static_const() {
    let code = emitted("const int LIMIT = 3\nmain {\n    print LIMIT\n}");
    assert!(
        code.contains("static const int64_t k_LIMIT = INT64_C(3);\n"),
        "{code}"
    );
    assert!(code.contains(", k_LIMIT);"), "{code}");
}

#[test]
fn narrowing_program_is_pinned() {
    let source = include_str!("../tests/fixtures/run/narrowing.fab");
    let source = crate::test_support::program("narrowing.fab", source);
    assert_eq!(source, NARROWING);
}

/// C4: division and remainder of an `int64_t` by a constant other than 0 and
/// -1 cannot leave `int64_t`, so they stay there with no exact helper.
#[test]
fn division_by_a_safe_constant_stays_in_int64() {
    let code = emitted("main {\n    var int n ← 7\n    print n / 2, n % -3\n}");
    assert!(code.contains("faber_fdiv_i64(l_n, INT64_C(2))"), "{code}");
    assert!(code.contains("faber_fmod_i64(l_n, -INT64_C(3))"), "{code}");
    assert!(!code.contains("faber_i128"), "{code}");
}

/// `x / -1` can leave `int64_t` (`INT64_MIN / -1`), and a variable divisor
/// can be zero or -1, so both keep the exact helper.
#[test]
fn division_by_minus_one_or_a_variable_stays_exact() {
    let code = emitted("main {\n    var int n ← 7\n    var int d ← 2\n    print n / -1, n / d\n}");
    assert!(code.contains("faber_div(l_n, -INT64_C(1))"), "{code}");
    assert!(code.contains("faber_div(l_n, l_d)"), "{code}");
    assert!(!code.contains("faber_fdiv_i64"), "{code}");
}

/// The fallback re-evaluates the expression, so an expression with a call
/// never takes the fast path: the call would run twice.
#[test]
fn an_expression_with_a_call_has_no_fast_path() {
    let code = emitted(
        "fn f() → int {\n    print \"f\"\n    return 1\n}\nmain {\n    var int n ← 1\n    n ← n + f()\n    print n\n}",
    );
    assert!(!code.contains("faber_add_o"), "{code}");
    assert!(
        code.contains(
            "int64_t faber_t0 = l_n; int64_t faber_t1 = f_f(); faber_add(faber_t0, faber_t1);"
        ),
        "{code}"
    );
}

/// Typecheck puts no store node where its range rule proves the result fits
/// (`big + 1 - 1`); the value narrows through `faber_fit_i64`.
#[test]
fn a_proven_range_narrows_through_fit() {
    let code = emitted(
        "main {\n    var int big ← 9223372036854775807\n    var int back ← big + 1 - 1\n    print back, big\n}",
    );
    assert!(code.contains("faber_o ? faber_fit_i64(faber_sub(faber_add(l_big, INT64_C(1)), INT64_C(1))) : faber_r;"), "{code}");
}
