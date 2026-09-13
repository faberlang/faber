//! TEU4: Faber re-emit tail for `@ commune` union fields.

use super::{FaberCodegen, KeywordSurface, TypeBindingKind, empty_presentation};
use radix_hir::{
    DefId, HirEnum, HirId, HirItem, HirItemKind, HirModule, HirVariant, HirVariantField,
};
use radix_lexer::{Interner, Span, TokenKind};
use radix_types::{Primitive, TypeTable};

fn span() -> Span {
    Span::new(0, 0)
}

struct LatinSurface;

impl KeywordSurface for LatinSurface {
    fn keyword(&self, kind: TokenKind) -> &str {
        match kind {
            TokenKind::Discretio => "discretio",
            _ => "?",
        }
    }

    fn type_surface(&self, kind: TypeBindingKind) -> &str {
        match kind {
            TypeBindingKind::Primitive(Primitive::Textus) => "textus",
            TypeBindingKind::Primitive(Primitive::Numerus) => "numerus",
            _ => "ignotum",
        }
    }
}

fn emit_shape_error() -> String {
    let mut interner = Interner::new();
    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);
    let enum_def = DefId(radix_types::USER_DEF_ID_BASE);
    let incompatible_def = DefId(radix_types::USER_DEF_ID_BASE + 1);
    let mismatch_def = DefId(radix_types::USER_DEF_ID_BASE + 2);
    let hir = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(1),
            def_id: enum_def,
            kind: HirItemKind::Enum(HirEnum {
                name: interner.intern("ShapeError"),
                type_params: Vec::new(),
                commune_fields: vec![HirVariantField {
                    name: interner.intern("message"),
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
            }),
            span: span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    let mut codegen = FaberCodegen::default();
    codegen
        .generate(
            &hir,
            &empty_presentation(),
            &types,
            &interner,
            &LatinSurface,
        )
        .expect("faber commune emit succeeds")
        .code
}

#[test]
fn commune_fields_reemit_once_on_the_union() {
    let code = emit_shape_error();
    assert!(
        code.contains("@ commune") && code.contains("textus message"),
        "re-emit must print the commune region once: {code}"
    );
    assert!(
        code.contains("discretio ShapeError"),
        "union declaration missing: {code}"
    );
    assert!(
        !code.contains("Incompatible {"),
        "unit-looking variant must stay field-free in source: {code}"
    );
    assert!(
        code.contains("ElementMismatch")
            && code.contains("numerus expected")
            && code.contains("numerus actual"),
        "structured variant must keep payload fields: {code}"
    );
}
