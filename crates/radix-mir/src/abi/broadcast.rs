//! Typed broadcast fact for the device ABI (S6-C2).
//!
//! The explicit bias-broadcast contract requires the device ABI to record the
//! typed broadcast fact (operand/result shapes + broadcast declaration) as a
//! carried fact (A3) — never inferred from names or declarations. This module
//! owns the fact type and the resolver that derives it from a MIR function
//! body (a rank-extension tensor-tensor `TensorAdd`, e.g. the S6-C2
//! `addita_bias` lowering `[B,D]` + `[D]`). A mismatched broadcast pair fails
//! closed before emission (A3/A7).

use crate::{
    MirAbiError, MirCollectionOp, MirFunction, MirRuntimeCall, MirStatementKind,
    MirValidationContext, collection_ops::statement_collection_op, local_ty::operand_local_ty,
    tensor_type::tensor_literal_dims,
};
use serde::{Deserialize, Serialize};

/// How a tensor operand is broadcast against its elementwise peer. The
/// declaration is a carried fact (A3), never derived from source spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MirBroadcastDeclaration {
    /// Rank extension: the lower-rank operand is replicated across the
    /// leading axes of the higher-rank operand — LHS rank = RHS rank + 1 and
    /// the lower-rank dims equal the higher-rank trailing dims (`[B,D]` +
    /// `[D]` → `[B,D]`). The S6-C1 cotangent reduction (`SumAxis` over the
    /// broadcast axes) owns the backward.
    RankExtension,
}

/// The typed broadcast fact the device ABI records for a rank-extension
/// elementwise op (S6-C2): the operand/result shapes and the broadcast
/// declaration. The wire carries the mirror of this fact under a versioned
/// bump with admission (A7); a shape mismatch fails closed at admission.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirRankExtensionBroadcast {
    /// Higher-rank operand shape (e.g. `[B, D]`).
    pub lhs_shape: Vec<u64>,
    /// Lower-rank operand shape (e.g. `[D]`).
    pub rhs_shape: Vec<u64>,
    /// Result shape (equals the higher-rank operand shape for an elementwise
    /// add).
    pub result_shape: Vec<u64>,
    /// The broadcast declaration.
    pub declaration: MirBroadcastDeclaration,
}

/// Resolve the typed rank-extension broadcast fact for a kernel function.
///
/// Scans the kernel body for tensor-tensor `TensorAdd` statements. Returns:
/// - `Ok(None)` when the kernel has no tensor-tensor `TensorAdd` (no fact);
/// - `Ok(Some(fact))` for a provable rank-extension pair
///   (higher rank = lower rank + 1, lower dims = higher trailing dims,
///   result = higher-rank shape);
/// - `Err` (fail closed) for any tensor-tensor `TensorAdd` whose operand
///   shapes are neither equal nor a provable rank extension — such a
///   broadcast cannot be represented as a carried fact and must not reach
///   emission (A3).
///
/// # Errors
///
/// Returns [`MirAbiError`] when an operand type cannot be resolved to a
/// static device-view shape or a tensor-tensor `TensorAdd` carries an
/// invalid broadcast pair.
pub fn rank_extension_broadcast_fact(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<Option<MirRankExtensionBroadcast>, MirAbiError> {
    let mut found: Option<MirRankExtensionBroadcast> = None;
    for block in &function.blocks {
        for statement in &block.statements {
            if statement_collection_op(statement) != Some(MirCollectionOp::TensorAdd) {
                continue;
            }
            let MirStatementKind::RuntimeCall {
                call: MirRuntimeCall {
                    args, return_ty, ..
                },
                ..
            } = &statement.kind
            else {
                continue;
            };
            let Some(left) = args.first().and_then(|arg| operand_local_ty(function, arg)) else {
                continue;
            };
            let Some(right) = args.get(1).and_then(|arg| operand_local_ty(function, arg)) else {
                continue;
            };
            let left_dims = tensor_literal_dims(left, validation).map_err(MirAbiError::new)?;
            let right_dims = tensor_literal_dims(right, validation).map_err(MirAbiError::new)?;
            if left_dims == right_dims {
                continue;
            }
            let result_dims =
                tensor_literal_dims(*return_ty, validation).map_err(MirAbiError::new)?;
            let Some(fact) = classify_rank_extension_pair(&left_dims, &right_dims, &result_dims)
            else {
                return Err(MirAbiError::new(format!(
                    "tensor-tensor add broadcast pair {left_dims:?} + {right_dims:?} → {result_dims:?} is neither same-shape nor a rank extension (S6-C2 A3/A7); the typed broadcast fact cannot be recorded"
                )));
            };
            if found.as_ref().is_some_and(|existing| existing != &fact) {
                return Err(MirAbiError::new(format!(
                    "kernel carries conflicting rank-extension broadcast facts ({found:?} vs {fact:?})"
                )));
            }
            found = Some(fact);
        }
    }
    Ok(found)
}

/// Classify a tensor-tensor add shape triple as the rank-extension pair.
///
/// The higher-rank operand must be exactly one rank above the lower-rank
/// operand and the lower-rank dims must equal the higher-rank trailing dims;
/// the result must be the higher-rank shape (elementwise add preserves it).
fn classify_rank_extension_pair(
    left_dims: &[u64],
    right_dims: &[u64],
    result_dims: &[u64],
) -> Option<MirRankExtensionBroadcast> {
    let (higher, lower) = match (left_dims.len(), right_dims.len()) {
        (h, l) if h == l + 1 => (left_dims, right_dims),
        (h, l) if l == h + 1 => (right_dims, left_dims),
        _ => return None,
    };
    if lower != &higher[1..] {
        return None;
    }
    if result_dims != higher {
        return None;
    }
    Some(MirRankExtensionBroadcast {
        lhs_shape: left_dims.to_vec(),
        rhs_shape: right_dims.to_vec(),
        result_shape: result_dims.to_vec(),
        declaration: MirBroadcastDeclaration::RankExtension,
    })
}
