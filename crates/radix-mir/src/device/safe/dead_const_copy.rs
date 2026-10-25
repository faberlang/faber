//! The dead non-kernel-scalar const-copy slot scan: plain
//! destinations of a DEAD const-copy Assign whose value is a scalar the
//! kernel surface cannot express — the `LayerNorm` lowering's `i64` axis
//! const.

use crate::{
    MirFunction, MirLayoutKind, MirLayoutTable, MirLocalId, MirPlaceBase, MirScalarLayout,
    MirTempId, MirType,
};
use handroll::FxHashSet;

/// The dead non-kernel-scalar const-copy slots of one function (need
/// a0184031): plain destinations of a DEAD const-copy Assign whose value is
/// a scalar the kernel surface cannot express — the `LayerNorm` lowering's
/// `i64` axis const (`Assign(Local(3) <- Constant(Int(axis)))` typed `i64`;
/// the recipe call carries the axis directly, so the const local is never
/// read).
///
/// The Metal emitter skips exactly this statement (26424928f Gap 1 —
/// "format/legality-only": the destination is never read, so no semantics
/// change) and the NVVM emitter emits it as a harmless native-i64 slot
/// store; the production CUDA artifact path runs this shared gate BEFORE the
/// emitter, so the gate must agree. The CTO council constraints
/// (5f1b1348) hold structurally:
///
/// - a slot is dead only when NO statement or terminator reads it (the read
///   set covers terminators — a value read only by a block's terminator is
///   still live);
/// - a slot is never a function param (a param is a side effect — always
///   live, and resource-visible), never a result-aggregation slot;
/// - the statement-level skip additionally requires an UNPROJECTED plain
///   slot (an output-projection / projected store is never skipped), a
///   pure `Constant` RHS (no runtime call, no expression — the emitters'
///   dead-skip admits only `MirOperand::Constant`), and a non-kernel-scalar
///   value — so a LIVE or merely-difficult (non-constant) i64 stays
///   fail-closed with the layout diagnostics.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(super) struct DeadConstCopySlots {
    pub(super) locals: FxHashSet<MirLocalId>,
    pub(super) temps: FxHashSet<MirTempId>,
}

impl DeadConstCopySlots {
    /// Whether a place base is one of the dead non-kernel-scalar const-copy
    /// slots.
    pub(super) fn contains(&self, base: &MirPlaceBase) -> bool {
        match base {
            MirPlaceBase::Local(local) => self.locals.contains(local),
            MirPlaceBase::Temp(temp) => self.temps.contains(temp),
        }
    }
}

/// The dead non-kernel-scalar const-copy slots of one function (need
/// a0184031) — the liveness scan the gate shares with the emitters' skip.
///
/// A slot is collected when its declared type is a non-kernel scalar (a
/// `Scalar` layout the kernel surface cannot express — i64 etc.) and it is
/// DEAD: no statement or terminator reads it, and a local slot is not a
/// function param (a param is a side effect — always live). A temp slot
/// traces through the lowered body's forward copy (`Local(l) ← Temp(t)`)
/// exactly like the fold-artifact slots: the temp is dead when the local it
/// forwards into (or a direct operand consumer) is never read.
pub(super) fn dead_non_kernel_scalar_const_slots(
    function: &MirFunction,
    layouts: &MirLayoutTable,
) -> DeadConstCopySlots {
    let read_locals = crate::read_analysis::function_read_locals(function);
    let mut dead = DeadConstCopySlots::default();
    for local in &function.locals {
        if is_non_kernel_scalar_layout(layouts, local.ty)
            && !function.params.iter().any(|param| param.local == local.id)
            && !read_locals.contains(&local.id)
        {
            dead.locals.insert(local.id);
        }
    }
    for temp in &function.temps {
        if !is_non_kernel_scalar_layout(layouts, temp.ty)
            || crate::read_analysis::temp_directly_read(function, temp.id)
        {
            continue;
        }
        match crate::read_analysis::temp_forward_local(function, temp.id) {
            None => {
                dead.temps.insert(temp.id);
            }
            Some(local)
                if !function.params.iter().any(|param| param.local == local)
                    && !read_locals.contains(&local) =>
            {
                dead.temps.insert(temp.id);
            }
            Some(_) => {}
        }
    }
    dead
}

/// Whether a type's layout is a scalar the kernel surface cannot express — a
/// `Scalar` layout that is NOT one of the kernel scalars (f32, bool, i32,
/// u32). A DEAD const-copy of such a value (the `LayerNorm` lowering's `i64`
/// axis const) is skipped as a format/legality-only drop (need,
/// mirroring the Metal emitter's `value_is_non_kernel_scalar`, 26424928f
/// Gap 1). A LIVE or merely-difficult (non-constant) value keeps the layout
/// diagnostics.
pub(super) fn is_non_kernel_scalar_layout(layouts: &MirLayoutTable, ty: MirType) -> bool {
    matches!(
        layouts.layout_for_type(ty),
        Ok(layout)
            if matches!(
                layout.kind,
                MirLayoutKind::Scalar(scalar)
                    if !matches!(
                        scalar,
                        MirScalarLayout::F32
                            | MirScalarLayout::Bool
                            | MirScalarLayout::I32
                            | MirScalarLayout::U32
                    )
            )
    )
}
