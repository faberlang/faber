//! Shim: the `monomorphize` lowering module moved to `radix-mir-lower`; this
//! copy hosts its companion test so the test's module path and `#[path]` stay
//! unchanged.

pub use radix_mir_lower::lower::monomorphize::*;

// The moved file began with `use super::*;`; its companion test relied on that
// scope, so the shim re-exports it.
pub use super::*;
pub use radix_mir::generic::{
    MirShapeBindings, bind_generic_type_witness_with_shapes, canonical_index, function_contains_shape_size,
    instantiate_existing_generic_type, instantiate_generic_type_with_shapes, substitute_shape_size_constants,
    substitute_tensor_from_flat_shape_size_constants,
};
pub use radix_mir::ty::index_projection_result_ty;
pub use radix_mir::{
    MirBuiltinEdgePolicy, MirCallee, MirEdgePolicy, MirExternalFunctionIdentity, MirFunction, MirFunctionId,
    MirFunctionIndex, MirGenericBindings, MirIntrinsic, MirMonomorphization, MirMonomorphizationKey, MirOperand,
    MirPlace, MirPlaceBase, MirProgram, MirProjection, MirStatementKind, MirTerminatorKind, MirType,
    type_contains_param,
};
pub use radix_semantic::IndexId;

#[cfg(test)]
#[path = "monomorphize_test.rs"]
mod tests;
