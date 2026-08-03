//! Compile-time tensor/vector shape pretty-printing for Faber re-emission.
//!
//! Converts `IndexExpr` values to their Faber surface spelling
//! (literal, parameter, or tuple shape). Used by the Faber codegen
//! backend to re-emit type figura annotations in canonical form.
//! Does not perform index evaluation or type inference.
use radix_lexer::Interner;
use radix_types::{IndexExpr, IndexId, TypeTable};

#[must_use]
pub fn format_figura(index_id: IndexId, types: &TypeTable, interner: &Interner) -> String {
    match types.get_index(index_id) {
        IndexExpr::Literal(value) => value.to_string(),
        IndexExpr::Param(symbol) => interner.resolve(*symbol).to_owned(),
        IndexExpr::Tuple(dims) if dims.is_empty() => "[]".to_owned(),
        IndexExpr::Tuple(dims) => {
            let parts = dims
                .iter()
                .map(|dim| format_figura(*dim, types, interner))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{parts}]")
        }
        IndexExpr::Infer(_) | IndexExpr::Unspecified => "_".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radix_lexer::Interner;
    use radix_types::{IndexExpr, IndexVar, TypeTable};

    #[test]
    fn format_figura_literal_param_tuple_and_infer() {
        let mut types = TypeTable::new();
        let mut interner = Interner::new();
        let n_sym = interner.intern("N");

        let lit = types.intern_index(IndexExpr::Literal(4));
        assert_eq!(format_figura(lit, &types, &interner), "4");

        let param = types.intern_index(IndexExpr::Param(n_sym));
        assert_eq!(format_figura(param, &types, &interner), "N");

        let d2 = types.intern_index(IndexExpr::Literal(2));
        let d3 = types.intern_index(IndexExpr::Literal(3));
        let tuple = types.intern_index(IndexExpr::Tuple(vec![d2, d3]));
        assert_eq!(format_figura(tuple, &types, &interner), "[2, 3]");

        let empty = types.intern_index(IndexExpr::Tuple(vec![]));
        assert_eq!(format_figura(empty, &types, &interner), "[]");

        let infer = types.intern_index(IndexExpr::Infer(IndexVar(0)));
        assert_eq!(format_figura(infer, &types, &interner), "_");

        let unspecified = types.intern_index(IndexExpr::Unspecified);
        assert_eq!(format_figura(unspecified, &types, &interner), "_");
    }
}
