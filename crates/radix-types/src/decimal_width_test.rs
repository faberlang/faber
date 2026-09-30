//! decimal-widths D1: d64 marker law (`d32` removed, D11.7), the decimal
//! lattice, and the exact-text scaled parser that backs the literal carrier.

use crate::numeric_width::decimal_text_to_scaled;
use crate::{NumericWidth, Primitive, WidthError};

#[test]
fn d_markers_parse_on_numerus_only() {
    // `d32` was removed (D11.7): a dedicated error, never a width.
    assert_eq!(
        NumericWidth::from_marker("d32", Primitive::Numerus),
        Err(WidthError::RemovedDecimal)
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
fn decimal_lattice_has_one_width() {
    assert!(NumericWidth::is_widening_assignable(
        NumericWidth::D64,
        NumericWidth::D64
    ));
    // decimals never cross into the integer or fractus lattices
    for other in [
        NumericWidth::I64,
        NumericWidth::U64,
        NumericWidth::F32,
        NumericWidth::F64,
    ] {
        assert!(!NumericWidth::is_widening_assignable(
            NumericWidth::D64,
            other
        ));
        assert!(!NumericWidth::is_widening_assignable(
            other,
            NumericWidth::D64
        ));
    }
    assert!(!NumericWidth::D64.is_float());
    assert!(!NumericWidth::D64.is_signed_int());
    assert!(!NumericWidth::D64.is_unsigned_int());
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
    use crate::{TypeTable, is_decimal_index_scalar, is_integer_index_scalar};
    let mut types = TypeTable::new();
    let d64 = types.sized_numeric(Primitive::Numerus, NumericWidth::D64);
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    assert!(!is_integer_index_scalar(&types, d64));
    assert!(is_integer_index_scalar(&types, i32));
    assert!(is_decimal_index_scalar(&types, d64));
    assert!(!is_decimal_index_scalar(&types, i32));
}

#[test]
fn tensor_media_width_rejects_decimal_elements() {
    use crate::{TypeTable, tensor_media_width};
    let mut types = TypeTable::new();
    let d64 = types.sized_numeric(Primitive::Numerus, NumericWidth::D64);
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    assert_eq!(tensor_media_width(&types, d64), None);
    assert_eq!(tensor_media_width(&types, i32), Some(NumericWidth::F64));
    assert_eq!(tensor_media_width(&types, f32), Some(NumericWidth::F32));
}
