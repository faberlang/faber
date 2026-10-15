//! Pinned-output tests for the scalar-and-print subset.

use crate::test_support::emitted;

const SCALARS: &str = include_str!("../tests/fixtures/run/scalars.c");
const C_EDGES: &str = include_str!("../tests/fixtures/run/c_edges.c");

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
    assert!(code.contains("const int64_t neg = -INT64_C(7);"), "{code}");
    assert!(code.contains("\\n\", neg);"), "{code}");
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
        code.contains("    const int64_t n = INT64_C(42);\n"),
        "{code}"
    );
    assert!(
        code.contains("    const char *const s = \"x\";\n"),
        "{code}"
    );
    assert!(code.contains("    int64_t m = INT64_C(1);\n"), "{code}");
    assert!(code.contains("    const char *t = \"y\";\n"), "{code}");
    assert!(code.contains(", n, s, m, t);"), "{code}");
}

#[test]
fn c_words_do_not_collide_with_faber_names() {
    let code = emitted(
        "main {\n    const int stdout ← 1\n    const int printf ← 2\n    const int size_t ← 3\n    print stdout, printf, size_t\n}",
    );
    assert!(
        code.contains("const int64_t stdout_1 = INT64_C(1);"),
        "{code}"
    );
    assert!(
        code.contains("const int64_t printf_1 = INT64_C(2);"),
        "{code}"
    );
    assert!(
        code.contains("const int64_t size_t_v = INT64_C(3);"),
        "{code}"
    );
    assert!(code.contains(", stdout_1, printf_1, size_t_v);"), "{code}");
}

#[test]
fn no_allocation_or_free_is_emitted() {
    let code = emitted("main {\n    print \"a\"\n}");
    for banned in ["malloc(", "calloc(", "realloc(", "free("] {
        assert!(!code.contains(banned), "{code}");
    }
}
