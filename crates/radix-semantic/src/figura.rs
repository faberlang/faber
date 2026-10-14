//! Lowering and pretty-printing for surface `Figura` index syntax.
//!
//! Parser and HIR lowering share this module so `tensor<T, Figura>` spellings
//! intern into the same [`IndexExpr`] arena that unification and inference use.

use super::index::{IndexExpr, IndexId};
use super::{Resolver, SymbolKind, TypeTable};
use radix_lexer::Interner;
use radix_syntax::{FiguraExpr, LiftExtent};
use radix_types::{
    IndexArithmeticError, compose_index_affine_constant, divide_index_exact,
    evaluate_index_dimension,
};

pub const TENSOR_INCOMPLETE_MSG: &str =
    "tensor requires element type and Figura; use tensor<T, Figura>";
pub const TENSOR_MISSING_FIGURA_MSG: &str =
    "tensor requires a Figura shape; use tensor<T, []> for rank-0";
pub const SPARSA_INCOMPLETE_MSG: &str =
    "sparsa requires element type and Figura; use sparsa<T, Figura>";
pub const SPARSA_MISSING_FIGURA_MSG: &str =
    "sparsa requires a Figura shape; use sparsa<T, []> for rank-0";

pub const FIGURA_UNKNOWN_INDEX_MSG: &str = "unknown index variable in Figura";
pub const FIGURA_TYPE_PARAM_IN_INDEX_MSG: &str = "type parameter cannot appear in index position";
pub const FIGURA_NOT_INDEX_PARAM_MSG: &str = "name does not refer to an index parameter";
pub const FIGURA_ARITHMETIC_DIVIDE_BY_ZERO_MSG: &str = "Figura divisor must be greater than zero";
pub const FIGURA_ARITHMETIC_NEGATIVE_MSG: &str = "Figura expression produces a negative extent";
pub const FIGURA_ARITHMETIC_NON_INTEGRAL_MSG: &str = "Figura division must divide evenly";
pub const FIGURA_ARITHMETIC_OVERFLOW_MSG: &str =
    "Figura expression exceeds the natural-number range";
pub const FIGURA_ARITHMETIC_NOT_SCALAR_MSG: &str = "Figura arithmetic requires a scalar dimension";

pub(crate) fn figura_arithmetic_error_message(error: IndexArithmeticError) -> &'static str {
    match error {
        IndexArithmeticError::DivideByZero => FIGURA_ARITHMETIC_DIVIDE_BY_ZERO_MSG,
        IndexArithmeticError::Negative => FIGURA_ARITHMETIC_NEGATIVE_MSG,
        IndexArithmeticError::NonIntegral => FIGURA_ARITHMETIC_NON_INTEGRAL_MSG,
        IndexArithmeticError::Overflow => FIGURA_ARITHMETIC_OVERFLOW_MSG,
        IndexArithmeticError::NotScalar => FIGURA_ARITHMETIC_NOT_SCALAR_MSG,
    }
}

pub(crate) fn figura_arithmetic_reason(message: &str) -> Option<&'static str> {
    match message {
        FIGURA_ARITHMETIC_DIVIDE_BY_ZERO_MSG => Some("division by zero"),
        FIGURA_ARITHMETIC_NEGATIVE_MSG => Some("negative extent"),
        FIGURA_ARITHMETIC_NON_INTEGRAL_MSG => Some("non-integral division"),
        FIGURA_ARITHMETIC_OVERFLOW_MSG => Some("extent overflow"),
        FIGURA_ARITHMETIC_NOT_SCALAR_MSG => Some("non-scalar dimension"),
        _ => None,
    }
}

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
        FiguraExpr::Lift(extent) => match &**extent {
            LiftExtent::Literal(value) => Ok(types.intern_index(IndexExpr::Literal(*value))),
            LiftExtent::Name(ident) => {
                // The spelled lift lowers to the same index term as its
                // payload; the negative-literal refusal ran at resolve.
                lower_figura_expr(&FiguraExpr::Var(ident.clone()), resolver, types)
            }
            LiftExtent::Paren(term) => lower_figura_expr(term, resolver, types),
            LiftExtent::Negative { .. } => Err(FIGURA_NOT_INDEX_PARAM_MSG),
        },
        FiguraExpr::Add(_, _) | FiguraExpr::Subtract(_, _) => {
            lower_figura_affine_expr(figura, resolver, types)
        }
        FiguraExpr::Divide(value, divisor) => {
            let numerator = lower_figura_expr(value, resolver, types)?;
            divide_index_exact(types, numerator, *divisor).map_err(figura_arithmetic_error_message)
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

fn lower_figura_affine_expr(
    figura: &FiguraExpr,
    resolver: &Resolver,
    types: &mut TypeTable,
) -> Result<IndexId, &'static str> {
    let mut offset = 0i128;
    let base_expr =
        flatten_figura_affine(figura, &mut offset).map_err(figura_arithmetic_error_message)?;
    let base = lower_figura_expr(base_expr, resolver, types)?;
    let term = compose_index_affine_constant(types, base, offset)
        .map_err(figura_arithmetic_error_message)?;
    match evaluate_index_dimension(term, types).map_err(figura_arithmetic_error_message)? {
        Some(value) => Ok(types.intern_index(IndexExpr::Literal(value))),
        None => Ok(term),
    }
}

pub(crate) fn flatten_figura_affine<'a>(
    figura: &'a FiguraExpr,
    offset: &mut i128,
) -> Result<&'a FiguraExpr, IndexArithmeticError> {
    match figura {
        FiguraExpr::Add(value, amount) => {
            *offset = offset
                .checked_add(i128::from(*amount))
                .ok_or(IndexArithmeticError::Overflow)?;
            flatten_figura_affine(value, offset)
        }
        FiguraExpr::Subtract(value, amount) => {
            *offset = offset
                .checked_sub(i128::from(*amount))
                .ok_or(IndexArithmeticError::Overflow)?;
            flatten_figura_affine(value, offset)
        }
        _ => Ok(figura),
    }
}

fn resolve_index_param(
    name: radix_lexer::Symbol,
    resolver: &Resolver,
) -> Result<&super::Symbol, &'static str> {
    let def_id = resolver.lookup(name).ok_or(FIGURA_UNKNOWN_INDEX_MSG)?;
    resolver.get_symbol(def_id).ok_or(FIGURA_UNKNOWN_INDEX_MSG)
}

/// Extract compile-time literal dimensions when every term is a natural literal.
///
/// Re-exported from `radix-types` — see [`radix_types::i64s_from_literal_index`].
#[must_use]
pub fn i64s_from_literal_index(index_id: IndexId, types: &TypeTable) -> Option<Vec<i64>> {
    radix_types::i64s_from_literal_index(index_id, types)
}

/// Pretty-print an interned index expression as Faber `Figura` syntax.
#[must_use]
pub fn format_figura(index_id: IndexId, types: &TypeTable, interner: &Interner) -> String {
    format_figura_at_precedence(index_id, types, interner, 0).0
}

fn format_figura_at_precedence(
    index_id: IndexId,
    types: &TypeTable,
    interner: &Interner,
    parent_precedence: u8,
) -> (String, u8) {
    let text = match types.get_index(index_id) {
        IndexExpr::Literal(value) => value.to_string(),
        IndexExpr::Param(symbol) => interner.resolve(*symbol).to_owned(),
        IndexExpr::Runtime(source) => format!("<runtime:{source:?}>"),
        IndexExpr::Affine { base, offset } if *offset == 0 => {
            // An offset of zero is exactly its base: delegate both text and
            // precedence so a parent never adds parentheses around it.
            return format_figura_at_precedence(*base, types, interner, parent_precedence);
        }
        IndexExpr::Affine { base, offset } => {
            let (base, _) = format_figura_at_precedence(*base, types, interner, 1);
            if *offset < 0 {
                format!("{base} - {}", offset.unsigned_abs())
            } else {
                format!("{base} + {offset}")
            }
        }
        IndexExpr::Divide { numerator, divisor } => {
            let (numerator, _) = format_figura_at_precedence(*numerator, types, interner, 2);
            format!("{numerator} / {divisor}")
        }
        IndexExpr::Tuple(dims) if dims.is_empty() => "[]".to_owned(),
        IndexExpr::Tuple(dims) => {
            let parts = dims
                .iter()
                .map(|dim| format_figura_at_precedence(*dim, types, interner, 0).0)
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{parts}]")
        }
        IndexExpr::Infer(_) | IndexExpr::Unspecified => "_".to_owned(),
    };
    let precedence = match types.get_index(index_id) {
        IndexExpr::Affine { .. } => 1,
        IndexExpr::Divide { .. } => 2,
        _ => 3,
    };
    if precedence < parent_precedence {
        (format!("({text})"), precedence)
    } else {
        (text, precedence)
    }
}
