// Enum representation selection tests for MIR validation.
//
// These tests prove that MirValidationContext::enum_representation and its
// helper methods select the correct enum ABI variant from metadata provided
// by the lowerer. Each strategy (unit-tag, pointer-niche, boolean-payload,
// integer-payload, float-payload, tagged-union, tagged-product) has its own
// test. Edge cases include missing metadata, full-range domains, invalid
// domains, and wide carrier selection.

use super::*;
use crate::lexer::Symbol;
use crate::semantic::{NumericWidth, Primitive, TypeTable};
use radix_mir::validate::*;

fn ty(types: &TypeTable, primitive: Primitive) -> MirType {
    MirType::semantic(types.primitive(primitive))
}

#[test]
fn enum_representation_reports_unit_tags_in_variant_order() {
    let types = TypeTable::new();
    let enum_def = crate::hir::DefId(30);
    let variant_a = crate::hir::DefId(31);
    let variant_b = crate::hir::DefId(32);
    let mut context = MirValidationContext::new(&types);
    context
        .enum_variants
        .insert(enum_def, vec![variant_b, variant_a]);
    context.variant_parents.insert(variant_a, enum_def);
    context.variant_parents.insert(variant_b, enum_def);
    context
        .variant_fields
        .insert(variant_a, handroll::FxHashMap::default());
    context
        .variant_fields
        .insert(variant_b, handroll::FxHashMap::default());

    assert_eq!(
        context.enum_representation(enum_def),
        Some(MirEnumRepresentation::UnitTags { tag_layout: MirScalarLayout::U8, variants: vec![variant_b, variant_a] })
    );
    assert_eq!(context.enum_tag_layout(enum_def), Some(MirScalarLayout::U8));
    assert_eq!(context.enum_variant_tag(enum_def, variant_b), Some(0));
    assert_eq!(context.enum_variant_tag(enum_def, variant_a), Some(1));
}

#[test]
fn enum_representation_flattens_payload_fields_by_variant_then_symbol() {
    let types = TypeTable::new();
    let number = ty(&types, Primitive::Numerus);
    let boolean = ty(&types, Primitive::Bivalens);
    let enum_def = crate::hir::DefId(40);
    let variant_a = crate::hir::DefId(41);
    let variant_b = crate::hir::DefId(42);
    let mut context = MirValidationContext::new(&types);
    context
        .enum_variants
        .insert(enum_def, vec![variant_a, variant_b]);
    context.variant_parents.insert(variant_a, enum_def);
    context.variant_parents.insert(variant_b, enum_def);

    let mut variant_a_fields = handroll::FxHashMap::default();
    variant_a_fields.insert(Symbol(9), number);
    variant_a_fields.insert(Symbol(2), boolean);
    context.variant_fields.insert(variant_a, variant_a_fields);

    let mut variant_b_fields = handroll::FxHashMap::default();
    variant_b_fields.insert(Symbol(1), number);
    context.variant_fields.insert(variant_b, variant_b_fields);

    assert_eq!(
        context.enum_payload_fields(enum_def),
        Some(vec![
            MirEnumPayloadField { variant: variant_a, field: Symbol(2), ty: boolean },
            MirEnumPayloadField { variant: variant_a, field: Symbol(9), ty: number },
            MirEnumPayloadField { variant: variant_b, field: Symbol(1), ty: number },
        ])
    );
    assert_eq!(
        context.enum_representation(enum_def),
        Some(MirEnumRepresentation::TaggedProduct {
            tag_layout: MirScalarLayout::U8,
            fields: vec![
                MirEnumPayloadField { variant: variant_a, field: Symbol(2), ty: boolean },
                MirEnumPayloadField { variant: variant_a, field: Symbol(9), ty: number },
                MirEnumPayloadField { variant: variant_b, field: Symbol(1), ty: number },
            ],
        })
    );
    assert_eq!(context.enum_tag_layout(enum_def), Some(MirScalarLayout::U8));
}

#[test]
fn enum_representation_selects_tagged_union_for_one_payload_slot() {
    let types = TypeTable::new();
    let number = ty(&types, Primitive::Numerus);
    let enum_def = crate::hir::DefId(45);
    let unit_variant = crate::hir::DefId(46);
    let variant_a = crate::hir::DefId(47);
    let variant_b = crate::hir::DefId(48);
    let mut context = MirValidationContext::new(&types);
    context
        .enum_variants
        .insert(enum_def, vec![unit_variant, variant_a, variant_b]);
    context.variant_parents.insert(unit_variant, enum_def);
    context.variant_parents.insert(variant_a, enum_def);
    context.variant_parents.insert(variant_b, enum_def);
    context
        .variant_fields
        .insert(unit_variant, handroll::FxHashMap::default());

    let mut variant_a_fields = handroll::FxHashMap::default();
    variant_a_fields.insert(Symbol(9), number);
    context.variant_fields.insert(variant_a, variant_a_fields);

    let mut variant_b_fields = handroll::FxHashMap::default();
    variant_b_fields.insert(Symbol(1), number);
    context.variant_fields.insert(variant_b, variant_b_fields);

    let fields = vec![
        MirEnumPayloadField { variant: variant_a, field: Symbol(9), ty: number },
        MirEnumPayloadField { variant: variant_b, field: Symbol(1), ty: number },
    ];
    assert_eq!(
        context.enum_representation(enum_def),
        Some(MirEnumRepresentation::TaggedUnion {
            tag_layout: MirScalarLayout::U8,
            fields: fields.clone(),
            payload_ty: number,
        })
    );
    assert_eq!(context.enum_payload_fields(enum_def), Some(fields));
    assert_eq!(context.enum_tag_layout(enum_def), Some(MirScalarLayout::U8));
}

#[test]
fn enum_representation_selects_pointer_niche_for_unit_and_pointer_payload() {
    let types = TypeTable::new();
    let text = ty(&types, Primitive::Textus);
    let enum_def = crate::hir::DefId(49);
    let unit_variant = crate::hir::DefId(50);
    let payload_variant = crate::hir::DefId(51);
    let field = Symbol(1);
    let mut context = MirValidationContext::new(&types);
    context
        .enum_variants
        .insert(enum_def, vec![unit_variant, payload_variant]);
    context.variant_parents.insert(unit_variant, enum_def);
    context.variant_parents.insert(payload_variant, enum_def);
    context
        .variant_fields
        .insert(unit_variant, handroll::FxHashMap::default());

    let mut payload_fields = handroll::FxHashMap::default();
    payload_fields.insert(field, text);
    context
        .variant_fields
        .insert(payload_variant, payload_fields);

    let payload = MirEnumPayloadField { variant: payload_variant, field, ty: text };
    assert_eq!(
        context.enum_representation(enum_def),
        Some(MirEnumRepresentation::PointerNiche { unit_variant, payload })
    );
    assert_eq!(context.enum_payload_fields(enum_def), Some(vec![payload]));
    assert_eq!(context.enum_tag_layout(enum_def), None);
}

#[test]
fn enum_representation_selects_boolean_payload_packing_for_unit_and_bool_payload() {
    let types = TypeTable::new();
    let boolean = ty(&types, Primitive::Bivalens);
    let enum_def = crate::hir::DefId(55);
    let unit_variant = crate::hir::DefId(56);
    let payload_variant = crate::hir::DefId(57);
    let field = Symbol(1);
    let mut context = MirValidationContext::new(&types);
    context
        .enum_variants
        .insert(enum_def, vec![unit_variant, payload_variant]);
    context.variant_parents.insert(unit_variant, enum_def);
    context.variant_parents.insert(payload_variant, enum_def);
    context
        .variant_fields
        .insert(unit_variant, handroll::FxHashMap::default());

    let mut payload_fields = handroll::FxHashMap::default();
    payload_fields.insert(field, boolean);
    context
        .variant_fields
        .insert(payload_variant, payload_fields);

    let payload = MirEnumPayloadField { variant: payload_variant, field, ty: boolean };
    assert_eq!(
        context.enum_representation(enum_def),
        Some(MirEnumRepresentation::BooleanPayload { unit_variant, payload })
    );
    assert_eq!(context.enum_payload_fields(enum_def), Some(vec![payload]));
    assert_eq!(context.enum_tag_layout(enum_def), None);
}

#[test]
fn enum_representation_selects_integer_payload_packing_for_unit_and_narrow_integer_payload() {
    let mut types = TypeTable::new();
    let integer = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32));
    let enum_def = crate::hir::DefId(58);
    let unit_variant = crate::hir::DefId(59);
    let payload_variant = crate::hir::DefId(60);
    let field = Symbol(1);
    let mut context = MirValidationContext::new(&types);
    context
        .enum_variants
        .insert(enum_def, vec![unit_variant, payload_variant]);
    context.variant_parents.insert(unit_variant, enum_def);
    context.variant_parents.insert(payload_variant, enum_def);
    context
        .variant_fields
        .insert(unit_variant, handroll::FxHashMap::default());

    let mut payload_fields = handroll::FxHashMap::default();
    payload_fields.insert(field, integer);
    context
        .variant_fields
        .insert(payload_variant, payload_fields);

    let payload = MirEnumPayloadField { variant: payload_variant, field, ty: integer };
    assert_eq!(
        context.enum_representation(enum_def),
        Some(MirEnumRepresentation::IntegerPayload {
            unit_variant,
            payload,
            payload_layout: MirScalarLayout::U32,
            carrier_layout: MirScalarLayout::U64,
            sentinel: 4_294_967_296,
        })
    );
    assert_eq!(context.enum_payload_fields(enum_def), Some(vec![payload]));
    assert_eq!(context.enum_tag_layout(enum_def), None);
}

#[test]
fn enum_representation_selects_integer_payload_packing_for_unit_and_wide_integer_payload() {
    let mut types = TypeTable::new();
    let i64_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::I64));
    let u64_ty = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U64));

    for (offset, payload_ty, payload_layout, sentinel) in [
        (0_u32, i64_ty, MirScalarLayout::I64, INTEGER_PAYLOAD_I64_SENTINEL),
        (10_u32, u64_ty, MirScalarLayout::U64, INTEGER_PAYLOAD_U64_SENTINEL),
    ] {
        let enum_def = crate::hir::DefId(61 + offset);
        let unit_variant = crate::hir::DefId(62 + offset);
        let payload_variant = crate::hir::DefId(63 + offset);
        let field = Symbol(1);
        let mut context = MirValidationContext::new(&types);
        context
            .enum_variants
            .insert(enum_def, vec![unit_variant, payload_variant]);
        context.variant_parents.insert(unit_variant, enum_def);
        context.variant_parents.insert(payload_variant, enum_def);
        context
            .variant_fields
            .insert(unit_variant, handroll::FxHashMap::default());

        let mut payload_fields = handroll::FxHashMap::default();
        payload_fields.insert(field, payload_ty);
        context
            .variant_fields
            .insert(payload_variant, payload_fields);

        let payload = MirEnumPayloadField { variant: payload_variant, field, ty: payload_ty };
        assert_eq!(
            context.enum_representation(enum_def),
            Some(MirEnumRepresentation::IntegerPayload {
                unit_variant,
                payload,
                payload_layout,
                carrier_layout: MirScalarLayout::I128,
                sentinel,
            })
        );
    }
}

#[test]
fn enum_representation_selects_same_width_integer_payload_packing_for_explicit_range_domain() {
    let mut types = TypeTable::new();
    let integer = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U32));
    let enum_def = crate::hir::DefId(82);
    let unit_variant = crate::hir::DefId(83);
    let payload_variant = crate::hir::DefId(84);
    let field = Symbol(1);
    let mut context = MirValidationContext::new(&types);
    context
        .enum_variants
        .insert(enum_def, vec![unit_variant, payload_variant]);
    context.variant_parents.insert(unit_variant, enum_def);
    context.variant_parents.insert(payload_variant, enum_def);
    context
        .variant_fields
        .insert(unit_variant, handroll::FxHashMap::default());

    let mut payload_fields = handroll::FxHashMap::default();
    payload_fields.insert(field, integer);
    context
        .variant_fields
        .insert(payload_variant, payload_fields);
    context
        .scalar_value_domains
        .insert(integer.semantic_id(), MirScalarValueDomain::IntegerRange { min: 0, max: 99 });

    let payload = MirEnumPayloadField { variant: payload_variant, field, ty: integer };
    assert_eq!(
        context.enum_representation(enum_def),
        Some(MirEnumRepresentation::IntegerPayload {
            unit_variant,
            payload,
            payload_layout: MirScalarLayout::U32,
            carrier_layout: MirScalarLayout::U32,
            sentinel: 100,
        })
    );
}

#[test]
fn enum_representation_prefers_lower_sentinel_for_signed_range_domain() {
    let mut types = TypeTable::new();
    let integer = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::I16));
    let enum_def = crate::hir::DefId(87);
    let unit_variant = crate::hir::DefId(88);
    let payload_variant = crate::hir::DefId(89);
    let field = Symbol(1);
    let mut context = MirValidationContext::new(&types);
    context
        .enum_variants
        .insert(enum_def, vec![unit_variant, payload_variant]);
    context.variant_parents.insert(unit_variant, enum_def);
    context.variant_parents.insert(payload_variant, enum_def);
    context
        .variant_fields
        .insert(unit_variant, handroll::FxHashMap::default());

    let mut payload_fields = handroll::FxHashMap::default();
    payload_fields.insert(field, integer);
    context
        .variant_fields
        .insert(payload_variant, payload_fields);
    context
        .scalar_value_domains
        .insert(integer.semantic_id(), MirScalarValueDomain::IntegerRange { min: -10, max: 10 });

    let payload = MirEnumPayloadField { variant: payload_variant, field, ty: integer };
    assert_eq!(
        context.enum_representation(enum_def),
        Some(MirEnumRepresentation::IntegerPayload {
            unit_variant,
            payload,
            payload_layout: MirScalarLayout::I16,
            carrier_layout: MirScalarLayout::I16,
            sentinel: -11,
        })
    );
}

#[test]
fn enum_representation_ignores_invalid_or_full_range_domains_for_same_width_packing() {
    let mut types = TypeTable::new();
    let integer = MirType::semantic(types.sized_numeric(Primitive::Numerus, NumericWidth::U8));

    for (offset, domain) in [
        (0_u32, MirScalarValueDomain::IntegerRange { min: 0, max: 255 }),
        (10_u32, MirScalarValueDomain::IntegerRange { min: -1, max: 10 }),
        (20_u32, MirScalarValueDomain::IntegerRange { min: 20, max: 10 }),
    ] {
        let enum_def = crate::hir::DefId(91 + offset);
        let unit_variant = crate::hir::DefId(92 + offset);
        let payload_variant = crate::hir::DefId(93 + offset);
        let field = Symbol(1);
        let mut context = MirValidationContext::new(&types);
        context
            .enum_variants
            .insert(enum_def, vec![unit_variant, payload_variant]);
        context.variant_parents.insert(unit_variant, enum_def);
        context.variant_parents.insert(payload_variant, enum_def);
        context
            .variant_fields
            .insert(unit_variant, handroll::FxHashMap::default());

        let mut payload_fields = handroll::FxHashMap::default();
        payload_fields.insert(field, integer);
        context
            .variant_fields
            .insert(payload_variant, payload_fields);
        context
            .scalar_value_domains
            .insert(integer.semantic_id(), domain);

        let Some(MirEnumRepresentation::IntegerPayload { carrier_layout, sentinel, .. }) =
            context.enum_representation(enum_def)
        else {
            panic!("u8 payload should still use widened integer packing");
        };
        assert_eq!(carrier_layout, MirScalarLayout::U16);
        assert_eq!(sentinel, INTEGER_PAYLOAD_U8_SENTINEL);
    }
}

#[test]
fn enum_representation_keeps_float_scalar_payloads_as_tagged_union() {
    let mut types = TypeTable::new();
    let f32_ty = MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32));

    let enum_def = crate::hir::DefId(61);
    let unit_variant = crate::hir::DefId(62);
    let payload_variant = crate::hir::DefId(63);
    let field = Symbol(1);
    let mut context = MirValidationContext::new(&types);
    context
        .enum_variants
        .insert(enum_def, vec![unit_variant, payload_variant]);
    context.variant_parents.insert(unit_variant, enum_def);
    context.variant_parents.insert(payload_variant, enum_def);
    context
        .variant_fields
        .insert(unit_variant, handroll::FxHashMap::default());

    let mut payload_fields = handroll::FxHashMap::default();
    payload_fields.insert(field, f32_ty);
    context
        .variant_fields
        .insert(payload_variant, payload_fields);

    assert!(matches!(
        context.enum_representation(enum_def),
        Some(MirEnumRepresentation::TaggedUnion { payload_ty, .. }) if payload_ty == f32_ty
    ));
}

#[test]
fn enum_representation_selects_float_payload_packing_for_explicit_non_nan_domain() {
    let mut types = TypeTable::new();
    let f32_ty = MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F32));

    let enum_def = crate::hir::DefId(65);
    let unit_variant = crate::hir::DefId(66);
    let payload_variant = crate::hir::DefId(67);
    let field = Symbol(1);
    let mut context = MirValidationContext::new(&types);
    context
        .enum_variants
        .insert(enum_def, vec![unit_variant, payload_variant]);
    context.variant_parents.insert(unit_variant, enum_def);
    context.variant_parents.insert(payload_variant, enum_def);
    context
        .variant_fields
        .insert(unit_variant, handroll::FxHashMap::default());

    let mut payload_fields = handroll::FxHashMap::default();
    payload_fields.insert(field, f32_ty);
    context
        .variant_fields
        .insert(payload_variant, payload_fields);
    context
        .scalar_value_domains
        .insert(f32_ty.semantic_id(), MirScalarValueDomain::FloatNonNan);

    let payload = MirEnumPayloadField { variant: payload_variant, field, ty: f32_ty };
    assert_eq!(
        context.enum_representation(enum_def),
        Some(MirEnumRepresentation::FloatPayload { unit_variant, payload, payload_layout: MirScalarLayout::F32 })
    );
    assert_eq!(context.enum_payload_fields(enum_def), Some(vec![payload]));
    assert_eq!(context.enum_tag_layout(enum_def), None);
}

#[test]
fn enum_representation_selects_float_payload_packing_for_explicit_finite_domain() {
    let mut types = TypeTable::new();
    let f64_ty = MirType::semantic(types.sized_numeric(Primitive::Fractus, NumericWidth::F64));

    let enum_def = crate::hir::DefId(165);
    let unit_variant = crate::hir::DefId(166);
    let payload_variant = crate::hir::DefId(167);
    let field = Symbol(1);
    let mut context = MirValidationContext::new(&types);
    context
        .enum_variants
        .insert(enum_def, vec![unit_variant, payload_variant]);
    context.variant_parents.insert(unit_variant, enum_def);
    context.variant_parents.insert(payload_variant, enum_def);
    context
        .variant_fields
        .insert(unit_variant, handroll::FxHashMap::default());

    let mut payload_fields = handroll::FxHashMap::default();
    payload_fields.insert(field, f64_ty);
    context
        .variant_fields
        .insert(payload_variant, payload_fields);
    context
        .scalar_value_domains
        .insert(f64_ty.semantic_id(), MirScalarValueDomain::FloatFinite);

    let payload = MirEnumPayloadField { variant: payload_variant, field, ty: f64_ty };
    assert_eq!(
        context.enum_representation(enum_def),
        Some(MirEnumRepresentation::FloatPayload { unit_variant, payload, payload_layout: MirScalarLayout::F64 })
    );
    assert_eq!(context.enum_payload_fields(enum_def), Some(vec![payload]));
    assert_eq!(context.enum_tag_layout(enum_def), None);
}

#[test]
fn enum_representation_keeps_heterogeneous_one_field_payloads_as_product() {
    let types = TypeTable::new();
    let number = ty(&types, Primitive::Numerus);
    let boolean = ty(&types, Primitive::Bivalens);
    let enum_def = crate::hir::DefId(52);
    let variant_a = crate::hir::DefId(53);
    let variant_b = crate::hir::DefId(54);
    let mut context = MirValidationContext::new(&types);
    context
        .enum_variants
        .insert(enum_def, vec![variant_a, variant_b]);
    context.variant_parents.insert(variant_a, enum_def);
    context.variant_parents.insert(variant_b, enum_def);

    let mut variant_a_fields = handroll::FxHashMap::default();
    variant_a_fields.insert(Symbol(1), number);
    context.variant_fields.insert(variant_a, variant_a_fields);

    let mut variant_b_fields = handroll::FxHashMap::default();
    variant_b_fields.insert(Symbol(2), boolean);
    context.variant_fields.insert(variant_b, variant_b_fields);

    assert!(matches!(
        context.enum_representation(enum_def),
        Some(MirEnumRepresentation::TaggedProduct { .. })
    ));
}

#[test]
fn enum_representation_requires_parent_and_field_metadata() {
    let types = TypeTable::new();
    let enum_def = crate::hir::DefId(50);
    let variant = crate::hir::DefId(51);
    let mut context = MirValidationContext::new(&types);
    context.enum_variants.insert(enum_def, vec![variant]);

    assert_eq!(context.enum_representation(enum_def), None);
    assert_eq!(context.enum_variant_tag(enum_def, variant), None);

    context.variant_parents.insert(variant, enum_def);
    assert_eq!(context.enum_representation(enum_def), None);
}

#[test]
fn enum_representation_selects_compact_tag_width_from_variant_count() {
    let types = TypeTable::new();
    let enum_def = crate::hir::DefId(60);
    let mut context = MirValidationContext::new(&types);
    let variants = (0..257)
        .map(|index| crate::hir::DefId(1000 + index))
        .collect::<Vec<_>>();
    context.enum_variants.insert(enum_def, variants.clone());
    for variant in variants {
        context.variant_parents.insert(variant, enum_def);
        context
            .variant_fields
            .insert(variant, handroll::FxHashMap::default());
    }

    assert_eq!(context.enum_tag_layout(enum_def), Some(MirScalarLayout::U16));
}
