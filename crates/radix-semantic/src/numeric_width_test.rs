//! Tests for numeric width parsing, sugar-marker mapping, and width errors.
//!
//! These tests prove that width markers (f32, i64, etc.) map to the correct
//! NumericWidth variant and family (Fractus/Numerus), and that invalid
//! or mismatched widths produce WidthError diagnostics.

use super::numeric_width::{
    NumericWidth, WidthError, is_numeric_width_type_sugar, lista_sugar_width_marker,
    sparsa_sugar_width_marker, standalone_width_type, tensor_sugar_width_marker,
};
use crate::{Primitive, TypeTable};

#[test]
fn standalone_width_type_maps_markers_to_families() {
    assert_eq!(
        standalone_width_type("f32"),
        Some((Primitive::Fractus, NumericWidth::F32))
    );
    assert_eq!(
        standalone_width_type("i64"),
        Some((Primitive::Numerus, NumericWidth::I64))
    );
    assert!(standalone_width_type("textus").is_none());
}

#[test]
fn tensor_sugar_width_marker_strips_t_prefix() {
    assert_eq!(tensor_sugar_width_marker("tf32"), Some("f32"));
    assert_eq!(tensor_sugar_width_marker("tu8"), Some("u8"));
    assert!(tensor_sugar_width_marker("table").is_none());
    assert!(tensor_sugar_width_marker("tensor").is_none());
}

#[test]
fn sparsa_sugar_width_marker_strips_s_prefix() {
    assert_eq!(sparsa_sugar_width_marker("sf32"), Some("f32"));
    assert_eq!(sparsa_sugar_width_marker("su32"), Some("u32"));
    assert_eq!(sparsa_sugar_width_marker("si64"), Some("i64"));
    assert!(sparsa_sugar_width_marker("sparsa").is_none());
    assert!(sparsa_sugar_width_marker("string").is_none());
}

#[test]
fn is_numeric_width_type_sugar_recognizes_bare_and_prefixed_markers() {
    assert!(is_numeric_width_type_sugar("f32"));
    assert!(is_numeric_width_type_sugar("tf32"));
    assert!(is_numeric_width_type_sugar("sf32"));
    assert!(is_numeric_width_type_sugar("lu32"));
    assert!(!is_numeric_width_type_sugar("textus"));
    assert!(!is_numeric_width_type_sugar("lista"));
}

#[test]
fn lista_sugar_width_marker_strips_l_prefix() {
    assert_eq!(lista_sugar_width_marker("lf32"), Some("f32"));
    assert_eq!(lista_sugar_width_marker("lu32"), Some("u32"));
    assert_eq!(lista_sugar_width_marker("li64"), Some("i64"));
    assert!(lista_sugar_width_marker("lista").is_none());
    assert!(lista_sugar_width_marker("link").is_none());
}

#[test]
fn parses_signed_and_unsigned_numerus_widths() {
    assert_eq!(
        NumericWidth::from_marker("i32", Primitive::Numerus).ok(),
        Some(NumericWidth::I32)
    );
    assert_eq!(
        NumericWidth::from_marker("u64", Primitive::Numerus).ok(),
        Some(NumericWidth::U64)
    );
}

#[test]
fn rejects_float_markers_on_numerus() {
    assert_eq!(
        NumericWidth::from_marker("f32", Primitive::Numerus),
        Err(WidthError::FloatOnNumerus)
    );
}

#[test]
fn widening_lattice_allows_i32_to_i64() {
    assert!(NumericWidth::is_widening_assignable(
        NumericWidth::I32,
        NumericWidth::I64
    ));
    assert!(!NumericWidth::is_widening_assignable(
        NumericWidth::U8,
        NumericWidth::I8
    ));
}

#[test]
fn bare_numerus_equals_i64_sized_form() {
    let mut types = TypeTable::new();
    let bare = types.primitive(Primitive::Numerus);
    let sized = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    assert!(types.equals(bare, sized));
}

#[test]
fn tensor_element_rejects_textus() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    assert!(!super::is_tensor_element_type(&types, textus));
    let fractus = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    assert!(super::is_tensor_element_type(&types, fractus));
}

#[test]
fn tensor_element_conversio_allows_same_family_widen() {
    let mut types = TypeTable::new();
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let f64 = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    assert!(super::tensor_element_conversio_allowed(&types, i32, i64));
    assert!(super::tensor_element_conversio_allowed(&types, f32, f64));
}

#[test]
fn tensor_element_conversio_allows_same_family_narrow() {
    let mut types = TypeTable::new();
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let f64 = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    assert!(super::tensor_element_conversio_allowed(&types, i64, i32));
    assert!(super::tensor_element_conversio_allowed(&types, f64, f32));
}

#[test]
fn tensor_element_conversio_allows_cross_primitive_fractus_to_numerus() {
    let mut types = TypeTable::new();
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let f64 = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    assert!(super::tensor_element_conversio_allowed(&types, f64, i32));
}

#[test]
fn standalone_width_type_empty_string_is_none() {
    assert!(standalone_width_type("").is_none());
}

#[test]
fn is_numeric_width_type_sugar_rejects_empty_string() {
    assert!(!is_numeric_width_type_sugar(""));
}

#[test]
fn tensor_element_conversio_rejects_cross_family_sized_pair() {
    let mut types = TypeTable::new();
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let u8 = types.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    assert!(!super::tensor_element_conversio_allowed(&types, i64, u8));
    assert!(!super::tensor_element_conversio_allowed(&types, i32, f32));
}

#[test]
fn bits_markers_resolve_as_bare_markers_and_stay_apart_from_numbers() {
    use crate::test_support::{analyze_ok, analyze_source};
    for marker in ["b8", "b16", "b32", "b64"] {
        assert!(
            is_numeric_width_type_sugar(marker),
            "{marker} is bare marker sugar"
        );
        analyze_ok(&format!(
            "functio f({marker} x) → {marker} {{\n    redde x\n}}\n"
        ));
    }
    // Bits are not numbers and widths do not mix: no implicit edge either way.
    for (param, ret) in [("b8", "u8"), ("u8", "b8"), ("b8", "b16"), ("b64", "i64")] {
        let result = analyze_source(&format!(
            "functio f({param} x) → {ret} {{\n    redde x\n}}\n"
        ));
        assert!(
            !result.is_ok(),
            "{param} must not convert to {ret} implicitly"
        );
    }
    // Prefix sugar families do not extend to bits.
    assert!(!is_numeric_width_type_sugar("tb8"));
    assert!(standalone_width_type("b8").is_none());
}
