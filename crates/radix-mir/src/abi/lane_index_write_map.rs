//! Dense lane-domain suffixes preceded by call-invariant row coordinates.
//!
//! These are transfer facts, not bounds proofs. Device index guards remain.
use super::{BaseKey, IndexPlaces, LaneIndexProof, Sym, operand_key};
use crate::abi::MirKernelResourceAccess;
use crate::visit::MirVisitor;
use crate::{
    MirFunction, MirKernelSignature, MirLocalId, MirOperand, MirParamMode, MirPlace, MirPlaceBase,
    MirStatementKind, MirTerminatorKind, MirValidationContext, MirValueKind,
};

/// A coordinate fixed for the duration of one kernel call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaneWriteSelector {
    /// A nonnegative literal coordinate.
    Constant(u64),
    /// The declaring function's argument position, excluding the lane builtin.
    Argument(usize),
}

/// One dense lane-domain slice written through a resource binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaneWriteMap {
    /// The written tensor's ABI resource binding.
    pub binding: u32,
    /// Fixed coordinates before the contiguous lane-domain suffix.
    pub prefix: Vec<LaneWriteSelector>,
}

/// Recover every written tensor's dense lane slice. Unknown writes refuse the
/// whole map; callers retain their conservative launch policy in that case.
#[must_use]
pub fn lane_write_maps(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
    signature: &MirKernelSignature,
    source: MirLocalId,
) -> Option<Vec<LaneWriteMap>> {
    let proof = LaneIndexProof::new(function, validation)?;
    proof.lane?;
    let domain = proof.tensor_dims(source)?;
    let mut maps = Vec::new();
    let mut places = IndexPlaces::default();
    places.visit_function(function);
    for resource in signature
        .resources()
        .filter(|r| r.access != MirKernelResourceAccess::Read)
    {
        // Fault records are not tensor parameters and have their own combine law.
        let Some(local) = resource.source_local else {
            continue;
        };
        if !known_stores_only(function, local) {
            return None;
        }
        let dims = proof.tensor_dims(local)?;
        let prefix_len = dims.len().checked_sub(domain.len())?;
        let mut found = false;
        for (place, store) in &places.found {
            if !store {
                continue;
            }
            let Some((target, index)) = proof.addressed_tensor(place) else {
                return None;
            };
            if target != local {
                continue;
            }
            found = true;
            let axes = if dims.len() == 1 {
                vec![(index.clone(), None)]
            } else {
                proof.coordinates(index, &dims)?
            };
            // Shape equality is checked again against the actual call. Here
            // match the lane-coordinate expression, without assuming a bound.
            for axis in 0..domain.len() {
                let mut expected = Sym::Lane;
                for later in (axis + 1..domain.len()).rev() {
                    expected = Sym::Div(Box::new(expected), Box::new(domain[later].clone()));
                }
                if axis > 0 {
                    expected = Sym::Mod(Box::new(expected), Box::new(domain[axis].clone()));
                }
                if proof.eval_operand(&axes[prefix_len + axis].0, 0) != expected {
                    return None;
                }
            }
            let prefix = axes[..prefix_len]
                .iter()
                .map(|(operand, _)| selector(&proof, operand, 0))
                .collect::<Option<Vec<_>>>()?;
            let map = LaneWriteMap {
                binding: resource.binding,
                prefix,
            };
            if !maps.contains(&map) {
                maps.push(map);
            }
        }
        if !found {
            return None;
        }
    }
    Some(maps)
}

// Passing a written tensor through an unanalysed call or constructing directly
// into it can hide additional writes. Do not publish an incomplete write map.
fn known_stores_only(function: &MirFunction, local: MirLocalId) -> bool {
    let touches = |place: &MirPlace| match place.base {
        MirPlaceBase::Local(base) => super::resolve_parameter_alias(function, base) == local,
        MirPlaceBase::Temp(_) => false,
    };
    let argument = |arg: &MirOperand| {
        matches!(arg, MirOperand::Place(place)
        if place.projections.is_empty() && touches(place))
    };
    for block in &function.blocks {
        for statement in &block.statements {
            let unknown = match &statement.kind {
                MirStatementKind::Assign { .. } => false,
                MirStatementKind::Call {
                    destination, args, ..
                } => destination.as_ref().is_some_and(&touches) || args.iter().any(&argument),
                MirStatementKind::RuntimeCall { destination, call } => {
                    destination.as_ref().is_some_and(&touches) || call.args.iter().any(&argument)
                }
                MirStatementKind::Construct { destination, .. } => touches(destination),
            };
            if unknown {
                return false;
            }
        }
        if let MirTerminatorKind::TryCall {
            args,
            destination,
            error_place,
            ..
        } = &block.terminator.kind
            && (args.iter().any(&argument)
                || destination.as_ref().is_some_and(&touches)
                || touches(error_place))
        {
            return false;
        }
    }
    true
}

fn selector(
    proof: &LaneIndexProof<'_>,
    operand: &MirOperand,
    depth: u32,
) -> Option<LaneWriteSelector> {
    if depth > 64 {
        return None;
    }
    if let Sym::Const(value) = proof.eval_operand(operand, 0) {
        return u64::try_from(value).ok().map(LaneWriteSelector::Constant);
    }
    let key = operand_key(operand)?;
    if proof.defs.multi.contains(&key) {
        return None;
    }
    if let Some(value) = proof.defs.single.get(&key) {
        return match &value.kind {
            MirValueKind::Operand(source) => selector(proof, source, depth + 1),
            _ => None,
        };
    }
    proof
        .function
        .params
        .iter()
        .filter(|p| p.mode != MirParamMode::Lane)
        .position(|p| key == BaseKey::Local(p.local.0))
        .map(LaneWriteSelector::Argument)
}
