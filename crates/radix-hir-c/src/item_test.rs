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

#[test]
fn a_test_function_is_not_part_of_the_program() {
    let code =
        emitted("test \"never emitted\" {\n    assert 1 ≡ 2\n}\nmain {\n    print \"run\"\n}");
    assert!(!code.contains("never"), "{code}");
    assert!(code.contains("printf(\"%s\\n\", \"run\");"), "{code}");
}

#[test]
fn entry_arguments_are_the_text_list_after_the_program_name() {
    let code = emitted("main args args {\n    print args\n}");
    assert!(code.contains("int main(int argc, char **argv) {"), "{code}");
    assert!(
        code.contains("faber_list_text *const l_args = faber_argv_text(faber_a, argc, argv);"),
        "{code}"
    );
}

#[test]
fn an_uncalled_generic_function_is_skipped() {
    // `faber run` runs the entry only; a generic body has no C text until it
    // is instantiated, and one nothing calls contributes nothing.
    let code = emitted("fn id<T>(T x) → T {\n    return x\n}\nmain {\n    print \"ok\"\n}");
    assert!(!code.contains("f_id"), "{code}");
    assert!(code.contains("\"ok\""), "{code}");
}

#[test]
fn a_schema_declaration_has_no_runtime_effect() {
    // `faber run` declares the heading and runs the entry block; the schema
    // itself is compile-time only, and a use of its type is rejected where it
    // is used.
    let code = emitted(
        "schema Persona {\n    column string nomen\n    column int aetas\n}\nmain {\n    print \"schema declared\"\n}",
    );
    assert!(code.contains("\"schema declared\""), "{code}");
    assert!(!code.contains("Persona"), "{code}");
}

#[test]
fn a_mut_scalar_parameter_borrows_the_callers_cell() {
    // `mut int x` is a pointer to the caller's cell: the body reads and stores
    // through it, and a `mut` argument passes the caller's address.
    let code = emitted(
        "fn bump(mut int x) → void {\n    x ← x + 1\n}\nmain {\n    mut int n ← 1\n    bump(mut n)\n    print n\n}",
    );
    assert!(code.contains("static void f_bump(int64_t *p_x)"), "{code}");
    assert!(code.contains("f_bump((&l_n))"), "{code}");
    assert!(code.contains("(*p_x) ="), "{code}");
}

#[test]
fn a_ref_text_parameter_is_the_identity_pointer() {
    // Text is already a `const char *`; a `ref` view passes it unchanged.
    let code = emitted(
        "fn show(ref string s) → void {\n    print s\n}\nmain {\n    show(\"hi\")\n}",
    );
    assert!(code.contains("static void f_show(const char *p_s)"), "{code}");
}
