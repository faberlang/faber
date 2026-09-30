//! inf track U2: the `inf` width marker. `inf` is a locale-invariant
//! `numerus` marker for the unbounded integer (ruling 32/33): a width in the
//! trapping integer family with no range, the implicit lossless target of
//! every bounded integer width (ruling 35), no prefixed sugar, never a
//! tensor-family element.

use crate::numeric_width::{
    is_integer_index_scalar, is_numeric_width_type_sugar, is_standalone_width_name,
    is_tensor_numeric_element, is_unbounded_integer_type, lista_sugar_width_marker,
    matrix_sugar_width_marker, numerus_integer_width, sparsa_sugar_width_marker,
    standalone_width_type, tensor_sugar_width_marker, vector_sugar_width_marker,
};
use crate::{NumericWidth, Primitive, Type, TypeTable, WidthError};

const BOUNDED_INTEGERS: [NumericWidth; 8] = [
    NumericWidth::I8,
    NumericWidth::I16,
    NumericWidth::I32,
    NumericWidth::I64,
    NumericWidth::U8,
    NumericWidth::U16,
    NumericWidth::U32,
    NumericWidth::U64,
];

#[test]
fn inf_parses_as_a_numerus_marker_and_rejects_on_fractus() {
    assert_eq!(
        NumericWidth::from_marker("inf", Primitive::Numerus),
        Ok(NumericWidth::Inf)
    );
    assert_eq!(
        NumericWidth::from_marker("inf", Primitive::Fractus),
        Err(WidthError::IntOnFractus)
    );
    // The marker is locale-invariant ASCII: no other spelling parses.
    for other in ["∞", "infinity", "Inf", "INF", "unbounded", "linf"] {
        assert_eq!(
            NumericWidth::from_marker(other, Primitive::Numerus),
            Err(WidthError::UnrecognizedMarker),
            "{other}"
        );
    }
}

#[test]
fn inf_is_an_integer_width_with_no_range() {
    let inf = NumericWidth::Inf;
    assert!(inf.is_unbounded());
    assert_eq!(inf.integer_range(), None);
    assert_eq!(inf.unsigned_max(), None);
    assert!(!inf.is_signed_int());
    assert!(!inf.is_unsigned_int());
    assert!(!inf.is_float());
    assert!(!inf.is_decimal());
    assert_eq!(inf.rust_type(), "inf");
    assert_eq!(inf.faber_marker(), "inf");
    assert_eq!(inf.float_truncation_bounds(), None);
    for width in BOUNDED_INTEGERS {
        assert!(!width.is_unbounded(), "{width:?}");
    }
}

#[test]
fn every_bounded_integer_widens_into_inf_and_inf_widens_into_nothing() {
    for width in BOUNDED_INTEGERS {
        assert!(
            NumericWidth::is_widening_assignable(width, NumericWidth::Inf),
            "{width:?} -> inf"
        );
        assert!(
            !NumericWidth::is_widening_assignable(NumericWidth::Inf, width),
            "inf -> {width:?}"
        );
        // `u64 -> inf` is the lattice edge D11.6 asked for, and an ordinary
        // (not narrowing) widening for `↦`.
        assert!(NumericWidth::is_conversio_widening(
            width,
            NumericWidth::Inf
        ));
        assert!(!NumericWidth::is_explicit_narrowing_conversio(
            width,
            NumericWidth::Inf
        ));
    }
    assert!(NumericWidth::is_widening_assignable(
        NumericWidth::Inf,
        NumericWidth::Inf
    ));
    for other in [
        NumericWidth::D64,
        NumericWidth::F16,
        NumericWidth::BF16,
        NumericWidth::F32,
        NumericWidth::F64,
    ] {
        assert!(!NumericWidth::is_widening_assignable(
            other,
            NumericWidth::Inf
        ));
        assert!(!NumericWidth::is_widening_assignable(
            NumericWidth::Inf,
            other
        ));
    }
}

#[test]
fn inf_is_a_standalone_marker_but_never_prefixed_sugar() {
    assert_eq!(
        standalone_width_type("inf"),
        Some((Primitive::Numerus, NumericWidth::Inf))
    );
    assert!(is_standalone_width_name("inf"));
    for name in ["linf", "tinf", "vinf", "minf", "sinf"] {
        assert!(!is_numeric_width_type_sugar(name), "{name}");
    }
    assert_eq!(lista_sugar_width_marker("linf"), None);
    assert_eq!(tensor_sugar_width_marker("tinf"), None);
    assert_eq!(vector_sugar_width_marker("vinf"), None);
    assert_eq!(matrix_sugar_width_marker("minf"), None);
    assert_eq!(sparsa_sugar_width_marker("sinf"), None);
    // The sibling sugars are untouched.
    assert_eq!(lista_sugar_width_marker("lu8"), Some("u8"));
    assert_eq!(tensor_sugar_width_marker("tf32"), Some("f32"));
}

#[test]
fn inf_is_an_integer_width_that_is_never_a_tensor_element_or_index() {
    let mut types = TypeTable::new();
    let inf = types.sized_numeric(Primitive::Numerus, NumericWidth::Inf);
    assert_eq!(numerus_integer_width(&types, inf), Some(NumericWidth::Inf));
    assert!(is_unbounded_integer_type(&types, inf));
    assert!(!is_tensor_numeric_element(&types, inf));
    assert!(!is_integer_index_scalar(&types, inf));

    let u64_ty = types.sized_numeric(Primitive::Numerus, NumericWidth::U64);
    assert!(!is_unbounded_integer_type(&types, u64_ty));
    let bare = types.primitive(Primitive::Numerus);
    assert!(!is_unbounded_integer_type(&types, bare));
    assert!(is_tensor_numeric_element(&types, bare));

    // Aliases are seen through.
    let alias = types.intern(Type::Alias(crate::DefId(9_001), inf));
    assert!(is_unbounded_integer_type(&types, alias));
    assert!(!is_tensor_numeric_element(&types, alias));
}
