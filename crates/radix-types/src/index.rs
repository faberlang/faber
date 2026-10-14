//! Type-level index terms for indexed types (tensor shape is the first deployment).
//!
//! Index terms include literals, declared size variables (`magnitudo N`),
//! runtime extent identities, affine offsets, exact constant division, and tuple
//! shapes. Arithmetic is represented structurally so equality, substitution,
//! and runtime exactness checks never depend on rendered strings.
//!
//! WHY: Indexed types treat container size as a static fact parallel to dtype.
//! `Figura` in `tensor<T, Figura>` lowers into interned [`IndexExpr`] nodes.

use crate::DefId;
use radix_lexer::Symbol as LexSymbol;
use serde::{Deserialize, Serialize};

/// Arena index into a [`TypeTable`](crate::TypeTable) index store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IndexId(pub u32);

/// Identifier for an index inference metavariable (Gate 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IndexVar(pub u32);

/// Origin of one dimension whose value is supplied at runtime.
///
/// The variants distinguish scalar dimension values from axes read from
/// tensor carriers. HIR-local expression and call-argument identities stay
/// local to one analyzed unit; function-parameter identities use declaration
/// order so public callable shapes can cross module boundaries without
/// serializing producer-local `DefId`s.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RuntimeExtentId {
    /// A scalar runtime binding whose integer value is an extent.
    ScalarBinding(DefId),
    /// A scalar function argument used directly as a dimension, in parameter
    /// declaration order. This form is portable across module interfaces.
    ScalarParameter { parameter: u32 },
    /// An already-bound tensor value's shape axis.
    TensorBindingAxis { binding: DefId, axis: u32 },
    /// An axis read from a record field rooted at a tensor-valued binding.
    TensorFieldAxis {
        binding: DefId,
        fields: Vec<LexSymbol>,
        axis: u32,
    },
    /// A scalar expression result used as an extent (local to one HIR unit).
    ScalarExpression { expression: u32 },
    /// An axis read from an expression-produced tensor carrier.
    ExpressionAxis { expression: u32, axis: u32 },
    /// An axis read from a tensor field inside an expression-produced record.
    ExpressionFieldAxis {
        expression: u32,
        fields: Vec<LexSymbol>,
        axis: u32,
    },
    /// An axis of a value in a call's already-evaluated argument list.
    CallArgumentAxis { argument: u32, axis: u32 },
    /// An axis of a tensor field inside a call's already-evaluated argument.
    CallArgumentFieldAxis {
        argument: u32,
        fields: Vec<LexSymbol>,
        axis: u32,
    },
    /// An axis of a formal tensor parameter, in function parameter order.
    FunctionParameterAxis { parameter: u32, axis: u32 },
    /// An axis of a tensor field reached from a formal record parameter.
    FunctionParameterFieldAxis {
        parameter: u32,
        fields: Vec<LexSymbol>,
        axis: u32,
    },
}

/// A natural-number index term in the semantic type system.
///
/// Tuple nodes represent rank-n shapes (`[N, M]`). An empty tuple is rank-0
/// (`tensor<T, []>`). [`IndexExpr::Unspecified`] is an internal bridge for
/// legacy `tensor<T>` spellings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexExpr {
    /// Compile-time natural literal, including `0`.
    Literal(u64),
    /// Declared size parameter from `magnitudo N` in a generic parameter list.
    Param(LexSymbol),
    /// Runtime-origin extent whose identity points at a scalar or carrier axis.
    Runtime(RuntimeExtentId),
    /// Add a signed constant to another natural-number term.
    ///
    /// The term remains symbolic while its base is unknown. Evaluation reports
    /// a negative or overflowing natural when the base becomes concrete.
    Affine { base: IndexId, offset: i128 },
    /// Divide a term by a positive natural constant, requiring exact division
    /// whenever the numerator is known.
    Divide { numerator: IndexId, divisor: u64 },
    /// Rank-n compound shape.
    Tuple(Vec<IndexId>),
    /// Inference hole; solved by the index inference pass (Stage 3+).
    Infer(IndexVar),
    /// Internal sentinel: source omitted `Figura` (pre-grammar bridge).
    Unspecified,
}

/// Failure while evaluating or constructing natural-number index arithmetic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexArithmeticError {
    DivideByZero,
    Negative,
    NonIntegral,
    Overflow,
    NotScalar,
}

/// Evaluate an index term when all of its leaves are statically known.
///
/// Returns `Ok(None)` for parameters, runtime extents, and inference holes.
/// Exact division and natural-number range are checked for concrete terms.
///
/// # Errors
/// Returns an error for non-scalar terms, negative or overflowing extents,
/// division by zero, or a non-integral quotient.
pub fn evaluate_index_dimension(
    index_id: IndexId,
    types: &crate::TypeTable,
) -> Result<Option<u64>, IndexArithmeticError> {
    match types.get_index(index_id) {
        IndexExpr::Literal(value) => Ok(Some(*value)),
        IndexExpr::Affine { base, offset } => {
            let Some(base) = evaluate_index_dimension(*base, types)? else {
                return Ok(None);
            };
            let value = i128::from(base)
                .checked_add(*offset)
                .ok_or(IndexArithmeticError::Overflow)?;
            if value < 0 {
                return Err(IndexArithmeticError::Negative);
            }
            Ok(Some(
                u64::try_from(value).map_err(|_| IndexArithmeticError::Overflow)?,
            ))
        }
        IndexExpr::Divide { numerator, divisor } => {
            if *divisor == 0 {
                return Err(IndexArithmeticError::DivideByZero);
            }
            let Some(numerator) = evaluate_index_dimension(*numerator, types)? else {
                return Ok(None);
            };
            if numerator % *divisor != 0 {
                return Err(IndexArithmeticError::NonIntegral);
            }
            Ok(Some(numerator / *divisor))
        }
        IndexExpr::Param(_) | IndexExpr::Runtime(_) | IndexExpr::Infer(_) => Ok(None),
        IndexExpr::Tuple(_) | IndexExpr::Unspecified => Err(IndexArithmeticError::NotScalar),
    }
}

/// Add a signed constant to an index term, folding known values and combining
/// adjacent affine offsets.
///
/// # Errors
/// Returns an error for non-scalar terms or invalid concrete arithmetic,
/// including negative or overflowing results.
pub fn add_index_constant(
    types: &mut crate::TypeTable,
    base: IndexId,
    amount: i128,
) -> Result<IndexId, IndexArithmeticError> {
    let term = compose_index_affine_constant(types, base, amount)?;
    match evaluate_index_dimension(term, types)? {
        Some(value) => Ok(types.intern_index(IndexExpr::Literal(value))),
        None => Ok(term),
    }
}

/// Compose an affine offset without validating intermediate natural range.
///
/// Surface `Figura` is an unbounded constant expression whose final dimension
/// must be natural: `0 - 1 + 1` is therefore zero, while `0 - 1` is invalid.
/// Lowerers use this helper across a complete additive chain, then call
/// [`evaluate_index_dimension`] once at the expression boundary. Division
/// remains a separate exactness boundary.
///
/// # Errors
/// Returns an error for non-scalar terms or an overflowing combined offset.
pub fn compose_index_affine_constant(
    types: &mut crate::TypeTable,
    base: IndexId,
    amount: i128,
) -> Result<IndexId, IndexArithmeticError> {
    let (base, amount) = match types.get_index(base).clone() {
        IndexExpr::Affine {
            base,
            offset: previous,
        } => (
            base,
            previous
                .checked_add(amount)
                .ok_or(IndexArithmeticError::Overflow)?,
        ),
        _ => (base, amount),
    };
    if matches!(
        types.get_index(base),
        IndexExpr::Tuple(_) | IndexExpr::Unspecified
    ) {
        return Err(IndexArithmeticError::NotScalar);
    }
    if amount == 0 {
        Ok(base)
    } else {
        Ok(types.intern_index(IndexExpr::Affine {
            base,
            offset: amount,
        }))
    }
}

/// Divide an index term by a positive constant, folding known values and
/// retaining a structured exact-division obligation for symbolic terms.
///
/// # Errors
/// Returns an error for zero divisors, non-integral quotients, or invalid
/// concrete numerator arithmetic.
pub fn divide_index_exact(
    types: &mut crate::TypeTable,
    numerator: IndexId,
    divisor: u64,
) -> Result<IndexId, IndexArithmeticError> {
    if divisor == 0 {
        return Err(IndexArithmeticError::DivideByZero);
    }
    match evaluate_index_dimension(numerator, types)? {
        Some(value) => {
            if value % divisor != 0 {
                return Err(IndexArithmeticError::NonIntegral);
            }
            Ok(types.intern_index(IndexExpr::Literal(value / divisor)))
        }
        None => {
            if divisor == 1 {
                Ok(numerator)
            } else {
                Ok(types.intern_index(IndexExpr::Divide { numerator, divisor }))
            }
        }
    }
}

impl IndexExpr {
    /// Whether this expression is the internal unspecified-shape bridge.
    #[must_use]
    pub fn is_unspecified(&self) -> bool {
        matches!(self, Self::Unspecified)
    }

    /// Whether this expression is an inference hole awaiting constraints.
    #[must_use]
    pub fn is_infer(&self) -> bool {
        matches!(self, Self::Infer(_))
    }
}

/// Extract compile-time literal dimensions when every term is a natural literal.
///
/// Returns `None` when any dimension is symbolic (`Param`, `Runtime`, `Infer`,
/// or `Unspecified`) or does not fit the target `i64` representation.
#[must_use]
pub fn i64s_from_literal_index(index_id: IndexId, types: &crate::TypeTable) -> Option<Vec<i64>> {
    match types.get_index(index_id) {
        IndexExpr::Literal(value) => Some(vec![i64::try_from(*value).ok()?]),
        IndexExpr::Affine { .. } | IndexExpr::Divide { .. } => Some(vec![
            i64::try_from(evaluate_index_dimension(index_id, types).ok()??).ok()?,
        ]),
        IndexExpr::Tuple(dims) => {
            let mut out = Vec::with_capacity(dims.len());
            for dim in dims {
                out.push(i64::try_from(evaluate_index_dimension(*dim, types).ok()??).ok()?);
            }
            Some(out)
        }
        IndexExpr::Param(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Infer(_)
        | IndexExpr::Unspecified => None,
    }
}

/// Compute which axes need sum-reduction when reducing a broadcast result
/// cotangent back to an operand's original shape.
///
/// Returns `None` if the shapes are not broadcast-compatible. Returns
/// `Some(axes)` where `axes` is a list of axis indices in operand-local
/// space (0-indexed from the operand's own rank, not the padded rank).
#[must_use]
pub fn compute_cotangent_reduction_axes(
    operand_shape: IndexId,
    result_shape: IndexId,
    types: &crate::TypeTable,
) -> Option<Vec<usize>> {
    let op_dims = dims_from_shape(operand_shape, types);
    let res_dims = dims_from_shape(result_shape, types);

    let op_rank = op_dims.len();
    let res_rank = res_dims.len();
    let rank = op_rank.max(res_rank);
    let op_pad = rank - op_rank;
    let res_pad = rank - res_rank;

    let mut axes = Vec::new();
    for pos in 0..rank {
        let op_dim_is_one = if pos < op_pad {
            true
        } else {
            is_literal_one(op_dims[pos - op_pad], types)
        };

        let res_dim_is_one = if pos < res_pad {
            true
        } else {
            is_literal_one(res_dims[pos - res_pad], types)
        };

        // A broadcast result is never smaller than its operand: a result dim
        // of one against a non-one operand dim (including a left-padded
        // result against a present operand dim) is not compatible.
        if res_dim_is_one && !op_dim_is_one {
            return None;
        }

        if op_dim_is_one && !res_dim_is_one {
            if pos < op_pad {
                continue;
            }
            axes.push(pos - op_pad);
        } else if !op_dim_is_one
            && !res_dim_is_one
            && !types.index_equals(op_dims[pos - op_pad], res_dims[pos - res_pad])
        {
            return None;
        }
    }

    Some(axes)
}

fn dims_from_shape(shape_id: IndexId, types: &crate::TypeTable) -> Vec<IndexId> {
    match types.get_index(shape_id) {
        IndexExpr::Tuple(dims) => dims.clone(),
        IndexExpr::Literal(_)
        | IndexExpr::Param(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Affine { .. }
        | IndexExpr::Divide { .. }
        | IndexExpr::Infer(_) => vec![shape_id],
        IndexExpr::Unspecified => vec![],
    }
}

fn is_literal_one(id: IndexId, types: &crate::TypeTable) -> bool {
    matches!(types.get_index(id), IndexExpr::Literal(1))
}

/// Parse a NATURAL capacity spelling interned as a named type-argument ident.
///
/// BB-1R interns `8` in `ascii<8>` as the decimal ident `"8"`. Only a pure
/// ASCII-digit spelling is a capacity literal; other names are magnitudo
/// params or non-natural rejects at the resolve site.
#[must_use]
pub fn parse_natural_capacity(name: &str) -> Option<u64> {
    if !name.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    name.parse().ok()
}
