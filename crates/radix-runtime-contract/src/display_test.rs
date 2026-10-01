use super::display_fractus;

#[test]
fn an_integral_f32_prints_shortest_digits_plus_point_zero() {
    // The MIR runner's `display_float_at_width` is the oracle: shortest
    // round-trip digits plus `.0`, never the exact binary expansion.
    assert_eq!(
        display_fractus(f32::MAX),
        "340282350000000000000000000000000000000.0"
    );
    assert_eq!(display_fractus(1.0e20_f32), "100000000000000000000.0");
    assert_eq!(display_fractus(-0.0_f32), "-0.0");
}

#[test]
fn an_integral_f64_prints_its_exact_expansion() {
    assert_eq!(display_fractus(1.0e21_f64), "1000000000000000000000.0");
    assert_eq!(
        display_fractus(18_446_744_073_709_551_616.0_f64),
        "18446744073709551616.0"
    );
}

#[test]
fn a_fractional_value_prints_its_native_text() {
    assert_eq!(display_fractus(3.25_f32), "3.25");
    assert_eq!(display_fractus(0.000_000_1_f64), "0.0000001");
}
