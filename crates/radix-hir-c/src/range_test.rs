//! Range loops and the output kinds that share a statement position.

use crate::test_support::emitted;

#[test]
fn a_bounded_range_walks_an_int64_counter_from_the_bounds() {
    let code = emitted("main {\n    for range 0‥3 let i {\n        print i\n    }\n}");
    for part in [
        "const faber_i128 faber_a0 = INT64_C(0);",
        "const faber_i128 faber_e0 = INT64_C(3);",
        "for (int64_t faber_r0 = (int64_t)faber_a0; faber_r0 < (int64_t)faber_e0; faber_r0 += 1) {",
        "const int64_t l_i = faber_r0;",
    ] {
        assert!(code.contains(part), "{part:?} missing in {code}");
    }
}

#[test]
fn a_range_with_runtime_bounds_keeps_the_exact_counter() {
    // Nothing folds until the program runs, so the walk stays on the exact
    // carrier: an inclusive walk to `i64::MAX` steps past it, and only the
    // boundedness proof may take the plain counter away.
    let code = emitted(
        "main {\n    mut int top ← 3\n    for range 0‥top let i {\n        print i\n    }\n}",
    );
    assert!(
        code.contains("const bool faber_u0 = faber_a0 <= faber_e0;"),
        "{code}"
    );
    assert!(
        code.contains("for (faber_i128 faber_r0 = faber_a0; faber_u0 ? faber_r0 < faber_e0 : faber_r0 > faber_e0; faber_r0 = faber_u0 ? faber_add(faber_r0, 1) : faber_sub(faber_r0, 1)) {"),
        "{code}"
    );
    assert!(
        code.contains("const int64_t l_i = (int64_t)faber_r0;"),
        "{code}"
    );
}

#[test]
fn an_inclusive_range_compares_with_the_end_included() {
    let code = emitted("main {\n    for range 0…3 let i {\n        print i\n    }\n}");
    assert!(
        code.contains("faber_r0 <= (int64_t)faber_e0; faber_r0 += 1"),
        "{code}"
    );
}

#[test]
fn an_inclusive_walk_to_the_cap_keeps_the_exact_counter() {
    // The inclusive walk to `i64::MAX` steps past it once, which no
    // `int64_t` counter holds, so the boundedness proof refuses it.
    let code = emitted("main {\n    for range 0…9223372036854775807 let i {\n    }\n}");
    assert!(code.contains("for (faber_i128 faber_r0"), "{code}");
}

#[test]
fn a_written_step_aborts_unless_positive() {
    let code = emitted(
        "main {\n    mut int s ← 2\n    for range 0‥9 step s let i {\n        print i\n    }\n}",
    );
    assert!(code.contains("const faber_i128 faber_s0 = l_s;"), "{code}");
    assert!(
        code.contains("if (!(faber_s0 > 0)) { faber_abort(\"range step must be positive\"); }"),
        "{code}"
    );
    assert!(code.contains("faber_add(faber_r0, faber_s0)"), "{code}");
}

#[test]
fn a_range_product_evaluates_every_bound_before_the_walk() {
    let code = emitted("main {\n    for range 0‥2, 0‥3 let i, j {\n        print i, j\n    }\n}");
    let heads = code.matches("for (int64_t faber_r").count();
    assert_eq!(heads, 2, "{code}");
    let second_bounds = code.find("faber_a1 =").expect("second axis bounds");
    let first_head = code.find("for (int64_t faber_r0").expect("first head");
    assert!(second_bounds < first_head, "{code}");
}

#[test]
fn a_list_position_binds_the_loop_index() {
    let code = emitted(
        "main {\n    let list<int> xs ← [5, 6]\n    for from xs at [i] let x {\n        print i, x\n    }\n}",
    );
    assert!(code.contains("const int64_t l_i = faber_i0;"), "{code}");
    assert!(
        code.contains("const int64_t l_x = faber_t0->items[faber_i0];"),
        "{code}"
    );
}

#[test]
fn debug_and_warn_write_to_standard_error() {
    let code = emitted("main {\n    debug \"d\"\n    warn \"w\"\n    print \"p\"\n}");
    assert_eq!(
        code.matches("fprintf(stderr, \"%s\\n\"").count(),
        2,
        "{code}"
    );
    assert!(code.contains("printf(\"%s\\n\", \"p\");"), "{code}");
}

#[test]
fn a_type_alias_is_resolved_not_emitted() {
    let code = emitted("type Count = int\nmain {\n    let Count n ← 3\n    print n\n}");
    assert!(code.contains("const int64_t l_n = INT64_C(3);"), "{code}");
}

#[test]
fn a_lockstep_walk_is_one_index_over_every_source() {
    let code = emitted(
        "main {\n    let list<string> labels ← [\"a\", \"b\"]\n    let list<int> values ← [1, 2]\n    for from labels, values at [i] let label, value {\n        print i, label, value\n    }\n}",
    );
    assert!(
        code.contains("faber_abort(\"itera lockstep sources must have equal length\");"),
        "{code}"
    );
    assert!(
        code.contains("const char *const l_label = faber_t0->items[faber_i2];"),
        "{code}"
    );
    assert!(
        code.contains("const int64_t l_value = faber_t1->items[faber_i2];"),
        "{code}"
    );
    assert!(code.contains("const int64_t l_i = faber_i2;"), "{code}");
}
