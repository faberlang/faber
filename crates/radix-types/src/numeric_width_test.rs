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
    for marker in ["Be", "Le"] {
        assert!(
            !is_ascii_format_hint(marker),
            "{marker} is endian, not an ascii digit base"
        );
        assert!(is_conversio_radix_hint(marker));
    }
    // `via Bits` retired: no longer a conversio hint.
    assert!(!is_conversio_radix_hint("Bits"));
    // Lower-case spellings are accepted alongside the title-case surface form.
    assert!(is_ascii_format_hint("hex"));
    assert!(is_conversio_radix_hint("le"));
    assert!(is_conversio_radix_hint("code"));
}

// S03-1: lowering failures cross the producer boundary as typed issues, not
// prose that downstream tables re-parse into a diagnostic identity.
use crate::numeric_width::{
    WidthLowerIssue, lower_modular_word, lower_saturating_word, lower_standalone_width_type,
    lower_trapping_word,
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
            reported.push(issue);
        })
        .is_some()
    );
    assert_eq!(reported, vec![WidthLowerIssue::Params]);
    assert_eq!(reported[0].issue(), "width_marker_params");

    let mut reported = Vec::new();
    assert!(
        lower_standalone_width_type("f32", &[], &mut types, &mut |issue| {
            reported.push(issue);
        })
        .is_some()
    );
    assert_eq!(reported, [] as [WidthLowerIssue; 0]);
}

#[test]
fn modular_word_lowering_reports_typed_issues() {
    let mut interner = Interner::new();
    let mut types = TypeTable::new();

    let mut reported = Vec::new();
    lower_modular_word(&[], &interner, &mut types, &mut |issue| {
        reported.push(issue);
    });
    assert_eq!(reported, vec![WidthLowerIssue::ModularWordRequired]);
    assert_eq!(reported[0].issue(), "modular_word_width_required");

    let mut reported = Vec::new();
    let params = [marker_expr("d64", &mut interner)];
    lower_modular_word(&params, &interner, &mut types, &mut |issue| {
        reported.push(issue);
    });
    assert_eq!(reported, vec![WidthLowerIssue::ModularWordUnsupported]);
    assert_eq!(reported[0].issue(), "unsupported_modular_word_width");

    let mut reported = Vec::new();
    let params = [marker_expr("f32", &mut interner)];
    lower_modular_word(&params, &interner, &mut types, &mut |issue| {
        reported.push(issue);
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
        reported.push(issue);
    });
    assert_eq!(reported, vec![WidthLowerIssue::ModularWordShape]);
    assert_eq!(reported[0].issue(), "modular_word_width_shape");
}

#[test]
fn checked_integer_conversio_covers_every_range_losing_pair() {
    use NumericWidth::{F64, I8, I16, I64, U8, U16, U64};
    assert!(NumericWidth::is_checked_integer_conversio(I64, U8));
    assert!(NumericWidth::is_checked_integer_conversio(I64, I8));
    assert!(NumericWidth::is_checked_integer_conversio(U16, U8));
    assert!(NumericWidth::is_checked_integer_conversio(U16, I8));
    assert!(NumericWidth::is_checked_integer_conversio(I8, U64));
    assert!(NumericWidth::is_checked_integer_conversio(U64, I64));
    assert!(!NumericWidth::is_checked_integer_conversio(U8, I16));
    assert!(!NumericWidth::is_checked_integer_conversio(U8, U8));
    assert!(!NumericWidth::is_checked_integer_conversio(I8, I64));
    assert!(!NumericWidth::is_checked_integer_conversio(I64, F64));
}

#[test]
fn checked_integer_guards_name_only_reachable_bounds() {
    use NumericWidth::{I8, I64, U8, U16, U64};
    assert_eq!(
        NumericWidth::checked_integer_guards(I64, U8),
        (Some(0), Some(255))
    );
    assert_eq!(
        NumericWidth::checked_integer_guards(I64, I8),
        (Some(-128), Some(127))
    );
    assert_eq!(
        NumericWidth::checked_integer_guards(U16, U8),
        (None, Some(255))
    );
    assert_eq!(
        NumericWidth::checked_integer_guards(I8, U64),
        (Some(0), None)
    );
    assert_eq!(
        NumericWidth::checked_integer_guards(U64, I64),
        (None, Some(i128::from(i64::MAX)))
    );
    assert_eq!(NumericWidth::checked_integer_guards(U8, U16), (None, None));
}

#[test]
fn implicit_widening_admits_unsigned_into_strictly_wider_signed() {
    use NumericWidth::{I8, I16, I32, I64, U8, U16, U32, U64};
    for (from, to) in [
        (U8, I16),
        (U8, I32),
        (U8, I64),
        (U16, I32),
        (U16, I64),
        (U32, I64),
        (U8, U64),
        (U32, U64),
        (I8, I64),
    ] {
        assert!(
            NumericWidth::is_widening_assignable(from, to),
            "{from:?} -> {to:?}"
        );
    }
    for (from, to) in [(U8, I8), (U16, I16), (U32, I32), (U64, I64), (U64, I8)] {
        assert!(
            !NumericWidth::is_widening_assignable(from, to),
            "{from:?} -> {to:?}"
        );
    }
}

#[test]
fn implicit_widening_never_crosses_integer_into_float() {
    use NumericWidth::{F32, F64, I8, I16, I32, I64, U8, U16, U32, U64};
    for from in [I8, I16, I32, I64, U8, U16, U32, U64] {
        for to in [F32, F64] {
            assert!(
                !NumericWidth::is_widening_assignable(from, to),
                "{from:?} -> {to:?}"
            );
        }
    }
    assert!(NumericWidth::is_widening_assignable(F32, F64));
}

#[test]
fn policy_words_never_implicitly_assign_across_their_family() {
    let mut types = TypeTable::new();
    let plain = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let modular = types.intern(crate::Type::ModularWord(NumericWidth::U32));
    let saturating = types.intern(crate::Type::SaturatingWord(NumericWidth::U32));
    assert!(types.is_assignable(modular, modular));
    assert!(types.is_assignable(saturating, saturating));
    assert!(!types.is_assignable(plain, modular));
    assert!(!types.is_assignable(modular, plain));
    assert!(!types.is_assignable(modular, saturating));
}

#[test]
fn conversio_widening_keeps_signed_into_f64_but_not_u64() {
    use NumericWidth::{F64, I32, I64, U64};
    assert!(NumericWidth::is_conversio_widening(I32, F64));
    assert!(NumericWidth::is_conversio_widening(I64, F64));
    assert!(!NumericWidth::is_conversio_widening(U64, F64));
    assert!(!NumericWidth::is_explicit_narrowing_conversio(I64, F64));
}

#[test]
fn i64_to_f64_is_not_assignable_but_tensor_element_conversio_allows_it() {
    let mut types = TypeTable::new();
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let f64 = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    let bare_int = types.primitive(Primitive::Numerus);
    let bare_float = types.primitive(Primitive::Fractus);
    assert!(!types.is_assignable(i64, f64));
    assert!(!types.is_assignable(bare_int, bare_float));
    assert!(!types.is_assignable(bare_int, f64));
    assert!(super::tensor_element_conversio_allowed(&types, i64, f64));
    assert!(super::tensor_element_conversio_allowed(
        &types, bare_int, bare_float
    ));
}

#[test]
fn float_exactness_is_representability_not_a_magnitude_limit() {
    use crate::float_holds_integer as holds;
    // 2^53 + 1 needs 54 significant bits; 2^54 and 2^60 need one.
    assert!(holds(NumericWidth::F64, 1 << 53));
    assert!(!holds(NumericWidth::F64, (1 << 53) + 1));
    assert!(holds(NumericWidth::F64, 1 << 54));
    assert!(holds(NumericWidth::F64, 1 << 60));
    assert!(holds(NumericWidth::F64, -(1 << 60)));
    assert!(holds(NumericWidth::F64, 3 << 60));
    assert!(!holds(NumericWidth::F64, (1 << 60) + 1));
    assert!(holds(NumericWidth::F32, 16_777_216));
    assert!(!holds(NumericWidth::F32, 16_777_217));
    assert!(holds(NumericWidth::F32, 1 << 40));
    assert!(holds(NumericWidth::F16, 2048));
    assert!(!holds(NumericWidth::F16, 2049));
    assert!(holds(NumericWidth::F16, 65504));
    assert!(
        !holds(NumericWidth::F16, 65536),
        "finite range ends at 65504"
    );
    assert!(holds(NumericWidth::BF16, 256));
    assert!(!holds(NumericWidth::BF16, 257));
    assert!(holds(NumericWidth::BF16, 1 << 20));
    assert!(holds(NumericWidth::F64, 0));
    assert!(!holds(NumericWidth::I32, 1));
}

#[test]
fn float_overflow_is_rounding_to_infinity() {
    use crate::float_overflows as over;
    assert!(!over(NumericWidth::F32, f64::from(f32::MAX)));
    assert!(over(NumericWidth::F32, 1e40));
    assert!(over(NumericWidth::F32, -1e40));
    assert!(!over(NumericWidth::F16, 65504.0));
    assert!(!over(NumericWidth::F16, 65519.0));
    assert!(over(NumericWidth::F16, 65520.0));
    assert!(!over(NumericWidth::BF16, 3.389_531_389_251_535_5e38));
    assert!(over(NumericWidth::BF16, 1e39));
    assert!(!over(NumericWidth::F64, f64::MAX));
}

#[test]
fn policy_float_cells_lower_for_every_float_width_and_both_policies() {
    use crate::{FloatPolicy, Type};
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    for (marker, width) in [
        ("f16", NumericWidth::F16),
        ("bf16", NumericWidth::BF16),
        ("f32", NumericWidth::F32),
        ("f64", NumericWidth::F64),
    ] {
        let params = [marker_expr(marker, &mut interner)];
        let mut reported = Vec::new();
        let trapping = lower_trapping_word(&params, &interner, &mut types, &mut |issue| {
            reported.push(issue);
        });
        let saturating = lower_saturating_word(&params, &interner, &mut types, &mut |issue| {
            reported.push(issue);
        });
        assert!(reported.is_empty(), "{marker}: {reported:?}");
        assert_eq!(
            types.get(trapping),
            &Type::PolicyFloat(FloatPolicy::Trapping, width)
        );
        assert_eq!(
            types.get(saturating),
            &Type::PolicyFloat(FloatPolicy::Saturating, width)
        );
        assert_ne!(trapping, saturating);
    }
}

#[test]
fn tensor_copy_out_elementwise_methods_require_numeric_element() {
    use crate::tensor_method_requires_numeric_element;
    for name in ["subtracta", "multiplicata"] {
        assert!(
            tensor_method_requires_numeric_element(name),
            "tensor `{name}` must keep the numeric-element gate"
        );
    }
    for name in ["subtrahe", "multiplica"] {
        assert!(
            !tensor_method_requires_numeric_element(name),
            "imperative `{name}` is Numeric-only and must not gate tensor methods"
        );
    }
}

#[test]
fn policy_float_saturating_keeps_the_decimal_rejection() {
    let mut interner = Interner::new();
    let mut types = TypeTable::new();
    let params = [marker_expr("d64", &mut interner)];
    let mut reported = Vec::new();
    lower_saturating_word(&params, &interner, &mut types, &mut |issue| {
        reported.push(issue);
    });
    assert_eq!(reported, vec![WidthLowerIssue::SaturatingWordUnsupported]);
    assert_eq!(reported[0].issue(), "unsupported_saturating_word_width");
}

// ── Ranged store policies ────────────────────────────────────

mod ranged_int {
    use crate::numeric_width::{RangeEnd, WidthLowerIssue, resolve_ranged_int};
    use crate::{IntPolicy, NumericWidth, Primitive, Type, TypeTable};

    fn range(
        width: NumericWidth,
        lo: i128,
        hi: i128,
        inclusive: bool,
    ) -> Result<(i128, i128), WidthLowerIssue> {
        resolve_ranged_int(width, RangeEnd::Value(lo), RangeEnd::Value(hi), inclusive)
    }

    #[test]
    fn half_open_holds_the_low_end_up_to_but_not_the_high_end() {
        assert_eq!(range(NumericWidth::U32, 0, 8, false), Ok((0, 7)));
        assert_eq!(range(NumericWidth::I32, -2, 3, false), Ok((-2, 2)));
    }

    #[test]
    fn inclusive_holds_both_ends() {
        assert_eq!(range(NumericWidth::U32, 0, 7, true), Ok((0, 7)));
        assert_eq!(range(NumericWidth::U8, 0, 255, true), Ok((0, 255)));
    }

    #[test]
    fn infinity_is_no_bound_on_that_side_only() {
        let u32_full = NumericWidth::U32.integer_range().unwrap();
        assert_eq!(
            resolve_ranged_int(
                NumericWidth::U32,
                RangeEnd::Value(0),
                RangeEnd::PosInf,
                false
            ),
            Ok((0, u32_full.1))
        );
        let i32_full = NumericWidth::I32.integer_range().unwrap();
        assert_eq!(
            resolve_ranged_int(
                NumericWidth::I32,
                RangeEnd::NegInf,
                RangeEnd::Value(8),
                false
            ),
            Ok((i32_full.0, 7))
        );
        // `∞` as a low end and `-∞` as a high end are outside any width.
        assert_eq!(
            resolve_ranged_int(
                NumericWidth::I32,
                RangeEnd::PosInf,
                RangeEnd::Value(8),
                false
            ),
            Err(WidthLowerIssue::RangedOutsideWidth)
        );
        assert_eq!(
            resolve_ranged_int(
                NumericWidth::I32,
                RangeEnd::Value(0),
                RangeEnd::NegInf,
                false
            ),
            Err(WidthLowerIssue::RangedOutsideWidth)
        );
    }

    #[test]
    fn an_empty_range_is_refused() {
        assert_eq!(
            range(NumericWidth::U32, 5, 5, false),
            Err(WidthLowerIssue::RangedEmpty)
        );
        assert_eq!(
            range(NumericWidth::U32, 5, 3, true),
            Err(WidthLowerIssue::RangedEmpty)
        );
        // `5…5` holds exactly 5.
        assert_eq!(range(NumericWidth::U32, 5, 5, true), Ok((5, 5)));
    }

    #[test]
    fn a_range_must_fit_the_width() {
        assert_eq!(
            range(NumericWidth::U8, 0, 300, false),
            Err(WidthLowerIssue::RangedOutsideWidth)
        );
        assert_eq!(
            range(NumericWidth::I8, -200, 5, false),
            Err(WidthLowerIssue::RangedOutsideWidth)
        );
        assert_eq!(range(NumericWidth::U8, 0, 256, false), Ok((0, 255)));
    }

    #[test]
    fn unsigned_widths_refuse_a_negative_low_end() {
        assert_eq!(
            range(NumericWidth::U8, -1, 5, false),
            Err(WidthLowerIssue::RangedUnsignedNegative)
        );
        assert_eq!(
            range(NumericWidth::U64, -3, 5, false),
            Err(WidthLowerIssue::RangedUnsignedNegative)
        );
    }

    #[test]
    fn float_decimal_and_inf_widths_take_no_range() {
        for width in [
            NumericWidth::F32,
            NumericWidth::F64,
            NumericWidth::D64,
            NumericWidth::Inf,
        ] {
            assert_eq!(
                range(width, 0, 8, false),
                Err(WidthLowerIssue::RangedWidthUnsupported),
                "{width:?}"
            );
        }
    }

    #[test]
    fn the_whole_width_is_the_plain_form_it_abbreviates() {
        let mut types = TypeTable::new();
        let (lo, hi) = NumericWidth::U8.integer_range().unwrap();
        let trapping = types.ranged_int(IntPolicy::Trapping, NumericWidth::U8, lo, hi);
        assert_eq!(
            trapping,
            types.sized_numeric(Primitive::Numerus, NumericWidth::U8)
        );
        let saturating = types.ranged_int(IntPolicy::Saturating, NumericWidth::U8, lo, hi);
        assert_eq!(
            saturating,
            types.intern(Type::SaturatingWord(NumericWidth::U8))
        );
        let wrapping = types.ranged_int(IntPolicy::Wrapping, NumericWidth::U8, lo, hi);
        assert_eq!(wrapping, types.intern(Type::ModularWord(NumericWidth::U8)));
    }

    #[test]
    fn a_narrower_range_is_its_own_type_and_reads_as_its_width() {
        let mut types = TypeTable::new();
        let cell = types.ranged_int(IntPolicy::Saturating, NumericWidth::U32, 0, 7);
        let again = types.ranged_int(IntPolicy::Saturating, NumericWidth::U32, 0, 7);
        assert_eq!(cell, again, "interning is structural");
        let other = types.ranged_int(IntPolicy::Saturating, NumericWidth::U32, 0, 8);
        assert_ne!(cell, other);
        assert_eq!(
            types.ranged_int_parts(cell),
            Some((IntPolicy::Saturating, NumericWidth::U32, 0, 7))
        );
        let base = types.ranged_int_base(cell);
        assert_eq!(base, types.intern(Type::SaturatingWord(NumericWidth::U32)));
        assert_eq!(types.ranged_int_base_existing(cell), base);
    }

    #[test]
    fn a_ranged_cell_is_assignable_only_where_its_range_is_contained() {
        let mut types = TypeTable::new();
        let small = types.ranged_int(IntPolicy::Saturating, NumericWidth::U32, 0, 4);
        let large = types.ranged_int(IntPolicy::Saturating, NumericWidth::U32, 0, 9);
        let plain = types.intern(Type::SaturatingWord(NumericWidth::U32));
        assert!(types.is_assignable(small, large), "contained range");
        assert!(
            !types.is_assignable(large, small),
            "wider range needs a store"
        );
        assert!(
            types.is_assignable(small, plain),
            "a ranged cell reads as its width"
        );
        assert!(
            !types.is_assignable(plain, small),
            "the width needs a store"
        );
        let wrapping = types.ranged_int(IntPolicy::Wrapping, NumericWidth::U32, 0, 9);
        assert!(
            !types.is_assignable(small, wrapping),
            "policy families never mix"
        );
    }
}

mod ranged_list {
    use crate::numeric_width::{RangeEnd, WidthLowerIssue, resolve_ranged_list};
    use crate::{Primitive, Type, TypeTable};

    fn lengths(lo: i128, hi: i128, inclusive: bool) -> Result<(u64, Option<u64>), WidthLowerIssue> {
        resolve_ranged_list(RangeEnd::Value(lo), RangeEnd::Value(hi), inclusive)
    }

    #[test]
    fn a_half_open_range_stops_one_short_and_an_inclusive_one_holds_the_end() {
        assert_eq!(lengths(4, 8, false), Ok((4, Some(7))));
        assert_eq!(lengths(4, 8, true), Ok((4, Some(8))));
        assert_eq!(lengths(2, 2, true), Ok((2, Some(2))));
    }

    #[test]
    fn infinity_is_no_upper_bound() {
        assert_eq!(
            resolve_ranged_list(RangeEnd::Value(1), RangeEnd::PosInf, true),
            Ok((1, None))
        );
        assert_eq!(
            resolve_ranged_list(RangeEnd::NegInf, RangeEnd::Value(5), false),
            Ok((0, Some(4)))
        );
    }

    #[test]
    fn a_range_that_holds_no_length_is_refused() {
        assert_eq!(lengths(4, 4, false), Err(WidthLowerIssue::RangedListEmpty));
        assert_eq!(lengths(5, 3, true), Err(WidthLowerIssue::RangedListEmpty));
        assert_eq!(
            resolve_ranged_list(RangeEnd::PosInf, RangeEnd::PosInf, false),
            Err(WidthLowerIssue::RangedListEmpty)
        );
        assert_eq!(lengths(0, 0, false), Err(WidthLowerIssue::RangedListEmpty));
    }

    #[test]
    fn a_negative_end_is_refused() {
        assert_eq!(
            lengths(-1, 3, false),
            Err(WidthLowerIssue::RangedListBoundNegative)
        );
        assert_eq!(
            lengths(1, -3, false),
            Err(WidthLowerIssue::RangedListBoundNegative)
        );
        assert_eq!(
            resolve_ranged_list(RangeEnd::Value(1), RangeEnd::NegInf, false),
            Err(WidthLowerIssue::RangedListBoundNegative)
        );
    }

    #[test]
    fn the_sugar_forms_keep_one_identity() {
        let mut types = TypeTable::new();
        let int = types.primitive(Primitive::Numerus);
        // `list<T>` is `0…∞`; `list<T, size N>` is `0…N`.
        assert_eq!(types.ranged_array(int, 0, None), types.array(int));
        let capacity = types.intern_index(crate::IndexExpr::Literal(6));
        let bounded = types.bounded_array(int, capacity);
        // Index ids are not interned, so identity is structural equality.
        let ranged = types.ranged_array(int, 0, Some(6));
        assert!(types.equals(ranged, bounded));
        assert!(types.is_assignable(ranged, bounded));
    }

    #[test]
    fn a_range_with_a_floor_is_its_own_interned_type() {
        let mut types = TypeTable::new();
        let int = types.primitive(Primitive::Numerus);
        let a = types.ranged_array(int, 1, None);
        let b = types.ranged_array(int, 1, None);
        let c = types.ranged_array(int, 2, None);
        assert_eq!(a, b, "interning is structural");
        assert_ne!(a, c);
        assert!(matches!(
            types.get(a),
            Type::RangedArray {
                min: 1,
                max: None,
                ..
            }
        ));
        assert!(!types.is_assignable(a, c), "ranges are invariant");
    }
}
// Behaviour tables for the width-level contracts. These exercise each marker
// directly rather than relying on parser or Faber programs.
#[test]
fn every_bounded_integer_width_reports_its_marker_bounds_and_signedness() {
    use NumericWidth::{I8, I16, I32, I64, U8, U16, U32, U64};

    let cases = [
        ("i8", I8, i128::from(i8::MIN), i128::from(i8::MAX), None),
        ("i16", I16, i128::from(i16::MIN), i128::from(i16::MAX), None),
        ("i32", I32, i128::from(i32::MIN), i128::from(i32::MAX), None),
        ("i64", I64, i128::from(i64::MIN), i128::from(i64::MAX), None),
        ("u8", U8, 0, i128::from(u8::MAX), Some(u64::from(u8::MAX))),
        (
            "u16",
            U16,
            0,
            i128::from(u16::MAX),
            Some(u64::from(u16::MAX)),
        ),
        (
            "u32",
            U32,
            0,
            i128::from(u32::MAX),
            Some(u64::from(u32::MAX)),
        ),
        ("u64", U64, 0, i128::from(u64::MAX), Some(u64::MAX)),
    ];

    for (marker, width, min, max, unsigned_max) in cases {
        assert_eq!(
            NumericWidth::from_marker(marker, Primitive::Numerus),
            Ok(width)
        );
        assert_eq!(width.faber_marker(), marker);
        assert_eq!(width.rust_type(), marker);
        assert_eq!(width.integer_range(), Some((min, max)), "{marker}");
        assert_eq!(width.is_signed_int(), marker.starts_with('i'), "{marker}");
        assert_eq!(width.is_unsigned_int(), marker.starts_with('u'), "{marker}");
        assert!(!width.is_float(), "{marker}");
        assert_eq!(width.unsigned_max(), unsigned_max, "{marker}");
        assert_eq!(
            width.float_truncation_bounds(),
            Some((min.to_string(), (max + 1).to_string())),
            "{marker}"
        );
        assert_eq!(
            radix_width_resolve(width, min, max, true),
            Ok((min, max)),
            "both endpoints fit {marker}"
        );
        assert_eq!(
            radix_width_resolve(width, 0, 0, true),
            Ok((0, 0)),
            "zero fits {marker}"
        );
    }
}

fn radix_width_resolve(
    width: NumericWidth,
    lo: i128,
    hi: i128,
    inclusive: bool,
) -> Result<(i128, i128), crate::WidthLowerIssue> {
    crate::resolve_ranged_int(
        width,
        crate::RangeEnd::Value(lo),
        crate::RangeEnd::Value(hi),
        inclusive,
    )
}

#[test]
fn every_integer_width_rejects_one_past_its_edges() {
    use crate::WidthLowerIssue::{RangedOutsideWidth, RangedUnsignedNegative};
    use NumericWidth::{I8, I16, I32, I64, U8, U16, U32, U64};

    for (width, min, max, is_unsigned) in [
        (I8, i128::from(i8::MIN), i128::from(i8::MAX), false),
        (I16, i128::from(i16::MIN), i128::from(i16::MAX), false),
        (I32, i128::from(i32::MIN), i128::from(i32::MAX), false),
        (I64, i128::from(i64::MIN), i128::from(i64::MAX), false),
        (U8, 0, i128::from(u8::MAX), true),
        (U16, 0, i128::from(u16::MAX), true),
        (U32, 0, i128::from(u32::MAX), true),
        (U64, 0, i128::from(u64::MAX), true),
    ] {
        assert_eq!(
            radix_width_resolve(width, min - 1, max, true),
            Err(if is_unsigned {
                RangedUnsignedNegative
            } else {
                RangedOutsideWidth
            }),
            "one below {width:?}"
        );
        assert_eq!(
            radix_width_resolve(width, min, max + 1, true),
            Err(RangedOutsideWidth),
            "one above {width:?}"
        );
        // A half-open upper bound may be one past the width's inclusive max.
        assert_eq!(
            radix_width_resolve(width, min, max + 1, false),
            Ok((min, max)),
            "exclusive edge for {width:?}"
        );
    }
}

#[test]
fn float_and_decimal_width_metadata_is_not_an_integer_range() {
    use crate::WidthError;
    use NumericWidth::{BF16, D64, F16, F32, F64};

    for (marker, width) in [("f16", F16), ("bf16", BF16), ("f32", F32), ("f64", F64)] {
        assert_eq!(
            NumericWidth::from_marker(marker, Primitive::Fractus),
            Ok(width)
        );
        assert_eq!(
            NumericWidth::from_marker(marker, Primitive::Numerus),
            Err(WidthError::FloatOnNumerus)
        );
        assert_eq!(width.faber_marker(), marker);
        assert_eq!(width.rust_type(), marker);
        assert!(width.is_float(), "{marker}");
        assert!(!width.is_signed_int(), "{marker}");
        assert!(!width.is_unsigned_int(), "{marker}");
        assert_eq!(width.integer_range(), None, "{marker}");
        assert_eq!(width.unsigned_max(), None, "{marker}");
        assert_eq!(width.float_truncation_bounds(), None, "{marker}");
        assert_eq!(width.decimal_scale(), None, "{marker}");
    }

    assert_eq!(
        NumericWidth::from_marker("d64", Primitive::Numerus),
        Ok(D64)
    );
    assert_eq!(
        NumericWidth::from_marker("d64", Primitive::Fractus),
        Err(WidthError::UnrecognizedMarker)
    );
    assert_eq!(D64.faber_marker(), "d64");
    assert_eq!(D64.rust_type(), "d64");
    assert!(D64.is_decimal());
    assert!(!D64.is_float());
    assert!(!D64.is_signed_int());
    assert!(!D64.is_unsigned_int());
    assert_eq!(D64.decimal_scale(), Some(8));
    assert_eq!(D64.integer_range(), None);
    assert_eq!(D64.float_truncation_bounds(), None);
}

#[test]
fn raw_bits_markers_have_container_widths_but_no_numeric_width_entry() {
    use crate::{BitsWidth, WidthError};

    for (marker, bits) in [
        ("b8", BitsWidth::B8),
        ("b16", BitsWidth::B16),
        ("b32", BitsWidth::B32),
        ("b64", BitsWidth::B64),
    ] {
        assert_eq!(BitsWidth::from_marker(marker), Some(bits));
        assert_eq!(bits.faber_marker(), marker);
        assert_eq!(bits.bits(), marker[1..].parse::<u32>().unwrap());
        assert_eq!(
            NumericWidth::from_marker(marker, Primitive::Numerus),
            Err(WidthError::UnrecognizedMarker)
        );
        assert_eq!(
            NumericWidth::from_marker(marker, Primitive::Fractus),
            Err(WidthError::UnrecognizedMarker)
        );
    }
    for marker in ["b1", "b7", "b128", "B8", "b8x"] {
        assert_eq!(BitsWidth::from_marker(marker), None, "{marker}");
    }
}

#[test]
fn width_widening_narrowing_and_checked_conversion_tables_are_complete() {
    use NumericWidth::{BF16, D64, F16, F32, F64, I8, I16, I32, I64, U8, U16, U32, U64};

    let widths = [
        I8, I16, I32, I64, U8, U16, U32, U64, F16, BF16, F32, F64, D64,
    ];
    let widening_edges = [
        (I8, I16),
        (I8, I32),
        (I8, I64),
        (I16, I32),
        (I16, I64),
        (I32, I64),
        (U8, U16),
        (U8, U32),
        (U8, U64),
        (U8, I16),
        (U8, I32),
        (U8, I64),
        (U16, U32),
        (U16, U64),
        (U16, I32),
        (U16, I64),
        (U32, U64),
        (U32, I64),
        (F16, F32),
        (F16, F64),
        (F32, F64),
    ];
    let narrowing_edges = [
        (I16, I8),
        (I32, I8),
        (I32, I16),
        (I64, I8),
        (I64, I16),
        (I64, I32),
        (U16, U8),
        (U32, U8),
        (U32, U16),
        (U64, U8),
        (U64, U16),
        (U64, U32),
        (F32, F16),
        (F64, F16),
        (F64, F32),
    ];

    for from in widths {
        for to in widths {
            let same = from == to;
            let widening = widening_edges.contains(&(from, to));
            let narrowing = narrowing_edges.contains(&(from, to));
            let unsigned_to_signed = matches!(
                (from, to),
                (U8, I16 | I32 | I64) | (U16, I32 | I64) | (U32, I64)
            );
            let expected_assignable = same || widening;
            let expected_conversio_widening = (expected_assignable && !unsigned_to_signed)
                || (matches!(from, I8 | I16 | I32 | I64) && to == F64);

            assert_eq!(
                NumericWidth::is_widening_assignable(from, to),
                expected_assignable,
                "implicit {from:?} -> {to:?}"
            );
            assert_eq!(
                NumericWidth::is_conversio_widening(from, to),
                expected_conversio_widening,
                "explicit widening {from:?} -> {to:?}"
            );
            assert_eq!(
                NumericWidth::is_explicit_narrowing_conversio(from, to),
                narrowing,
                "explicit narrowing {from:?} -> {to:?}"
            );

            match (from.integer_range(), to.integer_range()) {
                (Some((from_min, from_max)), Some((to_min, to_max))) => {
                    let fits = to_min <= from_min && from_max <= to_max;
                    assert_eq!(
                        NumericWidth::is_checked_integer_conversio(from, to),
                        !fits,
                        "checked integer conversion {from:?} -> {to:?}"
                    );
                    assert_eq!(
                        NumericWidth::checked_integer_guards(from, to),
                        (
                            (to_min > from_min).then_some(to_min),
                            (to_max < from_max).then_some(to_max)
                        ),
                        "integer conversion guards {from:?} -> {to:?}"
                    );
                }
                _ => {
                    assert!(!NumericWidth::is_checked_integer_conversio(from, to));
                    assert_eq!(NumericWidth::checked_integer_guards(from, to), (None, None));
                }
            }
        }
    }
    // bf16 has no widening rank: its precision/range tradeoff is incomparable
    // with f16, despite both being 16-bit floating-point formats.
    assert!(!NumericWidth::is_widening_assignable(F16, BF16));
    assert!(!NumericWidth::is_widening_assignable(BF16, F16));
}

#[test]
fn float_integer_fit_checks_each_precision_edge_and_signed_zero() {
    use crate::{float_holds_integer as holds, float_overflows as over};
    use NumericWidth::{BF16, F16, F32, F64};

    for width in [F16, BF16, F32, F64] {
        assert!(holds(width, 0), "zero in {width:?}");
        assert!(holds(width, 1), "one in {width:?}");
        assert!(holds(width, -1), "negative one in {width:?}");
    }
    assert!(holds(F16, 2048));
    assert!(!holds(F16, 2049));
    assert!(holds(F16, -2048));
    assert!(!holds(F16, -2049));
    assert!(holds(F16, 65_504));
    assert!(over(F16, 65_520.0));

    assert!(holds(BF16, 256));
    assert!(!holds(BF16, 257));
    assert!(holds(BF16, 1 << 20));
    assert!(!holds(BF16, (1 << 20) + 1));

    assert!(holds(F32, 1 << 24));
    assert!(!holds(F32, (1 << 24) + 1));
    assert!(holds(F32, 1 << 40));
    assert!(holds(F64, 1 << 53));
    assert!(!holds(F64, (1 << 53) + 1));
    assert!(holds(F64, 1 << 100));
    assert!(!holds(F64, i128::MAX));
}
