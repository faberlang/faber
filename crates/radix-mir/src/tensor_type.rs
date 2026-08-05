//! Shared tensor-type literal-dimension and kernel-element-layout helpers.
//!
//! The ABI contract resolver and the plan-mapping pass both need the literal
//! dims of a tensor-typed MIR type and the kernel-scalar element-layout
//! predicate. This is the single implementation; each caller maps the shared
//! error (a stable `&'static str` message) onto its own error type.

use crate::{MirScalarLayout, MirType, MirValidationContext};
use radix_types::{IndexExpr, Type};

/// Literal shape dims of a tensor-typed MIR type (a flat literal is a rank-1
/// shape; a tuple is the dim list).
///
/// The error is a stable message string; callers map it onto their own error
/// type (the ABI resolver and the plan-mapping pass differ in error type but
/// share the message text).
pub(crate) fn tensor_literal_dims(
    ty: MirType,
    validation: &MirValidationContext<'_>,
) -> Result<Vec<u64>, &'static str> {
    let Type::Tensor(_, shape) = validation.types.get(ty.semantic_id()) else {
        return Err("kernel collection op requires tensor types");
    };
    match validation.types.get_index(*shape) {
        IndexExpr::Literal(count) => Ok(vec![*count]),
        IndexExpr::Tuple(dims) => {
            let mut out = Vec::with_capacity(dims.len());
            for &dim in dims {
                match validation.types.get_index(dim) {
                    IndexExpr::Literal(count) => out.push(*count),
                    _ => return Err("kernel collection op requires literal dimensions"),
                }
            }
            Ok(out)
        }
        _ => Err("kernel collection op requires static shape"),
    }
}

/// Whether a scalar layout is a kernel buffer element — the only scalar
/// layouts a kernel storage buffer can carry.
pub(crate) fn is_kernel_element_layout(layout: MirScalarLayout) -> bool {
    matches!(
        layout,
        MirScalarLayout::F32 | MirScalarLayout::I32 | MirScalarLayout::U32
    )
}
