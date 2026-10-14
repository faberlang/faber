//! High-level intermediate representation for semantically resolved Faber.
//!
//! HIR is the compiler boundary where parsed syntax becomes a stable semantic
//! model. Lowering consumes the AST after name resolution, carries `DefId`
//! references for definitions and bindings, preserves source spans for later
//! diagnostics, and normalizes constructs that later phases should not need to
//! rediscover from grammar shape.
//!
//! This layer is still intentionally high-level. It records declarations,
//! statements, expressions, handler structure, collection forms, and target
//! type references, but it does not prove type correctness, choose MIR-level
//! control-flow shape, or decide backend emission details. Typecheck, MIR, and
//! codegen should be able to trust HIR identity and span invariants without
//! treating HIR as already fully typed.
//!
//! INVARIANTS
//! ==========
//! - `DefId` identifies declarations and bindings after resolution.
//! - `HirId` identifies nodes that later analyses may annotate.
//! - Spans remain attached to HIR nodes even when syntax is normalized.
//! - Optional `TypeId` fields mean "not established yet" unless the field-level
//!   contract says the type was already required by the source form.
//! - Top-level executable source is represented as `HirModule::entry`, the
//!   executable block of one source module, separate from item declarations.
//!   It is not a package or program root.
//!
//! PHASE BOUNDARY
//! ==============
//! HIR lowering owns syntax-to-semantics normalization. Typecheck owns inferred
//! types and semantic validity. MIR/codegen own backend-shaped control flow,
//! storage layout, and target-specific lowering.

#[cfg(feature = "hir-fhir")]
pub mod artifact;
mod lower;
mod nodes;
#[cfg(feature = "hir-fhir")]
pub mod package;
pub(crate) mod presentation_witness;
#[cfg(feature = "hir-fhir")]
pub mod serialize;
pub mod visit;

pub use lower::{LowerError, LowerRequest, lower};
pub use nodes::{
    DefId, HirAnnotation, HirAnnotationField, HirAnnotationValue, HirArrayElement, HirAvailability, HirBinOp, HirBlock,
    HirBlockEnd, HirBreakableKind, HirCallArg, HirCape, HirCasuArm, HirConst, HirConversioRoute, HirConversioTarget,
    HirCustodi, HirCustodiClause, HirEnum, HirExpression, HirExpressionKind, HirField, HirFunction, HirId, HirImport,
    HirImportItem, HirIncDec, HirIncDecOp, HirInterface, HirInterfaceMethod, HirItem, HirItemKind, HirIteraMode,
    HirLiteral, HirLocal, HirMethod, HirModule, HirModuleEnd, HirNonNullKind, HirObjectField, HirObjectKey,
    HirOptionalChainKind, HirOrdoRouteMember, HirOrdoRow, HirOwnerRef, HirParam, HirParamMode, HirPattern,
    HirPresentation, HirRangeKind, HirReceiver, HirRefKind, HirSchema, HirSchemaColumn, HirScribeKind,
    HirSourceAnchorId, HirStatement, HirStatementKind, HirStatic, HirStoreKind, HirStoreSite, HirStruct,
    HirTestMetadata, HirTestModifier, HirTrivia, HirTriviaAttachment, HirTriviaOwner, HirTriviaOwnerKind, HirTypeAlias,
    HirTypeParam, HirTypeParamConstraint, HirUnOp, HirVariant, HirVariantField, LibraryBinding, LibraryIdentity,
    LibraryItem, LibraryItemKind, LibraryProvider, LibraryRegistry, NucleumRole,
};

pub use radix_semantic::LoweredProgram;
