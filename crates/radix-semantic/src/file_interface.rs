//! Portable export contracts for file-as-namespace imports.
//!
//! `FileInterface` is the cross-unit boundary for imports: it records a file's
//! public exports without carrying producer-local `TypeId` or `DefId` values.
//! Importers remap structural snapshots into their own semantic `TypeTable`.

pub use crate::file_interface_default::InterfaceFieldDefault;
use crate::{
    FuncSig, GenericParamDescriptor, GenericParamSort, IndexExpr, IndexId, InstansPrecision,
    NumericWidth, ParamType, Primitive, Resolver, SemanticGenericParamDefault, SemanticMutability,
    SemanticParamMode, SymbolKind, Type, TypeId, TypeParamConstraint, TypeTable,
};
use radix_hir::DefId;
use radix_lexer::Interner;
use radix_syntax::GenericParamKind;
use radix_types::{BuiltinTensorEdgePolicy, FloatPolicy};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Export contract for one analyzed `.fab` file.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FileInterface {
    /// Stable provider/package/module identity for locale projection.
    ///
    /// Import binding aliases are deliberately not part of this identity. A
    /// missing identity keeps the historical canonical-name behavior.
    pub identity: Option<InterfaceLibraryIdentity>,
    pub exports: BTreeMap<String, FileExport>,
    /// Exported functions marked `@ nondum`, keyed by export name.
    ///
    /// The gate travels with the interface so an importer refuses every use of
    /// the function (call, namespace member call, value reference) exactly as
    /// the declaring file does. Strings, not interner symbols: the importer
    /// owns a different interner.
    pub unavailable: BTreeMap<String, Vec<InterfaceUnavailable>>,
    /// Routes this file serves with `@ ad '<route>'` handlers, keyed by the
    /// exact route. Routes are program-wide facts independent of export
    /// visibility: a private handler still serves its route (D6.10).
    pub routes: BTreeMap<String, RouteExport>,
}

impl FileInterface {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, export: FileExport) {
        self.exports.insert(export.name.clone(), export);
    }

    /// Attach the stable library module identity used by locale packs.
    #[must_use]
    pub fn with_identity(mut self, identity: InterfaceLibraryIdentity) -> Self {
        self.identity = Some(identity);
        self
    }
}

/// The `@ nondum [target] ["reason"]` gate of one exported function.
///
/// `target: None` means the function is unavailable on every target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceUnavailable {
    pub target: Option<String>,
    pub reason: Option<String>,
}

impl InterfaceUnavailable {
    /// Portable form of every `@ nondum` record in `availability`.
    #[must_use]
    pub fn from_availability(
        availability: &[radix_hir::HirAvailability],
        interner: &Interner,
    ) -> Vec<Self> {
        availability
            .iter()
            .map(
                |radix_hir::HirAvailability::Nondum { target, reason }| Self {
                    target: target.map(|symbol| interner.resolve(symbol).to_owned()),
                    reason: reason.map(|symbol| interner.resolve(symbol).to_owned()),
                },
            )
            .collect()
    }

    /// The first gate in `gates` that applies on the target named by
    /// `matches_target`; a gate without a target applies everywhere.
    #[must_use]
    pub fn applicable(gates: &[Self], matches_target: impl Fn(&str) -> bool) -> Option<&Self> {
        gates
            .iter()
            .find(|gate| gate.target.as_deref().is_none_or(&matches_target))
    }
}

/// Execution shape of one `@ ad` route handler (D6.11 table rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub enum RouteHandlerKind {
    /// `functio f(…) → T` — one `item`, then `done`.
    Sync,
    /// `… fiet T` — as `Sync`, produced by an async task.
    Async,
    /// `… fiunt T` — one `item` per yield, then `done`.
    Cursor,
    /// `… fient T` — as `Cursor`, produced by an async task.
    AsyncCursor,
}

/// One route served by a `@ ad '<route>'` handler in an analyzed file.
///
/// `opener` is the caller-sent type `O` (`None` = `nihil`, a zero-parameter
/// handler); `reply` is the per-item type `R` (`None` = `vacuum`, `done`
/// only). Snapshots are portable, like every other interface fact.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteExport {
    pub route: String,
    /// Handler function name in its defining module (no export required).
    pub handler: String,
    pub opener: Option<InterfaceTypeSnapshot>,
    pub reply: Option<InterfaceTypeSnapshot>,
    pub kind: RouteHandlerKind,
    /// Whether the handler declares a `⇥ E` failure channel.
    pub failable: bool,
    /// Span of the `@ ad` annotation in the defining file.
    pub span: radix_lexer::Span,
}

/// Portable identity for one library module's public interface.
///
/// This identity is independent of the consumer's local import binding and
/// therefore remains stable when a module is imported under another alias.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
pub struct InterfaceLibraryIdentity {
    pub provider: String,
    pub package: Option<String>,
    pub module_path: Vec<String>,
}

impl InterfaceLibraryIdentity {
    /// Stable display key for diagnostics, tests, and locale pack validation.
    #[must_use]
    pub fn display_key(&self) -> String {
        let mut parts = vec![self.provider.clone()];
        if let Some(package) = &self.package {
            parts.push(package.clone());
        }
        parts.extend(self.module_path.iter().cloned());
        parts.join("::")
    }
}

/// Public member category used by locale projection rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterfaceMemberKind {
    Function,
    Type,
    Const,
    Method,
    Field,
    Variant,
}

/// One top-level public export from a file interface.
#[derive(Debug, Clone, PartialEq)]
pub struct FileExport {
    pub name: String,
    pub kind: FileExportKind,
}

/// Public declaration kinds that can appear in a file interface.
#[derive(Debug, Clone, PartialEq)]
pub enum FileExportKind {
    Function(InterfaceCallable),
    TypeAlias(InterfaceAliasExport),
    Struct(InterfaceStructExport),
    Enum(InterfaceNominalExport),
    Interface(InterfaceNominalExport),
    /// Constant type plus its folded compile-time value when the exporter's
    /// initializer folds to a scalar or text; `None` keeps the importer
    /// fail-closed (an imported constant without a value is not constant in a
    /// consumer's `=` initializer).
    Const(InterfaceTypeSnapshot, Option<InterfaceConstValue>),
}

/// Folded compile-time value of an exported constant: the scalar or text the
/// exporter's initializer evaluates to (the `statica` fold domain), or an
/// ordered list literal whose elements all fold to scalars or text.
#[derive(Debug, Clone, PartialEq)]
pub enum InterfaceConstValue {
    Int(i128),
    Float(f64),
    Bool(bool),
    Text(String),
    /// A list constant's elements, in order. Elements are scalar values
    /// (never nested lists); a list that holds anything else carries no value.
    List(Vec<InterfaceConstValue>),
}

/// Type-alias export: ordered declaration parameters plus the resolved body.
///
/// S0-R1: declaration parameter kind/order rides the export so consumers can
/// kind-direct applied arguments to the imported alias without guessing from
/// identifier spelling.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceAliasExport {
    pub name: String,
    pub params: Vec<InterfaceGenericParam>,
    pub body: InterfaceTypeSnapshot,
}

/// One ordered generic-declaration parameter carried by a type export.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceGenericParam {
    pub name: String,
    pub kind: GenericParamKind,
}

impl InterfaceGenericParam {
    /// Ordered kind list for kind-directed application checking.
    #[must_use]
    pub fn kinds(params: &[Self]) -> Vec<GenericParamKind> {
        params.iter().map(|param| param.kind).collect()
    }
}

/// Struct declaration contract needed by importers to validate struct literals
/// and receiver method calls without copying source declarations into the
/// importing unit.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceStructExport {
    /// Whether the declaration opts into typed JSON conversion.
    pub is_json: bool,
    /// Contracts the genus names in its `implet` clause, each as a portable
    /// nominal (applied to its arguments when the contract is generic). The
    /// importer installs them as the struct's declared conformance; without
    /// them an imported genus is never assignable to a contract slot.
    pub implements: Vec<InterfaceTypeSnapshot>,
    pub name: String,
    /// Ordered generic-declaration parameters (`typus`/`magnitudo`).
    pub params: Vec<InterfaceGenericParam>,
    pub fields: Vec<InterfaceStructField>,
    /// Inherent methods (`genus` methods) remapped like interface methods,
    /// each with its member visibility (D5.3).
    pub methods: Vec<InterfaceMethodExport>,
    /// Present when the genus was marked `@ annotatio` in the exporting unit.
    pub annotation_contract: Option<InterfaceAnnotationContract>,
}

/// Portable annotation-contract schema exported with a genus.
///
/// Importers install this into the annotation-contract registry so applications
/// can resolve without re-reading the exporter's source AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceAnnotationContract {
    /// Attachment target token (`functio` in v1).
    pub target: String,
    pub fields: Vec<InterfaceAnnotationContractField>,
    /// Qualified portable identity for the contract declaration.
    ///
    /// Provider/package/module/export components allow importers and frameworks
    /// to match contracts without relying on unit-local `DefId` values.
    pub qualified_identity: Option<InterfaceQualifiedIdentity>,
}

/// Portable provider/package/module/export identity for cross-unit handoff.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct InterfaceQualifiedIdentity {
    pub provider: String,
    pub package: Option<String>,
    pub module_path: Vec<String>,
    pub export_name: String,
}

/// One portable field in an exported annotation-contract schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceAnnotationContractField {
    pub name: String,
    /// Scalar type spelling (`textus`, `numerus`, `fractus`, `bivalens`).
    pub ty: String,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceStructField {
    pub name: String,
    pub ty: InterfaceTypeSnapshot,
    pub optional: bool,
    pub required: bool,
    /// Producer-side compile-time initializer for imported class construction.
    pub default: Option<InterfaceFieldDefault>,
    /// Authored genus field modifier (D16.1); `None` for unmarked and
    /// variant/union payload fields.
    pub modifier: Option<radix_syntax::FieldModifier>,
    /// Member visibility (D5.3); `Publica` for unmarked and variant/union
    /// payload fields. Private fields still cross so construction literals
    /// can set them (D5.4) and access can be rejected by name.
    pub visibility: radix_syntax::MemberVisibility,
}

/// Nominal declarations whose import contract is their type identity.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceNominalExport {
    pub name: String,
    /// Ordered generic-declaration parameters (`typus`/`magnitudo`).
    pub params: Vec<InterfaceGenericParam>,
    pub methods: Vec<InterfaceMethodExport>,
    /// Enum variant declarations (name + optional payload fields). Carried
    /// across the import boundary so consumers can reference library enum
    /// variants as values and construct them (codex-gap S1 U2 VALUE members).
    pub variants: Vec<InterfaceEnumVariant>,
    /// Union `@ commune` fields shared by every variant. Empty for `ordo`
    /// members and unions without a commune region. Member access on the
    /// imported union type (`e.message`) reads this table, not variant
    /// payloads.
    pub shared_fields: Vec<InterfaceStructField>,
    /// Compiler contract role an implendum carries through
    /// `@ radix contract "<role>"` (DECISIONS D1.4): `"ordering"` marks
    /// norma's `Orderable`. `None` for every other nominal.
    pub radix_contract: Option<String>,
}

/// One enum variant carried by a [`InterfaceNominalExport`].
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceEnumVariant {
    pub name: String,
    /// Payload fields in declaration order; empty for unit variants.
    pub fields: Vec<InterfaceStructField>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceMethodExport {
    pub name: String,
    pub callable: InterfaceCallable,
    /// Member visibility of a `genus` method (D5.3); `Publica` for contract
    /// and nominal methods.
    pub visibility: radix_syntax::MemberVisibility,
    /// `@ nondum` gates of a `genus` method; empty for an available method.
    /// Carried in memory only, like [`FileInterface::unavailable`].
    pub unavailable: Vec<InterfaceUnavailable>,
}

/// Structural type snapshot independent from a producer `TypeTable`.
#[derive(Debug, Clone, PartialEq)]
pub enum InterfaceTypeSnapshot {
    Primitive(Primitive),
    TensorEdgePolicy(BuiltinTensorEdgePolicy),
    Array(Box<InterfaceTypeSnapshot>),
    /// `list<T, size N>`; a bounded list of raw `b8` bits is the bounded byte
    /// buffer and imports as `BoundedOcteti`.
    BoundedArray(Box<InterfaceTypeSnapshot>, InterfaceIndexSnapshot),
    /// `list<T, size lo…hi>` length range; `max` is `None` for no upper end.
    RangedArray {
        element: Box<InterfaceTypeSnapshot>,
        min: u64,
        max: Option<u64>,
    },
    Queue(Box<InterfaceTypeSnapshot>),
    BoundedQueue(Box<InterfaceTypeSnapshot>, InterfaceIndexSnapshot),
    Stack(Box<InterfaceTypeSnapshot>),
    BoundedStack(Box<InterfaceTypeSnapshot>, InterfaceIndexSnapshot),
    Map(Box<InterfaceTypeSnapshot>, Box<InterfaceTypeSnapshot>),
    Ledger(Box<InterfaceTypeSnapshot>, Box<InterfaceTypeSnapshot>),
    Record(BTreeMap<String, InterfaceTypeSnapshot>),
    Set(Box<InterfaceTypeSnapshot>),
    Promissum(Box<InterfaceTypeSnapshot>),
    PromissumFailable(Box<InterfaceTypeSnapshot>, Box<InterfaceTypeSnapshot>),
    Cursor(Box<InterfaceTypeSnapshot>),
    AsyncCursor(Box<InterfaceTypeSnapshot>, Box<InterfaceTypeSnapshot>),
    Tensor(Box<InterfaceTypeSnapshot>, InterfaceIndexSnapshot),
    Vector(Box<InterfaceTypeSnapshot>, InterfaceIndexSnapshot),
    Matrix(Box<InterfaceTypeSnapshot>, InterfaceIndexSnapshot),
    Sparsa(Box<InterfaceTypeSnapshot>, InterfaceIndexSnapshot),
    Atomic(Box<InterfaceTypeSnapshot>),
    Intervallum(Box<InterfaceTypeSnapshot>),
    SizedNumeric(Primitive, NumericWidth),
    Bits(radix_types::BitsWidth),
    ModularWord(NumericWidth),
    SaturatingWord(NumericWidth),
    RangedInt {
        policy: radix_types::IntPolicy,
        width: NumericWidth,
        lo: i128,
        hi: i128,
    },
    PolicyFloat(FloatPolicy, NumericWidth),
    SizedInstans(InstansPrecision),
    Option(Box<InterfaceTypeSnapshot>),
    Ref(SemanticMutability, Box<InterfaceTypeSnapshot>),
    Func(InterfaceCallable),
    Param(String),
    Applied(Box<InterfaceTypeSnapshot>, Vec<InterfaceAppliedArg>),
    Union(Vec<InterfaceTypeSnapshot>),
    Intersection(Vec<InterfaceTypeSnapshot>),
    Tuple(Vec<InterfaceTypeSnapshot>),
    Nominal(InterfaceNominalType),
}

/// Kinded applied argument in a portable applied-type snapshot.
///
/// S0-R1 mirror of `radix_types::AppliedArg`: a size argument serializes as an
/// index snapshot and never becomes a fake type argument.
#[derive(Debug, Clone, PartialEq)]
pub enum InterfaceAppliedArg {
    Type(InterfaceTypeSnapshot),
    Size(InterfaceIndexSnapshot),
}

/// Snapshot of a type-level index expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceIndexSnapshot {
    Literal(u64),
    Param(String),
    Affine {
        base: Box<InterfaceIndexSnapshot>,
        offset: i128,
    },
    Divide {
        numerator: Box<InterfaceIndexSnapshot>,
        divisor: u64,
    },
    Tuple(Vec<InterfaceIndexSnapshot>),
    RuntimeTensorParameterAxis {
        parameter: u32,
        axis: u32,
    },
    RuntimeTensorParameterFieldAxis {
        parameter: u32,
        fields: Vec<String>,
        axis: u32,
    },
    RuntimeScalarParameter {
        parameter: u32,
    },
    Unspecified,
}

/// Callable signature snapshot used by function exports and function types.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceCallable {
    /// Ordered generic-declaration parameters (`typus`/`magnitudo`) with
    /// constraint and default.
    pub type_params: Vec<InterfaceCallableParam>,
    /// Generic type parameters whose value is a compile-time tensor edge
    /// policy selector in an exported body. The element type and rank keep
    /// the obligation portable without exporting producer `TypeId`s.
    pub tensor_edge_policy_requirements: Vec<InterfaceTensorEdgePolicyRequirement>,
    pub params: Vec<InterfaceParam>,
    pub ret: Box<InterfaceTypeSnapshot>,
    pub err: Option<Box<InterfaceTypeSnapshot>>,
    pub is_async: bool,
    pub is_generator: bool,
    /// v2 device contract (`@ nucleum`) riding the callable export (DFV2-3).
    ///
    /// `None` for ordinary callables and for function-type snapshots; the
    /// device fact exists only where an exporting declaration carries the
    /// annotation. The role fact never travels without the body channel.
    pub device: Option<InterfaceDeviceContract>,
}

impl InterfaceCallable {
    /// Record which parameters carry the `axis` marker, in declaration order.
    ///
    /// A [`FuncSig`] collapses `axis` into [`SemanticParamMode::Owned`], so a
    /// producer holding the lowered declaration applies the separable flag
    /// after snapshotting: the interface record keeps it and the importer's
    /// echo rule can check imported calls like local ones.
    pub fn set_axis_params(&mut self, axis: &[bool]) {
        for (param, axis) in self.params.iter_mut().zip(axis) {
            param.axis = *axis;
        }
    }
}

/// Portable `limes<P>()` obligation on one exported generic type parameter.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceTensorEdgePolicyRequirement {
    /// Name of the ordered generic type parameter used as the policy selector.
    pub policy_param: String,
    /// Tensor element type checked by the provider callbacks.
    pub element_type: InterfaceTypeSnapshot,
    /// Statically known tensor rank.
    pub rank: usize,
}

/// v2 device contract for one exported `@ nucleum` callable (DFV2-3).
///
/// Design `device-fragments` §9: kernel-ness crosses the import boundary —
/// an imported `@ nucleum` function is a device function of the consuming
/// unit — and the body channel travels with it. A role fact alone never
/// admits a device call; [`Self::body`] decides composability.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceDeviceContract {
    /// `@ nucleum` (compute Kernel) or `@ nucleum fragment` (composition
    /// Fragment). Mirrors `MirDeviceRole`; graphics `@ fragment` never
    /// appears here.
    pub role: InterfaceDeviceRole,
    /// Launch-surface gate: `@ publica`/`@ public` marks a launchable entry;
    /// an unmarked device function is a module-internal helper.
    pub visibility: InterfaceDeviceVisibility,
    /// Explicit body availability and source for cross-unit composition.
    pub body: InterfaceBodyChannel,
}

impl InterfaceDeviceContract {
    /// Whether the body channel can back a concrete device composition.
    ///
    /// Fail-closed: only a present source body or a canonical artifact is
    /// composable; a signature alone never is.
    #[must_use]
    pub fn is_composable(&self) -> bool {
        !matches!(self.body, InterfaceBodyChannel::Missing)
    }
}

/// Device role carried by an exported `@ nucleum` callable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterfaceDeviceRole {
    /// `@ nucleum` — device function; launchable entry when public.
    Kernel,
    /// `@ nucleum fragment` — composition helper, never a dispatch stage.
    Fragment,
}

/// Launch visibility carried by an exported device callable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterfaceDeviceVisibility {
    /// `@ nucleum @ publica` — dispatch/reflection entry in the consumer.
    Public,
    /// Unmarked `@ nucleum` — composable helper, never a launch entry.
    Private,
}

/// Explicit body channel for an imported device callable (DFV2-3).
///
/// The load-bearing `gradus owns all ML kernel source` fact: role propagation
/// without body propagation recreates the pinned import-loss red, so the
/// channel states are explicit. [`InterfaceBodyChannel::Missing`] is a
/// compile-time failure at use, never a silent signature-only fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InterfaceBodyChannel {
    /// The exporter's source body is available to this consumer (the
    /// interface was snapshotted from the exporter's source).
    Source,
    /// A serialized canonical body artifact is available for cross-package
    /// composition (durable artifact production is DFV2-5).
    Canonical(InterfaceCanonicalBody),
    /// No usable body: the export is signature-only (or the artifact is
    /// incompatible). Missing and incompatible share one fail-closed state.
    Missing,
}

/// Serialized canonical body artifact riding the body channel.
///
/// Opaque to the transport: `template` is the canonical body bytes and
/// `body_hash` the deterministic digest over them (DFV2-5 owns durable
/// production and the full artifact identity; DFV2-4 consumes the payload).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceCanonicalBody {
    /// Canonical serialized body template bytes.
    pub template: Vec<u8>,
    /// FNV-1a/64 digest over [`Self::template`], hex-encoded. Deterministic
    /// across processes so cache identity (body hash) is stable.
    pub body_hash: String,
}

impl InterfaceCanonicalBody {
    /// Wrap canonical template bytes, hashing them.
    #[must_use]
    pub fn new(template: Vec<u8>) -> Self {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in &template {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        Self {
            template,
            body_hash: format!("{hash:016x}"),
        }
    }
}

/// One declared generic parameter of an exported callable.
///
/// Kind, constraint, and default travel with the name so importers rebuild
/// [`GenericParamDescriptor`] rows without parallel vectors or shape
/// occurrence.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceCallableParam {
    pub name: String,
    pub kind: GenericParamKind,
    pub constraint: InterfaceTypeParamConstraint,
    pub default: Option<InterfaceGenericParamDefault>,
}

/// Snapshot form of a generic parameter's declaration-time default.
///
/// The variant always matches the parameter's [`GenericParamKind`].
#[derive(Debug, Clone, PartialEq)]
pub enum InterfaceGenericParamDefault {
    Typus(Box<InterfaceTypeSnapshot>),
    Magnitudo(u64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum InterfaceTypeParamConstraint {
    Any,
    OneOf(Vec<InterfaceTypeSnapshot>),
}

/// Parameter type and passing mode for a callable snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct InterfaceParam {
    pub ty: InterfaceTypeSnapshot,
    pub mode: SemanticParamMode,
    pub optional: bool,
    /// Whether the parameter is a source `sponte` slot (accepts an optional
    /// argument), as opposed to a merely defaulted, omittable parameter.
    pub sponte: bool,
    /// Whether the parameter is declared with the `axis` marker (need
    /// bc775857). Semantic `mode` collapses `axis` to `Owned` (the parameter
    /// is an ordinary int inside the body), so the echo rule needs this
    /// separable fact to ride the exported parameter record across a file
    /// interface; otherwise an imported call skips the `axis` echo check.
    pub axis: bool,
}

/// Nominal type identity that must not be guessed during Stage 1 import.
///
/// `identity` is the canonical module-qualified key of the nominal's home
/// module (provider + package + module path). Importers resolve nested
/// nominals through this key so the same nominal referenced from a library
/// analysis and from the consumer analysis unifies to one semantic type in
/// the merged package-MIR program (codex-gap S1 U2 nominal identity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InterfaceNominalType {
    pub kind: InterfaceNominalKind,
    pub name: String,
    /// Canonical module-qualified identity of the nominal's home module.
    pub identity: Option<InterfaceLibraryIdentity>,
    /// The AST export snapshot guesses the kind of names the exporting module
    /// does not declare locally (qualified references default to Struct).
    /// When set, importers may resolve the nominal through its module
    /// identity or name under any sibling kind instead of failing on the
    /// guessed kind alone (cds-u6 G4). Authoritative semantic snapshots
    /// (faber package path) leave this false and stay kind-exact
    /// (council-11 `correct_before_next_phase`).
    pub kind_guessed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InterfaceNominalKind {
    Struct,
    Enum,
    Interface,
}

/// Structured failure from snapshot or import operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileInterfaceError {
    NominalTypeUnsupported {
        kind: InterfaceNominalKind,
    },
    NominalTypeAmbiguous {
        kind: InterfaceNominalKind,
        name: String,
    },
    /// Identity-bearing nominal snapshot whose canonical module-qualified key
    /// the consumer's canonical table does not know. Fail-closed (council-11
    /// `correct_before_next_phase`): a snapshot carrying a canonical identity
    /// must never fall through to legacy short-name / local-symbol resolution,
    /// which could silently bind an unknown nominal to a different consumer
    /// nominal. Legacy name-based resolution remains available only for
    /// identity-less legacy snapshots.
    NominalIdentityUnknown {
        kind: InterfaceNominalKind,
        name: String,
    },
    InferenceTypeUnsupported,
    ErrorTypeUnsupported,
    FieldDefaultUnsupported,
    IndexInferenceUnsupported,
    TensorEdgePolicyRequirementUnsupported,
    InvalidIndexArithmetic(radix_types::IndexArithmeticError),
}

pub type FileInterfaceResult<T> = Result<T, FileInterfaceError>;

// TYPE SNAPSHOT

/// Snapshot a semantic type into a portable interface type.
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when the type is inference-only, an error
/// type, a tensor index still inferring, or a nominal that cannot be named
/// without a resolver.
pub fn snapshot_interface_type(
    type_id: TypeId,
    types: &TypeTable,
    interner: &Interner,
) -> FileInterfaceResult<InterfaceTypeSnapshot> {
    snapshot_interface_type_inner(type_id, types, interner, None, None)
}

/// Snapshot a semantic type with access to nominal declaration names.
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when the type is inference-only, an error
/// type, a tensor index still inferring, or a nominal whose declaration name
/// cannot be recovered.
pub fn snapshot_interface_type_with_resolver(
    type_id: TypeId,
    types: &TypeTable,
    interner: &Interner,
    resolver: &Resolver,
) -> FileInterfaceResult<InterfaceTypeSnapshot> {
    snapshot_interface_type_with_identity(type_id, types, interner, resolver, None)
}

/// Snapshot a semantic type with access to nominal declaration names and the
/// canonical module-qualified identity of the snapshotting module.
///
/// Nested nominal snapshots carry the nominal's home-module identity so the
/// consumer resolves them through the canonical nominal-identity key
/// (codex-gap S1 U2). `module_identity` is the snapshotting module's own
/// identity; nominals declared by *another* imported module resolve their
/// identity through the resolver's file-interface table.
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when the type is inference-only, an error
/// type, a tensor index still inferring, or a nominal whose declaration name
/// cannot be recovered.
pub fn snapshot_interface_type_with_identity(
    type_id: TypeId,
    types: &TypeTable,
    interner: &Interner,
    resolver: &Resolver,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> FileInterfaceResult<InterfaceTypeSnapshot> {
    snapshot_interface_type_inner(type_id, types, interner, Some(resolver), module_identity)
}

fn snapshot_interface_type_inner(
    type_id: TypeId,
    types: &TypeTable,
    interner: &Interner,
    resolver: Option<&Resolver>,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> FileInterfaceResult<InterfaceTypeSnapshot> {
    match types.get(type_id) {
        Type::Primitive(primitive) => Ok(InterfaceTypeSnapshot::Primitive(*primitive)),
        Type::TensorEdgePolicy(policy) => Ok(InterfaceTypeSnapshot::TensorEdgePolicy(*policy)),
        Type::Array(element) => Ok(InterfaceTypeSnapshot::Array(Box::new(
            snapshot_interface_type_inner(*element, types, interner, resolver, module_identity)?,
        ))),
        Type::BoundedArray { element, capacity } => Ok(InterfaceTypeSnapshot::BoundedArray(
            Box::new(snapshot_interface_type_inner(
                *element,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            snapshot_interface_index(*capacity, types, interner)?,
        )),
        Type::RangedArray { element, min, max } => Ok(InterfaceTypeSnapshot::RangedArray {
            element: Box::new(snapshot_interface_type_inner(
                *element,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            min: *min,
            max: *max,
        }),
        Type::BoundedOcteti { capacity } => Ok(InterfaceTypeSnapshot::BoundedArray(
            Box::new(InterfaceTypeSnapshot::Bits(radix_types::BitsWidth::B8)),
            snapshot_interface_index(*capacity, types, interner)?,
        )),
        Type::Queue(element) => Ok(InterfaceTypeSnapshot::Queue(Box::new(
            snapshot_interface_type_inner(*element, types, interner, resolver, module_identity)?,
        ))),
        Type::BoundedQueue { element, capacity } => Ok(InterfaceTypeSnapshot::BoundedQueue(
            Box::new(snapshot_interface_type_inner(
                *element,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            snapshot_interface_index(*capacity, types, interner)?,
        )),
        Type::Stack(element) => Ok(InterfaceTypeSnapshot::Stack(Box::new(
            snapshot_interface_type_inner(*element, types, interner, resolver, module_identity)?,
        ))),
        Type::BoundedStack { element, capacity } => Ok(InterfaceTypeSnapshot::BoundedStack(
            Box::new(snapshot_interface_type_inner(
                *element,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            snapshot_interface_index(*capacity, types, interner)?,
        )),
        Type::Ledger(key, value) => Ok(InterfaceTypeSnapshot::Ledger(
            Box::new(snapshot_interface_type_inner(
                *key,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            Box::new(snapshot_interface_type_inner(
                *value,
                types,
                interner,
                resolver,
                module_identity,
            )?),
        )),
        Type::Map(key, value) => Ok(InterfaceTypeSnapshot::Map(
            Box::new(snapshot_interface_type_inner(
                *key,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            Box::new(snapshot_interface_type_inner(
                *value,
                types,
                interner,
                resolver,
                module_identity,
            )?),
        )),
        Type::Record(fields) => fields
            .iter()
            .map(|(name, field_ty)| {
                Ok((
                    interner.resolve(*name).to_owned(),
                    snapshot_interface_type_inner(
                        *field_ty,
                        types,
                        interner,
                        resolver,
                        module_identity,
                    )?,
                ))
            })
            .collect::<FileInterfaceResult<BTreeMap<_, _>>>()
            .map(InterfaceTypeSnapshot::Record),
        Type::Set(element) => Ok(InterfaceTypeSnapshot::Set(Box::new(
            snapshot_interface_type_inner(*element, types, interner, resolver, module_identity)?,
        ))),
        Type::Promissum(inner) => Ok(InterfaceTypeSnapshot::Promissum(Box::new(
            snapshot_interface_type_inner(*inner, types, interner, resolver, module_identity)?,
        ))),
        Type::PromissumFailable(success, alternate) => {
            Ok(InterfaceTypeSnapshot::PromissumFailable(
                Box::new(snapshot_interface_type_inner(
                    *success,
                    types,
                    interner,
                    resolver,
                    module_identity,
                )?),
                Box::new(snapshot_interface_type_inner(
                    *alternate,
                    types,
                    interner,
                    resolver,
                    module_identity,
                )?),
            ))
        }
        Type::Cursor(inner) => Ok(InterfaceTypeSnapshot::Cursor(Box::new(
            snapshot_interface_type_inner(*inner, types, interner, resolver, module_identity)?,
        ))),
        Type::AsyncCursor(item, alternate) => Ok(InterfaceTypeSnapshot::AsyncCursor(
            Box::new(snapshot_interface_type_inner(
                *item,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            Box::new(snapshot_interface_type_inner(
                *alternate,
                types,
                interner,
                resolver,
                module_identity,
            )?),
        )),
        Type::Tensor(element, shape) => Ok(InterfaceTypeSnapshot::Tensor(
            Box::new(snapshot_interface_type_inner(
                *element,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            snapshot_interface_index(*shape, types, interner)?,
        )),
        Type::Vector(element, width) => Ok(InterfaceTypeSnapshot::Vector(
            Box::new(snapshot_interface_type_inner(
                *element,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            snapshot_interface_index(*width, types, interner)?,
        )),
        Type::Matrix(element, shape) => Ok(InterfaceTypeSnapshot::Matrix(
            Box::new(snapshot_interface_type_inner(
                *element,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            snapshot_interface_index(*shape, types, interner)?,
        )),
        Type::Sparsa(element, shape) => Ok(InterfaceTypeSnapshot::Sparsa(
            Box::new(snapshot_interface_type_inner(
                *element,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            snapshot_interface_index(*shape, types, interner)?,
        )),
        Type::Atomic(element) => Ok(InterfaceTypeSnapshot::Atomic(Box::new(
            snapshot_interface_type_inner(*element, types, interner, resolver, module_identity)?,
        ))),
        Type::Intervallum(bound, _) => Ok(InterfaceTypeSnapshot::Intervallum(Box::new(
            snapshot_interface_type_inner(*bound, types, interner, resolver, module_identity)?,
        ))),
        Type::SizedNumeric(primitive, width) => {
            Ok(InterfaceTypeSnapshot::SizedNumeric(*primitive, *width))
        }
        Type::Bits(width) => Ok(InterfaceTypeSnapshot::Bits(*width)),
        Type::ModularWord(width) => Ok(InterfaceTypeSnapshot::ModularWord(*width)),
        Type::SaturatingWord(width) => Ok(InterfaceTypeSnapshot::SaturatingWord(*width)),
        Type::RangedInt {
            policy,
            width,
            lo,
            hi,
        } => Ok(InterfaceTypeSnapshot::RangedInt {
            policy: *policy,
            width: *width,
            lo: *lo,
            hi: *hi,
        }),
        Type::PolicyFloat(policy, width) => Ok(InterfaceTypeSnapshot::PolicyFloat(*policy, *width)),
        Type::SizedInstans(precision) => Ok(InterfaceTypeSnapshot::SizedInstans(*precision)),
        Type::Option(inner) => Ok(InterfaceTypeSnapshot::Option(Box::new(
            snapshot_interface_type_inner(*inner, types, interner, resolver, module_identity)?,
        ))),
        Type::Ref(mutability, inner) => Ok(InterfaceTypeSnapshot::Ref(
            *mutability,
            Box::new(snapshot_interface_type_inner(
                *inner,
                types,
                interner,
                resolver,
                module_identity,
            )?),
        )),
        Type::Struct(def_id) => snapshot_nominal_type(
            *def_id,
            InterfaceNominalKind::Struct,
            interner,
            resolver,
            module_identity,
        ),
        Type::Enum(def_id) => snapshot_nominal_type(
            *def_id,
            InterfaceNominalKind::Enum,
            interner,
            resolver,
            module_identity,
        ),
        Type::Interface(def_id) => snapshot_nominal_type(
            *def_id,
            InterfaceNominalKind::Interface,
            interner,
            resolver,
            module_identity,
        ),
        Type::Alias(_, aliased) => {
            snapshot_interface_type_inner(*aliased, types, interner, resolver, module_identity)
        }
        Type::Func(sig) => Ok(InterfaceTypeSnapshot::Func(
            snapshot_interface_callable_inner(sig, types, interner, resolver, module_identity)?,
        )),
        Type::Param(name) => Ok(InterfaceTypeSnapshot::Param(
            interner.resolve(*name).to_owned(),
        )),
        Type::Applied(base, args) => Ok(InterfaceTypeSnapshot::Applied(
            Box::new(snapshot_interface_type_inner(
                *base,
                types,
                interner,
                resolver,
                module_identity,
            )?),
            args.iter()
                .map(|arg| match arg {
                    radix_types::AppliedArg::Type(id) => snapshot_interface_type_inner(
                        *id,
                        types,
                        interner,
                        resolver,
                        module_identity,
                    )
                    .map(InterfaceAppliedArg::Type),
                    radix_types::AppliedArg::Size(index) => {
                        snapshot_interface_index(*index, types, interner)
                            .map(InterfaceAppliedArg::Size)
                    }
                })
                .collect::<FileInterfaceResult<Vec<_>>>()?,
        )),
        Type::Infer(_)
        | Type::InferUnion(_)
        | Type::MarkerHole { .. }
        | Type::BoundedTextus { .. }
        | Type::BoundedAscii { .. } => Err(FileInterfaceError::InferenceTypeUnsupported),
        Type::Union(members) => members
            .iter()
            .map(|member| {
                snapshot_interface_type_inner(*member, types, interner, resolver, module_identity)
            })
            .collect::<FileInterfaceResult<Vec<_>>>()
            .map(InterfaceTypeSnapshot::Union),
        Type::Intersection(members) => members
            .iter()
            .map(|member| {
                snapshot_interface_type_inner(*member, types, interner, resolver, module_identity)
            })
            .collect::<FileInterfaceResult<Vec<_>>>()
            .map(InterfaceTypeSnapshot::Intersection),
        Type::Tuple(elements) => elements
            .iter()
            .map(|element| {
                snapshot_interface_type_inner(*element, types, interner, resolver, module_identity)
            })
            .collect::<FileInterfaceResult<Vec<_>>>()
            .map(InterfaceTypeSnapshot::Tuple),
        Type::Error => Err(FileInterfaceError::ErrorTypeUnsupported),
    }
}

/// Snapshot one declared `implet` contract as a portable nominal reference,
/// applied to its arguments when the contract is generic.
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when the contract or an argument has no
/// portable form.
pub fn snapshot_contract_ref_with_identity(
    contract: &radix_types::ContractRef,
    types: &TypeTable,
    interner: &Interner,
    resolver: &Resolver,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> FileInterfaceResult<InterfaceTypeSnapshot> {
    let base = snapshot_nominal_type(
        contract.contract,
        InterfaceNominalKind::Interface,
        interner,
        Some(resolver),
        module_identity,
    )?;
    if contract.args.is_empty() {
        return Ok(base);
    }
    let args = contract
        .args
        .iter()
        .map(|arg| {
            snapshot_interface_type_inner(*arg, types, interner, Some(resolver), module_identity)
                .map(InterfaceAppliedArg::Type)
        })
        .collect::<FileInterfaceResult<Vec<_>>>()?;
    Ok(InterfaceTypeSnapshot::Applied(Box::new(base), args))
}

/// Import one portable `implet` contract reference into the target table.
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when the snapshot does not resolve to an
/// interface nominal (a struct or enum named in `implet` is not a contract).
pub fn import_contract_ref_with_resolver(
    snapshot: &InterfaceTypeSnapshot,
    types: &mut TypeTable,
    interner: &mut Interner,
    resolver: &Resolver,
) -> FileInterfaceResult<radix_types::ContractRef> {
    let ty = import_interface_type_inner(snapshot, types, interner, Some(resolver))?;
    let contract_ref = match types.get(ty) {
        Type::Interface(contract) => Some(radix_types::ContractRef {
            contract: *contract,
            args: Vec::new(),
        }),
        Type::Applied(base, args) => match types.get(*base) {
            Type::Interface(contract) => args
                .iter()
                .map(|arg| match arg {
                    radix_types::AppliedArg::Type(arg) => Some(*arg),
                    radix_types::AppliedArg::Size(_) => None,
                })
                .collect::<Option<Vec<_>>>()
                .map(|args| radix_types::ContractRef {
                    contract: *contract,
                    args,
                }),
            _ => None,
        },
        _ => None,
    };
    contract_ref.ok_or(FileInterfaceError::NominalTypeUnsupported {
        kind: InterfaceNominalKind::Interface,
    })
}

fn snapshot_nominal_type(
    def_id: DefId,
    kind: InterfaceNominalKind,
    interner: &Interner,
    resolver: Option<&Resolver>,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> FileInterfaceResult<InterfaceTypeSnapshot> {
    let Some(resolver) = resolver else {
        return Err(FileInterfaceError::NominalTypeUnsupported { kind });
    };
    let Some(symbol) = resolver.get_symbol(def_id) else {
        return Err(FileInterfaceError::NominalTypeUnsupported { kind });
    };
    let identity = resolver
        .nominal_module_identity(def_id)
        .or_else(|| module_identity.cloned());
    Ok(InterfaceTypeSnapshot::Nominal(InterfaceNominalType {
        kind,
        name: interner.resolve(symbol.name).to_owned(),
        identity,
        kind_guessed: false,
    }))
}

// TYPE IMPORT

/// Import a portable interface type into the target semantic type table.
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when a nested nominal cannot be remapped
/// without a resolver.
pub fn import_interface_type(
    snapshot: &InterfaceTypeSnapshot,
    types: &mut TypeTable,
    interner: &mut Interner,
) -> FileInterfaceResult<TypeId> {
    import_interface_type_inner(snapshot, types, interner, None)
}

/// Import a portable interface type with access to collected local declarations.
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when a nominal is missing, ambiguous, or
/// carries a canonical identity the consumer does not know.
pub fn import_interface_type_with_resolver(
    snapshot: &InterfaceTypeSnapshot,
    types: &mut TypeTable,
    interner: &mut Interner,
    resolver: &Resolver,
) -> FileInterfaceResult<TypeId> {
    import_interface_type_inner(snapshot, types, interner, Some(resolver))
}

fn import_interface_type_inner(
    snapshot: &InterfaceTypeSnapshot,
    types: &mut TypeTable,
    interner: &mut Interner,
    resolver: Option<&Resolver>,
) -> FileInterfaceResult<TypeId> {
    match snapshot {
        InterfaceTypeSnapshot::Primitive(primitive) => Ok(types.primitive(*primitive)),
        InterfaceTypeSnapshot::TensorEdgePolicy(policy) => Ok(types.tensor_edge_policy(*policy)),
        InterfaceTypeSnapshot::Array(element) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            Ok(types.array(element))
        }
        InterfaceTypeSnapshot::BoundedArray(element, capacity) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            let capacity = import_interface_index(capacity, types, interner)?;
            Ok(types.bounded_array(element, capacity))
        }
        InterfaceTypeSnapshot::RangedArray { element, min, max } => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            Ok(types.ranged_array(element, *min, *max))
        }
        InterfaceTypeSnapshot::Queue(element) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            Ok(types.queue(element))
        }
        InterfaceTypeSnapshot::BoundedQueue(element, capacity) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            let capacity = import_interface_index(capacity, types, interner)?;
            Ok(types.bounded_queue(element, capacity))
        }
        InterfaceTypeSnapshot::Stack(element) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            Ok(types.stack(element))
        }
        InterfaceTypeSnapshot::BoundedStack(element, capacity) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            let capacity = import_interface_index(capacity, types, interner)?;
            Ok(types.bounded_stack(element, capacity))
        }
        InterfaceTypeSnapshot::Map(key, value) => {
            let key = import_interface_type_inner(key, types, interner, resolver)?;
            let value = import_interface_type_inner(value, types, interner, resolver)?;
            Ok(types.map(key, value))
        }
        InterfaceTypeSnapshot::Ledger(key, value) => {
            let key = import_interface_type_inner(key, types, interner, resolver)?;
            let value = import_interface_type_inner(value, types, interner, resolver)?;
            Ok(types.ledger(key, value))
        }
        InterfaceTypeSnapshot::Record(fields) => {
            let mut remapped = Vec::with_capacity(fields.len());
            for (name, field_ty) in fields {
                remapped.push((
                    interner.intern(name),
                    import_interface_type_inner(field_ty, types, interner, resolver)?,
                ));
            }
            Ok(types.record(remapped))
        }
        InterfaceTypeSnapshot::Set(element) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            Ok(types.set(element))
        }
        InterfaceTypeSnapshot::Promissum(inner) => {
            let inner = import_interface_type_inner(inner, types, interner, resolver)?;
            Ok(types.promissum(inner))
        }
        InterfaceTypeSnapshot::PromissumFailable(success, alternate) => {
            let success = import_interface_type_inner(success, types, interner, resolver)?;
            let alternate = import_interface_type_inner(alternate, types, interner, resolver)?;
            Ok(types.promissum_failable(success, alternate))
        }
        InterfaceTypeSnapshot::Cursor(inner) => {
            let inner = import_interface_type_inner(inner, types, interner, resolver)?;
            Ok(types.cursor(inner))
        }
        InterfaceTypeSnapshot::AsyncCursor(item, alternate) => {
            let item = import_interface_type_inner(item, types, interner, resolver)?;
            let alternate = import_interface_type_inner(alternate, types, interner, resolver)?;
            Ok(types.async_cursor(item, alternate))
        }
        InterfaceTypeSnapshot::Tensor(element, shape) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            let shape = import_interface_index(shape, types, interner)?;
            Ok(types.tensor_with_shape(element, shape))
        }
        InterfaceTypeSnapshot::Vector(element, width) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            let width = import_interface_index(width, types, interner)?;
            Ok(types.vector_with_width(element, width))
        }
        InterfaceTypeSnapshot::Matrix(element, shape) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            let shape = import_interface_index(shape, types, interner)?;
            Ok(types.matrix_with_shape(element, shape))
        }
        InterfaceTypeSnapshot::Sparsa(element, shape) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            let shape = import_interface_index(shape, types, interner)?;
            Ok(types.sparsa_with_shape(element, shape))
        }
        InterfaceTypeSnapshot::Atomic(element) => {
            let element = import_interface_type_inner(element, types, interner, resolver)?;
            Ok(types.atomic(element))
        }
        InterfaceTypeSnapshot::Intervallum(bound) => {
            let bound = import_interface_type_inner(bound, types, interner, resolver)?;
            Ok(types.intervallum(bound))
        }
        InterfaceTypeSnapshot::SizedNumeric(primitive, width) => {
            Ok(types.sized_numeric(*primitive, *width))
        }
        InterfaceTypeSnapshot::Bits(width) => Ok(types.intern(Type::Bits(*width))),
        InterfaceTypeSnapshot::ModularWord(width) => Ok(types.intern(Type::ModularWord(*width))),
        InterfaceTypeSnapshot::SaturatingWord(width) => {
            Ok(types.intern(Type::SaturatingWord(*width)))
        }
        InterfaceTypeSnapshot::RangedInt {
            policy,
            width,
            lo,
            hi,
        } => Ok(types.ranged_int(*policy, *width, *lo, *hi)),
        InterfaceTypeSnapshot::PolicyFloat(policy, width) => {
            Ok(types.intern(Type::PolicyFloat(*policy, *width)))
        }
        InterfaceTypeSnapshot::SizedInstans(precision) => Ok(types.sized_instans(*precision)),
        InterfaceTypeSnapshot::Option(inner) => {
            let inner = import_interface_type_inner(inner, types, interner, resolver)?;
            Ok(types.option(inner))
        }
        InterfaceTypeSnapshot::Ref(mutability, inner) => {
            let inner = import_interface_type_inner(inner, types, interner, resolver)?;
            Ok(types.reference(*mutability, inner))
        }
        InterfaceTypeSnapshot::Func(callable) => {
            let sig = import_interface_callable_inner(callable, types, interner, resolver)?;
            Ok(types.intern(Type::Func(sig)))
        }
        InterfaceTypeSnapshot::Param(name) => Ok(types.intern(Type::Param(interner.intern(name)))),
        InterfaceTypeSnapshot::Applied(base, args) => {
            let base = import_interface_type_inner(base, types, interner, resolver)?;
            let args = args
                .iter()
                .map(|arg| match arg {
                    InterfaceAppliedArg::Type(ty) => {
                        import_interface_type_inner(ty, types, interner, resolver)
                            .map(radix_types::AppliedArg::Type)
                    }
                    InterfaceAppliedArg::Size(index) => {
                        import_interface_index(index, types, interner)
                            .map(radix_types::AppliedArg::Size)
                    }
                })
                .collect::<FileInterfaceResult<Vec<_>>>()?;
            Ok(types.intern(Type::Applied(base, args)))
        }
        InterfaceTypeSnapshot::Union(members) => {
            let members = members
                .iter()
                .map(|member| import_interface_type_inner(member, types, interner, resolver))
                .collect::<FileInterfaceResult<Vec<_>>>()?;
            Ok(types.intern(Type::Union(members)))
        }
        InterfaceTypeSnapshot::Intersection(members) => {
            let members = members
                .iter()
                .map(|member| import_interface_type_inner(member, types, interner, resolver))
                .collect::<FileInterfaceResult<Vec<_>>>()?;
            Ok(types.intern(Type::Intersection(members)))
        }
        InterfaceTypeSnapshot::Tuple(elements) => {
            let elements = elements
                .iter()
                .map(|element| import_interface_type_inner(element, types, interner, resolver))
                .collect::<FileInterfaceResult<Vec<_>>>()?;
            Ok(types.tuple(elements))
        }
        InterfaceTypeSnapshot::Nominal(nominal) => {
            import_nominal_type(nominal, types, interner, resolver)
        }
    }
}

fn import_nominal_type(
    nominal: &InterfaceNominalType,
    types: &mut TypeTable,
    interner: &mut Interner,
    resolver: Option<&Resolver>,
) -> FileInterfaceResult<TypeId> {
    let Some(resolver) = resolver else {
        return Err(FileInterfaceError::NominalTypeUnsupported { kind: nominal.kind });
    };
    let name = interner.intern(&nominal.name);
    // Canonical nominal-identity lookup first: the nominal snapshot carries
    // its home module's qualified identity, and consumer imports register
    // nominals under that key. Same-name nominals from different module
    // identities therefore resolve to distinct semantic types (codex-gap S1
    // U2 negative proof: coincident raw DefIds must not unify). Identity-less
    // nominals keep the legacy name-keyed path below (ambiguity-guarded).
    //
    // The kind is tried flexibly (exact first, then the sibling kinds) ONLY
    // when the snapshot marks it as guessed — the AST export snapshot
    // defaults qualified/unknown names to Struct, while the consumer
    // registers the nominal under its true declared kind. Within one module
    // identity a name denotes exactly one nominal, so a kind-flexible
    // canonical lookup cannot over-unify (cds-u6 G4: `fn bar(...) →
    // visus.Visus` must not be export-skipped on a kind guess alone).
    // Authoritative semantic snapshots (kind_guessed = false) keep the
    // kind-exact fail-closed contract (council-11).
    if let Some(identity) = nominal.identity.as_ref() {
        if nominal.kind_guessed {
            for candidate in [
                nominal.kind,
                InterfaceNominalKind::Enum,
                InterfaceNominalKind::Struct,
                InterfaceNominalKind::Interface,
            ] {
                if let Some(type_id) =
                    resolver.imported_nominal_type_by_identity(candidate, Some(identity), name)
                {
                    return Ok(type_id);
                }
            }
        } else if let Some(type_id) =
            resolver.imported_nominal_type_by_identity(nominal.kind, Some(identity), name)
        {
            return Ok(type_id);
        }
        // Fail-closed (council-11 correct_before_next_phase): a snapshot that
        // carries a canonical identity must NOT fall through to the legacy
        // short-name / local-symbol resolution below — a canonical-table miss
        // means this consumer does not know the nominal's home module, and
        // name-based fallback could silently bind the unknown nominal to a
        // DIFFERENT consumer nominal. Legacy name-based resolution stays
        // available only for identity-less snapshots.
        return Err(FileInterfaceError::NominalIdentityUnknown {
            kind: nominal.kind,
            name: nominal.name.clone(),
        });
    }
    if resolver.imported_nominal_type_is_ambiguous(nominal.kind, name) {
        return Err(FileInterfaceError::NominalTypeAmbiguous {
            kind: nominal.kind,
            name: nominal.name.clone(),
        });
    }
    if let Some(type_id) = resolver.imported_nominal_type(nominal.kind, name) {
        return Ok(type_id);
    }
    // Kind-flexible legacy fallback: identity-less snapshots from the AST
    // export builder carry a guessed kind for names the exporting module does
    // not declare locally. Never pick an ambiguous name-keyed entry (that
    // would silently bind a colliding short name from another namespace).
    if nominal.kind_guessed {
        for candidate in [
            InterfaceNominalKind::Enum,
            InterfaceNominalKind::Struct,
            InterfaceNominalKind::Interface,
        ] {
            if candidate == nominal.kind
                || resolver.imported_nominal_type_is_ambiguous(candidate, name)
            {
                continue;
            }
            if let Some(type_id) = resolver.imported_nominal_type(candidate, name) {
                return Ok(type_id);
            }
        }
    }
    let Some(def_id) = resolver.lookup_type(name) else {
        return Err(FileInterfaceError::NominalTypeUnsupported { kind: nominal.kind });
    };
    let Some(symbol) = resolver.get_symbol(def_id) else {
        return Err(FileInterfaceError::NominalTypeUnsupported { kind: nominal.kind });
    };
    match (nominal.kind, symbol.kind) {
        (InterfaceNominalKind::Struct, SymbolKind::Struct) => Ok(symbol
            .ty
            .unwrap_or_else(|| types.intern(Type::Struct(def_id)))),
        (InterfaceNominalKind::Enum, SymbolKind::Enum) => Ok(symbol
            .ty
            .unwrap_or_else(|| types.intern(Type::Enum(def_id)))),
        (InterfaceNominalKind::Interface, SymbolKind::Interface) => Ok(symbol
            .ty
            .unwrap_or_else(|| types.intern(Type::Interface(def_id)))),
        _ => Err(FileInterfaceError::NominalTypeUnsupported { kind: nominal.kind }),
    }
}

// CALLABLE CONTRACT

/// Import a portable callable into a target [`FuncSig`].
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when a parameter, return, error, or type-param
/// constraint type cannot be remapped.
pub fn import_interface_callable_with_resolver(
    callable: &InterfaceCallable,
    types: &mut TypeTable,
    interner: &mut Interner,
    resolver: &Resolver,
) -> FileInterfaceResult<FuncSig> {
    import_interface_callable_inner(callable, types, interner, Some(resolver))
}

/// Rebuild the declared return wrapper from the portable async/generator
/// flags.
///
/// The snapshot carries the raw declared return type; importing re-applies
/// the wrapper the exporting `FuncSig` declared (`async_cursor`,
/// `promissum[_failable]`, `array`). A return that already carries its wrapper
/// is left alone, so a round trip does not double-wrap.
fn import_callable_return_shape(
    callable: &InterfaceCallable,
    mut ret: TypeId,
    mut err: Option<TypeId>,
    types: &mut TypeTable,
) -> (TypeId, Option<TypeId>) {
    if callable.is_async
        && callable.is_generator
        && !matches!(types.get(ret), Type::AsyncCursor(_, _))
    {
        let alternate = err
            .take()
            .unwrap_or_else(|| types.primitive(Primitive::Numquam));
        ret = types.async_cursor(ret, alternate);
    } else if callable.is_async
        && !callable.is_generator
        && !matches!(
            types.get(ret),
            Type::Promissum(_) | Type::PromissumFailable(_, _)
        )
    {
        ret = if let Some(err_ty) = err.take() {
            types.promissum_failable(ret, err_ty)
        } else {
            types.promissum(ret)
        };
    }
    if callable.is_generator && !callable.is_async && !matches!(types.get(ret), Type::Array(_)) {
        ret = types.array(ret);
    }
    (ret, err)
}

fn import_interface_callable_inner(
    callable: &InterfaceCallable,
    types: &mut TypeTable,
    interner: &mut Interner,
    resolver: Option<&Resolver>,
) -> FileInterfaceResult<FuncSig> {
    let ret = import_interface_type_inner(&callable.ret, types, interner, resolver)?;
    let err = callable
        .err
        .as_ref()
        .map(|err| import_interface_type_inner(err, types, interner, resolver))
        .transpose()?;
    let (ret, err) = import_callable_return_shape(callable, ret, err, types);
    Ok(FuncSig {
        type_params: callable
            .type_params
            .iter()
            .map(|param| {
                Ok(GenericParamDescriptor {
                    name: interner.intern(&param.name),
                    sort: match param.kind {
                        GenericParamKind::Typus => GenericParamSort::Typus,
                        GenericParamKind::Magnitudo => GenericParamSort::Magnitudo,
                    },
                    constraint: import_type_param_constraint(
                        &param.constraint,
                        types,
                        interner,
                        resolver,
                    )?,
                    default: param
                        .default
                        .as_ref()
                        .map(|default| match default {
                            InterfaceGenericParamDefault::Typus(snapshot) => {
                                import_interface_type_inner(snapshot, types, interner, resolver)
                                    .map(SemanticGenericParamDefault::Typus)
                            }
                            InterfaceGenericParamDefault::Magnitudo(value) => {
                                Ok(SemanticGenericParamDefault::Magnitudo(*value))
                            }
                        })
                        .transpose()?,
                })
            })
            .collect::<FileInterfaceResult<Vec<_>>>()?,
        params: callable
            .params
            .iter()
            .map(|param| {
                Ok(ParamType {
                    ty: import_interface_type_inner(&param.ty, types, interner, resolver)?,
                    mode: param.mode,
                    optional: param.optional,
                    sponte: param.sponte,
                })
            })
            .collect::<FileInterfaceResult<Vec<_>>>()?,
        ret,
        err,
        is_async: callable.is_async,
        is_generator: callable.is_generator,
    })
}

/// Snapshot a callable with nominal names resolved through the resolver.
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when a parameter, return, error, or type-param
/// constraint type cannot be snapshotted.
pub fn snapshot_interface_callable_with_resolver(
    sig: &FuncSig,
    types: &TypeTable,
    interner: &Interner,
    resolver: &Resolver,
) -> FileInterfaceResult<InterfaceCallable> {
    snapshot_interface_callable_inner(sig, types, interner, Some(resolver), None)
}

/// Snapshot a callable with nominal names resolved through the resolver and
/// the canonical module-qualified identity of the snapshotting module
/// (codex-gap S1 U2 nominal identity for nested callable nominals).
///
/// # Errors
///
/// Returns [`FileInterfaceError`] when a parameter, return, error, or type-param
/// constraint type cannot be snapshotted.
pub fn snapshot_interface_callable_with_identity(
    sig: &FuncSig,
    types: &TypeTable,
    interner: &Interner,
    resolver: &Resolver,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> FileInterfaceResult<InterfaceCallable> {
    snapshot_interface_callable_inner(sig, types, interner, Some(resolver), module_identity)
}

fn snapshot_interface_callable_inner(
    sig: &FuncSig,
    types: &TypeTable,
    interner: &Interner,
    resolver: Option<&Resolver>,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> FileInterfaceResult<InterfaceCallable> {
    Ok(InterfaceCallable {
        type_params: sig
            .type_params
            .iter()
            .map(|param| {
                Ok(InterfaceCallableParam {
                    name: interner.resolve(param.name).to_owned(),
                    kind: match param.sort {
                        GenericParamSort::Typus => GenericParamKind::Typus,
                        GenericParamSort::Magnitudo => GenericParamKind::Magnitudo,
                    },
                    constraint: snapshot_type_param_constraint(
                        &param.constraint,
                        types,
                        interner,
                        resolver,
                        module_identity,
                    )?,
                    default: param
                        .default
                        .as_ref()
                        .map(|default| match default {
                            SemanticGenericParamDefault::Typus(ty) => {
                                snapshot_interface_type_inner(
                                    *ty,
                                    types,
                                    interner,
                                    resolver,
                                    module_identity,
                                )
                                .map(|snapshot| {
                                    InterfaceGenericParamDefault::Typus(Box::new(snapshot))
                                })
                            }
                            SemanticGenericParamDefault::Magnitudo(value) => {
                                Ok(InterfaceGenericParamDefault::Magnitudo(*value))
                            }
                        })
                        .transpose()?,
                })
            })
            .collect::<FileInterfaceResult<Vec<_>>>()?,
        tensor_edge_policy_requirements: Vec::new(),
        params: sig
            .params
            .iter()
            .map(|param| {
                Ok(InterfaceParam {
                    ty: snapshot_interface_type_inner(
                        param.ty,
                        types,
                        interner,
                        resolver,
                        module_identity,
                    )?,
                    mode: param.mode,
                    optional: param.optional,
                    sponte: param.sponte,
                    // Producers with source access overwrite this from the
                    // declaration's `axis` marker (`set_axis_params`); a
                    // `FuncSig` alone cannot carry the marker.
                    axis: false,
                })
            })
            .collect::<FileInterfaceResult<Vec<_>>>()?,
        ret: Box::new(snapshot_interface_type_inner(
            sig.ret,
            types,
            interner,
            resolver,
            module_identity,
        )?),
        err: sig
            .err
            .map(|err| {
                snapshot_interface_type_inner(err, types, interner, resolver, module_identity)
                    .map(Box::new)
            })
            .transpose()?,
        is_async: sig.is_async,
        is_generator: sig.is_generator,
        // A semantic `FuncSig` carries no device fact; the v2 contract is
        // attached by the export-surface builders that know the declaration's
        // annotations and body availability (DFV2-3).
        device: None,
    })
}

fn import_type_param_constraint(
    constraint: &InterfaceTypeParamConstraint,
    types: &mut TypeTable,
    interner: &mut Interner,
    resolver: Option<&Resolver>,
) -> FileInterfaceResult<TypeParamConstraint> {
    match constraint {
        InterfaceTypeParamConstraint::Any => Ok(TypeParamConstraint::Any),
        InterfaceTypeParamConstraint::OneOf(options) => options
            .iter()
            .map(|option| import_interface_type_inner(option, types, interner, resolver))
            .collect::<FileInterfaceResult<Vec<_>>>()
            .map(TypeParamConstraint::OneOf),
    }
}

fn snapshot_type_param_constraint(
    constraint: &TypeParamConstraint,
    types: &TypeTable,
    interner: &Interner,
    resolver: Option<&Resolver>,
    module_identity: Option<&InterfaceLibraryIdentity>,
) -> FileInterfaceResult<InterfaceTypeParamConstraint> {
    match constraint {
        TypeParamConstraint::Any => Ok(InterfaceTypeParamConstraint::Any),
        TypeParamConstraint::OneOf(options) => options
            .iter()
            .map(|option| {
                snapshot_interface_type_inner(*option, types, interner, resolver, module_identity)
            })
            .collect::<FileInterfaceResult<Vec<_>>>()
            .map(InterfaceTypeParamConstraint::OneOf),
        // Contract bounds (D1.1) have no portable interface shape yet; the
        // export is skipped (fail closed) rather than exported unbounded.
        TypeParamConstraint::Implet(_) => Err(FileInterfaceError::NominalTypeUnsupported {
            kind: InterfaceNominalKind::Interface,
        }),
    }
}

fn snapshot_interface_index(
    index_id: IndexId,
    types: &TypeTable,
    interner: &Interner,
) -> FileInterfaceResult<InterfaceIndexSnapshot> {
    match types.get_index(index_id) {
        IndexExpr::Literal(value) => Ok(InterfaceIndexSnapshot::Literal(*value)),
        IndexExpr::Param(name) => Ok(InterfaceIndexSnapshot::Param(
            interner.resolve(*name).to_owned(),
        )),
        IndexExpr::Affine { base, offset } => {
            radix_types::evaluate_index_dimension(index_id, types)
                .map_err(FileInterfaceError::InvalidIndexArithmetic)?;
            Ok(InterfaceIndexSnapshot::Affine {
                base: Box::new(snapshot_interface_index(*base, types, interner)?),
                offset: *offset,
            })
        }
        IndexExpr::Divide { numerator, divisor } => {
            radix_types::evaluate_index_dimension(index_id, types)
                .map_err(FileInterfaceError::InvalidIndexArithmetic)?;
            Ok(InterfaceIndexSnapshot::Divide {
                numerator: Box::new(snapshot_interface_index(*numerator, types, interner)?),
                divisor: *divisor,
            })
        }
        IndexExpr::Tuple(dims) => dims
            .iter()
            .map(|dim| snapshot_interface_index(*dim, types, interner))
            .collect::<FileInterfaceResult<Vec<_>>>()
            .map(InterfaceIndexSnapshot::Tuple),
        IndexExpr::Infer(_) => Err(FileInterfaceError::IndexInferenceUnsupported),
        IndexExpr::Runtime(source) => match source {
            radix_types::RuntimeExtentId::ScalarParameter { parameter } => {
                Ok(InterfaceIndexSnapshot::RuntimeScalarParameter {
                    parameter: *parameter,
                })
            }
            radix_types::RuntimeExtentId::FunctionParameterAxis { parameter, axis } => {
                Ok(InterfaceIndexSnapshot::RuntimeTensorParameterAxis {
                    parameter: *parameter,
                    axis: *axis,
                })
            }
            radix_types::RuntimeExtentId::FunctionParameterFieldAxis {
                parameter,
                fields,
                axis,
            } => Ok(InterfaceIndexSnapshot::RuntimeTensorParameterFieldAxis {
                parameter: *parameter,
                fields: fields
                    .iter()
                    .map(|field| interner.resolve(*field).to_owned())
                    .collect(),
                axis: *axis,
            }),
            radix_types::RuntimeExtentId::ScalarBinding(_)
            | radix_types::RuntimeExtentId::TensorBindingAxis { .. }
            | radix_types::RuntimeExtentId::TensorFieldAxis { .. }
            | radix_types::RuntimeExtentId::ScalarExpression { .. }
            | radix_types::RuntimeExtentId::ExpressionAxis { .. }
            | radix_types::RuntimeExtentId::ExpressionFieldAxis { .. }
            | radix_types::RuntimeExtentId::CallArgumentAxis { .. }
            | radix_types::RuntimeExtentId::CallArgumentFieldAxis { .. } => {
                Err(FileInterfaceError::IndexInferenceUnsupported)
            }
        },
        IndexExpr::Unspecified => Ok(InterfaceIndexSnapshot::Unspecified),
    }
}

fn import_interface_index(
    snapshot: &InterfaceIndexSnapshot,
    types: &mut TypeTable,
    interner: &mut Interner,
) -> FileInterfaceResult<IndexId> {
    match snapshot {
        InterfaceIndexSnapshot::Literal(value) => {
            Ok(types.intern_index(IndexExpr::Literal(*value)))
        }
        InterfaceIndexSnapshot::Param(name) => {
            Ok(types.intern_index(IndexExpr::Param(interner.intern(name))))
        }
        InterfaceIndexSnapshot::Affine { base, offset } => {
            let base = import_interface_index(base, types, interner)?;
            radix_types::add_index_constant(types, base, *offset)
                .map_err(FileInterfaceError::InvalidIndexArithmetic)
        }
        InterfaceIndexSnapshot::Divide { numerator, divisor } => {
            let numerator = import_interface_index(numerator, types, interner)?;
            radix_types::divide_index_exact(types, numerator, *divisor)
                .map_err(FileInterfaceError::InvalidIndexArithmetic)
        }
        InterfaceIndexSnapshot::RuntimeTensorParameterAxis { parameter, axis } => Ok(types
            .intern_index(IndexExpr::Runtime(
                radix_types::RuntimeExtentId::FunctionParameterAxis {
                    parameter: *parameter,
                    axis: *axis,
                },
            ))),
        InterfaceIndexSnapshot::RuntimeTensorParameterFieldAxis {
            parameter,
            fields,
            axis,
        } => Ok(types.intern_index(IndexExpr::Runtime(
            radix_types::RuntimeExtentId::FunctionParameterFieldAxis {
                parameter: *parameter,
                fields: fields.iter().map(|field| interner.intern(field)).collect(),
                axis: *axis,
            },
        ))),
        InterfaceIndexSnapshot::RuntimeScalarParameter { parameter } => Ok(types.intern_index(
            IndexExpr::Runtime(radix_types::RuntimeExtentId::ScalarParameter {
                parameter: *parameter,
            }),
        )),
        InterfaceIndexSnapshot::Tuple(dims) => dims
            .iter()
            .map(|dim| import_interface_index(dim, types, interner))
            .collect::<FileInterfaceResult<Vec<_>>>()
            .map(IndexExpr::Tuple)
            .map(|tuple| types.intern_index(tuple)),
        InterfaceIndexSnapshot::Unspecified => Ok(types.unspecified_shape()),
    }
}
