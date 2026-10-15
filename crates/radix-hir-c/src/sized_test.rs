//! Sized integers: a plain C cell, read widened, written through a range check.

use crate::test_support::emitted;

#[test]
fn a_sized_local_is_a_c_integer_of_that_width() {
    let code = emitted("main {\n    let u8 a ← 200\n    mut i32 b ← -5\n    print a, b\n}");
    assert!(code.contains("const uint8_t l_a = INT64_C(200);"), "{code}");
    assert!(code.contains("int32_t l_b = -INT64_C(5);"), "{code}");
    // Read as `int64_t`, so `%PRId64` always gets what it asks for; the
    // runner joins sized integers into one line like any other integer.
    assert!(
        code.contains(
            "printf(\"%\" PRId64 \" %\" PRId64 \"\\n\", ((int64_t)l_a), ((int64_t)l_b));"
        ),
        "{code}"
    );
}

#[test]
fn a_store_into_a_sized_cell_checks_the_range() {
    let code =
        emitted("main {\n    let u8 a ← 200\n    mut u8 m ← a\n    m ← a + 100\n    print m\n}");
    assert!(code.contains("faber_store_u8("), "{code}");
    assert!(code.contains("\"assignment to `m`\""), "{code}");
}

#[test]
fn u64_is_read_as_an_exact_value() {
    let code = emitted("main {\n    let u64 big ← 18446744073709551615\n    print big\n}");
    assert!(
        code.contains("faber_exact_text") || code.contains("faber_i128_text"),
        "{code}"
    );
    assert!(code.contains("((faber_i128)l_big)"), "{code}");
}

#[test]
fn a_sized_parameter_and_result_are_checked_and_widened() {
    let code = emitted("fn bump(u8 x) → u8 {\n    return x + 250\n}\nmain {\n    print bump(3)\n}");
    assert!(
        code.contains("static uint8_t f_bump(uint8_t p_x)"),
        "{code}"
    );
    assert!(
        code.contains("faber_store_u8(") && code.contains("\"return\""),
        "{code}"
    );
    assert!(code.contains("((int64_t)f_bump("), "{code}");
}

#[test]
fn an_ascription_checks_against_its_type() {
    let code = emitted("main {\n    let i32 narrow ← 7 ∷ i32\n    print narrow\n}");
    assert!(
        code.contains("const int32_t l_narrow = INT64_C(7);"),
        "{code}"
    );
}
