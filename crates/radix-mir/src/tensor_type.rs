//! Shared tensor-type literal-dimension and kernel-element-layout helpers.
//!
//! The ABI contract resolver and the plan-mapping pass both need the literal
//! dims of a tensor-typed MIR type and the kernel-scalar element-layout
//! predicate. This is the single implementation; each caller maps the shared
//! error (a stable `&'static str` message) onto its own error type.

use crate::{MirScalarLayout, MirType, MirValidationContext};
use radix_types::{IndexExpr, Type};

/// One dimension of a tensor-typed MIR type: a compile-time literal, or an
/// extent that is only bound at run time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dim {
    Literal(u64),
    Runtime,
}

/// The stable message a caller gets when a shape has a run-time extent where a
/// literal one is required. The ABI count reader matches on it to keep the
/// lane-form runtime-shaped marker.
pub(crate) const NON_LITERAL_DIM_MESSAGE: &str = "kernel collection op requires literal dimensions";
pub(crate) const ZERO_LITERAL_DIM_MESSAGE: &str =
    "kernel tensor shape requires positive literal dimensions";
pub(crate) const DIM_PRODUCT_OVERFLOW_MESSAGE: &str = "kernel tensor shape element count overflows";

/// Shape dims of a tensor-typed MIR type (a flat literal is a rank-1 shape; a
/// tuple is the dim list). A non-literal dim is [`Dim::Runtime`]; a shape with
/// no known rank is an error.
///
/// The error is a stable message string; callers map it onto their own error
/// type (the ABI resolver and the plan-mapping pass differ in error type but
/// share the message text).
pub(crate) fn tensor_dims(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<Vec<Dim>, &'static str> {
    let Type::Tensor(_, shape) = validation.types.get(ty.semantic_id()) else {
        return Err("kernel collection op requires tensor types");
    };
    match validation.types.get_index(*shape) {
        IndexExpr::Literal(count) => Ok(vec![Dim::Literal(*count)]),
        IndexExpr::Tuple(dims) => Ok(dims
            .iter()
            .map(|&dim| match validation.types.get_index(dim) {
                IndexExpr::Literal(count) => Dim::Literal(*count),
                _ => Dim::Runtime,
            })
            .collect()),
        _ => Err("kernel collection op requires static shape"),
    }
}

/// Literal shape dims of a tensor-typed MIR type; a run-time extent is an
/// error. See [`tensor_dims`].
pub(crate) fn tensor_literal_dims(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<Vec<u64>, &'static str> {
    tensor_dims(ty, validation)?
        .into_iter()
        .map(|dim| match dim {
            Dim::Literal(count) => Ok(count),
            Dim::Runtime => Err(NON_LITERAL_DIM_MESSAGE),
        })
        .collect()
}

/// Checked element count of a shape dim list. A run-time dim is a refusal, a
/// zero literal dim is a refusal (no empty kernel buffer), and a product that
/// overflows `u64` is a refusal — never a saturated count that downstream reads
/// as a literal allocation size.
pub(crate) fn dims_product_checked(dims: &[Dim]) -> Result<u64, &'static str> {
    let mut product: u64 = 1;
    let mut has_runtime_dim = false;
    for dim in dims {
        match dim {
            Dim::Runtime => has_runtime_dim = true,
            Dim::Literal(0) => {
                return Err(ZERO_LITERAL_DIM_MESSAGE);
            }
            Dim::Literal(count) => {
                product = product
                    .checked_mul(*count)
                    .ok_or(DIM_PRODUCT_OVERFLOW_MESSAGE)?;
            }
        }
    }
    if has_runtime_dim {
        Err(NON_LITERAL_DIM_MESSAGE)
    } else {
        Ok(product)
    }
}

/// Whether a scalar layout is a kernel buffer element — the only scalar
/// layouts a kernel storage buffer can carry. Language booleans keep their
/// one-bit value layout but use one canonical byte per device-buffer cell.
pub(crate) fn is_kernel_element_layout(layout: MirScalarLayout) -> bool {
    matches!(
        layout,
        MirScalarLayout::Bool
            | MirScalarLayout::Bf16
            | MirScalarLayout::F32
            | MirScalarLayout::I32
            | MirScalarLayout::U32
            | MirScalarLayout::I64
            | MirScalarLayout::U64
            | MirScalarLayout::D64
            | MirScalarLayout::U8
    )
}

#[cfg(test)]
#[path = "tensor_type_test.rs"]
mod tests;
