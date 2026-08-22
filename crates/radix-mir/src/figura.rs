//! Index literal resolution and formatting for MIR.
//!
//! Resolves literal index values from the type table and formats index
//! expressions for diagnostic display. `format_figura` is dead code in this
//! crate but kept for caller convenience in sibling leaf targets.
use radix_lexer::Interner;
use radix_types::{IndexExpr, IndexId, TypeTable};

pub fn i64s_from_literal_index(index_id: IndexId, types: &TypeTable) -> Option<Vec<i64>> {
    match types.get_index(index_id) {
        IndexExpr::Literal(value) => Some(vec![i64::try_from(*value).ok()?]),
        IndexExpr::Tuple(dims) => {
            let mut out = Vec::with_capacity(dims.len());
            for dim in dims {
                let IndexExpr::Literal(value) = types.get_index(*dim) else {
                    return None;
                };
                out.push(i64::try_from(*value).ok()?);
            }
            Some(out)
        }
        IndexExpr::Param(_) | IndexExpr::Infer(_) | IndexExpr::Unspecified => None,
    }
}

#[allow(dead_code)]
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
