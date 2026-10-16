//! Scalar float math methods (libm at the declared width) and the explicit
//! `.approx(other, tolerance)` compare.

use crate::test_support::emitted;

#[test]
fn float_math_methods_use_libm_at_the_declared_width() {
    let code = emitted(
        "main {\n    let float x ← 2.0\n    print x.sqrt()\n    print x.sin()\n    let f32 y ← 2.0\n    print y.sqrt()\n}",
    );
    assert!(code.contains("sqrt(l_x)"), "{code}");
    assert!(code.contains("sin(l_x)"), "{code}");
    assert!(code.contains("sqrtf(l_y)"), "{code}");
}

#[test]
fn float_approx_is_the_promoted_tolerance_compare() {
    let code = emitted(
        "main {\n    let float a ← 42.145\n    let float b ← 42.0\n    print a.approx(b, 0.2)\n}",
    );
    assert!(
        code.contains("fabs((double)l_a - (double)l_b) <= (double)"),
        "{code}"
    );
}

#[test]
fn fuzzy_equality_promotes_an_integer_against_a_float() {
    let code = emitted(
        "main {\n    let int n ← 9007199254740993\n    let float f ← 9007199254740992.0\n    print n ≈ f\n}",
    );
    assert!(code.contains("faber_approx_f64(((double)"), "{code}");
}
