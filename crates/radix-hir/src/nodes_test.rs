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
