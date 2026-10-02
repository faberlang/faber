use super::f16_bits_to_f32;

/// Documented decode patterns: `(bits, decoded f32 bits, label)`. `1.5` is the
/// mantissa row: `1.0` and the subnormal/Inf rows alone leave a wrong mantissa
/// divisor (`1024.0`) decoding identically.
const DOCUMENTED_PATTERNS: &[(u16, u32, &str)] = &[
    (0x3c00, 0x3f80_0000, "1.0"),
    (0x3e00, 0x3fc0_0000, "1.5"),
    (0x0001, 0x3380_0000, "subnormal 2^-24"),
    (0x7c00, 0x7f80_0000, "+Inf"),
    (0xfc00, 0xff80_0000, "-Inf"),
];

#[test]
fn f16_decode_matches_documented_patterns_bit_for_bit() {
    for (bits, expected, label) in DOCUMENTED_PATTERNS {
        assert_eq!(
            f16_bits_to_f32(*bits).to_bits(),
            *expected,
            "{label} ({bits:#06x})"
        );
    }
}

#[test]
fn f16_decode_keeps_signed_zero_and_nan() {
    assert_eq!(f16_bits_to_f32(0x0000).to_bits(), 0.0_f32.to_bits());
    assert_eq!(f16_bits_to_f32(0x8000).to_bits(), (-0.0_f32).to_bits());
    assert!(f16_bits_to_f32(0x7e00).is_nan());
    assert!(f16_bits_to_f32(0xfe00).is_nan());
}

#[test]
fn f16_decode_negates_through_the_sign_bit() {
    assert_eq!(f16_bits_to_f32(0xbc00).to_bits(), (-1.0_f32).to_bits());
    assert_eq!(
        f16_bits_to_f32(0x8001).to_bits(),
        (-(2.0_f32.powi(-24))).to_bits()
    );
}
