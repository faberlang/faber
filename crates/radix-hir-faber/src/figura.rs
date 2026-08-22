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
#[path = "figura_test.rs"]
mod tests;
