//! The per-lane scalar rule of a lifted register operator.
//!
//! A `vector` or `matrix` operator against a scalar or a same-shape peer is
//! the scalar operator applied to each lane (gpu-reset rule 15.1). This module
//! is the single home of that rule: [`LaneOp::apply`] emits one lane's
//! statements through a [`LaneSink`], so the vector expander (deferred until
//! the width is a literal) and the matrix lowerer (literal shape at lowering)
//! build identical lanes.
//!
//! INVARIANTS
//! ==========
//! - A lane is computed exact: the operator never clamps, wraps, or traps
//!   per operation. A width limit applies once, at the store of the lane into
//!   the result element (`checked`, the integer store node of ruling 19(g)).
//! - `÷` converts integer lanes to the result float first and traps an
//!   integer zero divisor through a discarded `1 / divisor`, exactly as the
//!   scalar `÷` lowers; float lanes divide as `/` does.
//! - `⤒`/`⤓` widen both lanes to the result element, then take the larger or
//!   smaller lane (the scalar `maximus`/`minimus`).

use crate::{
    MirBinOp, MirCollectionOp, MirConstant, MirConversion, MirConversionFlavor, MirIntrinsic,
    MirLaneOp, MirOperand, MirType,
};
use radix_hir::{HirStoreKind, HirStoreSite};
use radix_types::{Primitive, Type, TypeTable};

/// Where one lane's statements go.
pub trait LaneSink {
    /// `lhs op rhs` as a fresh temp of type `ty`.
    fn lane_binary(
        &mut self,
        op: MirBinOp,
        lhs: MirOperand,
        rhs: MirOperand,
        ty: MirType,
    ) -> MirOperand;

    /// `operand` read into a fresh temp of type `ty` (a lane place becomes a
    /// value a runtime call can take).
    fn lane_read(&mut self, operand: MirOperand, ty: MirType) -> MirOperand;

    /// A runtime `Convert` of `value` from `source` to `target`; with a
    /// `store` site it is the checked store node.
    fn lane_convert(
        &mut self,
        value: MirOperand,
        source: MirType,
        target: MirType,
        store: Option<HirStoreSite>,
    ) -> MirOperand;

    /// A runtime call whose result lands in a fresh temp of type `ty`.
    fn lane_call(
        &mut self,
        intrinsic: MirIntrinsic,
        args: Vec<MirOperand>,
        ty: MirType,
    ) -> MirOperand;
}

/// The per-lane operator of one lifted site: the operator, whether each lane
/// is stored under `element` with the integer trap, the operand lane types
/// (left, right) and the result element type.
pub struct LaneOp<'t> {
    pub op: MirLaneOp,
    pub checked: bool,
    pub lane_tys: (MirType, MirType),
    pub element: MirType,
    pub types: &'t TypeTable,
}

/// `value` stored under `element`: a checked `Convert` that traps when the
/// exact lane leaves the element's range (the same store node the lowerer
/// emits for an integer slot). `value` itself when unchecked.
pub fn stored_lane<S: LaneSink>(
    sink: &mut S,
    value: MirOperand,
    element: MirType,
    checked: bool,
) -> MirOperand {
    if !checked {
        return value;
    }
    let store = HirStoreSite {
        kind: HirStoreKind::Element,
        slot: None,
        inferred_from: None,
        inferred_text: None,
    };
    sink.lane_convert(value, element, element, Some(store))
}

/// Whether a lane type is an integer (a `wrapping`/`saturating` word counts).
fn is_integer_lane(types: &TypeTable, ty: MirType) -> bool {
    matches!(
        types.get(types.peel_access_marker(ty.semantic_id()).1),
        Type::Primitive(Primitive::Numerus)
            | Type::SizedNumeric(Primitive::Numerus, _)
            | Type::ModularWord(_)
            | Type::SaturatingWord(_)
            | Type::RangedInt { .. }
    )
}

impl LaneOp<'_> {
    /// One lane of the result: `lhs op rhs`, exact, then stored under the
    /// element type when `checked`.
    pub fn apply<S: LaneSink>(&self, sink: &mut S, lhs: MirOperand, rhs: MirOperand) -> MirOperand {
        let value = match self.op.bin_op() {
            Some(op) => sink.lane_binary(op, lhs, rhs, self.element),
            None if self.op == MirLaneOp::TrueDiv => self.true_div(sink, lhs, rhs),
            None => self.extremum(sink, lhs, rhs),
        };
        stored_lane(sink, value, self.element, self.checked)
    }

    fn true_div<S: LaneSink>(&self, sink: &mut S, lhs: MirOperand, rhs: MirOperand) -> MirOperand {
        let (lhs_ty, rhs_ty) = self.lane_tys;
        if !(is_integer_lane(self.types, lhs_ty) && is_integer_lane(self.types, rhs_ty)) {
            return sink.lane_binary(MirBinOp::Div, lhs, rhs, self.element);
        }
        let divisor = sink.lane_read(rhs, rhs_ty);
        let dividend = sink.lane_read(lhs, lhs_ty);
        // The `1 / divisor` temporary is the zero-divisor trap, not dead code:
        // nothing reads it, but evaluating it is what fails.
        let one = MirOperand::Constant(MirConstant::Int(1));
        sink.lane_binary(MirBinOp::Div, one, divisor.clone(), rhs_ty);
        let dividend = sink.lane_convert(dividend, lhs_ty, self.element, None);
        let divisor = sink.lane_convert(divisor, rhs_ty, self.element, None);
        sink.lane_binary(MirBinOp::Div, dividend, divisor, self.element)
    }

    fn extremum<S: LaneSink>(&self, sink: &mut S, lhs: MirOperand, rhs: MirOperand) -> MirOperand {
        let (lhs_ty, rhs_ty) = self.lane_tys;
        let element = self.element;
        let widen = |sink: &mut S, value: MirOperand, ty: MirType| {
            let read = sink.lane_read(value, ty);
            if ty == element {
                read
            } else {
                sink.lane_convert(read, ty, element, None)
            }
        };
        let lhs = widen(sink, lhs, lhs_ty);
        let rhs = widen(sink, rhs, rhs_ty);
        let collection = if self.op == MirLaneOp::Max {
            MirCollectionOp::Maximum
        } else {
            MirCollectionOp::Minimum
        };
        sink.lane_call(
            MirIntrinsic::Collection(collection),
            vec![lhs, rhs],
            element,
        )
    }
}

/// A runtime `Convert` node for [`LaneSink`] implementors.
#[must_use]
pub fn lane_conversion(
    source: MirType,
    target: MirType,
    store: Option<HirStoreSite>,
) -> MirConversion {
    MirConversion {
        store,
        flavor: MirConversionFlavor::Runtime,
        source_ty: source,
        target_ty: target,
        params: Vec::new(),
        recovery: None,
        struct_defaults: Vec::new(),
    }
}
