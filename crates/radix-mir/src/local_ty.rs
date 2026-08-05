//! Shared params-then-locals MIR local-type lookup.
//!
//! The local-type lookup (params first, then declared locals) is used by the
//! ABI contract resolver, the plan-mapping pass, the static-shape fold, and
//! the kernel decomposition. This is the single implementation; the
//! [`MirTypeLookup`] trait for place-base resolution lives here too.

use crate::{MirFunction, MirLocalId, MirOperand, MirTempId, MirType};

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

/// Per-function `MirType` lookup for place-base resolution.
///
/// Each probe's `FunctionContext` implements this so shared helpers can resolve
/// local and temp storage types without depending on probe-specific state.
pub trait MirTypeLookup {
    fn local_ty(&self, id: MirLocalId) -> Option<MirType>;
    fn temp_ty(&self, id: MirTempId) -> Option<MirType>;
}
