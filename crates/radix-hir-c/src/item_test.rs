//! File-scope behavior: storage of uncalled functions, the header a signature
//! needs, and module constants that are built at run time.

use crate::test_support::emitted;

#[test]
fn an_uncalled_function_is_marked_unused() {
    let code = emitted(
        "fn used() → int {\n    return 1\n}\nfn idle() → int {\n    return 2\n}\nmain {\n    print used()\n}",
    );
    assert!(code.contains("static int64_t f_used(void);\n"), "{code}");
    assert!(
        code.contains("static __attribute__((unused)) int64_t f_idle(void);\n"),
        "{code}"
    );
    assert!(
        code.contains("static __attribute__((unused)) int64_t f_idle(void) {\n"),
        "{code}"
    );
}

#[test]
fn a_function_only_it_calls_is_marked_unused() {
    let code = emitted(
        "fn down(int n) → int {\n    if n ≤ 0 {\n        return 0\n    }\n    return down(n - 1)\n}",
    );
    assert!(
        code.contains("static __attribute__((unused)) int64_t f_down(int64_t);\n"),
        "{code}"
    );
}

#[test]
fn a_result_type_alone_pulls_in_the_header() {
    let code = emitted("fn yes() → bool {\n    return true\n}");
    assert!(code.contains("#include \"faber.h\""), "{code}");
    assert!(
        code.contains("static __attribute__((unused)) bool f_yes(void);\n"),
        "{code}"
    );
}

#[test]
fn a_list_constant_is_set_in_main() {
    let code = emitted("const list<int> XS = [1, 2, 3]\nmain {\n    print XS.length()\n}");
    assert!(code.contains("static faber_list_i64 *k_XS;\n"), "{code}");
    let main = code.find("int main(void) {").expect("main");
    let init = code
        .find("    k_XS = faber_list_i64_of(faber_a, 3,")
        .expect("initializer");
    let root = code.find("faber_arena faber_root").expect("root arena");
    assert!(root < init && main < init, "{code}");
}

#[test]
fn a_literal_constant_stays_at_file_scope() {
    let code = emitted("const string GREETING = \"hi\"\nmain {\n    print GREETING\n}");
    assert!(
        code.contains("static const char *const k_GREETING = \"hi\";\n"),
        "{code}"
    );
}
