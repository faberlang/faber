use super::*;
use radix_lexer::Interner;
use radix_types::{IndexExpr, IndexVar, TypeTable};

fn figura(codegen: &crate::FaberCodegen<'_>, index: IndexId) -> String {
    codegen.format_figura(index)
}

#[test]
fn format_figura_literal_param_tuple_and_infer() {
    let mut types = TypeTable::new();
    let mut interner = Interner::new();
    let n_sym = interner.intern("N");

    let lit = types.intern_index(IndexExpr::Literal(4));
    assert_eq!(
        figura(&crate::FaberCodegen::new(&types, &interner), lit),
        "4"
    );

    let param = types.intern_index(IndexExpr::Param(n_sym));
    assert_eq!(
        figura(&crate::FaberCodegen::new(&types, &interner), param),
        "N"
    );

    let d2 = types.intern_index(IndexExpr::Literal(2));
    let d3 = types.intern_index(IndexExpr::Literal(3));
    let tuple = types.intern_index(IndexExpr::Tuple(vec![d2, d3]));
    assert_eq!(
        figura(&crate::FaberCodegen::new(&types, &interner), tuple),
        "[2, 3]"
    );

    let empty = types.intern_index(IndexExpr::Tuple(vec![]));
    assert_eq!(
        figura(&crate::FaberCodegen::new(&types, &interner), empty),
        "[]"
    );

    let infer = types.intern_index(IndexExpr::Infer(IndexVar(0)));
    assert_eq!(
        figura(&crate::FaberCodegen::new(&types, &interner), infer),
        "_"
    );

    let unspecified = types.intern_index(IndexExpr::Unspecified);
    assert_eq!(
        figura(&crate::FaberCodegen::new(&types, &interner), unspecified),
        "_"
    );
}
