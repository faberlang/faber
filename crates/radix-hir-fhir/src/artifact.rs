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
//! - **F5**: `ResolverSnapshot` carries only namespace exports, file
//!   interfaces, used namespaces, and imported nominal types.
//! - **CTM E.2**: Caches and memos (`scopes`, `current_scope`, `lookup_shadow`,
//!   `gpu_builtins`, `gpu_builtins_order`) are excluded from the wire shape
//!   and rebuilt on load.
//! - **CXO F4**: Insertion-order mirrors and `RefCell` caches are excluded.
//! - **F19**: `ResolverSnapshot` struct TYPE is foundation-owned in `radix-hir`;
//!   `Resolver::to_snapshot()` lives in `radix`.

use radix_lexer::Symbol;
use radix_types::{DefId, FuncSig, SemanticParamMode, TypeId, TypeTableSnapshot};
use serde::{Deserialize, Serialize};

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
    },
}

/// Serializable struct field within a file interface snapshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructFieldSnapshot {
    pub name: Symbol,
    pub ty: TypeId,
    pub optional: bool,
    pub required: bool,
}

// ---------------------------------------------------------------------------
// ResolverSnapshot
// ---------------------------------------------------------------------------

/// Serializable snapshot of resolver import state for HIR artifact wire format (F4, F5, F19).
///
/// Contains exactly the five fields that codegen reads from the resolver
/// (F5, F-071). Runtime-only state — scopes, shadow map, builtins, gpu builtins,
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
    pub ty: CliTypeWire,
    pub short: Option<String>,
    pub long: Option<String>,
    pub description: Option<String>,
    pub global: bool,
    pub default: Option<CliDefaultWire>,
    pub flag: bool,
}

/// Wire type for CLI operand declarations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CliOperandWire {
    pub binding: String,
    pub ty: CliTypeWire,
    pub rest: bool,
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
/// Fields that are diagnostic-only (spans, `binding_symbol` references) are
/// excluded from the wire shape.
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
    pub direct_failure: bool,
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
// HirArtifact
// ---------------------------------------------------------------------------

/// Schema version for the HIR artifact format.
///
/// Starts at 1 and ratchets on each breaking HIR change (`F3`). Version 2
/// adds the [`HirPresentation`](radix_hir::HirPresentation) sidecar to
/// [`HirArtifact`]; schema-v1 artifacts are rejected by the loader (no
/// compatibility decoder). Version 3 removes the retired `Requirit` test
/// modifier (serde variant removal changes the wire format).
pub const SCHEMA_VERSION: u32 = 3;

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
/// - **F14**: `radix_lanes`, `graphics_source` excluded.
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
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::stub_artifact;

    #[test]
    fn hir_artifact_round_trip() {
        let original = stub_artifact();
        let bytes = postcard::to_allocvec(&original).expect("serialize HirArtifact");
        let deserialized: HirArtifact =
            postcard::from_bytes(&bytes).expect("deserialize HirArtifact");
        assert_eq!(original.schema_version, 3);
        assert_eq!(original, deserialized);
    }
}
