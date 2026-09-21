//! [`NumericWidth`] marker parsing (standalone and sugar-prefixed), widening
//! lattice, tensor element policy, and bare-numerus identity with i64.

use crate::{
    NumericWidth, WidthError, is_ascii_format_hint, is_conversio_radix_hint,
    is_numeric_width_type_sugar, lista_sugar_width_marker, matrix_sugar_width_marker,
    sparsa_sugar_width_marker, standalone_width_type, tensor_sugar_width_marker,
    vector_sugar_width_marker,
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
        ("bf16", NumericWidth::BF16),
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
    assert!(is_numeric_width_type_sugar("bf16"));
    assert!(is_numeric_width_type_sugar("tbf16"));
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
    assert_eq!(
        NumericWidth::from_marker("bf16", Primitive::Numerus),
        Err(WidthError::FloatOnNumerus)
    );
}

#[test]
fn from_marker_admits_bf16_on_fractus() {
    assert_eq!(
        NumericWidth::from_marker("bf16", Primitive::Fractus),
        Ok(NumericWidth::BF16)
    );
    assert!(NumericWidth::BF16.is_float());
}

#[test]
fn bf16_sugar_compositions_resolve() {
    assert_eq!(
        tensor_sugar_width_marker("tbf16"),
        Some("bf16"),
        "tensor sugar tbf16 resolves to bf16"
    );
    assert_eq!(
        lista_sugar_width_marker("lbf16"),
        Some("bf16"),
        "lista sugar lbf16 resolves to bf16"
    );
    assert_eq!(
        vector_sugar_width_marker("vbf16"),
        Some("bf16"),
        "vector sugar vbf16 resolves to bf16"
    );
    assert_eq!(
        matrix_sugar_width_marker("mbf16"),
        Some("bf16"),
        "matrix sugar mbf16 resolves to bf16"
    );
    assert_eq!(
        sparsa_sugar_width_marker("sbf16"),
        Some("bf16"),
        "sparsa sugar sbf16 resolves to bf16"
    );
}

#[test]
fn prefix_closure_law_bf16_never_decomposes() {
    // `bf16` is a whole width marker, admitted by name in `from_marker`.
    assert_eq!(
        standalone_width_type("bf16"),
        Some((Primitive::Fractus, NumericWidth::BF16))
    );
    // The sugar grammar strips a prefix char, so `b` + `f16` would shadow the
    // whole marker if `b` ever joined the prefix set. It must never do so:
    // the five sugar prefixes are t/l/v/m/s, and none is `b`.
    for prefix in ['t', 'l', 'v', 'm', 's'] {
        assert_ne!(prefix, 'b', "prefix {prefix:?} must not be 'b'");
        assert_eq!(
            super::width_prefixed_sugar_marker("bf16", prefix),
            None,
            "sugar prefix {prefix:?} must not decompose bf16"
        );
    }
    // No sugar entry point decomposes `bf16`.
    assert_eq!(tensor_sugar_width_marker("bf16"), None);
    assert_eq!(lista_sugar_width_marker("bf16"), None);
    assert_eq!(vector_sugar_width_marker("bf16"), None);
    assert_eq!(matrix_sugar_width_marker("bf16"), None);
    assert_eq!(sparsa_sugar_width_marker("bf16"), None);
    // The hazard the law prevents: `b` + `f16` *is* a valid decomposition, so
    // the `b` prefix stays excluded from the sugar prefix set forever.
    assert_eq!(super::width_prefixed_sugar_marker("bf16", 'b'), Some("f16"));
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

#[test]
fn ascii_format_hints_are_hex_bin_oct_only() {
    for marker in ["Hex", "Bin", "Oct"] {
        assert!(
            is_ascii_format_hint(marker),
            "{marker} is an ascii pack hint"
        );
        assert!(
            is_conversio_radix_hint(marker),
            "{marker} stays in the shared conversio hint set"
        );
    }
    for marker in ["Be", "Le", "Bits"] {
        assert!(
            !is_ascii_format_hint(marker),
            "{marker} is endian/bitcast, not an ascii digit base"
        );
        assert!(is_conversio_radix_hint(marker));
    }
}

// S03-1: lowering failures cross the producer boundary as typed issues, not
// prose that downstream tables re-parse into a diagnostic identity.
use crate::numeric_width::{
    WidthLowerIssue, lower_modular_word, lower_sized_primitive, lower_standalone_width_type,
};
use radix_lexer::{Interner, Span};
use radix_syntax::{GenericArgExpr, Ident, TypeExpr, TypeExprKind, TypeFormLevel};

fn marker_expr(spelling: &str, interner: &mut Interner) -> TypeExpr {
    generic_marker_expr(spelling, Vec::new(), interner)
}

fn generic_marker_expr(
    spelling: &str,
    args: Vec<GenericArgExpr>,
    interner: &mut Interner,
) -> TypeExpr {
    TypeExpr {
        nullable: false,
        mode: None,
        kind: TypeExprKind::Named(
            Ident {
                name: interner.intern(spelling),
                span: Span { start: 0, end: 0 },
            },
            args,
        ),
        span: Span { start: 0, end: 0 },
        form: TypeFormLevel::LONG,
        iuncta_label: None,
    }
}

#[test]
fn width_lowering_failures_report_typed_issues() {
    let mut interner = Interner::new();
    let mut types = TypeTable::new();

    let mut reported = Vec::new();
    let params = [marker_expr("u8", &mut interner)];
    assert!(
        lower_standalone_width_type("f32", &params, &mut types, &mut |issue| {
            reported.push(issue)
        })
        .is_some()
    );
    assert_eq!(reported, vec![WidthLowerIssue::Params]);
    assert_eq!(reported[0].issue(), "width_marker_params");

    let mut reported = Vec::new();
    assert!(
        lower_standalone_width_type("f32", &[], &mut types, &mut |issue| {
            reported.push(issue)
        })
        .is_some()
    );
    assert!(reported.is_empty());
}

#[test]
fn sized_primitive_lowering_reports_typed_issues() {
    let mut interner = Interner::new();
    let mut types = TypeTable::new();

    let cases: [(Primitive, Vec<TypeExpr>, WidthLowerIssue, &str); 6] = [
        (
            Primitive::Numerus,
            vec![marker_expr("f32", &mut interner)],
            WidthLowerIssue::FloatOnNumerus,
            "float_width_on_numerus",
        ),
        (
            Primitive::Fractus,
            vec![marker_expr("i32", &mut interner)],
            WidthLowerIssue::IntOnFractus,
            "integer_width_on_fractus",
        ),
        (
            Primitive::Numerus,
            vec![marker_expr("u128", &mut interner)],
            WidthLowerIssue::InvalidMarker(Primitive::Numerus),
            "invalid_numerus_width_marker",
        ),
        (
            Primitive::Fractus,
            vec![marker_expr("x9", &mut interner)],
            WidthLowerIssue::InvalidMarker(Primitive::Fractus),
            "invalid_fractus_width_marker",
        ),
        (
            Primitive::Numerus,
            vec![
                marker_expr("i32", &mut interner),
                marker_expr("i64", &mut interner),
            ],
            WidthLowerIssue::TooManyParams(Primitive::Numerus),
            "numerus_width_params",
        ),
        (
            Primitive::Numerus,
            vec![generic_marker_expr(
                "foo",
                vec![GenericArgExpr::Size(radix_syntax::FiguraExpr::Literal(1))],
                &mut interner,
            )],
            WidthLowerIssue::Shape,
            "width_marker_shape",
        ),
    ];

    for (prim, params, expected, issue_name) in cases {
        let mut reported = Vec::new();
        lower_sized_primitive(prim, &params, &interner, &mut types, &mut |issue| {
            reported.push(issue)
        });
        assert_eq!(reported, vec![expected], "issue name {issue_name}");
        assert_eq!(reported[0].issue(), issue_name);
    }
}

#[test]
fn modular_word_lowering_reports_typed_issues() {
    let mut interner = Interner::new();
    let mut types = TypeTable::new();

    let mut reported = Vec::new();
    lower_modular_word(&[], &interner, &mut types, &mut |issue| {
        reported.push(issue)
    });
    assert_eq!(reported, vec![WidthLowerIssue::ModularWordRequired]);
    assert_eq!(reported[0].issue(), "modular_word_width_required");

    let mut reported = Vec::new();
    let params = [marker_expr("i32", &mut interner)];
    lower_modular_word(&params, &interner, &mut types, &mut |issue| {
        reported.push(issue)
    });
    assert_eq!(reported, vec![WidthLowerIssue::ModularWordUnsupported]);
    assert_eq!(reported[0].issue(), "unsupported_modular_word_width");

    let mut reported = Vec::new();
    let params = [marker_expr("f32", &mut interner)];
    lower_modular_word(&params, &interner, &mut types, &mut |issue| {
        reported.push(issue)
    });
    assert_eq!(reported, vec![WidthLowerIssue::ModularWordUnsignedRequired]);
    assert_eq!(reported[0].issue(), "modular_word_unsigned_width_required");

    let mut reported = Vec::new();
    let params = [generic_marker_expr(
        "foo",
        vec![GenericArgExpr::Size(radix_syntax::FiguraExpr::Literal(1))],
        &mut interner,
    )];
    lower_modular_word(&params, &interner, &mut types, &mut |issue| {
        reported.push(issue)
    });
    assert_eq!(reported, vec![WidthLowerIssue::ModularWordShape]);
    assert_eq!(reported[0].issue(), "modular_word_width_shape");
}
