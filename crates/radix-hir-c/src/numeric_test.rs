//! Pinned-output tests for the integer methods and the bitwise operators.

use crate::test_support::emitted;

#[test]
fn bitwise_operators_use_the_exact_helpers() {
    let code = emitted(
        "main {\n    var int a ← 12\n    var int b ← 10\n    print a ∧ b\n    print a ∨ b\n    print a ⊻ b\n    print ¬ a\n}",
    );
    for call in [
        "faber_bit_and(l_a, l_b)",
        "faber_bit_or(l_a, l_b)",
        "faber_bit_xor(l_a, l_b)",
        "faber_bit_not(l_a)",
    ] {
        assert!(code.contains(call), "{call:?} missing in {code}");
    }
}

#[test]
fn shifts_use_the_exact_helpers() {
    let code = emitted("main {\n    var int a ← 12\n    var int n ← 2\n    print a ⇐ n\n    print a ⇒ n\n}");
    for call in ["faber_shl(l_a, l_n)", "faber_shr(l_a, l_n)"] {
        assert!(code.contains(call), "{call:?} missing in {code}");
    }
}

#[test]
fn bitwise_constant_folds_with_the_cap() {
    let code = emitted("main {\n    print 0b1010 ∧ 0b1100\n    print 1 ⇐ 4\n    print 0 - 1 ⇒ 200\n}");
    assert!(code.contains("INT64_C(8)"), "{code}");
    assert!(code.contains("INT64_C(16)"), "{code}");
    // A right shift past the width fills with the sign, not by truncating.
    assert!(code.contains("-INT64_C(1)"), "{code}");
}

#[test]
fn integer_methods_use_the_exact_helpers() {
    let code = emitted(
        "main {\n    var int a ← 0 - 7\n    print a.abs()\n    print a.sign()\n    print a.minimum(3)\n    print a.maximum(3)\n    print a.power(2)\n}",
    );
    for call in [
        "faber_abs(l_a)",
        "faber_sign(l_a)",
        "faber_min_i128(l_a, INT64_C(3))",
        "faber_max_i128(l_a, INT64_C(3))",
        "faber_pow(l_a, INT64_C(2))",
    ] {
        assert!(code.contains(call), "{call:?} missing in {code}");
    }
}

#[test]
fn float_power_uses_libm_pow() {
    let code = emitted("main {\n    var float a ← 2.0\n    print a.power(3.0)\n}");
    assert!(code.contains("faber_pow_f64(l_a, 3.0)"), "{code}");
}
