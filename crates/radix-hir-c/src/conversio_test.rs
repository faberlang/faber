//! Explicit conversions: the call chosen and where recovery goes.

use crate::test_support::emitted;

#[test]
fn an_infallible_conversion_is_one_expression() {
    let code = emitted(
        "main {\n    let int n ← 7\n    print n ↦ float\n    print n ↦ string\n    print n ↦ bool\n}",
    );
    assert!(code.contains("((double)l_n)"), "{code}");
    assert!(code.contains("faber_int_text(faber_a, l_n)"), "{code}");
    assert!(code.contains("((l_n) != 0)"), "{code}");
}

#[test]
fn parsing_text_without_recovery_fails_with_the_runners_text() {
    let code = emitted("main {\n    print \"42\" ↦ int\n}");
    assert!(code.contains("faber_try_text_int(\"42\""), "{code}");
    assert!(
        code.contains("faber_conv_fail(FABER_CONV_TEXT_INT, "),
        "{code}"
    );
}

#[test]
fn recovery_takes_the_place_of_a_failed_conversion() {
    let code = emitted("main {\n    print \"x\" ↦ int ⊥ -1\n}");
    assert!(code.contains("== 0 ? ((int64_t)faber_v"), "{code}");
    assert!(!code.contains("faber_conv_fail"), "{code}");
}

#[test]
fn a_float_to_integer_conversion_needs_its_rounding_hint() {
    let code = emitted("main {\n    let float f ← 2.5\n    print f ↦ int via HalfEven\n}");
    assert!(code.contains("faber_try_float_int(l_f, 3,"), "{code}");
}

#[test]
fn a_list_converts_to_a_fixed_bounded_copy() {
    let code =
        emitted("main {\n    let list<int> d ← [1, 2, 3]\n    print d ↦ list<int, size 2>\n}");
    assert!(
        code.contains("faber_list_i64_bounded(faber_a, l_d, 2,"),
        "{code}"
    );
}

#[test]
fn a_bounded_list_conversion_with_recovery_takes_the_fallback() {
    let code = emitted(
        "main {\n    let list<int> d ← [1, 2, 3]\n    print d ↦ list<int, size 2> ⊥ [7, 8]\n}",
    );
    assert!(
        code.contains("faber_list_i64_bounded(faber_a, l_d, 2,"),
        "{code}"
    );
    assert!(code.contains("faber_list_i64_of"), "{code}");
}

#[test]
fn a_radix_hint_reads_digits_of_that_radix() {
    let code = emitted("main {\n    print \"ff\" ↦ i32 via Hex\n}");
    assert!(code.contains("faber_try_text_int(\"ff\", 16,"), "{code}");
}

#[test]
fn text_and_float_to_bool_and_bool_to_float() {
    let code = emitted(
        "main {\n    let float f ← 2.5\n    print \"true\" ↦ bool\n    print f ↦ bool\n    print true ↦ float\n}",
    );
    // `text ↦ bool` parses the locale's `true` / `false` words only, and any
    // other text fails with the runner's text; `fractus ↦ bool` is non-zero.
    assert!(code.contains("strcmp("), "{code}");
    assert!(code.contains("FABER_CONV_TEXT_BOOL"), "{code}");
    assert!(code.contains("((l_f) != 0.0)"), "{code}");
    assert!(code.contains("? 1.0 : 0.0)"), "{code}");
}

#[test]
fn a_bounded_text_conversion_checks_its_capacity() {
    let code = emitted(
        "main {\n    let string raw ← \"ab\"\n    let string<size 8> t ← raw ↦ string<size 8>\n    print t\n}",
    );
    assert!(code.contains("faber_bounded_text(l_raw, 8)"), "{code}");
}

#[test]
fn a_bounded_ascii_resize_checks_the_new_capacity() {
    let code = emitted(
        "main {\n    let ascii<size 8> b ← 'abcd'\n    let ascii<size 4> shrunk ← b ↦ ascii<size 4>\n    print shrunk\n}",
    );
    assert!(code.contains("faber_text_length("), "{code}");
    assert!(
        code.contains("ascii to bounded ascii conversion failed"),
        "{code}"
    );
}

#[test]
fn an_ascii_pack_is_the_fixed_width_radix() {
    let code = emitted(
        "main {\n    let ascii<size 8> padded ← 37 ↦ ascii<size 8> via Hex\n    print padded\n}",
    );
    assert!(code.contains("faber_try_ascii_pack(faber_a,"), "{code}");
    assert!(code.contains(", 16, 8,"), "{code}");
}
