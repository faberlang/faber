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
