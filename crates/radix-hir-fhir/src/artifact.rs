//! Serializable snapshot types for HIR artifact round-trips.
//!
//! These types are the portable FHIR wire contract (F19) and live in the
//! `radix-hir-fhir` format crate. Package `radix` constructs snapshots at the
//! façade boundary via `to_snapshot()` methods and reconstructs runtime types
//! from snapshots during loading (U2.2).
//!
//! FREEZES CITED
//! =============
//! - **F4**: Snapshot discipline — `ResolverSnapshot` is the wire shape for
//!   resolver data; `Resolver` itself never derives `Serialize`.
//! - **F5**: `ResolverSnapshot` carries namespace exports, file interfaces,
//!   used namespaces, imported nominal types, and coherence-registry
//!   conversion rows.
//! - **CTM E.2**: Caches and memos (`scopes`, `current_scope`, `lookup_shadow`,
//!   `gpu_builtins`, `gpu_builtins_order`) are excluded from the wire shape
//!   and rebuilt on load.
//! - **CXO F4**: Insertion-order mirrors and `RefCell` caches are excluded.
//! - **F19**: `ResolverSnapshot` struct TYPE is foundation-owned in `radix-hir`;
//!   `Resolver::to_snapshot()` lives in `radix`.
//! - **E3**: coherence-registry conversion rows are preserved on the resolver
//!   snapshot so prebuilt artifacts retain typed error-conversion lookup.

use radix_lexer::{Span, Symbol};
use radix_types::{
    BuiltinTensorEdgePolicy, DefId, FuncSig, IndexId, NumericWidth, ResolvedGenericCall,
    SemanticParamMode, TypeId, TypeTableSnapshot,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Nominal kind for serialization (mirrors radix::file_interface::InterfaceNominalKind)
// ---------------------------------------------------------------------------

/// Serializable nominal type category for imported types (F5, F19).
///
/// Mirrors `InterfaceNominalKind` from the `radix` crate so the foundation
/// `radix-hir` can own the snapshot representation without a reverse
/// dependency. `Resolver::to_snapshot()` maps between the two enums.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NominalKind {
    Struct,
    Enum,
    Interface,
}

/// Portable compiler-lane kind. Kept in the FHIR crate so artifact loading
/// does not depend on the semantic implementation crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RadixLaneWire {
    Air,
    Mir,
    HirDirect,
}

/// Portable lane entry, including the optional reverse-AD companion identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RadixLaneEntryWire {
    pub def_id: DefId,
    pub lane: RadixLaneWire,
    pub backward: Option<RadixBackwardWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RadixBackwardWire {
    pub companion_name: Symbol,
    pub companion_def_id: DefId,
    pub selected_params: Option<Vec<Symbol>>,
}

// ---------------------------------------------------------------------------
// File interface snapshot types
// ---------------------------------------------------------------------------

/// Serializable snapshot of one imported file interface (F4, F5, CTM E.2).
///
/// Captures export-level data that codegen reads from the resolver at
/// `build_file_import_namespaces`. Replaces the runtime-only
/// `ImportedFileInterface` (which carries `FuncSig` references tied to a
/// transient `TypeTable`). Reconstruction during loading (U2.2) rebuilds
/// the runtime `ImportedFileInterface` from this snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FileInterfaceSnapshot {
    pub exports: Vec<(Symbol, FileExportSnapshot)>,
    /// `@ nondum` gates of exported free functions, keyed by the
    /// consumer-visible member spelling. Members absent here are available.
    pub unavailable: Vec<(Symbol, Vec<UnavailableWire>)>,
    /// Folded compile-time values of exported constants, keyed by the
    /// consumer-visible member spelling (schema v52). A constant absent here
    /// did not fold and stays non-constant for a consumer.
    pub const_values: Vec<(Symbol, ConstValueWire)>,
}

/// Serializable folded constant value of a file-interface export.
///
/// Mirrors the semantic `InterfaceConstValue`. An integer is two 64-bit
/// halves because the full `i128` fold domain has no `MessagePack` encoding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ConstValueWire {
    /// Two's-complement `i128` as `(high, low)` halves.
    Int {
        high: i64,
        low: u64,
    },
    Float(f64),
    Bool(bool),
    Text(String),
    List(Vec<ConstValueWire>),
}

/// Serializable `@ nondum [target] ["reason"]` gate (schema v40).
///
/// `target: None` means unavailable on every target. Mirrors the semantic
/// `InterfaceUnavailable` record so a bodiless `@ nondum` declaration stays
/// refused (SEM017) after crossing the artifact boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnavailableWire {
    pub target: Option<String>,
    pub reason: Option<String>,
}

/// Serializable export entry within a file interface snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FileExportSnapshot {
    Function(FuncSig),
    Type {
        ty: TypeId,
        def_id: Option<DefId>,
    },
    Struct {
        def_id: DefId,
        ty: TypeId,
        fields: Vec<StructFieldSnapshot>,
        methods: Vec<StructMethodSnapshot>,
        is_json: bool,
        /// Contracts the struct names in `implet`, in the snapshot's own
        /// type table, so a loaded artifact still admits the struct into a
        /// contract-typed slot.
        implements: Vec<radix_types::ContractRef>,
    },
}

/// Serializable struct field within a file interface snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructFieldSnapshot {
    pub name: Symbol,
    pub ty: TypeId,
    pub optional: bool,
    pub required: bool,
    /// Authored genus field modifier (D16.1).
    pub modifier: Option<radix_syntax::FieldModifier>,
    /// Genus member visibility (D5.3).
    pub visibility: radix_syntax::MemberVisibility,
    /// Compile-time initializer of the field (schema v52), already in this
    /// artifact's type and text arenas. A class `const` read and a construction
    /// literal that omits a defaulted field read it. Its `HirId`s are not part
    /// of the unit's HIR and carry no meaning.
    pub init: Option<radix_hir::HirExpression>,
}

/// Serializable imported method identity and callable contract for a struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructMethodSnapshot {
    pub def_id: DefId,
    pub name: Symbol,
    pub surface_name: Symbol,
    pub sig: FuncSig,
    pub visibility: radix_syntax::MemberVisibility,
    /// `@ nondum` gates of the method; every call is refused (SEM017).
    pub unavailable: Vec<UnavailableWire>,
}

// ---------------------------------------------------------------------------
// ResolverSnapshot
// ---------------------------------------------------------------------------

/// One coherence-registry conversion row in a resolver snapshot. Keeping the
/// ordered pair, builtin flag, and the owning ordinary function's `DefId` on
/// the wire prevents a prebuilt artifact from silently losing law-6 lookup
/// facts when loaded.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConversionRowSnapshot {
    pub source: TypeId,
    pub destination: TypeId,
    /// Owning ordinary function; `None` marks a compiler-shipped builtin row.
    pub fn_def_id: Option<DefId>,
    pub span: Span,
    pub builtin: bool,
}

/// Serializable snapshot of resolver import state for HIR artifact wire format (F4, F5, F19).
///
/// Contains the import-facing fields and the conversion coherence registry
/// rows that codegen and later semantic consumers read from the resolver.
/// Runtime-only state — scopes, shadow map, builtins, gpu builtins,
/// insertion-order mirrors — is excluded (CTM E.2, CXO F4) and rebuilt during
/// loading.
///
/// This type is defined in `radix-hir` (foundation crate) per F19. The
/// `Resolver::to_snapshot()` constructor lives in `radix` where it has
/// access to the full `Resolver` struct.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResolverSnapshot {
    /// Package-supplied export tables keyed by namespace binding symbol.
    /// Serialized as `Vec<(Symbol, Vec<Symbol>)>` to avoid `FxHashMap` serde
    /// issues; reconstruction rebuilds the `FxHashMap`.
    pub namespace_exports: Vec<(Symbol, Vec<Symbol>)>,

    /// Package-supplied typed file interfaces keyed by namespace binding symbol.
    pub file_interfaces: Vec<(Symbol, FileInterfaceSnapshot)>,

    /// Namespace bindings referenced from qualified type paths.
    pub used_namespaces: Vec<Symbol>,

    /// Imported nominal export types keyed by (kind, portable name).
    pub imported_nominal_types: Vec<((NominalKind, Symbol), TypeId)>,

    /// Nominal names that occur in more than one imported namespace (F-071).
    ///
    /// The current file-interface snapshot format does not carry namespace
    /// provenance for nested nominal types. This set prevents the last imported
    /// namespace from silently winning during reconstruction.
    pub ambiguous_imported_nominal_types: Vec<(NominalKind, Symbol)>,

    /// Conversion coherence registry rows keyed by the ordered pair
    /// `(source → destination)`. The loader rebuilds the resolver's row table
    /// from these; builtin and user rows share the one shape.
    pub conversion_rows: Vec<ConversionRowSnapshot>,

    /// Namespace bindings the unit's `module.NAME` reads resolve through
    /// (schema v52): the module definition and its local binding name, which
    /// keys the file interface. Sorted by definition id.
    pub module_bindings: Vec<(DefId, Symbol)>,
}

// ---------------------------------------------------------------------------
// CLI wire types (F18)
// ---------------------------------------------------------------------------

/// Wire type for CLI program mode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CliModeWire {
    NotCli,
    SingleCommand,
    Subcommand,
}

/// Wire type for CLI exit expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CliExitWire {
    Fixed(i64),
    Binding(String),
    Field { object: String, field: String },
    Unsupported,
}

/// Wire type for CLI option types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CliTypeWire {
    Textus,
    Numerus,
    Fractus,
    Bivalens,
    Octeti,
    Ignotum,
    ListaTextus,
    ListaNumerus,
}

/// Wire type for CLI default values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CliDefaultWire {
    Text(String),
    Integer(i64),
    Float(f64),
    Bool(bool),
    Nil,
    Expr(String),
}

/// Wire type for CLI option declarations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CliOptionWire {
    pub binding: String,
    /// Interned binding symbol: the record field key CLI argument records use
    /// (schema v54).
    pub binding_symbol: Symbol,
    pub ty: CliTypeWire,
    pub short: Option<String>,
    pub long: Option<String>,
    pub description: Option<String>,
    pub global: bool,
    pub default: Option<CliDefaultWire>,
    #[serde(rename = "flag")]
    pub is_flag: bool,
}

/// Wire type for CLI operand declarations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CliOperandWire {
    pub binding: String,
    /// Interned binding symbol: the record field key CLI argument records use
    /// (schema v54).
    pub binding_symbol: Symbol,
    pub ty: CliTypeWire,
    #[serde(rename = "rest")]
    pub is_rest: bool,
    pub description: Option<String>,
    pub global: bool,
    pub default: Option<CliDefaultWire>,
}

/// Wire type for CLI subcommand declarations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CliCommandWire {
    pub path: Vec<String>,
    pub module_path: Option<Vec<String>>,
    pub function: String,
    pub function_symbol: Symbol,
    pub args_binding: Option<String>,
    pub aliases: Vec<String>,
    pub description: Option<String>,
    pub options: Vec<CliOptionWire>,
    pub operands: Vec<CliOperandWire>,
}

/// Wire type for CLI program contract (F18).
///
/// Mirrors `radix::cli::CliProgram` but lives in the foundation crate so
/// serde derives and wire-format placement are owned by `radix-hir`.
/// Spans are diagnostic-only and excluded from the wire shape; option and
/// operand `binding_symbol`s are not (they key the CLI argument records).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CliProgramWire {
    pub name: String,
    pub entry_args: String,
    pub mode: CliModeWire,
    pub version: Option<String>,
    pub description: Option<String>,
    pub global_options: Vec<CliOptionWire>,
    pub global_operands: Vec<CliOperandWire>,
    pub options: Vec<CliOptionWire>,
    pub operands: Vec<CliOperandWire>,
    pub commands: Vec<CliCommandWire>,
    pub exit: Option<CliExitWire>,
}

// ---------------------------------------------------------------------------
// Source identity
// ---------------------------------------------------------------------------

/// Source identity for HIR artifact provenance (F1, F2).
///
/// Carries a content hash and a build-host-relative path. No absolute
/// filesystem paths are stored — that would leak build-host details into
/// the artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIdentity {
    /// Content hash of the source file (e.g. SHA-256 hex).
    pub content_hash: String,
    /// Path relative to the project root (no absolute paths).
    pub relative_path: String,
}

// ---------------------------------------------------------------------------
// Function fact wire DTOs (F20)
// ---------------------------------------------------------------------------
//
// FunctionFactTable lives in crate `radix` (semantic/facts). radix-hir
// owns the wire DTOs so HirArtifact can carry function-fact data without
// a reverse dependency. Mapping between radix::semantic::FunctionFactTable
// and FunctionFactTableWire is deferred to U3.2 (emit) / U2.2 (load).

/// Wire capture mode (mirrors `radix::semantic::function_facts::CaptureMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CaptureModeWire {
    Read,
    Mutate,
    Consume,
}

/// Wire capture fact (mirrors `radix::semantic::function_facts::CaptureFact`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureFactWire {
    pub def_id: DefId,
    pub mode: CaptureModeWire,
}

/// Wire function fact (mirrors `radix::semantic::function_facts::FunctionFact`).
///
/// Field types use `Vec` rather than `FxHashSet`/`FxHashMap` so the wire
/// shape is fully deterministic and independent of the hasher.
#[expect(
    clippy::struct_excessive_bools,
    reason = "wire DTO mirrors independent semantic fact flags"
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionFactWire {
    #[serde(rename = "direct_failure")]
    pub has_direct_failure: bool,
    pub may_fail: bool,
    pub is_async: bool,
    pub is_generator: bool,
    pub requires_await: bool,
    pub call_edges: Vec<DefId>,
    pub captures: Vec<CaptureFactWire>,
    /// Parameter-mode entries keyed by parameter def id.
    pub param_modes: Vec<(DefId, SemanticParamMode)>,
    pub err_ty: Option<TypeId>,
}

/// Wire function-fact table (mirrors `radix::semantic::function_facts::FunctionFactTable`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionFactTableWire {
    pub entries: Vec<(DefId, FunctionFactWire)>,
    pub entry: Option<FunctionFactWire>,
}

// ---------------------------------------------------------------------------
// Resolved-use wire DTOs (F20)
// ---------------------------------------------------------------------------
//
// ResolvedUse lives in crate `radix` (semantic/resolved_use). radix-hir
// owns the wire DTOs per F20. Mapping between radix types and wire types
// is deferred to U3.2 (emit) / U2.2 (load).

/// Wire resolved-use key (mirrors `radix::semantic::resolved_use::ResolvedUseKey`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResolvedUseKeyWire {
    Local(DefId),
    Portable(String),
}

/// Wire resolved-use kind (mirrors `radix::semantic::resolved_use::ResolvedUseKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ResolvedUseKindWire {
    Path,
    Call,
    MethodCall,
    TypeRef,
    ImportBinding,
}

/// Wire resolved use (mirrors `radix::semantic::resolved_use::ResolvedUse`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedUseWire {
    pub key: ResolvedUseKeyWire,
    pub kind: ResolvedUseKindWire,
    pub enclosing: Option<DefId>,
    /// Unit-local definition the site resolves to (`None` for import
    /// bindings), so a loaded index rebuilds `by_local` exactly like direct
    /// analysis instead of counting every import as used.
    pub local: Option<DefId>,
}

// ---------------------------------------------------------------------------
// GPU builtin wire DTO (F13, F20)
// ---------------------------------------------------------------------------
//
// MirKernelBuiltin lives in crate `radix-mir` (abi.rs). radix-hir
// owns the wire DTO so HirArtifact can carry GPU builtin assignments
// without a reverse dependency. Mapping between radix-mir types and
// GpuBuiltinWire is deferred to U3.2 (emit) / U2.2 (load).

/// Wire GPU kernel builtin (mirrors `radix_mir::MirKernelBuiltin`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GpuBuiltinWire {
    GlobalInvocationX,
    GlobalInvocationY,
    GlobalInvocationZ,
    GlobalInvocationId,
    WorkgroupIdX,
    WorkgroupIdY,
    WorkgroupIdZ,
    WorkgroupId,
    LocalInvocationIdX,
    LocalInvocationIdY,
    LocalInvocationIdZ,
    LocalInvocationId,
    WorkgroupSizeX,
    WorkgroupSizeY,
    WorkgroupSizeZ,
}

// ---------------------------------------------------------------------------
// Capability provenance wire DTOs (SCR-04)
// ---------------------------------------------------------------------------
//
// `FunctionProvenanceTable` lives in crate `radix` (semantic/capability).
// radix-hir owns the wire DTOs so HirArtifact can carry the durable
// borrow-provenance summaries without a reverse dependency. The DTO carries
// only stable signature/provenance facts; the CFG and `RegionFacts` layout
// are deliberately absent (SCR-04 persistence law).

/// Wire returned-borrow provenance (mirrors `radix_types::ReturnedBorrowProvenance`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ReturnedBorrowWire {
    None,
    Parameter(DefId),
    Definition(DefId),
}

/// Wire required region relationship (mirrors `radix_types::RegionRelationship`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RegionRelationshipWire {
    pub from: DefId,
    pub to: DefId,
}

/// Wire last-use fact (mirrors `radix::semantic::capability::LastUseFact`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LastUseWire {
    pub borrower: DefId,
    pub site: radix_hir::HirId,
}

/// Wire provenance summary for one function boundary.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionProvenanceWire {
    pub function: DefId,
    pub returned_borrow: ReturnedBorrowWire,
    pub required_regions: Vec<RegionRelationshipWire>,
    pub last_uses: Vec<LastUseWire>,
}

/// Wire provenance table (mirrors `radix::semantic::capability::FunctionProvenanceTable`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FunctionProvenanceTableWire {
    pub functions: Vec<FunctionProvenanceWire>,
}

// ---------------------------------------------------------------------------
// HirArtifact
// ---------------------------------------------------------------------------

/// Schema version for the HIR artifact format.
///
/// Starts at 1 and ratchets on each breaking HIR change (`F3`). Version 2
/// adds the [`HirPresentation`](radix_hir::HirPresentation) sidecar to
/// [`HirArtifact`]; schema-v1 artifacts are rejected by the loader (no
/// compatibility decoder). Version 3 removes the retired `Requirit` test
/// modifier (serde variant removal changes the wire format). Version 4
/// represents parameter modes as the unified mode unification fact: a
/// parameter's `HirParamMode` (and the semantic `param_modes` entries) now
/// derive from the type-position access marker; the parameter-prefix slot is
/// retired (MU-5b). Type-position
/// markers model the mode on the marked `Type::Ref(...)` and fn-type/closure
/// parameter slots now carry their own modes. Version 6 carried resolver
/// conversion-arm rows so prebuilt artifacts preserve error-conversion
/// lookup.
///
/// Old artifacts fail closed by design: `decode.rs` has no compatibility
/// decoder, so any schema mismatch (v1–v6) is rejected as
/// [`HirArtifactError::UnsupportedSchemaVersion`] before the payload is
/// interpreted — never a confusing field-layout error.
///
/// v5 (EOT-3W): the unreleased `HirExpressionKind::Transfer` (EOT-3M) and
/// `FirstMatch` (AFM-2) variants shifted serde ordinals in the directly-
/// serde'd HIR payload. Version 6 adds resolver conversion-arm rows — both
/// are wire breaks that ratchet the version rather than inventing a
/// compatibility decoder. Version 7 (CTR-05) removes the destination-owned
/// conversion-arm rows and the enum's retired conversion-arm payload from
/// the directly-serde'd HIR payload, replacing them with coherence-registry
/// rows keyed by the ordered pair `(source → destination)`. Version 8
/// (SCR-04) adds the durable capability-provenance summaries
/// (`FunctionProvenanceTableWire`); older artifacts fail closed at the
/// schema gate as usual. Version 9 adds `HirPresentation::authored_types`;
/// this serialized HIR presentation change is a wire break, so there is no
/// compatibility decoder for schema-v8 artifacts. Version 10 carries the
/// resolved generic binding vectors (`TypeTable.resolved_generic_calls`) so
/// decoded modules keep the binder's explicit/inferred/defaulted call facts
/// instead of only the declaration descriptors. Version 11 carries compiler
/// lane and backward-companion metadata so rehydrated AIR templates cannot be
/// mistaken for ordinary HIR-only functions or silently bypass AIR admission.
/// Version 12 records authored `↢` await-directed binding spellings in the
/// presentation sidecar. Version 13 preserves imported structs' JSON conversion
/// opt-in in resolver snapshots. Version 14 carries analyzed provider units
/// (carriage) for a unit's library/file import edges so loaded-surface HIR
/// emission maps over carried units exactly as the direct surface does; a
/// serialized HIR addition is a wire break, so there is no compatibility
/// decoder for schema-v13 artifacts. Version 15 adds the authored `iuncta`
/// element label on `HirTypeForm` and `HirPresentation::tuple_label_accesses`
/// (by-label `p.gw` reads lowered to constant-index nodes): both are
/// serialized HIR presentation additions, so schema-v14 artifacts fail closed
/// at the schema gate as usual. Version 16 drops the dead `HirCasuArm::guard`
/// field (D4.1: `discerne` arms carry no guards), a serialized HIR removal,
/// so schema-v15 artifacts fail closed at the schema gate. Version 17 drops `HirStruct.extends` and `HirStruct.is_abstract` (class
/// inheritance removed, DECISIONS D8.1); the serialized struct shape changed,
/// so schema-v16 artifacts fail closed. Version 18 adds
/// `HirExpressionKind::Format` (the `¶` operator, D2.1), shifting the serde
/// ordinal of `Error`; schema-v17 artifacts fail closed. Version 19 carries
/// contract bounds and generic `implet` (D1.1/D1.2): `HirStruct.implements`
/// holds contract references with type arguments, `HirTypeParamConstraint`
/// gains `Implet`, and `HirInterfaceMethod` gains a `def_id`; schema-v18
/// artifacts fail closed. Version 20 replaces `HirField.is_static` with the
/// authored field modifier (`fixum` / `varia` / `generis`, D16.1) and carries
/// it on `StructFieldSnapshot`; schema-v19
/// artifacts fail closed. Version 21 adds the authored `regio` declaration
/// (`HirPresentation::region`, D7.7) and the `HirTriviaOwnerKind::Region`
/// owner that carries the module doc; schema-v20 artifacts fail closed.
/// Version 22 adds `HirPresentation::insere_exprs` (authored build-time
/// embed paths, D8.10); schema-v21 artifacts fail closed. Version 23 carries
/// genus member visibility on `StructFieldSnapshot` (D5.3); schema-v22
/// artifacts fail closed. Version 24 adds
/// `HirPresentation::function_modifier_bindings` (which `errors` / `options` /
/// `exit` signature modifier introduced each runtime-provided binding local);
/// schema-v23 artifacts fail closed. Version 25 adds
/// `HirExpressionKind::Insere` (a build-time embed in any `=` position, st1
/// U4), shifting the serde ordinal of the variants after `Praefixum`;
/// schema-v24 artifacts fail closed. Version 26 adds
/// `HirExpressionKind::Widen` (the implicit lossless bounded-integer → `inf`
/// widening typecheck inserts, inf-wn), shifting the serde ordinal of the
/// variants after `Conversio`; schema-v25 artifacts fail closed. Version 27 adds
/// `HirPresentation::glyph_ids` (which transpose calls were authored
/// with the `ᵀ` glyph); schema-v26 artifacts fail closed. Version 28 adds
/// `HirPresentation::numeric_twin_ids` (which assignments are desugared
/// in-place numeric twin method calls); schema-v27 artifacts fail closed.
/// Version 29 adds `Type::PolicyFloat` (the `trapping<W>` / `saturating<W>`
/// float cell, N7E-1), shifting the serde ordinal of the `Type` variants after
/// `SaturatingWord`; schema-v28 artifacts fail closed. Version 30 (IB-1)
/// converts `HirExpressionKind::Itera` from a positional tuple to named fields
/// and adds the authored element-binder `mutable` flag to `Itera`,
/// `IteraLockstep`, `IteraRange` and `Summa`; schema-v29 artifacts fail closed.
/// Version 31 adds runtime shape-boundary obligations so artifact loading
/// preserves recoverable tensor extent checks; schema-v30 artifacts fail closed.
/// Version 32 adds per-target kernel requirements so loaded artifacts retain
/// the semantic admission contract; schema-v31 artifacts fail closed.
/// Version 33 adds resolved tensor edge-policy facts and generic provider
/// obligations; schema-v32 artifacts fail closed. Version 34 carries runtime
/// shape-boundary obligations inside provider units; schema-v33 artifacts fail
/// closed because a provider's entry, call, and return guards are required by
/// Rust code generation just like the root unit's guards. Version 35 carries
/// imported struct method identities and signatures; schema-v34 artifacts fail
/// closed because policy callbacks need their imported owner/name route.
/// Version 36 reshapes `HirExpressionKind::Transfer { sink, payloads }` into
/// `CopyInto { target, value }` (the `⇇` copy-into statement); schema-v35
/// artifacts fail closed because the serialized variant payload changed.
/// Version 37 removes the legacy `HirExpressionKind::Summa` and `Extremum`
/// variants in favor of `Reduction`; schema-v36 artifacts fail closed because
/// the serialized enum ordinals changed.
/// Version 38 adds `HirTypeSlot::TypedZero` (the authored type of a typed zero
/// `Type {}`); schema-v37 artifacts fail closed because the sidecar registry
/// gained a slot variant.
/// Version 39 adds `HirPresentation::empty_dense_ids` (which nodes typecheck
/// rewrote from `empty` to the typed zero of a dense tensor, vector or matrix
/// slot, so the Faber re-emitter prints `empty` again); schema-v38 artifacts
/// fail closed. Version 40 carries `@ nondum` availability gates on imported
/// free functions and genus methods in the resolver file-interface snapshot
/// (`FileInterfaceSnapshot::unavailable`, `StructMethodSnapshot::unavailable`)
/// so a loaded artifact still refuses calls with SEM017; schema-v39 artifacts
/// fail closed. Version 41 adds `HirPresentation::inferred_constructor_ids`
/// (which nodes typecheck rewrote from an inferred-constructor hole `_ { … }`,
/// so the Faber re-emitter prints the hole again); schema-v40 artifacts fail
/// closed. Version 42 carries a struct's declared `implet` contracts in the
/// resolver file-interface snapshot (`FileExportSnapshot::Struct::implements`)
/// so a loaded artifact still admits an imported struct into a contract-typed
/// slot; schema-v41 artifacts fail closed. Version 43 carries each resolved
/// use's unit-local `DefId` (`ResolvedUseWire::local`) so a loaded artifact
/// rebuilds the resolved-use local index exactly like direct analysis instead
/// of counting every import binding as used; schema-v42 artifacts fail closed.
/// Version 44 carries the four program display tokens (`true`, `false`,
/// `none`, `tuple`) of the unit's code locale so a loaded unit prints the
/// words the author's locale prints; schema-v43 artifacts fail closed.
/// Version 45 adds `HirPresentation::axis_sugar` (which nodes typecheck
/// expanded from an authored `t.sum(axis n)` / `t.mean(axis n)`, so the Faber
/// re-emitter prints the call again); schema-v44 artifacts fail closed.
/// Version 46 carries `HirArtifact::conversation_constructor`, the
/// compiler-owned `sermo` definition the authored-form validator needs to pair
/// a bare carrier witness with the authored `sermo<O, R>` type children;
/// schema-v45 artifacts fail closed. Version 47 adds the source-shaped
/// `HirExpressionKind::Scan` node and its proven grouped-key domain; schema-v46
/// artifacts fail closed because the serialized HIR enum changed.
/// Version 48 adds `HirPresentation::template_holes` (the authored template
/// hole spans of interpolated strings) and the derived wire-shape hash
/// (`HirArtifact::shape_hash`, see [`crate::wire_shape_hash`]) beside the
/// version. `HirPresentation::radical_quarta_ids` (2026-10-19) was added
/// without its own bump, so earlier v47 artifacts carry a different
/// positional layout than this reader; schema-v47 artifacts fail closed. From
/// version 48 on, a serialized-shape change is also caught mechanically: the
/// header hash is derived from the live type definitions, so a layout change
/// that forgot this bump still fails closed on decode.
/// Version 49 adds `TensorEdgePolicyTableWire::ordo_order`, the resolved
/// `ordo` order rows (declared discriminants per `≺ ≻ ≤ ≥` comparison site)
/// that backends read to lower an enum comparison as a rank lookup; schema-v48
/// artifacts fail closed. Before it the wire carried no order rows, so a loaded
/// unit lowered an `ordo` comparison as a plain operator and gave wrong results.
/// Version 50 adds `Type::RangedInt` (a ranged store policy,
/// `saturating|trapping|wrapping<W, lo‥hi>`) to the type table; schema-v49
/// artifacts fail closed because the serialized `Type` enum gained a variant.
/// Version 51 adds the authored raw-bits widths to the serialized type table
/// (`TypeTableSnapshot::authored_bits`): the arena cannot tell an authored `b8`
/// from the seeded `octeti` byte list, so a loaded unit reported no authored
/// `b8` and backend bits gates saw a different type than the direct unit;
/// schema-v50 artifacts fail closed.
/// Version 52 adds `FileInterfaceSnapshot::const_values`, the folded values of
/// imported constants (`ConstValueWire`): a loaded unit re-spelled an imported
/// constant where the direct unit folded its value, so MIR could not resolve
/// the projection base. The value is reached through the module binding, so
/// `ResolverSnapshot::module_bindings` carries each namespace binding's
/// definition and name too, and `StructFieldSnapshot::init` the initializers
/// of imported genus fields (a class `const` read folds through one);
/// schema-v51 artifacts fail closed.
/// Version 53 adds `ShapeObligationTableWire::stores` and
/// `ShapeRelationBoundaryWire::Store`: the declaration-store shape pins the
/// semantic table keeps. The wire dropped them, so code generation from a
/// loaded unit lowered no runtime shape check where the direct unit lowered
/// one; schema-v52 artifacts fail closed.
/// Version 54 adds `CliOptionWire::binding_symbol` and
/// `CliOperandWire::binding_symbol`. Lowering keys the CLI argument record by
/// them, and the loader substituted `Symbol(0)`, so a loaded CLI program built
/// records whose fields did not match the record type in the type table;
/// schema-v53 artifacts fail closed.
/// Version 55 adds `HirArtifact::contract_roles`: the ordering, closing and
/// sinking contract identities (and their import bindings) the type table
/// records. A loaded unit carried none, so a backend treated an imported
/// `Orderable` as an ordinary trait and resolved it to
/// `__faber_unresolved_def`; schema-v54 artifacts fail closed.
/// Version 56 retires the dedicated first-match expression and adds
/// `HirReductionOp::First` (`reduce via first`, la `reducta via prima`): the
/// `HirExpressionKind::FirstMatch` variant is gone, shifting the serde ordinals
/// of every later expression variant, and the reduction operator enum gained a
/// variant; schema-v55 artifacts fail closed.
/// Version 57 adds the `MessagePack` signed-128-bit integer extension
/// (`fixext 16`, extension type 1, big-endian two's complement): the unit
/// carries `i128` facts (affine size offsets, ranged-integer bounds) and the
/// named-field codec had no wire form for them, so a unit holding one could
/// not be written. Every `i128` field now rides the extension and reads back
/// only from it; schema-v56 artifacts fail closed.
/// Version 58 adds authored ranged-policy arguments to `HirTypeFormArg`, so
/// formatters and loaded HIR preserve the range glyph, infinity endpoints, and
/// inclusive/exclusive spelling; schema-v57 artifacts fail closed.
/// Version 59 adds `Type::RangedArray` (a list length range,
/// `list<T, size lo‥hi>`, need) to the type table; schema-v58
/// artifacts fail closed because the serialized `Type` enum gained a variant.
/// Version 60 adds `ShapeObligationTableWire::proven_indexes`: the index
/// accesses the checker proved in bounds. The wire dropped
/// them, so MIR lowering from a loaded unit kept the bounds check (a trap on
/// every backend that emits one) where the direct unit lowered a proven
/// projection; schema-v59 artifacts fail closed.
/// Version 61 adds `HirTypeFormHead::ImportedAlias`: an imported alias written
/// by its authored path (`decl.Quad`). A file-interface import carries the alias
/// as its resolved body with no `DefId`, so no `Named` head could name it and
/// the degenerate semantic form over the body failed the validator for every
/// collection-shaped alias (a sized list among them); schema-v60 artifacts fail
/// closed because the serialized head enum gained a variant.
/// Version 62 adds the `UnionSharedField` trivia-owner kind, which records the
/// shared-field ordinal against its union definition so FHIR can retain those
/// member comments; schema-v61 artifacts fail closed because the serialized
/// owner-kind enum gained a variant.
pub const SCHEMA_VERSION: u32 = 62;

/// Portable boundary where a retained runtime shape equality is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShapeRelationBoundaryWire {
    Call,
    FunctionEntry,
    FunctionReturn,
    /// A declaration store pinning opaque initializer axes to the
    /// annotation's size terms (schema v53).
    Store,
}

/// One runtime equality in the semantic type-index arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShapeRelationWire {
    pub left: IndexId,
    pub right: IndexId,
    pub span: Span,
    pub boundary: ShapeRelationBoundaryWire,
}

/// Call-site obligations retained for portable shape checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShapeCallObligationsWire {
    pub call: radix_hir::HirId,
    pub relations: Vec<ShapeRelationWire>,
}

/// The `@ radix contract` roles the type table records for a unit (schema v55).
///
/// Backends recognize norma's `Orderable`, `Closable` and `Sink` by these
/// identities and emit no trait, import or impl for them. Imported contracts
/// are not declared in the unit's HIR, so the identities cannot be re-derived
/// from a loaded artifact.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContractRolesWire {
    pub ordering: Option<DefId>,
    /// Selective import bindings that name the ordering contract, sorted.
    pub ordering_bindings: Vec<DefId>,
    pub closing: Option<DefId>,
    pub sinking: Option<DefId>,
    /// Selective import bindings that name the sinking contract, sorted.
    pub sinking_bindings: Vec<DefId>,
}

/// Declaration-store obligations retained for portable shape checks, keyed by
/// the initializer expression.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShapeStoreObligationsWire {
    pub initializer: radix_hir::HirId,
    pub relations: Vec<ShapeRelationWire>,
}

/// Function-boundary obligations retained for portable shape checks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShapeFunctionObligationsWire {
    pub function: DefId,
    pub relations: Vec<ShapeRelationWire>,
}

/// One generic descriptor's runtime dimension witness at a call boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeSizeBindingWire {
    pub parameter: u32,
    pub witness: IndexId,
    pub span: Span,
}

/// Call-site runtime size bindings, kept outside the generic specialization key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallSizeBindingsWire {
    pub call: radix_hir::HirId,
    pub bindings: Vec<RuntimeSizeBindingWire>,
}

/// Function formal size bindings, witnessed by runtime formal carriers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionSizeBindingsWire {
    pub function: DefId,
    pub bindings: Vec<RuntimeSizeBindingWire>,
}

/// Runtime shape equalities retained by semantic analysis.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShapeObligationTableWire {
    pub calls: Vec<ShapeCallObligationsWire>,
    pub call_size_bindings: Vec<CallSizeBindingsWire>,
    pub functions: Vec<ShapeFunctionObligationsWire>,
    /// Declaration-store pins (schema v53); before it the wire dropped them
    /// and a loaded unit lowered no runtime shape check at the store.
    pub stores: Vec<ShapeStoreObligationsWire>,
    pub function_size_bindings: Vec<FunctionSizeBindingsWire>,
    /// Index accesses the checker proved in bounds (schema v60), sorted by id.
    pub proven_indexes: Vec<radix_hir::HirId>,
}

// ---------------------------------------------------------------------------
// Per-target kernel requirements (K15)
// ---------------------------------------------------------------------------

/// Device target recorded by the semantic kernel-requirements analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum KernelTargetWire {
    MirAmd,
    MirMetal,
    MirWgsl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KernelPrimitiveWire {
    Numerus,
    Fractus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum KernelNumericPolicyWire {
    Exact,
    Wrapping,
    Saturating,
    PolicyFloat,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelWidthRequirementWire {
    pub span: Span,
    pub primitive: KernelPrimitiveWire,
    pub width: NumericWidth,
    pub policy: KernelNumericPolicyWire,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KernelRangeExprWire {
    Constant(i128),
    Shape(IndexId),
    Add(Box<Self>, Box<Self>),
    Subtract(Box<Self>, Box<Self>),
    Multiply(Box<Self>, Box<Self>),
    FloorDivide(Box<Self>, Box<Self>),
    Minimum(Box<Self>, Box<Self>),
    Maximum(Box<Self>, Box<Self>),
    Parameter(DefId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelValueRangeWire {
    pub minimum: KernelRangeExprWire,
    pub maximum: KernelRangeExprWire,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelExtentConditionWire {
    pub span: Span,
    pub range: KernelValueRangeWire,
    pub minimum: i128,
    pub maximum: i128,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelTrappingStoreWire {
    pub span: Span,
    pub width: NumericWidth,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum KernelRequirementFailureWire {
    RecursiveCall,
    CalleeNotAdmitted {
        callee: DefId,
    },
    UnsupportedIntegerWidth {
        span: Span,
        width: NumericWidth,
    },
    UnsupportedFloatWidth {
        span: Span,
        width: NumericWidth,
    },
    IntermediateOutOfRange {
        span: Span,
        range: KernelValueRangeWire,
    },
    StoreOutOfRange {
        span: Span,
        range: KernelValueRangeWire,
    },
    UnsupportedIntegerRange {
        span: Span,
    },
    DivideByZeroPossible {
        span: Span,
    },
    NegativeShiftCountPossible {
        span: Span,
    },
    TensorEdgePolicyDeferred {
        span: Span,
    },
    SaturatingLoweringDeferred {
        span: Span,
        width: NumericWidth,
    },
    OptionFRequired {
        span: Span,
        width: NumericWidth,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelTargetRequirementWire {
    pub target: KernelTargetWire,
    pub admitted: bool,
    pub recursive: bool,
    pub callees: Vec<DefId>,
    pub widths: Vec<KernelWidthRequirementWire>,
    pub extent_conditions: Vec<KernelExtentConditionWire>,
    pub trapping_stores: Vec<KernelTrappingStoreWire>,
    pub failures: Vec<KernelRequirementFailureWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelRequirementWire {
    pub def_id: DefId,
    pub name: String,
    pub target_reports: Vec<KernelTargetRequirementWire>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct KernelRequirementsWire {
    pub kernels: Vec<KernelRequirementWire>,
}

// ---------------------------------------------------------------------------
// Tensor edge-policy facts (K12)
// ---------------------------------------------------------------------------

/// Resolved policy identity retained by semantic analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolvedTensorEdgePolicyWire {
    Builtin(BuiltinTensorEdgePolicy),
    Custom {
        policy_def: DefId,
        remap: DefId,
        transform: Option<DefId>,
    },
    Deferred {
        policy_type: TypeId,
    },
}

/// Shift shape and exact offset expression identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TensorShiftContextWire {
    pub source_shape: IndexId,
    pub shift_expression: radix_hir::HirId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TensorEdgePolicyCallWire {
    pub call: radix_hir::HirId,
    pub policy: ResolvedTensorEdgePolicyWire,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TensorEdgePolicyRequirementWire {
    pub policy_type: TypeId,
    pub element_type: TypeId,
    pub rank: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TensorEdgePolicyFunctionRequirementsWire {
    pub function: DefId,
    pub requirements: Vec<TensorEdgePolicyRequirementWire>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TensorEdgePolicyProviderImplWire {
    /// Concrete policy identity, including applied generic class arguments.
    pub policy_type: TypeId,
    pub element_type: TypeId,
    pub rank: usize,
    pub policy: ResolvedTensorEdgePolicyWire,
}

/// Resolved `ordo` order rows of one `≺ ≻ ≤ ≥` comparison site (schema v49).
///
/// Typecheck records them keyed by the comparison's left-operand HIR id; every
/// backend lowers the comparison as an integer comparison over the declared
/// discriminants. `rows` keeps the live order: one `(variant, discriminant)`
/// pair per member, in declaration order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrdoOrderComparisonWire {
    /// Left-operand HIR id of the comparison.
    pub lhs: radix_hir::HirId,
    /// `(variant, declared discriminant)` per member of the compared `ordo`.
    pub rows: Vec<(DefId, i64)>,
}

/// Deterministic portable representation of the neutral HIR sidecar.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TensorEdgePolicyTableWire {
    pub calls: Vec<TensorEdgePolicyCallWire>,
    pub shifted_expressions: Vec<radix_hir::HirId>,
    pub shifted_view_expressions: Vec<(radix_hir::HirId, TensorShiftContextWire)>,
    pub shifted_bindings: Vec<DefId>,
    pub shifted_view_bindings: Vec<(DefId, TensorShiftContextWire)>,
    pub limes_contexts: Vec<(radix_hir::HirId, TensorShiftContextWire)>,
    pub function_requirements: Vec<TensorEdgePolicyFunctionRequirementsWire>,
    pub provider_impls: Vec<TensorEdgePolicyProviderImplWire>,
    /// Resolved `ordo` order rows per comparison site, sorted by `lhs`
    /// (schema v49).
    pub ordo_order: Vec<OrdoOrderComparisonWire>,
}

/// Serializable snapshot of one analyzed provider unit carried for HIR
/// emission (ARCH-U4 carriage).
///
/// Carries exactly the arenas the Rust provider-module emitter maps over —
/// HIR, type table, interner, resolver (namespace bridge + failable-export
/// facts), and the provider's own nested carriage. Provider diagnostics and
/// the presentation sidecar are not wire facts (matching the unit artifact's
/// frozen field set): a carried unit's diagnostics are warnings-only by
/// construction (an analysis error travels as a `Failed` carriage marker
/// instead), and provider emission is trivia-blind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderUnitWire {
    /// Lowered program of the provider module.
    pub hir: radix_hir::HirModule,

    /// Type information produced by the provider's semantic analysis.
    pub types: TypeTableSnapshot,

    /// Complete resolved generic vectors for the provider's checked generic
    /// calls, restored onto the reconstructed type table (schema v10
    /// treatment, per fragment).
    pub resolved_generic_calls: Vec<ResolvedGenericCall>,

    /// Ordered string interner table owning the provider's names.
    pub interner: Vec<String>,

    /// Resolver snapshot carrying the provider's own file interfaces.
    pub resolver: ResolverSnapshot,

    /// Provider edge-policy facts from the same typed-HIR snapshot (schema v33).
    pub tensor_edge_policies: TensorEdgePolicyTableWire,

    /// Provider runtime shape guards from the same typed-HIR snapshot (schema v34).
    pub shape_obligations: ShapeObligationTableWire,

    /// Carriage for the provider's own nested namespace imports
    /// (`norma:valor` → `norma:chorda` emit as sibling modules).
    pub nested: BTreeMap<String, ProviderCarriageEntryWire>,
}

/// One carried provider unit on the wire, keyed by import-path spelling.
///
/// `Failed` preserves the failures-as-markers semantics across the artifact:
/// a provider whose carriage analysis failed at build time keeps failing
/// emission with its named message after load instead of silently degrading
/// to a missing-carriage error.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ProviderCarriageEntryWire {
    /// Analyzed provider unit (boxed: far larger than the `Failed` marker;
    /// serde encodes `Box<T>` exactly as `T`, so the wire shape is unchanged).
    Unit(Box<ProviderUnitWire>),
    /// Contract discovery or analysis failed for this target; `message` is
    /// the fail-closed emission error's tail clause.
    Failed {
        /// What failed and the leading diagnostic message.
        message: String,
    },
}

/// Portable program display tokens of the unit's code locale (schema v44).
///
/// The words a program prints for a `bivalens`, the null value, and a tuple
/// head follow the code locale, so an English unit loaded from an artifact
/// must keep printing `true`/`false`/`none`/`tuple` instead of falling back
/// to the Latin defaults. Kept in the format crate (not
/// `radix-runtime-contract`) so artifact loading stays free of runtime
/// dependencies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisplayTokensWire {
    /// Printed for a true `bivalens`.
    pub true_: String,
    /// Printed for a false `bivalens`.
    pub false_: String,
    /// Printed for the null value.
    pub none: String,
    /// Printed as the head of a tuple.
    pub tuple: String,
}

/// Serializable snapshot of one analyzed compilation unit (F1, F2, F4).
///
/// Owns every rehydratable fact family produced by analysis so a single
/// `postcard` round-trip can persist or transmit the unit. Fields excluded
/// from the wire shape per the frozen field set (`F9`, `F12`, `F14`) are rebuilt
/// by the loader (`U2.1`–`U2.4`).
///
/// FREEZES CITED
/// =============
/// - **F1**: postcard binary serialization only.
/// - **F2**: `HirArtifact` struct lives in `radix-hir` foundation crate.
/// - **F3**: schema version starts at 1.
/// - **F13**: `gpu_builtins` included via wire DTO.
/// - **F14**: `graphics_source` excluded; compiler lanes are preserved because
///   AIR specialization depends on them.
/// - **F17**: `cli_program` via `CliProgramWire` (`F18`).
/// - **F18**: `CliProgramWire` DTO owned by `radix-hir`.
/// - **F19**: `ResolverSnapshot` foundation-owned in `radix-hir`.
/// - **F20**: `FunctionFactTableWire`, `ResolvedUseWire`, `GpuBuiltinWire` are
///   wire DTOs owned by `radix-hir`; mapping to runtime types deferred to
///   `U3.2`/`U2.x`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirArtifact {
    /// Schema version — starts at 1, ratchets on breaking changes (F3).
    pub schema_version: u32,

    /// Hash of the serialized wire shape (types, field order, enum variants)
    /// derived from the live type definitions, not hand-maintained: always
    /// [`crate::wire_shape_hash`] when written. Second wire field, read right
    /// after the version; decode rejects any artifact whose hash differs from
    /// the reader's, so a layout change that forgot the `SCHEMA_VERSION` bump
    /// still fails closed instead of decoding misaligned.
    pub shape_hash: u64,

    /// Content-hash + relative-path identity (no absolute build-host paths).
    pub source_identity: SourceIdentity,

    /// Full lowered HIR program (direct serde).
    pub hir: radix_hir::HirModule,

    /// Presentation sidecar: trivia attachments, structural ends, and the
    /// entry-start anchor (schema v2). Inseparable from the HIR value it
    /// annotates; never on [`HirModule`](radix_hir::HirModule).
    pub presentation: radix_hir::HirPresentation,

    /// CLI program contract when the unit declares `incipit argumenta` (`F17`, `F18`).
    pub cli_program: Option<CliProgramWire>,

    /// Type table snapshot (direct serde).
    pub types: TypeTableSnapshot,

    /// Complete resolved generic vectors for the unit's checked generic
    /// calls, in call checking order (schema v10). Restored onto the
    /// reconstructed [`radix_types::TypeTable`] so specialization reads the
    /// binder's explicit/inferred/defaulted facts without re-running
    /// inference.
    pub resolved_generic_calls: Vec<ResolvedGenericCall>,

    /// Compiler lane metadata required to rehydrate and specialize AIR
    /// templates (schema v11).
    pub radix_lanes: Vec<RadixLaneEntryWire>,

    /// Ordered string interner table (not the Interner struct directly).
    pub interner: Vec<String>,

    /// Resolver import state snapshot (F4, F5, F19).
    pub resolver: ResolverSnapshot,

    /// Library import registry (`F15`; serde-derivable per `U0.3`).
    pub libraries: radix_hir::LibraryRegistry,

    /// Function effect and capture facts (`F20`: wire DTO).
    pub function_facts: FunctionFactTableWire,

    /// Resolved definition uses (`F20`: wire DTO; serializes as `Vec` per `U0.4` snapshot).
    pub resolved_uses: Vec<ResolvedUseWire>,

    /// Monotonic analysis stamp for coherence (direct serde: u64).
    pub analysis_stamp: u64,

    /// GPU kernel builtin assignments keyed by declaring `DefId` (`F13`, `F20`: wire DTO).
    pub gpu_builtins: Vec<(DefId, GpuBuiltinWire)>,

    /// Durable borrow-provenance and last-use summaries for every function
    /// boundary (SCR-04: wire DTO; F-CAP-04/05 authority on the wire).
    pub capability_provenance: FunctionProvenanceTableWire,

    /// Analyzed provider units for this unit's import edges, keyed by
    /// import-path spelling (schema v14; ARCH-U4 carriage). Empty for units
    /// whose import edges carry no provider imports.
    pub provider_units: BTreeMap<String, ProviderCarriageEntryWire>,

    /// Runtime shape boundary equalities (schema v31).
    pub shape_obligations: ShapeObligationTableWire,

    /// Compiler-owned contract identities (schema v55).
    pub contract_roles: ContractRolesWire,

    /// Per-target inferred device requirements (schema v32).
    pub kernel_requirements: KernelRequirementsWire,

    /// Resolved edge-policy dispatch and generic provider facts (schema v33).
    pub tensor_edge_policies: TensorEdgePolicyTableWire,

    /// Program display tokens of the unit's code locale (schema v44).
    pub display_tokens: DisplayTokensWire,

    /// Compiler-owned conversation constructor of this unit (schema v46).
    ///
    /// `sermo<O, R>` is finalized to its bare `Type::Struct` carrier, so the
    /// authored `sermo<O, R>` children have no `Applied` witness argument list
    /// to pair with. The compiler owns the constructor identity
    /// (`Resolver::builtin_sermo_def_id`), and recording it explicitly lets the
    /// artifact validator admit the bare carrier for exactly that definition.
    /// `None` means the unit registered no conversation builtin, and then no
    /// bare carrier may pair with authored arguments.
    pub conversation_constructor: Option<DefId>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "artifact_test.rs"]
mod tests;
