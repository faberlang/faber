//! decimal-widths D1: d32/d64 marker law, decimal widening lattice, and the
//! exact-text scaled parser that backs the literal carrier.

use crate::numeric_width::decimal_text_to_scaled;
use crate::{NumericWidth, Primitive, WidthError};

#[test]
fn d_markers_parse_on_numerus_only() {
    assert_eq!(
        NumericWidth::from_marker("d32", Primitive::Numerus),
        Ok(NumericWidth::D32)
    );
    assert_eq!(
        NumericWidth::from_marker("d64", Primitive::Numerus),
        Ok(NumericWidth::D64)
    );
    // d-markers reject on fractus (the mirror of FloatOnNumerus).
    assert_eq!(
        NumericWidth::from_marker("d32", Primitive::Fractus),
        Err(WidthError::UnrecognizedMarker)
    );
    assert_eq!(
        NumericWidth::from_marker("d64", Primitive::Fractus),
        Err(WidthError::UnrecognizedMarker)
    );
    // f-markers still reject on numerus (surgical relaxation only).
    assert_eq!(
        NumericWidth::from_marker("f32", Primitive::Numerus),
        Err(WidthError::FloatOnNumerus)
    );
}

#[test]
fn decimal_lattice_grows_d32_to_d64_only() {
    assert!(NumericWidth::is_widening_assignable(NumericWidth::D32, NumericWidth::D64));
    assert!(NumericWidth::is_widening_assignable(NumericWidth::D32, NumericWidth::D32));
    assert!(!NumericWidth::is_widening_assignable(NumericWidth::D64, NumericWidth::D32));
    // decimals never cross into the integer or fractus lattices
    for other in [
        NumericWidth::I64,
        NumericWidth::U64,
        NumericWidth::F32,
        NumericWidth::F64,
    ] {
        assert!(!NumericWidth::is_widening_assignable(NumericWidth::D32, other));
        assert!(!NumericWidth::is_widening_assignable(other, NumericWidth::D32));
        assert!(!NumericWidth::is_widening_assignable(NumericWidth::D64, other));
        assert!(!NumericWidth::is_widening_assignable(other, NumericWidth::D64));
    }
    assert!(!NumericWidth::D32.is_float());
    assert!(!NumericWidth::D32.is_signed_int());
    assert!(!NumericWidth::D64.is_unsigned_int());
    assert_eq!(NumericWidth::D32.decimal_scale(), Some(7));
    assert_eq!(NumericWidth::D64.decimal_scale(), Some(9));
    assert_eq!(NumericWidth::F64.decimal_scale(), None);
}

#[test]
fn decimal_text_to_scaled_is_exact() {
    // 4.2 arrives as 42×10⁻¹ — not the f64 nearest (4.2000000000000001776…).
    assert_eq!(decimal_text_to_scaled("4.2"), Some((42, -1)));
    assert_eq!(decimal_text_to_scaled("42"), Some((42, 0)));
    assert_eq!(decimal_text_to_scaled("0.001"), Some((1, -3)));
    assert_eq!(decimal_text_to_scaled("1e-3"), Some((1, -3)));
    assert_eq!(decimal_text_to_scaled("1E+2"), Some((1, 2)));
    assert_eq!(decimal_text_to_scaled("1.5e2"), Some((15, 1)));
    assert_eq!(decimal_text_to_scaled(""), None);
    assert_eq!(decimal_text_to_scaled("abc"), None);
    assert_eq!(decimal_text_to_scaled("1e"), None);
}

// --- D2 wildcard audit gates (index scalars / tensor media) ---

#[test]
fn decimal_widths_reject_as_index_scalars() {
    use crate::{is_decimal_index_scalar, is_integer_index_scalar, TypeTable};
    let mut types = TypeTable::new();
    let d32 = types.sized_numeric(Primitive::Numerus, NumericWidth::D32);
    let d64 = types.sized_numeric(Primitive::Numerus, NumericWidth::D64);
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    assert!(!is_integer_index_scalar(&types, d32));
    assert!(!is_integer_index_scalar(&types, d64));
    assert!(is_integer_index_scalar(&types, i32));
    assert!(is_decimal_index_scalar(&types, d32));
    assert!(!is_decimal_index_scalar(&types, i32));
}

#[test]
fn tensor_media_width_rejects_decimal_elements() {
    use crate::{tensor_media_width, TypeTable};
    let mut types = TypeTable::new();
    let d32 = types.sized_numeric(Primitive::Numerus, NumericWidth::D32);
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    assert_eq!(tensor_media_width(&types, d32), None);
    assert_eq!(
        tensor_media_width(&types, i32),
        Some(NumericWidth::F64)
    );
    assert_eq!(
        tensor_media_width(&types, f32),
        Some(NumericWidth::F32)
    );
}

// --- D3 conversio matrix (explicit `↦` arms) ---

#[test]
fn decimal_conversio_admits_widening_arm_only() {
    // The conversio matrix within the decimal family is exactly the widening
    // arm: d32→d64 (identity included). The reverse d64→d32 is a narrowing
    // with no `↦` row — decimals never narrow through conversio, unlike
    // integer widths where explicit narrowing is an established law.
    assert!(!NumericWidth::is_decimal_narrowing_conversio(
        NumericWidth::D32,
        NumericWidth::D64
    ));
    assert!(!NumericWidth::is_decimal_narrowing_conversio(
        NumericWidth::D32,
        NumericWidth::D32
    ));
    assert!(NumericWidth::is_decimal_narrowing_conversio(
        NumericWidth::D64,
        NumericWidth::D32
    ));
    assert!(!NumericWidth::is_decimal_narrowing_conversio(
        NumericWidth::D64,
        NumericWidth::D64
    ));
    // Non-decimal pairs never trip the decimal narrowing arm.
    assert!(!NumericWidth::is_decimal_narrowing_conversio(
        NumericWidth::I64,
        NumericWidth::I32
    ));
    assert!(!NumericWidth::is_decimal_narrowing_conversio(
        NumericWidth::F64,
        NumericWidth::F32
    ));
}
