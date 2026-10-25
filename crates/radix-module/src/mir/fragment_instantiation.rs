//! Shim: fragment instantiation moved to `radix-mir-lower`; this copy hosts its
//! companion test so the test's module path and `#[path]` stay unchanged.

pub use radix_mir_lower::fragment_instantiation::*;

// The moved file's import scope, re-exported for its companion test.
pub use handroll::FxHashMap;
pub use radix_diagnostics::DiagnosticArg;
pub use radix_hir::DefId;
pub use radix_lexer::Span;
pub use radix_mir::device::MirDeviceFunctionMetadataMap;
pub use radix_mir::{
    MirCallee, MirDeviceRole, MirFunction, MirFunctionId, MirFunctionIndex, MirFunctionOrigin, MirKernelShaderStage,
    MirLayoutTable, MirMonomorphization, MirOperand, MirParamMode, MirPlace, MirPlaceBase, MirStatementKind,
    MirTerminatorKind, MirType, MirValueId, type_contains_param,
};
pub use radix_semantic::{
    Primitive, Type, TypeId, TypeTable, arg_value,
    passes::fragment_definition::{DeviceEffectKind, FragmentEffectTable},
};

#[cfg(test)]
#[path = "fragment_instantiation_test.rs"]
mod tests;
