//! The `d64` fault site of a kernel (kernel-wide types ruling 2026-10-28).
//!
//! A `d64` expression computes in a 128-bit carrier and lands in an `i64`
//! mantissa cell at the store, so a kernel that does decimal arithmetic can
//! trap on the device: a store whose rounded value leaves the `i64` range, a
//! division by zero, a carrier overflow. The device reports it through the
//! option-F fault record (ruling 18(d)) as ONE extra slot, the last site of
//! the record, written by the lowest faulting lane. A kernel without `d64`
//! arithmetic has no such site.

use radix_types::{NumericWidth, Type, TypeTable};

use crate::{
    MirFunction, MirIntrinsic, MirStatementKind, MirType, MirUnOp, MirValidationContext,
    MirValueKind,
};

/// The text of the `d64` fault site: a decimal store, division or carrier
/// that left its range.
pub const D64_FAULT_TEXT: &str = "d64 value out of range";

/// Whether `ty` is `d64`, looking through aliases.
#[must_use]
pub fn is_d64_type(ty: MirType, types: &TypeTable) -> bool {
    let mut semantic = ty.semantic_id();
    loop {
        match types.get(semantic) {
            Type::Alias(_, target) => semantic = *target,
            Type::SizedNumeric(_, NumericWidth::D64) => return true,
            _ => return false,
        }
    }
}

/// Whether `function` does `d64` arithmetic (`+ - * / %` or negation yielding a
/// `d64`) or converts between `d64` and an integer, which gives it the one
/// `d64` fault site.
#[must_use]
pub fn d64_fault_site(function: &MirFunction, validation: &MirValidationContext<'_>) -> bool {
    function.blocks.iter().any(|block| {
        block.statements.iter().any(|statement| {
            // `int ↦ d64` and `d64 ↦ int` can leave their target range.
            if let MirStatementKind::RuntimeCall { call, .. } = &statement.kind
                && let MirIntrinsic::Convert(conversion) = &call.intrinsic
            {
                return is_d64_type(conversion.source_ty, &validation.types)
                    != is_d64_type(conversion.target_ty, &validation.types);
            }
            let MirStatementKind::Assign { value, .. } = &statement.kind else {
                return false;
            };
            let arithmetic = match &value.kind {
                MirValueKind::Binary { op, .. } => {
                    use crate::MirBinOp::{Add, Div, Mod, Mul, Sub};
                    matches!(op, Add | Sub | Mul | Div | Mod)
                }
                MirValueKind::Unary {
                    op: MirUnOp::Neg, ..
                } => true,
                _ => false,
            };
            arithmetic && is_d64_type(value.ty, &validation.types)
        })
    })
}
