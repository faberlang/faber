//! Compiler-owned tensor type-directed construction and reshape.
//!
//! Static spellings carry the complete result tensor in an explicit type
//! argument; dynamic receiver methods carry runtime shape values as arguments.

use super::{Type, TypeId, TypeTable, i64s_from_literal_index};

/// Resolved complete tensor type with a literal static shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteTensorType {
    pub elem: TypeId,
    pub shape: super::index::IndexId,
    pub dims: Vec<i64>,
}

pub const TENSOR_TYPE_ARG_INCOMPLETE_MSG: &str = "tensor type argument must be a complete tensor type such as tf32[2, 2] or tensor<numerus, [4]>";
pub const TENSOR_STRUCTA_FORMA_OLD_FORM_MSG: &str =
    "tensor.structa_forma was removed; use tensor.structa<CompleteTensorType>(data)";
pub const TENSOR_RECEIVER_CREATA_TYPE_ARGS_MSG: &str = "tensor receiver creata does not accept type arguments; use tensor.creata<CompleteTensorType>(fill)";
pub const TENSOR_STRUCTA_RENAMED_MSG: &str =
    "tensor receiver method structa is retired; use the data ↦ tensor conversion";
pub const TENSOR_FORMA_TYPE_ARG_ELEMENT_MSG: &str =
    "formata target element type must match the receiver element type";
pub const TENSOR_FORMA_VALUE_ARGS_MSG: &str =
    "formata does not accept value arguments; use grid.forma(shape) for runtime reshape";

fn normalized_type(ty: TypeId, types: &TypeTable) -> &Type {
    // Peel aliases and the declared access marker (`ref`/`mut`/`own`/`copy`)
    // so a copy-marked tensor local still normalizes to its tensor payload.
    let (_, payload) = types.peel_access_marker(ty);
    match types.get(payload) {
        Type::Alias(_, inner) => normalized_type(*inner, types),
        other => other,
    }
}

/// Extract element type and literal static dimensions from a complete tensor type.
#[must_use]
pub fn complete_tensor_type(ty: TypeId, types: &TypeTable) -> Option<CompleteTensorType> {
    let Type::Tensor(elem, shape) = normalized_type(ty, types) else {
        return None;
    };
    let dims = i64s_from_literal_index(*shape, types)?;
    Some(CompleteTensorType {
        elem: *elem,
        shape: *shape,
        dims,
    })
}

/// Whether `ty` is a tensor type (complete or not).
#[must_use]
pub fn is_tensor_type(ty: TypeId, types: &TypeTable) -> bool {
    matches!(normalized_type(ty, types), Type::Tensor(_, _))
}
