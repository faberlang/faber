//! HIR-to-MIR lowering orchestration for the compiler-developer MIR surface.
//!
//! This module is the boundary between a fully analyzed HIR unit and the
//! execution-shaped MIR model used by MIR dumps, validation, and temporary MIR
//! probes. It deliberately depends on [`AnalyzedModuleApplicationView`] rather than raw HIR so
//! lowering can preserve semantic identity, resolved definitions, inferred
//! types, entry metadata, and the interner without rebuilding semantic context.
//!
//! ERROR STRATEGY
//! ==============
//! MIR lowering is fail-closed infrastructure. Unsupported HIR surfaces, missing
//! semantic types, sealed-block misuse, and post-lowering validation failures are
//! reported as [`MirError`] values; they are not guessed around in order to emit
//! partial MIR. This keeps developer-facing MIR inspection honest while the MIR
//! subset grows toward the full language.
//!
//! ORCHESTRATION
//! =============
//! [`MirLowerer`] owns whole-program order: collect context maps, lower
//! supported top-level items, synthesize the primitive entry function when
//! allowed, and hand the complete program to validation. [`FunctionBuilder`]
//! owns one function body at a time, including local bindings, temporaries,
//! open basic blocks, loop/handler stacks, and value numbering.
//!
//! INVARIANTS
//! ==========
//! - Every emitted block is finalized with exactly one terminator before the
//!   program reaches validation.
//! - MIR types are copied from semantic `TypeId`s; missing HIR type information
//!   is a lowering error, not a codegen fallback.
//! - Unsupported features remain explicit diagnostics so later MIR work can add
//!   support without inheriting silent behavior.

use handroll::{FxHashMap, FxHashSet};
use radix_hir::visit::{HirVisitor, walk_expr, walk_program};
use radix_hir::{
    DefId, HirArrayElement, HirBinOp, HirBlock, HirCape, HirCasuArm, HirConst, HirConversioTarget, HirExpression,
    HirExpressionKind, HirField, HirFunction, HirItem, HirItemKind, HirIteraMode, HirLiteral, HirLocal, HirMethod,
    HirNonNullKind, HirObjectField, HirObjectKey, HirOptionalChainKind, HirParamMode, HirPattern, HirRangeKind,
    HirScribeKind, HirStatement, HirStatementKind, HirStruct, HirTypeParam, HirUnOp,
};
use radix_lexer::{Interner, Span, Symbol};
use radix_mir::device::{MirDeviceFunctionMetadata, MirDeviceFunctionMetadataMap, is_public_entry};
use radix_mir::lane_op::LaneOp;
use radix_mir::validate::PatchProgramError;
use radix_mir::vector_expand::{expand_vector_ops, numeric_zero_constant};
use radix_mir::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBinOp, MirBlock, MirBlockId,
    MirBuiltinEdgePolicy, MirCallee, MirCaptureMode, MirClosureCallee, MirClosureCapture, MirClosureEnvironment,
    MirClosureEnvironmentId, MirClosureValue, MirConstant, MirConversion, MirConversionFlavor, MirDeviceRole,
    MirDiagnosticKind, MirEdgePolicy, MirFilumCoordinate, MirFilumForm, MirFilumGroupKey, MirFilumGroupOp,
    MirFilumReduceOp, MirFunction, MirFunctionId, MirFunctionOrigin, MirFunctionVisibility, MirGenericBindings,
    MirIntrinsic, MirKernelShaderStage, MirKeyValueOperand, MirLaneOp, MirLayoutTable, MirLiftOp,
    MirLocal as MirLocalDecl, MirLocalId, MirMonomorphization, MirNamedOperand, MirOperand, MirOptionChainLink,
    MirOptionOp, MirOptionUnwrapMode, MirParam, MirParamMode, MirPlace, MirPlaceBase, MirProgram, MirProjection,
    MirProvider, MirProviderKind, MirReduceOp, MirRuntimeCall, MirScanGroupAxis, MirScanGroupOp, MirScanOp,
    MirScanTotal, MirShapeBinding, MirShapeCheck, MirShapeExpr, MirShapeRelation, MirStatement, MirStatementKind,
    MirStructFieldDefaults, MirSwitchCase, MirSyntheticFunctionIdentity, MirTemp, MirTempId, MirTerminator,
    MirTerminatorKind, MirType, MirTypeCheckOp, MirUnOp, MirValidationContext, MirValue, MirValueId, MirValueKind,
    MirVectorOp, ValidatedMir, dump_program, instantiate_existing_generic_type, terminator_successors,
    type_contains_param,
};
use radix_semantic::analyzed_application::AnalyzedModuleApplicationView;
use radix_semantic::cli::{CliDefault, CliOperand, CliOption, CliType};
use radix_semantic::file_interface::InterfaceConstValue;
use radix_semantic::{
    ConversionPair, IndexExpr, IndexId, NumericWidth, Primitive, RuntimeExtentId, RuntimeSizeBinding, ShapeRelation,
    ShapeRelationBoundary, Type, TypeId, TypeTable,
};
use radix_types::{MirCollectionOp, MirKernelBuiltin, SemanticMutability};
use register_lift::LaneSource;
use std::sync::{Arc, Mutex};

mod aggregate;
mod async_surface;
mod callable;
mod collection_higher_order;

mod context;
mod control;
mod conversio_route;
pub mod expr;
mod fractus_potentia;
mod item;
pub mod lane_form;
pub mod monomorphize;
pub mod package_instantiate;
pub mod place;
mod register_lift;
pub mod runtime;
mod scan;
mod scan_device;
mod stmt;
mod tensor_lift;
mod union_dispatch;

use context::{LoweringContextMaps, function_param_map, struct_field_map};
use expr::HirExpressionLoweringVisitor;
use item::ItemLoweringPass;
pub use package_instantiate::{
    ImportedDeviceIndex, ImportedDeviceRegistration, ImportedDeviceRoute, PackageStructMetadata,
    instantiate_merged_generic_calls, instantiate_merged_generic_calls_with_devices,
    instantiate_merged_generic_calls_with_devices_and_metadata,
    instantiate_merged_generic_calls_with_tensor_edge_policy,
};
use runtime::SyntheticHelperKey;

/// Developer-facing MIR lowering diagnostic.
///
/// MIR lowering errors describe gaps between the typed HIR surface and the MIR
/// subset currently implemented. They carry the primary span and, for
/// composition diagnostics, an optional secondary definition span. The caller
/// is a compiler inspection/probe surface, not the primary user diagnostic
/// renderer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirError {
    pub message: String,
    pub issue: String,
    pub span: Span,
    pub definition_span: Option<Span>,
}

impl From<super::fragment_instantiation::FragmentInstantiationError> for MirError {
    fn from(error: super::fragment_instantiation::FragmentInstantiationError) -> Self {
        Self {
            message: error.message(),
            issue: error.issue.to_owned(),
            span: error.span,
            definition_span: error.definition_span,
        }
    }
}

/// Expand the deferred vector ops of one executable function (every shape
/// size already literal), recording any site that cannot expand. Every place
/// a function becomes executable calls this, so a surviving op is a producer
/// bug that validation rejects by name.
pub fn expand_deferred_vector_ops(types: &TypeTable, function: &mut MirFunction, errors: &mut Vec<MirError>) {
    errors.extend(
        expand_vector_ops(types, function)
            .into_iter()
            .map(|error| MirError {
                message: error.message,
                issue: error.issue,
                span: error.span,
                definition_span: None,
            }),
    );
}

impl MirError {
    fn unsupported(span: Span, what: impl Into<String>) -> Self {
        let what = what.into();
        Self {
            message: format!("unsupported MIR lowering: {what}"),
            issue: mir_lower_issue("unsupported_mir_lowering", &what),
            span,
            definition_span: None,
        }
    }

    fn missing_type(span: Span, what: impl Into<String>) -> Self {
        let what = what.into();
        Self {
            message: format!("missing type information for MIR lowering: {what}"),
            issue: mir_lower_issue("missing_type_information_for_mir_lowering", &what),
            span,
            definition_span: None,
        }
    }

    fn validation(span: Span, what: impl Into<String>) -> Self {
        let what = what.into();
        Self {
            message: format!("invalid MIR: {what}"),
            issue: mir_lower_issue("invalid_mir", &what),
            span,
            definition_span: None,
        }
    }

    fn missing_generic_air_template(span: Span, source: u32) -> Self {
        Self {
            message: format!("generic AIR definition #{source} has no analyzed template"),
            issue: "generic_air_template_missing".to_owned(),
            span,
            definition_span: None,
        }
    }
}

fn mir_lower_issue(prefix: &str, detail: &str) -> String {
    let mut issue = prefix.to_owned();
    let mut separated = false;
    for ch in detail.chars() {
        if ch.is_ascii_alphanumeric() {
            if !separated {
                issue.push('_');
            }
            issue.push(ch.to_ascii_lowercase());
            separated = true;
        } else if !issue.ends_with('_') {
            issue.push('_');
            separated = true;
        }
    }
    issue.trim_end_matches('_').to_owned()
}

/// MIR output plus the proving validated-MIR token and auxiliary lowering data.
pub struct LoweredMirUnit<'a> {
    /// Complete symbol table after lowering-added runtime and diagnostic symbols.
    pub interner: Interner,
    pub monomorphizations: Vec<MirMonomorphization>,
    pub closure_environments: Vec<MirClosureEnvironment>,
    /// Explicit role/origin/visibility facts for source-derived and source-less
    /// device functions. Consumers must use this carrier rather than infer
    /// private status from a missing source definition or compute stage alone.
    pub device_function_metadata: MirDeviceFunctionMetadataMap,
    /// Generic source bodies removed from [`Self::validated`] after unit
    /// monomorphization. Package linking keeps them so a later unit can
    /// instantiate a shape the defining unit never called.
    pub generic_templates: Vec<MirFunction>,
    /// Declaration-order generic parameters for [`Self::generic_templates`],
    /// keyed by the template's `function.source` (rewritten to a synthetic
    /// def at package link). `is_shape` is `magnitudo`; otherwise `typus`.
    pub generic_params: Vec<(DefId, Vec<MirGenericParam>)>,
    /// Portable tensor edge-policy provider facts and generic limes sites.
    /// This contains source identities and ranks, never unit-local `TypeId`s,
    /// so package linking can carry it across independently allocated type
    /// arenas and resolve callbacks after function IDs are merged.
    pub tensor_edge_policy_metadata: MirTensorEdgePolicyMetadata,
    /// Validated MIR token and sole owner of the program that passed
    /// [`validate_program`] against the enclosed context.
    pub validated: ValidatedMir<'a>,
    /// Owned companion-relation carrier (S3-A2): primal → companion device
    /// materialization facts, populated by the driver's
    /// `apply_air_backward_bundle` (driver-side today; part of the AIR
    /// backward bundle the driver applies around the lowering)
    /// instead of being discarded after validation. The faber device-program
    /// constructor consumes it to lower generated companions into kernels.
    pub companions: radix_mir::device::MirCompanionMap,
    /// Imported-device routes transferred from this unit's lowering context
    /// (KPC-WIRE `done_when` (c)), keyed by import-item `DefId`: the v2 device
    /// role, launch visibility, provider-route identity, and explicit body
    /// channel collected at the import items. Package linking joins each
    /// route to the merged synthetic template `DefId` and instantiates
    /// against the resulting index; the single-unit path leaves the map
    /// untouched.
    pub imported_device_routes: FxHashMap<DefId, Vec<ImportedDeviceRoute>>,
    /// Concrete imported-device instances registered for this unit by
    /// package-link instantiation (KPC-WIRE `done_when` (d)): one entry per
    /// [`MirMonomorphizationKey`], each proving the instance was admitted
    /// under its route's role + body channel rather than a role bit alone.
    /// Empty until package linking runs the device-aware instantiation.
    pub imported_device_registrations: Vec<ImportedDeviceRegistration>,
    /// The display words of the file this unit was lowered from. Every
    /// printing function of the unit carries them in the validation table;
    /// package linking also reads them for [`Self::generic_templates`], which
    /// leave the program before the table is filled.
    pub display_tokens: radix_runtime_contract::display_tokens::DisplayTokens,
}

impl LoweredMirUnit<'_> {
    /// Consume this carrier, patch its unvalidated program, and return a
    /// replacement carrier only after validation succeeds.
    ///
    /// # Errors
    /// Returns only the patch or validation error; the consumed carrier and
    /// proof token are not recoverable on rejection.
    pub fn patch_program<E>(
        self,
        patch: impl FnOnce(&mut MirProgram) -> Result<(), E>,
    ) -> Result<Self, PatchProgramError<E>> {
        let Self {
            interner,
            monomorphizations,
            closure_environments,
            device_function_metadata,
            generic_templates,
            generic_params,
            tensor_edge_policy_metadata,
            validated,
            companions,
            imported_device_routes,
            imported_device_registrations,
            display_tokens,
        } = self;
        let validated = validated.patch_program(patch)?;
        Ok(Self {
            interner,
            monomorphizations,
            closure_environments,
            device_function_metadata,
            generic_templates,
            generic_params,
            tensor_edge_policy_metadata,
            validated,
            companions,
            imported_device_routes,
            imported_device_registrations,
            display_tokens,
        })
    }
}

impl std::fmt::Debug for LoweredMirUnit<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LoweredMirUnit")
            .field("validated", &self.validated)
            .field("monomorphizations", &self.monomorphizations)
            .field("closure_environments", &self.closure_environments)
            .field("device_function_metadata", &self.device_function_metadata)
            .field("generic_templates", &self.generic_templates)
            .field("generic_params", &self.generic_params)
            .field("tensor_edge_policy_metadata", &self.tensor_edge_policy_metadata)
            .field("companions", &self.companions)
            .field("imported_device_registrations", &self.imported_device_registrations)
            .finish_non_exhaustive()
    }
}

/// One generic parameter in source declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirGenericParam {
    pub name: Symbol,
    pub is_shape: bool,
    /// Stable formal-carrier witness used to key dynamic shape parameters.
    /// Runtime actual extents stay on call operands and never specialize the
    /// body by their value or source origin.
    pub runtime_witness: Option<IndexId>,
}

/// Portable selector for one semantically checked tensor edge-policy
/// provider. Custom callbacks remain source `DefId`s until package linking
/// has assigned final MIR function IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirTensorEdgePolicySelector {
    /// Compiler-owned shape-aware policy.
    Builtin(MirBuiltinEdgePolicy),
    /// Checked, zero-field custom policy and its ordinary callback methods.
    Custom {
        policy_def: DefId,
        remap: DefId,
        transform: Option<DefId>,
    },
}

/// Arena-independent identity for a concrete policy or tensor element type.
/// Only closed, fully resolved forms are admitted; unsupported symbolic forms
/// stay out of the provider table and fail closed at package linking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirTensorEdgeTypeIdentity {
    Primitive(Primitive),
    BuiltinPolicy(radix_types::BuiltinTensorEdgePolicy),
    Struct(DefId),
    Enum(DefId),
    Interface(DefId),
    Applied(Box<Self>, Vec<MirTensorEdgeAppliedArg>),
    Array(Box<Self>),
    Option(Box<Self>),
    Ref(SemanticMutability, Box<Self>),
    Tensor(Box<Self>, MirTensorEdgeShapeIdentity),
    Vector(Box<Self>, MirTensorEdgeShapeIdentity),
    Matrix(Box<Self>, MirTensorEdgeShapeIdentity),
    Sparsa(Box<Self>, MirTensorEdgeShapeIdentity),
    SizedNumeric(Primitive, NumericWidth),
    ModularWord(NumericWidth),
    SaturatingWord(NumericWidth),
    PolicyFloat(radix_types::FloatPolicy, NumericWidth),
    SizedInstans(radix_types::InstansPrecision),
    Tuple(Vec<Self>),
    Union(Vec<Self>),
    Intersection(Vec<Self>),
    Record(Vec<(String, Self)>),
}

/// A statically closed shape inside a concrete tensor-valued element or
/// policy type argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirTensorEdgeShapeIdentity {
    Literal(u64),
    Tuple(Vec<Self>),
}

/// An applied type argument, with size terms represented by closed values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MirTensorEdgeAppliedArg {
    Type(MirTensorEdgeTypeIdentity),
    Size(MirTensorEdgeShapeIdentity),
}

/// A checked policy provider available to concrete generic specializations.
/// The callback signatures were checked against the element type during
/// semantic discharge; package MIR rechecks the final method MIDs and tensor
/// rank after substitution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirTensorEdgePolicyProvider {
    /// Concrete selector identity (builtin tag or custom source nominal).
    pub selector: MirTensorEdgePolicySelector,
    /// Exact custom policy type. Builtins are universal and use `None`.
    pub policy_type: Option<MirTensorEdgeTypeIdentity>,
    /// Exact tensor element type accepted by a custom transform. Builtins
    /// are universal and use `None`.
    pub element_type: Option<MirTensorEdgeTypeIdentity>,
    /// Statically known tensor rank accepted by the semantic policy check.
    pub rank: usize,
}

/// One symbolic `limes<P>()` site in a generic MIR template.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MirTensorLimesTemplate {
    /// Template source identity, rewritten to the package namespace at link.
    pub source: DefId,
    /// Destination temporary used to locate this runtime call in a clone.
    pub destination: MirTempId,
    /// Zero-based index among typus arguments in the monomorphization key.
    pub policy_type_argument: usize,
    /// Statically known tensor rank.
    pub rank: usize,
}

/// Cross-unit facts needed to resolve deferred generic limes sites.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MirTensorEdgePolicyMetadata {
    /// All semantically checked concrete providers reachable in the package.
    pub providers: Vec<MirTensorEdgePolicyProvider>,
    /// Symbolic limes sites retained for generic templates.
    pub generic_limes: Vec<MirTensorLimesTemplate>,
}

fn portable_tensor_edge_policy_providers(
    policies: &radix_hir::TensorEdgePolicyTable,
    types: &TypeTable,
    interner: &Interner,
) -> Vec<MirTensorEdgePolicyProvider> {
    let mut providers = Vec::new();
    for provider in &policies.provider_impls {
        let (selector, policy_type, element_type) = match provider.policy {
            radix_hir::ResolvedTensorEdgePolicy::Builtin(policy) => (
                MirTensorEdgePolicySelector::Builtin(match policy {
                    radix_types::BuiltinTensorEdgePolicy::Clamp => MirBuiltinEdgePolicy::Clamp,
                    radix_types::BuiltinTensorEdgePolicy::Reflect => MirBuiltinEdgePolicy::Reflect,
                    radix_types::BuiltinTensorEdgePolicy::Wrap => MirBuiltinEdgePolicy::Wrap,
                }),
                None,
                None,
            ),
            radix_hir::ResolvedTensorEdgePolicy::Custom { policy_def, remap, transform } => {
                let (Some(policy_type), Some(element_type)) = (
                    portable_tensor_edge_type_identity(types, interner, provider.policy_type),
                    portable_tensor_edge_type_identity(types, interner, provider.element_type),
                ) else {
                    continue;
                };
                (
                    MirTensorEdgePolicySelector::Custom { policy_def, remap, transform },
                    Some(policy_type),
                    Some(element_type),
                )
            }
            radix_hir::ResolvedTensorEdgePolicy::Deferred { .. } => continue,
        };
        let portable = MirTensorEdgePolicyProvider { selector, policy_type, element_type, rank: provider.rank };
        if !providers.contains(&portable) {
            providers.push(portable);
        }
    }
    providers
}

fn portable_tensor_edge_type_identity(
    types: &TypeTable,
    interner: &Interner,
    ty: TypeId,
) -> Option<MirTensorEdgeTypeIdentity> {
    use radix_types::AppliedArg;

    let identity = match types.get(ty) {
        Type::Primitive(primitive) => MirTensorEdgeTypeIdentity::Primitive(*primitive),
        Type::TensorEdgePolicy(policy) => MirTensorEdgeTypeIdentity::BuiltinPolicy(*policy),
        Type::Struct(definition) => MirTensorEdgeTypeIdentity::Struct(*definition),
        Type::Enum(definition) => MirTensorEdgeTypeIdentity::Enum(*definition),
        Type::Interface(definition) => MirTensorEdgeTypeIdentity::Interface(*definition),
        Type::Alias(_, inner) => return portable_tensor_edge_type_identity(types, interner, *inner),
        Type::Applied(base, args) => MirTensorEdgeTypeIdentity::Applied(
            Box::new(portable_tensor_edge_type_identity(types, interner, *base)?),
            args.iter()
                .map(|argument| match argument {
                    AppliedArg::Type(ty) => Some(MirTensorEdgeAppliedArg::Type(portable_tensor_edge_type_identity(
                        types, interner, *ty,
                    )?)),
                    AppliedArg::Size(shape) => Some(MirTensorEdgeAppliedArg::Size(
                        portable_tensor_edge_shape_identity(types, *shape)?,
                    )),
                })
                .collect::<Option<Vec<_>>>()?,
        ),
        Type::Array(element) => {
            MirTensorEdgeTypeIdentity::Array(Box::new(portable_tensor_edge_type_identity(types, interner, *element)?))
        }
        Type::Option(element) => {
            MirTensorEdgeTypeIdentity::Option(Box::new(portable_tensor_edge_type_identity(types, interner, *element)?))
        }
        Type::Ref(mutability, referent) => MirTensorEdgeTypeIdentity::Ref(
            *mutability,
            Box::new(portable_tensor_edge_type_identity(types, interner, *referent)?),
        ),
        Type::Tensor(element, shape) => MirTensorEdgeTypeIdentity::Tensor(
            Box::new(portable_tensor_edge_type_identity(types, interner, *element)?),
            portable_tensor_edge_shape_identity(types, *shape)?,
        ),
        Type::Vector(element, shape) => MirTensorEdgeTypeIdentity::Vector(
            Box::new(portable_tensor_edge_type_identity(types, interner, *element)?),
            portable_tensor_edge_shape_identity(types, *shape)?,
        ),
        Type::Matrix(element, shape) => MirTensorEdgeTypeIdentity::Matrix(
            Box::new(portable_tensor_edge_type_identity(types, interner, *element)?),
            portable_tensor_edge_shape_identity(types, *shape)?,
        ),
        Type::Sparsa(element, shape) => MirTensorEdgeTypeIdentity::Sparsa(
            Box::new(portable_tensor_edge_type_identity(types, interner, *element)?),
            portable_tensor_edge_shape_identity(types, *shape)?,
        ),
        Type::SizedNumeric(primitive, width) => MirTensorEdgeTypeIdentity::SizedNumeric(*primitive, *width),
        Type::ModularWord(width) => MirTensorEdgeTypeIdentity::ModularWord(*width),
        Type::SaturatingWord(width) => MirTensorEdgeTypeIdentity::SaturatingWord(*width),
        Type::PolicyFloat(policy, width) => MirTensorEdgeTypeIdentity::PolicyFloat(*policy, *width),
        Type::SizedInstans(precision) => MirTensorEdgeTypeIdentity::SizedInstans(*precision),
        Type::Tuple(items) => MirTensorEdgeTypeIdentity::Tuple(
            items
                .iter()
                .map(|item| portable_tensor_edge_type_identity(types, interner, *item))
                .collect::<Option<Vec<_>>>()?,
        ),
        Type::Union(items) => MirTensorEdgeTypeIdentity::Union(
            items
                .iter()
                .map(|item| portable_tensor_edge_type_identity(types, interner, *item))
                .collect::<Option<Vec<_>>>()?,
        ),
        Type::Intersection(items) => MirTensorEdgeTypeIdentity::Intersection(
            items
                .iter()
                .map(|item| portable_tensor_edge_type_identity(types, interner, *item))
                .collect::<Option<Vec<_>>>()?,
        ),
        Type::Record(fields) => portable_tensor_edge_record_identity(types, interner, fields)?,
        // A ranged cell has no portable tensor-edge identity (tensor edges
        // over ranges are need 5 of the ranged-policy series).
        Type::RangedInt { .. }
        | Type::BoundedArray { .. } | Type::RangedArray { .. }
        | Type::Queue(_)
        | Type::BoundedQueue { .. }
        | Type::Stack(_)
        | Type::BoundedStack { .. }
        | Type::BoundedTextus { .. }
        | Type::BoundedAscii { .. }
        | Type::BoundedOcteti { .. }
        | Type::Map(_, _)
        | Type::Ledger(_, _)
        | Type::Set(_)
        | Type::Promissum(_)
        | Type::PromissumFailable(_, _)
        | Type::Cursor(_)
        | Type::AsyncCursor(_, _)
        | Type::Atomic(_)
        | Type::Intervallum(..)
        // Bits have no tensor-edge provider identity.
        | Type::Bits(_)
        | Type::Func(_)
        | Type::Param(_)
        | Type::Infer(_)
        | Type::InferUnion(_)
        | Type::MarkerHole { .. }
        | Type::Error => return None,
    };
    Some(identity)
}

fn portable_tensor_edge_record_identity(
    types: &TypeTable,
    interner: &Interner,
    fields: &FxHashMap<Symbol, TypeId>,
) -> Option<MirTensorEdgeTypeIdentity> {
    let mut fields = fields
        .iter()
        .map(|(name, field_ty)| {
            Some((
                interner.resolve(*name).to_owned(),
                portable_tensor_edge_type_identity(types, interner, *field_ty)?,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    fields.sort_by(|left, right| left.0.cmp(&right.0));
    Some(MirTensorEdgeTypeIdentity::Record(fields))
}

fn portable_tensor_edge_shape_identity(types: &TypeTable, shape: IndexId) -> Option<MirTensorEdgeShapeIdentity> {
    match types.get_index(shape) {
        IndexExpr::Literal(value) => Some(MirTensorEdgeShapeIdentity::Literal(*value)),
        IndexExpr::Tuple(items) => Some(MirTensorEdgeShapeIdentity::Tuple(
            items
                .iter()
                .map(|item| portable_tensor_edge_shape_identity(types, *item))
                .collect::<Option<Vec<_>>>()?,
        )),
        IndexExpr::Param(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Affine { .. }
        | IndexExpr::Divide { .. }
        | IndexExpr::Infer(_)
        | IndexExpr::Unspecified => None,
    }
}

pub use radix_types::KernelLowering;

#[derive(Debug, Clone)]
pub struct ImportedCallableParam {
    pub optional: bool,
    pub default: Option<HirExpression>,
}

#[derive(Default)]
pub struct MirLoweringOptions {
    /// How `@ kernel` functions lower; see [`KernelLowering`].
    pub kernel_lowering: KernelLowering,
    /// Parsed runtime CLI record fields keyed by the `argumenta` record
    /// `TypeId` (entry record and each `@ imperium` command record), so
    /// surfaces sharing a binding name do not collide.
    pub cli_parsed_record_fields_by_type: Arc<FxHashMap<TypeId, Vec<MirRuntimeRecordField>>>,
    /// Descriptor-derived CLI record fields keyed by the `argumenta` record
    /// type (entry + `@ imperium` command surfaces), used by the shared
    /// default-path lowering when the caller supplied no parsed runtime
    /// fields for a CLI record local.
    pub cli_record_fields_by_type: Arc<FxHashMap<TypeId, Vec<MirRuntimeRecordField>>>,
    /// S8.2: build CLI-record entry/command records from runtime-filled temps
    /// (see [`FunctionBuilderContext::cli_adapter_active`]).
    pub cli_adapter_active: bool,
    /// Function item `DefId` to a shader stage the caller resolved outside the
    /// language (SPIKE: manifest-bound annotation contracts). Consulted only
    /// when the function carries no built-in stage flag, so the compiler core
    /// reads no stage word for these functions.
    pub stage_bindings: Arc<FxHashMap<DefId, MirKernelShaderStage>>,
    /// Optional/default parameters for package-linked callees, whose synthetic
    /// `DefIds` are absent from this unit's `function_params`; the package seam
    /// supplies their metadata here.
    pub imported_function_params: Arc<FxHashMap<DefId, Vec<ImportedCallableParam>>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirRuntimeRecordField {
    pub name: Symbol,
    pub value: MirRuntimeRecordValue,
}

impl MirRuntimeRecordField {
    #[must_use]
    pub fn operand(field: MirNamedOperand) -> Self {
        Self { name: field.name, value: MirRuntimeRecordValue::Operand(field.value) }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MirRuntimeRecordValue {
    Operand(MirOperand),
    Array {
        ty: MirType,
        items: Vec<MirOperand>,
    },
    /// An array of arrays (a rest `octeti` operand): `ty` is the outer array
    /// type, and each row is one inner array typed by its element type.
    Rows {
        ty: MirType,
        rows: Vec<Vec<MirOperand>>,
    },
}

/// Lower a semantically analyzed HIR unit into a validated MIR unit.
///
/// The unit is read immutably: every shape lowering synthesizes — the
/// `ensure_*` scanners below, CLI record field types, and monomorphization's
/// concrete shapes — goes into a lowering-owned extension table cloned from
/// the unit's arena once per call. Upstream phases own what they created, so
/// lowering never mutates the analyzed unit's `TypeTable` or `Interner`, and
/// the returned token carries its own table through
/// [`MirValidationContext::with_owned_types`].
///
/// The returned unit carries a program that has already passed MIR validation,
/// along with the validation token and lowering metadata. Lowering failures and
/// validation failures share the same error type so callers can treat the MIR
/// pipeline as one fail-closed developer command.
///
/// # Errors
/// Returns `MirError` values for unsupported HIR surfaces, missing semantic
/// types, sealed-block misuse, a corrupt type arena, and post-lowering
/// validation failures.
pub fn lower_analyzed_unit(
    unit: AnalyzedModuleApplicationView<'_>,
    options: MirLoweringOptions,
) -> Result<LoweredMirUnit<'_>, Vec<MirError>> {
    // Lowering-owned extension arena: one clone of the analyzed table per
    // call; every synthesized shape below interns into this table, never
    // into the unit's.
    let mut types = clone_lowering_type_table(unit)?;
    // Same law for the interner: lowering-added runtime and diagnostic
    // symbols intern into this clone, so the analyzed unit's interner keeps
    // exactly the symbols upstream phases produced.
    let mut interner = unit.interner.clone();
    // The snapshot carries only the arena; the ordering-contract fact (D1.4)
    // rides along for monomorphization's bound dispatch.
    types.ordering_contract = unit.types.ordering_contract;
    scan::ensure_scan_types(unit.hir, &mut types);
    ensure_map_iteration_array_types(&mut types);
    ensure_itera_coordinate_array_types(unit.hir, &mut types);
    ensure_enum_iteration_array_types(unit.hir, &mut types);
    ensure_intervallum_iteration_array_types(unit.hir, &mut types);
    ensure_tensor_bracket_index_array_types(unit.hir, &mut types);
    ensure_runtime_extent_shape_array_type(&mut types);
    ensure_applied_genus_self_types(unit.hir, &mut types);
    if options.kernel_lowering == KernelLowering::Device {
        lane_form::ensure_lane_word_type(&mut types);
    }
    // The layout table is rebuilt after monomorphization's shape finalization
    // so it covers the concrete shapes/type graph the instances carry.
    prepare_mir_lowering_interner(&mut interner);
    // Merge the caller-supplied CLI record fields (parsed argv, adapter, or
    // package-MIR runtime records) with descriptor-derived fields keyed by the
    // `argumenta` record type: the shared default-path lowering fills every
    // CLI record from the analyzed `CliProgram` surface (option/operand
    // binding + typed defaults) so CLI programs lower through the same program
    // graph without caller input. Caller-supplied fields win per local.
    let cli_record_fields_by_type = cli_descriptor_record_fields_by_type(unit, &mut interner, &mut types);
    let empty_textus = interner.intern("");
    let diagnostics = DiagnosticSymbols {
        format_templates: [interner.intern("§"), interner.intern("§ §"), interner.intern("§ § §")],
        separator: interner.intern(" "),
        nihil: interner.intern(&unit.display_tokens.none),
        lockstep_length_mismatch: interner.intern("itera lockstep sources must have equal length"),
        ordo_no_member: interner.intern("no ordo member has this value"),
        potentia_negative_exponent: interner.intern("numerus potentia failed: negative exponent"),
        range_step_not_positive: interner.intern("range step must be positive"),
        map_changed_size: interner.intern("map changed size during iteration"),
        ponde_invalid_index: interner.intern(radix_runtime_contract::tensor::ERR_PONDE_INVALID_INDEX),
        accipe_invalid_index: interner.intern(radix_runtime_contract::tensor::ERR_ACCIPE_INVALID_INDEX),
        at_out_of_range: interner.intern("at: index out of range or key missing"),
        slice_out_of_range: interner.intern("slice_out_of_range § § §"),
        take_out_of_range: interner.intern("take_out_of_range § §"),
        ranged_push_past_maximum: interner.intern("list length range: push past the maximum"),
        ranged_remove_below_minimum: interner.intern("list length range: removal below the minimum"),
    };
    let kernel_lowering = options.kernel_lowering;
    let mut lowerer = MirLowerer {
        unit,
        types: &types,
        interner: &interner,
        options: MirLoweringOptions { cli_record_fields_by_type, ..options },
        empty_textus,
        diagnostics,
        errors: Vec::new(),
        functions: Vec::new(),
        closure_environments: Vec::new(),
        device_function_metadata: MirDeviceFunctionMetadataMap::default(),
        pending_tensor_limes: Arc::new(Mutex::new(Vec::new())),
        deferred_tensor_limes: Vec::new(),
        struct_field_types: None,
    };
    lowerer.lower();

    let lowering_errors = std::mem::take(&mut lowerer.errors);
    if lowering_errors.is_empty() {
        let monomorphize::MonomorphizeOutput {
            monomorphizations,
            templates: generic_templates,
            generic_params,
            tensor_limes_templates,
        } = lowerer.monomorphize();
        let monomorphization_errors = std::mem::take(&mut lowerer.errors);
        if !monomorphization_errors.is_empty() {
            return Err(monomorphization_errors);
        }
        let functions = std::mem::take(&mut lowerer.functions);
        let closure_environments = std::mem::take(&mut lowerer.closure_environments);
        let mut device_function_metadata = std::mem::take(&mut lowerer.device_function_metadata);
        let lowering_interner = interner.clone();
        drop(lowerer);
        let mut program = MirProgram { functions };
        // Shape substitution interns the concrete shapes/types each instance
        // carries into the lowering-owned arena before layouts and validation
        // read them back.
        finalize_lowered_shapes(unit, &mut types, &mut program, &monomorphizations)?;
        // Device-form post-processing: output-buffer copies, fragment
        // instantiation, and composition all run only when lowering a device
        // kernel; host lowering keeps the storage-preserving copy and skips
        // the fragment machinery.
        if kernel_lowering == KernelLowering::Device {
            apply_device_form_lowering(
                unit,
                &types,
                &interner,
                &mut program,
                &monomorphizations,
                &device_function_metadata,
                &generic_templates,
            )?;
        }
        attach_backend_refusals(unit, &program, &mut device_function_metadata);
        // The display tokens the file holding the unit prints through: its
        // own locale pack's words.
        let display_tokens = unit.display_tokens.clone();
        let (validated, imported_device_routes, tensor_edge_policy_providers) =
            build_lowering_validation(unit, program, types, &interner, &display_tokens, &closure_environments)?;
        Ok(LoweredMirUnit {
            interner: lowering_interner,
            monomorphizations,
            closure_environments,
            device_function_metadata,
            generic_templates,
            generic_params,
            tensor_edge_policy_metadata: MirTensorEdgePolicyMetadata {
                providers: tensor_edge_policy_providers,
                generic_limes: tensor_limes_templates,
            },
            validated,
            companions: radix_mir::device::MirCompanionMap::default(),
            imported_device_routes,
            imported_device_registrations: Vec::new(),
            display_tokens,
        })
    } else {
        Err(lowering_errors)
    }
}

/// Carry the per-target kernel requirement rows to the emit boundary: a
/// kernel function (and every source-less closure it owns) whose row refuses a
/// portable device backend records that backend in its metadata, so a package
/// route that never sees the semantic report still refuses by name before it
/// emits. A function with no row (a generic instance, an imported shape) stays
/// unmarked here; its own unit's lowering marks it.
fn attach_backend_refusals(
    unit: AnalyzedModuleApplicationView<'_>,
    program: &MirProgram,
    metadata: &mut MirDeviceFunctionMetadataMap,
) {
    use radix_mir::device::{MirDeviceBackend, MirRefusedBackends};
    use radix_target::Target;

    if unit.kernel_requirements.kernels.is_empty() {
        return;
    }
    let source_of: FxHashMap<MirFunctionId, DefId> = program
        .functions
        .iter()
        .filter_map(|function| Some((function.id, function.source?)))
        .collect();
    for row in metadata.values_mut() {
        let Some(requirement) = source_of
            .get(&row.identity.parent)
            .and_then(|def_id| unit.kernel_requirements.kernel(*def_id))
        else {
            continue;
        };
        let mut refused = MirRefusedBackends::default();
        for report in requirement
            .target_reports
            .iter()
            .filter(|report| !report.admitted)
        {
            let backend = match report.target {
                Target::MirAmd => MirDeviceBackend::Amd,
                Target::MirMetal => MirDeviceBackend::Metal,
                Target::MirWgsl => MirDeviceBackend::Wgsl,
                _ => continue,
            };
            refused = refused.with(backend);
        }
        row.refused_backends = refused;
    }
}

fn clone_lowering_type_table(unit: AnalyzedModuleApplicationView<'_>) -> Result<TypeTable, Vec<MirError>> {
    TypeTable::from_snapshot(unit.types.snapshot()).map_err(|message| {
        vec![MirError {
            message: format!("MIR lowering could not clone the type arena: {message}"),
            issue: "mir_type_arena_clone_failed".to_owned(),
            span: Span::default(),
            definition_span: None,
        }]
    })
}

fn finalize_lowered_shapes(
    unit: AnalyzedModuleApplicationView<'_>,
    types: &mut TypeTable,
    program: &mut MirProgram,
    monomorphizations: &[MirMonomorphization],
) -> Result<(), Vec<MirError>> {
    let errors =
        monomorphize::finalize_monomorphized_shapes(unit.hir, types, &mut program.functions, monomorphizations);
    if errors.is_empty() {
        return Ok(());
    }
    Err(errors)
}

fn apply_device_form_lowering(
    unit: AnalyzedModuleApplicationView<'_>,
    types: &TypeTable,
    interner: &Interner,
    program: &mut MirProgram,
    monomorphizations: &[MirMonomorphization],
    device_function_metadata: &MirDeviceFunctionMetadataMap,
    generic_templates: &[MirFunction],
) -> Result<(), Vec<MirError>> {
    // Full (un-gated) device roles: composition checks classify every
    // `@ nucleum` function, including private helpers that produce no
    // entry row. Entry gating happens on the driver's emit/dispatch path
    // via `device_entry_roles_from_hir`.
    let roles = radix_mir::device_roles_from_hir(unit.hir);
    // `⇇` on a device function lowers as the output-buffer store the
    // recipes already write; the host keeps the storage-preserving copy.
    // Host kernel lowering keeps the host copy for every function.
    for function in &mut program.functions {
        if function
            .source
            .is_some_and(|source| roles.contains_key(&source))
        {
            radix_mir::device_copy_into::lower_device_copy_into(function);
        }
    }
    // Device fragment checking and composition are device-form machinery.
    // Host kernel lowering skips both: a kernel is an ordinary function,
    // and a call to a helper kernel stays a plain call.
    // DFV2-4: launch entries are the public `@ nucleum` sources — the
    // only device functions that stay program symbols when device
    // code also calls them; composed private helpers are pruned.
    let launch_entries = radix_mir::device_entry_roles_from_hir(unit.hir, interner)
        .into_iter()
        .filter(|(_, role)| *role == radix_mir::MirDeviceRole::Kernel)
        .map(|(source, _)| source)
        .collect::<FxHashSet<_>>();
    let layouts = MirLayoutTable::from_types(types);
    let summaries =
        radix_semantic::passes::fragment_definition::check(unit.hir, unit.resolver, types, interner).summaries;
    let instantiation_errors = super::fragment_instantiation::check_with_metadata(
        &program.functions,
        monomorphizations,
        &layouts,
        types,
        &roles,
        Some(&summaries),
        device_function_metadata,
    );
    if !instantiation_errors.is_empty() {
        return Err(instantiation_errors
            .into_iter()
            .map(MirError::from)
            .collect());
    }
    let composition_errors = super::fragment_composition::compose_keeping_template_callees(
        &mut program.functions,
        monomorphizations,
        &roles,
        device_function_metadata,
        super::fragment_composition::active_budget(),
        Some(interner),
        &launch_entries,
        generic_templates,
    );
    if !composition_errors.is_empty() {
        return Err(composition_errors
            .into_iter()
            .map(|error| MirError {
                message: error.message(),
                issue: error.issue.to_owned(),
                span: error.span,
                definition_span: error.definition_span,
            })
            .collect());
    }
    Ok(())
}

/// The validated MIR plus the route and tensor-edge-policy tables it carries.
type LoweringValidation<'a> = (
    ValidatedMir<'a>,
    FxHashMap<DefId, Vec<ImportedDeviceRoute>>,
    Vec<MirTensorEdgePolicyProvider>,
);

fn build_lowering_validation<'a>(
    unit: AnalyzedModuleApplicationView<'a>,
    program: MirProgram,
    types: TypeTable,
    interner: &Interner,
    display_tokens: &radix_runtime_contract::display_tokens::DisplayTokens,
    closure_environments: &[MirClosureEnvironment],
) -> Result<LoweringValidation<'a>, Vec<MirError>> {
    let mut maps = LoweringContextMaps::collect(unit);
    // KPC-WIRE done_when (c): the module-owned route map is transferred
    // exactly once instead of being dropped with the rest of the walk.
    let imported_device_routes = maps.take_imported_device_routes();
    let mut validation = maps.validation;
    align_function_signatures_with_mir_params(&mut validation, &program.functions);
    record_function_display_tokens(&mut validation, &program, &types, interner, display_tokens);
    validation.layouts = MirLayoutTable::from_types(&types);
    validation.closure_environments = closure_environments
        .iter()
        .map(|environment| (environment.id, environment.clone()))
        .collect();
    // The escaping context resolves MIR symbols through the lowering-owned
    // interner clone — it carries the runtime/diagnostic symbols this
    // lowering interned (format templates, the lockstep panic text), which
    // the analyzed unit's interner no longer gains by mutation.
    validation.interner = radix_mir::MirInterner::Owned(std::sync::Arc::new(interner.clone()));
    // Ownership hand-off: the validation token takes the lowering-owned
    // arena, so the returned unit carries its own table and borrows
    // nothing from the lowering call itself.
    let tensor_edge_policy_providers =
        portable_tensor_edge_policy_providers(unit.tensor_edge_policies, &types, interner);
    let validation = validation.with_owned_types(types);
    let validated = ValidatedMir::new(program, validation).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| MirError::validation(error.span, error.message))
            .collect::<Vec<_>>()
    })?;
    Ok((validated, imported_device_routes, tensor_edge_policy_providers))
}

/// Whether the intrinsic renders program output (`print`, interpolation,
/// `nota`, `vide`, `mone`, `scribe`).
fn is_print_family(intrinsic: &MirIntrinsic) -> bool {
    matches!(
        intrinsic,
        MirIntrinsic::Diagnostic(_) | MirIntrinsic::FormatString { .. } | MirIntrinsic::FormatValue { .. }
    )
}

/// Give every function holding a print-family site, and the explicit entry
/// function, an id into the program's display-token table. The id rides per
/// function, not per call: every site of a function comes from one file, and
/// `MirRuntimeCall` has hundreds of struct-literal users across the backends.
/// The entry carries its file's tokens even when it holds no print site: the
/// llvm and wasm hosts read the entry's tokens once at start-up and apply them
/// to every print in the program.
fn record_function_display_tokens(
    validation: &mut MirValidationContext<'_>,
    program: &MirProgram,
    types: &TypeTable,
    interner: &Interner,
    tokens: &radix_runtime_contract::display_tokens::DisplayTokens,
) {
    let entry = radix_mir::MirNames::new(program, types, interner)
        .explicit_entry_function()
        .map(|function| function.id);
    for function in &program.functions {
        if holds_print_site(function) || Some(function.id) == entry {
            validation.display_tokens.assign(function.id, tokens);
        }
    }
}

/// Whether `function` holds a print-family site. Package linking asks this of
/// generic templates, which leave the program before the table is filled.
#[must_use]
pub fn holds_print_site(function: &MirFunction) -> bool {
    function.blocks.iter().flat_map(|block| &block.statements).any(|statement| {
        matches!(&statement.kind, MirStatementKind::RuntimeCall { call, .. } if is_print_family(&call.intrinsic))
    })
}

/// Replace HIR reference wrappers in local callable signatures with the MIR
/// storage types. `MirParamMode` carries the borrow ABI separately, while call
/// argument validation must compare the caller's value place with the callee's
/// referent slot type.
fn align_function_signatures_with_mir_params(validation: &mut MirValidationContext<'_>, functions: &[MirFunction]) {
    for function in functions {
        let Some(source) = function.source else {
            continue;
        };
        let Some(signature) = validation.functions.get_mut(&source) else {
            continue;
        };
        for (expected, param) in signature.params.iter_mut().zip(&function.params) {
            expected.ty = param.ty;
        }
    }
}

/// Pre-intern symbols synthesized by MIR lowering before a target clones the
/// analyzed-unit interner for emission.
pub fn prepare_mir_lowering_interner(interner: &mut Interner) {
    for template in ["", "§", "§ §", "§ § §", " ", "nihil", "processus", "argv"] {
        interner.intern(template);
    }
}

/// Build descriptor-derived CLI record fields keyed by the `argumenta` record
/// type.
///
/// The shared default-path lowering constructs every `argumenta` record
/// (entry and `@ imperium` command functions) from the analyzed CLI descriptor
/// when the caller supplied no parsed runtime fields for that local: each
/// option/operand binding becomes a named record field carrying its typed
/// default (flags default to false, undefaulted value options to `nihil`,
/// rest operands to an empty array, required operands to a type-shaped
/// placeholder the runtime rebinds). Keying by record type keeps entry and
/// command surfaces that share a binding name (e.g. `cli/cli.fab` entry and
/// `greet` both bind `args`) from colliding.
fn cli_descriptor_record_fields_by_type(
    unit: AnalyzedModuleApplicationView<'_>,
    interner: &mut Interner,
    types: &mut TypeTable,
) -> Arc<FxHashMap<TypeId, Vec<MirRuntimeRecordField>>> {
    let Some(cli) = unit.cli_program else {
        return Arc::new(FxHashMap::default());
    };
    let mut fields_by_type = FxHashMap::default();
    if let Some(entry_ty) = cli_entry_args_type(unit) {
        let fields = cli_descriptor_record_fields(
            interner,
            types,
            cli.global_options.iter().chain(cli.options.iter()),
            cli.global_operands.iter().chain(cli.operands.iter()),
        );
        fields_by_type.entry(entry_ty).or_insert(fields);
    }
    for command in &cli.commands {
        if command.args_binding.is_none() {
            continue;
        }
        let Some(command_ty) = cli_command_record_type(unit, command.function_symbol) else {
            continue;
        };
        let fields = cli_descriptor_record_fields(
            interner,
            types,
            cli.global_options.iter().chain(command.options.iter()),
            cli.global_operands.iter().chain(command.operands.iter()),
        );
        fields_by_type.entry(command_ty).or_insert(fields);
    }
    Arc::new(fields_by_type)
}

/// Record type of the root `incipit argumenta` local, read from the HIR entry
/// block's runtime-provided args local.
fn cli_entry_args_type(unit: AnalyzedModuleApplicationView<'_>) -> Option<TypeId> {
    let name = unit.hir.entry_args_name?;
    let entry = unit.hir.entry.as_ref()?;
    entry.statements.iter().find_map(|statement| {
        let HirStatementKind::Local(local) = &statement.kind else {
            return None;
        };
        (local.name == name && local.runtime_provided)
            .then_some(local.ty)
            .flatten()
    })
}

/// Record type of a mounted command's `argumenta` param, read from the HIR
/// command function's CLI-args parameter.
fn cli_command_record_type(unit: AnalyzedModuleApplicationView<'_>, function_symbol: Symbol) -> Option<TypeId> {
    unit.hir.items.iter().find_map(|item| {
        let HirItemKind::Function(function) = &item.kind else {
            return None;
        };
        (function.name == function_symbol)
            .then(|| function.cli_args.as_ref().map(|param| param.ty))
            .flatten()
    })
}

/// Build the shared-MIR `argumenta` record surface for one CLI entry point or
/// command from the normalized CLI descriptor: options first (in global-then-
/// local order), then operands, matching the HIR lowerer's entry and command
/// argument record types.
fn cli_descriptor_record_fields<'a>(
    interner: &mut Interner,
    types: &mut TypeTable,
    options: impl Iterator<Item = &'a CliOption>,
    operands: impl Iterator<Item = &'a CliOperand>,
) -> Vec<MirRuntimeRecordField> {
    let mut fields = Vec::new();
    for option in options {
        let value = cli_option_record_value(interner, types, option);
        fields.push(MirRuntimeRecordField { name: option.binding_symbol, value });
    }
    for operand in operands {
        let value = cli_operand_record_value(interner, types, operand);
        fields.push(MirRuntimeRecordField { name: operand.binding_symbol, value });
    }
    fields
}

/// Record value of one CLI option binding. Flags carry their parsed default; a
/// defaulted list-shaped option is the empty array the runtime rebinds; an
/// undefaulted value option is `nihil` of its `option(value)` record slot.
fn cli_option_record_value(
    interner: &mut Interner,
    types: &mut TypeTable,
    option: &CliOption,
) -> MirRuntimeRecordValue {
    if option.flag {
        MirRuntimeRecordValue::Operand(MirOperand::Constant(MirConstant::Bool(match &option.default {
            Some(CliDefault::Bool(value)) => *value,
            _ => false,
        })))
    } else if matches!(option.ty, CliType::Octeti) {
        if option.default.is_some() {
            // A defaulted octeti option is a non-optional `list<b8>`; the
            // runtime rebinds the bytes from argv.
            MirRuntimeRecordValue::Array { ty: MirType::semantic(types.octeti_list()), items: Vec::new() }
        } else {
            MirRuntimeRecordValue::Operand(MirOperand::Constant(MirConstant::Nil))
        }
    } else if matches!(option.ty, CliType::ListaTextus | CliType::ListaNumerus) {
        if option.default.is_some() {
            // Defaulted list options are non-optional `array(element)`; no
            // literal list default exists, so the compile-time shape is the
            // empty array.
            let element = cli_record_base_type(types, &option.ty);
            MirRuntimeRecordValue::Array { ty: MirType::semantic(types.array(element)), items: Vec::new() }
        } else {
            MirRuntimeRecordValue::Operand(MirOperand::Constant(MirConstant::Nil))
        }
    } else if let Some(default) = &option.default {
        MirRuntimeRecordValue::Operand(
            cli_literal_constant(interner, default).unwrap_or_else(|| cli_type_zero_operand(interner, &option.ty)),
        )
    } else {
        // Undefaulted value options are `option(value)` in the record type.
        MirRuntimeRecordValue::Operand(MirOperand::Constant(MirConstant::Nil))
    }
}

/// Record value of one CLI operand binding. A non-rest `octeti` operand is one
/// `list<b8>` array; rest and list operands are empty arrays; an undefaulted
/// scalar operand is the type-shaped placeholder the runtime rebinds.
fn cli_operand_record_value(
    interner: &mut Interner,
    types: &mut TypeTable,
    operand: &CliOperand,
) -> MirRuntimeRecordValue {
    if matches!(operand.ty, CliType::Octeti) && !operand.rest {
        // An octeti operand is one `list<b8>` array; the runtime rebinds
        // the bytes from argv.
        MirRuntimeRecordValue::Array { ty: MirType::semantic(types.octeti_list()), items: Vec::new() }
    } else if operand.rest || matches!(operand.ty, CliType::ListaTextus | CliType::ListaNumerus) {
        let element = cli_record_base_type(types, &operand.ty);
        MirRuntimeRecordValue::Array { ty: MirType::semantic(types.array(element)), items: Vec::new() }
    } else if let Some(default) = &operand.default {
        MirRuntimeRecordValue::Operand(
            cli_literal_constant(interner, default).unwrap_or_else(|| cli_type_zero_operand(interner, &operand.ty)),
        )
    } else {
        MirRuntimeRecordValue::Operand(cli_type_zero_operand(interner, &operand.ty))
    }
}

/// Map a CLI boundary type to the semantic primitive used as its record-field
/// base (mirroring `cli_value_type` in the HIR lowerer).
fn cli_record_base_type(types: &mut TypeTable, ty: &CliType) -> TypeId {
    match ty {
        CliType::Textus | CliType::Ignotum | CliType::ListaTextus => types.primitive(Primitive::Textus),
        CliType::Numerus | CliType::ListaNumerus => types.primitive(Primitive::Numerus),
        CliType::Fractus => types.primitive(Primitive::Fractus),
        CliType::Bivalens => types.primitive(Primitive::Bivalens),
        CliType::Octeti => types.octeti_list(),
    }
}

/// Literal `CliDefault` → MIR constant. Non-literal defaults return `None` so
/// callers fall back to a type-shaped placeholder.
fn cli_literal_constant(interner: &mut Interner, default: &CliDefault) -> Option<MirOperand> {
    let constant = match default {
        CliDefault::Text(value) => MirConstant::String(interner.intern(value)),
        CliDefault::Integer(value) => MirConstant::Int(*value),
        CliDefault::Float(value) => MirConstant::Float(*value),
        CliDefault::Bool(value) => MirConstant::Bool(*value),
        CliDefault::Nil => MirConstant::Nil,
        CliDefault::Expr(_) => return None,
    };
    Some(MirOperand::Constant(constant))
}

/// Type-shaped zero for a required CLI operand (or non-literal default) the
/// runtime rebinds from argv.
fn cli_type_zero_operand(interner: &mut Interner, ty: &CliType) -> MirOperand {
    let constant = match ty {
        CliType::Textus | CliType::Ignotum => MirConstant::String(interner.intern("")),
        CliType::Numerus => MirConstant::Int(0),
        CliType::Fractus => MirConstant::Float(0.0),
        CliType::Bivalens => MirConstant::Bool(false),
        // List-shaped fields (including octeti, a `list<b8>`) are always
        // array aggregates in the record; a zero constant is only reachable
        // for a non-list fallback path.
        CliType::Octeti | CliType::ListaTextus | CliType::ListaNumerus => MirConstant::Nil,
    };
    MirOperand::Constant(constant)
}

/// Lower and render a deterministic MIR dump for compiler inspection.
///
/// This helper exists so `radix mir` can expose one operation without letting
/// dump rendering bypass validation.
///
/// # Errors
/// Returns `MirError` values for unsupported HIR surfaces, missing semantic
/// types, sealed-block misuse, and post-lowering validation failures.
pub fn dump_analyzed_unit(unit: AnalyzedModuleApplicationView<'_>) -> Result<String, Vec<MirError>> {
    lower_analyzed_unit(unit, MirLoweringOptions::default()).map(|lowered| dump_program(lowered.validated.program()))
}

fn ensure_map_iteration_array_types(types: &mut TypeTable) {
    let mut elements = FxHashSet::default();
    for index in 0..types.type_count() {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
        )]
        match types.get(TypeId(index as u32)) {
            Type::Map(key, value) | Type::Ledger(key, value) => {
                elements.insert(*key);
                elements.insert(*value);
            }
            Type::Cursor(element) | Type::AsyncCursor(element, _) => {
                elements.insert(*element);
            }
            _ => {}
        }
    }
    for element in elements {
        types.ensure_array(element);
    }
}

/// A tensor with a run-time extent can be the target of `octeti ↦ tensor`,
/// whose shape operand is a `lista<numerus>`. Lowering holds the type table
/// immutably, so the list type is interned here, once, when any tensor type
/// names a run-time extent.
fn ensure_runtime_extent_shape_array_type(types: &mut TypeTable) {
    let mut seen = FxHashSet::default();
    let has_runtime_extent = (0..types.type_count()).any(|index| {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "TypeId is u32 by design; type_count is bounded by u32::MAX"
        )]
        match types.get(TypeId(index as u32)) {
            Type::Tensor(_, shape) => {
                index_contains_runtime_extent(types, *shape, &mut seen)
                    || index_may_contain_runtime_extent(types, *shape, &mut FxHashSet::default())
            }
            _ => false,
        }
    });
    if has_runtime_extent {
        let numerus = types.primitive(Primitive::Numerus);
        types.ensure_array(numerus);
    }
}

/// A generic genus's canonical self form `Genus<Param…>` is what a method
/// receiver must name: `applied_genus_receiver` reads the receiver from the
/// type table, and a bare `Type::Struct` binds no class parameter, so a call on
/// `Genus<3>` could never witness `N`. Intern one applied form per generic
/// genus, in declaration param order, so the receiver exists before the read-only
/// lowering walk starts. Lowering holds the type table immutably afterwards.
fn ensure_applied_genus_self_types(hir: &radix_hir::HirModule, types: &mut TypeTable) {
    use radix_types::AppliedArg;

    for item in &hir.items {
        let HirItemKind::Struct(strukt) = &item.kind else {
            continue;
        };
        if strukt.type_params.is_empty() {
            continue;
        }
        let base = types.intern(Type::Struct(item.def_id));
        let args = strukt
            .type_params
            .iter()
            .map(|param| {
                if param.is_typus() {
                    AppliedArg::Type(types.intern(Type::Param(param.name)))
                } else {
                    AppliedArg::Size(size_param_index(types, param.name))
                }
            })
            .collect();
        types.intern(Type::Applied(base, args));
    }
}

/// The interned `IndexExpr::Param` node for a declared size parameter, reusing
/// an existing node so shape witnesses share one identity (GSF-1).
fn size_param_index(types: &mut TypeTable, name: Symbol) -> IndexId {
    let existing = (0..types.index_count()).find_map(|ordinal| {
        #[expect(clippy::cast_possible_truncation, reason = "IndexId is a u32 arena index by design")]
        let id = IndexId(ordinal as u32);
        matches!(types.get_index(id), IndexExpr::Param(found) if *found == name).then_some(id)
    });
    existing.unwrap_or_else(|| types.intern_index(IndexExpr::Param(name)))
}

fn ensure_itera_coordinate_array_types(hir: &radix_hir::HirModule, types: &mut TypeTable) {
    struct IteraCoordinateCollector<'a> {
        types: &'a TypeTable,
        elements: FxHashSet<TypeId>,
    }

    impl HirVisitor for IteraCoordinateCollector<'_> {
        fn visit_expr(&mut self, expr: &HirExpression) {
            match &expr.kind {
                HirExpressionKind::Itera { iter, .. } => {
                    self.collect_shaped_element(iter);
                }
                HirExpressionKind::Reduction { source, .. } | HirExpressionKind::Scan { source, .. } => {
                    self.collect_shaped_element(source);
                    self.collect_shaped_element(expr);
                }
                _ => {}
            }
            walk_expr(self, expr);
        }
    }

    impl IteraCoordinateCollector<'_> {
        fn collect_shaped_element(&mut self, iter: &HirExpression) {
            let mut iter_ty = iter.ty;
            while let Some(ty) = iter_ty {
                match self.types.get(ty) {
                    Type::Alias(_, inner) | Type::Ref(_, inner) => iter_ty = Some(*inner),
                    Type::Tensor(element, _) | Type::Matrix(element, _) => {
                        self.elements.insert(*element);
                        break;
                    }
                    _ => break,
                }
            }
        }
    }

    let mut collector = IteraCoordinateCollector { types, elements: FxHashSet::default() };
    walk_program(&mut collector, hir);
    let elements = std::mem::take(&mut collector.elements);
    drop(collector);
    if !elements.is_empty() {
        let numerus = types.primitive(Primitive::Numerus);
        types.ensure_array(numerus);
        types.intern(Type::Option(numerus));
        for element in elements {
            types.ensure_array(element);
            types.intern(Type::Option(element));
        }
    }
}

/// An `itera ex` over an enum name walks the enum's declared members
///. Lowering materializes that series as a `lista<Enum>` temp,
/// so the list type is interned here, once, before function lowering begins
/// (function lowering holds the type table immutably).
fn ensure_enum_iteration_array_types(hir: &radix_hir::HirModule, types: &mut TypeTable) {
    struct EnumIterationCollector<'a> {
        types: &'a TypeTable,
        elements: FxHashSet<TypeId>,
    }

    impl HirVisitor for EnumIterationCollector<'_> {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if let HirExpressionKind::Itera { iter, .. } = &expr.kind
                && let Some(ty) = iter.ty
                && matches!(self.types.get(ty), Type::Enum(_))
            {
                self.elements.insert(ty);
            }
            walk_expr(self, expr);
        }
    }

    let mut collector = EnumIterationCollector { types, elements: FxHashSet::default() };
    walk_program(&mut collector, hir);
    let elements = std::mem::take(&mut collector.elements);
    drop(collector);
    for element in elements {
        types.ensure_array(element);
    }
}

/// An `itera ab` over a value of `intervallum` type (not a range literal)
/// materializes the interval into a `lista<T>` temp, so the
/// list type is interned here, once, before function lowering begins
/// (function lowering holds the type table immutably).
fn ensure_intervallum_iteration_array_types(hir: &radix_hir::HirModule, types: &mut TypeTable) {
    struct IntervallumIterationCollector<'a> {
        types: &'a TypeTable,
        elements: FxHashSet<TypeId>,
    }

    impl HirVisitor for IntervallumIterationCollector<'_> {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if let HirExpressionKind::Itera { mode, iter, .. } = &expr.kind
                && matches!(mode, HirIteraMode::Ab)
                && !matches!(&iter.kind, HirExpressionKind::Intervallum { .. })
                && let Some(ty) = iter.ty
                && let Type::Intervallum(bound, _) = self.types.get(ty)
            {
                self.elements.insert(*bound);
            }
            walk_expr(self, expr);
        }
    }

    let mut collector = IntervallumIterationCollector { types, elements: FxHashSet::default() };
    walk_program(&mut collector, hir);
    let elements = std::mem::take(&mut collector.elements);
    drop(collector);
    for element in elements {
        types.ensure_array(element);
    }
}

fn ensure_tensor_bracket_index_array_types(hir: &radix_hir::HirModule, types: &mut TypeTable) {
    fn index_has_range(index: &HirExpression) -> bool {
        match &index.kind {
            HirExpressionKind::Intervallum { .. } => true,
            HirExpressionKind::Array(elements) => elements.iter().any(|element| {
                matches!(
                    element,
                    HirArrayElement::Expr(slot)
                        if matches!(slot.kind, HirExpressionKind::Intervallum { .. })
                )
            }),
            _ => false,
        }
    }

    struct TensorBracketIndexArrayCollector<'a> {
        types: &'a TypeTable,
        elements: FxHashSet<TypeId>,
    }

    impl HirVisitor for TensorBracketIndexArrayCollector<'_> {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if let HirExpressionKind::Index(object, index) = &expr.kind {
                let object_is_coordinate = object
                    .ty
                    .is_some_and(|ty| radix_semantic::tensor_bracket::is_coordinate_bracket_receiver(self.types, ty));
                let index_is_list = index
                    .ty
                    .is_some_and(|ty| radix_semantic::tensor_bracket::is_bracket_index_list_type(self.types, ty));
                // WHY: a bracket *range* (`t[1 ‥ 4]`) lowers to `TensorSliceNd`
                // with five one-entry lists, and a slot list with a range slot
                // (`g[[1, 0 ‥ 3]]`) lowers to six N-entry lists, but a range is
                // untyped in this position, so no list type for its entries
                // exists. Lowering holds the type table immutably and
                // `find_array` only finds interned lists, so the axis-entry types
                // (`numerus` counter, `bivalens` keep/inclusive flags) are
                // interned here.
                if object_is_coordinate && index_has_range(index) {
                    self.elements
                        .insert(self.types.primitive(Primitive::Numerus));
                    self.elements
                        .insert(self.types.primitive(Primitive::Bivalens));
                }
                if object_is_coordinate
                    && !index_is_list
                    && let Some(index_ty) = index.ty
                {
                    self.elements.insert(index_ty);
                }
            }
            walk_expr(self, expr);
        }
    }

    let mut collector = TensorBracketIndexArrayCollector { types, elements: FxHashSet::default() };
    walk_program(&mut collector, hir);
    for element in collector.elements {
        types.ensure_array(element);
    }
}

#[derive(Clone, Copy)]
struct DiagnosticSymbols {
    format_templates: [Symbol; 3],
    separator: Symbol,
    /// The rendered null-value textus used by the scribe grouping when an
    /// option argument's payload is absent (`display::option` semantics).
    ///
    /// Keep this in lockstep with the Rust runtime carrier
    /// (`faber::display::option`), which owns the text. D23.2 splits the null
    /// *value* (`nulla`) from the null *type* (`nihil`); the value display must
    /// therefore move to `nulla` on both sides together — this string, the
    /// runtime carrier, and the corpus `.expected` oracles — in the D23.2
    /// sweep, never here alone.
    nihil: Symbol,
    /// Lockstep `itera ex` runtime admission message: the
    /// fail-closed equal-length guard's panic textus.
    lockstep_length_mismatch: Symbol,
    /// Failed `numerus ↦ Ordo` payload (D9.4): no member has the value.
    ordo_no_member: Symbol,
    /// Integer `potentia` with a negative exponent (ruling 28): the
    /// arithmetic trap's panic textus.
    potentia_negative_exponent: Symbol,
    /// A range step that is zero or negative at run time (ruling 2026-10-02):
    /// the stride guard's panic textus.
    range_step_not_positive: Symbol,
    /// Bracket store added a key to a map while a loop over it ran (need, O1(3)).
    map_changed_size: Symbol,
    /// The checked `ponde` store's out-of-range error textus (K11): the
    /// runtime-contract message the fatal index form also reports.
    ponde_invalid_index: Symbol,
    /// The checked `accipe` read's out-of-range error textus (K11-b).
    accipe_invalid_index: Symbol,
    /// The `at` read's (la `apud`) out-of-range index / missing key textus
    ///.
    at_out_of_range: Symbol,
    /// The list `slice` window's (la `sectio`) fault payload template: the
    /// fault kind plus start, end and list length as data.
    slice_out_of_range: Symbol,
    /// The list `take` trim's (la `prima`) fault payload template: the fault
    /// kind plus count and list length as data.
    take_out_of_range: Symbol,
    /// A push past the maximum of a list length range.
    ranged_push_past_maximum: Symbol,
    /// A removal below the minimum of a list length range.
    ranged_remove_below_minimum: Symbol,
}

struct MirLowerer<'a> {
    unit: AnalyzedModuleApplicationView<'a>,
    /// Lowering-owned extension arena (a per-call clone of `unit.types` that
    /// this lowering alone extends). Every type read during the walk goes
    /// through this table so synthesized shapes resolve like analyzed ones.
    types: &'a TypeTable,
    /// Lowering-owned interner clone: same law as [`Self::types`] for the
    /// runtime/diagnostic symbols lowering interns.
    interner: &'a Interner,
    options: MirLoweringOptions,
    empty_textus: Symbol,
    diagnostics: DiagnosticSymbols,
    errors: Vec<MirError>,
    functions: Vec<MirFunction>,
    closure_environments: Vec<MirClosureEnvironment>,
    device_function_metadata: MirDeviceFunctionMetadataMap,
    /// Custom policy callbacks are resolved after all source callables have
    /// MIR ids. Nested builders share this queue; final MIR carries MIDs only.
    pending_tensor_limes: Arc<Mutex<Vec<PendingTensorLimes>>>,
    /// Deferred generic policy selectors retained until each concrete generic
    /// body is cloned and its typus arguments are known.
    deferred_tensor_limes: Vec<PendingTensorLimes>,
    /// Struct field types collected during the body-lowering walk and retained
    /// for generic monomorphization, avoiding a second HIR traversal.
    struct_field_types: Option<Arc<FxHashMap<DefId, FxHashMap<Symbol, MirType>>>>,
}

#[derive(Debug, Clone, Copy)]
struct PendingTensorLimes {
    function: MirFunctionId,
    destination: MirTempId,
    dispatch: PendingTensorLimesDispatch,
    span: Span,
}

#[derive(Debug, Clone, Copy)]
enum PendingTensorLimesDispatch {
    Custom {
        policy_def: DefId,
        remap: DefId,
        transform: Option<DefId>,
    },
    Deferred {
        policy_type: TypeId,
    },
}

/// MIR locals, CFG, and side-effect functions produced by one callable lowering pass.
struct CallableLoweringParts {
    params: Vec<MirParam>,
    locals: Vec<MirLocalDecl>,
    temps: Vec<MirTemp>,
    blocks: Vec<MirBlock>,
    errors: Vec<MirError>,
    synthetics: Vec<MirFunction>,
    closure_environments: Vec<MirClosureEnvironment>,
    device_function_metadata: MirDeviceFunctionMetadataMap,
    lane_domain_local: Option<MirLocalId>,
}

struct CallableLoweringConfig<'input> {
    error_ty: Option<MirType>,
    parent_function_id: MirFunctionId,
    device_role: Option<MirDeviceRole>,
    target: CallableLoweringTarget<'input>,
    def_id: DefId,
}

enum CallableLoweringTarget<'input> {
    Function(&'input HirFunction),
    Method {
        struct_item: &'input HirItem,
        strukt: &'input HirStruct,
        method: &'input HirMethod,
        receiver_ty: TypeId,
    },
}

impl CallableLoweringTarget<'_> {
    /// The signature/body carrier both targets share: a free function, or a
    /// method's function body.
    fn hir_function(&self) -> &HirFunction {
        match self {
            Self::Function(function) => function,
            Self::Method { method, .. } => &method.func,
        }
    }
}

/// Per-function lowering inputs collected once per whole-unit pass.
pub struct BodyLoweringContext<'maps, 'unit> {
    context_maps: &'maps LoweringContextMaps<'unit>,
    struct_fields: Arc<FxHashMap<DefId, Vec<&'unit HirField>>>,
    struct_field_types: Arc<FxHashMap<DefId, FxHashMap<radix_lexer::Symbol, MirType>>>,
    function_params: Arc<FxHashMap<DefId, Vec<context::CallableParamMeta<'unit>>>>,
    empty_textus: Symbol,
    diagnostics: DiagnosticSymbols,
}

impl<'a> MirLowerer<'a> {
    /// Run the whole-unit lowering sequence before MIR validation.
    ///
    /// Whole-unit maps are collected up front because function bodies need
    /// signatures, enum variant metadata, provider imports, and validation
    /// tables that may be declared later in source order.
    fn lower(&mut self) {
        let context_maps = LoweringContextMaps::collect(self.unit);
        let struct_fields = Arc::new(struct_field_map(self.unit));
        let struct_field_types = Arc::new(context_maps.validation.struct_fields.clone());
        self.struct_field_types = Some(Arc::clone(&struct_field_types));
        let function_params = Arc::new(function_param_map(self.unit));
        let body_ctx = BodyLoweringContext {
            context_maps: &context_maps,
            struct_fields,
            struct_field_types,
            function_params,
            empty_textus: self.empty_textus,
            diagnostics: self.diagnostics,
        };
        ItemLoweringPass::new(self, &body_ctx).lower_items();

        if let Some(entry) = &self.unit.hir.entry {
            self.lower_entry(entry, &body_ctx);
        }
        self.resolve_pending_tensor_limes();
    }

    /// Replace the private lowering placeholders for custom `limes` calls
    /// with callback MIDs after all source functions have been assigned IDs.
    /// Missing callback bodies or call destinations fail closed before MIR
    /// validation/serialization.
    fn resolve_pending_tensor_limes(&mut self) {
        let pending = if let Ok(mut pending) = self.pending_tensor_limes.lock() {
            std::mem::take(&mut *pending)
        } else {
            self.errors.push(MirError::unsupported(
                Span::default(),
                "tensor limes callback queue was poisoned during lowering",
            ));
            return;
        };
        if pending.is_empty() {
            return;
        }

        let functions_by_source = self
            .functions
            .iter()
            .filter_map(|function| function.source.map(|source| (source, function.id)))
            .collect::<FxHashMap<_, _>>();
        for pending in pending {
            let PendingTensorLimesDispatch::Custom { policy_def, remap: remap_def, transform: transform_def } =
                pending.dispatch
            else {
                self.deferred_tensor_limes.push(pending);
                continue;
            };
            let Some(remap) = functions_by_source.get(&remap_def).copied() else {
                self.errors.push(MirError::unsupported(
                    pending.span,
                    "custom tensor limes remap callback has no lowered function",
                ));
                continue;
            };
            let transform = match transform_def {
                Some(def_id) => {
                    if let Some(function) = functions_by_source.get(&def_id).copied() {
                        Some(function)
                    } else {
                        self.errors.push(MirError::unsupported(
                            pending.span,
                            "custom tensor limes transform callback has no lowered function",
                        ));
                        continue;
                    }
                }
                None => None,
            };
            let Some(function) = self
                .functions
                .iter_mut()
                .find(|function| function.id == pending.function)
            else {
                self.errors.push(MirError::unsupported(
                    pending.span,
                    "custom tensor limes call has no containing function",
                ));
                continue;
            };
            let statement = function.blocks.iter_mut().flat_map(|block| &mut block.statements).find(|statement| {
                matches!(
                    &statement.kind,
                    MirStatementKind::RuntimeCall {
                        destination: Some(place),
                        call,
                    } if place.base == MirPlaceBase::Temp(pending.destination)
                        && place.projections.is_empty()
                        && matches!(&call.intrinsic, MirIntrinsic::TensorLimes(MirEdgePolicy::Builtin(MirBuiltinEdgePolicy::Clamp)))
                )
            });
            let Some(statement) = statement else {
                self.errors.push(MirError::unsupported(
                    pending.span,
                    "custom tensor limes placeholder was not found in its lowered function",
                ));
                continue;
            };
            if let MirStatementKind::RuntimeCall { call, .. } = &mut statement.kind {
                call.intrinsic = MirIntrinsic::TensorLimes(MirEdgePolicy::Custom { policy_def, remap, transform });
            }
        }
    }

    /// Compute shader stage from HIR function flags and emit a diagnostic when
    /// conflicting annotations make the stage unresolvable.
    fn compute_shader_stage(
        &mut self,
        function: &HirFunction,
        def_id: DefId,
        span: Span,
    ) -> Option<MirKernelShaderStage> {
        let stage = shader_stage_for_function(function).or_else(|| self.options.stage_bindings.get(&def_id).copied());
        if stage.is_none() && !annotations_exclusive(function) {
            self.errors.push(MirError::validation(
                span,
                "conflicting shader-stage annotations: at most one of @nucleum, @nucleum fragment, @vertex, or @fragment is allowed",
            ));
        }
        stage
    }

    /// Lower one HIR function item into one MIR function.
    ///
    /// Function lowering is intentionally type-strict: parameters, locals, the
    /// return type, and alternate-exit type must already be present on HIR. The
    /// builder may still emit local lowering errors, but this wrapper preserves
    /// source-order accounting for the function while the whole-unit pipeline
    /// decides whether validation is allowed to run.
    fn lower_function(&mut self, item: &HirItem, function: &HirFunction, body_ctx: &BodyLoweringContext<'_, '_>) {
        let Some(return_ty) = function.ret_ty else {
            self.errors
                .push(MirError::missing_type(item.span, "function return type"));
            return;
        };
        self.push_lowered_callable(
            function.name,
            item.def_id,
            item.span,
            return_ty,
            function,
            &item.annotations,
            CallableLoweringTarget::Function(function),
            body_ctx,
        );
    }

    fn applied_genus_receiver(&self, def_id: DefId, strukt: &HirStruct) -> Option<TypeId> {
        use radix_types::AppliedArg;

        if strukt.type_params.is_empty() {
            return None;
        }

        #[expect(clippy::cast_possible_truncation, reason = "TypeId is u32 by design")]
        (0..self.types.type_count()).find_map(|index| {
            let id = TypeId(index as u32);
            let Type::Applied(base, args) = self.types.get(id) else {
                return None;
            };
            if !matches!(self.types.get(*base), Type::Struct(found) if *found == def_id)
                || args.len() != strukt.type_params.len()
            {
                return None;
            }
            args.iter()
                .zip(&strukt.type_params)
                .all(|(arg, param)| match arg {
                    AppliedArg::Type(ty) => {
                        param.is_typus() && matches!(self.types.get(*ty), Type::Param(name) if *name == param.name)
                    }
                    AppliedArg::Size(index) => {
                        !param.is_typus()
                            && matches!(self.types.get_index(*index), IndexExpr::Param(name) if *name == param.name)
                    }
                })
                .then_some(id)
        })
    }

    /// Lower a genus method as an ordinary MIR function with an explicit
    /// receiver parameter bound to the struct definition used by `ego`.
    fn lower_method(
        &mut self,
        struct_item: &HirItem,
        strukt: &HirStruct,
        method: &HirMethod,
        body_ctx: &BodyLoweringContext<'_, '_>,
    ) {
        let Some(return_ty) = method.func.ret_ty else {
            self.errors
                .push(MirError::missing_type(method.span, "method return type"));
            return;
        };
        let receiver_ty = self
            .applied_genus_receiver(struct_item.def_id, strukt)
            .or_else(|| self.types.find_struct(struct_item.def_id));
        let Some(receiver_ty) = receiver_ty else {
            self.errors
                .push(MirError::missing_type(method.span, "method receiver type"));
            return;
        };
        self.push_lowered_callable(
            method.func.name,
            method.def_id,
            method.span,
            return_ty,
            &method.func,
            &method.annotations,
            CallableLoweringTarget::Method { struct_item, strukt, method, receiver_ty },
            body_ctx,
        );
    }

    /// Lower one callable body and push its `MirFunction` (plus any synthetic
    /// companion functions) into the program.
    ///
    /// Shared spine of [`Self::lower_function`] and [`Self::lower_method`].
    /// Preserves source-order error accounting: the caller's return-type guard
    /// fires first, then body lowering errors, then the shader-stage
    /// annotation check (which may append a validation error last).
    fn push_lowered_callable(
        &mut self,
        name: Symbol,
        def_id: DefId,
        span: Span,
        return_ty: TypeId,
        function: &HirFunction,
        annotations: &[radix_hir::HirAnnotation],
        target: CallableLoweringTarget<'_>,
        body_ctx: &BodyLoweringContext<'_, '_>,
    ) {
        let error_ty = function.err_ty.map(MirType::semantic);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; lowered function count is bounded by u32::MAX"
        )]
        let parent_function_id = MirFunctionId(self.functions.len() as u32);
        let device_role = device_role_for_function(function);
        let CallableLoweringParts {
            params,
            locals,
            temps,
            blocks,
            errors,
            synthetics,
            closure_environments,
            device_function_metadata,
            lane_domain_local,
        } = self.lower_callable_body(
            body_ctx,
            &CallableLoweringConfig { error_ty, parent_function_id, device_role, target, def_id },
        );
        self.errors.extend(errors);
        self.closure_environments.extend(closure_environments);
        self.device_function_metadata
            .extend(device_function_metadata);

        let shader_stage = self.compute_shader_stage(function, def_id, span);
        // KCM-05: named device functions carry the same explicit role,
        // stage, and visibility facts as source-less kernel closures. The
        // device-program constructor is fail-closed when this row is absent,
        // so populate it at the source-function lowering boundary rather than
        // making package consumers reconstruct HIR visibility later.
        if let (Some(role), Some(shader_stage)) = (device_role, shader_stage) {
            self.device_function_metadata.insert(
                parent_function_id,
                MirDeviceFunctionMetadata {
                    identity: MirSyntheticFunctionIdentity { parent: parent_function_id, ordinal: 0 },
                    role,
                    shader_stage,
                    visibility: if is_public_entry(annotations, self.interner) {
                        MirFunctionVisibility::Public
                    } else {
                        MirFunctionVisibility::Private
                    },
                    origin: MirFunctionOrigin::Source,
                    lane_domain_local,
                    refused_backends: radix_mir::device::MirRefusedBackends::default(),
                    // The declared tier of the source kernel. The safe switch
                    // already lowered it in analysis, so this is the
                    // effective tier the emitters read.
                    tier: radix_hir::kernel_tier_of(annotations, self.interner),
                },
            );
        }
        self.push_function_with_synthetics(
            MirFunction {
                id: parent_function_id,
                source: Some(def_id),
                name: Some(name),
                params,
                locals,
                temps,
                blocks,
                return_ty: MirType::semantic(return_ty),
                error_ty,
                is_async: function.is_async,
                is_generator: function.is_generator,
                shader_stage,
                span,
            },
            synthetics,
        );
    }

    /// Fresh per-callable lowering context from the whole-unit maps, wired to
    /// this lowerer's CLI-record options. The returned context outlives the
    /// maps borrow (it owns its Arc-cloned tables), so the unit lifetime `'a`
    /// is named explicitly.
    fn builder_context(&self, body_ctx: &BodyLoweringContext<'_, 'a>) -> FunctionBuilderContext<'a> {
        let mut context = body_ctx.context_maps.builder_context(
            self.interner,
            body_ctx.empty_textus,
            body_ctx.diagnostics,
            Arc::clone(&body_ctx.struct_fields),
            Arc::clone(&body_ctx.struct_field_types),
            Arc::clone(&body_ctx.function_params),
            Arc::clone(&self.options.imported_function_params),
            Arc::clone(&self.options.cli_parsed_record_fields_by_type),
            Arc::clone(&self.options.cli_record_fields_by_type),
            self.options.cli_adapter_active,
        );
        context.shape_calls = Arc::new(self.unit.shape_obligations.calls.clone());
        context.shape_functions = Arc::new(self.unit.shape_obligations.functions.clone());
        context.call_size_bindings = Arc::new(self.unit.shape_obligations.call_size_bindings.clone());
        context.shape_stores = Arc::new(self.unit.shape_obligations.stores.clone());
        context.proven_indexes = Arc::new(self.unit.shape_obligations.proven_indexes.clone());
        context.function_size_bindings = Arc::new(self.unit.shape_obligations.function_size_bindings.clone());
        context.tensor_edge_policies = Arc::new(self.unit.tensor_edge_policies.clone());
        context.pending_tensor_limes = Arc::clone(&self.pending_tensor_limes);
        context.kernel_lowering = self.options.kernel_lowering;
        context
    }

    /// Lower one function or method body through a fresh [`FunctionBuilder`].
    ///
    /// Parameter binding differs between top-level functions and genus methods,
    /// so callers supply an `add_params` hook while this helper owns builder
    /// context construction, body visitation, and synthetic-function extraction.
    fn lower_callable_body(
        &mut self,
        body_ctx: &BodyLoweringContext<'_, '_>,
        config: &CallableLoweringConfig<'_>,
    ) -> CallableLoweringParts {
        let context = self.builder_context(body_ctx);
        let function_shape_relations = context
            .shape_functions
            .get(&config.def_id)
            .cloned()
            .unwrap_or_default();
        let function_size_bindings = context
            .function_size_bindings
            .get(&config.def_id)
            .cloned()
            .unwrap_or_default();
        let formal_parameter_offset = u32::from(matches!(&config.target, CallableLoweringTarget::Method { .. }));
        let callable = config.target.hir_function();
        let shader_stage =
            shader_stage_for_function(callable).or_else(|| self.options.stage_bindings.get(&config.def_id).copied());
        let mut builder = FunctionBuilder::for_function(
            self.types,
            config.error_ty,
            context,
            config.parent_function_id,
            config.device_role,
            shader_stage,
        );
        builder.function_shape_relations = function_shape_relations;
        builder.formal_parameter_offset = formal_parameter_offset;
        builder.size_params = size_param_names(&callable.type_params);
        let size_type_params = &callable.type_params;
        for binding in function_size_bindings {
            if let Some(parameter) = size_type_params.get(binding.parameter as usize)
                && !parameter.is_typus()
            {
                builder
                    .size_param_sources
                    .insert(parameter.def_id, binding.witness);
            }
        }
        builder.bound_methods = self.bound_method_targets(&callable.type_params);
        let (body, cli_args) = match &config.target {
            CallableLoweringTarget::Function(function) => {
                for param in &function.params {
                    builder.add_param(param.def_id, param.name, param.ty, param.mode, param.span);
                }
                if let Some(body) = function.body.as_ref() {
                    builder.prepare_lane_region(body);
                }
                if function.nucleum_fragment {
                    for (def_id, name, ty, span) in inherited_lane_binding_locals(function, self.unit.resolver) {
                        builder.add_inherited_lane_param(def_id, name, ty, span);
                    }
                }
                (function.body.as_ref(), function.cli_args.as_ref())
            }
            CallableLoweringTarget::Method { struct_item, strukt, method, receiver_ty } => {
                builder.add_param(struct_item.def_id, strukt.name, *receiver_ty, HirParamMode::Owned, method.span);
                for param in &method.func.params {
                    builder.add_param(param.def_id, param.name, param.ty, param.mode, param.span);
                }
                if let Some(body) = method.func.body.as_ref() {
                    builder.prepare_lane_region(body);
                }
                (method.func.body.as_ref(), None)
            }
        };

        let blocks = match body {
            Some(body) => builder.lower_body_with_cli_args(body, cli_args),
            None => Vec::new(),
        };
        let synthetics = builder.take_synthetic_functions();
        let closure_environments = builder.take_closure_environments();
        let device_function_metadata = builder.take_device_function_metadata();
        let lane_domain_local = builder.lane_region.and_then(|region| region.source_local);
        CallableLoweringParts {
            params: builder.params,
            locals: builder.locals,
            temps: builder.temps,
            blocks,
            errors: builder.errors,
            synthetics,
            closure_environments,
            device_function_metadata,
            lane_domain_local,
        }
    }

    fn push_function_with_synthetics(&mut self, function: MirFunction, synthetics: Vec<MirFunction>) {
        self.functions.push(function);
        for synthetic in synthetics {
            self.functions.push(synthetic);
        }
    }

    /// Lower the top-level entry block as a synthetic vacuum-returning function.
    ///
    /// Entry lowering reuses ordinary function-body MIR construction so the MIR
    /// stays target-neutral and unsupported entry contents fail through the same
    /// diagnostics as unsupported function contents.
    fn lower_entry(&mut self, entry: &HirBlock, body_ctx: &BodyLoweringContext<'_, '_>) {
        let vacuum = self.types.primitive(Primitive::Vacuum);
        let numerus = self.types.primitive(Primitive::Numerus);
        let mut entry_return_ty = MirType::semantic(vacuum);
        let context = self.builder_context(body_ctx);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; lowered function count is bounded by u32::MAX"
        )]
        let parent_function_id = MirFunctionId(self.functions.len() as u32);
        let (locals, temps, blocks, errors, synthetics, closure_environments, device_function_metadata) = {
            let mut builder = FunctionBuilder::for_function(self.types, None, context, parent_function_id, None, None);
            builder.shared_bindings = radix_hir::capture::CaptureFacts::for_block(entry).shared;
            let entry_block = builder.fresh_block(entry.span);
            builder.switch_to(entry_block);
            let mut consts_lowered = true;
            for item in &self.unit.hir.items {
                if let HirItemKind::Constant(const_item) = &item.kind
                    && builder
                        .lower_const_binding(item.def_id, const_item, item.span)
                        .is_none()
                {
                    consts_lowered = false;
                    break;
                }
            }
            if consts_lowered {
                builder.lower_raw_entry_args(entry, self.unit.hir.entry_args_name);
                if let Some(expr) = entry
                    .expr
                    .as_deref()
                    .filter(|expr| expr.ty == Some(numerus))
                {
                    match builder.lower_block_expr(entry, expr) {
                        Some(operand) => {
                            entry_return_ty = MirType::semantic(numerus);
                            builder.terminate_open_current(MirTerminatorKind::Return(Some(operand)), entry.span);
                        }
                        None => {
                            builder.terminate_open_current(MirTerminatorKind::Return(None), entry.span);
                        }
                    }
                } else {
                    builder.visit_block(entry);
                    builder.terminate_open_current(MirTerminatorKind::Return(None), entry.span);
                }
            }
            let blocks = builder.finish_blocks();
            let synthetics = builder.take_synthetic_functions();
            let closure_environments = builder.take_closure_environments();
            let device_function_metadata = builder.take_device_function_metadata();
            (
                builder.locals,
                builder.temps,
                blocks,
                builder.errors,
                synthetics,
                closure_environments,
                device_function_metadata,
            )
        };
        self.errors.extend(errors);
        self.closure_environments.extend(closure_environments);
        self.device_function_metadata
            .extend(device_function_metadata);

        self.push_function_with_synthetics(
            MirFunction {
                id: parent_function_id,
                source: None,
                name: None,
                params: Vec::new(),
                locals,
                temps,
                blocks,
                return_ty: entry_return_ty,
                error_ty: None,
                is_async: false,
                is_generator: false,
                shader_stage: None,
                span: entry.span,
            },
            synthetics,
        );
    }
}

fn inherited_lane_binding_locals(
    function: &HirFunction,
    resolver: &radix_semantic::Resolver,
) -> Vec<(DefId, Symbol, TypeId, Span)> {
    let Some(body) = function.body.as_ref() else {
        return Vec::new();
    };
    body.statements
        .iter()
        .filter_map(|statement| {
            let HirStatementKind::Local(local) = &statement.kind else {
                return None;
            };
            if !local.runtime_provided {
                return None;
            }
            let symbol = resolver.get_symbol(local.def_id)?;
            (symbol.kind == radix_semantic::SymbolKind::Lane).then_some((
                local.def_id,
                local.name,
                local.ty?,
                statement.span,
            ))
        })
        .collect()
}

/// Annotation exclusivity check shared by device-role and shader-stage
/// assignment (CTO a7c616ed, extended NF-2: at most one of Entry/Fragment/
/// `is_vertex`/`is_fragment` may be set).
fn annotations_exclusive(function: &HirFunction) -> bool {
    [
        function.is_kernel,
        function.nucleum_fragment,
        function.is_vertex,
        function.is_fragment,
    ]
    .iter()
    .filter(|&&b| b)
    .count()
        <= 1
}

/// Map HIR function flags to MIR device roles.
///
/// `MirDeviceRole` classifies compute entry (`Kernel`) and the nucleum
/// composition role (`Fragment`). Graphics shader stages (`@ vertex`/
/// `@ fragment`) set `is_vertex`/`is_fragment` on `HirFunction`, but the
/// shader-stage identity seam is `MirKernelShaderStage`, not
/// `MirDeviceRole` (CTO a7c616ed: do not conflate execution stage with
/// compute helper role).
fn device_role_for_function(function: &HirFunction) -> Option<MirDeviceRole> {
    if !annotations_exclusive(function) {
        return None;
    }
    if function.is_kernel {
        Some(MirDeviceRole::Kernel)
    } else if function.nucleum_fragment {
        Some(MirDeviceRole::Fragment)
    } else {
        None
    }
}

/// Map HIR stage flags to MIR shader-stage identity.
///
/// `MirKernelShaderStage` distinguishes execution stages from composition
/// roles. Conflicting annotations fail closed to `None`.
pub fn shader_stage_for_function(function: &HirFunction) -> Option<MirKernelShaderStage> {
    if !annotations_exclusive(function) {
        return None;
    }
    if function.is_kernel {
        Some(MirKernelShaderStage::Compute)
    } else if function.is_vertex {
        Some(MirKernelShaderStage::Vertex)
    } else if function.is_fragment {
        Some(MirKernelShaderStage::Fragment)
    } else {
        None
    }
}

/// Body-visible `magnitudo` parameters of one callable, def → name. A value
/// position naming one of these defs carries its compile-time size into MIR
/// (sgr C2a); `typus` parameters are types, never values.
impl MirLowerer<'_> {
    /// `(type parameter, method) → contract method` for one callable's
    /// `implet` bounds (D1.1). The first bound that declares a method wins,
    /// matching the typechecker's lookup order.
    fn bound_method_targets(&self, params: &[HirTypeParam]) -> FxHashMap<(Symbol, Symbol), DefId> {
        let mut targets = FxHashMap::default();
        for param in params {
            let radix_hir::HirTypeParamConstraint::Implet(bounds) = &param.constraint else {
                continue;
            };
            for bound in bounds {
                // D1.4: a call through the ordering bound targets the
                // contract itself; monomorphization picks the witness genus's
                // `compare` or the built-in compare. The contract may be
                // imported, so its method has no local definition.
                if self.unit.types.ordering_contract == Some(bound.contract)
                    && let Some(compare) = self
                        .unit
                        .interner
                        .lookup(radix_semantic::radix_contract::ORDERING_METHOD)
                {
                    targets
                        .entry((param.name, compare))
                        .or_insert(bound.contract);
                    continue;
                }
                let Some(interface) = self
                    .unit
                    .hir
                    .items
                    .iter()
                    .find_map(|item| match &item.kind {
                        HirItemKind::Interface(interface) if item.def_id == bound.contract => Some(interface),
                        _ => None,
                    })
                else {
                    continue;
                };
                for method in &interface.methods {
                    targets
                        .entry((param.name, method.name))
                        .or_insert(method.def_id);
                }
            }
        }
        targets
    }
}

fn size_param_names(params: &[HirTypeParam]) -> FxHashMap<DefId, Symbol> {
    params
        .iter()
        .filter(|param| !param.is_typus())
        .map(|param| (param.def_id, param.name))
        .collect()
}

/// Interned `IndexExpr::Param` node for one size parameter. Signature shapes
/// intern the node whenever the parameter appears in a type, so the ordinary
/// body-visible case resolves; a parameter with no node anywhere has no
/// symbolic carrier and the path fails closed.
fn symbolic_size_index(types: &TypeTable, name: Symbol) -> Option<IndexId> {
    (0..types.index_count()).find_map(|index| {
        #[expect(clippy::cast_possible_truncation, reason = "IndexId is a u32 arena index by design")]
        let id = IndexId(index as u32);
        matches!(types.get_index(id), IndexExpr::Param(found) if *found == name).then_some(id)
    })
}

fn index_contains_runtime_extent(types: &TypeTable, index: IndexId, seen: &mut FxHashSet<IndexId>) -> bool {
    index_contains_extent(types, index, false, seen)
}

/// As [`index_contains_runtime_extent`], but a symbolic `size` parameter also
/// counts: lowering resolves a witnessed parameter to a run-time tensor axis,
/// so the shape array type must exist before that resolution is known.
fn index_may_contain_runtime_extent(types: &TypeTable, index: IndexId, seen: &mut FxHashSet<IndexId>) -> bool {
    index_contains_extent(types, index, true, seen)
}

fn index_contains_extent(
    types: &TypeTable,
    index: IndexId,
    param_is_runtime: bool,
    seen: &mut FxHashSet<IndexId>,
) -> bool {
    if !seen.insert(index) {
        return false;
    }
    match types.get_index(index) {
        IndexExpr::Runtime(_) => true,
        IndexExpr::Param(_) => param_is_runtime,
        IndexExpr::Affine { base, .. } => index_contains_extent(types, *base, param_is_runtime, seen),
        IndexExpr::Divide { numerator, .. } => index_contains_extent(types, *numerator, param_is_runtime, seen),
        IndexExpr::Tuple(dimensions) => dimensions
            .iter()
            .any(|dimension| index_contains_extent(types, *dimension, param_is_runtime, seen)),
        IndexExpr::Literal(_) | IndexExpr::Infer(_) | IndexExpr::Unspecified => false,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct LocalBinding {
    local: MirLocalId,
    ty: MirType,
}

pub struct OpenBlock {
    id: MirBlockId,
    pub statements: Vec<MirStatement>,
    pub terminator: Option<MirTerminator>,
    span: Span,
}

#[derive(Debug, Clone, Copy)]
struct BreakableContext {
    perge_target: Option<MirBlockId>,
    /// Exit edge for `rumpe` (and breakable `fac` fall-through).
    rumpe_target: MirBlockId,
    /// Whether any `rumpe` actually targeted this scope's exit edge. Used to
    /// decide if the after-block stays live when the body's straight-line
    /// fall-through is dead (e.g. a body whose only effect is `rumpe`).
    rumpe_used: bool,
}

/// Active `reducta via summa` sequential fold: a valued `redde` inside the
/// term body adds into `accumulator` with `+` and continues the iteration
/// loop instead of returning from the enclosing function.
/// Exact temporaries for one bucket; no per-operation store policy applies.
#[derive(Clone)]
struct ReductionBucket {
    accumulator: MirTempId,
    score: MirTempId,
    seen: MirTempId,
    coordinates: Vec<MirTempId>,
}

#[derive(Clone)]
struct ReductionFoldContext {
    op: radix_hir::HirReductionOp,
    term_ty: MirType,
    accumulator_ty: MirType,
    buckets: Vec<ReductionBucket>,
    coordinates: Vec<MirLocalId>,
    coordinate_ty: MirType,
    group: Option<HirExpression>,
    group_dims: Vec<usize>,
    error_tensor: Option<MirOperand>,
    /// The term binder's local: the element a `First` fold stores when its
    /// predicate holds. Unused by every other operator.
    element: Option<MirLocalId>,
}

#[derive(Debug, Clone, Copy)]
struct FilumCapture {
    term_result: MirLocalId,
    ty: MirType,
}

/// An open handled block that carries device fault-delivery markers (U9-5).
#[derive(Debug, Clone, Copy)]
struct DeviceBlockContext {
    /// `breakables.len()` when the block opened: a `rumpe`/`perge` leaves the
    /// block exactly when the target breakable was pushed at or before this
    /// depth.
    breakables_len: usize,
    omnia: bool,
}

#[derive(Debug, Clone)]
struct HandlerContext {
    error_place: MirPlace,
    error_ty: MirType,
    error_block: MirBlockId,
    /// DFS push order of this capture boundary (shared counter with
    /// `TrapContext`), so mixed `fac { capta { … } }` nesting routes each
    /// alternate exit to the innermost boundary.
    depth: usize,
}

/// Active `capta { ... }` capture boundary (UT): failable calls and `iace`
/// inside the body lower to `TryCall` whose error block wraps the payload
/// into the trap carrier (the union-typed destination) instead of entering a
/// cape body or reaching `ReturnError`.
#[derive(Debug, Clone)]
struct TrapContext {
    /// Union-typed destination the wrapped result is assigned into.
    destination: MirPlace,
    /// Join block both arms converge on.
    join_block: MirBlockId,
    /// Whether any error wrap block already targets `join_block` (keeps the
    /// reachability decision honest when the body itself diverges).
    error_paths: bool,
    /// DFS push order (shared counter with `HandlerContext`).
    depth: usize,
}

#[derive(Debug, Clone)]
struct ProviderImport {
    kind: MirProviderKind,
    module: Vec<Symbol>,
    item: Symbol,
    /// Package-resolved file-interface call targets (`verb → synthesized
    /// external-definition id`) when the import resolves to a sibling
    /// user-code module under package identity context (D-PA1). `None` keeps
    /// the provider route (raw import spelling).
    file_interface_functions: Option<FxHashMap<Symbol, DefId>>,
}

#[derive(Debug, Clone, Copy)]
struct MethodTarget {
    def_id: DefId,
}

#[derive(Debug, Clone)]
struct ImportedMethodProvider {
    provider: ProviderImport,
    canonical_name: Symbol,
}

/// One callee parameter, snapshotted out of [`FunctionBuilderContext`]'s
/// `function_params` before call lowering mutably borrows the builder.
#[derive(Clone, Copy)]
struct CallableParamLowerMeta<'a> {
    /// Declared parameter type; `None` for a synthetic receiver slot.
    ty: Option<TypeId>,
    optional: bool,
    default: Option<&'a HirExpression>,
}

/// Immutable whole-unit facts needed while lowering one function body.
///
/// This context is intentionally cloned into each [`FunctionBuilder`] instead of
/// letting builders mutate global lowering state. Per-function lowering can then
/// accumulate local errors and CFG state while still resolving function errors,
/// aggregate fields, enum payload names, and provider imports consistently.
pub struct FunctionBuilderContext<'a> {
    interner: Option<&'a Interner>,
    empty_textus: Symbol,
    diagnostic_format_templates: [Symbol; 3],
    diagnostic_separator: Symbol,
    diagnostic_nihil: Symbol,
    /// Lockstep `itera ex` runtime admission message: the
    /// fail-closed equal-length guard's panic textus.
    diagnostic_lockstep_length_mismatch: Symbol,
    /// Failed `numerus ↦ Ordo` payload textus (D9.4).
    diagnostic_ordo_no_member: Symbol,
    diagnostic_potentia_negative_exponent: Symbol,
    diagnostic_range_step_not_positive: Symbol,
    diagnostic_map_changed_size: Symbol,
    diagnostic_ponde_invalid_index: Symbol,
    diagnostic_accipe_invalid_index: Symbol,
    diagnostic_at_out_of_range: Symbol,
    /// The list `slice` fault payload template.
    diagnostic_slice_out_of_range: Symbol,
    /// The list `take` fault payload template.
    diagnostic_take_out_of_range: Symbol,
    diagnostic_ranged_push_past_maximum: Symbol,
    diagnostic_ranged_remove_below_minimum: Symbol,
    /// A `⇥` kernel exists in the unit: handled blocks emit the device
    /// fault-delivery markers (U9-5).
    fault_delivery: bool,
    function_errors: Arc<FxHashMap<DefId, MirType>>,
    function_async: Arc<FxHashMap<DefId, bool>>,
    function_generator: Arc<FxHashMap<DefId, bool>>,
    function_params: Arc<FxHashMap<DefId, Vec<context::CallableParamMeta<'a>>>>,
    imported_function_params: Arc<FxHashMap<DefId, Vec<ImportedCallableParam>>>,
    tensor_edge_policies: Arc<radix_hir::TensorEdgePolicyTable>,
    pending_tensor_limes: Arc<Mutex<Vec<PendingTensorLimes>>>,
    shape_calls: Arc<FxHashMap<radix_hir::HirId, Vec<ShapeRelation>>>,
    shape_functions: Arc<FxHashMap<DefId, Vec<ShapeRelation>>>,
    call_size_bindings: Arc<FxHashMap<radix_hir::HirId, Vec<RuntimeSizeBinding>>>,
    shape_stores: Arc<FxHashMap<radix_hir::HirId, Vec<ShapeRelation>>>,
    /// Index accesses the checker proved in range: lowered as
    /// [`MirProjection::ProvenIndex`]. Absent means checked.
    proven_indexes: Arc<FxHashSet<radix_hir::HirId>>,
    function_size_bindings: Arc<FxHashMap<DefId, Vec<RuntimeSizeBinding>>>,
    structs: Arc<FxHashMap<DefId, Vec<&'a HirField>>>,
    /// Producer-side defaults for imported class fields.
    imported_struct_defaults: Arc<FxHashMap<DefId, Vec<(Symbol, HirExpression)>>>,
    /// Field-name → `MirType` metadata for EVERY struct the analysis knows
    /// (local + imported file-interface structs). Local structs are also in
    /// [`Self::structs`]; imported structs exist only here. Aggregate
    /// materialization falls back to this map for zero-init of imported
    /// struct-typed values (FMIR e2e-hardening CTO-1).
    struct_field_types: Arc<FxHashMap<DefId, FxHashMap<radix_lexer::Symbol, MirType>>>,
    variant_parents: Arc<FxHashMap<DefId, DefId>>,
    /// Enum declaration → ordered variant defs (local + imported file-interface
    /// enums). The no-initializer `varia` zero-init picks the first variant.
    enum_variants: Arc<FxHashMap<DefId, Vec<DefId>>>,
    enum_type_params: Arc<FxHashMap<DefId, Vec<Symbol>>>,
    pub variant_fields: Arc<FxHashMap<DefId, Vec<radix_lexer::Symbol>>>,
    variant_field_tys: Arc<FxHashMap<DefId, FxHashMap<radix_lexer::Symbol, MirType>>>,
    /// Registry conversion rows keyed by ordered pair `(source →
    /// destination)`: the coherence registry's user rows (CTR-06). The
    /// failable-call recovery path uses these to re-select the registered
    /// function when a package rewrite dropped the call-site conversion.
    conversion_rows: Arc<FxHashMap<ConversionPair, DefId>>,
    sermo_def: Option<DefId>,
    meus_def: Option<DefId>,
    tuus_def: Option<DefId>,
    scrinium_def: Option<DefId>,
    top_level_consts: Arc<FxHashMap<DefId, &'a HirConst>>,
    /// Static (`generis`) genus fields by `(struct, field)`: a read through the
    /// type name inlines the field's initializer.
    static_fields: Arc<FxHashMap<(DefId, Symbol), &'a HirField>>,
    /// Static (`generis`) fields of imported genus exports by `(struct, field)`:
    /// the interface's folded initializer, inlined by a `module.Tab.Z` read.
    imported_static_fields: Arc<FxHashMap<(DefId, Symbol), HirExpression>>,
    imported_constants: Arc<FxHashMap<(DefId, Symbol), InterfaceConstValue>>,
    provider_imports: Arc<FxHashMap<DefId, ProviderImport>>,
    imported_type_members: Arc<FxHashMap<(DefId, Symbol), MirType>>,
    method_targets: Arc<FxHashMap<(DefId, Symbol), MethodTarget>>,
    creo_hooks: Arc<FxHashMap<DefId, DefId>>,
    interface_implementors: Arc<FxHashMap<DefId, Vec<(DefId, TypeId)>>>,
    imported_method_providers: Arc<FxHashMap<(DefId, Symbol), ImportedMethodProvider>>,
    gpu_builtins: Arc<FxHashMap<DefId, MirKernelBuiltin>>,
    cli_parsed_record_fields_by_type: Arc<FxHashMap<TypeId, Vec<MirRuntimeRecordField>>>,
    /// Descriptor-derived CLI record fields keyed by `argumenta` record type;
    /// the shared default-path fallback when the caller supplied no parsed
    /// runtime fields for a CLI record local.
    cli_record_fields_by_type: Arc<FxHashMap<TypeId, Vec<MirRuntimeRecordField>>>,
    /// S8.2: CLI-record locals/params are constructed from runtime-filled
    /// temps the emitted LLVM adapter seeds from the static descriptor parse
    /// table (the compile-time field values are only shape placeholders).
    cli_adapter_active: bool,
    /// How kernels lower for this lowering call (see [`KernelLowering`]).
    kernel_lowering: KernelLowering,
}

impl<'a> FunctionBuilderContext<'a> {
    /// Borrow-free snapshot of callee param metadata for call lowering.
    ///
    /// `lower_direct_call_args` must lower default expressions while mutably
    /// borrowing the builder, so it cannot hold an immutable borrow of
    /// `function_params` on `self`. Extract the small
    /// `(type, optional, default)` triples first instead of cloning the full
    /// metadata vec.
    fn callable_param_lower_meta(&self, callee: DefId) -> Option<Vec<CallableParamLowerMeta<'a>>> {
        self.function_params.get(&callee).map(|params| {
            params
                .iter()
                .map(|param| CallableParamLowerMeta { ty: param.ty, optional: param.optional, default: param.default })
                .collect()
        })
    }

    pub fn empty() -> Self {
        Self {
            interner: None,
            empty_textus: Symbol(0),
            diagnostic_format_templates: [Symbol(0); 3],
            diagnostic_separator: Symbol(0),
            diagnostic_nihil: Symbol(0),
            diagnostic_lockstep_length_mismatch: Symbol(0),
            diagnostic_ordo_no_member: Symbol(0),
            diagnostic_potentia_negative_exponent: Symbol(0),
            diagnostic_range_step_not_positive: Symbol(0),
            diagnostic_map_changed_size: Symbol(0),
            diagnostic_ponde_invalid_index: Symbol(0),
            diagnostic_accipe_invalid_index: Symbol(0),
            diagnostic_at_out_of_range: Symbol(0),
            diagnostic_slice_out_of_range: Symbol(0),
            diagnostic_take_out_of_range: Symbol(0),
            diagnostic_ranged_push_past_maximum: Symbol(0),
            diagnostic_ranged_remove_below_minimum: Symbol(0),
            fault_delivery: false,
            function_errors: Arc::new(FxHashMap::default()),
            function_async: Arc::new(FxHashMap::default()),
            function_generator: Arc::new(FxHashMap::default()),
            function_params: Arc::new(FxHashMap::default()),
            imported_function_params: Arc::new(FxHashMap::default()),
            tensor_edge_policies: Arc::new(radix_hir::TensorEdgePolicyTable::default()),
            pending_tensor_limes: Arc::new(Mutex::new(Vec::new())),
            shape_calls: Arc::new(FxHashMap::default()),
            shape_functions: Arc::new(FxHashMap::default()),
            call_size_bindings: Arc::new(FxHashMap::default()),
            shape_stores: Arc::new(FxHashMap::default()),
            proven_indexes: Arc::new(FxHashSet::default()),
            function_size_bindings: Arc::new(FxHashMap::default()),
            structs: Arc::new(FxHashMap::default()),
            imported_struct_defaults: Arc::new(FxHashMap::default()),
            struct_field_types: Arc::new(FxHashMap::default()),
            variant_parents: Arc::new(FxHashMap::default()),
            enum_variants: Arc::new(FxHashMap::default()),
            enum_type_params: Arc::new(FxHashMap::default()),
            variant_fields: Arc::new(FxHashMap::default()),
            variant_field_tys: Arc::new(FxHashMap::default()),
            conversion_rows: Arc::new(FxHashMap::default()),
            sermo_def: None,
            meus_def: None,
            tuus_def: None,
            scrinium_def: None,
            top_level_consts: Arc::new(FxHashMap::default()),
            static_fields: Arc::new(FxHashMap::default()),
            imported_static_fields: Arc::new(FxHashMap::default()),
            imported_constants: Arc::new(FxHashMap::default()),
            provider_imports: Arc::new(FxHashMap::default()),
            imported_type_members: Arc::new(FxHashMap::default()),
            method_targets: Arc::new(FxHashMap::default()),
            creo_hooks: Arc::new(FxHashMap::default()),
            interface_implementors: Arc::new(FxHashMap::default()),
            imported_method_providers: Arc::new(FxHashMap::default()),
            gpu_builtins: Arc::new(FxHashMap::default()),
            cli_parsed_record_fields_by_type: Arc::new(FxHashMap::default()),
            cli_record_fields_by_type: Arc::new(FxHashMap::default()),
            cli_adapter_active: false,
            kernel_lowering: KernelLowering::Device,
        }
    }
}

/// Collect top-level constant paths before CFG lowering so each cached local
/// receives its assignment in the function entry block.
struct TopLevelConstCollector<'a> {
    top_level_consts: &'a FxHashMap<DefId, &'a HirConst>,
    found: Vec<DefId>,
    seen: FxHashSet<DefId>,
}

impl HirVisitor for TopLevelConstCollector<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Path(def_id) = expr.kind
            && self.top_level_consts.contains_key(&def_id)
            && self.seen.insert(def_id)
        {
            self.found.push(def_id);
        }
        walk_expr(self, expr);
    }
}

/// Stateful builder for one MIR function.
///
/// The builder translates typed HIR body structure into MIR locals, temporaries,
/// values, and basic blocks. Its mutable state models the current CFG frontier:
/// `current` is `None` after a terminator, `breakables` records `perge`/`rumpe`
/// targets, and `handlers` records active alternate-exit handlers. Any attempt
/// to keep emitting after a block is sealed becomes a lowering error.
/// One record tensor element an assignment target reaches through: the
/// receiver and coordinate list it was read with, and the decoded copy the
/// target's field store mutates (see `FunctionBuilder::element_writebacks`).
pub(crate) struct ElementWriteback {
    receiver: MirOperand,
    index: MirOperand,
    element: MirPlace,
}

pub struct FunctionBuilder<'a> {
    types: &'a TypeTable,
    error_ty: Option<MirType>,
    pub context: FunctionBuilderContext<'a>,
    pub bindings: FxHashMap<DefId, LocalBinding>,
    /// While lowering `casu A aut B`, every alternative binds the same
    /// `DefId`s: payload bindings reuse the local an earlier alternative made.
    shared_pattern_bindings: bool,
    params: Vec<MirParam>,
    locals: Vec<MirLocalDecl>,
    pub temps: Vec<MirTemp>,
    pub blocks: Vec<OpenBlock>,
    current: Option<MirBlockId>,
    /// The block that received the most recent terminator. A body that ends in
    /// `mori` leaves this as the diverging block even when the block was
    /// created before the body began.
    last_terminated: Option<MirBlockId>,
    breakables: Vec<BreakableContext>,
    reduction_fold: Option<ReductionFoldContext>,
    /// Active `reducta via summa … filum` term capture (SFR-5): a valued
    /// `redde` inside the captured term body stores the term value into
    /// `term_result` and
    /// exits the body — the statements land in the filum plan entry, not the
    /// kernel's inline flow.
    filum_capture: Option<FilumCapture>,
    handlers: Vec<HandlerContext>,
    /// Handled blocks open now that carry device markers; empty unless the
    /// unit declares a `⇥` kernel.
    device_blocks: Vec<DeviceBlockContext>,
    /// Active `capta` trap boundaries (UT); see [`TrapContext`].
    traps: Vec<TrapContext>,
    /// Monotonic DFS order shared by handler and trap pushes, so
    /// `innermost_capture` picks the lexically innermost boundary.
    capture_depth: usize,
    materializing_consts: FxHashSet<DefId>,
    /// `object!` method receivers lowered inside the call being lowered:
    /// `(storage place, asserted payload temp, storage type)`. A receiver
    /// write lands on the payload temp, so a mutating call stores the payload
    /// back into the storage afterwards (`lower_method_call`).
    asserted_receivers: Vec<(MirPlace, MirPlace, MirType)>,
    pub errors: Vec<MirError>,
    next_value: u32,
    synthetic_functions: Vec<MirFunction>,
    /// Synthesized check helpers keyed by signature: byte-identical helpers
    /// are emitted once per lowering instead of once per call site.
    synthetic_helpers: FxHashMap<SyntheticHelperKey, MirFunctionId>,
    closure_environments: Vec<MirClosureEnvironment>,
    device_function_metadata: MirDeviceFunctionMetadataMap,
    closure_temp_callees: FxHashMap<MirTempId, MirClosureCallee>,
    closure_local_callees: FxHashMap<MirLocalId, MirClosureCallee>,
    captured_bindings: FxHashMap<DefId, MirPlace>,
    captured_binding_metadata: FxHashMap<DefId, (Symbol, MirType)>,
    /// While an assignment target is lowered: the record tensor elements the
    /// target's place reaches through. A record row decodes to a fresh value
    /// on every read, so a store into one of its fields lands in that copy
    /// and the store of the whole element back into the tensor finishes the
    /// write. `None` outside an assignment target.
    element_writebacks: Option<Vec<ElementWriteback>>,
    /// `var` locals of this body that closures capture and that are rebound
    /// somewhere: their storage is a shared cell (operator ruling 2026-09-24,
    /// unmarked closures share the enclosing scope).
    shared_bindings: FxHashSet<DefId>,
    /// In-scope shared bindings → the place holding the cell itself (the
    /// declaring local, or a closure-environment capture slot). Paths read and
    /// write the content through `captured_bindings` (`cell.SharedCell`);
    /// nested closures capture the cell from here.
    shared_cells: FxHashMap<DefId, MirPlace>,
    /// Enclosing callable's `magnitudo` parameters, def → name. A body path
    /// to one of these defs lowers to the symbolic size operand, never a
    /// runtime local (sgr C2a).
    size_params: FxHashMap<DefId, Symbol>,
    /// Enclosing callable's contract-bound methods (D1.1), keyed by
    /// `(type parameter, method)`: the contract method a `T` receiver call
    /// targets until monomorphization picks the witness's own method.
    bound_methods: FxHashMap<(Symbol, Symbol), DefId>,
    parent_function_id: MirFunctionId,
    device_role: Option<MirDeviceRole>,
    pub shader_stage: Option<MirKernelShaderStage>,
    /// `copy` parameters (D-6): `(param local, owned duplicate local, type,
    /// span)`. The body binds the duplicate; entry emits the deep copy.
    copy_param_entries: Vec<(MirLocalId, MirLocalId, MirType, Span)>,
    function_shape_relations: Vec<ShapeRelation>,
    formal_parameter_offset: u32,
    size_param_sources: FxHashMap<DefId, IndexId>,
    /// The pre-scanned top-level `thread lane` region of a device kernel body.
    lane_region: Option<lane_form::LaneRegion>,
}

impl<'a> FunctionBuilder<'a> {
    pub fn new(types: &'a TypeTable) -> Self {
        Self::for_function(types, None, FunctionBuilderContext::empty(), MirFunctionId(0), None, None)
    }

    fn for_function(
        types: &'a TypeTable,
        error_ty: Option<MirType>,
        context: FunctionBuilderContext<'a>,
        parent_function_id: MirFunctionId,
        device_role: Option<MirDeviceRole>,
        shader_stage: Option<MirKernelShaderStage>,
    ) -> Self {
        Self {
            types,
            error_ty,
            context,
            bindings: FxHashMap::default(),
            shared_pattern_bindings: false,
            params: Vec::new(),
            locals: Vec::new(),
            temps: Vec::new(),
            blocks: Vec::new(),
            current: None,
            last_terminated: None,
            breakables: Vec::new(),
            reduction_fold: None,
            filum_capture: None,
            handlers: Vec::new(),
            device_blocks: Vec::new(),
            traps: Vec::new(),
            capture_depth: 0,
            materializing_consts: FxHashSet::default(),
            asserted_receivers: Vec::new(),
            errors: Vec::new(),
            next_value: 0,
            synthetic_functions: Vec::new(),
            synthetic_helpers: FxHashMap::default(),
            closure_environments: Vec::new(),
            device_function_metadata: MirDeviceFunctionMetadataMap::default(),
            closure_temp_callees: FxHashMap::default(),
            closure_local_callees: FxHashMap::default(),
            captured_bindings: FxHashMap::default(),
            captured_binding_metadata: FxHashMap::default(),
            element_writebacks: None,
            shared_bindings: FxHashSet::default(),
            shared_cells: FxHashMap::default(),
            size_params: FxHashMap::default(),
            bound_methods: FxHashMap::default(),
            parent_function_id,
            device_role,
            shader_stage,
            copy_param_entries: Vec::new(),
            function_shape_relations: Vec::new(),
            formal_parameter_offset: 0,
            size_param_sources: FxHashMap::default(),
            lane_region: None,
        }
    }

    /// Whether this body lowers in the device-translatable form: it is a
    /// device function and the route does not ask for host kernel lowering.
    fn is_device_lowering(&self) -> bool {
        self.device_role.is_some() && self.context.kernel_lowering == KernelLowering::Device
    }

    /// Whether the route lowers kernels as ordinary host functions. Unlike
    /// [`Self::is_device_lowering`], this holds for every body of the
    /// lowering call, kernel or not.
    fn is_host_kernel_lowering(&self) -> bool {
        self.context.kernel_lowering == KernelLowering::Host
    }

    /// The run-time witness recorded for a body-visible `size` parameter: the
    /// tensor argument axis that carries its value (`fn decode<size N>(
    /// tensor<f32, [N]> proto, ..)` records `proto`'s first axis). `None`
    /// means the parameter has no value at the use site.
    fn size_param_source(&self, symbol: Symbol) -> Option<IndexId> {
        let def_id = self
            .size_params
            .iter()
            .find_map(|(def_id, name)| (*name == symbol).then_some(*def_id))?;
        self.size_param_sources.get(&def_id).copied()
    }

    /// Whether a declared extent is bound at run time in this body. A
    /// `Runtime` extent always is, on every path. A `size` parameter counts
    /// through its recorded tensor-axis witness only when
    /// `witnessed_size_params` says this path reads witnesses (the octeti
    /// conversion, the same resolution [`Self::lower_shape_expr`] applies
    /// there); elsewhere a parameter stays symbolic, as
    /// [`index_contains_runtime_extent`] treats it, so package finalization
    /// can substitute the axis's `ShapeSize` term (GSF-2).
    fn body_index_contains_runtime_extent(
        &self,
        index: IndexId,
        seen: &mut FxHashSet<IndexId>,
        witnessed_size_params: bool,
    ) -> bool {
        if !seen.insert(index) {
            return false;
        }
        match self.types.get_index(index) {
            IndexExpr::Param(symbol) if witnessed_size_params => self
                .size_param_source(*symbol)
                .is_some_and(|source| self.body_index_contains_runtime_extent(source, seen, witnessed_size_params)),
            IndexExpr::Param(_) | IndexExpr::Literal(_) | IndexExpr::Infer(_) | IndexExpr::Unspecified => false,
            IndexExpr::Runtime(_) => true,
            IndexExpr::Affine { base, .. } => {
                self.body_index_contains_runtime_extent(*base, seen, witnessed_size_params)
            }
            IndexExpr::Divide { numerator, .. } => {
                self.body_index_contains_runtime_extent(*numerator, seen, witnessed_size_params)
            }
            IndexExpr::Tuple(dimensions) => dimensions
                .iter()
                .any(|dimension| self.body_index_contains_runtime_extent(*dimension, seen, witnessed_size_params)),
        }
    }

    /// The innermost active `capta` trap when it is the innermost capture
    /// boundary (deeper than any active `cape` handler), marking that an
    /// error wrap block will target the trap join. `None` keeps the existing
    /// handler / propagation routing.
    fn take_innermost_trap(&mut self) -> Option<TrapContext> {
        let handler_depth = self.handlers.last().map_or(0, |handler| handler.depth);
        let trap = self.traps.last_mut()?;
        if trap.depth <= handler_depth {
            return None;
        }
        let snapshot = trap.clone();
        trap.error_paths = true;
        Some(snapshot)
    }

    pub fn add_param(&mut self, def_id: DefId, name: radix_lexer::Symbol, ty: TypeId, mode: HirParamMode, span: Span) {
        let local = self.next_local_id();
        // MIR carries access intent in `MirParamMode`; the local slot carries
        // the payload value type so MIR calls, returns, and places stay in the
        // storage domain. Every type-position access marker is a source-level
        // wrapper, including `own`/`copy` even though HIR collapses those to
        // value-owned mode.
        let (copy_param, storage_ty) = match self.normalized_type(ty) {
            Type::Ref(mutability, inner) => (*mutability == SemanticMutability::Copy, *inner),
            _ => (false, ty),
        };
        let mir_ty = MirType::semantic(storage_ty);
        // Keep the MIR ABI mode and local mutability paired. In particular,
        // `in` must remain `MirParamMode::MutRef` so call frames can preserve a
        // scalar mutable borrow instead of treating it as an owned value.
        let (mir_mode, mutable) = match mode {
            HirParamMode::Owned | HirParamMode::Axis => (MirParamMode::Owned, false),
            HirParamMode::De => (MirParamMode::Ref, false),
            HirParamMode::In => (MirParamMode::MutRef, true),
            HirParamMode::Ex => (MirParamMode::Move, false),
        };
        self.params
            .push(MirParam { local, name: Some(name), ty: mir_ty, mode: mir_mode, span });
        self.locals
            .push(MirLocalDecl { id: local, name: Some(name), ty: mir_ty, mutable, span });
        if copy_param {
            // `copy` (D-6): the callee receives a fresh owned duplicate at
            // entry; the caller's value is never shared with the body.
            let duplicate = self.next_local_id();
            self.locals
                .push(MirLocalDecl { id: duplicate, name: Some(name), ty: mir_ty, mutable: true, span });
            self.copy_param_entries
                .push((local, duplicate, mir_ty, span));
            self.bindings
                .insert(def_id, LocalBinding { local: duplicate, ty: mir_ty });
            return;
        }
        self.bindings
            .insert(def_id, LocalBinding { local, ty: mir_ty });
    }

    /// Emit the entry deep copy for every `copy` parameter (D-6).
    fn emit_copy_param_entries(&mut self) {
        for (param, duplicate, ty, span) in std::mem::take(&mut self.copy_param_entries) {
            self.runtime_call_to_place(
                Some(MirPlace::local(duplicate)),
                MirIntrinsic::Duplicate,
                vec![MirOperand::Place(MirPlace::local(param))],
                ty,
                span,
            );
        }
    }

    fn add_lane_param(&mut self, def_id: DefId, name: radix_lexer::Symbol, ty: TypeId, span: Span) {
        self.add_lane_param_with_mode(def_id, name, ty, span, MirParamMode::Lane);
    }

    fn add_inherited_lane_param(&mut self, def_id: DefId, name: radix_lexer::Symbol, ty: TypeId, span: Span) {
        self.add_lane_param_with_mode(def_id, name, ty, span, MirParamMode::LaneInherited);
    }

    fn add_lane_param_with_mode(
        &mut self,
        def_id: DefId,
        name: radix_lexer::Symbol,
        ty: TypeId,
        span: Span,
        mode: MirParamMode,
    ) {
        let local = self.next_local_id();
        let mir_ty = MirType::semantic(ty);
        self.params
            .push(MirParam { local, name: Some(name), ty: mir_ty, mode, span });
        self.locals
            .push(MirLocalDecl { id: local, name: Some(name), ty: mir_ty, mutable: false, span });
        self.bindings
            .insert(def_id, LocalBinding { local, ty: mir_ty });
    }

    /// Inject synthetic `MirLocalDecl`s (name, type, mutable) for a
    /// `@vertex` function, and create `DefId`→local bindings so HIR paths
    /// in the function body (e.g. `position`, `out_position`) resolve to
    /// these injected MIR locals.
    ///
    /// The item list is pre-built in [`lower_analyzed_unit`].
    pub fn add_test_binding(&mut self, def_id: DefId, ty: MirType, mutable: bool) -> MirLocalId {
        let local = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: local, name: None, ty, mutable, span: Span::default() });
        self.bindings.insert(def_id, LocalBinding { local, ty });
        local
    }

    fn lower_body(&mut self, body: &HirBlock) -> Vec<MirBlock> {
        self.lower_body_with_cli_args(body, None)
    }

    fn lower_body_with_cli_args(&mut self, body: &HirBlock, cli_args: Option<&radix_hir::HirParam>) -> Vec<MirBlock> {
        self.shared_bindings = radix_hir::capture::CaptureFacts::for_block(body).shared;
        let entry = self.fresh_block(body.span);
        self.switch_to(entry);
        // `None` means the check already recorded its diagnostic in `self.errors`.
        self.emit_function_entry_shape_check(body.span);
        self.emit_copy_param_entries();
        if let Some(cli_args) = cli_args {
            self.add_runtime_record_local(cli_args);
        }
        self.materialize_referenced_top_level_consts(body);
        self.visit_block(body);
        self.terminate_open_current(MirTerminatorKind::Return(None), body.span);
        self.finish_blocks()
    }

    /// Materialize each top-level constant used by this body before lowering
    /// any control flow. A lazy first-use binding is not safe when that first
    /// path sits inside a branch or loop that can be skipped: later uses share
    /// the cached local but would then read it before its assignment.
    fn materialize_referenced_top_level_consts(&mut self, body: &HirBlock) {
        let mut collector = TopLevelConstCollector {
            top_level_consts: &self.context.top_level_consts,
            found: Vec::new(),
            seen: FxHashSet::default(),
        };
        collector.visit_block(body);

        for def_id in collector.found {
            let Some(konst) = self.context.top_level_consts.get(&def_id).copied() else {
                continue;
            };
            self.lower_top_level_const_binding(def_id, konst, body.span);
        }
    }

    fn add_runtime_record_local(&mut self, param: &radix_hir::HirParam) {
        let mir_ty = MirType::semantic(param.ty);
        let id = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id, name: Some(param.name), ty: mir_ty, mutable: true, span: param.span });
        if matches!(self.types.get(param.ty), Type::Record(_)) {
            self.lower_cli_record_initializer(id, param.ty, mir_ty, param.span);
        } else {
            self.errors
                .push(MirError::unsupported(param.span, "non-record CLI argument binding"));
        }
        self.bindings
            .insert(param.def_id, LocalBinding { local: id, ty: mir_ty });
    }

    /// S8.2: one runtime-filled temp per record field, in the record type's
    /// field order, so the emitted CLI adapter can seed the record construct
    /// from the static descriptor parse table.
    fn runtime_adapter_record_fields(&mut self, record_ty: TypeId, span: Span) -> Vec<MirNamedOperand> {
        let fields = match self.types.get(record_ty) {
            Type::Record(fields) => fields.clone(),
            _ => return Vec::new(),
        };
        fields
            .into_iter()
            .map(|(name, field_ty)| {
                let ty = MirType::semantic(field_ty);
                let temp = self.push_temp(ty, span);
                MirNamedOperand { name, value: MirOperand::Temp(temp) }
            })
            .collect()
    }

    fn runtime_record_fields(&mut self, fields: Vec<MirRuntimeRecordField>, span: Span) -> Vec<MirNamedOperand> {
        fields
            .into_iter()
            .map(|field| {
                let value = match field.value {
                    MirRuntimeRecordValue::Operand(value) => value,
                    MirRuntimeRecordValue::Array { ty, items } => self.construct_temp(
                        MirAggregateKind::Array,
                        MirAggregateFields::Ordered(items.into_iter().map(MirAggregateItem::Operand).collect()),
                        ty,
                        span,
                    ),
                    MirRuntimeRecordValue::Rows { ty, rows } => {
                        let row_ty = match self.types.get(ty.semantic_id()) {
                            Type::Array(element) => MirType::semantic(*element),
                            _ => ty,
                        };
                        let items = rows
                            .into_iter()
                            .map(|row| {
                                let items = row.into_iter().map(MirAggregateItem::Operand).collect();
                                MirAggregateItem::Operand(self.construct_temp(
                                    MirAggregateKind::Array,
                                    MirAggregateFields::Ordered(items),
                                    row_ty,
                                    span,
                                ))
                            })
                            .collect();
                        self.construct_temp(MirAggregateKind::Array, MirAggregateFields::Ordered(items), ty, span)
                    }
                };
                MirNamedOperand { name: field.name, value }
            })
            .collect()
    }

    /// Descriptor-derived CLI record fields for a record type (shared
    /// default-path fallback). Returns an empty surface for non-CLI record
    /// locals, matching the pre-fallback behavior.
    fn cli_record_fields_for_type(&mut self, ty: TypeId, span: Span) -> Vec<MirNamedOperand> {
        let Some(fields) = self.context.cli_record_fields_by_type.get(&ty).cloned() else {
            return Vec::new();
        };
        self.runtime_record_fields(fields, span)
    }

    /// Lower a CLI-bound record (param or runtime-provided local) into a
    /// constructed record aggregate assigned to `id`. Field sources: S8.2
    /// runtime-filled adapter temps, parsed runtime fields for this local, or
    /// descriptor-derived fields keyed by the record type.
    fn lower_cli_record_initializer(&mut self, id: MirLocalId, ty: TypeId, mir_ty: MirType, span: Span) {
        let fields = if self.context.cli_adapter_active {
            // S8.2: the record reads runtime-filled temps; the emitted adapter
            // seeds them from the typed value table.
            self.runtime_adapter_record_fields(ty, span)
        } else if let Some(fields) = self
            .context
            .cli_parsed_record_fields_by_type
            .get(&ty)
            .cloned()
        {
            self.runtime_record_fields(fields, span)
        } else {
            self.cli_record_fields_for_type(ty, span)
        };
        let value = self.construct_temp(MirAggregateKind::Record, MirAggregateFields::Named(fields), mir_ty, span);
        self.assign(MirPlace::local(id), value, mir_ty, span);
    }

    /// Raw entry args are host-provided, unlike descriptor-backed CLI records.
    /// Bind only the synthesized entry local here; shader runtime locals remain
    /// the device runtime's responsibility.
    fn lower_raw_entry_args(&mut self, entry: &HirBlock, args_name: Option<Symbol>) {
        let Some(statement) = entry.statements.first() else {
            return;
        };
        let HirStatementKind::Local(local) = &statement.kind else {
            return;
        };
        if !local.runtime_provided || Some(local.name) != args_name {
            return;
        }
        let Some(ty) = local.ty else {
            return;
        };
        if !matches!(self.types.get(ty), Type::Array(element) if *element == self.types.primitive(Primitive::Textus)) {
            return;
        }
        if self.lower_local(local, statement.span).is_none() {
            return;
        }
        let binding = self.bindings[&local.def_id];
        let Some((module, name)) = self
            .context
            .interner
            .and_then(|interner| Some((interner.lookup("processus")?, interner.lookup("argv")?)))
        else {
            self.errors
                .push(MirError::unsupported(statement.span, "raw entry args provider identity"));
            return;
        };
        self.runtime_call_to_place(
            Some(MirPlace::local(binding.local)),
            MirIntrinsic::Provider(MirProvider {
                kind: MirProviderKind::Kernel(radix_mir::KernelModule::Processus),
                module: vec![module],
                name,
            }),
            Vec::new(),
            binding.ty,
            statement.span,
        );
    }

    pub fn lower_local(&mut self, local: &HirLocal, span: Span) -> Option<()> {
        if local.runtime_provided && self.bindings.contains_key(&local.def_id) {
            return Some(());
        }

        let Some(ty) = local.ty else {
            self.errors
                .push(MirError::missing_type(span, "local declaration"));
            return None;
        };

        // A `copy` bind elaborates to an exclusive owned value (D-6), and an
        // `own` bind takes the source's value over: the local stores the
        // payload type, like a `copy` / `own` parameter, so field projection
        // and method receivers see the struct, not the access wrapper.
        let copy_bind = self.types.is_copy_view(ty);
        // A `ref` local reads a place: its storage type is the payload too.
        let mir_ty = if copy_bind || self.types.is_own_view(ty) || self.types.is_ref_view(ty) {
            MirType::semantic(self.types.peel_access_marker(ty).1)
        } else {
            MirType::semantic(ty)
        };
        let id = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id, name: Some(local.name), ty: mir_ty, mutable: local.mutable, span });

        if local.mutable && self.shared_bindings.contains(&local.def_id) {
            return self.lower_shared_local(local, id, ty, mir_ty, span);
        }

        if let Some(init) = &local.init {
            if let HirExpressionKind::ConversioAssign { source, recovery, .. } = &init.kind {
                // Typed `↤` initializer (`sit numerus n ↤ "42"`): the node's
                // target is this binding, so the destination is the local we
                // just declared. Convert the source to the declared type and
                // store once — no self-reference to resolve.
                self.lower_conversio_assign_to_destination(
                    source,
                    recovery.as_deref(),
                    MirPlace::local(id),
                    mir_ty,
                    span,
                    init,
                )?;
            } else if let Some(site) = &local.await_store {
                self.lower_await_store(init, MirPlace::local(id), mir_ty, site.clone(), span)?;
            } else if local.size_decl && !local.size_static {
                // A runtime `const size` is a lift store: evaluate its source
                // once, check the scalar witness for non-negativity, then store.
                let value = self.lower_expr_value(init)?;
                self.runtime_call_to_place(
                    None,
                    MirIntrinsic::TensorShapeCheck(MirShapeCheck {
                        relations: Vec::new(),
                        size_bindings: vec![MirShapeBinding {
                            parameter: 0,
                            witness: MirShapeExpr::ScalarArgument { argument: 0 },
                        }],
                    }),
                    vec![value.clone()],
                    MirType::semantic(self.types.primitive(Primitive::Vacuum)),
                    span,
                );
                self.assign(MirPlace::local(id), value, mir_ty, span);
            } else if self.context.shape_stores.contains_key(&init.id)
                && !matches!(init.kind, HirExpressionKind::Call(..) | HirExpressionKind::MethodCall(..))
            {
                let value = self.lower_expr_value(init)?;
                self.emit_store_shape_check(init.id, value.clone(), span)?;
                self.assign(MirPlace::local(id), value, mir_ty, span);
            } else if copy_bind {
                // `copy` bind (D-6): deep-duplicate the initializer; the
                // source stays live and shares nothing with the binding.
                let source = self.lower_expr_value(init)?;
                self.runtime_call_to_place(
                    Some(MirPlace::local(id)),
                    MirIntrinsic::Duplicate,
                    vec![source],
                    mir_ty,
                    span,
                );
            } else {
                self.lower_expr_to_destination(init, MirPlace::local(id), mir_ty)?;
            }
        } else if local.runtime_provided && matches!(self.types.get(ty), Type::Record(_)) {
            self.lower_cli_record_initializer(id, ty, mir_ty, span);
        } else if local.runtime_provided {
            // Runtime-provided vertex shader locals (position, out_position)
            // are bound by the runtime, not by an initializer or zero-init.
        } else if local.mutable {
            // M-007: `varia` without initializer receives a type zero at the decl site.
            let zero = self.type_zero_operand(ty, span)?;
            self.assign(MirPlace::local(id), zero, mir_ty, span);
        }
        // `fixum`/`sit` deferred-init (`!mutable`, no init`) stays unassigned until `←`.
        self.bindings
            .insert(local.def_id, LocalBinding { local: id, ty: mir_ty });
        Some(())
    }

    /// Declare a captured-and-rebound `var` as a fresh shared cell.
    ///
    /// The initializer (or type zero) lowers into a staging temp, then one
    /// `SharedCell` construct stores the cell in the declared local. The
    /// construct runs at every execution of the declaration, so a loop body
    /// gets a fresh cell per iteration. Every path to the binding then goes
    /// through `local.SharedCell`, and closures capture the cell itself.
    fn lower_shared_local(
        &mut self,
        local: &HirLocal,
        id: MirLocalId,
        ty: TypeId,
        mir_ty: MirType,
        span: Span,
    ) -> Option<()> {
        let staging = self.push_temp(mir_ty, span);
        let staging_place = MirPlace::temp(staging);
        if let Some(init) = &local.init {
            if let HirExpressionKind::ConversioAssign { source, recovery, .. } = &init.kind {
                self.lower_conversio_assign_to_destination(
                    source,
                    recovery.as_deref(),
                    staging_place.clone(),
                    mir_ty,
                    span,
                    init,
                )?;
            } else if let Some(site) = &local.await_store {
                self.lower_await_store(init, staging_place.clone(), mir_ty, site.clone(), span)?;
            } else {
                self.lower_expr_to_destination(init, staging_place.clone(), mir_ty)?;
            }
        } else {
            let zero = self.type_zero_operand(ty, span)?;
            self.assign(staging_place.clone(), zero, mir_ty, span);
        }
        self.append_stmt(MirStatement {
            kind: MirStatementKind::Construct {
                destination: MirPlace::local(id),
                aggregate: MirAggregate {
                    kind: MirAggregateKind::SharedCell,
                    ty: mir_ty,
                    fields: MirAggregateFields::Ordered(vec![MirAggregateItem::Operand(MirOperand::Place(
                        staging_place,
                    ))]),
                },
            },
            span,
        });
        let cell = MirPlace::local(id);
        let mut content = cell.clone();
        content.projections.push(MirProjection::SharedCell);
        self.captured_bindings.insert(local.def_id, content);
        self.captured_binding_metadata
            .insert(local.def_id, (local.name, mir_ty));
        self.shared_cells.insert(local.def_id, cell);
        self.bindings
            .insert(local.def_id, LocalBinding { local: id, ty: mir_ty });
        Some(())
    }

    pub fn lower_const_binding(&mut self, item_def_id: DefId, konst: &HirConst, span: Span) -> Option<()> {
        let Some(ty) = konst.ty else {
            self.errors
                .push(MirError::missing_type(span, "top-level const declaration"));
            return None;
        };

        let mir_ty = MirType::semantic(ty);
        let id = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id, name: Some(konst.name), ty: mir_ty, mutable: false, span });
        if let HirExpressionKind::ConversioAssign { source, recovery, .. } = &konst.value.kind {
            // Typed `↤` initializer: the destination is the const binding just
            // declared, so convert the source to its declared type and store once.
            self.lower_conversio_assign_to_destination(
                source,
                recovery.as_deref(),
                MirPlace::local(id),
                mir_ty,
                span,
                &konst.value,
            )?;
        } else {
            self.lower_expr_to_destination(&konst.value, MirPlace::local(id), mir_ty)?;
        }
        self.bindings
            .insert(item_def_id, LocalBinding { local: id, ty: mir_ty });
        Some(())
    }

    fn lower_top_level_const_binding(
        &mut self,
        item_def_id: DefId,
        konst: &HirConst,
        span: Span,
    ) -> Option<LocalBinding> {
        if let Some(binding) = self.bindings.get(&item_def_id).copied() {
            return Some(binding);
        }
        if !self.materializing_consts.insert(item_def_id) {
            self.errors
                .push(MirError::unsupported(span, "recursive top-level const before MIR lowering"));
            return None;
        }

        let Some(ty) = konst.ty else {
            self.errors
                .push(MirError::missing_type(span, "top-level const declaration"));
            self.materializing_consts.remove(&item_def_id);
            return None;
        };

        let mir_ty = MirType::semantic(ty);
        let id = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id, name: Some(konst.name), ty: mir_ty, mutable: false, span });
        let lowered = self.lower_expr_to_destination(&konst.value, MirPlace::local(id), mir_ty);
        self.materializing_consts.remove(&item_def_id);
        lowered?;
        let binding = LocalBinding { local: id, ty: mir_ty };
        self.bindings.insert(item_def_id, binding);
        Some(binding)
    }

    pub fn lower_assignment_expr(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        let HirExpressionKind::Assign(lhs, rhs) = &expr.kind else {
            return self.lower_expr_value(expr);
        };

        if let Some(result) = self.lower_tensor_bracket_set(lhs, rhs, expr.span) {
            return Some(result);
        }
        if let Some(result) = self.lower_union_field_assign(lhs, rhs, expr.span) {
            return result;
        }

        let fallback_ty = self.expr_ty(rhs)?;
        let (place, ty, writebacks) = self.lower_assignment_place_writeback(lhs, fallback_ty)?;
        self.lower_expr_to_destination(rhs, place.clone(), ty)?;
        self.flush_element_writebacks(writebacks, expr.span);
        Some(MirOperand::Place(place))
    }

    /// Lower conversion-directed assignment (`target ↤ source`, U+21A4).
    ///
    /// `↤` is derived sugar for an explicit conversion — `x ↤ e` ≡
    /// `x ← e ↦ typeof(x)` — per the 2026-10-05 operator ruling
    /// (`docs/factory/tensor-conversions/goal.md`, C2). The shared `↦`
    /// conversion owner `lower_conversio` is the single conversion
    /// implementation; no parallel path exists or may be added (the C2
    /// equivalence tests in `lower/conversion_check_test.rs` pin that).
    ///
    /// Eval order matches ordinary `←` exactly, with the conversion inserted
    /// between source evaluation and the store:
    ///   1. destination place evaluated once (receiver/index once),
    ///   2. source evaluated once,
    ///   3. converted once through the shared `↦` conversion owner
    ///      (`lower_conversio`) — `⊥` default runs only on conversion failure,
    ///   4. stored exactly once.
    ///
    /// The result value denotes the stored value (the destination place), the
    /// same contract as ordinary assignment — no `↤`-specific exception.
    /// There is no codegen-only rewrite that duplicates any evaluation.
    fn lower_conversio_assign_expr(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        let HirExpressionKind::ConversioAssign { target, source, recovery, .. } = &expr.kind else {
            return self.lower_expr_value(expr);
        };
        let fallback_ty = self.expr_ty(source)?;
        let (place, ty, writebacks) = self.lower_assignment_place_writeback(target, fallback_ty)?;
        self.lower_conversio_assign_to_destination(source, recovery.as_deref(), place.clone(), ty, expr.span, expr)?;
        self.flush_element_writebacks(writebacks, expr.span);
        Some(MirOperand::Place(place))
    }

    /// Lower the conversion half of `↤` directly into a known destination.
    ///
    /// Used by typed `fixum`/`varia` initializers (and constants) where the
    /// `↤` node's target IS the binding being declared, so there is no
    /// self-reference to resolve: source evaluated once, converted once through
    /// the shared `↦` conversion owner, stored exactly once.
    fn lower_conversio_assign_to_destination(
        &mut self,
        source: &HirExpression,
        recovery: Option<&HirExpression>,
        destination: MirPlace,
        ty: MirType,
        span: Span,
        expr: &HirExpression,
    ) -> Option<()> {
        // `a ↤ x via R`: `expr` is the `↤` node, so the `via` hint travels
        // exactly as the explicit `x ↦ T via R` conversion's params.
        let params: &[Symbol] = match &expr.kind {
            HirExpressionKind::ConversioAssign { params, .. } => params,
            _ => &[],
        };
        let converted = self.lower_conversio(source, ty.semantic_id(), params, recovery, None, expr)?;
        self.assign(destination, converted, ty, span);
        Some(())
    }

    fn lower_tensor_bracket_set(&mut self, lhs: &HirExpression, rhs: &HirExpression, span: Span) -> Option<MirOperand> {
        if self.is_device_lowering() {
            return None;
        }
        let HirExpressionKind::Index(object, index) = &lhs.kind else {
            return None;
        };
        if !object
            .ty
            .is_some_and(|ty| radix_semantic::tensor_bracket::is_coordinate_bracket_receiver(self.types, ty))
        {
            return None;
        }

        let receiver = self.lower_expr_value(object)?;
        let index = self.lower_tensor_index_list_operand(index)?;
        let value = self.lower_expr_value(rhs)?;
        let vacuum = MirType::semantic(self.types.primitive(Primitive::Vacuum));
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorSet),
            vec![receiver, index, value],
            vacuum,
            span,
        ))
    }

    fn lower_inc_dec_stmt(&mut self, span: Span, inc_dec: &radix_hir::HirIncDec) {
        let Some(fallback_ty) = self.expr_ty(&inc_dec.target) else {
            return;
        };
        let Some((place, ty, writebacks)) = self.lower_assignment_place_writeback(&inc_dec.target, fallback_ty) else {
            return;
        };
        let bin_op = match inc_dec.op {
            radix_hir::HirIncDecOp::Inc => MirBinOp::Add,
            radix_hir::HirIncDecOp::Dec => MirBinOp::Sub,
        };
        let lhs = MirOperand::Place(place.clone());
        let rhs = MirOperand::Constant(MirConstant::Int(1));
        let value = self.assign_temp(MirValueKind::Binary { op: bin_op, lhs, rhs }, ty, span);
        // `place ↑` is a store (d11-6 row 4): the exact successor lands in the
        // slot under the slot's policy, and a failure has the store identity.
        let value = if !self.is_device_lowering() && self.is_integer_store_slot(ty) {
            let slot = match &inc_dec.target.kind {
                HirExpressionKind::Path(_) | HirExpressionKind::Field(..) => place
                    .projections
                    .iter()
                    .rev()
                    .find_map(|projection| match projection {
                        MirProjection::Field(name) => Some(*name),
                        _ => None,
                    })
                    .or_else(|| match place.base {
                        MirPlaceBase::Local(id) => self
                            .locals
                            .iter()
                            .find(|local| local.id == id)
                            .and_then(|local| local.name),
                        MirPlaceBase::Temp(_) => None,
                    }),
                _ => None,
            };
            let site = radix_hir::HirStoreSite {
                kind: radix_hir::HirStoreKind::Assignment,
                slot,
                inferred_from: None,
                inferred_text: None,
            };
            self.integer_store_convert(value, ty, site, span)
        } else {
            value
        };
        self.assign(place, value, ty, span);
        self.flush_element_writebacks(writebacks, span);
    }

    /// Whether `ty` is an integer slot whose policy a store applies: a
    /// trapping `numerus`, a `wrapping` or a `saturating` word.
    pub(super) fn is_integer_store_slot(&self, ty: MirType) -> bool {
        match self.normalized_type(self.types.peel_access_marker(ty.semantic_id()).1) {
            // Every integer slot takes the node: at the cap the exact successor
            // (`i64::MAX + 1`) is a value the carrier holds, so only the store
            // check keeps it out of an `i64` slot (ruling 17).
            Type::ModularWord(_)
            | Type::SaturatingWord(_)
            | Type::RangedInt { .. }
            | Type::Primitive(Primitive::Numerus) => true,
            // Deliberate: a store into `inf` is total (nothing to fit), so it
            // takes no store node; `integer_range()` is `None` for `inf`.
            Type::SizedNumeric(Primitive::Numerus, width) => width.integer_range().is_some(),
            _ => false,
        }
    }

    /// A checked `Convert` of an integer value into its own slot type: the
    /// store node for a position whose HIR carries no `Conversio` (`x ↑`).
    fn integer_store_convert(
        &mut self,
        value: MirOperand,
        slot_ty: MirType,
        site: radix_hir::HirStoreSite,
        span: Span,
    ) -> MirOperand {
        self.runtime_call_value(
            MirIntrinsic::Convert(MirConversion {
                store: Some(site),
                flavor: MirConversionFlavor::Runtime,
                source_ty: slot_ty,
                target_ty: slot_ty,
                params: Vec::new(),
                recovery: None,
                struct_defaults: Vec::new(),
            }),
            vec![value],
            slot_ty,
            span,
        )
    }

    /// The place a mutation of `expr` writes, for a caller that cannot finish
    /// a write-back. A target that reaches through a record tensor element
    /// needs the whole element stored back after the write, so it is refused
    /// by name here rather than written into a copy and lost.
    fn lower_assignment_place_with_fallback(
        &mut self,
        expr: &HirExpression,
        fallback_ty: MirType,
    ) -> Option<(MirPlace, MirType)> {
        let (place, ty, writebacks) = self.lower_assignment_place_writeback(expr, fallback_ty)?;
        if !writebacks.is_empty() {
            self.errors.push(MirError::unsupported(
                expr.span,
                "in-place mutation through a record tensor element is not lowered; assign the field or the whole element",
            ));
            return None;
        }
        Some((place, ty))
    }

    /// The place an assignment to `expr` writes, with the record tensor
    /// elements it reaches through: the caller stores the written place and
    /// then calls [`Self::flush_element_writebacks`].
    fn lower_assignment_place_writeback(
        &mut self,
        expr: &HirExpression,
        fallback_ty: MirType,
    ) -> Option<(MirPlace, MirType, Vec<ElementWriteback>)> {
        let outer = self.element_writebacks.replace(Vec::new());
        let result = self.lower_assignment_place_inner(expr, fallback_ty);
        let writebacks = std::mem::replace(&mut self.element_writebacks, outer).unwrap_or_default();
        result.map(|(place, ty)| (place, ty, writebacks))
    }

    /// Store each element read for an assignment target back into its tensor
    /// (`TensorSet`), after the target's field store.
    fn flush_element_writebacks(&mut self, writebacks: Vec<ElementWriteback>, span: Span) {
        let vacuum = MirType::semantic(self.types.primitive(Primitive::Vacuum));
        for writeback in writebacks.into_iter().rev() {
            self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::TensorSet),
                vec![
                    writeback.receiver,
                    writeback.index,
                    MirOperand::Place(writeback.element),
                ],
                vacuum,
                span,
            );
        }
    }

    fn lower_assignment_place_inner(
        &mut self,
        expr: &HirExpression,
        fallback_ty: MirType,
    ) -> Option<(MirPlace, MirType)> {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                if let Some(place) = self.captured_bindings.get(def_id).cloned() {
                    let Some((_, ty)) = self.captured_binding_metadata.get(def_id).copied() else {
                        self.errors.push(MirError::unsupported(
                            expr.span,
                            "assignment target capture is missing metadata",
                        ));
                        return None;
                    };
                    return Some((place, ty));
                }
                let Some(binding) = self.bindings.get(def_id).copied() else {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "assignment target that does not resolve to a local",
                    ));
                    return None;
                };

                Some((MirPlace::local(binding.local), binding.ty))
            }
            HirExpressionKind::Field(object, name) => {
                // The base lowers through the projectable-place path so a
                // narrowed optional base (`cur not is none { cur.v ← 5 }`)
                // unwraps before the projection; a non-narrowed base yields
                // the same place the direct recursion produced.
                let mut place = self.lower_projectable_place(object)?;
                place.projections.push(MirProjection::Field(*name));
                let ty = expr.ty.map_or(fallback_ty, MirType::semantic);
                Some((place, ty))
            }
            HirExpressionKind::Index(object, index) => {
                let mut place = self.lower_projectable_place(object)?;
                let projection = self.lower_index_projection(&place, object.ty, index, expr.id)?;
                place.projections.push(projection);
                let ty = expr.ty.map_or(fallback_ty, MirType::semantic);
                Some((place, ty))
            }
            _ => {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "assignment target that is not an addressable place",
                ));
                None
            }
        }
    }

    fn lower_transfer_expr(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        let operand = self.lower_expr_value(expr)?;
        match operand {
            MirOperand::Constant(_) | MirOperand::Value(_) => {
                let ty = self.expr_ty(expr)?;
                Some(self.assign_temp(MirValueKind::Operand(operand), ty, expr.span))
            }
            MirOperand::Place(_) | MirOperand::Temp(_) => Some(operand),
        }
    }

    fn lower_return_expr(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_transfer_expr(expr)
    }

    fn lower_path(&mut self, def_id: DefId, expr: &HirExpression) -> Option<MirOperand> {
        if let Some(place) = self.captured_bindings.get(&def_id).cloned() {
            let storage_ty = self
                .captured_binding_metadata
                .get(&def_id)
                .map(|(_, ty)| *ty);
            let place = self.adapt_narrowed_place(place, storage_ty, expr)?;
            return Some(MirOperand::Place(place));
        }
        let Some(binding) = self.bindings.get(&def_id).copied() else {
            if self.context.variant_parents.contains_key(&def_id) {
                let Some(field_names) = self.context.variant_fields.get(&def_id) else {
                    self.errors
                        .push(MirError::unsupported(expr.span, "enum variant path is missing field metadata"));
                    return None;
                };
                if !field_names.is_empty() {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "payload enum variant path without constructor call",
                    ));
                    return None;
                }
                let ty = self.variant_value_ty(expr)?;
                return Some(self.construct_temp(
                    MirAggregateKind::EnumVariant(def_id),
                    MirAggregateFields::Ordered(Vec::new()),
                    ty,
                    expr.span,
                ));
            }
            if let Some(konst) = self.context.top_level_consts.get(&def_id).copied() {
                let binding = self.lower_top_level_const_binding(def_id, konst, expr.span)?;
                return Some(MirOperand::Place(MirPlace::local(binding.local)));
            }
            // sgr C2a: a body-visible `magnitudo` parameter names its
            // compile-time size, not a runtime value. Carry it as the
            // symbolic shape-size operand on the parameter's own interned
            // node — shape bindings at instantiation fold it to the concrete
            // size; AIR never sees it unsubstituted.
            if let Some(name) = self.size_params.get(&def_id).copied()
                && let Some(shape) = self
                    .size_param_sources
                    .get(&def_id)
                    .copied()
                    .or_else(|| symbolic_size_index(self.types, name))
            {
                if index_contains_runtime_extent(self.types, shape, &mut FxHashSet::default()) {
                    let mut arguments = Vec::new();
                    let expression = match self.lower_shape_expr(
                        shape,
                        &mut arguments,
                        None,
                        ShapeRelationBoundary::FunctionEntry,
                        0,
                    ) {
                        Ok(expression) => expression,
                        Err(message) => {
                            self.errors.push(MirError::unsupported(expr.span, message));
                            return None;
                        }
                    };
                    // A device body reads the axis extent the launch already
                    // carries, as the loop bounds do: no `f64` shape-size call.
                    if self.is_device_lowering()
                        && let MirShapeExpr::TensorAxis { argument, axis } = &expression
                        && let Some(MirOperand::Place(tensor)) = usize::try_from(*argument)
                            .ok()
                            .and_then(|index| arguments.get(index))
                    {
                        let ty = self.expr_ty(expr)?;
                        return Some(self.assign_temp(
                            MirValueKind::TensorExtent { tensor: MirOperand::Place(tensor.clone()), axis: *axis },
                            ty,
                            expr.span,
                        ));
                    }
                    let scalar = self.runtime_call_value(
                        MirIntrinsic::TensorShapeSize(expression),
                        arguments,
                        MirType::semantic(self.types.primitive(Primitive::Fractus)),
                        expr.span,
                    );
                    // Size-type Z1 force: the size name reads as an int, so
                    // convert the Fractus shape-size operand (same pattern as
                    // lower/runtime.rs rope shape arguments).
                    let numerus = self.expr_ty(expr)?;
                    return Some(self.runtime_call_value(
                        MirIntrinsic::Convert(MirConversion {
                            store: None,
                            flavor: MirConversionFlavor::Runtime,
                            source_ty: MirType::semantic(self.types.primitive(Primitive::Fractus)),
                            target_ty: numerus,
                            params: Vec::new(),
                            recovery: None,
                            struct_defaults: Vec::new(),
                        }),
                        vec![scalar],
                        numerus,
                        expr.span,
                    ));
                }
                return Some(MirOperand::Constant(MirConstant::ShapeSize { shape, axis: None }));
            }
            if let Some(value) = self.lower_function_value(def_id, expr) {
                return Some(value);
            }
            if !self.context.function_params.contains_key(&def_id) {
                self.errors
                    .push(MirError::unsupported(expr.span, "path that does not resolve to a local value"));
            }
            return None;
        };

        let place = self.adapt_narrowed_place(MirPlace::local(binding.local), Some(binding.ty), expr)?;
        Some(MirOperand::Place(place))
    }

    /// Lower a folded imported module constant to the constant its interface
    /// carries. Text values stay unlowered (the borrowed interner cannot mint
    /// the symbol), so they keep the original unresolved-path diagnostic. A
    /// list constant is rebuilt in the importing function from its element
    /// values, the way a same-module list literal lowers.
    fn lower_imported_constant(&mut self, value: &InterfaceConstValue, expr: &HirExpression) -> Option<MirOperand> {
        if let InterfaceConstValue::List(items) = value {
            let element_ty = match self.normalized_type(expr.ty?) {
                Type::Array(element) => Some(*element),
                _ => None,
            };
            let mut operands = Vec::with_capacity(items.len());
            for item in items {
                operands.push(self.lower_imported_scalar(item, element_ty, expr.span)?);
            }
            return self.construct_array_from_operands(operands, expr);
        }
        self.lower_imported_scalar(value, expr.ty, expr.span)
    }

    fn lower_imported_scalar(
        &mut self,
        value: &InterfaceConstValue,
        ty: Option<TypeId>,
        span: Span,
    ) -> Option<MirOperand> {
        match value {
            InterfaceConstValue::Int(value) => {
                if let Ok(unsigned) = u64::try_from(*value) {
                    return self.lower_literal(&HirLiteral::Int(unsigned), ty, span);
                }
                let signed = i64::try_from(*value).ok()?;
                Some(MirOperand::Constant(MirConstant::Int(signed)))
            }
            InterfaceConstValue::Float(value) => self.lower_literal(&HirLiteral::Float(*value), ty, span),
            InterfaceConstValue::Bool(value) => Some(MirOperand::Constant(MirConstant::Bool(*value))),
            InterfaceConstValue::Text(_) | InterfaceConstValue::List(_) => None,
        }
    }

    fn lower_literal(&mut self, literal: &HirLiteral, ty: Option<TypeId>, span: Span) -> Option<MirOperand> {
        match literal {
            HirLiteral::Int(value) => {
                // A raw-bits literal has to reach a `bN` call parameter
                // carrying its own width: a bare `Int`/`UInt` constant is typed
                // `numerus`, and MIR rejects it against a `bN` parameter. Ride
                // a temp typed by the literal's own semantic type, the way a
                // bounded `ascii<N>` rides one (need, item 11), and
                // keep the zero-extended `UInt` carrier a raw-bits cell stores
                // (it prints as hex, and the `Int` carrier does not).
                if let Some(bits_ty) = ty.filter(|ty| matches!(self.normalized_type(*ty), Type::Bits(_))) {
                    return Some(self.assign_temp(
                        MirValueKind::Operand(MirOperand::Constant(MirConstant::UInt(*value))),
                        MirType::semantic(bits_ty),
                        span,
                    ));
                }
                // Type-aware carrier: unsigned targets (`numerus<uW>` unsigned,
                // `modulus<uW>`) carry `UInt` even for small magnitudes, so
                // literals agree with conversion and arithmetic results on
                // equality, collection keys, and switch matching.
                if self.literal_ty_is_unsigned(ty) {
                    return Some(MirOperand::Constant(MirConstant::UInt(*value)));
                }
                if let Ok(value) = i64::try_from(*value) {
                    return Some(MirOperand::Constant(MirConstant::Int(value)));
                }
                // An `inf` slot holds the upper half of u64 exactly as `UInt`.
                if self.literal_ty_is_unbounded(ty) {
                    return Some(MirOperand::Constant(MirConstant::UInt(*value)));
                }
                // Upper-half u64 magnitude may not be sign-encoded into
                // `Int(i64)`; semantic analysis already rejected it for signed
                // targets, so reaching here is a contract violation.
                self.errors.push(MirError::unsupported(
                    span,
                    "unsigned integer literal above i64::MAX requires an unsigned target",
                ));
                None
            }
            // A literal beyond `u64` types only as `inf` (ruling 34): an exact
            // bignum constant, never truncated.
            HirLiteral::BigInt(digits) => Some(MirOperand::Constant(inf_literal_constant(digits))),
            // decimal-widths D1 successor: a decimal-typed
            // text literal lowers to its exact scaled carrier through the one
            // shared text→carrier conversion; the f64 field never intermediates.
            HirLiteral::FloatText { value, text } if let Some(width) = self.decimal_literal_width(ty) => {
                let Some(interner) = self.context.interner else {
                    // Without the interner the digit text is unreadable; keep
                    // the historical float constant instead of guessing.
                    return Some(MirOperand::Constant(MirConstant::Float(*value)));
                };
                if let Some(carrier) =
                    radix_codegen_shared::decimal::decimal_text_to_carrier(interner.resolve(*text), width)
                {
                    Some(MirOperand::Constant(MirConstant::Int(carrier)))
                } else {
                    self.errors.push(MirError::unsupported(
                        span,
                        "decimal literal carrier is outside the declared width",
                    ));
                    None
                }
            }
            // Non-decimal text literals keep the historical float constant.
            HirLiteral::Float(value) | HirLiteral::FloatText { value, .. } => {
                let operand = MirOperand::Constant(MirConstant::Float(*value));
                match self.sized_fractus_literal_ty(ty) {
                    Some(ty) => Some(self.assign_temp(MirValueKind::Operand(operand), ty, span)),
                    None => Some(operand),
                }
            }
            HirLiteral::String(value) => Some(MirOperand::Constant(MirConstant::String(*value))),
            HirLiteral::Ascii(value) => {
                let operand = MirOperand::Constant(MirConstant::Ascii(*value));
                // WHY: a MIR ascii constant is typed bare `ascii`, so as a
                // bare aggregate item it cannot satisfy a bounded `ascii<N>`
                // element type (need, item 11), nor a `littera`
                // slot where semantic analysis already typed a one-letter
                // literal as the scalar (D10.5). A temp typed by the
                // literal's own semantic type carries the slot.
                match ty.filter(|ty| {
                    matches!(
                        self.normalized_type(*ty),
                        Type::BoundedAscii { .. } | Type::Primitive(Primitive::Littera)
                    )
                }) {
                    Some(ty) => Some(self.assign_temp(MirValueKind::Operand(operand), MirType::semantic(ty), span)),
                    None => Some(operand),
                }
            }
            HirLiteral::Bool(value) => Some(MirOperand::Constant(MirConstant::Bool(*value))),
            HirLiteral::Nil => Some(MirOperand::Constant(MirConstant::Nil)),
            HirLiteral::Octeti(sym) => self.lower_octeti_literal(*sym, ty, span),
            HirLiteral::Regex(pattern, flags) => {
                Some(MirOperand::Constant(MirConstant::Regex { pattern: *pattern, flags: *flags }))
            }
            HirLiteral::JsonValor(value) => self.lower_json_valor_literal(value, ty, span),
        }
    }

    fn sized_fractus_literal_ty(&self, ty: Option<TypeId>) -> Option<MirType> {
        let mut ty = ty?;
        loop {
            match self.types.get(ty) {
                Type::Alias(_, inner) => ty = *inner,
                Type::SizedNumeric(Primitive::Fractus, _) | Type::PolicyFloat(_, _) => {
                    return Some(MirType::semantic(ty));
                }
                _ => return None,
            }
        }
    }

    /// The decimal width of a literal's target type, if any.
    ///
    /// Decimal widths ride `Numerus` (the scaled-integer carrier), not
    /// `Fractus`; mirrors the rust backend's decimal-literal detection.
    fn decimal_literal_width(&self, ty: Option<TypeId>) -> Option<NumericWidth> {
        let mut ty = ty?;
        loop {
            match self.types.get(ty) {
                Type::Alias(_, inner) => ty = *inner,
                Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal() => return Some(*width),
                _ => return None,
            }
        }
    }

    /// Whether an integer literal carries `UInt`. Only u64-width targets do:
    /// every narrower value fits the signed `Int` carrier, matching
    /// `modular_word_result` and the runner conversion boundaries, so
    /// literals, conversions, and arithmetic results stay carrier-consistent
    /// per width. Fails closed: a missing type is not unsigned. (Semantic
    /// analysis types every literal and rejects upper-half magnitudes for
    /// signed targets, so `None` here only arises for small literals that
    /// still fit the signed carrier.)
    fn literal_ty_is_unsigned(&self, ty: Option<TypeId>) -> bool {
        let Some(mut ty) = ty else { return false };
        loop {
            match self.types.get(ty) {
                Type::Alias(_, inner) => ty = *inner,
                Type::SizedNumeric(Primitive::Numerus, NumericWidth::U64)
                | Type::ModularWord(NumericWidth::U64)
                | Type::SaturatingWord(NumericWidth::U64)
                | Type::RangedInt { width: NumericWidth::U64, .. }
                // A raw-bits cell always holds the zero-extended unsigned
                // pattern.
                | Type::Bits(_) => return true,
                _ => return false,
            }
        }
    }

    /// Whether a literal's type is the unbounded `inf` (an `inf` slot holds
    /// any integer literal exactly, ruling 34).
    fn literal_ty_is_unbounded(&self, ty: Option<TypeId>) -> bool {
        ty.is_some_and(|ty| radix_types::numeric_width::is_unbounded_integer_type(self.types, ty))
    }

    fn lower_octeti_literal(&mut self, sym: Symbol, ty: Option<TypeId>, span: Span) -> Option<MirOperand> {
        if ty.is_none() {
            self.errors
                .push(MirError::missing_type(span, "list<b8> literal"));
            return None;
        }
        let Some(interner) = self.context.interner else {
            return Some(MirOperand::Constant(MirConstant::Octeti(Vec::new())));
        };
        let raw = interner.resolve(sym);
        let Ok(bytes) = radix_lexer::octeti_literal::decode_octeti_hex(raw) else {
            self.errors.push(MirError::unsupported(
                span,
                "list<b8> literal failed decode before MIR lowering",
            ));
            return None;
        };
        // Need: a byte literal is a `lista<b8>`, so it lowers to a
        // list of byte patterns typed by the cell; the opaque `Octeti`
        // constant remains only for a legacy primitive-typed slot.
        if let Some(list_ty) = ty.filter(|ty| self.is_bits_byte_list(*ty)) {
            let items = bytes
                .into_iter()
                .map(|byte| MirAggregateItem::Operand(MirOperand::Constant(MirConstant::UInt(u64::from(byte)))))
                .collect();
            return Some(self.construct_temp(
                MirAggregateKind::Array,
                MirAggregateFields::Ordered(items),
                MirType::semantic(list_ty),
                span,
            ));
        }
        Some(MirOperand::Constant(MirConstant::Octeti(bytes)))
    }

    /// Whether `ty` is `lista<b8>` (aliases looked through).
    fn is_bits_byte_list(&self, mut ty: TypeId) -> bool {
        while let Type::Alias(_, inner) = self.types.get(ty) {
            ty = *inner;
        }
        let Type::Array(mut element) = *self.types.get(ty) else {
            return false;
        };
        while let Type::Alias(_, inner) = self.types.get(element) {
            element = *inner;
        }
        matches!(self.types.get(element), Type::Bits(radix_types::BitsWidth::B8))
    }

    fn lower_unary(&mut self, op: HirUnOp, operand: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        if let Some(lift) = self.tensor_lift_unary_op(op, operand) {
            return self.lower_tensor_lift(lift, &[operand], expr);
        }
        if op == HirUnOp::Neg
            && let Some(lowered) = self.lower_register_negate(operand, expr)
        {
            return lowered;
        }
        if op == HirUnOp::Not
            && let Some(lowered) = self.lower_register_not(operand, expr)
        {
            return lowered;
        }
        if matches!(op, HirUnOp::Neg | HirUnOp::Not)
            && let Some(lowered) = self.lower_lista_negate(op, operand, expr)
        {
            return lowered;
        }
        let mir_op = mir_un_op(op);
        // `-9223372036854775808`: the magnitude 2^63 has no `Int(i64)` form,
        // but its negation is exactly `i64::MIN` (d11-6 ruling 12: an untyped
        // constant is an exact value), so the negated literal is one constant.
        if op == HirUnOp::Neg
            && let HirExpressionKind::Literal(HirLiteral::Int(magnitude)) = &operand.kind
            && *magnitude == i64::MIN.unsigned_abs()
            && !self.literal_ty_is_unsigned(operand.ty)
        {
            return Some(MirOperand::Constant(MirConstant::Int(i64::MIN)));
        }
        let op = mir_op;
        let operand_expr = operand;
        let mut operand = self.lower_expr_value(operand_expr)?;
        let mut ty = self.expr_ty(expr)?;
        if op == MirUnOp::Neg {
            let operand_ty = self.expr_ty(operand_expr)?;
            let (operand_marker, operand_payload) = self.types.peel_access_marker(operand_ty.semantic_id());
            ty = MirType::semantic(self.types.peel_access_marker(ty.semantic_id()).1);
            if operand_marker.is_some() {
                // A marked scalar parameter is represented in HIR by its access
                // type, while the MIR parameter local stores the numeric payload.
                // Read that place into a payload-typed value before negating it;
                // carrying the marker through to the unary result makes MIR see a
                // non-numeric `ref`/`mut` type.
                operand = self.assign_temp(
                    MirValueKind::Operand(operand),
                    MirType::semantic(operand_payload),
                    operand_expr.span,
                );
            }
        }

        Some(self.assign_temp(MirValueKind::Unary { op, operand }, ty, expr.span))
    }

    /// Lowers a runtime variant check (`est <type>`) to MIR.
    ///
    /// MIR does not model valor variant discrimination directly; per the existing
    /// `air/to_mir.rs` rule, `est` collapses to an equality test at this level.
    /// Lower `expr est <type>` / `expr non est <type>` into a runner-dispatched
    /// variant-tag check.
    ///
    /// The target type travels with the intrinsic so the runtime can check the
    /// operand's actual variant tag instead of approximating it as a nullity
    /// gate. Device/application lanes keep failing closed on the intrinsic
    /// (only the MIR runner dispatches `RuntimeTypeCheck`).
    fn lower_type_check(
        &mut self,
        positive: bool,
        ty: TypeId,
        expr: &HirExpression,
        outer: &HirExpression,
    ) -> Option<MirOperand> {
        let operand = self.lower_expr_value(expr)?;
        let result_ty = self.expr_ty(outer)?;
        // A type test against the null type is the null-presence probe: route
        // it back to the unary `is_nil` / `is_not_nil` lowering that the
        // legacy `est nihil` value form used, rather than the runner-dispatched
        // variant-tag intrinsic (DECISIONS D23.1).
        if matches!(self.types.get(ty), Type::Primitive(Primitive::Nihil)) {
            return Some(self.lower_nil_test(positive, operand, result_ty, outer.span));
        }
        Some(self.runtime_call_value(
            MirIntrinsic::TypeCheck(MirTypeCheckOp { positive, ty: MirType::semantic(ty) }),
            vec![operand],
            result_ty,
            outer.span,
        ))
    }

    pub fn lower_binary(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        // K4: a scalar operator over tensors is one elementwise lift; both
        // operands are always evaluated, so it precedes the short-circuit.
        if let Some(lift) = self.tensor_lift_binary_op(op, lhs, rhs) {
            return self.lower_tensor_lift(lift, &[lhs, rhs], expr);
        }
        // A scalar operator against a register vector or matrix unrolls per lane.
        if let Some(lowered) = self.lower_register_lift_binary(op, lhs, rhs, expr) {
            return lowered;
        }
        // List `% ÷ ⊙ ⊘` are elementwise collection ops, not scalar or glyph
        // products.
        if matches!(
            op,
            HirBinOp::Mod | HirBinOp::TrueDiv | HirBinOp::Hadamard | HirBinOp::HadamardDiv
        ) && self.is_lista_or_queue_stack_arithmetic_operand(lhs, rhs)
        {
            return self.lower_lista_elementwise(op, lhs, rhs, expr);
        }
        // `et` / `aut` over a `lista<bivalens>` lift per element (Q-C of the
        // 2026-10-04 comparisons ruling); both sides are always evaluated, so
        // it precedes the short-circuit.
        if matches!(op, HirBinOp::And | HirBinOp::Or)
            && expr.ty.is_some_and(|ty| {
                matches!(
                    self.normalized_type(ty),
                    Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
                )
            })
        {
            return self.lower_lista_elementwise(op, lhs, rhs, expr);
        }
        if matches!(op, HirBinOp::And | HirBinOp::Or) {
            return self.lower_logical_short_circuit(op, lhs, rhs, expr);
        }
        if matches!(op, HirBinOp::Is | HirBinOp::IsNot)
            && let Some(predicate) = self.lower_is_predicate(op, lhs, rhs, expr)
        {
            return Some(predicate);
        }
        if matches!(op, HirBinOp::Between | HirBinOp::NotBetween) {
            return self.lower_between_comparison(op, lhs, rhs, expr);
        }
        if matches!(op, HirBinOp::InRange | HirBinOp::NotInRange) {
            return self.lower_range_membership(op == HirBinOp::NotInRange, lhs, rhs, expr);
        }
        // TENSOR-GLYPHS-3A: glyph products dispatch by operand family before
        // the primitive `mir_bin_op` fallback (which fails them closed).
        if op.is_glyph_product() {
            return self.lower_glyph_product(op, lhs, rhs, expr);
        }
        if op == HirBinOp::TrueDiv {
            return self.lower_true_division(lhs, rhs, expr);
        }
        // Numeric tensor/vector/matrix `+`/`-` take the live `addita`/`subtrahe`
        // method paths (`TensorAdd` / `TensorSub`), never primitive scalar ops.
        if matches!(op, HirBinOp::Add | HirBinOp::Sub) && self.is_addita_plus_operands(lhs, rhs) {
            return self.lower_glyph_product(op, lhs, rhs, expr);
        }
        if matches!(op, HirBinOp::FuzzyEq | HirBinOp::FuzzyNotEq) {
            return self.lower_fuzzy_comparison(op, lhs, rhs, expr);
        }
        // A1/C5 numeric collection laws have a dedicated MIR collection-op
        // seam: every pair reaches the runner, which pairs the live lengths
        // (a capacity never stands in for one).
        if matches!(op, HirBinOp::Add | HirBinOp::Sub | HirBinOp::Mul | HirBinOp::Div)
            && self.is_lista_or_queue_stack_arithmetic_operand(lhs, rhs)
        {
            return self.lower_lista_elementwise(op, lhs, rhs, expr);
        }
        // `≺ ≻ ≤ ≥ ≅ ≇` over a numeric list lift per element to a `lista<bivalens>`
        // (the typechecker types the result as a list; a whole-value `≅` over
        // non-numeric lists stays one `bivalens`).
        if matches!(
            op,
            HirBinOp::Lt | HirBinOp::Gt | HirBinOp::LtEq | HirBinOp::GtEq | HirBinOp::ApproxEq | HirBinOp::ApproxNotEq
        ) && expr.ty.is_some_and(|ty| {
            matches!(
                self.normalized_type(ty),
                Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
            )
        }) {
            return self.lower_lista_elementwise(op, lhs, rhs, expr);
        }
        if matches!(op, HirBinOp::Lt | HirBinOp::Gt | HirBinOp::LtEq | HirBinOp::GtEq)
            && lhs
                .ty
                .is_some_and(|ty| matches!(self.types.get(self.types.peel_access_marker(ty).1), Type::Tuple(_)))
        {
            return self.lower_tuple_order(op, lhs, rhs, expr);
        }
        // Operator ruling 2026-10-15: `≺ ≻ ≤ ≥` over two values
        // of one `ordo` order by their declared discriminants. Typecheck
        // recorded the rows keyed by the left operand; convert both operands to
        // those integers and compare.
        if matches!(op, HirBinOp::Lt | HirBinOp::Gt | HirBinOp::LtEq | HirBinOp::GtEq)
            && let Some(rows) = self
                .context
                .tensor_edge_policies
                .ordo_order
                .comparisons
                .get(&lhs.id)
                .cloned()
        {
            return self.lower_ordo_order_compare(op, &rows, lhs, rhs, expr);
        }
        let Some(mir_op) = mir_bin_op(op) else {
            self.errors
                .push(MirError::unsupported(expr.span, "binary operator without a MIR primitive"));
            return None;
        };
        // GT-1: the decimal d64-scale join is the shared comparison join —
        // ordering (mixed-ordering-join) and the ≅/≈ value-equality tiers both
        // widen decimal operands at the d64 carrier before the bool op.
        let (mut lhs_value, mut rhs_value) = if matches!(
            op,
            HirBinOp::Lt
                | HirBinOp::Gt
                | HirBinOp::LtEq
                | HirBinOp::GtEq
                | HirBinOp::Eq
                | HirBinOp::NotEq
                | HirBinOp::StrictEq
                | HirBinOp::StrictNotEq
                | HirBinOp::ApproxEq
                | HirBinOp::ApproxNotEq
        ) {
            self.lower_decimal_join_operands(lhs, rhs)?
        } else {
            (self.lower_expr_value(lhs)?, self.lower_expr_value(rhs)?)
        };
        if matches!(
            op,
            HirBinOp::Add | HirBinOp::Lt | HirBinOp::Gt | HirBinOp::LtEq | HirBinOp::GtEq
        ) {
            lhs_value = self.widen_bounded_cord_operand(lhs_value, lhs)?;
            rhs_value = self.widen_bounded_cord_operand(rhs_value, rhs)?;
        }
        let ty = self.expr_ty(expr)?;

        Some(self.assign_temp(
            MirValueKind::Binary { op: mir_op, lhs: lhs_value, rhs: rhs_value },
            ty,
            expr.span,
        ))
    }

    /// Widen bounded text operands to the unbounded textus carrier required by MIR.
    fn widen_bounded_cord_operand(&mut self, operand: MirOperand, expr: &HirExpression) -> Option<MirOperand> {
        let source_ty = self.expr_ty(expr)?;
        if !matches!(
            self.normalized_type(source_ty.semantic_id()),
            Type::BoundedTextus { .. } | Type::BoundedAscii { .. }
        ) {
            return Some(operand);
        }
        let target_ty = MirType::semantic(self.types.primitive(Primitive::Textus));
        Some(self.runtime_call_value(
            MirIntrinsic::Convert(MirConversion {
                store: None,
                flavor: MirConversionFlavor::Runtime,
                source_ty,
                target_ty,
                params: Vec::new(),
                recovery: None,
                struct_defaults: Vec::new(),
            }),
            vec![operand],
            target_ty,
            expr.span,
        ))
    }

    /// Lower argument `index` (receiver excluded) of a collection runtime call.
    /// The text operands the MIR validator requires to be exactly `textus`
    /// (`starts_with`/`ends_with`/`split` separator, `replace` pair) widen
    /// from the ASCII family first.
    pub(super) fn lower_collection_call_arg(
        &mut self,
        op: MirCollectionOp,
        index: usize,
        arg: &HirExpression,
    ) -> Option<MirOperand> {
        let lowered = self.lower_call_arg_value(arg)?;
        let text_operand = match op {
            MirCollectionOp::StartsWith | MirCollectionOp::EndsWith | MirCollectionOp::Split => index == 0,
            MirCollectionOp::Replace => index < 2,
            _ => false,
        };
        if text_operand {
            self.widen_text_argument(lowered, arg)
        } else {
            Some(lowered)
        }
    }

    /// Widen an ASCII-family text argument to the `textus` carrier the MIR
    /// text rows require. The checker admits bare `ascii` and `ascii<1>` where
    /// `textus` is expected (implicit ASCII widening) and retypes the argument
    /// expression to the parameter, so the operand's own carrier is read from
    /// MIR. Bare `ascii` is retyped the way `let string t ← x` retypes it; a
    /// bounded cord takes the `↦ textus` copy.
    fn widen_text_argument(&mut self, operand: MirOperand, expr: &HirExpression) -> Option<MirOperand> {
        let textus = MirType::semantic(self.types.primitive(Primitive::Textus));
        let carrier = self.operand_carrier_ty(&operand);
        match carrier.map(|ty| self.normalized_type(ty.semantic_id())) {
            Some(Type::Primitive(Primitive::Ascii)) => {
                Some(self.assign_temp(MirValueKind::Operand(operand), textus, expr.span))
            }
            Some(Type::BoundedTextus { .. } | Type::BoundedAscii { .. }) => {
                let source_ty = carrier?;
                Some(self.runtime_call_value(
                    MirIntrinsic::Convert(MirConversion {
                        store: None,
                        flavor: MirConversionFlavor::Runtime,
                        source_ty,
                        target_ty: textus,
                        params: Vec::new(),
                        recovery: None,
                        struct_defaults: Vec::new(),
                    }),
                    vec![operand],
                    textus,
                    expr.span,
                ))
            }
            _ => Some(operand),
        }
    }

    /// The carrier type of a lowered operand when it is a constant, a temp, or
    /// a projection-free local. `None` for operands whose type is not recorded
    /// on the builder.
    fn operand_carrier_ty(&self, operand: &MirOperand) -> Option<MirType> {
        match operand {
            MirOperand::Constant(MirConstant::Ascii(_)) => {
                Some(MirType::semantic(self.types.primitive(Primitive::Ascii)))
            }
            MirOperand::Temp(temp) => self.temps.iter().find(|t| t.id == *temp).map(|t| t.ty),
            MirOperand::Place(place) if place.projections.is_empty() => match place.base {
                MirPlaceBase::Local(local) => self.locals.iter().find(|l| l.id == local).map(|l| l.ty),
                MirPlaceBase::Temp(temp) => self.temps.iter().find(|t| t.id == temp).map(|t| t.ty),
            },
            _ => None,
        }
    }

    /// Lower an `ordo` ordering comparison by converting both operands to their
    /// declared discriminants and comparing the integers.
    fn lower_ordo_order_compare(
        &mut self,
        op: HirBinOp,
        rows: &[(DefId, i64)],
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let int_ty = self.types.primitive(Primitive::Numerus);
        let lhs_value = self.lower_ordo_operand_discriminant(lhs, rows, int_ty)?;
        let rhs_value = self.lower_ordo_operand_discriminant(rhs, rows, int_ty)?;
        let mir_op = mir_bin_op(op)?;
        let result_ty = self.expr_ty(expr)?;
        Some(self.assign_temp(
            MirValueKind::Binary { op: mir_op, lhs: lhs_value, rhs: rhs_value },
            result_ty,
            expr.span,
        ))
    }

    /// Convert one `ordo` operand to its declared discriminant: an equality
    /// chain over the members, in declaration order, that writes one integer
    /// temp — the same shape `lower_ordo_conversio` emits for `Ordo ↦ numerus`.
    fn lower_ordo_operand_discriminant(
        &mut self,
        source: &HirExpression,
        rows: &[(DefId, i64)],
        int_ty: TypeId,
    ) -> Option<MirOperand> {
        let span = source.span;
        let source_ty = self.expr_ty(source)?;
        let value = self.lower_expr_value(source)?;
        let mir_int_ty = MirType::semantic(int_ty);
        let result = MirPlace::temp(self.push_temp(mir_int_ty, span));
        let join_id = self.fresh_block(span);
        let bool_ty = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let mut reaches_join = false;

        for (variant, declared) in rows {
            let probe = self.construct_temp(
                MirAggregateKind::EnumVariant(*variant),
                MirAggregateFields::Ordered(Vec::new()),
                source_ty,
                span,
            );
            let condition = self.binary_temp(MirBinOp::Eq, value.clone(), probe, bool_ty, span);
            let then_id = self.fresh_block(span);
            let else_id = self.fresh_block(span);
            self.terminate_current(
                MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_id },
                span,
            );
            self.switch_to(then_id);
            self.assign(
                result.clone(),
                MirOperand::Constant(MirConstant::Int(*declared)),
                mir_int_ty,
                span,
            );
            reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);
            self.switch_to(else_id);
        }

        // Every `ordo` value is one of the members, so the fall-through cannot
        // be reached.
        self.terminate_current(MirTerminatorKind::Unreachable, span);
        if reaches_join {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, span);
        }
        Some(MirOperand::Place(result))
    }

    fn lower_between_comparison(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let collection = self.lower_expr_value(rhs)?;
        let value = self.lower_expr_value(lhs)?;
        let ty = self.expr_ty(expr)?;
        let contained = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Contains),
            vec![collection, value],
            ty,
            expr.span,
        );
        if op == HirBinOp::Between {
            return Some(contained);
        }
        // `∉` is the existing boolean negation of the same `Contains` call.
        Some(self.assign_temp(MirValueKind::Unary { op: MirUnOp::Not, operand: contained }, ty, expr.span))
    }

    fn lower_fuzzy_comparison(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        // GT-1: fuzzy equality joins decimal operands at the d64 scale
        // like every other comparison tier; raw scaled carriers would
        // compare a d64 carrier against a peer at another scale.
        let (lhs, rhs) = self.lower_decimal_join_operands(lhs, rhs)?;
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Approximate),
            vec![
                lhs,
                rhs,
                MirOperand::Constant(MirConstant::Float(1e-9)),
                MirOperand::Constant(MirConstant::Float(0.0)),
                MirOperand::Constant(MirConstant::Bool(matches!(op, HirBinOp::FuzzyNotEq))),
            ],
            ty,
            expr.span,
        ))
    }

    /// `x ∈ r` / `x ∉ r` over an interval (`InRange` / `NotInRange`).
    ///
    /// A literal `lo‥hi` / `lo…hi` right operand lowers to scalar compares
    /// (`lo <= x` and `x < hi`, or `x <= hi` when inclusive), so no interval
    /// object exists and the form is legal in kernels. Evaluation order is
    /// `x`, `lo`, `hi`, each once. Any other interval-typed right operand (a
    /// variable) takes the runtime `IntervalContains` op, which the runner
    /// already uses for `r.continet(x)`. `∉` is the boolean negation.
    fn lower_range_membership(
        &mut self,
        negated: bool,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let value = self.lower_expr_value(lhs)?;
        let contained = if let HirExpressionKind::Intervallum { start, end, step: None, kind } = &rhs.kind {
            let low = self.lower_expr_value(start)?;
            let high = self.lower_expr_value(end)?;
            let upper = if matches!(kind, HirRangeKind::Inclusive) {
                MirBinOp::LtEq
            } else {
                MirBinOp::Lt
            };
            let above = self.assign_temp(
                MirValueKind::Binary { op: MirBinOp::GtEq, lhs: value.clone(), rhs: low },
                ty,
                expr.span,
            );
            let below = self.assign_temp(MirValueKind::Binary { op: upper, lhs: value, rhs: high }, ty, expr.span);
            self.assign_temp(
                MirValueKind::Binary { op: MirBinOp::And, lhs: above, rhs: below },
                ty,
                expr.span,
            )
        } else {
            let interval = self.lower_expr_value(rhs)?;
            self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::IntervalContains),
                vec![interval, value],
                ty,
                expr.span,
            )
        };
        if !negated {
            return Some(contained);
        }
        Some(self.assign_temp(MirValueKind::Unary { op: MirUnOp::Not, operand: contained }, ty, expr.span))
    }

    /// `÷` (d11-6 ruling 15): integer operands convert to `f64` and divide;
    /// float and decimal operands lower exactly like `/`. An integer zero
    /// divisor traps (ruling 19c) through a discarded integer `1 / divisor`,
    /// which fails with the runtime's standard division error.
    fn lower_true_division(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let lhs_ty = self.expr_ty(lhs)?;
        let rhs_ty = self.expr_ty(rhs)?;
        let is_integer = |this: &Self, ty: MirType| {
            let semantic = this.types.peel_access_marker(ty.semantic_id()).1;
            // `wrapping` and `saturating` words are integers too (ruling 15).
            matches!(
                this.normalized_type(semantic),
                Type::Primitive(Primitive::Numerus)
                    | Type::SizedNumeric(Primitive::Numerus, _)
                    | Type::ModularWord(_)
                    | Type::SaturatingWord(_)
                    | Type::RangedInt { .. }
            ) && !matches!(
                this.normalized_type(semantic),
                Type::SizedNumeric(_, width) if width.is_decimal()
            )
        };
        if !(is_integer(self, lhs_ty) && is_integer(self, rhs_ty)) {
            let (lhs, rhs) = self.lower_decimal_join_operands(lhs, rhs)?;
            return Some(self.assign_temp(MirValueKind::Binary { op: MirBinOp::Div, lhs, rhs }, ty, expr.span));
        }
        let lhs_value = self.lower_expr_value(lhs)?;
        let rhs_value = self.lower_expr_value(rhs)?;
        // The `1 / divisor` temporary is the zero-divisor trap, not dead code:
        // nothing reads it, but evaluating it is what fails. A nonzero
        // constant divisor cannot trap, so only that case emits nothing.
        if !matches!(
            rhs_value,
            MirOperand::Constant(MirConstant::Int(value)) if value != 0
        ) && !matches!(
            rhs_value,
            MirOperand::Constant(MirConstant::UInt(value)) if value != 0
        ) {
            let one = MirOperand::Constant(MirConstant::Int(1));
            self.binary_temp(MirBinOp::Div, one, rhs_value.clone(), rhs_ty, expr.span);
        }
        // An untyped constant operand joins the typed operand's range
        // computation (ruling 12), so its HIR type can be a word type while
        // the MIR constant is a plain integer: convert from the constant's
        // own MIR type, which is what the validator checks the operand against.
        let convert = |this: &mut Self, value: MirOperand, source_ty: MirType| {
            let source_ty = match &value {
                MirOperand::Constant(MirConstant::Int(_)) => {
                    MirType::semantic(this.types.primitive(Primitive::Numerus))
                }
                MirOperand::Constant(MirConstant::UInt(_)) => this
                    .types
                    .find_sized_numeric(Primitive::Numerus, NumericWidth::U64)
                    .map_or_else(
                        || MirType::semantic(this.types.primitive(Primitive::Numerus)),
                        MirType::semantic,
                    ),
                _ => source_ty,
            };
            this.runtime_call_value(
                MirIntrinsic::Convert(MirConversion {
                    store: None,
                    flavor: MirConversionFlavor::Runtime,
                    source_ty,
                    target_ty: ty,
                    params: Vec::new(),
                    recovery: None,
                    struct_defaults: Vec::new(),
                }),
                vec![value],
                ty,
                expr.span,
            )
        };
        let lhs_float = convert(self, lhs_value, lhs_ty);
        let rhs_float = convert(self, rhs_value, rhs_ty);
        Some(self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Div, lhs: lhs_float, rhs: rhs_float },
            ty,
            expr.span,
        ))
    }

    /// Tuple `≺ ≻ ≤ ≥` (D1.4): lexicographic three-way compare, then the
    /// glyph against zero. Elements must be built-in orderables; a genus
    /// element would need its own `compare` call per position, which MIR
    /// does not expand yet, so it fails closed here.
    fn lower_tuple_order(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let tuple_ty = lhs.ty?;
        if radix_types::ordering::builtin_ordering_kind(self.types, tuple_ty).is_none() {
            self.errors.push(MirError::unsupported(
                expr.span,
                "tuple ordering over a genus element is not lowered on MIR",
            ));
            return None;
        }
        let lhs_value = self.lower_expr_value(lhs)?;
        let rhs_value = self.lower_expr_value(rhs)?;
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let ordering = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Compare),
            vec![lhs_value, rhs_value],
            numerus,
            expr.span,
        );
        let mir_op = mir_bin_op(op)?;
        let ty = self.expr_ty(expr)?;
        Some(self.assign_temp(
            MirValueKind::Binary { op: mir_op, lhs: ordering, rhs: MirOperand::Constant(MirConstant::Int(0)) },
            ty,
            expr.span,
        ))
    }

    fn lower_logical_short_circuit(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let result = self.push_temp(ty, expr.span);
        let result_place = MirPlace::temp(result);
        let lhs_value = self.lower_expr_value(lhs)?;

        let rhs_id = self.fresh_block(rhs.span);
        let settled_id = self.fresh_block(expr.span);
        let join_id = self.fresh_block(expr.span);
        let (then_block, else_block, settled_value) = match op {
            HirBinOp::And => (rhs_id, settled_id, false),
            HirBinOp::Or => (settled_id, rhs_id, true),
            _ => return None,
        };

        self.terminate_current(
            MirTerminatorKind::Branch { condition: lhs_value, then_block, else_block },
            expr.span,
        );

        self.switch_to(rhs_id);
        let rhs_value = self.lower_expr_value(rhs)?;
        self.assign(result_place.clone(), rhs_value, ty, rhs.span);
        let rhs_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), expr.span);

        self.switch_to(settled_id);
        self.assign(
            result_place.clone(),
            MirOperand::Constant(MirConstant::Bool(settled_value)),
            ty,
            expr.span,
        );
        let settled_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), expr.span);

        if rhs_reaches || settled_reaches {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, expr.span);
        }

        Some(MirOperand::Temp(result))
    }

    /// Placeholder operand for an omitted `sponte` parameter without `vel`.
    ///
    /// The omitted value is `nihil`, but the argument still has to type as the
    /// parameter's declared slot. A callee resolved by MIR function id (the
    /// package linker's canonicalization step) carries no optional markers —
    /// the validator reads its signature from the MIR parameter list alone — so
    /// a bare `nil` operand there is a call-argument type mismatch against the
    /// declared `textus`/`numerus` parameter. Materializing the nil in a slot of
    /// the declared type keeps the argument assignable at every callee shape
    /// while the runtime value stays `nihil` (so `x is none` still holds).
    fn omitted_optional_operand(&mut self, ty: Option<TypeId>, span: Span) -> MirOperand {
        // WHY: an imported (package-linked) callee's declared parameter types
        // are not carried in `imported_function_params`; its signature keeps
        // the `optional` flag, which is what the validator exempts a bare nil
        // against.
        let Some(ty) = ty else {
            return MirOperand::Constant(MirConstant::Nil);
        };
        // A `ref`/`mut` parameter's MIR slot is the referent, not the view.
        let (_, ty) = self.types.peel_access_marker(ty);
        let ty = MirType::semantic(ty);
        self.assign_temp(MirValueKind::Operand(MirOperand::Constant(MirConstant::Nil)), ty, span)
    }

    fn type_zero_struct_operand(&mut self, def_id: DefId, mir_ty: MirType, span: Span) -> Option<MirOperand> {
        let fields = self.materialize_struct_fields(def_id, &[], aggregate::StructFieldFill::AllZero, span)?;
        Some(self.construct_temp(
            MirAggregateKind::Struct(def_id),
            MirAggregateFields::Named(fields),
            mir_ty,
            span,
        ))
    }

    /// M-007 zero of a no-initializer enum-typed local: the first variant in
    /// declaration order with a fully zeroed payload. A `varia` placeholder is
    /// assigned before any read, so any variant is sound; the first is the
    /// deterministic "zero" (mirrors C enum zero-init picking enumerator 0).
    /// Fails closed with a specific diagnostic when the enum has no variants or
    /// the chosen variant's payload metadata is missing.
    fn type_zero_enum_operand(&mut self, def_id: DefId, mir_ty: MirType, span: Span) -> Option<MirOperand> {
        let variants = self
            .context
            .enum_variants
            .get(&def_id)
            .cloned()
            .unwrap_or_default();
        let Some(&first) = variants.first() else {
            self.errors.push(MirError::unsupported(
                span,
                "enum type zero-init before MIR lowering completed: enum has no variants",
            ));
            return None;
        };
        let Some(field_names) = self.context.variant_fields.get(&first).cloned() else {
            self.errors.push(MirError::unsupported(
                span,
                "enum type zero-init before MIR lowering completed: variant payload metadata missing",
            ));
            return None;
        };
        let mut fields = Vec::with_capacity(field_names.len());
        for name in field_names {
            let Some(field_ty) = self
                .context
                .variant_field_tys
                .get(&first)
                .and_then(|tys| tys.get(&name))
                .copied()
            else {
                self.errors.push(MirError::unsupported(
                    span,
                    "enum type zero-init before MIR lowering completed: variant payload type metadata missing",
                ));
                return None;
            };
            let value = self.type_zero_operand(field_ty.semantic_id(), span)?;
            fields.push(MirNamedOperand { name, value });
        }
        Some(self.construct_temp(
            MirAggregateKind::EnumVariant(first),
            MirAggregateFields::Named(fields),
            mir_ty,
            span,
        ))
    }

    /// Type-shaped zero/default used at MIR lowering sites for voluntary slots.
    pub(super) fn type_zero_operand(&mut self, ty: TypeId, span: Span) -> Option<MirOperand> {
        let mir_ty = MirType::semantic(ty);
        match self.normalized_type(ty) {
            Type::Primitive(Primitive::Fractus) => Some(MirOperand::Constant(MirConstant::Float(0.0))),
            Type::Primitive(Primitive::Bivalens) => Some(MirOperand::Constant(MirConstant::Bool(false))),
            Type::Primitive(Primitive::Nihil | Primitive::Ignotum) | Type::Option(_) => {
                Some(MirOperand::Constant(MirConstant::Nil))
            }
            Type::Primitive(Primitive::Vacuum) => Some(MirOperand::Constant(MirConstant::Unit)),
            Type::Primitive(Primitive::Numerus) => Some(MirOperand::Constant(MirConstant::Int(0))),
            // SizedNumeric zeros are wrapped in a typed temp: a bare constant
            // carries the primitive `numerus`/`fractus` type, which the MIR
            // validator rejects when assigned to a sized field/local (e.g.
            // `f32` or `numerus<u8>`) without narrowing. The temp carries the
            // sized type, mirroring how the literal path types field values.
            Type::SizedNumeric(Primitive::Numerus, _) => {
                Some(self.assign_temp(MirValueKind::Operand(MirOperand::Constant(MirConstant::Int(0))), mir_ty, span))
            }
            // N7E-3: a policy-float cell zero-inits as the bare float of its width.
            Type::SizedNumeric(Primitive::Fractus, _) | Type::PolicyFloat(_, _) => Some(self.assign_temp(
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(0.0))),
                mir_ty,
                span,
            )),
            Type::Primitive(Primitive::Textus | Primitive::Ascii) => {
                Some(MirOperand::Constant(MirConstant::String(self.context.empty_textus)))
            }
            Type::Primitive(Primitive::Octeti) => Some(MirOperand::Constant(MirConstant::Octeti(Vec::new()))),
            Type::Primitive(Primitive::Regex) => {
                self.errors.push(MirError::unsupported(
                    span,
                    "regex type zero-init before MIR lowering completed",
                ));
                None
            }
            Type::Primitive(Primitive::Match) => {
                self.errors
                    .push(MirError::unsupported(span, "regex match type zero-init"));
                None
            }
            Type::Union(members) => {
                let nihil = self.types.primitive(Primitive::Nihil);
                if members.contains(&nihil) {
                    Some(MirOperand::Constant(MirConstant::Nil))
                } else {
                    // A Nihil-less value union takes the first member's zero,
                    // wrapped in a typed temp of the union type: a bare
                    // constant carries its member primitive type, which the
                    // validator rejects against the union destination.
                    let Some(&first) = members.first() else {
                        self.errors.push(MirError::unsupported(
                            span,
                            "union type zero-init before MIR lowering completed: union has no members",
                        ));
                        return None;
                    };
                    let member_zero = self.type_zero_operand(first, span)?;
                    Some(self.assign_temp(MirValueKind::Operand(member_zero), mir_ty, span))
                }
            }
            Type::Array(_) => Some(self.construct_temp(
                MirAggregateKind::Array,
                MirAggregateFields::Ordered(Vec::new()),
                mir_ty,
                span,
            )),
            Type::Map(_, _) | Type::Ledger(_, _) => {
                Some(self.construct_temp(MirAggregateKind::Map, MirAggregateFields::Keyed(Vec::new()), mir_ty, span))
            }
            Type::Set(_) => {
                Some(self.construct_temp(MirAggregateKind::Set, MirAggregateFields::Ordered(Vec::new()), mir_ty, span))
            }
            Type::Sparsa(_, _) => Some(self.construct_temp(
                MirAggregateKind::SparseTensor,
                MirAggregateFields::Ordered(Vec::new()),
                mir_ty,
                span,
            )),
            Type::Struct(def_id) => self.type_zero_struct_operand(*def_id, mir_ty, span),
            Type::Enum(def_id) => self.type_zero_enum_operand(*def_id, mir_ty, span),
            _ => {
                self.errors
                    .push(MirError::unsupported(span, "type zero-init before MIR lowering completed"));
                None
            }
        }
    }

    fn lower_is_predicate(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let result_ty = self.expr_ty(expr)?;
        if let HirExpressionKind::Field(object, member) = &rhs.kind
            && let HirExpressionKind::Path(import_def) = &object.kind
            && let Some(target_ty) = self
                .context
                .imported_type_members
                .get(&(*import_def, *member))
                .copied()
        {
            let operand = self.lower_expr_value(lhs)?;
            return Some(self.runtime_call_value(
                MirIntrinsic::TypeCheck(MirTypeCheckOp { positive: matches!(op, HirBinOp::Is), ty: target_ty }),
                vec![operand],
                result_ty,
                expr.span,
            ));
        }
        if matches!(rhs.kind, HirExpressionKind::Literal(HirLiteral::Nil)) {
            let operand = self.lower_expr_value(lhs)?;
            return Some(self.lower_nil_comparison(op, operand, result_ty, expr.span));
        }
        if matches!(lhs.kind, HirExpressionKind::Literal(HirLiteral::Nil)) {
            let operand = self.lower_expr_value(rhs)?;
            return Some(self.lower_nil_comparison(op, operand, result_ty, expr.span));
        }
        None
    }

    /// `est`/`non est` against a literal `nihil` collapses to a nil-check
    /// unary op (`est` → `IsNil`, `non est` → `IsNotNil`), matching the
    /// `air/to_mir.rs` rule that variant discrimination reduces to equality
    /// at this level.
    fn lower_nil_comparison(
        &mut self,
        op: HirBinOp,
        operand: MirOperand,
        result_ty: MirType,
        span: Span,
    ) -> MirOperand {
        self.lower_nil_test(matches!(op, HirBinOp::Is), operand, result_ty, span)
    }

    /// `est nihil` → `IsNil`, `non est nihil` → `IsNotNil`.
    fn lower_nil_test(&mut self, positive: bool, operand: MirOperand, result_ty: MirType, span: Span) -> MirOperand {
        let op = if positive { MirUnOp::IsNil } else { MirUnOp::IsNotNil };
        self.assign_temp(MirValueKind::Unary { op, operand }, result_ty, span)
    }

    /// `vel` short-circuits (operator ruling 2026-09-24): the fallback is
    /// evaluated only when the left operand is `nihil`. An effect-free
    /// fallback (literal, path) keeps the single `option coalesce` op, since
    /// eager evaluation is unobservable; any other fallback lowers behind a
    /// nil branch, mirroring `lower_logical_short_circuit`.
    fn lower_coalesce(&mut self, lhs: &HirExpression, rhs: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        // K4: `vel` over a tensor fills each `nihil` element; both operands are evaluated.
        if let Some(lift) = self.tensor_lift_binary_op(HirBinOp::Coalesce, lhs, rhs) {
            return self.lower_tensor_lift(lift, &[lhs, rhs], expr);
        }
        if rhs.is_effect_free_operand() {
            let value = self.lower_expr_value(lhs)?;
            let fallback = self.lower_expr_value(rhs)?;
            let ty = self.expr_ty(expr)?;
            return Some(self.assign_temp(
                MirValueKind::Option(MirOptionOp::Coalesce { value, fallback }),
                ty,
                expr.span,
            ));
        }
        self.lower_coalesce_short_circuit(lhs, rhs, expr)
    }

    fn lower_coalesce_short_circuit(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let result = self.push_temp(ty, expr.span);
        let result_place = MirPlace::temp(result);
        let value = self.lower_expr_value(lhs)?;
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let is_nil = self.assign_temp(
            MirValueKind::Unary { op: MirUnOp::IsNil, operand: value.clone() },
            bivalens,
            lhs.span,
        );

        let fallback_id = self.fresh_block(rhs.span);
        let present_id = self.fresh_block(lhs.span);
        let join_id = self.fresh_block(expr.span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: is_nil, then_block: fallback_id, else_block: present_id },
            expr.span,
        );

        self.switch_to(fallback_id);
        let fallback = self.lower_expr_value(rhs)?;
        self.assign(result_place.clone(), fallback, ty, rhs.span);
        let fallback_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), expr.span);

        self.switch_to(present_id);
        // WHY: a nullable result (`T ∪ nihil vel T ∪ nihil`) keeps the option
        // carrier; a non-nullable result takes the proven-present payload.
        let result_nullable = expr.ty.is_some_and(|ty| {
            self.types.nullable_inner(ty).is_some()
                || self
                    .types
                    .equals(ty, self.types.primitive(Primitive::Nihil))
        });
        let lhs_nullable = lhs
            .ty
            .is_some_and(|ty| self.types.nullable_inner(ty).is_some());
        let present = if lhs_nullable && !result_nullable {
            self.assign_temp(
                MirValueKind::Option(MirOptionOp::Unwrap { value, mode: MirOptionUnwrapMode::Assume }),
                ty,
                lhs.span,
            )
        } else {
            value
        };
        self.assign(result_place, present, ty, lhs.span);
        let present_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), expr.span);

        if fallback_reaches || present_reaches {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, expr.span);
        }
        Some(MirOperand::Temp(result))
    }

    /// An optional-chain link short-circuits (the optional-chain rule of the
    /// grammar: an absent base runs nothing after the `?`). The link's
    /// arguments and index are evaluated only on the present branch, so an
    /// effectful argument never runs for an absent base. A base that is not
    /// nullable, or a link with only effect-free operands, keeps the single
    /// `option chain` op: eager evaluation is unobservable there.
    pub(super) fn lower_option_chain_guarded(
        &mut self,
        base: &MirOperand,
        base_ty: Option<TypeId>,
        operands_effect_free: bool,
        ty: MirType,
        span: Span,
        build: impl FnOnce(&mut Self, &MirOperand) -> Option<MirOptionChainLink>,
    ) -> Option<MirOperand> {
        // WHY: a place operand would otherwise be read once by the nil test and
        // again after effectful index/call operands. Capture the receiver now,
        // before the guard and before those operands run.
        let base = if let MirOperand::Place(place) = base {
            let base_ty = MirType::semantic(base_ty?);
            self.assign_temp(MirValueKind::Operand(MirOperand::Place(place.clone())), base_ty, span)
        } else {
            base.clone()
        };
        let base_nullable = base_ty.is_some_and(|ty| self.types.nullable_inner(ty).is_some());
        if operands_effect_free || !base_nullable {
            let link = build(self, &base)?;
            return Some(self.assign_temp(MirValueKind::Option(MirOptionOp::Chain { base, link }), ty, span));
        }
        let result = self.push_temp(ty, span);
        let result_place = MirPlace::temp(result);
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let is_nil = self.assign_temp(
            MirValueKind::Unary { op: MirUnOp::IsNil, operand: base.clone() },
            bivalens,
            span,
        );
        let absent_id = self.fresh_block(span);
        let present_id = self.fresh_block(span);
        let join_id = self.fresh_block(span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: is_nil, then_block: absent_id, else_block: present_id },
            span,
        );

        self.switch_to(absent_id);
        let none = self.assign_temp(MirValueKind::Option(MirOptionOp::None), ty, span);
        self.assign(result_place.clone(), none, ty, span);
        let absent_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);

        self.switch_to(present_id);
        let link = build(self, &base)?;
        let chained = self.assign_temp(MirValueKind::Option(MirOptionOp::Chain { base: base.clone(), link }), ty, span);
        self.assign(result_place, chained, ty, span);
        let present_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);

        if absent_reaches || present_reaches {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, span);
        }
        Some(MirOperand::Temp(result))
    }

    fn assign(&mut self, place: MirPlace, operand: MirOperand, ty: MirType, span: Span) {
        let value = self.new_value(MirValueKind::Operand(operand), ty, span);
        self.append_stmt(MirStatement { kind: MirStatementKind::Assign { place, value }, span });
    }

    fn assign_temp(&mut self, kind: MirValueKind, ty: MirType, span: Span) -> MirOperand {
        MirOperand::Temp(self.assign_new_temp(kind, ty, span))
    }

    fn assign_new_temp(&mut self, kind: MirValueKind, ty: MirType, span: Span) -> MirTempId {
        let temp = self.push_temp(ty, span);
        let value = self.new_value(kind, ty, span);
        self.append_stmt(MirStatement { kind: MirStatementKind::Assign { place: MirPlace::temp(temp), value }, span });
        temp
    }

    fn record_closure_destination(&mut self, destination: &MirPlace, value: &MirOperand) {
        if !destination.projections.is_empty() {
            return;
        }
        let MirPlaceBase::Local(local) = destination.base else {
            return;
        };
        if let Some(closure) = self.closure_callee_for_operand(value) {
            self.closure_local_callees.insert(local, closure);
        } else {
            self.closure_local_callees.remove(&local);
        }
    }

    fn construct_temp(
        &mut self,
        kind: MirAggregateKind,
        fields: MirAggregateFields,
        ty: MirType,
        span: Span,
    ) -> MirOperand {
        let temp = self.push_temp(ty, span);
        self.append_stmt(MirStatement {
            kind: MirStatementKind::Construct {
                destination: MirPlace::temp(temp),
                aggregate: MirAggregate { kind, ty, fields },
            },
            span,
        });
        MirOperand::Temp(temp)
    }

    pub fn variant_payload(&mut self, variant: DefId, args: Vec<MirOperand>, span: Span) -> MirAggregateFields {
        let Some(field_names) = self.context.variant_fields.get(&variant) else {
            self.errors.push(MirError::unsupported(
                span,
                "variant payload field metadata missing at MIR lowering",
            ));
            return MirAggregateFields::Ordered(args.into_iter().map(MirAggregateItem::Operand).collect());
        };
        if field_names.len() != args.len() {
            self.errors.push(MirError::unsupported(
                span,
                format!(
                    "variant payload arity mismatch: expected {} fields {}, got {} argument{}",
                    field_names.len(),
                    field_names
                        .iter()
                        .map(|s| format!("{s:?}"))
                        .collect::<Vec<_>>()
                        .join(", "),
                    args.len(),
                    if args.len() == 1 { "" } else { "s" },
                ),
            ));
            return MirAggregateFields::Ordered(args.into_iter().map(MirAggregateItem::Operand).collect());
        }
        MirAggregateFields::Named(
            field_names
                .iter()
                .copied()
                .zip(args)
                .map(|(name, value)| MirNamedOperand { name, value })
                .collect(),
        )
    }

    fn new_value(&mut self, kind: MirValueKind, ty: MirType, span: Span) -> MirValue {
        let id = MirValueId(self.next_value);
        self.next_value += 1;
        MirValue { id, kind, ty, span }
    }

    fn push_temp(&mut self, ty: MirType, span: Span) -> MirTempId {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirTempId is u32 by design; lowered temp count is bounded by u32::MAX"
        )]
        let id = MirTempId(self.temps.len() as u32);
        self.temps.push(MirTemp { id, ty, span });
        id
    }

    pub fn fresh_block(&mut self, span: Span) -> MirBlockId {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirBlockId is u32 by design; lowered block count is bounded by u32::MAX"
        )]
        let id = MirBlockId(self.blocks.len() as u32);
        self.blocks
            .push(OpenBlock { id, statements: Vec::new(), terminator: None, span });
        id
    }

    pub fn switch_to(&mut self, block: MirBlockId) {
        if self.block(block).terminator.is_some() {
            self.current = None;
            return;
        }
        self.current = Some(block);
    }

    fn current_is_open(&self) -> bool {
        self.current
            .is_some_and(|id| self.block(id).terminator.is_none())
    }

    fn append_stmt(&mut self, stmt: MirStatement) {
        let Some(current) = self.current else {
            self.errors
                .push(MirError::unsupported(stmt.span, "statement after a MIR terminator"));
            return;
        };

        let block = self.block_mut(current);
        if block.terminator.is_some() {
            self.errors
                .push(MirError::unsupported(stmt.span, "statement after a MIR terminator"));
            self.current = None;
            return;
        }
        block.statements.push(stmt);
    }

    fn terminate_current(&mut self, kind: MirTerminatorKind, span: Span) -> bool {
        let Some(current) = self.current else {
            self.errors.push(MirError::unsupported(
                span,
                "terminator emitted after current MIR block was sealed",
            ));
            return false;
        };

        let block = self.block_mut(current);
        if block.terminator.is_some() {
            self.errors
                .push(MirError::unsupported(span, "duplicate MIR block terminator"));
            self.current = None;
            return false;
        }

        block.terminator = Some(MirTerminator { kind, span });
        self.current = None;
        self.last_terminated = Some(current);
        true
    }

    fn terminate_open_current(&mut self, kind: MirTerminatorKind, span: Span) -> bool {
        if !self.current_is_open() {
            return false;
        }
        self.terminate_current(kind, span)
    }

    fn seal_unreachable(&mut self, block: MirBlockId, span: Span) {
        let open = self.block(block).terminator.is_none();
        if open {
            self.block_mut(block).terminator = Some(MirTerminator { kind: MirTerminatorKind::Unreachable, span });
        }
        if self.current == Some(block) {
            self.current = None;
        }
    }

    fn finish_blocks(&mut self) -> Vec<MirBlock> {
        for index in 0..self.blocks.len() {
            if self.blocks[index].terminator.is_none() {
                let span = self.blocks[index].span;
                self.blocks[index].terminator = Some(MirTerminator { kind: MirTerminatorKind::Unreachable, span });
            }
        }

        self.blocks
            .drain(..)
            .map(|block| MirBlock {
                id: block.id,
                statements: block.statements,
                terminator: block
                    .terminator
                    .unwrap_or(MirTerminator { kind: MirTerminatorKind::Unreachable, span: block.span }),
                span: block.span,
            })
            .collect()
    }

    fn block(&self, id: MirBlockId) -> &OpenBlock {
        &self.blocks[id.0 as usize]
    }

    fn block_mut(&mut self, id: MirBlockId) -> &mut OpenBlock {
        &mut self.blocks[id.0 as usize]
    }

    fn next_local_id(&self) -> MirLocalId {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirLocalId is u32 by design; lowered local count is bounded by u32::MAX"
        )]
        MirLocalId(self.locals.len() as u32)
    }

    fn expr_ty(&mut self, expr: &HirExpression) -> Option<MirType> {
        let Some(ty) = expr.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "expression"));
            return None;
        };
        Some(MirType::semantic(ty))
    }

    /// The enum type of a variant-construction expression.
    ///
    /// WHY: a variant stored into an optional slot (`Color ∪ none o ← Color.Green`,
    /// a nullable class field) reaches lowering typed as the slot's `T ∪ none`,
    /// but the value itself is a `T`; the aggregate must carry the enum type and
    /// the store lifts it into the optional slot.
    pub(super) fn variant_value_ty(&mut self, expr: &HirExpression) -> Option<MirType> {
        let ty = self.expr_ty(expr)?;
        Some(
            self.types
                .nullable_inner(ty.semantic_id())
                .map_or(ty, MirType::semantic),
        )
    }

    fn is_vacuum(&self, ty: MirType) -> bool {
        ty.semantic_id() == self.types.primitive(Primitive::Vacuum)
    }

    fn is_addita_plus_operands(&self, lhs: &HirExpression, rhs: &HirExpression) -> bool {
        let (Some(lhs_ty), Some(rhs_ty)) = (lhs.ty, rhs.ty) else {
            return false;
        };
        matches!(
            (
                self.normalized_type(self.types.peel_access_marker(lhs_ty).1),
                self.normalized_type(self.types.peel_access_marker(rhs_ty).1)
            ),
            (Type::Tensor(_, _), Type::Tensor(_, _))
                | (Type::Vector(_, _), Type::Vector(_, _))
                | (Type::Matrix(_, _), Type::Matrix(_, _))
        )
    }

    /// Join decimal comparison operands at the d64 scaled carrier before the
    /// bool-valued MIR binary op. Shared by ordering and the ≅/≈ equality
    /// tiers (GT-1). The result type of a comparison is bivalens, so runner
    /// normalization keyed only from that result would otherwise miss the
    /// decimal operand widths entirely.
    fn lower_decimal_join_operands(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
    ) -> Option<(MirOperand, MirOperand)> {
        let Some((lhs_ty, rhs_ty, target_ty)) = self.decimal_join_target(lhs, rhs) else {
            let (l, r) = (self.lower_expr_value(lhs)?, self.lower_expr_value(rhs)?);
            // A decimal beside a float compares by exact value (ruling 22); the
            // runner reads the decimal's scale from the operand's static type,
            // so a constant operand is materialized into a typed temp.
            let l = self.typed_decimal_operand(lhs, l)?;
            let r = self.typed_decimal_operand(rhs, r)?;
            return Some((l, r));
        };
        let (lhs_expr, rhs_expr) = (lhs, rhs);
        let lhs = self.lower_join_operand(lhs_expr, lhs_ty, target_ty)?;
        let rhs = self.lower_join_operand(rhs_expr, rhs_ty, target_ty)?;
        // An operand already at the join type is read as it is; type it too.
        let lhs = self.typed_decimal_operand(lhs_expr, lhs)?;
        let rhs = self.typed_decimal_operand(rhs_expr, rhs)?;
        Some((lhs, rhs))
    }

    fn typed_decimal_operand(&mut self, expr: &HirExpression, operand: MirOperand) -> Option<MirOperand> {
        // A projected place (`xs[0]`, `p.x`, `t[0]`) carries no type of its own
        // either, so it is read into a typed temp like a constant.
        let untyped = match &operand {
            MirOperand::Constant(_) => true,
            MirOperand::Place(place) => !place.projections.is_empty(),
            _ => false,
        };
        if !untyped {
            return Some(operand);
        }
        let ty = self.expr_ty(expr)?;
        let decimal = matches!(
            self.normalized_type(self.types.peel_access_marker(ty.semantic_id()).1),
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal()
        );
        Some(if decimal {
            self.assign_temp(MirValueKind::Operand(operand), ty, expr.span)
        } else {
            operand
        })
    }

    fn decimal_join_target(&mut self, lhs: &HirExpression, rhs: &HirExpression) -> Option<(MirType, MirType, TypeId)> {
        let lhs_ty = self.expr_ty(lhs)?;
        let rhs_ty = self.expr_ty(rhs)?;
        let lhs_semantic = self.types.peel_access_marker(lhs_ty.semantic_id()).1;
        let rhs_semantic = self.types.peel_access_marker(rhs_ty.semantic_id()).1;
        let lhs_width = match self.normalized_type(lhs_semantic) {
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal() => Some(*width),
            _ => None,
        };
        let rhs_width = match self.normalized_type(rhs_semantic) {
            Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal() => Some(*width),
            _ => None,
        };
        if lhs_width.is_none() && rhs_width.is_none() {
            return None;
        }
        // Ruling 22: a decimal beside a non-constant integer compares by exact
        // value; joining the integer at `d64` would trap for a large one. The
        // runner reads both static types (a constant integer still joins).
        let plain_integer = |expr: &HirExpression, semantic: TypeId| {
            let ty = self.normalized_type(semantic);
            // Ruling 37: `inf` beside a decimal is an exact cross-family
            // comparison like any plain integer — constant or not (joining it
            // at `d64` would trap for a large value).
            if matches!(ty, Type::SizedNumeric(Primitive::Numerus, width) if width.is_unbounded()) {
                return true;
            }
            radix_semantic::passes::const_int_value(expr).is_none()
                && match ty {
                    Type::Primitive(Primitive::Numerus) => true,
                    Type::SizedNumeric(Primitive::Numerus, width) => width.integer_range().is_some(),
                    _ => false,
                }
        };
        if (lhs_width.is_some() && rhs_width.is_none() && plain_integer(rhs, rhs_semantic))
            || (rhs_width.is_some() && lhs_width.is_none() && plain_integer(lhs, lhs_semantic))
        {
            return None;
        }
        if matches!(
            self.normalized_type(lhs_semantic),
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) | Type::PolicyFloat(_, _)
        ) || matches!(
            self.normalized_type(rhs_semantic),
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) | Type::PolicyFloat(_, _)
        ) {
            return None;
        }
        // The semantic ordering gate interns d64 as the lossless join. Reuse
        // that existing TypeId because FunctionBuilder borrows the shared
        // TypeTable immutably; the fallback keeps direct-HIR tests usable.
        let d64 = (0..self.types.type_count()).find_map(|index| {
            let index = u32::try_from(index).ok()?;
            let ty = TypeId(index);
            matches!(
                self.normalized_type(ty),
                Type::SizedNumeric(Primitive::Numerus, NumericWidth::D64)
            )
            .then_some(ty)
        });
        let target = d64.unwrap_or(match (lhs_width, rhs_width) {
            (Some(NumericWidth::D64), _) | (None, None) => lhs_semantic,
            (_, Some(NumericWidth::D64)) => rhs_semantic,
            (Some(_), _) => lhs_semantic,
            (_, Some(_)) => rhs_semantic,
        });
        Some((lhs_ty, rhs_ty, target))
    }

    fn lower_join_operand(
        &mut self,
        expr: &HirExpression,
        source_ty: MirType,
        target_ty: TypeId,
    ) -> Option<MirOperand> {
        let value = self.lower_expr_value(expr)?;
        if self.types.equals(source_ty.semantic_id(), target_ty) {
            return Some(value);
        }
        let target_ty = MirType::semantic(target_ty);
        Some(self.runtime_call_value(
            MirIntrinsic::Convert(MirConversion {
                store: None,
                flavor: MirConversionFlavor::Runtime,
                source_ty,
                target_ty,
                params: Vec::new(),
                recovery: None,
                struct_defaults: Vec::new(),
            }),
            vec![value],
            target_ty,
            expr.span,
        ))
    }

    fn is_lista_or_queue_stack_arithmetic_operand(&self, lhs: &HirExpression, rhs: &HirExpression) -> bool {
        [lhs.ty, rhs.ty].into_iter().flatten().any(|ty| {
            matches!(
                self.normalized_type(ty),
                Type::Array(_)
                    | Type::BoundedArray { .. }
                    | Type::RangedArray { .. }
                    | Type::Queue(_)
                    | Type::BoundedQueue { .. }
                    | Type::Stack(_)
                    | Type::BoundedStack { .. }
            )
        })
    }

    fn normalized_type(&self, mut ty: TypeId) -> &Type {
        loop {
            match self.types.get(ty) {
                Type::Alias(_, inner) => ty = *inner,
                other => return other,
            }
        }
    }

    /// Carrier under `ref`/`mut`/`own`/`copy` wrappers.
    ///
    /// Typecheck peels access markers before method lookup. MIR method
    /// resolution must too: a `mut list.get` or `ref Struct.method()` otherwise
    /// fail-closes as "method call before runtime/provider MIR lowering".
    fn method_receiver_payload_id(&self, ty: TypeId) -> TypeId {
        self.types.peel_access_marker(ty).1
    }

    fn method_receiver_struct_def(&self, ty: TypeId) -> Option<DefId> {
        let payload = self.method_receiver_payload_id(ty);
        match self.types.get(payload) {
            Type::Struct(def_id) => Some(*def_id),
            Type::Applied(base, _) => match self.types.get(*base) {
                Type::Struct(def_id) => Some(*def_id),
                _ => None,
            },
            _ => None,
        }
    }
}

/// The MIR constant of an `inf` literal (canonical decimal digits, ruling
/// 34): the 64-bit constants when the value fits them, so arithmetic and
/// equality see one spelling, and an exact `Magnus` constant beyond.
pub(super) fn inf_literal_constant(digits: &str) -> MirConstant {
    match handroll::Magnus::parse_decimal(digits) {
        Ok(value) => {
            if let Some(v) = value.to_i64() {
                MirConstant::Int(v)
            } else if let Some(v) = value.to_u64() {
                MirConstant::UInt(v)
            } else {
                MirConstant::Magnus(value.to_string())
            }
        }
        Err(_) => MirConstant::Magnus(digits.to_owned()),
    }
}

fn mir_un_op(op: HirUnOp) -> MirUnOp {
    match op {
        HirUnOp::Neg => MirUnOp::Neg,
        HirUnOp::Not => MirUnOp::Not,
        HirUnOp::BitNot => MirUnOp::BitNot,
    }
}

fn mir_bin_op(op: HirBinOp) -> Option<MirBinOp> {
    match op {
        HirBinOp::Add => Some(MirBinOp::Add),
        HirBinOp::Sub => Some(MirBinOp::Sub),
        HirBinOp::Mul => Some(MirBinOp::Mul),
        HirBinOp::Div => Some(MirBinOp::Div),
        HirBinOp::Mod => Some(MirBinOp::Mod),
        HirBinOp::Eq | HirBinOp::StrictEq | HirBinOp::ApproxEq | HirBinOp::Is => Some(MirBinOp::Eq),
        HirBinOp::NotEq | HirBinOp::StrictNotEq | HirBinOp::ApproxNotEq | HirBinOp::IsNot => Some(MirBinOp::NotEq),
        HirBinOp::Lt => Some(MirBinOp::Lt),
        HirBinOp::Gt => Some(MirBinOp::Gt),
        HirBinOp::LtEq => Some(MirBinOp::LtEq),
        HirBinOp::GtEq => Some(MirBinOp::GtEq),
        HirBinOp::And => Some(MirBinOp::And),
        HirBinOp::Or => Some(MirBinOp::Or),
        HirBinOp::Coalesce => Some(MirBinOp::Coalesce),
        HirBinOp::BitAnd => Some(MirBinOp::BitAnd),
        HirBinOp::BitOr => Some(MirBinOp::BitOr),
        HirBinOp::BitXor => Some(MirBinOp::BitXor),
        HirBinOp::Shl => Some(MirBinOp::Shl),
        HirBinOp::Shr => Some(MirBinOp::Shr),
        // Fuzzy comparisons are intercepted by `lower_fuzzy_comparison` before
        // this point; the explicit arm fails closed for any future caller.
        // Membership (`Between`, `NotBetween`, `InRange`, `NotInRange`) is
        // intercepted by `lower_binary` before this point; the explicit arm
        // fails closed for any future caller.
        // Glyph products are intercepted by `lower_binary` before this point;
        // keep the explicit fail-closed arm for any future caller.
        // `÷` (d11-6 ruling 15) has no MIR primitive yet; fail closed.
        HirBinOp::FuzzyEq
        | HirBinOp::FuzzyNotEq
        | HirBinOp::InRange
        | HirBinOp::Between
        | HirBinOp::NotInRange
        | HirBinOp::NotBetween
        | HirBinOp::Dot
        | HirBinOp::Cross
        | HirBinOp::TensorProduct
        | HirBinOp::Hadamard
        | HirBinOp::HadamardDiv
        | HirBinOp::TrueDiv => None,
    }
}

fn unsupported_expr_kind_name(kind: &HirExpressionKind) -> &'static str {
    match kind {
        HirExpressionKind::MethodCall(..) => "method calls before receiver-aware MIR lowering",
        HirExpressionKind::Field(_, _) => "field access before aggregate place MIR lowering",
        HirExpressionKind::Index(_, _) => "index access before aggregate place MIR lowering",
        HirExpressionKind::OptionalChain(_, _) => "optional chains before nullable control-flow MIR lowering",
        HirExpressionKind::NonNull(_, _) => "non-null assertions before nullable control-flow MIR lowering",
        HirExpressionKind::Marked { .. } => "value-side markers before marked-value MIR lowering",
        HirExpressionKind::Gradient { .. } => {
            "gradient expressions before tensor-glyphs 2B backward-companion MIR lowering"
        }
        HirExpressionKind::Block(_) => "block expressions before nested-block MIR lowering",
        // UT: trap lowers through `visit_trap_expr`; kept exhaustive here for
        // any future caller.
        HirExpressionKind::Trap(_) => "capta trap before capture-boundary MIR lowering",
        HirExpressionKind::CallDefault { .. } => "call default before capture-boundary MIR lowering",
        HirExpressionKind::Si { .. } => "si before control-flow MIR lowering",
        HirExpressionKind::Discerne { .. } => "discerne before switch MIR lowering",
        HirExpressionKind::Loop(_) => "loop before control-flow MIR lowering",
        HirExpressionKind::Dum(_, _) => "dum before control-flow MIR lowering",
        HirExpressionKind::Itera { .. } => "itera before iterator MIR lowering",
        HirExpressionKind::IteraLockstep { .. } => {
            "lockstep itera before aligned-walk MIR lowering"
        }
        // Unreachable from the lowering dispatcher (the NR-3 range product
        // lowers through `visit_itera_range_expr`); kept so this naming
        // match stays exhaustive for any future caller.
        HirExpressionKind::IteraRange { .. } => {
            "range-product itera before ordered-walk MIR lowering"
        }
        HirExpressionKind::Reduction { .. } => "reducta_not_lowered",
        HirExpressionKind::Scan { .. } => "scan_not_lowered",
        HirExpressionKind::Sort { .. } => "sort_expression_not_lowered",
        HirExpressionKind::Intervallum { .. } => "range expressions before range MIR lowering",
        HirExpressionKind::Array(_) => "array literals before aggregate MIR lowering",
        HirExpressionKind::Struct(_, _) => "struct literals before aggregate MIR lowering",
        HirExpressionKind::Tuple(..) => "tuple literals before aggregate MIR lowering",
        HirExpressionKind::Scribe(kind, _) => scribe_kind_name(*kind),
        HirExpressionKind::Scriptum(_, _) => "scriptum templates before format intrinsic MIR lowering",
        HirExpressionKind::Format { .. } => "¶ format before format intrinsic MIR lowering",
        HirExpressionKind::ReadLine(_) => "lege before input MIR lowering",
        HirExpressionKind::Praefixum(_) => "praefixum before compile-time MIR lowering",
        HirExpressionKind::Insere(_) => "insere before the post-typecheck embed fold",
        HirExpressionKind::Widen { .. } => "bounded-to-inf widening before conversion MIR lowering",
        HirExpressionKind::Adfirma(_, _) => "adfirma before assert intrinsic MIR lowering",
        HirExpressionKind::Panic(_) => "mori fatal flow",
        HirExpressionKind::Throw(_) => "iace error-flow",
        HirExpressionKind::Handled { .. } => "structured cape before local-handler MIR lowering",
        HirExpressionKind::Clausura(..) => "closure expression",
        HirExpressionKind::Cede(_) => "legacy context-dependent cede carrier",
        HirExpressionKind::Verte { .. } => "verte before conversion MIR lowering",
        HirExpressionKind::Conversio { .. } => "conversio before runtime conversion MIR lowering",
        HirExpressionKind::ConversioAssign { .. } => {
            "conversio-assign before conversion-directed assignment MIR lowering"
        }
        HirExpressionKind::Ad { .. } => "ad expression before stream conversation MIR lowering",
        HirExpressionKind::Ref(_, _) => "references before borrow-aware MIR lowering",
        HirExpressionKind::Deref(_) => "dereferences before borrow-aware MIR lowering",
        HirExpressionKind::Error => "error expressions",
        HirExpressionKind::Path(_)
        | HirExpressionKind::Literal(_)
        | HirExpressionKind::Vacua
        | HirExpressionKind::Binary(_, _, _)
        | HirExpressionKind::Unary(_, _)
        | HirExpressionKind::Call(..)
        | HirExpressionKind::Assign(_, _)
        // Await/yield expression forms lower through their own dispatch arms
        // (`async_surface.rs`); they are unreachable here.
        | HirExpressionKind::Reddet(_)
        | HirExpressionKind::Tacebit(_)
        | HirExpressionKind::Yield(_)
        // CopyInto and SinkInto lower through their own visitor arms; they are
        // unreachable on the unsupported path.
        | HirExpressionKind::CopyInto { .. }
        | HirExpressionKind::SinkInto { .. }
        | HirExpressionKind::TypeCheck { .. } => "primitive expression",
    }
}

fn scribe_kind_name(kind: HirScribeKind) -> &'static str {
    match kind {
        HirScribeKind::Nota => "nota before print/runtime intrinsic MIR lowering",
        HirScribeKind::Vide => "vide before print/runtime intrinsic MIR lowering",
        HirScribeKind::Mone => "mone before print/runtime intrinsic MIR lowering",
        HirScribeKind::Scribe => "scribe before print/runtime intrinsic MIR lowering",
    }
}
