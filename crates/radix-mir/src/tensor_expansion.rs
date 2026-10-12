//! Static explicit expansion on the device path.
//!
//! `expanded(t.shape())` uses `t` as a typed shape witness after specialization.
//! The list is not device data. The witnessed call keeps two operands, but the
//! second contributes only its shape; the first contributes the values. Array
//! witnesses with live or unresolved contents remain unsupported on devices.

use crate::literal_indexed_access::MirSlot;
use crate::static_shape_fold::{ShapeFoldError, ShapeFoldKind};
use crate::{
    MirCollectionOp, MirFunction, MirIntrinsic, MirOperand, MirRuntimeCall, MirStatementKind,
    MirValidationContext, MirValueKind,
};
use rustc_hash::{FxHashMap, FxHashSet};

/// Source-axis mapping for a concrete, explicitly requested tensor expansion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TensorExpansion {
    pub source_shape: Vec<u64>,
    pub result_shape: Vec<u64>,
    /// Target axis selected for each source axis. Singleton axes read zero.
    pub source_axes: Vec<usize>,
}

/// Cost of mapping one source axis onto one target axis: zero for an exact
/// extent match, one when a singleton source axis stretches, `None` when the
/// source axis cannot map onto the target axis at all.
fn singleton_stretch(source_dim: u64, target_dim: u64) -> Option<usize> {
    if source_dim == target_dim {
        Some(0)
    } else if source_dim == 1 {
        Some(1)
    } else {
        None
    }
}

impl TensorExpansion {
    /// Exact matches win over singleton stretches; ties choose rightmost axes,
    /// as in the semantic checker and MIR runner's `expanded_view`.
    #[must_use]
    pub fn new(source: &[u64], target: &[u64]) -> Option<Self> {
        if source.len() > target.len() || source.contains(&0) || target.contains(&0) {
            return None;
        }
        let impossible = usize::MAX;
        let mut costs = vec![vec![impossible; target.len() + 1]; source.len() + 1];
        costs[source.len()].fill(0);
        for source_axis in (0..source.len()).rev() {
            for first_target in (0..target.len()).rev() {
                for target_axis in first_target..target.len() {
                    let Some(stretch) =
                        singleton_stretch(source[source_axis], target[target_axis])
                    else {
                        continue;
                    };
                    let suffix = costs[source_axis + 1][target_axis + 1];
                    if suffix != impossible {
                        costs[source_axis][first_target] =
                            costs[source_axis][first_target].min(suffix + stretch);
                    }
                }
            }
        }
        let mut remaining = costs[0][0];
        if remaining == impossible {
            return None;
        }
        let mut source_axes = Vec::with_capacity(source.len());
        let mut first_target = 0;
        for source_axis in 0..source.len() {
            let (axis, stretch) = (first_target..target.len()).rev().find_map(|axis| {
                let stretch = singleton_stretch(source[source_axis], target[axis])?;
                let suffix = costs[source_axis + 1][axis + 1];
                (suffix != impossible && suffix + stretch == remaining).then_some((axis, stretch))
            })?;
            source_axes.push(axis);
            first_target = axis + 1;
            remaining -= stretch;
        }
        // Index arithmetic must fit the device's flat-index carrier.
        source
            .iter()
            .try_fold(1_u32, |n, d| n.checked_mul(u32::try_from(*d).ok()?))?;
        target
            .iter()
            .try_fold(1_u32, |n, d| n.checked_mul(u32::try_from(*d).ok()?))?;
        Some(Self {
            source_shape: source.to_vec(),
            result_shape: target.to_vec(),
            source_axes,
        })
    }

    /// Resolve only the typed tensor-witness form. Ordinary shape lists must
    /// first be proven and folded; equal element counts are not shape proof.
    #[must_use]
    pub fn for_call(
        function: &MirFunction,
        call: &MirRuntimeCall,
        validation: &MirValidationContext<'_>,
    ) -> Option<Self> {
        if call.intrinsic != MirIntrinsic::Collection(MirCollectionOp::TensorExpand)
            || call.args.len() != 2
        {
            return None;
        }
        // Projected operands need a projection-aware type/layout proof.
        // The static slot witness never guesses a projection's shape.
        MirSlot::from_operand(&call.args[0])?;
        MirSlot::from_operand(&call.args[1])?;
        let source_ty = crate::local_ty::operand_ty(function, &call.args[0])?;
        let witness_ty = crate::local_ty::operand_ty(function, &call.args[1])?;
        Self::for_types(source_ty, witness_ty, call.return_ty, validation)
    }

    /// Validate the tensor-witness form without depending on a function's slot table.
    #[must_use]
    pub fn for_types(
        source_ty: crate::MirType,
        witness_ty: crate::MirType,
        result_ty: crate::MirType,
        validation: &MirValidationContext<'_>,
    ) -> Option<Self> {
        let (
            radix_types::Type::Tensor(source_element, _),
            radix_types::Type::Tensor(result_element, _),
        ) = (
            validation.types.get(source_ty.semantic_id()),
            validation.types.get(result_ty.semantic_id()),
        )
        else {
            return None;
        };
        if !validation.types.equals(*source_element, *result_element) {
            return None;
        }
        let source = crate::tensor_type::tensor_literal_dims(source_ty, validation).ok()?;
        let witness = crate::tensor_type::tensor_literal_dims(witness_ty, validation).ok()?;
        let target = crate::tensor_type::tensor_literal_dims(result_ty, validation).ok()?;
        if witness != target {
            return None;
        }
        Self::new(&source, &target)
    }

    /// Per-source-axis row-major index terms: `(target axis, target stride,
    /// target extent, source stride)`. Singleton source axes contribute zero.
    #[must_use]
    pub fn index_terms(&self) -> Vec<(usize, u64, u64, u64)> {
        self.source_axes
            .iter()
            .enumerate()
            .filter(|(source_axis, _)| self.source_shape[*source_axis] != 1)
            .map(|(source_axis, &target_axis)| {
                (
                    target_axis,
                    self.result_shape[target_axis + 1..].iter().product(),
                    self.result_shape[target_axis],
                    self.source_shape[source_axis + 1..].iter().product(),
                )
            })
            .collect()
    }
}

/// Replace a proven `TensorShape` list operand by its static tensor witness.
/// Only shape producers rendered unread by this replacement are removed.
/// Shared/escaping shape lists stay live and fail closed at device admission.
pub(crate) fn fold_expansion_witnesses(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> Result<MirFunction, ShapeFoldError> {
    // The fold handles straight-line shape producers only. A multi-block
    // producer needs dominance and mutation proof, not global slot guessing.
    if function.blocks.len() != 1 {
        return Ok(function.clone());
    }
    let mut definitions = FxHashMap::<MirSlot, usize>::default();
    for statement in function.blocks.iter().flat_map(|b| &b.statements) {
        if let Some(slot) = crate::read_analysis::statement_destination_place(statement)
            .and_then(MirSlot::from_place)
        {
            *definitions.entry(slot).or_default() += 1;
        }
    }
    let mut witnesses = FxHashMap::<MirSlot, MirOperand>::default();
    let mut forwarded_shapes = FxHashMap::<MirSlot, MirSlot>::default();
    for statement in &function.blocks[0].statements {
        let Some(slot) = crate::read_analysis::statement_destination_place(statement)
            .and_then(MirSlot::from_place)
        else {
            continue;
        };
        if definitions.get(&slot) != Some(&1) {
            continue;
        }
        let source = match &statement.kind {
            MirStatementKind::RuntimeCall { call, .. }
                if call.intrinsic == MirIntrinsic::Collection(MirCollectionOp::TensorShape) =>
            {
                call.args.first().cloned()
            }
            MirStatementKind::Assign { value, .. } => match &value.kind {
                MirValueKind::Operand(operand) => MirSlot::from_operand(operand)
                    .and_then(|s| witnesses.get(&s))
                    .cloned(),
                _ => None,
            },
            _ => None,
        };
        if let Some(source) = source {
            if let MirStatementKind::Assign { value, .. } = &statement.kind
                && let MirValueKind::Operand(operand) = &value.kind
                && let Some(parent) = MirSlot::from_operand(operand)
            {
                forwarded_shapes.insert(slot, parent);
            }
            witnesses.insert(slot, source);
        }
    }
    let mut folded = function.clone();
    let mut consumed_shapes = FxHashSet::default();
    for statement in &mut folded.blocks[0].statements {
        let MirStatementKind::RuntimeCall { call, .. } = &mut statement.kind else {
            continue;
        };
        if call.intrinsic != MirIntrinsic::Collection(MirCollectionOp::TensorExpand) {
            continue;
        }
        let Some(shape_slot) = call.args.get(1).and_then(MirSlot::from_operand) else {
            continue;
        };
        let Some(witness) = witnesses.get(&shape_slot) else {
            continue;
        };
        let mut candidate = call.clone();
        candidate.args[1] = witness.clone();
        if TensorExpansion::for_call(function, &candidate, validation).is_none() {
            return Err(ShapeFoldError { kind: ShapeFoldKind::NotDeviceMaterializable,
                message: "tensor expansion requires a static compatible source and an exact target-shape witness".to_owned() });
        }
        *call = candidate;
        let mut ancestor = Some(shape_slot);
        while let Some(slot) = ancestor {
            if !consumed_shapes.insert(slot) {
                break;
            }
            ancestor = forwarded_shapes.get(&slot).copied();
        }
    }
    // Remove only the now-dead witness graph, including temp-to-local copies.
    loop {
        let prior = folded.blocks[0].statements.len();
        let reads = folded.blocks[0]
            .statements
            .iter()
            .flat_map(crate::read_analysis::statement_read_slots)
            .chain(crate::read_analysis::terminator_read_slots(
                &folded.blocks[0].terminator.kind,
            ))
            .collect::<Vec<_>>();
        let retained = folded.blocks[0]
            .statements
            .iter()
            .filter(|statement| {
                let Some(place) = crate::read_analysis::statement_destination_place(statement)
                else {
                    return true;
                };
                !MirSlot::from_place(place).is_some_and(|s| consumed_shapes.contains(&s))
                    || reads.contains(&place.base)
            })
            .cloned()
            .collect();
        folded.blocks[0].statements = retained;
        if folded.blocks[0].statements.len() == prior {
            break;
        }
    }
    Ok(folded)
}

#[cfg(test)]
#[path = "tensor_expansion_test.rs"]
mod tests;
