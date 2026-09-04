//! Shared params-then-locals MIR local-type lookup.
//!
//! The local-type lookup (params first, then declared locals) is used by the
//! ABI contract resolver, the plan-mapping pass, the static-shape fold, and
//! the kernel decomposition. This is the single implementation; the
//! [`MirTypeLookup`] trait for place-base resolution lives here too.

use crate::{
    MirFunction, MirLocalId, MirOperand, MirStatementKind, MirTempId, MirType, MirValueId,
};

/// The MIR type of a local (params first, then declared locals).
pub(crate) fn local_ty(function: &MirFunction, local: MirLocalId) -> Option<MirType> {
    function
        .params
        .iter()
        .find(|param| param.local == local)
        .map(|param| param.ty)
        .or_else(|| {
            function
                .locals
                .iter()
                .find(|entry| entry.id == local)
                .map(|entry| entry.ty)
        })
}

/// Resolve a runtime-call operand to the MIR type of the local it references
/// (params first, then locals).
pub(crate) fn operand_local_ty(function: &MirFunction, operand: &MirOperand) -> Option<MirType> {
    let local = crate::read_analysis::operand_local(operand)?;
    local_ty(function, local)
}

/// Resolve the type of any typed MIR operand, including compiler-created
/// temporaries and statement-defined values. Plan mapping needs this broader
/// lookup for expressions such as `weights.transpose()` whose result is a
/// `Value`, not a source local.
pub(crate) fn operand_ty(function: &MirFunction, operand: &MirOperand) -> Option<MirType> {
    match operand {
        MirOperand::Place(place) => match place.base {
            crate::MirPlaceBase::Local(local) => local_ty(function, local),
            crate::MirPlaceBase::Temp(temp) => function.temp_ty(temp),
        },
        MirOperand::Temp(temp) => function.temp_ty(*temp),
        MirOperand::Value(value) => value_ty(function, *value),
        MirOperand::Constant(_) => None,
    }
}

fn value_ty(function: &MirFunction, value: MirValueId) -> Option<MirType> {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .find_map(|statement| {
            let MirStatementKind::Assign { value: defined, .. } = &statement.kind else {
                return None;
            };
            (defined.id == value).then_some(defined.ty)
        })
}

/// Per-function `MirType` lookup for place-base resolution.
///
/// Each probe's `FunctionContext` implements this so shared helpers can resolve
/// local and temp storage types without depending on probe-specific state.
pub trait MirTypeLookup {
    fn local_ty(&self, id: MirLocalId) -> Option<MirType>;
    fn temp_ty(&self, id: MirTempId) -> Option<MirType>;
}
