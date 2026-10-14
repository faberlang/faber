//! Shim host for the extracted HIR→MIR lowering (`radix-mir-lower`).
//!
//! Goal `radix-crate-dag`, unit C-M1: the lowering's production files moved to
//! the `radix-mir-lower` crate. This module keeps `crate::mir::lower::*`
//! resolving for the monolith.
//!
//! It also hosts the lowering's companion tests, which stay in `radix-module`
//! because they reach the driver (which sits above the extracted crate). Those
//! tests were written against the module's whole scope — their `use super::*`
//! pulled in the lowering's own imports alongside its items — so the
//! re-export block below restores that scope. It is test-only: production code
//! reaches the lowering through the glob on line 13.

pub use radix_mir_lower::lower::*;

// The moved module's imported scope, re-exported for its companion tests.
#[cfg(test)]
pub use radix_hir::visit::{HirVisitor, walk_expr, walk_program};
#[cfg(test)]
pub use radix_hir::{
    DefId, HirArrayElement, HirBinOp, HirBlock, HirCape, HirCasuArm, HirConst, HirConversioTarget, HirExpression,
    HirExpressionKind, HirField, HirFunction, HirItem, HirItemKind, HirIteraMode, HirLiteral, HirLocal, HirMethod,
    HirNonNullKind, HirObjectField, HirObjectKey, HirOptionalChainKind, HirParamMode, HirPattern, HirRangeKind,
    HirScribeKind, HirStatement, HirStatementKind, HirStruct, HirTypeParam, HirUnOp,
};
#[cfg(test)]
pub use radix_lexer::{Interner, Span, Symbol};
#[cfg(test)]
pub use radix_mir::device::{MirDeviceFunctionMetadata, MirDeviceFunctionMetadataMap, is_public_entry};
#[cfg(test)]
pub use radix_mir::lane_op::LaneOp;
#[cfg(test)]
pub use radix_mir::validate::PatchProgramError;
#[cfg(test)]
pub use radix_mir::vector_expand::{expand_vector_ops, numeric_zero_constant};
#[cfg(test)]
pub use radix_mir::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBinOp, MirBlock, MirBlockId,
    MirBuiltinEdgePolicy, MirCallee, MirCaptureMode, MirClosureCallee, MirClosureCapture, MirClosureEnvironment,
    MirClosureEnvironmentId, MirClosureValue, MirConstant, MirConversion, MirConversionFlavor, MirDeviceRole,
    MirDiagnosticKind, MirEdgePolicy, MirFilumCoordinate, MirFilumForm, MirFilumReduceOp, MirFunction, MirFunctionId,
    MirFunctionOrigin, MirFunctionVisibility, MirGenericBindings, MirIntrinsic, MirKernelShaderStage,
    MirKeyValueOperand, MirLaneOp, MirLayoutTable, MirLiftOp, MirLocal as MirLocalDecl, MirLocalId,
    MirMonomorphization, MirNamedOperand, MirOperand, MirOptionChainLink, MirOptionOp, MirOptionUnwrapMode, MirParam,
    MirParamMode, MirPlace, MirPlaceBase, MirProgram, MirProjection, MirProvider, MirProviderKind, MirReduceOp,
    MirRuntimeCall, MirShapeBinding, MirShapeCheck, MirShapeExpr, MirShapeRelation, MirStatement, MirStatementKind,
    MirStructFieldDefaults, MirSwitchCase, MirSyntheticFunctionIdentity, MirTemp, MirTempId, MirTerminator,
    MirTerminatorKind, MirType, MirTypeCheckOp, MirUnOp, MirValidationContext, MirValue, MirValueId, MirValueKind,
    MirVectorOp, ValidatedMir, dump_program, instantiate_existing_generic_type, type_contains_param,
};
#[cfg(test)]
pub use radix_mir_lower::lower::expr::HirExpressionLoweringVisitor;
#[cfg(test)]
pub use radix_semantic::analyzed_application::AnalyzedModuleApplicationView;
#[cfg(test)]
pub use radix_semantic::cli::{CliDefault, CliOperand, CliOption, CliType};
#[cfg(test)]
pub use radix_semantic::file_interface::InterfaceConstValue;
#[cfg(test)]
pub use radix_semantic::{
    ConversionPair, IndexExpr, IndexId, NumericWidth, Primitive, RuntimeExtentId, RuntimeSizeBinding, ShapeRelation,
    ShapeRelationBoundary, Type, TypeId, TypeTable,
};
#[cfg(test)]
pub use radix_types::{MirCollectionOp, MirKernelBuiltin, SemanticMutability};
#[cfg(test)]
pub use rustc_hash::{FxHashMap, FxHashSet};
#[cfg(test)]
pub use std::sync::{Arc, Mutex};

// Submodule shims: one per moved module that owns a companion test. Their tests
// stay here, and their `super::` must keep naming the same module path it named
// before the move.
#[cfg(test)]
mod expr;
#[cfg(test)]
mod monomorphize;
#[cfg(test)]
mod package_instantiate;
#[cfg(test)]
mod place;

#[cfg(test)]
#[path = "lower/bounded_octeti_run_test.rs"]
mod bounded_octeti_run_tests;
#[cfg(test)]
#[path = "lower/bounded_textus_run_test.rs"]
mod bounded_textus_run_tests;
#[cfg(test)]
#[path = "lower/conversion_check_test.rs"]
mod conversion_check_tests;
#[cfg(test)]
#[path = "lower/display_tokens_test.rs"]
mod display_tokens_tests;
#[cfg(test)]
#[path = "lower/empty_dense_test.rs"]
mod empty_dense_tests;
#[cfg(test)]
#[path = "lower/entry_args_test.rs"]
mod entry_args_tests;
#[cfg(test)]
#[path = "lower/explicit_generic_constructor_test.rs"]
mod explicit_generic_constructor_tests;
#[cfg(test)]
#[path = "lower/first_match_test.rs"]
mod first_match_tests;
#[cfg(test)]
#[path = "lower_glyph_test.rs"]
mod glyph_tests;
#[cfg(test)]
#[path = "lower/map_size_guard_test.rs"]
mod map_size_guard_tests;
#[cfg(test)]
#[path = "lower/octeti_itera_test.rs"]
mod octeti_itera_tests;
#[cfg(test)]
#[path = "lower/octeti_tensor_run_test.rs"]
mod octeti_tensor_run_tests;
#[cfg(test)]
#[path = "optional_field_unwrap_test.rs"]
mod optional_field_unwrap_tests;
#[cfg(test)]
#[path = "lower/own_class_field_test.rs"]
mod own_class_field_tests;
#[cfg(test)]
#[path = "own_textus_test.rs"]
mod own_textus_tests;
#[cfg(test)]
#[path = "lower/runtime_shape_test.rs"]
mod runtime_shape_tests;
#[cfg(test)]
#[path = "lower/slot_pin_run_test.rs"]
mod slot_pin_run_tests;
#[cfg(test)]
#[path = "lower_test.rs"]
mod tests;
