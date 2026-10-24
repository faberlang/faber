//! Constant throw sites of a kernel (gpu-reset U9-6).
//!
//! A kernel that declares `⇥ text` can throw. The only throw a device can
//! carry is a CONSTANT text: the lowered `ReturnError` operand is either a
//! string constant or a temp/local whose single definition in the function is
//! a string constant. Each such `ReturnError` is one RECOVERABLE fault site.
//! A `ReturnError` of any other operand (run-time-built text) is not a throw
//! site and keeps the device gate closed.
//!
//! # Site numbering
//!
//! A kernel's fault sites are numbered dense from 0 in one list: tensor index
//! sites (the [`super::LaneIndexProof::unproven`] order), then distinct
//! register-vector index operands in first-use order (both groups `Fatal`),
//! then throw sites in function block order (`Recoverable`). This module
//! numbers only the throws, by ORDINAL `0..n`; the ABI adds both index-site
//! counts to get the record slot. [`crate::Region::Fault`] carries the ordinal.

use std::collections::BTreeMap;

use radix_lexer::Symbol;
use radix_types::{Primitive, Type, TypeTable};

use crate::{
    MirBlockId, MirConstant, MirFunction, MirOperand, MirPlaceBase, MirStatementKind,
    MirTerminatorKind, MirValueKind,
};

use super::lane_index::BaseKey;

/// One constant throw: the block whose terminator is the `ReturnError`, and
/// the interned text it delivers.
#[derive(Debug, Clone, PartialEq)]
pub struct ThrowSite {
    pub block: MirBlockId,
    pub text: Symbol,
    /// The slot holding the constant text (a temp or local defined once), when
    /// the operand is not the constant itself. The device never materializes it.
    pub slot: Option<MirPlaceBase>,
}

/// The constant throw sites of `function`, in block order; `None` when the
/// function does not declare a text error channel or any `ReturnError` is not
/// a constant text (the device gate then reports the failable shape).
/// `Some(vec![])` is a text-channel function that never throws on its own.
#[must_use]
pub fn throw_sites(function: &MirFunction, types: &TypeTable) -> Option<Vec<ThrowSite>> {
    let error_ty = function.error_ty?;
    if !matches!(
        types.get(error_ty.semantic_id()),
        Type::Primitive(Primitive::Textus)
    ) {
        return None;
    }
    let mut constants: BTreeMap<BaseKey, Symbol> = BTreeMap::new();
    let mut multi = std::collections::BTreeSet::new();
    for block in &function.blocks {
        for statement in &block.statements {
            let destinations: Vec<&crate::MirPlace> = match &statement.kind {
                MirStatementKind::Assign { place, value } => {
                    let key = BaseKey::of(&place.base);
                    if let (
                        true,
                        MirValueKind::Operand(MirOperand::Constant(MirConstant::String(text))),
                    ) = (place.projections.is_empty(), &value.kind)
                        && !constants.contains_key(&key)
                    {
                        constants.insert(key, *text);
                        continue;
                    }
                    vec![place]
                }
                MirStatementKind::Call { destination, .. }
                | MirStatementKind::RuntimeCall { destination, .. } => destination.iter().collect(),
                MirStatementKind::Construct { destination, .. } => vec![destination],
            };
            for place in destinations {
                multi.insert(BaseKey::of(&place.base));
            }
        }
        if let MirTerminatorKind::TryCall {
            destination,
            error_place,
            ..
        } = &block.terminator.kind
        {
            for place in destination.iter().chain(std::iter::once(error_place)) {
                multi.insert(BaseKey::of(&place.base));
            }
        }
    }
    let mut sites = Vec::new();
    for block in &function.blocks {
        let MirTerminatorKind::ReturnError(operand) = &block.terminator.kind else {
            continue;
        };
        let (text, slot) = match operand {
            MirOperand::Constant(MirConstant::String(text)) => (*text, None),
            other => {
                let key = super::lane_index::operand_key(other)?;
                if multi.contains(&key) {
                    return None;
                }
                let slot = match other {
                    MirOperand::Temp(temp) => MirPlaceBase::Temp(*temp),
                    MirOperand::Place(place) => place.base.clone(),
                    _ => return None,
                };
                (*constants.get(&key)?, Some(slot))
            }
        };
        sites.push(ThrowSite {
            block: block.id,
            text,
            slot,
        });
    }
    Some(sites)
}
