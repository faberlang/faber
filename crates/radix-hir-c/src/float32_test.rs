//! `f32` (binary32, goal unit C7): the width rides the class, a `float` cell
//! rounds per operation, and the display, methods and list family are the f32
//! ones.

use crate::test_support::emitted;

#[test]
fn a_binary32_literal_is_a_float_literal() {
    let code = emitted("main {\n    let f32 a ← 0.1\n    print a\n}");
    assert!(code.contains("float l_a = 0.1f"), "{code}");
    assert!(code.contains("faber_f32_text(l_a"), "{code}");
}

#[test]
fn binary32_arithmetic_is_plain_c_float() {
    let code = emitted("main {\n    let f32 a ← 0.1\n    let f32 b ← 0.2\n    print a + b\n}");
    assert!(code.contains("(l_a + l_b)"), "{code}");
}

#[test]
fn binary32_mod_uses_the_f32_helper() {
    let code = emitted("main {\n    let f32 a ← 0.1\n    let f32 b ← 0.2\n    print a % b\n}");
    assert!(code.contains("faber_fmod_f32(l_a, l_b)"), "{code}");
}

#[test]
fn binary32_methods_use_the_f32_helpers() {
    let code = emitted(
        "main {\n    let f32 a ← 0.1\n    let f32 b ← 0.2\n    print a.abs()\n    print a.minimum(b)\n}",
    );
    assert!(code.contains("faber_abs_f32(l_a)"), "{code}");
    assert!(code.contains("faber_minimum_f32(l_a, l_b)"), "{code}");
}

#[test]
fn binary32_conversions_round_at_the_declared_width() {
    let code = emitted(
        "main {\n    print 1 ↦ f32\n    print \"2.5\" ↦ f32\n    print 1.5 ↦ f32\n}",
    );
    assert!(code.contains("((float)INT64_C(1))"), "{code}");
    assert!(code.contains("(float)faber_v"), "{code}");
    // A binary64 to binary32 conversion is the C cast.
    assert!(code.contains("((float)1.5)"), "{code}");
    assert!(code.contains("faber_try_text_float"), "{code}");
}

#[test]
fn a_binary32_list_is_the_f32_family() {
    let code = emitted("main {\n    let list<f32> xs ← [0.5, 1.5]\n    print xs\n}");
    assert!(code.contains("faber_list_f32_of"), "{code}");
    assert!(code.contains("faber_list_f32_show"), "{code}");
}
