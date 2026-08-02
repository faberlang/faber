//! [`NumericWidth`] marker parsing (standalone and sugar-prefixed), widening
//! lattice, tensor element policy, and bare-numerus identity with i64.

use crate::{
    is_numeric_width_type_sugar, lista_sugar_width_marker, sparsa_sugar_width_marker,
    standalone_width_type, tensor_sugar_width_marker, NumericWidth, WidthError,
};
use crate::{Primitive, TypeTable};

#[test]
fn standalone_width_type_maps_integer_markers() {
    for (marker, width) in [
        ("i8", NumericWidth::I8),
        ("i16", NumericWidth::I16),
        ("i32", NumericWidth::I32),
        ("i64", NumericWidth::I64),
        ("u8", NumericWidth::U8),
        ("u16", NumericWidth::U16),
        ("u32", NumericWidth::U32),
        ("u64", NumericWidth::U64),
    ] {
        assert_eq!(
            standalone_width_type(marker),
            Some((Primitive::Numerus, width)),
            "marker {marker:?} should map to numerus<{width:?}>"
        );
    }
}

#[test]
fn standalone_width_type_maps_float_markers() {
    for (marker, width) in [
        ("f16", NumericWidth::F16),
        ("f32", NumericWidth::F32),
        ("f64", NumericWidth::F64),
    ] {
        assert_eq!(
            standalone_width_type(marker),
            Some((Primitive::Fractus, width)),
            "marker {marker:?} should map to fractus<{width:?}>"
        );
    }
}

#[test]
fn standalone_width_type_rejects_non_markers() {
    assert!(standalone_width_type("textus").is_none());
    assert!(standalone_width_type("bivalens").is_none());
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
    for (marker, expected) in [
        ("i8", NumericWidth::I8),
        ("i16", NumericWidth::I16),
        ("i32", NumericWidth::I32),
        ("i64", NumericWidth::I64),
        ("u8", NumericWidth::U8),
        ("u16", NumericWidth::U16),
        ("u32", NumericWidth::U32),
        ("u64", NumericWidth::U64),
    ] {
        assert_eq!(
            NumericWidth::from_marker(marker, Primitive::Numerus).ok(),
            Some(expected),
            "marker {marker:?} should parse to {expected:?}"
        );
    }
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
    assert!(NumericWidth::widening_assignable(
        NumericWidth::I32,
        NumericWidth::I64
    ));
    assert!(!NumericWidth::widening_assignable(
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
fn tensor_element_conversio_allows_sized_widen_and_narrow() {
    let mut types = TypeTable::new();
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let f64 = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    assert!(super::tensor_element_conversio_allowed(&types, i32, i64));
    assert!(super::tensor_element_conversio_allowed(&types, i64, i32));
    assert!(super::tensor_element_conversio_allowed(&types, f32, f64));
    assert!(super::tensor_element_conversio_allowed(&types, f64, f32));
    assert!(super::tensor_element_conversio_allowed(&types, f64, i32));
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
fn default_for_numerus_returns_i64() {
    assert_eq!(
        NumericWidth::default_for(Primitive::Numerus),
        Some(NumericWidth::I64)
    );
}

#[test]
fn default_for_fractus_returns_f64() {
    assert_eq!(
        NumericWidth::default_for(Primitive::Fractus),
        Some(NumericWidth::F64)
    );
}

#[test]
fn default_for_non_numeric_returns_none() {
    for prim in [
        Primitive::Ascii,
        Primitive::Textus,
        Primitive::Bivalens,
        Primitive::Nihil,
        Primitive::Vacuum,
        Primitive::Numquam,
        Primitive::Ignotum,
        Primitive::Octeti,
    ] {
        assert_eq!(
            NumericWidth::default_for(prim),
            None,
            "{prim:?} should not have a numeric width"
        );
    }
}

#[test]
fn is_default_for_returns_false_for_non_numeric() {
    assert!(!NumericWidth::I64.is_default_for(Primitive::Textus));
    assert!(!NumericWidth::F64.is_default_for(Primitive::Bivalens));
}
