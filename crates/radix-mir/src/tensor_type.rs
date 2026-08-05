//! Shared tensor-type literal-dimension and kernel-element-layout helpers.
//!
//! The ABI contract resolver and the plan-mapping pass both need the literal
//! dims of a tensor-typed MIR type and the kernel-scalar element-layout
//! predicate. This is the single implementation; each caller maps the shared
//! error (a stable `&'static str` message) onto its own error type.

use crate::{MirFunction, MirLayoutKind, MirScalarLayout, MirType, MirValidationContext};
use radix_types::{IndexExpr, NumericWidth, Primitive, Type};

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

/// Whether the kernel/subchain body's operator graph is f32-safe — the typed
/// f64→f32 kernel-scalar narrowing precondition (D-6a, residual-doc option
/// (b)).
///
/// The graph is f32-safe when the body carries at least one tensor-typed
/// value and every tensor element type resolves to F32. For such graphs an
/// f64 scalar at the device boundary (a lane's scalar-return local or a
/// companion's f64 upstream seed) narrows to a 1-element F32 buffer by the
/// explicit typed rule — never an implicit truncation. A body with no tensor
/// graph at all (a pure-scalar f64 kernel) or any non-F32 tensor element
/// (e.g. an f64 tensor graph) keeps the existing fail-closed typed
/// diagnostics.
///
/// The MLP shape is exactly the f32-safe case: `mlp_loss` computes over
/// `tensor<f32, [4,4]>` values and only the scalar loss/seed is f64, so
/// narrowing the scalar at the device boundary is a lossless f32
/// materialization of the f32-safe graph (the CPU oracle keeps the f64 loss).
pub(crate) fn operator_graph_is_f32_safe(
    function: &MirFunction,
    validation: &MirValidationContext<'_>,
) -> bool {
    let mut saw_tensor = false;
    for ty in function_value_types(function) {
        if let Ok(layout) = validation.layouts.layout_for_type(ty) {
            if let MirLayoutKind::DeviceView(view) = layout.kind {
                saw_tensor = true;
                if view.element != MirScalarLayout::F32 {
                    return false;
                }
            }
        }
    }
    saw_tensor
}

/// Every value type carried by a function's body (params, locals, temps, and
/// the return type) — the operator-graph surface the f32-safe predicate scans.
fn function_value_types(function: &MirFunction) -> impl Iterator<Item = MirType> + '_ {
    function
        .params
        .iter()
        .map(|param| param.ty)
        .chain(function.locals.iter().map(|local| local.ty))
        .chain(function.temps.iter().map(|temp| temp.ty))
        .chain(std::iter::once(function.return_ty))
}

/// The F32 scalar type a narrowed f64 device-boundary scalar binds as.
///
/// Returns `None` only when the frozen type table lacks an interned
/// `fractus<f32>` (an f32-safe graph always carries one — its tensor element
/// types are F32 — so callers can map `None` onto their own fail-closed
/// diagnostic).
pub(crate) fn f32_scalar_type(validation: &MirValidationContext<'_>) -> Option<MirType> {
    validation
        .types
        .find_sized_numeric(Primitive::Fractus, NumericWidth::F32)
        .map(MirType::semantic)
}
