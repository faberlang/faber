//! TEU4: FHIR round-trip of `@ commune` union fields.

use crate::decode::{decode, encode};
use crate::test_support::stub_artifact;
use radix_hir::{HirEnum, HirId, HirItem, HirItemKind, HirVariant, HirVariantField};
use radix_lexer::{Interner, Span};
use radix_types::{DefId, Primitive, TypeTable};

fn span() -> Span {
    Span::new(0, 0)
}

#[test]
fn commune_fields_round_trip_on_union_payload() {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);
    let enum_def = DefId(radix_types::USER_DEF_ID_BASE);
    let incompatible_def = DefId(radix_types::USER_DEF_ID_BASE + 1);
    let mismatch_def = DefId(radix_types::USER_DEF_ID_BASE + 2);
    let message = interner.intern("message");
    let hir_enum = HirEnum {
        name: interner.intern("ShapeError"),
        type_params: Vec::new(),
        commune_fields: vec![HirVariantField {
            name: message,
            ty: textus,
            span: span(),
        }],
        variants: vec![
            HirVariant {
                def_id: incompatible_def,
                name: interner.intern("Incompatible"),
                fields: Vec::new(),
                span: span(),
            },
            HirVariant {
                def_id: mismatch_def,
                name: interner.intern("ElementMismatch"),
                fields: vec![
                    HirVariantField {
                        name: interner.intern("expected"),
                        ty: numerus,
                        span: span(),
                    },
                    HirVariantField {
                        name: interner.intern("actual"),
                        ty: numerus,
                        span: span(),
                    },
                ],
                span: span(),
            },
        ],
    };
    let mut artifact = stub_artifact();
    artifact.hir.items = vec![HirItem {
        annotations: Vec::new(),
        id: HirId(0),
        def_id: enum_def,
        kind: HirItemKind::Enum(hir_enum.clone()),
        span: span(),
    }];
    artifact.types = types.snapshot();
    artifact.interner = interner.strings().to_vec();

    let bytes = encode(&artifact).expect("encode commune artifact");
    let decoded = decode(&bytes).expect("decode commune artifact");
    let HirItemKind::Enum(decoded_enum) = &decoded.hir.items[0].kind else {
        panic!("expected enum item, got {:?}", decoded.hir.items[0].kind);
    };
    assert_eq!(decoded_enum.commune_fields.len(), 1);
    assert_eq!(decoded_enum.commune_fields[0].name, message);
    assert_eq!(decoded_enum.commune_fields[0].ty, textus);
    assert_eq!(decoded_enum.variants.len(), 2);
    assert!(
        decoded_enum.variants[0].fields.is_empty(),
        "unit-looking variant payload stays empty on the wire"
    );
    assert_eq!(decoded_enum.variants[1].fields.len(), 2);
}
