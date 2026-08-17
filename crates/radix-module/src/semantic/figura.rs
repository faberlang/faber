//! Lowering and pretty-printing for surface `Figura` index syntax.
//!
//! Parser and HIR lowering share this module so `tensor<T, Figura>` spellings
//! intern into the same [`IndexExpr`] arena that unification and inference use.

use super::index::{IndexExpr, IndexId};
use super::{Resolver, SymbolKind, TypeTable};
use crate::lexer::Interner;
use crate::syntax::FiguraExpr;

pub const TENSOR_INCOMPLETE_MSG: &str = "tensor requires element type and Figura; use tensor<T, Figura>";
pub const TENSOR_MISSING_FIGURA_MSG: &str = "tensor requires a Figura shape; use tensor<T, []> for rank-0";
pub const SPARSA_INCOMPLETE_MSG: &str = "sparsa requires element type and Figura; use sparsa<T, Figura>";
pub const SPARSA_MISSING_FIGURA_MSG: &str = "sparsa requires a Figura shape; use sparsa<T, []> for rank-0";

pub const FIGURA_UNKNOWN_INDEX_MSG: &str = "unknown index variable in Figura";
pub const FIGURA_TYPE_PARAM_IN_INDEX_MSG: &str = "type parameter cannot appear in index position";
pub const FIGURA_NOT_INDEX_PARAM_MSG: &str = "name does not refer to an index parameter";

/// Lower surface `Figura` syntax into the shared index arena.
///
/// # Errors
///
/// Returns an error message when `figura` references an undefined index
/// parameter or uses an unsupported expression form.
pub fn lower_figura_expr(
    figura: &FiguraExpr,
    resolver: &Resolver,
    types: &mut TypeTable,
) -> Result<IndexId, &'static str> {
    match figura {
        FiguraExpr::Infer => {
            let var = types.fresh_index_infer_var();
            Ok(types.intern_index(IndexExpr::Infer(var)))
        }
        FiguraExpr::Literal(value) => Ok(types.intern_index(IndexExpr::Literal(*value))),
        FiguraExpr::Var(ident) => {
            let symbol = resolve_index_param(ident.name, resolver)?;
            match symbol.kind {
                SymbolKind::IndexParam => Ok(types.intern_index(IndexExpr::Param(ident.name))),
                SymbolKind::TypeParam => Err(FIGURA_TYPE_PARAM_IN_INDEX_MSG),
                _ => Err(FIGURA_NOT_INDEX_PARAM_MSG),
            }
        }
        FiguraExpr::Tuple(dims) => {
            let lowered = dims
                .iter()
                .map(|dim| lower_figura_expr(dim, resolver, types))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(types.intern_index(IndexExpr::Tuple(lowered)))
        }
    }
}

fn resolve_index_param(name: crate::lexer::Symbol, resolver: &Resolver) -> Result<&super::Symbol, &'static str> {
    let def_id = resolver.lookup(name).ok_or(FIGURA_UNKNOWN_INDEX_MSG)?;
    resolver.get_symbol(def_id).ok_or(FIGURA_UNKNOWN_INDEX_MSG)
}

/// Extract compile-time literal dimensions when every term is a natural literal.
///
/// Re-exported from `radix-types` — see [`radix_types::literal_index_i64s`].
#[must_use]
pub fn literal_index_i64s(index_id: IndexId, types: &TypeTable) -> Option<Vec<i64>> {
    radix_types::literal_index_i64s(index_id, types)
}

/// Pretty-print an interned index expression as Faber `Figura` syntax.
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

#[cfg(test)]
#[path = "figura_literal_test.rs"]
mod literal_tests;
