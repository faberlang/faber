//! Leaf-owned tests for the `HirBinOp` glyph-product helpers used by the
//! fail-closed arm sweep (typecheck, AIR/MIR lowering, backend leaves).

use super::HirBinOp;

#[test]
fn glyph_products_classify_consistently() {
    for op in [
        HirBinOp::Dot,
        HirBinOp::Cross,
        HirBinOp::TensorProduct,
        HirBinOp::Hadamard,
        HirBinOp::HadamardDiv,
    ] {
        assert!(
            op.is_glyph_product(),
            "{op:?} must classify as a glyph product"
        );
        assert!(
            op.glyph_spelling().is_some(),
            "{op:?} must carry a glyph spelling"
        );
    }
}

#[test]
fn non_glyph_ops_do_not_classify_as_glyph_products() {
    for op in [
        HirBinOp::Add,
        HirBinOp::Mul,
        HirBinOp::Eq,
        HirBinOp::Coalesce,
        HirBinOp::InRange,
        HirBinOp::Between,
        HirBinOp::NotInRange,
        HirBinOp::NotBetween,
    ] {
        assert!(
            !op.is_glyph_product(),
            "{op:?} must not classify as a glyph product"
        );
        assert_eq!(
            op.glyph_spelling(),
            None,
            "{op:?} must carry no glyph spelling"
        );
    }
}

#[test]
fn glyph_spellings_match_the_pinned_codepoints() {
    assert_eq!(HirBinOp::Dot.glyph_spelling(), Some("·"));
    assert_eq!(HirBinOp::Cross.glyph_spelling(), Some("×"));
    assert_eq!(HirBinOp::TensorProduct.glyph_spelling(), Some("⊗"));
    assert_eq!(HirBinOp::Hadamard.glyph_spelling(), Some("⊙"));
    assert_eq!(HirBinOp::HadamardDiv.glyph_spelling(), Some("⊘"));
}

#[test]
fn walking_an_implicit_store_reaches_its_source() {
    use super::{
        HirConversioTarget, HirExpression, HirExpressionKind, HirId, HirLiteral, HirStoreKind,
        HirStoreSite,
    };
    use crate::visit::{HirVisitor, walk_expr};
    use radix_lexer::Span;
    use radix_types::TypeId;

    struct Literals(u32);
    impl HirVisitor for Literals {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if matches!(expr.kind, HirExpressionKind::Literal(_)) {
                self.0 += 1;
            }
            walk_expr(self, expr);
        }
    }

    let source = HirExpression {
        id: HirId(1),
        kind: HirExpressionKind::Literal(HirLiteral::Int(300)),
        ty: None,
        span: Span::default(),
    };
    let store = HirExpression {
        id: HirId(2),
        kind: HirExpressionKind::Conversio {
            source: Box::new(source),
            target: HirConversioTarget::Type(TypeId(1)),
            params: Vec::new(),
            recovery: None,
            route: None,
            implicit: Some(HirStoreSite {
                kind: HirStoreKind::Declaration,
                slot: None,
                inferred_from: None,
                inferred_text: None,
            }),
        },
        ty: None,
        span: Span::default(),
    };
    let mut literals = Literals(0);
    literals.visit_expr(&store);
    assert_eq!(literals.0, 1);
}

/// An interface snapshot written before `size_static` existed (a map without
/// that key) must still deserialize; the field is serde-defaulted to `false`.
#[test]
fn hir_local_without_size_static_deserializes() {
    use super::{DefId, HirLocal};
    use radix_lexer::Symbol;
    use serde::Deserialize;
    use serde::de::value::{Error as ValueError, MapDeserializer};
    use serde::de::{Deserializer, IntoDeserializer, Visitor};

    // The small set of value shapes the local's fields need: newtype-wrapped
    // ids, booleans, and null for absent options.
    enum FieldValue {
        U32(u32),
        Bool(bool),
        Null,
    }

    struct FieldDeserializer(FieldValue);

    impl<'de> Deserializer<'de> for FieldDeserializer {
        type Error = ValueError;

        fn deserialize_any<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            match self.0 {
                FieldValue::U32(value) => visitor.visit_u32(value),
                FieldValue::Bool(value) => visitor.visit_bool(value),
                FieldValue::Null => visitor.visit_unit(),
            }
        }

        fn deserialize_newtype_struct<V>(
            self,
            _name: &'static str,
            visitor: V,
        ) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            visitor.visit_newtype_struct(self)
        }

        serde::forward_to_deserialize_any! {
            bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
            bytes byte_buf option unit unit_struct seq tuple tuple_struct map struct
            enum identifier ignored_any
        }
    }

    impl<'de> IntoDeserializer<'de, ValueError> for FieldValue {
        type Deserializer = FieldDeserializer;

        fn into_deserializer(self) -> FieldDeserializer {
            FieldDeserializer(self)
        }
    }

    let original = HirLocal {
        def_id: DefId(7),
        name: Symbol(9),
        ty: None,
        init: None,
        mutable: false,
        is_await: false,
        runtime_provided: false,
        await_store: None,
        size_decl: false,
        size_static: false,
    };
    // Every field except `size_static`, matching an older snapshot's map.
    let entries = vec![
        ("def_id", FieldValue::U32(original.def_id.0)),
        ("name", FieldValue::U32(original.name.0)),
        ("ty", FieldValue::Null),
        ("init", FieldValue::Null),
        ("mutable", FieldValue::Bool(original.mutable)),
        ("is_await", FieldValue::Bool(original.is_await)),
        (
            "runtime_provided",
            FieldValue::Bool(original.runtime_provided),
        ),
        ("await_store", FieldValue::Null),
        ("size_decl", FieldValue::Bool(original.size_decl)),
    ];
    let map = MapDeserializer::<_, ValueError>::new(entries.into_iter());
    let restored =
        HirLocal::deserialize(map).expect("a snapshot without size_static must still deserialize");
    assert!(!restored.size_static);
}
