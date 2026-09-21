//! Compile-time tensor/vector shape pretty-printing for Faber re-emission.
//!
//! Converts `IndexExpr` values to their Faber surface spelling
//! (literal, parameter, or tuple shape). Used by the Faber codegen
//! backend to re-emit type figura annotations in canonical form.
//! Does not perform index evaluation or type inference. The type facts
//! and interner ride the [`super::FaberCodegen`] context.
use radix_types::{IndexExpr, IndexId};

impl super::FaberCodegen<'_> {
    /// Pretty-print an interned index expression as Faber `Figura` syntax.
    pub(super) fn format_figura(&self, index_id: IndexId) -> String {
        match self.types.get_index(index_id) {
            IndexExpr::Literal(value) => value.to_string(),
            IndexExpr::Param(symbol) => self.interner.resolve(*symbol).to_owned(),
            IndexExpr::Tuple(dims) if dims.is_empty() => "[]".to_owned(),
            IndexExpr::Tuple(dims) => {
                let parts = dims
                    .iter()
                    .map(|dim| self.format_figura(*dim))
                    .collect::<Vec<_>>()
                    .join(", ");
                format!("[{parts}]")
            }
            IndexExpr::Infer(_) | IndexExpr::Unspecified => "_".to_owned(),
        }
    }
}

#[cfg(test)]
#[path = "figura_test.rs"]
mod tests;
