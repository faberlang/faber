//! Shim: fragment composition moved to `radix-mir-lower`; this copy hosts its
//! companion tests so their module path and `#[path]` stay unchanged.

pub use radix_mir_lower::fragment_composition::*;

// The moved file's import scope, re-exported for its companion tests.
pub use handroll::{FxHashMap, FxHashSet};
pub use radix_diagnostics::DiagnosticArg;
pub use radix_hir::DefId;
pub use radix_lexer::{Interner, Span};
pub use radix_mir::device::MirDeviceFunctionMetadataMap;
pub use radix_mir::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirBlock, MirBlockId, MirCallee, MirConversion, MirDeviceRole,
    MirFunction, MirFunctionId, MirFunctionOrigin, MirIntrinsic, MirKeyValueOperand, MirLocal, MirLocalId,
    MirMonomorphization, MirNamedOperand, MirOperand, MirOptionChainLink, MirOptionOp, MirParam, MirParamMode,
    MirPlace, MirPlaceBase, MirProjection, MirRuntimeCall, MirStatement, MirStatementKind, MirTemp, MirTempId,
    MirTerminator, MirTerminatorKind, MirType, MirValue, MirValueId, MirValueKind,
};
pub use radix_semantic::arg_value;

#[cfg(test)]
#[path = "fragment_corpus_test.rs"]
mod corpus;
#[cfg(test)]
#[path = "fragment_composition_test.rs"]
mod tests;
