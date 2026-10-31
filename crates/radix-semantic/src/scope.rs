//! Lexical scope tree and definition identity registry.
//!
//! Name binding in Radix has two related responsibilities: find the closest
//! visible declaration for a source name, and preserve stable [`DefId`] metadata
//! once that declaration has been found. This module keeps those concerns
//! separate. Scopes store name-to-definition bindings for lexical lookup, while
//! the resolver's symbol table stores the metadata later passes need after the
//! original scope may no longer be current.
//!
//! PASS ROLE
//! =========
//! Collection defines top-level symbols into the global scope. Resolution then
//! enters and exits lexical scopes while binding local names, parameters, and
//! references. HIR lowering and later HIR analyses consume the resulting
//! definition identities instead of re-resolving source names.
//!
//! CONTROL-FLOW CONTEXT
//! ====================
//! Scope kinds are semantic context markers as well as lookup containers.
//! `in_loop` deliberately stops at a function boundary so `break` and
//! `continue` inside nested functions do not inherit an enclosing loop.
//!
//! INVARIANTS
//! ==========
//! - `ScopeId` indexes `Resolver::scopes`; the global scope is always `0`.
//! - Each scope parent points to an earlier scope, forming a tree rooted at
//!   global.
//! - Every `DefId` stored in a scope's symbol map has corresponding metadata in
//!   `Resolver::symbols`.
//! - `current_scope` always points at a valid scope and never exits past global.

use super::types::{FuncSig, Type, TypeId, TypeTable};
use crate::file_interface::{
    FileExportKind, FileInterface, FileInterfaceError, InterfaceConstValue,
    InterfaceDeviceContract, InterfaceGenericParam, InterfaceLibraryIdentity, InterfaceMemberKind,
    InterfaceMethodExport, InterfaceNominalExport, InterfaceNominalKind, InterfaceStructExport,
    InterfaceUnavailable, import_interface_callable_with_resolver,
    import_interface_type_with_resolver,
};
use crate::locale::LocalePack;
use crate::passes::visibility_annotation::VisibilityTier;
use handroll::{FxHashMap, FxHashSet};
use radix_hir::{DefId, HirExpression, TensorEdgePolicyRequirement};
use radix_lexer::{Span, Symbol as LexSymbol};
use std::cell::RefCell;
use std::collections::BTreeMap;

/// One provider/file-interface member that could not be installed at the
/// consumer locale boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInterfaceSkippedExport {
    /// Import binding name retained for the existing WARN014 surface.
    pub namespace: String,
    /// Canonical member name retained for the existing WARN014 surface.
    pub export: String,
    /// Stable debug spelling retained for the existing WARN014 surface.
    pub reason: String,
    pub provider: String,
    pub package: String,
    pub module: String,
    pub owner: String,
    pub kind: String,
    pub canonical_name: String,
    pub projected_surfaces: Vec<String>,
    pub remap_reason: String,
}

impl FileInterfaceSkippedExport {
    fn new(
        namespace: &str,
        identity: Option<&InterfaceLibraryIdentity>,
        owner: Option<&str>,
        kind: InterfaceMemberKind,
        canonical_name: &str,
        projected_surfaces: Vec<String>,
        reason: impl Into<String>,
    ) -> Self {
        let remap_reason = reason.into();
        let (provider, package, module) = identity.map_or_else(
            || (String::new(), String::new(), String::new()),
            |identity| {
                (
                    identity.provider.clone(),
                    identity.package.clone().unwrap_or_default(),
                    identity.module_path.join("::"),
                )
            },
        );
        Self {
            namespace: namespace.to_owned(),
            export: canonical_name.to_owned(),
            reason: remap_reason.clone(),
            provider,
            package,
            module,
            owner: owner.unwrap_or_default().to_owned(),
            kind: interface_member_kind_name(kind).to_owned(),
            canonical_name: canonical_name.to_owned(),
            projected_surfaces,
            remap_reason,
        }
    }

    fn from_error(
        namespace: &str,
        identity: Option<&InterfaceLibraryIdentity>,
        owner: Option<&str>,
        kind: InterfaceMemberKind,
        canonical_name: &str,
        projected_surfaces: Vec<String>,
        err: &FileInterfaceError,
    ) -> Self {
        Self::new(
            namespace,
            identity,
            owner,
            kind,
            canonical_name,
            projected_surfaces,
            format!("{err:?}"),
        )
    }
}

fn interface_member_kind_name(kind: InterfaceMemberKind) -> &'static str {
    match kind {
        InterfaceMemberKind::Function => "function",
        InterfaceMemberKind::Type => "type",
        InterfaceMemberKind::Const => "const",
        InterfaceMemberKind::Method => "method",
        InterfaceMemberKind::Field => "field",
        InterfaceMemberKind::Variant => "variant",
    }
}

#[cfg(feature = "hir-fhir")]
fn interface_unavailable_to_wire(gate: &InterfaceUnavailable) -> radix_hir_fhir::UnavailableWire {
    radix_hir_fhir::UnavailableWire {
        target: gate.target.clone(),
        reason: gate.reason.clone(),
    }
}

#[cfg(feature = "hir-fhir")]
fn interface_unavailable_from_wire(gate: radix_hir_fhir::UnavailableWire) -> InterfaceUnavailable {
    InterfaceUnavailable {
        target: gate.target,
        reason: gate.reason,
    }
}

#[cfg(feature = "hir-fhir")]
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn interface_const_value_to_wire(value: &InterfaceConstValue) -> radix_hir_fhir::ConstValueWire {
    use radix_hir_fhir::ConstValueWire;
    match value {
        InterfaceConstValue::Int(int) => ConstValueWire::Int {
            high: (*int >> 64) as i64,
            low: *int as u64,
        },
        InterfaceConstValue::Float(float) => ConstValueWire::Float(*float),
        InterfaceConstValue::Bool(flag) => ConstValueWire::Bool(*flag),
        InterfaceConstValue::Text(text) => ConstValueWire::Text(text.clone()),
        InterfaceConstValue::List(items) => {
            ConstValueWire::List(items.iter().map(interface_const_value_to_wire).collect())
        }
    }
}

#[cfg(feature = "hir-fhir")]
fn interface_const_value_from_wire(value: radix_hir_fhir::ConstValueWire) -> InterfaceConstValue {
    use radix_hir_fhir::ConstValueWire;
    match value {
        ConstValueWire::Int { high, low } => {
            InterfaceConstValue::Int((i128::from(high) << 64) | i128::from(low))
        }
        ConstValueWire::Float(float) => InterfaceConstValue::Float(float),
        ConstValueWire::Bool(flag) => InterfaceConstValue::Bool(flag),
        ConstValueWire::Text(text) => InterfaceConstValue::Text(text),
        ConstValueWire::List(items) => InterfaceConstValue::List(
            items
                .into_iter()
                .map(interface_const_value_from_wire)
                .collect(),
        ),
    }
}

#[cfg(feature = "hir-fhir")]
fn interface_nominal_kind_from_wire(kind: radix_hir_fhir::NominalKind) -> InterfaceNominalKind {
    match kind {
        radix_hir_fhir::NominalKind::Struct => InterfaceNominalKind::Struct,
        radix_hir_fhir::NominalKind::Enum => InterfaceNominalKind::Enum,
        radix_hir_fhir::NominalKind::Interface => InterfaceNominalKind::Interface,
    }
}

#[cfg(feature = "hir-fhir")]
fn interface_nominal_kind_to_wire(kind: InterfaceNominalKind) -> radix_hir_fhir::NominalKind {
    match kind {
        InterfaceNominalKind::Struct => radix_hir_fhir::NominalKind::Struct,
        InterfaceNominalKind::Enum => radix_hir_fhir::NominalKind::Enum,
        InterfaceNominalKind::Interface => radix_hir_fhir::NominalKind::Interface,
    }
}

#[cfg(feature = "hir-fhir")]
fn nominal_kind_wire_sort_key(kind: radix_hir_fhir::NominalKind) -> u8 {
    match kind {
        radix_hir_fhir::NominalKind::Struct => 0,
        radix_hir_fhir::NominalKind::Enum => 1,
        radix_hir_fhir::NominalKind::Interface => 2,
    }
}

/// Stable index into the resolver's scope arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);

/// Semantic role for a lexical scope.
///
/// The role is used both for name-binding shape and for context-sensitive
/// validation such as loop-only control flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    /// Root scope for declarations visible at compilation-unit level.
    Global,

    /// Reserved module boundary for package/import-aware resolution.
    Module,

    /// Function body boundary; loop checks do not search past it.
    Function,

    /// Ordinary lexical block for local bindings.
    Block,

    /// Loop body where `break` and `continue` are valid.
    Loop,

    /// Breakable block (`fac`, closure `fac`, future `meus` / `tuus` arms).
    Breakable,

    /// Pattern-match arm scope for bindings introduced by a case.
    Match,
}

/// One name binding inside a lexical frame.
///
/// `kind` is stored at define time so type-namespace lookup can decide
/// type-vs-value with a single map probe per frame (no second `symbols` hash).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScopeEntry {
    pub def_id: DefId,
    pub kind: SymbolKind,
}

/// One lexical lookup frame.
///
/// `symbols` maps source-level interned names to definition identities. The
/// heavier symbol metadata lives in [`Resolver`] so callers can retain a
/// `DefId` after leaving the scope where it was declared.
#[derive(Clone, Debug)]
pub struct Scope {
    pub id: ScopeId,
    pub parent: Option<ScopeId>,
    pub kind: ScopeKind,
    pub symbols: FxHashMap<LexSymbol, ScopeEntry>,
    /// Reader-pack type spellings displaced from `symbols` by a value binding of
    /// the same name in this frame. They stay reachable from type syntax.
    pub shadowed_types: FxHashMap<LexSymbol, ScopeEntry>,
    /// Prior nearest-name values for bindings introduced in this frame.
    pub shadow_deltas: Vec<(LexSymbol, Option<DefId>)>,
}

impl Scope {
    #[must_use]
    pub fn new(id: ScopeId, parent: Option<ScopeId>, kind: ScopeKind) -> Self {
        Self {
            id,
            parent,
            kind,
            symbols: FxHashMap::default(),
            shadowed_types: FxHashMap::default(),
            shadow_deltas: Vec::new(),
        }
    }
}

/// Metadata attached to one definition identity.
///
/// Symbols are the resolver's long-lived contract with HIR lowering and later
/// semantic passes: once a source name has been resolved to a `DefId`, this
/// record carries the declaration kind, optional type, mutability, and source
/// span needed for diagnostics.
#[derive(Clone, Debug)]
pub struct Symbol {
    pub def_id: DefId,
    pub name: LexSymbol,
    pub kind: SymbolKind,
    pub ty: Option<TypeId>,
    pub mutable: bool,
    pub span: Span,
}

/// Qualified source spellings for imported nominal exports, keyed by type id
/// (type positions) and by definition id (struct-literal expressions).
#[derive(Debug, Clone, Default)]
pub struct ImportedSpellings {
    /// `TypeId` → `base.Vector3` for nominal type references.
    pub by_type: FxHashMap<TypeId, String>,
    /// `DefId` → `base.Vector3` for nominal construction expressions.
    pub by_def: FxHashMap<DefId, String>,
    /// `DefId` → `base.Kind.VARIANT` for `casu` pattern arms. Pattern
    /// position accepts (and ambiguity requires) the union-qualified
    /// three-part spelling; expression/call position does not, so it cannot
    /// ride `by_def`.
    pub by_def_pattern: FxHashMap<DefId, String>,
}

/// One imported file namespace after its portable interface has been remapped
/// into this analysis unit's symbol and type tables.
#[derive(Debug, Clone)]
pub struct ImportedFileInterface {
    exports: FxHashMap<LexSymbol, ImportedFileExport>,
    /// Generic edge-policy contracts exported with callables, keyed by their
    /// consumer-visible member spelling. The synthetic `DefId` is local to this
    /// resolver and exists only to key semantic call obligations.
    tensor_edge_policy_requirements:
        FxHashMap<LexSymbol, (DefId, Vec<TensorEdgePolicyRequirement>)>,
    /// Canonical source names accepted in addition to the consumer-locale
    /// spelling. The exported table stays surface-keyed for check/resolve.
    /// Target emit (Rust/TS) must write the canonical name: the generated
    /// library module is compiled from the library's native source.
    canonical_exports: FxHashMap<LexSymbol, LexSymbol>,
    /// Non-`la` pack declared rows for this identity: only pack surfaces
    /// resolve. Canonical names are not aliases unless listed as identity rows.
    strict_members: bool,
    identity: Option<InterfaceLibraryIdentity>,
    /// Const data-member export names. Distinct from type exports: only
    /// consts are first-class value references (`module.CONST`), resolved by
    /// [`Resolver::namespace_file_const`].
    const_members: FxHashSet<LexSymbol>,
    /// Folded values of const members whose exporter folded them (ST2-1),
    /// keyed like `const_members`. A const without an entry is not constant
    /// in a consumer's `=` initializer.
    const_values: FxHashMap<LexSymbol, InterfaceConstValue>,
    /// `@ nondum` gates of exported functions, keyed like `exports` (the
    /// consumer-visible member spelling).
    unavailable: FxHashMap<LexSymbol, Vec<InterfaceUnavailable>>,
    /// Device (`@ nucleum`) contracts of exported functions, keyed like
    /// `exports`. Semantic-only: the FHIR wire does not carry it, and a
    /// resolver rebuilt from a snapshot has none.
    device_contracts: FxHashMap<LexSymbol, InterfaceDeviceContract>,
}

/// Remapped contract for a single imported file export.
#[derive(Debug, Clone)]
enum ImportedFileExport {
    /// Imported function: remapped signature plus the declared `axis`
    /// markers in parameter order. A [`FuncSig`] collapses `axis` into
    /// `SemanticParamMode::Owned`, so the flags travel beside it rather
    /// than being lost at the import boundary.
    Function(FuncSig, Vec<bool>),
    Type(ImportedFileType),
    Struct(ImportedFileStruct),
}

#[derive(Debug, Clone)]
struct ImportedFileType {
    ty: TypeId,
    def_id: Option<DefId>,
    methods: Vec<ImportedFileInterfaceMethod>,
    /// Ordered declaration parameter kinds for the exported alias/nominal
    /// (S0-R1). Drives kind-directed application checking on
    /// `namespace.Type<…>` uses; empty for const exports and legacy
    /// artifact-restored interfaces.
    decl_param_kinds: Vec<radix_syntax::GenericParamKind>,
}

/// One `@ ad` route served by an imported file, with its conversation
/// types materialized into this file's type table (D6.11).
#[derive(Debug, Clone, Copy)]
pub struct ImportedAdRoute {
    pub route: LexSymbol,
    /// Handler parameter type, or `nihil`.
    pub opener: TypeId,
    /// Item type, or `vacuum`.
    pub reply: TypeId,
}

#[derive(Debug, Clone)]
pub struct ImportedFileInterfaceMethod {
    /// Consumer-local definition identity for this imported callback. It is
    /// allocated while the imported method descriptor is installed so later
    /// semantic sidecars can retain a checked callback identity without
    /// borrowing a producer arena's `DefId`.
    pub def_id: DefId,
    /// Canonical library method identity.
    pub name: LexSymbol,
    /// Consumer-locale spelling accepted at the source boundary.
    pub surface_name: LexSymbol,
    pub sig: FuncSig,
    /// Declared `axis` markers in parameter order; empty when the exporter
    /// predates the flag or the method declares none.
    pub axis: Vec<bool>,
    /// Member visibility for `genus` methods (D5.3); public elsewhere.
    pub visibility: radix_syntax::MemberVisibility,
    /// `@ nondum` gates of a `genus` method; every call is refused (SEM017).
    pub unavailable: Vec<InterfaceUnavailable>,
}

#[derive(Debug, Clone)]
struct ImportedFileStruct {
    is_json: bool,
    /// Contracts the exporting genus names in `implet`, remapped into this
    /// file's type table. Phase 2 of file import fills it (every nominal is
    /// installed by then); typecheck reads it as the declared conformance.
    implements: Vec<radix_types::ContractRef>,
    def_id: DefId,
    ty: TypeId,
    fields: Vec<ImportedFileStructField>,
    methods: Vec<ImportedFileInterfaceMethod>,
    /// Ordered generic-declaration parameter names, interned at import so the
    /// MIR lowering context can register `struct_type_params` for imported
    /// generic structs (field projection through an applied nominal).
    type_params: Vec<LexSymbol>,
}

#[derive(Debug, Clone)]
pub struct ImportedFileStructField {
    pub name: LexSymbol,
    pub ty: TypeId,
    pub optional: bool,
    pub required: bool,
    /// Authored genus field modifier (D16.1), carried across the interface.
    pub modifier: Option<radix_syntax::FieldModifier>,
    /// Member visibility (D5.3), carried across the interface.
    pub visibility: radix_syntax::MemberVisibility,
    pub init: Option<HirExpression>,
}

/// One imported file struct: its def id, fields, interface methods, and type parameters.
pub type ImportedFileStructEntry = (
    DefId,
    Vec<ImportedFileStructField>,
    Vec<ImportedFileInterfaceMethod>,
    Vec<LexSymbol>,
);

#[derive(Debug, Clone, Copy)]
pub struct ImportedFileTypeExport {
    pub member: LexSymbol,
    pub def_id: DefId,
}

/// Canonical module-qualified nominal-identity key.
///
/// Shape: nominal kind + home-module `InterfaceLibraryIdentity` (provider,
/// package, module path) + export name. Two references to the same nominal
/// (library analysis + consumer analysis) carry the same key and unify to one
/// semantic type; distinct nominals with the same short name from different
/// module identities carry different keys and stay distinct.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ImportedNominalKey {
    kind: InterfaceNominalKind,
    identity: Option<InterfaceLibraryIdentity>,
    name: LexSymbol,
}

/// Consumer-side enum variant metadata.
#[derive(Debug, Clone)]
pub struct ImportedEnumVariant {
    pub def_id: DefId,
    pub name: LexSymbol,
    /// Payload fields in declaration order (consumer type table); empty for
    /// unit variants.
    pub fields: Vec<ImportedFileStructField>,
    /// True when at least one payload field failed consumer-side remap and is
    /// omitted from `fields`. The row stays visible so the commune projection
    /// can arm the variant (its pattern binds only the shared field), but
    /// [`Resolver::variant_child`] excludes the row so the package-MIR merge
    /// still fails closed on the unknown payload identity (council-11).
    pub payload_unresolved: bool,
}

/// One `@ commune` field on a union, local or imported.
///
/// Construction and matching concatenate these in front of each variant's own
/// payload. Member access on the union type looks them up here.
#[derive(Debug, Clone, Copy)]
pub struct CommuneField {
    pub name: LexSymbol,
    pub ty: TypeId,
    pub span: Span,
}

/// Ordered concrete conversion-pair identity: exactly `(source → destination)`.
///
/// `(A, B)` and `(B, A)` are distinct rows. Generic helpers cannot key rows;
/// both ends must be concrete `TypeId`s by the time a row is enrolled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConversionPair {
    pub source: TypeId,
    pub destination: TypeId,
}

impl ConversionPair {
    #[must_use]
    pub fn new(source: TypeId, destination: TypeId) -> Self {
        Self {
            source,
            destination,
        }
    }
}

/// One row of the conversion coherence registry.
///
/// Builtin and user rows share this shape and the same ordered-pair identity.
/// A builtin row carries no user `fn_def_id` and enrolls in the global scope,
/// so it is visible from every scope and can never be shadowed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversionRow {
    pub pair: ConversionPair,
    /// Owning callable; `None` marks a compiler-shipped builtin row.
    pub fn_def_id: Option<DefId>,
    /// Declaration span; builtin rows seeded without a source location use
    /// [`Span::default`].
    pub span: Span,
    /// Scope the row was enrolled in.
    pub scope: ScopeId,
    /// Whether the row is compiler-shipped. Builtin rows reject user rows for
    /// the same pair at enrollment time (non-shadowing).
    pub builtin: bool,
}

/// Why a conversion row could not be enrolled.
///
/// Both variants carry the two source spans of the conflicting rows so
/// diagnostics can report both declarations, mirroring the union duplicate-arm
/// check's `first_span` / `second_span` shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionRowConflict {
    /// The ordered pair is already enrolled in the same scope.
    Duplicate { first_span: Span, second_span: Span },
    /// The ordered pair is owned by a compiler-shipped builtin row; a user row
    /// may not shadow it in any scope.
    BuiltinShadow { builtin_span: Span, user_span: Span },
}

fn exported_member_kind(export: &FileExportKind) -> InterfaceMemberKind {
    match export {
        FileExportKind::Function(_) => InterfaceMemberKind::Function,
        FileExportKind::TypeAlias(_)
        | FileExportKind::Struct(_)
        | FileExportKind::Enum(_)
        | FileExportKind::Interface(_) => InterfaceMemberKind::Type,
        FileExportKind::Const(..) => InterfaceMemberKind::Const,
    }
}

/// Whether an installed file export carries the given consumer `DefId`.
fn export_contains_def_id(export: &ImportedFileExport, def_id: DefId) -> bool {
    match export {
        ImportedFileExport::Function(..) => false,
        ImportedFileExport::Type(ty) => ty.def_id == Some(def_id),
        ImportedFileExport::Struct(strukt) => strukt.def_id == def_id,
    }
}

fn projected_member_names(
    locale_pack: Option<&LocalePack>,
    identity: Option<&InterfaceLibraryIdentity>,
    owner: Option<&str>,
    kind: InterfaceMemberKind,
    canonical: &str,
) -> Vec<String> {
    let Some(pack) = locale_pack else {
        return vec![canonical.to_owned()];
    };
    let Some(identity) = identity else {
        return vec![canonical.to_owned()];
    };
    let surfaces = pack.library_member_surfaces(identity, owner, kind, canonical);
    if surfaces.is_empty() {
        vec![canonical.to_owned()]
    } else {
        surfaces.into_iter().map(str::to_owned).collect()
    }
}

/// Resolve the source identity and canonical member name of a re-export.
///
/// Library interfaces retain child exports under names such as
/// `data.BufferGeometry`. The nested nominal still belongs to the child
/// module (`geometry/data`), not the parent facade (`geometry`). Importing the
/// re-export under the parent identity therefore makes a same-module nominal
/// in a callable signature look unknown. Carry the re-export path back into
/// the identity and project locale rows against the child member.
fn reexport_context(
    identity: Option<&InterfaceLibraryIdentity>,
    member: &str,
) -> (Option<InterfaceLibraryIdentity>, String) {
    let Some((prefix, canonical)) = member.rsplit_once('.') else {
        return (identity.cloned(), member.to_owned());
    };
    let Some(identity) = identity else {
        return (None, canonical.to_owned());
    };
    let mut child_identity = identity.clone();
    child_identity.module_path.extend(
        prefix
            .split('.')
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned),
    );
    (Some(child_identity), canonical.to_owned())
}

fn is_identity_locale_pack(locale_pack: Option<&LocalePack>) -> bool {
    locale_pack.is_none_or(|pack| pack.metadata.id == "la")
}

fn pack_defines_library_identity(
    locale_pack: Option<&LocalePack>,
    identity: Option<&InterfaceLibraryIdentity>,
) -> bool {
    match (locale_pack, identity) {
        (Some(pack), Some(identity)) => pack.defines_library_identity(identity),
        _ => false,
    }
}

/// Install a member under a non-`la` pack only when the pack lists it.
/// `la` / no pack, and libraries the pack does not define, stay identity-surface.
fn install_localized_member(
    locale_pack: Option<&LocalePack>,
    identity: Option<&InterfaceLibraryIdentity>,
    owner: Option<&str>,
    kind: Option<InterfaceMemberKind>,
    canonical: &str,
) -> bool {
    if is_identity_locale_pack(locale_pack) || !pack_defines_library_identity(locale_pack, identity)
    {
        return true;
    }
    let (Some(pack), Some(identity), Some(kind)) = (locale_pack, identity, kind) else {
        return true;
    };
    pack.library_member_surface(identity, owner, kind, canonical)
        .is_some()
}

fn strict_library_members(
    locale_pack: Option<&LocalePack>,
    identity: Option<&InterfaceLibraryIdentity>,
) -> bool {
    !is_identity_locale_pack(locale_pack) && pack_defines_library_identity(locale_pack, identity)
}

/// Return the surfaces that a skipped member was authorized to use.
///
/// A strict pack with no row intentionally returns an empty list. Falling back
/// to the canonical spelling here would make the diagnostic look as though the
/// omitted member remained available, which is precisely the boundary this
/// record exists to make visible.
fn skipped_member_surfaces(
    locale_pack: Option<&LocalePack>,
    identity: Option<&InterfaceLibraryIdentity>,
    owner: Option<&str>,
    kind: InterfaceMemberKind,
    canonical: &str,
) -> Vec<String> {
    if strict_library_members(locale_pack, identity) {
        return locale_pack
            .and_then(|pack| {
                identity
                    .map(|identity| pack.library_member_surfaces(identity, owner, kind, canonical))
            })
            .unwrap_or_default()
            .into_iter()
            .map(str::to_owned)
            .collect();
    }
    projected_member_names(locale_pack, identity, owner, kind, canonical)
}

/// Declaration category for a resolved definition.
///
/// These categories are semantic rather than syntactic; for example, methods
/// and free functions share callable behavior later, but keeping distinct
/// symbol kinds preserves enough provenance for member lookup and diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    Local,
    /// Compiler-injected `@ nucleum lane "name"` binding.
    Lane,
    Param,
    Function,
    Method,
    Struct,
    Enum,
    Variant,
    Field,
    Interface,
    TypeAlias,
    TypeParam,
    /// Compile-time size/index parameter (`magnitudo N`).
    IndexParam,
    /// Relational-view schema declaration (`schema`). A type-namespace
    /// identity per the `typus`/`genus` declared-type precedent, distinct from
    /// a genus: the schema carries ordered relational column metadata, never a
    /// field list, and is never a runtime value.
    Schema,
    Module,
}

impl SymbolKind {
    /// Return whether this symbol can be named from type syntax.
    #[must_use]
    pub fn is_type(self) -> bool {
        matches!(
            self,
            SymbolKind::Struct
                | SymbolKind::Enum
                | SymbolKind::Interface
                | SymbolKind::TypeAlias
                | SymbolKind::TypeParam
                | SymbolKind::Schema
        )
    }

    /// Return whether this symbol is a reasonable value-name suggestion.
    #[must_use]
    pub fn is_value_name(self) -> bool {
        !matches!(
            self,
            SymbolKind::Struct
                | SymbolKind::Enum
                | SymbolKind::Interface
                | SymbolKind::TypeAlias
                | SymbolKind::TypeParam
                | SymbolKind::IndexParam
                | SymbolKind::Schema
        )
    }
}

/// Result of a bare-name variant lookup through the per-union namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BareVariantLookup {
    /// No union in scope defines this variant name.
    None,
    /// Exactly one union defines it — unambiguous bare resolution.
    Single(DefId),
    /// Two or more unions define it — bare use is an ambiguity.
    Ambiguous { defining_unions: usize },
}

impl BareVariantLookup {
    /// Return the resolved variant when the bare name is unambiguous.
    #[must_use]
    pub fn def_id(self) -> Option<DefId> {
        match self {
            BareVariantLookup::Single(def_id) => Some(def_id),
            BareVariantLookup::None | BareVariantLookup::Ambiguous { .. } => None,
        }
    }
}

/// Result of resolving a variant path's qualified imported parent (`a.A.X`).
///
/// Qualification binds the imported parent identity before any bare-variant
/// fallback (CTO UVN-1): two imported unions may share a terminal variant
/// name, and a miss against an identified variant-bearing parent is a hard
/// error — it never falls through to the bare table and binds through another
/// union.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualifiedVariantPathLookup {
    /// Not a qualified imported variant path: no module namespace prefix, the
    /// penultimate segment names no type export, or the type export carries
    /// no variants. Callers keep the legacy bare fallback.
    NotQualified,
    /// The terminal segment is one of the imported type's declared variants.
    Variant(DefId),
    /// The penultimate names a variant-bearing type export of the namespace
    /// but the terminal is not among its declared variants — fail closed.
    UnknownVariant,
}

pub use radix_types::USER_DEF_ID_BASE;
const BUILTIN_DEF_ID_BASE: u32 = 0x0000_0001;

/// Mutable name-resolution state shared by collection, resolution, and HIR lowering.
///
/// The resolver behaves like a stack while walking syntax, but stores scopes in
/// an arena so `ScopeId` and `DefId` values remain stable after traversal moves
/// elsewhere. Def-id ranges are partitioned so compiler-injected builtins (low
/// range) never collide with user declarations (high range). This keeps user
/// def ids stable regardless of how many builtins register before user
/// collection.
#[derive(Clone)]
pub struct Resolver {
    scopes: Vec<Scope>,
    symbols: FxHashMap<DefId, Symbol>,
    /// Ordered declaration-identity metadata for applied type arguments.
    /// Kept separate from identifier spellings so kind selection is never
    /// inferred from the source name.
    generic_param_kinds: FxHashMap<DefId, Vec<radix_syntax::GenericParamKind>>,
    /// Ordered generic descriptors, including declaration defaults, for local
    /// named types. HIR lowering uses this identity-keyed row when applying a
    /// trailing default suffix to a type application.
    generic_param_descriptors: FxHashMap<DefId, Vec<radix_types::GenericParamDescriptor>>,
    current_scope: ScopeId,
    next_def_id: u32,
    next_builtin_def_id: u32,
    builtin_forma_def_id: Option<DefId>,
    builtin_forma_template_field: Option<LexSymbol>,
    builtin_forma_params_field: Option<LexSymbol>,
    builtin_scrinium_def_id: Option<DefId>,
    builtin_scrinium_field_symbols: Vec<LexSymbol>,
    builtin_status_def_id: Option<DefId>,
    builtin_sermo_def_id: Option<DefId>,
    builtin_meus_def_id: Option<DefId>,
    builtin_tuus_def_id: Option<DefId>,
    /// Builtin generic genus `trapped<R, T>` (f9d8a611): definition id, plus
    /// whether the program applied it (a `trapped<...>` type position or a
    /// `capta` boundary, which applies it implicitly).
    builtin_trapped_def_id: Option<DefId>,
    builtin_trapped_applied: bool,
    /// Conversation types of `@ ad` routes served by imported files (D6.11).
    imported_ad_routes: Vec<ImportedAdRoute>,
    builtin_status_variants: Vec<(DefId, LexSymbol)>,
    variant_parents: FxHashMap<DefId, DefId>,
    variants_by_parent_name: FxHashMap<(DefId, LexSymbol), DefId>,
    variants_by_name: FxHashMap<LexSymbol, Vec<DefId>>,
    /// O(1) GPU builtin lookup by definition id (hot path in call checking).
    gpu_builtins: FxHashMap<DefId, radix_types::MirKernelBuiltin>,
    /// Insertion-order mirror of `gpu_builtins` for callers that need a slice.
    gpu_builtins_order: Vec<(DefId, radix_types::MirKernelBuiltin)>,
    /// Nearest visible name → `DefId` for the current scope chain (O(1) lookup).
    /// Maintained eagerly on define / `exit_scope`.
    lookup_shadow: FxHashMap<LexSymbol, DefId>,
    /// Cache of `name_suggestion_key` folds per interned symbol.
    ///
    /// The folded key is a pure function of the interned string, so entries
    /// never go stale across scope push/pop (a per-snapshot candidate-set
    /// cache would be unsound — visible names are scope-position-dependent).
    /// Lazily populated by the semantic name-suggestion path; derived runtime
    /// state, excluded from snapshots.
    pub(crate) name_suggestion_folds: RefCell<FxHashMap<LexSymbol, String>>,
    /// Optional package-supplied export table keyed by namespace binding name;
    /// each member carries its visibility tier so the import-seam policy can
    /// admit same-package `@ interna` and reject the rest.
    namespace_exports: FxHashMap<LexSymbol, FxHashMap<LexSymbol, VisibilityTier>>,
    /// Per-binding same-package flag. Missing bindings are cross-package.
    namespace_seams: FxHashMap<LexSymbol, bool>,
    /// All target top-level names, including module-private, keyed by binding.
    namespace_declarations: FxHashMap<LexSymbol, FxHashMap<LexSymbol, VisibilityTier>>,
    /// Package-supplied typed file interfaces keyed by namespace binding name.
    file_interfaces: FxHashMap<LexSymbol, ImportedFileInterface>,
    /// Imported nominal export types keyed by portable nominal name.
    imported_nominal_types: FxHashMap<(InterfaceNominalKind, LexSymbol), TypeId>,
    /// Nominal names that occur in more than one imported namespace.
    ///
    /// The current file-interface snapshot format does not carry namespace
    /// provenance for nested nominal types. Marking collisions explicitly
    /// prevents the last imported namespace from silently winning.
    ambiguous_imported_nominal_types: FxHashSet<(InterfaceNominalKind, LexSymbol)>,
    /// Canonical nominal-identity table: imported nominals keyed by their
    /// module-qualified identity (provider + package + module path) plus export
    /// name. The same module imported under two aliases unifies to one semantic
    /// type; same-name nominals from different module identities stay distinct.
    /// Runtime-only state, excluded from FHIR snapshots; the legacy name-keyed
    /// table above remains the snapshot-restored resolution surface.
    canonical_imported_nominal_types: FxHashMap<ImportedNominalKey, TypeId>,
    /// Canonical key → consumer `DefId` for the unified nominal. Duplicate
    /// installs (same module, second alias) reuse the first install's def so
    /// type positions, struct literals, and variant references all key off one
    /// identity.
    canonical_imported_nominal_defs: FxHashMap<ImportedNominalKey, DefId>,
    /// Consumer-side enum variant metadata keyed by the imported enum's
    /// consumer `DefId`. Populated when an enum export is installed; consumed
    /// by typecheck (variant references) and the MIR lowering context (variant
    /// aggregates).
    imported_enum_variants: FxHashMap<DefId, Vec<ImportedEnumVariant>>,
    /// `@ commune` fields keyed by the union's `DefId` (local after lowering,
    /// imported after file-interface install). Runtime-only: FHIR snapshots
    /// rebuild imported rows from the interface; TEU3 carries the region on
    /// HIR for local emit.
    commune_fields: FxHashMap<DefId, Vec<CommuneField>>,
    /// Conversion coherence registry keyed by the ordered concrete pair
    /// `(source → destination)`. Builtin and user rows share this one
    /// table; there is no second compatibility table.
    conversion_rows: FxHashMap<ConversionPair, Vec<ConversionRow>>,
    /// `DefIds` of nominal types installed from imported file interfaces.
    /// Runtime-only ownership surface for orphan-style conversion rules.
    imported_type_def_ids: FxHashSet<DefId>,
    /// Namespace bindings referenced from qualified type paths.
    used_namespaces: FxHashSet<LexSymbol>,
    /// Resolved relational schema metadata keyed by schema definition
    /// identity. Populated by the resolve pass; the ordered five-field column
    /// metadata is the schema-side admission contract. Runtime-only state,
    /// excluded from import snapshots: schemas are source-level declarations
    /// of the analyzed unit.
    schemas: FxHashMap<DefId, crate::SchemaDef>,
    /// Reader-locale pack `[types]` surfaces registered as builtin `TypeAlias`
    /// defs. These identities are not HIR items, so `NameCatalog` and imported
    /// type-paths miss them unless emit admits this map.
    locale_pack_type_aliases: FxHashMap<DefId, LexSymbol>,
    /// Builtin-claimed expression nodes (`lege`/`lineam`/`vacua`/`scriptum`)
    /// whose surface spelling is bound to a value in the live lexical scope.
    /// Lowering consults this table and emits a name load instead of the builtin.
    /// Runtime-only walk state; excluded from FHIR snapshots.
    shadowed_builtins: FxHashMap<radix_syntax::NodeId, DefId>,
    /// Surface spellings of the scribe family (`nota`/`vide`/`mone`/`scribe`
    /// and the active pack's aliases). Empty means registry-direct Latin.
    /// Runtime-only hint state for `SEM001.scribe_callee`; excluded from snapshots.
    scribe_family_surfaces: FxHashSet<String>,
    /// Checker-side `iuncta` element labels keyed by binding `DefId`.
    ///
    /// Labels never enter `Type::Tuple`. Runtime-only; excluded from snapshots.
    tuple_element_labels: FxHashMap<DefId, Vec<Option<LexSymbol>>>,
    /// Member names of a binding annotation's closed set, keyed by the
    /// binding `DefId`.
    ///
    /// A `discerne` arm names a member of the scrutinee's set or it is an
    /// error; when every named arm of a subject position is misspelled, the
    /// arms alone cannot spell the set. The annotation can: `Punctum ∪ Linea`
    /// names both members, and `Color`/`Forma` name their variants. Runtime-only;
    /// excluded from snapshots.
    subject_member_sets: FxHashMap<DefId, Vec<String>>,
    /// Selective imports that bind a type export (D22.5), keyed by the
    /// caller-owned binding `DefId` → (import path, exported spelling). The
    /// binding names the imported type itself: type positions resolve it to
    /// the same `TypeId` as the qualified path. Runtime-only; excluded from
    /// snapshots (type positions are resolved before the artifact is cut).
    selective_type_imports: FxHashMap<DefId, (LexSymbol, LexSymbol)>,
    /// Selective-import bindings that name an exported function marked
    /// `@ nondum`, keyed by the caller-owned binding `DefId`. Every use of the
    /// binding is refused (SEM017). Runtime-only; excluded from snapshots.
    unavailable_bindings: FxHashMap<DefId, Vec<InterfaceUnavailable>>,
    /// Declaration parameter names of imported implendum exports (D1.2).
    imported_contract_type_params: FxHashMap<DefId, Vec<LexSymbol>>,
    /// Declaration parameter names of imported generic enum exports, keyed by
    /// the consumer enum `DefId`. Variant payload fields reference these names,
    /// so construction needs them to substitute applied witnesses.
    imported_enum_type_params: FxHashMap<DefId, Vec<LexSymbol>>,
    /// `@ radix contract` roles of imported implendum exports (D1.4).
    imported_contract_roles: FxHashMap<DefId, String>,
}

impl Resolver {
    /// Build a resolver with an empty global scope.
    #[must_use]
    pub fn new() -> Self {
        let global = Scope::new(ScopeId(0), None, ScopeKind::Global);
        Self {
            scopes: vec![global],
            symbols: FxHashMap::default(),
            generic_param_kinds: FxHashMap::default(),
            generic_param_descriptors: FxHashMap::default(),
            current_scope: ScopeId(0),
            next_def_id: USER_DEF_ID_BASE,
            next_builtin_def_id: BUILTIN_DEF_ID_BASE,
            builtin_forma_def_id: None,
            builtin_forma_template_field: None,
            builtin_forma_params_field: None,
            builtin_scrinium_def_id: None,
            builtin_scrinium_field_symbols: Vec::new(),
            builtin_status_def_id: None,
            builtin_sermo_def_id: None,
            builtin_meus_def_id: None,
            builtin_tuus_def_id: None,
            builtin_trapped_def_id: None,
            builtin_trapped_applied: false,
            imported_ad_routes: Vec::new(),
            builtin_status_variants: Vec::new(),
            variant_parents: FxHashMap::default(),
            variants_by_parent_name: FxHashMap::default(),
            variants_by_name: FxHashMap::default(),
            gpu_builtins: FxHashMap::default(),
            gpu_builtins_order: Vec::new(),
            lookup_shadow: FxHashMap::default(),
            name_suggestion_folds: RefCell::new(FxHashMap::default()),
            namespace_exports: FxHashMap::default(),
            namespace_seams: FxHashMap::default(),
            namespace_declarations: FxHashMap::default(),
            file_interfaces: FxHashMap::default(),
            imported_nominal_types: FxHashMap::default(),
            ambiguous_imported_nominal_types: FxHashSet::default(),
            canonical_imported_nominal_types: FxHashMap::default(),
            canonical_imported_nominal_defs: FxHashMap::default(),
            imported_enum_variants: FxHashMap::default(),
            commune_fields: FxHashMap::default(),
            conversion_rows: FxHashMap::default(),
            imported_type_def_ids: FxHashSet::default(),
            used_namespaces: FxHashSet::default(),
            schemas: FxHashMap::default(),
            locale_pack_type_aliases: FxHashMap::default(),
            shadowed_builtins: FxHashMap::default(),
            scribe_family_surfaces: FxHashSet::default(),
            tuple_element_labels: FxHashMap::default(),
            subject_member_sets: FxHashMap::default(),
            selective_type_imports: FxHashMap::default(),
            unavailable_bindings: FxHashMap::default(),
            imported_contract_type_params: FxHashMap::default(),
            imported_enum_type_params: FxHashMap::default(),
            imported_contract_roles: FxHashMap::default(),
        }
    }

    /// Record scribe-family surfaces from the active reader pack.
    ///
    /// An empty set (no pack) keeps registry-direct Latin detection.
    pub fn load_scribe_family_surfaces(&mut self, pack: Option<&LocalePack>) {
        self.scribe_family_surfaces.clear();
        let Some(pack) = pack else {
            return;
        };
        for key in ["nota", "vide", "mone", "scribe"] {
            if let Some(binding) = pack.keywords.get(key) {
                self.scribe_family_surfaces
                    .insert(binding.canonical.clone());
                self.scribe_family_surfaces
                    .extend(binding.aliases.iter().cloned());
            }
        }
    }

    /// Return whether `spelling` is a scribe-family surface in the active pack.
    #[must_use]
    pub(crate) fn is_scribe_family_surface(&self, spelling: &str) -> bool {
        if !self.scribe_family_surfaces.is_empty() {
            return self.scribe_family_surfaces.contains(spelling);
        }
        matches!(
            radix_lexer::lookup_keyword_spec(spelling).and_then(|spec| spec.token_kind.clone()),
            Some(
                radix_lexer::TokenKind::Nota
                    | radix_lexer::TokenKind::Vide
                    | radix_lexer::TokenKind::Mone
                    | radix_lexer::TokenKind::Scribe
            )
        )
    }

    /// Install package-supplied file-namespace exports (name → tier) using this
    /// compilation's interner.
    pub fn set_namespace_exports_from_strings(
        &mut self,
        exports: &BTreeMap<String, BTreeMap<String, VisibilityTier>>,
        interner: &mut radix_lexer::Interner,
    ) {
        self.namespace_exports.clear();
        for (namespace, members) in exports {
            let namespace = interner.intern(namespace);
            let members = members
                .iter()
                .map(|(member, tier)| (interner.intern(member), *tier))
                .collect();
            self.namespace_exports.insert(namespace, members);
        }
    }

    /// Install per-binding same-package flags for the import-seam check.
    pub fn set_namespace_seams_from_strings(
        &mut self,
        seams: &BTreeMap<String, bool>,
        interner: &mut radix_lexer::Interner,
    ) {
        self.namespace_seams.clear();
        for (namespace, same_package) in seams {
            self.namespace_seams
                .insert(interner.intern(namespace), *same_package);
        }
    }

    /// Install all target declaration tiers, including module-private names.
    pub fn set_namespace_declarations_from_strings(
        &mut self,
        declarations: &BTreeMap<String, BTreeMap<String, VisibilityTier>>,
        interner: &mut radix_lexer::Interner,
    ) {
        self.namespace_declarations.clear();
        for (namespace, members) in declarations {
            let namespace = interner.intern(namespace);
            let members = members
                .iter()
                .map(|(member, tier)| (interner.intern(member), *tier))
                .collect();
            self.namespace_declarations.insert(namespace, members);
        }
    }

    /// Install portable file interfaces after remapping every snapshot into
    /// this compilation's semantic tables.
    ///
    /// Unsupported exports are **skipped**, not fatal. One poison export (e.g.
    /// a generic or unknown nominal in a multi-export module) must not block
    /// sibling plain exports such as `solum.carpe` / `solum.exstat`.
    ///
    /// Returns skipped export records so callers can emit non-fatal diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`FileInterfaceError`] when a nominal type identity cannot be
    /// resolved or is ambiguous across imported namespaces.
    pub fn set_file_interfaces_from_snapshots(
        &mut self,
        interfaces: &BTreeMap<String, FileInterface>,
        interner: &mut radix_lexer::Interner,
        types: &mut TypeTable,
    ) -> Result<Vec<FileInterfaceSkippedExport>, FileInterfaceError> {
        self.set_file_interfaces_from_snapshots_with_locale(interfaces, interner, types, None)
    }

    /// Install portable file interfaces and project public member spellings
    /// through the consumer locale pack when a stable library identity exists.
    ///
    /// # Errors
    ///
    /// Returns [`FileInterfaceError`] when a nominal type identity cannot be
    /// resolved or is ambiguous across imported namespaces.
    pub fn set_file_interfaces_from_snapshots_with_locale(
        &mut self,
        interfaces: &BTreeMap<String, FileInterface>,
        interner: &mut radix_lexer::Interner,
        types: &mut TypeTable,
        locale_pack: Option<&LocalePack>,
    ) -> Result<Vec<FileInterfaceSkippedExport>, FileInterfaceError> {
        let mut imported_interfaces = FxHashMap::default();
        let mut skipped = Vec::new();

        // Phase 1: install every nominal shell before remapping any payload.
        // A public file interface may name a nominal exported by another
        // namespace that sorts later in `interfaces` (e.g. scene -> triga).
        // Keeping this phase global prevents BTreeMap order from hollowing
        // otherwise valid struct fields and callable signatures.
        for (namespace_name, interface) in interfaces {
            let namespace = interner.intern(namespace_name);
            let mut exports = FxHashMap::default();
            let mut canonical_exports = FxHashMap::default();
            let identity = interface.identity.clone();
            let strict_members = strict_library_members(locale_pack, identity.as_ref());

            for (member, export) in &interface.exports {
                let member_kind = exported_member_kind(&export.kind);
                let (export_identity, canonical_member) =
                    reexport_context(identity.as_ref(), member);
                if !install_localized_member(
                    locale_pack,
                    export_identity.as_ref(),
                    None,
                    Some(member_kind),
                    &canonical_member,
                ) {
                    skipped.push(FileInterfaceSkippedExport::new(
                        namespace_name,
                        export_identity.as_ref(),
                        None,
                        member_kind,
                        member,
                        skipped_member_surfaces(
                            locale_pack,
                            export_identity.as_ref(),
                            None,
                            member_kind,
                            &canonical_member,
                        ),
                        "strict_locale_pack_member_omitted",
                    ));
                    continue;
                }
                let projected_members = projected_member_names(
                    locale_pack,
                    export_identity.as_ref(),
                    None,
                    member_kind,
                    &canonical_member,
                );
                let primary_member = projected_members
                    .first()
                    .map_or(member.as_str(), String::as_str);
                let primary_sym = interner.intern(primary_member);
                if primary_sym != interner.intern(member) {
                    canonical_exports.insert(interner.intern(member), primary_sym);
                }
                let imported = match &export.kind {
                    FileExportKind::Struct(strukt) => self.import_struct_file_export_shell(
                        strukt,
                        types,
                        interner,
                        export_identity.as_ref(),
                    ),
                    FileExportKind::Enum(nominal) => self.import_enum_file_export(
                        interner.intern(&nominal.name),
                        types,
                        nominal,
                        interner,
                        export_identity.as_ref(),
                    ),
                    FileExportKind::Interface(nominal) => {
                        let imported = self.import_nominal_file_export(
                            interner.intern(&nominal.name),
                            SymbolKind::Interface,
                            types,
                            Type::Interface,
                            Vec::new(),
                            export_identity.as_ref(),
                            InterfaceGenericParam::kinds(&nominal.params),
                        );
                        // Contract arity and argument substitution (D1.2)
                        // need the imported implendum's parameter names.
                        if let ImportedFileExport::Type(ImportedFileType {
                            def_id: Some(def_id),
                            ..
                        }) = &imported
                        {
                            let params = nominal
                                .params
                                .iter()
                                .map(|param| interner.intern(&param.name))
                                .collect();
                            self.imported_contract_type_params.insert(*def_id, params);
                            if let Some(role) = &nominal.radix_contract {
                                self.imported_contract_roles.insert(*def_id, role.clone());
                            }
                        }
                        imported
                    }
                    FileExportKind::TypeAlias(_)
                    | FileExportKind::Const(..)
                    | FileExportKind::Function(_) => continue,
                };
                for projected_member in projected_members {
                    exports.insert(interner.intern(&projected_member), imported.clone());
                }
            }
            imported_interfaces.insert(
                namespace,
                ImportedFileInterface {
                    exports,
                    tensor_edge_policy_requirements: FxHashMap::default(),
                    canonical_exports,
                    strict_members,
                    identity,
                    const_members: FxHashSet::default(),
                    const_values: FxHashMap::default(),
                    unavailable: FxHashMap::default(),
                    device_contracts: FxHashMap::default(),
                },
            );
        }

        // Phase 2: remap aliases/constants, then fill struct fields and
        // interface methods now that every nominal shell exists.
        for (namespace_name, interface) in interfaces {
            let namespace = interner.intern(namespace_name);
            let Some(imported) = imported_interfaces.get_mut(&namespace) else {
                continue;
            };
            // Locale projection governs cross-package surfaces. Same-package
            // imports still need nested fields and methods to typecheck the
            // provider's own sibling modules, even when those internals have
            // no public pack row.
            let same_package = self.namespace_same_package(namespace);

            for (member, export) in &interface.exports {
                let member_kind = exported_member_kind(&export.kind);
                let (export_identity, canonical_member) =
                    reexport_context(imported.identity.as_ref(), member);
                if !install_localized_member(
                    locale_pack,
                    export_identity.as_ref(),
                    None,
                    Some(member_kind),
                    &canonical_member,
                ) {
                    continue;
                }
                let projected_members = projected_member_names(
                    locale_pack,
                    export_identity.as_ref(),
                    None,
                    member_kind,
                    &canonical_member,
                );
                // Const data members and type aliases both remap to an
                // `ImportedFileExport::Type`; record the const members so the
                // typechecker can resolve `module.CONST` value references
                // without exposing type exports as value references.
                let is_const = matches!(&export.kind, FileExportKind::Const(..));
                let (snapshot, decl_param_kinds) = match &export.kind {
                    FileExportKind::TypeAlias(alias) => (
                        &alias.body,
                        crate::file_interface::InterfaceGenericParam::kinds(&alias.params),
                    ),
                    FileExportKind::Const(snapshot, _) => (snapshot, Vec::new()),
                    _ => continue,
                };
                match import_interface_type_with_resolver(snapshot, types, interner, self) {
                    Ok(ty) => {
                        let exported = ImportedFileExport::Type(ImportedFileType {
                            ty,
                            def_id: None,
                            methods: Vec::new(),
                            decl_param_kinds,
                        });
                        for projected_member in projected_members {
                            let member_sym = interner.intern(&projected_member);
                            if is_const {
                                imported.const_members.insert(member_sym);
                                if let FileExportKind::Const(_, Some(value)) = &export.kind {
                                    imported.const_values.insert(member_sym, value.clone());
                                }
                            }
                            imported.exports.insert(member_sym, exported.clone());
                        }
                    }
                    Err(err) => skipped.push(FileInterfaceSkippedExport::from_error(
                        namespace_name,
                        export_identity.as_ref(),
                        None,
                        member_kind,
                        member,
                        projected_members.clone(),
                        &err,
                    )),
                }
            }

            // Skip fields/methods that cannot be remapped; keep the export shell.
            for (member, export) in &interface.exports {
                let member_kind = exported_member_kind(&export.kind);
                let (export_identity, canonical_member) =
                    reexport_context(imported.identity.as_ref(), member);
                if !install_localized_member(
                    locale_pack,
                    export_identity.as_ref(),
                    None,
                    Some(member_kind),
                    &canonical_member,
                ) {
                    continue;
                }
                let projected_members = projected_member_names(
                    locale_pack,
                    export_identity.as_ref(),
                    None,
                    member_kind,
                    &canonical_member,
                );
                match &export.kind {
                    FileExportKind::Struct(strukt) => {
                        let (fields, field_skipped) = self.import_struct_fields_partial(
                            namespace_name,
                            strukt,
                            export_identity.as_ref(),
                            locale_pack,
                            types,
                            interner,
                        );
                        let (methods, method_skipped) = self.import_interface_methods_partial(
                            namespace_name,
                            &strukt.methods,
                            &strukt.name,
                            export_identity.as_ref(),
                            locale_pack,
                            same_package,
                            types,
                            interner,
                        );
                        let implements = self.import_struct_implements(strukt, types, interner);
                        skipped.extend(field_skipped);
                        skipped.extend(method_skipped);
                        for projected_member in projected_members {
                            let member_sym = interner.intern(&projected_member);
                            if let Some(ImportedFileExport::Struct(existing)) =
                                imported.exports.get_mut(&member_sym)
                            {
                                existing.fields.clone_from(&fields);
                                existing.methods.clone_from(&methods);
                                existing.implements.clone_from(&implements);
                            }
                        }
                    }
                    FileExportKind::Enum(nominal) => {
                        for projected_member in projected_members {
                            let member_sym = interner.intern(&projected_member);
                            if let Some(ImportedFileExport::Type(existing)) =
                                imported.exports.get(&member_sym)
                                && let Some(enum_def) = existing.def_id
                            {
                                self.import_enum_variants_partial(
                                    enum_def, nominal, types, interner,
                                );
                            }
                        }
                    }
                    FileExportKind::Interface(nominal) => {
                        let (methods, method_skipped) = self.import_interface_methods_partial(
                            namespace_name,
                            &nominal.methods,
                            &nominal.name,
                            export_identity.as_ref(),
                            locale_pack,
                            same_package,
                            types,
                            interner,
                        );
                        skipped.extend(method_skipped);
                        for projected_member in projected_members {
                            let member_sym = interner.intern(&projected_member);
                            if let Some(ImportedFileExport::Type(existing)) =
                                imported.exports.get_mut(&member_sym)
                            {
                                existing.methods.clone_from(&methods);
                            }
                        }
                    }
                    _ => {}
                }
            }

            // Phase 3: functions (signatures may name imported genera).
            for (member, export) in &interface.exports {
                let FileExportKind::Function(callable) = &export.kind else {
                    continue;
                };
                let (export_identity, canonical_member) =
                    reexport_context(imported.identity.as_ref(), member);
                if !install_localized_member(
                    locale_pack,
                    export_identity.as_ref(),
                    None,
                    Some(InterfaceMemberKind::Function),
                    &canonical_member,
                ) {
                    continue;
                }
                let projected_members = projected_member_names(
                    locale_pack,
                    export_identity.as_ref(),
                    None,
                    InterfaceMemberKind::Function,
                    &canonical_member,
                );
                let requirements = callable
                    .tensor_edge_policy_requirements
                    .iter()
                    .map(|requirement| {
                        let policy_param = interner.intern(&requirement.policy_param);
                        let policy_type = types.intern(Type::Param(policy_param));
                        let element_type = import_interface_type_with_resolver(
                            &requirement.element_type,
                            types,
                            interner,
                            self,
                        )?;
                        Ok(TensorEdgePolicyRequirement {
                            policy_type,
                            element_type,
                            rank: requirement.rank,
                        })
                    })
                    .collect::<Result<Vec<_>, FileInterfaceError>>();
                match (
                    import_interface_callable_with_resolver(callable, types, interner, self),
                    requirements,
                ) {
                    (Ok(sig), Ok(requirements)) => {
                        let axis: Vec<bool> =
                            callable.params.iter().map(|param| param.axis).collect();
                        let exported = ImportedFileExport::Function(sig, axis);
                        let policy_def =
                            (!requirements.is_empty()).then(|| self.fresh_builtin_def_id());
                        let gates = interface.unavailable.get(member);
                        for projected_member in projected_members {
                            let member = interner.intern(&projected_member);
                            imported.exports.insert(member, exported.clone());
                            if let Some(gates) = gates {
                                imported.unavailable.insert(member, gates.clone());
                            }
                            if let Some(contract) = &callable.device {
                                imported.device_contracts.insert(member, contract.clone());
                            }
                            if let Some(policy_def) = policy_def {
                                imported
                                    .tensor_edge_policy_requirements
                                    .insert(member, (policy_def, requirements.clone()));
                            }
                        }
                    }
                    (Err(err), _) | (_, Err(err)) => {
                        skipped.push(FileInterfaceSkippedExport::from_error(
                            namespace_name,
                            export_identity.as_ref(),
                            None,
                            InterfaceMemberKind::Function,
                            member,
                            projected_members.clone(),
                            &err,
                        ));
                    }
                }
            }
        }
        self.file_interfaces = imported_interfaces;
        self.import_ad_routes(interfaces, interner, types);
        Ok(skipped)
    }

    /// Materialize the conversation types of imported `@ ad` handlers. A
    /// route whose opener or reply names a nominal this file cannot see stays
    /// untyped: its call sites keep bare `sermo` (D6.12).
    fn import_ad_routes(
        &mut self,
        interfaces: &BTreeMap<String, FileInterface>,
        interner: &mut radix_lexer::Interner,
        types: &mut TypeTable,
    ) {
        let mut routes = Vec::new();
        for export in interfaces
            .values()
            .flat_map(|interface| interface.routes.values())
        {
            let import = |snapshot: Option<&crate::file_interface::InterfaceTypeSnapshot>,
                          fallback: radix_types::Primitive,
                          types: &mut TypeTable,
                          interner: &mut radix_lexer::Interner| {
                match snapshot {
                    Some(snapshot) => {
                        import_interface_type_with_resolver(snapshot, types, interner, self).ok()
                    }
                    None => Some(types.primitive(fallback)),
                }
            };
            let Some(opener) = import(
                export.opener.as_ref(),
                radix_types::Primitive::Nihil,
                types,
                interner,
            ) else {
                continue;
            };
            let Some(reply) = import(
                export.reply.as_ref(),
                radix_types::Primitive::Vacuum,
                types,
                interner,
            ) else {
                continue;
            };
            routes.push(ImportedAdRoute {
                route: interner.intern(&export.route),
                opener,
                reply,
            });
        }
        self.imported_ad_routes = routes;
    }

    /// Conversation types of routes served by imported files.
    #[must_use]
    pub fn imported_ad_routes(&self) -> &[ImportedAdRoute] {
        &self.imported_ad_routes
    }

    fn import_nominal_file_export(
        &mut self,
        name: LexSymbol,
        kind: SymbolKind,
        types: &mut TypeTable,
        make_type: impl FnOnce(DefId) -> Type,
        methods: Vec<ImportedFileInterfaceMethod>,
        identity: Option<&InterfaceLibraryIdentity>,
        decl_param_kinds: Vec<radix_syntax::GenericParamKind>,
    ) -> ImportedFileExport {
        let nominal_kind = match kind {
            SymbolKind::Struct => Some(InterfaceNominalKind::Struct),
            SymbolKind::Enum => Some(InterfaceNominalKind::Enum),
            SymbolKind::Interface => Some(InterfaceNominalKind::Interface),
            _ => None,
        };
        // Canonical duplicate-reuse: the same module installed under a second
        // alias must unify to one semantic type, so reuse the first install's
        // type + def instead of minting a fresh nominal. Only applies to
        // identity-bearing interfaces: without a module identity the provenance
        // is unknown and the legacy name-keyed ambiguity guard must keep firing.
        if let Some((existing_ty, existing_def)) =
            identity.and_then(|_| self.canonical_nominal_for(nominal_kind, identity, name))
        {
            return ImportedFileExport::Type(ImportedFileType {
                ty: existing_ty,
                def_id: Some(existing_def),
                methods,
                decl_param_kinds,
            });
        }
        let def_id = self.fresh_def_id();
        let ty = types.intern(make_type(def_id));
        if let Some(nominal_kind) = nominal_kind {
            self.register_imported_nominal_type(nominal_kind, identity, name, ty, def_id);
        }
        if !decl_param_kinds.is_empty() {
            self.register_generic_param_kinds(def_id, decl_param_kinds.clone());
        }
        self.symbols.insert(
            def_id,
            Symbol {
                def_id,
                name,
                kind,
                ty: Some(ty),
                mutable: false,
                span: Span::default(),
            },
        );
        ImportedFileExport::Type(ImportedFileType {
            ty,
            def_id: Some(def_id),
            methods,
            decl_param_kinds,
        })
    }

    /// Install an imported enum export: the enum nominal shell plus consumer
    /// variant definitions.
    ///
    /// Variants get fresh consumer `DefId`s registered under the enum's parent
    /// link so `module.VARIANT` value references resolve, typecheck as
    /// `Type::Enum(consumer_def)`, and lower through the entry's variant
    /// aggregate seam.
    fn import_enum_file_export(
        &mut self,
        name: LexSymbol,
        types: &mut TypeTable,
        nominal: &InterfaceNominalExport,
        interner: &mut radix_lexer::Interner,
        identity: Option<&InterfaceLibraryIdentity>,
    ) -> ImportedFileExport {
        // Canonical duplicate-reuse: a second alias of the same module unifies
        // to one semantic type. Only for identity-bearing interfaces; without
        // identity the legacy name-keyed ambiguity guard must keep firing.
        if let Some((existing_ty, existing_def)) = identity.and_then(|_| {
            self.canonical_nominal_for(Some(InterfaceNominalKind::Enum), identity, name)
        }) {
            return ImportedFileExport::Type(ImportedFileType {
                ty: existing_ty,
                def_id: Some(existing_def),
                methods: Vec::new(),
                decl_param_kinds: InterfaceGenericParam::kinds(&nominal.params),
            });
        }
        let enum_def = self.fresh_def_id();
        let ty = types.intern(Type::Enum(enum_def));
        self.register_imported_nominal_type(
            InterfaceNominalKind::Enum,
            identity,
            name,
            ty,
            enum_def,
        );
        let decl_param_kinds = InterfaceGenericParam::kinds(&nominal.params);
        if !decl_param_kinds.is_empty() {
            self.register_generic_param_kinds(enum_def, decl_param_kinds.clone());
            let params = nominal
                .params
                .iter()
                .map(|param| interner.intern(&param.name))
                .collect();
            self.imported_enum_type_params.insert(enum_def, params);
        }
        // Phase 1 installs the enum nominal + variant shells (names, symbols,
        // parents) only. Payload fields are remapped in phase 2 once every
        // namespace's nominal shells exist: a variant field may reference a
        // nominal from a sibling module that sorts later in the interface map,
        // and eager remap would skip the whole export (WARN014) on an
        // otherwise-valid cross-module union (S1-U2: gradus `capsula.Manifesta`
        // carries `manifestum.ManifestumGguf`).
        let variants = nominal
            .variants
            .iter()
            .map(|variant| {
                let variant_def = self.fresh_def_id();
                let variant_name = interner.intern(&variant.name);
                self.symbols.insert(
                    variant_def,
                    Symbol {
                        def_id: variant_def,
                        name: variant_name,
                        kind: SymbolKind::Variant,
                        ty: Some(ty),
                        mutable: false,
                        span: Span::default(),
                    },
                );
                self.register_variant_parent(variant_def, enum_def);
                ImportedEnumVariant {
                    def_id: variant_def,
                    name: variant_name,
                    fields: Vec::new(),
                    payload_unresolved: false,
                }
            })
            .collect();
        self.imported_enum_variants.insert(enum_def, variants);
        self.symbols.insert(
            enum_def,
            Symbol {
                def_id: enum_def,
                name,
                kind: SymbolKind::Enum,
                ty: Some(ty),
                mutable: false,
                span: Span::default(),
            },
        );
        ImportedFileExport::Type(ImportedFileType {
            ty,
            def_id: Some(enum_def),
            methods: Vec::new(),
            decl_param_kinds,
        })
    }

    /// Remap enum variant payload fields (phase 2 of file import). Payload
    /// fields that fail consumer-side remap are omitted from the row, but the
    /// row itself always stays, flagged `payload_unresolved`: the commune
    /// projection arms every variant from these rows and its pattern binds
    /// only the shared field, so a dropped row would leave that variant to
    /// the runtime trap fallback, while `variant_child`
    /// excludes flagged rows so the package-MIR merge still fails closed on
    /// the unknown payload identity (council-11 `correct_before_next_phase`).
    fn import_enum_variants_partial(
        &mut self,
        enum_def: DefId,
        nominal: &InterfaceNominalExport,
        types: &mut TypeTable,
        interner: &mut radix_lexer::Interner,
    ) {
        let Some(existing) = self.imported_enum_variants.get(&enum_def).cloned() else {
            return;
        };
        let remapped = existing
            .iter()
            .zip(nominal.variants.iter())
            .map(|(existing_variant, snapshot_variant)| {
                let mut payload_unresolved = false;
                let fields = snapshot_variant
                    .fields
                    .iter()
                    .filter_map(|field| {
                        if let Ok((ty, init)) =
                            import_interface_type_with_resolver(&field.ty, types, interner, self)
                                .and_then(|ty| {
                                    let init = field
                                        .default
                                        .as_ref()
                                        .map(|default| default.import(types, interner, self))
                                        .transpose()?;
                                    Ok((ty, init))
                                })
                        {
                            Some(ImportedFileStructField {
                                name: interner.intern(&field.name),
                                ty,
                                optional: field.optional,
                                required: field.required,
                                modifier: field.modifier,
                                visibility: field.visibility,
                                init,
                            })
                        } else {
                            payload_unresolved = true;
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                ImportedEnumVariant {
                    def_id: existing_variant.def_id,
                    name: existing_variant.name,
                    fields,
                    payload_unresolved,
                }
            })
            .collect();
        self.imported_enum_variants.insert(enum_def, remapped);
        let commune = nominal
            .shared_fields
            .iter()
            .filter_map(|field| {
                import_interface_type_with_resolver(&field.ty, types, interner, self)
                    .ok()
                    .map(|ty| CommuneField {
                        name: interner.intern(&field.name),
                        ty,
                        span: Span::default(),
                    })
            })
            .collect();
        self.commune_fields.insert(enum_def, commune);
    }

    /// Import methods, skipping signatures that cannot be remapped.
    fn import_interface_methods_partial(
        &mut self,
        namespace: &str,
        methods: &[InterfaceMethodExport],
        owner: &str,
        identity: Option<&InterfaceLibraryIdentity>,
        locale_pack: Option<&LocalePack>,
        same_package: bool,
        types: &mut TypeTable,
        interner: &mut radix_lexer::Interner,
    ) -> (
        Vec<ImportedFileInterfaceMethod>,
        Vec<FileInterfaceSkippedExport>,
    ) {
        let mut imported = Vec::new();
        let mut skipped = Vec::new();
        for method in methods {
            let kind = InterfaceMemberKind::Method;
            if !same_package
                && !install_localized_member(
                    locale_pack,
                    identity,
                    Some(owner),
                    Some(kind),
                    &method.name,
                )
            {
                skipped.push(FileInterfaceSkippedExport::new(
                    namespace,
                    identity,
                    Some(owner),
                    kind,
                    &method.name,
                    skipped_member_surfaces(locale_pack, identity, Some(owner), kind, &method.name),
                    "strict_locale_pack_member_omitted",
                ));
                continue;
            }
            let projected_surfaces =
                projected_member_names(locale_pack, identity, Some(owner), kind, &method.name);
            let sig = match import_interface_callable_with_resolver(
                &method.callable,
                types,
                interner,
                self,
            ) {
                Ok(sig) => sig,
                Err(err) => {
                    skipped.push(FileInterfaceSkippedExport::from_error(
                        namespace,
                        identity,
                        Some(owner),
                        kind,
                        &method.name,
                        projected_surfaces,
                        &err,
                    ));
                    continue;
                }
            };
            let name = interner.intern(&method.name);
            let surface_name = interner.intern(
                projected_surfaces
                    .first()
                    .map_or(method.name.as_str(), String::as_str),
            );
            let axis: Vec<bool> = method
                .callable
                .params
                .iter()
                .map(|param| param.axis)
                .collect();
            imported.push(ImportedFileInterfaceMethod {
                def_id: self.fresh_def_id(),
                name,
                surface_name,
                sig,
                axis,
                visibility: method.visibility,
                unavailable: method.unavailable.clone(),
            });
        }
        (imported, skipped)
    }

    /// Install struct nominal identity without fields (phase 1 of file import).
    fn import_struct_file_export_shell(
        &mut self,
        strukt: &InterfaceStructExport,
        types: &mut TypeTable,
        interner: &mut radix_lexer::Interner,
        identity: Option<&InterfaceLibraryIdentity>,
    ) -> ImportedFileExport {
        let name = interner.intern(&strukt.name);
        let type_params: Vec<LexSymbol> = strukt
            .params
            .iter()
            .map(|param| interner.intern(&param.name))
            .collect();
        // Canonical duplicate-reuse: a second alias of the same module unifies
        // to one semantic type. Only for identity-bearing interfaces; without
        // identity the legacy name-keyed ambiguity guard must keep firing.
        if let Some((existing_ty, existing_def)) = identity.and_then(|_| {
            self.canonical_nominal_for(Some(InterfaceNominalKind::Struct), identity, name)
        }) {
            return ImportedFileExport::Struct(ImportedFileStruct {
                is_json: strukt.is_json,
                implements: Vec::new(),
                def_id: existing_def,
                ty: existing_ty,
                fields: Vec::new(),
                methods: Vec::new(),
                type_params,
            });
        }
        let def_id = self.fresh_def_id();
        let ty = types.intern(Type::Struct(def_id));
        self.register_imported_nominal_type(
            InterfaceNominalKind::Struct,
            identity,
            name,
            ty,
            def_id,
        );
        self.register_generic_param_kinds(
            def_id,
            crate::file_interface::InterfaceGenericParam::kinds(&strukt.params),
        );
        self.symbols.insert(
            def_id,
            Symbol {
                def_id,
                name,
                kind: SymbolKind::Struct,
                ty: Some(ty),
                mutable: false,
                span: Span::default(),
            },
        );
        ImportedFileExport::Struct(ImportedFileStruct {
            is_json: strukt.is_json,
            implements: Vec::new(),
            def_id,
            ty,
            fields: Vec::new(),
            methods: Vec::new(),
            type_params,
        })
    }

    /// Remap the contracts an exported struct names in `implet`.
    ///
    /// A contract that does not resolve to an interface in this file's tables
    /// is dropped, so the struct stays unassignable to it (fail closed).
    fn import_struct_implements(
        &self,
        strukt: &InterfaceStructExport,
        types: &mut TypeTable,
        interner: &mut radix_lexer::Interner,
    ) -> Vec<radix_types::ContractRef> {
        strukt
            .implements
            .iter()
            .filter_map(|contract| {
                crate::file_interface::import_contract_ref_with_resolver(
                    contract, types, interner, self,
                )
                .ok()
            })
            .collect()
    }

    /// Import struct fields, skipping any field whose type cannot be remapped.
    fn import_struct_fields_partial(
        &self,
        namespace: &str,
        strukt: &InterfaceStructExport,
        identity: Option<&InterfaceLibraryIdentity>,
        locale_pack: Option<&LocalePack>,
        types: &mut TypeTable,
        interner: &mut radix_lexer::Interner,
    ) -> (
        Vec<ImportedFileStructField>,
        Vec<FileInterfaceSkippedExport>,
    ) {
        let mut imported = Vec::new();
        let mut skipped = Vec::new();
        for field in &strukt.fields {
            let kind = InterfaceMemberKind::Field;
            // Struct fields are part of the nominal payload, not namespace
            // members. A strict consumer locale may omit field rows while
            // still importing the owning type (as the triga geometry pack
            // does), so field presence must not be gated by the member table.
            // Keep the remap below fail-closed: unsupported field types still
            // produce WARN014 and are omitted from the imported contract.
            let projected_surfaces = projected_member_names(
                locale_pack,
                identity,
                Some(&strukt.name),
                kind,
                &field.name,
            );
            let ty = match import_interface_type_with_resolver(&field.ty, types, interner, self) {
                Ok(ty) => ty,
                Err(err) => {
                    skipped.push(FileInterfaceSkippedExport::from_error(
                        namespace,
                        identity,
                        Some(&strukt.name),
                        kind,
                        &field.name,
                        projected_surfaces,
                        &err,
                    ));
                    continue;
                }
            };
            let init = match field
                .default
                .as_ref()
                .map(|default| default.import(types, interner, self))
                .transpose()
            {
                Ok(init) => init,
                Err(err) => {
                    skipped.push(FileInterfaceSkippedExport::from_error(
                        namespace,
                        identity,
                        Some(&strukt.name),
                        kind,
                        &field.name,
                        projected_surfaces,
                        &err,
                    ));
                    continue;
                }
            };
            imported.push(ImportedFileStructField {
                name: interner.intern(&field.name),
                ty,
                optional: field.optional,
                required: field.required,
                modifier: field.modifier,
                visibility: field.visibility,
                init,
            });
        }
        (imported, skipped)
    }

    /// Return whether a namespace member is exportable, or unrestricted when no
    /// table exists. `@ interna` is importable only for same-package consumers;
    /// module-private names never are.
    #[must_use]
    pub fn namespace_exports_member(&self, namespace: LexSymbol, member: LexSymbol) -> bool {
        if self
            .namespace_member_rejection_issue(namespace, member)
            .is_some()
        {
            return false;
        }
        if let Some(interface) = self
            .file_interfaces
            .get(&namespace)
            .filter(|interface| !interface.exports.is_empty())
        {
            if Self::file_interface_export_key(interface, member).is_some() {
                return true;
            }
            return self
                .namespace_exports
                .get(&namespace)
                .is_some_and(|members| members.contains_key(&member));
        }
        self.namespace_exports
            .get(&namespace)
            .is_none_or(|members| members.contains_key(&member))
    }

    #[must_use]
    pub fn namespace_has_file_interface(&self, namespace: LexSymbol) -> bool {
        self.file_interfaces.contains_key(&namespace)
    }

    /// Record that a selective import binding names an imported type export.
    pub fn bind_selective_type_import(
        &mut self,
        binding: DefId,
        path: LexSymbol,
        member: LexSymbol,
    ) {
        self.selective_type_imports.insert(binding, (path, member));
    }

    /// The imported type a selective import binding names, if it binds one.
    #[must_use]
    pub fn selective_import_type(&self, binding: DefId) -> Option<TypeId> {
        let (path, member) = self.selective_type_imports.get(&binding)?;
        self.namespace_file_type(*path, *member)
    }

    /// The imported type the visible name `name` binds through a selective
    /// import, if the nearest binding of `name` is one.
    #[must_use]
    pub fn lookup_selective_import_type(&self, name: LexSymbol) -> Option<TypeId> {
        self.selective_import_type(self.lookup(name)?)
    }

    /// Declaration parameter kinds of the imported type the visible name
    /// `name` binds through a selective import.
    #[must_use]
    pub fn lookup_selective_import_decl_param_kinds(
        &self,
        name: LexSymbol,
    ) -> Option<Vec<radix_syntax::GenericParamKind>> {
        let (path, member) = self.selective_type_imports.get(&self.lookup(name)?)?;
        self.namespace_file_decl_param_kinds(*path, *member)
    }

    /// Return the remapped function signature for an imported file namespace member.
    ///
    /// `@ interna` signatures are offered only when the import-seam policy
    /// allows the reference (same package).
    #[must_use]
    pub fn namespace_file_function(
        &self,
        namespace: LexSymbol,
        member: LexSymbol,
    ) -> Option<&FuncSig> {
        if self
            .namespace_member_rejection_issue(namespace, member)
            .is_some()
        {
            return None;
        }
        let interface = self.file_interfaces.get(&namespace)?;
        match interface
            .exports
            .get(&Self::file_interface_export_key(interface, member)?)?
        {
            ImportedFileExport::Function(sig, _) => Some(sig),
            ImportedFileExport::Type(_) | ImportedFileExport::Struct(_) => None,
        }
    }

    // Post-HIR canonical member lookups.
    //
    // HIR records canonical library export names (locale surfaces exist only
    // at the source boundary), while the installed export map is keyed by the
    // consumer-locale surface spelling. These lookups resolve a canonical name
    // through the reverse projection and do not re-apply the source-boundary
    // strict rule — the resolve pass rejects non-listed spellings before HIR
    // lowering, so the typechecker sees only names projected from valid
    // surfaces. The source-boundary API (`namespace_file_function` et al.)
    // keeps its strict behavior for pre-HIR callers.

    /// Surface export key that carries a post-HIR canonical member name.
    fn file_interface_export_key_for_canonical(
        interface: &ImportedFileInterface,
        canonical: LexSymbol,
    ) -> Option<LexSymbol> {
        let surface = interface
            .canonical_exports
            .get(&canonical)
            .copied()
            .unwrap_or(canonical);
        interface.exports.contains_key(&surface).then_some(surface)
    }

    /// Whether a post-HIR canonical member name is an installed file-interface
    /// member of `namespace` (type, function, or const).
    #[must_use]
    pub fn namespace_exports_member_canonical(
        &self,
        namespace: LexSymbol,
        canonical: LexSymbol,
    ) -> bool {
        if self
            .namespace_member_rejection_issue(namespace, canonical)
            .is_some()
        {
            return false;
        }
        if let Some(interface) = self
            .file_interfaces
            .get(&namespace)
            .filter(|interface| !interface.exports.is_empty())
        {
            if Self::file_interface_export_key_for_canonical(interface, canonical).is_some() {
                return true;
            }
            return self
                .namespace_exports
                .get(&namespace)
                .is_some_and(|members| members.contains_key(&canonical));
        }
        self.namespace_exports
            .get(&namespace)
            .is_none_or(|members| members.contains_key(&canonical))
    }

    /// Remapped function signature for a post-HIR canonical file-interface member.
    #[must_use]
    pub fn namespace_file_function_canonical(
        &self,
        namespace: LexSymbol,
        canonical: LexSymbol,
    ) -> Option<&FuncSig> {
        if self
            .namespace_member_rejection_issue(namespace, canonical)
            .is_some()
        {
            return None;
        }
        let interface = self.file_interfaces.get(&namespace)?;
        match interface
            .exports
            .get(&Self::file_interface_export_key_for_canonical(
                interface, canonical,
            )?)? {
            ImportedFileExport::Function(sig, _) => Some(sig),
            ImportedFileExport::Type(_) | ImportedFileExport::Struct(_) => None,
        }
    }

    /// Declared `axis` markers of the imported function a post-HIR canonical
    /// member names, in parameter order. `None` when the member is not an
    /// imported function; a shorter vector than the signature's parameter
    /// count is treated as all-false by the call checker.
    #[must_use]
    pub fn namespace_file_axis_params_canonical(
        &self,
        namespace: LexSymbol,
        canonical: LexSymbol,
    ) -> Option<&[bool]> {
        if self
            .namespace_member_rejection_issue(namespace, canonical)
            .is_some()
        {
            return None;
        }
        let interface = self.file_interfaces.get(&namespace)?;
        match interface
            .exports
            .get(&Self::file_interface_export_key_for_canonical(
                interface, canonical,
            )?)? {
            ImportedFileExport::Function(_, axis) => Some(axis),
            ImportedFileExport::Type(_) | ImportedFileExport::Struct(_) => None,
        }
    }

    /// `@ nondum` gates of the imported function a source-spelled namespace
    /// member names; `None` when the member is unmarked or not a function.
    #[must_use]
    pub fn namespace_file_unavailable(
        &self,
        namespace: LexSymbol,
        member: LexSymbol,
    ) -> Option<&[InterfaceUnavailable]> {
        let interface = self.file_interfaces.get(&namespace)?;
        let key = Self::file_interface_export_key(interface, member)?;
        interface.unavailable.get(&key).map(Vec::as_slice)
    }

    /// Device (`@ nucleum`) contract of the imported function a post-HIR
    /// canonical member name resolves to; `None` for an ordinary function or
    /// a member that is not a function.
    #[must_use]
    pub fn namespace_file_device_contract_canonical(
        &self,
        namespace: LexSymbol,
        canonical: LexSymbol,
    ) -> Option<&InterfaceDeviceContract> {
        if self
            .namespace_member_rejection_issue(namespace, canonical)
            .is_some()
        {
            return None;
        }
        let interface = self.file_interfaces.get(&namespace)?;
        let key = Self::file_interface_export_key_for_canonical(interface, canonical)?;
        interface.device_contracts.get(&key)
    }

    /// `@ nondum` gates of the imported function a post-HIR canonical member
    /// name resolves to; `None` when the member is unmarked or not a function.
    #[must_use]
    pub fn namespace_file_unavailable_canonical(
        &self,
        namespace: LexSymbol,
        canonical: LexSymbol,
    ) -> Option<&[InterfaceUnavailable]> {
        let interface = self.file_interfaces.get(&namespace)?;
        let key = Self::file_interface_export_key_for_canonical(interface, canonical)?;
        interface.unavailable.get(&key).map(Vec::as_slice)
    }

    /// Record that a selective import binding names an `@ nondum` function.
    pub fn bind_unavailable_import(&mut self, binding: DefId, gates: Vec<InterfaceUnavailable>) {
        self.unavailable_bindings.insert(binding, gates);
    }

    /// `@ nondum` gates of the function a selective import binding names.
    #[must_use]
    pub fn binding_unavailable(&self, binding: DefId) -> Option<&[InterfaceUnavailable]> {
        self.unavailable_bindings.get(&binding).map(Vec::as_slice)
    }

    /// Whether any selective import binding names an `@ nondum` function.
    #[must_use]
    pub fn has_unavailable_bindings(&self) -> bool {
        !self.unavailable_bindings.is_empty()
    }

    /// Edge-policy obligations exported by a generic namespace function.
    ///
    /// The returned identity is a resolver-local key, never a producer
    /// `DefId`; requirement payload types have already been remapped into the
    /// consumer's arena.
    #[must_use]
    pub fn namespace_file_tensor_edge_policy_requirements_canonical(
        &self,
        namespace: LexSymbol,
        canonical: LexSymbol,
    ) -> Option<(DefId, &[TensorEdgePolicyRequirement])> {
        if self
            .namespace_member_rejection_issue(namespace, canonical)
            .is_some()
        {
            return None;
        }
        let interface = self.file_interfaces.get(&namespace)?;
        let member = Self::file_interface_export_key_for_canonical(interface, canonical)?;
        let (def_id, requirements) = interface.tensor_edge_policy_requirements.get(&member)?;
        Some((*def_id, requirements))
    }

    /// Remapped value type for a post-HIR canonical file-interface CONST member.
    #[must_use]
    pub fn namespace_file_const_canonical(
        &self,
        namespace: LexSymbol,
        canonical: LexSymbol,
    ) -> Option<TypeId> {
        let interface = self.file_interfaces.get(&namespace)?;
        let member = Self::file_interface_export_key_for_canonical(interface, canonical)?;
        if !interface.const_members.contains(&member) {
            return None;
        }
        match interface.exports.get(&member)? {
            ImportedFileExport::Type(ty) => Some(ty.ty),
            ImportedFileExport::Struct(_) | ImportedFileExport::Function(..) => None,
        }
    }

    /// Whether the source spelling `member` is accepted at the source boundary
    /// for `namespace`'s installed file interface.
    ///
    /// A strict (identity-defining, non-`la`) pack is total for that library:
    /// only its listed surface spellings resolve, even though post-HIR
    /// canonical lookups could resolve the installed member. Identity and
    /// non-strict surfaces accept the names as written.
    #[must_use]
    pub fn namespace_member_surface_valid(&self, namespace: LexSymbol, member: LexSymbol) -> bool {
        match self.file_interfaces.get(&namespace) {
            Some(interface) if interface.strict_members => interface.exports.contains_key(&member),
            Some(_) | None => true,
        }
    }

    /// Visibility tier of an imported namespace member, if present.
    #[must_use]
    pub fn namespace_export_tier(
        &self,
        namespace: LexSymbol,
        member: LexSymbol,
    ) -> Option<VisibilityTier> {
        self.namespace_exports
            .get(&namespace)?
            .get(&member)
            .copied()
    }

    /// Effective declaration tier, including module-private names absent from
    /// the export surface.
    #[must_use]
    pub fn namespace_declaration_tier(
        &self,
        namespace: LexSymbol,
        member: LexSymbol,
    ) -> Option<VisibilityTier> {
        self.namespace_declarations
            .get(&namespace)?
            .get(&member)
            .copied()
            .or_else(|| self.namespace_export_tier(namespace, member))
    }

    /// Same-package flag for `namespace`. Missing bindings are cross-package.
    #[must_use]
    pub fn namespace_same_package(&self, namespace: LexSymbol) -> bool {
        self.namespace_seams
            .get(&namespace)
            .copied()
            .unwrap_or(false)
    }

    /// Structured import-seam issue when `member` exists but is not importable.
    #[must_use]
    pub fn namespace_member_rejection_issue(
        &self,
        namespace: LexSymbol,
        member: LexSymbol,
    ) -> Option<&'static str> {
        let seam = if self.namespace_same_package(namespace) {
            crate::passes::import_seam::ImportSeam::SamePackage
        } else {
            crate::passes::import_seam::ImportSeam::CrossPackage
        };
        crate::passes::import_seam::violation_issue(
            self.namespace_declaration_tier(namespace, member),
            seam,
        )
    }

    fn file_interface_export_key(
        interface: &ImportedFileInterface,
        member: LexSymbol,
    ) -> Option<LexSymbol> {
        if interface.exports.contains_key(&member) {
            return Some(member);
        }
        if interface.strict_members {
            return None;
        }
        interface.canonical_exports.get(&member).copied()
    }

    /// All member names exported by an installed file interface (types + functions).
    #[must_use]
    pub fn namespace_file_export_names(&self, namespace: LexSymbol) -> Vec<LexSymbol> {
        self.file_interfaces
            .get(&namespace)
            .map(|interface| interface.exports.keys().copied().collect())
            .unwrap_or_default()
    }

    /// Function exports installed for a file namespace binding.
    #[must_use]
    pub fn namespace_file_function_exports(
        &self,
        namespace: LexSymbol,
    ) -> Vec<(LexSymbol, &FuncSig)> {
        self.file_interfaces
            .get(&namespace)
            .into_iter()
            .flat_map(|interface| interface.exports.iter())
            .filter_map(|(member, export)| match export {
                ImportedFileExport::Function(sig, _) => Some((*member, sig)),
                ImportedFileExport::Type(_) | ImportedFileExport::Struct(_) => None,
            })
            .collect()
    }

    /// Return the remapped type payload for an imported file namespace member.
    ///
    /// `@ interna` type payloads are offered only when the import-seam policy
    /// allows the reference (same package).
    #[must_use]
    pub fn namespace_file_type(&self, namespace: LexSymbol, member: LexSymbol) -> Option<TypeId> {
        if self
            .namespace_member_rejection_issue(namespace, member)
            .is_some()
        {
            return None;
        }
        let interface = self.file_interfaces.get(&namespace)?;
        match interface
            .exports
            .get(&Self::file_interface_export_key(interface, member)?)?
        {
            ImportedFileExport::Type(ty) => Some(ty.ty),
            ImportedFileExport::Struct(strukt) => Some(strukt.ty),
            ImportedFileExport::Function(..) => None,
        }
    }

    /// Ordered declaration parameter kinds for an imported file namespace
    /// type member (`module.Type`), from the interface's `{name, kind}` export
    /// metadata (S0-R1). `None` when the member is not a type export.
    #[must_use]
    pub fn namespace_file_decl_param_kinds(
        &self,
        namespace: LexSymbol,
        member: LexSymbol,
    ) -> Option<Vec<radix_syntax::GenericParamKind>> {
        if self
            .namespace_member_rejection_issue(namespace, member)
            .is_some()
        {
            return None;
        }
        let interface = self.file_interfaces.get(&namespace)?;
        match interface
            .exports
            .get(&Self::file_interface_export_key(interface, member)?)?
        {
            ImportedFileExport::Type(ty) => Some(ty.decl_param_kinds.clone()),
            ImportedFileExport::Struct(strukt) => self
                .generic_param_kinds(strukt.def_id)
                .map(ToOwned::to_owned),
            ImportedFileExport::Function(..) => None,
        }
    }

    /// Return the remapped value type for an imported file namespace CONST
    /// data member (`module.CONST` value reference). Only consts are
    /// first-class value references; type exports (aliases/structs/interfaces)
    /// and functions resolve to `None` so they keep their existing type-check
    /// failure modes.
    #[must_use]
    pub fn namespace_file_const(&self, namespace: LexSymbol, member: LexSymbol) -> Option<TypeId> {
        let interface = self.file_interfaces.get(&namespace)?;
        let member = Self::file_interface_export_key(interface, member)?;
        if !interface.const_members.contains(&member) {
            return None;
        }
        match interface.exports.get(&member)? {
            ImportedFileExport::Type(ty) => Some(ty.ty),
            ImportedFileExport::Struct(_) | ImportedFileExport::Function(..) => None,
        }
    }

    /// Folded compile-time value of a post-HIR canonical file-interface CONST
    /// member, when its exporter folded one (ST2-1). `None` for a const whose
    /// initializer did not fold, and for every non-const member.
    #[must_use]
    pub fn namespace_file_const_value_canonical(
        &self,
        namespace: LexSymbol,
        canonical: LexSymbol,
    ) -> Option<&InterfaceConstValue> {
        let interface = self.file_interfaces.get(&namespace)?;
        let member = Self::file_interface_export_key_for_canonical(interface, canonical)?;
        interface.const_values.get(&member)
    }

    /// Return qualified source spellings for every imported nominal export,
    /// keyed both by type id and by definition id, so HIR backends can print
    /// imported types in type positions and struct-literal expressions.
    #[must_use]
    pub fn imported_spellings(&self, interner: &radix_lexer::Interner) -> ImportedSpellings {
        let mut by_type = FxHashMap::default();
        let mut by_def = FxHashMap::default();
        let mut by_def_pattern = FxHashMap::default();
        for (namespace, interface) in &self.file_interfaces {
            let namespace = interner.resolve(*namespace);
            for (member, export) in &interface.exports {
                let member = interner.resolve(*member);
                let spelling = format!("{namespace}.{member}");
                match export {
                    ImportedFileExport::Type(export) => {
                        by_type.insert(export.ty, spelling.clone());
                        if let Some(def_id) = export.def_id {
                            by_def.insert(def_id, spelling);
                        }
                        // Union/enum variants are first-class path values and
                        // pattern arms (`module.VARIANT`); give the emitter
                        // their qualified spelling so convert re-emits them
                        // instead of the `def_N` fallback. Two-part only:
                        // the three-part `module.Union.VARIANT` spelling is
                        // not accepted in expression/call position, so it
                        // cannot ride this shared map.
                        if let Some(enum_def) = export.def_id
                            && let Some(variants) = self.imported_enum_variants.get(&enum_def)
                        {
                            for variant in variants {
                                let variant_name = interner.resolve(variant.name);
                                by_def
                                    .insert(variant.def_id, format!("{namespace}.{variant_name}"));
                                by_def_pattern.insert(
                                    variant.def_id,
                                    format!("{namespace}.{member}.{variant_name}"),
                                );
                            }
                        }
                    }
                    ImportedFileExport::Struct(export) => {
                        by_type.insert(export.ty, spelling.clone());
                        by_def.insert(export.def_id, spelling);
                    }
                    ImportedFileExport::Function(..) => {}
                }
            }
        }
        ImportedSpellings {
            by_type,
            by_def,
            by_def_pattern,
        }
    }

    /// Return source spellings for compiler-generated backward companion
    /// functions registered during analysis (the `0x8000_0000` `DefId` family),
    /// keyed by definition id. The companion names are registered in the
    /// resolver before resolve but never appear as HIR definitions, so the
    /// emitter's DefId→name table cannot recover them; carrying the spelling
    /// here lets backends print companion call targets instead of the `def_N`
    /// fallback.
    #[must_use]
    pub fn generated_companion_spellings(
        &self,
        interner: &radix_lexer::Interner,
    ) -> FxHashMap<DefId, String> {
        let mut spellings = FxHashMap::default();
        for (def_id, symbol) in &self.symbols {
            if def_id.is_companion() {
                spellings.insert(*def_id, interner.resolve(symbol.name).to_owned());
            }
        }
        spellings
    }

    #[must_use]
    pub fn imported_nominal_type(
        &self,
        kind: InterfaceNominalKind,
        name: LexSymbol,
    ) -> Option<TypeId> {
        self.imported_nominal_types.get(&(kind, name)).copied()
    }

    /// Whether a nested nominal snapshot cannot be resolved by name alone.
    #[must_use]
    pub fn imported_nominal_type_is_ambiguous(
        &self,
        kind: InterfaceNominalKind,
        name: LexSymbol,
    ) -> bool {
        self.ambiguous_imported_nominal_types
            .contains(&(kind, name))
    }

    /// Resolve an imported nominal through its canonical module-qualified key:
    /// same module identity + kind + name → the unified consumer `TypeId`.
    /// Distinct module identities stay distinct even when their short names
    /// coincide.
    #[must_use]
    pub fn imported_nominal_type_by_identity(
        &self,
        kind: InterfaceNominalKind,
        identity: Option<&InterfaceLibraryIdentity>,
        name: LexSymbol,
    ) -> Option<TypeId> {
        self.canonical_imported_nominal_types
            .get(&ImportedNominalKey {
                kind,
                identity: identity.cloned(),
                name,
            })
            .copied()
    }

    /// Consumer `DefId` for a canonical nominal key (the unified import), so
    /// the merge-time remap can rewrite source nominal/variant definitions to
    /// the entry's identities.
    #[must_use]
    pub fn imported_nominal_def_id(
        &self,
        kind: InterfaceNominalKind,
        identity: Option<&InterfaceLibraryIdentity>,
        name: LexSymbol,
    ) -> Option<DefId> {
        self.canonical_imported_nominal_defs
            .get(&ImportedNominalKey {
                kind,
                identity: identity.cloned(),
                name,
            })
            .copied()
    }

    /// Consumer `DefId` for a canonical nominal key resolved by name TEXT.
    ///
    /// The package-MIR method-call linker holds only an immutable interner
    /// when it maps a linked library's nominal to the caller's entry-side
    /// `DefId`, so this variant resolves the key's symbol through `interner`
    /// instead of requiring a fresh `intern`.
    #[must_use]
    pub fn imported_nominal_def_id_by_name(
        &self,
        kind: InterfaceNominalKind,
        identity: Option<&InterfaceLibraryIdentity>,
        name: &str,
        interner: &radix_lexer::Interner,
    ) -> Option<DefId> {
        self.canonical_imported_nominal_defs
            .iter()
            .find_map(|(key, def_id)| {
                (key.kind == kind
                    && key.identity.as_ref() == identity
                    && interner.resolve(key.name) == name)
                    .then_some(*def_id)
            })
    }

    /// Whether this analysis has installed any canonical nominal under
    /// `identity` — i.e. the consumer imported (at least one nominal of) the
    /// home module. The package-MIR merge uses this to distinguish a module
    /// the consumer genuinely does not know (source-local defs legitimately
    /// stay) from a known module whose specific export is missing (fail-closed
    /// unknown-identity error).
    #[must_use]
    pub fn canonical_identity_known(&self, identity: &InterfaceLibraryIdentity) -> bool {
        self.canonical_imported_nominal_types
            .keys()
            .any(|key| key.identity.as_ref() == Some(identity))
    }

    fn canonical_nominal_for(
        &self,
        kind: Option<InterfaceNominalKind>,
        identity: Option<&InterfaceLibraryIdentity>,
        name: LexSymbol,
    ) -> Option<(TypeId, DefId)> {
        let kind = kind?;
        let key = ImportedNominalKey {
            kind,
            identity: identity.cloned(),
            name,
        };
        let ty = self.canonical_imported_nominal_types.get(&key).copied()?;
        let def_id = self.canonical_imported_nominal_defs.get(&key).copied()?;
        Some((ty, def_id))
    }

    /// The canonical module-qualified identity of the module that exports the
    /// nominal with `def_id`, when it is an imported nominal in this analysis
    /// (used by the snapshot side to attach home-module identity to nested
    /// nominal snapshots).
    #[must_use]
    pub fn nominal_module_identity(&self, def_id: DefId) -> Option<InterfaceLibraryIdentity> {
        self.file_interfaces.values().find_map(|interface| {
            interface
                .exports
                .values()
                .any(|export| export_contains_def_id(export, def_id))
                .then(|| interface.identity.clone())
                .flatten()
        })
    }

    fn register_imported_nominal_type(
        &mut self,
        kind: InterfaceNominalKind,
        identity: Option<&InterfaceLibraryIdentity>,
        name: LexSymbol,
        ty: TypeId,
        def_id: DefId,
    ) {
        self.note_imported_type_def(def_id);
        // Legacy name-keyed table (snapshot-serialized resolution surface).
        // Preserves the historical collision guard: same short name from two
        // different modules marks the name-keyed entry ambiguous.
        let key = (kind, name);
        if !self.ambiguous_imported_nominal_types.contains(&key) {
            if self
                .imported_nominal_types
                .get(&key)
                .is_some_and(|existing| *existing != ty)
            {
                self.imported_nominal_types.remove(&key);
                self.ambiguous_imported_nominal_types.insert(key);
            } else {
                self.imported_nominal_types.insert(key, ty);
            }
        }
        // Canonical module-qualified table (runtime-only unification surface):
        // independent of the name-keyed ambiguity — different module
        // identities are distinct keys, so `util.Foo` and `other.Foo` both
        // register canonically while the legacy name key stays ambiguous.
        let canonical_key = ImportedNominalKey {
            kind,
            identity: identity.cloned(),
            name,
        };
        self.canonical_imported_nominal_types
            .insert(canonical_key.clone(), ty);
        self.canonical_imported_nominal_defs
            .insert(canonical_key, def_id);
    }

    pub fn imported_json_structs(&self) -> impl Iterator<Item = DefId> + '_ {
        self.file_interfaces
            .values()
            .flat_map(|interface| interface.exports.values())
            .filter_map(|export| match export {
                ImportedFileExport::Struct(strukt) if strukt.is_json => Some(strukt.def_id),
                _ => None,
            })
    }

    /// The declared `implet` contracts of every imported struct that names
    /// any, keyed by the struct's consumer-local identity.
    #[must_use]
    pub fn imported_struct_contracts(&self) -> Vec<(DefId, Vec<radix_types::ContractRef>)> {
        self.file_interfaces
            .values()
            .flat_map(|interface| interface.exports.values())
            .filter_map(|export| match export {
                ImportedFileExport::Struct(strukt) if !strukt.implements.is_empty() => {
                    Some((strukt.def_id, strukt.implements.clone()))
                }
                _ => None,
            })
            .collect()
    }

    /// Flatten one imported struct export into the entry shape shared by the
    /// `imported_file_structs*` readers.
    fn imported_struct_entry(strukt: &ImportedFileStruct) -> ImportedFileStructEntry {
        (
            strukt.def_id,
            strukt.fields.clone(),
            strukt.methods.clone(),
            strukt.type_params.clone(),
        )
    }

    #[must_use]
    pub fn imported_file_structs(&self) -> Vec<ImportedFileStructEntry> {
        self.file_interfaces
            .values()
            .flat_map(|interface| interface.exports.values())
            .filter_map(|export| match export {
                ImportedFileExport::Struct(strukt) => Some(Self::imported_struct_entry(strukt)),
                ImportedFileExport::Function(..) | ImportedFileExport::Type(_) => None,
            })
            .collect()
    }

    /// Imported callback definitions and their file-namespace route.
    ///
    /// Each callback `DefId` is local to this resolver. The tuple keeps the
    /// owner nominal and imported module binding beside the exact exported
    /// method name so consumers can route the identity without borrowing a
    /// producer arena or guessing from a fallback spelling.
    #[must_use]
    pub fn imported_file_method_identities(
        &self,
    ) -> Vec<(DefId, DefId, LexSymbol, LexSymbol, LexSymbol)> {
        self.file_interfaces
            .iter()
            .flat_map(|(namespace, interface)| {
                interface
                    .exports
                    .iter()
                    .flat_map(move |(owner_name, export)| {
                        let ImportedFileExport::Struct(strukt) = export else {
                            return Vec::new();
                        };
                        strukt
                            .methods
                            .iter()
                            .map(|method| {
                                (
                                    method.def_id,
                                    strukt.def_id,
                                    *namespace,
                                    *owner_name,
                                    method.name,
                                )
                            })
                            .collect::<Vec<_>>()
                    })
            })
            .collect()
    }

    /// Export name of each imported `genus` and `discretio` (the display
    /// name `print` renders as a construction literal, D2.8).
    #[must_use]
    pub fn imported_file_nominal_names(&self) -> Vec<(DefId, LexSymbol)> {
        self.file_interfaces
            .values()
            .flat_map(|interface| interface.exports.iter())
            .filter_map(|(name, export)| match export {
                ImportedFileExport::Struct(strukt) => Some((strukt.def_id, *name)),
                ImportedFileExport::Type(ty) => ty.def_id.map(|def_id| (def_id, *name)),
                ImportedFileExport::Function(..) => None,
            })
            .collect()
    }

    /// Imported `genus` declarations reached through a cross-package
    /// namespace binding: their `@ interna` members are unreachable (D5.3).
    #[must_use]
    pub fn imported_cross_package_structs(&self) -> Vec<DefId> {
        self.file_interfaces
            .iter()
            .filter(|(namespace, _)| !self.namespace_same_package(**namespace))
            .flat_map(|(_, interface)| interface.exports.values())
            .filter_map(|export| match export {
                ImportedFileExport::Struct(strukt) => Some(strukt.def_id),
                ImportedFileExport::Function(..) | ImportedFileExport::Type(_) => None,
            })
            .collect()
    }

    #[must_use]
    pub fn imported_file_structs_for_namespace(
        &self,
        namespace: LexSymbol,
    ) -> Vec<ImportedFileStructEntry> {
        self.file_interfaces
            .get(&namespace)
            .into_iter()
            .flat_map(|interface| interface.exports.values())
            .filter_map(|export| match export {
                ImportedFileExport::Struct(strukt) => Some(Self::imported_struct_entry(strukt)),
                ImportedFileExport::Function(..) | ImportedFileExport::Type(_) => None,
            })
            .collect()
    }

    /// Declaration parameter names of an imported implendum (D1.2).
    #[must_use]
    pub fn imported_contract_type_params(&self, def_id: DefId) -> Option<&[LexSymbol]> {
        self.imported_contract_type_params
            .get(&def_id)
            .map(Vec::as_slice)
    }

    /// Declaration parameter names of an imported generic enum.
    #[must_use]
    pub fn imported_enum_type_params(&self, enum_def: DefId) -> Option<&[LexSymbol]> {
        self.imported_enum_type_params
            .get(&enum_def)
            .map(Vec::as_slice)
    }

    /// `@ radix contract` role of an imported implendum (D1.4).
    #[must_use]
    pub fn imported_contract_role(&self, def_id: DefId) -> Option<&str> {
        self.imported_contract_roles
            .get(&def_id)
            .map(String::as_str)
    }

    #[must_use]
    pub fn imported_file_interfaces(&self) -> Vec<(DefId, Vec<ImportedFileInterfaceMethod>)> {
        self.file_interfaces
            .values()
            .flat_map(|interface| interface.exports.values())
            .filter_map(|export| match export {
                ImportedFileExport::Type(ty) => Some((ty.def_id?, ty.methods.clone())),
                ImportedFileExport::Function(..) | ImportedFileExport::Struct(_) => None,
            })
            .filter(|(_, methods)| !methods.is_empty())
            .collect()
    }

    /// Record `@ commune` fields for a union definition.
    pub fn register_commune_fields(&mut self, enum_def: DefId, fields: Vec<CommuneField>) {
        self.commune_fields.insert(enum_def, fields);
    }

    /// Commune fields declared on `enum_def`, empty when the union has no region.
    #[must_use]
    pub fn commune_fields(&self, enum_def: DefId) -> &[CommuneField] {
        self.commune_fields
            .get(&enum_def)
            .map_or(&[], Vec::as_slice)
    }

    /// Lookup one commune field by name.
    #[must_use]
    pub fn commune_field(&self, enum_def: DefId, name: LexSymbol) -> Option<CommuneField> {
        self.commune_fields(enum_def)
            .iter()
            .copied()
            .find(|field| field.name == name)
    }

    /// Enroll one compiler-shipped builtin row in the coherence registry.
    ///
    /// Builtin rows occupy the global scope, are visible from every scope, and
    /// reject any later user row for the same ordered pair. Registering the
    /// same builtin pair twice is a duplicate conflict carrying both spans.
    ///
    /// # Errors
    ///
    /// Returns [`ConversionRowConflict::Duplicate`] when a builtin row for the
    /// pair is already enrolled.
    pub fn register_builtin_conversion(
        &mut self,
        pair: ConversionPair,
        span: Span,
    ) -> Result<(), ConversionRowConflict> {
        let rows = self.conversion_rows.entry(pair).or_default();
        if let Some(previous) = rows.iter().find(|row| row.builtin) {
            return Err(ConversionRowConflict::Duplicate {
                first_span: previous.span,
                second_span: span,
            });
        }
        rows.push(ConversionRow {
            pair,
            fn_def_id: None,
            span,
            scope: ScopeId(0),
            builtin: true,
        });
        Ok(())
    }

    /// Enroll one user row in the coherence registry, in the current scope.
    ///
    /// Fails closed when the pair is already enrolled in the same scope
    /// (duplicate, both spans) or owned by a builtin row in any scope
    /// (non-shadowing, builtin + user spans). At most one row per pair per
    /// scope.
    ///
    /// # Errors
    ///
    /// Returns [`ConversionRowConflict::BuiltinShadow`] when a builtin row owns
    /// the pair, or [`ConversionRowConflict::Duplicate`] when the current scope
    /// already holds a row for it.
    pub fn register_user_conversion(
        &mut self,
        pair: ConversionPair,
        fn_def_id: DefId,
        span: Span,
    ) -> Result<(), ConversionRowConflict> {
        let scope = self.current_scope;
        let rows = self.conversion_rows.entry(pair).or_default();
        if let Some(builtin) = rows.iter().find(|row| row.builtin) {
            return Err(ConversionRowConflict::BuiltinShadow {
                builtin_span: builtin.span,
                user_span: span,
            });
        }
        if let Some(previous) = rows.iter().find(|row| row.scope == scope) {
            return Err(ConversionRowConflict::Duplicate {
                first_span: previous.span,
                second_span: span,
            });
        }
        rows.push(ConversionRow {
            pair,
            fn_def_id: Some(fn_def_id),
            span,
            scope,
            builtin: false,
        });
        Ok(())
    }

    /// Direct, scope-aware row lookup for one ordered pair.
    ///
    /// Walks the current scope chain toward global and returns the first row
    /// enrolled in a visible scope. Lookup is direct only — no transitive
    /// composition; a miss is a miss.
    #[must_use]
    pub fn conversion_row_for(&self, source: TypeId, destination: TypeId) -> Option<ConversionRow> {
        let pair = ConversionPair::new(source, destination);
        let rows = self.conversion_rows.get(&pair)?;
        let mut scope = Some(self.current_scope);
        while let Some(scope_id) = scope {
            if let Some(row) = rows.iter().find(|row| row.scope == scope_id) {
                return Some(*row);
            }
            scope = self.scopes[scope_id.0 as usize].parent;
        }
        None
    }

    /// All enrolled rows for one ordered pair, builtin and user alike.
    #[must_use]
    pub fn conversion_rows_for(&self, pair: ConversionPair) -> &[ConversionRow] {
        self.conversion_rows.get(&pair).map_or(&[], Vec::as_slice)
    }

    /// Every enrolled row, builtin and user alike, in enrollment order.
    ///
    /// Whole-registry consumers (MIR lowering's direct-only conversion
    /// recovery) flatten scope visibility deliberately: production analysis
    /// enrolls all rows in the global scope, so the flat walk and
    /// [`Self::conversion_row_for`] agree on the live unit.
    pub fn conversion_rows(&self) -> impl Iterator<Item = ConversionRow> + '_ {
        self.conversion_rows.values().flatten().copied()
    }

    /// Record a nominal type definition as imported (foreign to this unit).
    pub(crate) fn note_imported_type_def(&mut self, def_id: DefId) {
        self.imported_type_def_ids.insert(def_id);
    }

    /// Whether a nominal type definition arrived from an imported file
    /// interface (foreign to this compilation unit).
    #[must_use]
    pub fn type_def_is_foreign(&self, def_id: DefId) -> bool {
        self.imported_type_def_ids.contains(&def_id)
    }

    /// Named-helper suggestion for a pair with no visible row.
    ///
    /// Direct lookup is the only selection mechanism, but a pair enrolled in a
    /// non-visible scope still names a callable the author can select
    /// explicitly. Returns the function's declared name symbol when such a
    /// row exists; the caller renders it through its interner.
    #[must_use]
    pub fn conversion_name_suggestion(
        &self,
        source: TypeId,
        destination: TypeId,
    ) -> Option<LexSymbol> {
        let pair = ConversionPair::new(source, destination);
        self.conversion_rows
            .get(&pair)?
            .iter()
            .find_map(|row| row.fn_def_id)
            .and_then(|def_id| self.symbols.get(&def_id))
            .map(|symbol| symbol.name)
    }

    /// Consumer-side imported enum declarations with their variant metadata.
    /// Enums install as `Type` exports; the variant metadata lives in the
    /// resolver's `imported_enum_variants` table keyed by the consumer enum
    /// `DefId`.
    #[must_use]
    pub fn imported_file_enums(&self) -> Vec<(DefId, TypeId, Vec<ImportedEnumVariant>)> {
        self.file_interfaces
            .values()
            .flat_map(|interface| interface.exports.values())
            .filter_map(|export| match export {
                ImportedFileExport::Type(ty) => {
                    let def_id = ty.def_id?;
                    let variants = self.imported_enum_variants.get(&def_id)?;
                    Some((def_id, ty.ty, variants.clone()))
                }
                ImportedFileExport::Function(..) | ImportedFileExport::Struct(_) => None,
            })
            .collect()
    }

    /// Resolve a namespace enum-variant value reference (`module.VARIANT`):
    /// returns the consumer variant `DefId` and the variant's value type
    /// (`Type::Enum(consumer enum def)`). Only variants of enums exported by
    /// the namespace resolve; consts/type exports keep their existing paths.
    ///
    /// A variant flagged `payload_unresolved` is excluded, matching
    /// [`Resolver::variant_child`]: its payload identity is unknown to this
    /// consumer, so a value-position reference must fail closed rather than
    /// name a variant the consumer cannot build.
    ///
    /// A member declared by more than one exported enum is ambiguous and
    /// resolves to `None`; the qualified form (`module.Enum.member`, see
    /// [`Resolver::namespace_file_type_variant`]) names the intended enum.
    #[must_use]
    pub fn namespace_file_variant(
        &self,
        namespace: LexSymbol,
        member: LexSymbol,
    ) -> Option<(DefId, TypeId)> {
        let interface = self.file_interfaces.get(&namespace)?;
        let mut found: Option<(DefId, TypeId)> = None;
        for export in interface.exports.values() {
            let ImportedFileExport::Type(ty) = export else {
                continue;
            };
            // Aliases and other non-enum type exports carry no variant table;
            // they must not end the scan before the namespace's enums.
            let Some(variants) = ty
                .def_id
                .and_then(|enum_def| self.imported_enum_variants.get(&enum_def))
            else {
                continue;
            };
            let Some(variant) = variants
                .iter()
                .find(|variant| variant.name == member && !variant.payload_unresolved)
            else {
                continue;
            };
            match found {
                // The same variant under a second export key is not a clash.
                Some((def_id, _)) if def_id == variant.def_id => {}
                // Two enums declare `member`: fail closed rather than let
                // export-table order pick one. `module.Enum.member` names it.
                Some(_) => return None,
                None => found = Some((variant.def_id, ty.ty)),
            }
        }
        found
    }

    /// Resolve a namespace type-member variant value reference
    /// (`module.Type.VARIANT`): the type named by the second segment must be
    /// a type export of the namespace and the terminal segment one of that
    /// type's declared variants. Returns the consumer variant
    /// `DefId` and the variant's value type (`Type::Enum(consumer type
    /// def)`), mirroring [`Resolver::namespace_file_variant`] for the
    /// single-segment form. Enums and unions both install as `Type` exports
    /// carrying variant metadata, so the two declarations share this path.
    /// As in [`Resolver::namespace_file_variant`], a variant flagged
    /// `payload_unresolved` is excluded.
    #[must_use]
    pub fn namespace_file_type_variant(
        &self,
        namespace: LexSymbol,
        type_name: LexSymbol,
        variant_name: LexSymbol,
    ) -> Option<(DefId, TypeId)> {
        let interface = self.file_interfaces.get(&namespace)?;
        let type_key = Self::file_interface_export_key(interface, type_name)?;
        let ImportedFileExport::Type(ty) = interface.exports.get(&type_key)? else {
            return None;
        };
        let type_def = ty.def_id?;
        let variants = self.imported_enum_variants.get(&type_def)?;
        let variant = variants
            .iter()
            .find(|variant| variant.name == variant_name && !variant.payload_unresolved)?;
        Some((variant.def_id, ty.ty))
    }

    /// Resolve the terminal segment of a variant path against a qualified
    /// imported parent (`a.A.X`, or a deeper module-qualified path).
    ///
    /// The first segment must name an installed module namespace binding and
    /// every middle segment must be a module-kind binding; the penultimate
    /// segment must name a type export of that namespace. When that type
    /// export carries variant metadata, the terminal segment resolves against
    /// it alone: a declared variant binds directly, anything else is a hard
    /// [`QualifiedVariantPathLookup::UnknownVariant`] — never a bare-table
    /// fallback through another union. Paths that do not name a qualified
    /// imported parent at all return
    /// [`QualifiedVariantPathLookup::NotQualified`] so callers keep the
    /// legacy single-segment policy.
    #[must_use]
    pub fn qualified_variant_path(&self, path: &[LexSymbol]) -> QualifiedVariantPathLookup {
        let Some((&variant_name, init)) = path.split_last() else {
            return QualifiedVariantPathLookup::NotQualified;
        };
        let Some((&type_name, prefix)) = init.split_last() else {
            return QualifiedVariantPathLookup::NotQualified;
        };
        let Some(&namespace) = prefix.first() else {
            return QualifiedVariantPathLookup::NotQualified;
        };
        for segment in &prefix[1..] {
            let Some(def_id) = self.lookup(*segment) else {
                return QualifiedVariantPathLookup::NotQualified;
            };
            if !matches!(
                self.get_symbol(def_id).map(|symbol| symbol.kind),
                Some(SymbolKind::Module)
            ) {
                return QualifiedVariantPathLookup::NotQualified;
            }
        }
        let Some(namespace_def) = self.lookup(namespace) else {
            return QualifiedVariantPathLookup::NotQualified;
        };
        if !matches!(
            self.get_symbol(namespace_def).map(|symbol| symbol.kind),
            Some(SymbolKind::Module)
        ) {
            return QualifiedVariantPathLookup::NotQualified;
        }
        let Some(type_def) = self.namespace_file_type_def(namespace, type_name) else {
            return QualifiedVariantPathLookup::NotQualified;
        };
        if !self.imported_nominal_has_variants(type_def) {
            return QualifiedVariantPathLookup::NotQualified;
        }
        match self.namespace_file_type_variant(namespace, type_name, variant_name) {
            Some((def_id, _)) => QualifiedVariantPathLookup::Variant(def_id),
            None => QualifiedVariantPathLookup::UnknownVariant,
        }
    }

    /// Return the consumer `DefId` of an imported file namespace type member
    /// (`module.Type`). Type aliases, structs, enums, and unions all install
    /// as `Type`/`Struct` exports.
    #[must_use]
    pub fn namespace_file_type_def(
        &self,
        namespace: LexSymbol,
        type_name: LexSymbol,
    ) -> Option<DefId> {
        let interface = self.file_interfaces.get(&namespace)?;
        let type_key = Self::file_interface_export_key(interface, type_name)?;
        let export = interface.exports.get(&type_key)?;
        match export {
            ImportedFileExport::Type(ty) => ty.def_id,
            ImportedFileExport::Struct(strukt) => Some(strukt.def_id),
            ImportedFileExport::Function(..) => None,
        }
    }

    /// Consumer-local `DefId` of an imported struct (genus) export, `None` for
    /// every other export kind. A `module.Tab.Z` static read names the genus
    /// through this definition.
    #[must_use]
    pub fn namespace_file_struct_def(
        &self,
        namespace: LexSymbol,
        type_name: LexSymbol,
    ) -> Option<DefId> {
        let interface = self.file_interfaces.get(&namespace)?;
        let type_key = Self::file_interface_export_key(interface, type_name)?;
        match interface.exports.get(&type_key)? {
            ImportedFileExport::Struct(strukt) => Some(strukt.def_id),
            ImportedFileExport::Type(_) | ImportedFileExport::Function(..) => None,
        }
    }

    /// Whether an imported nominal `DefId` carries variant metadata
    /// (enum/union exports). Struct exports and aliases return `false`.
    #[must_use]
    pub fn imported_nominal_has_variants(&self, def_id: DefId) -> bool {
        self.imported_enum_variants.contains_key(&def_id)
    }

    #[must_use]
    pub fn imported_file_type_exports(&self, namespace: LexSymbol) -> Vec<ImportedFileTypeExport> {
        self.file_interfaces
            .get(&namespace)
            .into_iter()
            .flat_map(|interface| interface.exports.iter())
            .filter_map(|(member, export)| match export {
                ImportedFileExport::Struct(strukt) => Some(ImportedFileTypeExport {
                    member: *member,
                    def_id: strukt.def_id,
                }),
                ImportedFileExport::Type(ty) => Some(ImportedFileTypeExport {
                    member: *member,
                    def_id: ty.def_id?,
                }),
                ImportedFileExport::Function(..) => None,
            })
            .collect()
    }

    /// Interned binding for an installed file interface, looked up by name
    /// without requiring a mutable interner.
    ///
    /// Transitive library leaves are installed on the consumer during
    /// `set_file_interfaces_from_snapshots` (so signatures can name them) but
    /// have no HIR import binding. Emit walks this to register those `DefIds`.
    #[must_use]
    pub fn file_interface_namespace(
        &self,
        name: &str,
        interner: &radix_lexer::Interner,
    ) -> Option<LexSymbol> {
        self.file_interfaces
            .keys()
            .copied()
            .find(|sym| interner.resolve(*sym) == name)
    }

    /// Every installed file-interface namespace, including transitive library
    /// leaves that have no consumer HIR import binding.
    pub fn file_interface_namespaces(&self) -> impl Iterator<Item = LexSymbol> + '_ {
        self.file_interfaces.keys().copied()
    }

    /// Canonical `FileInterface` export name for a consumer-locale surface.
    ///
    /// Target backends write this name. The library module is emitted from
    /// native source, so `Visus` must become `View`.
    #[must_use]
    pub fn imported_export_canonical_name(
        &self,
        namespace: LexSymbol,
        surface: LexSymbol,
    ) -> LexSymbol {
        let Some(interface) = self.file_interfaces.get(&namespace) else {
            return surface;
        };
        interface
            .canonical_exports
            .iter()
            .find_map(|(canonical, projected)| (*projected == surface).then_some(*canonical))
            .unwrap_or(surface)
    }

    /// Consumer-locale surface for a canonical `FileInterface` export name.
    ///
    /// Package-MIR linking keys library call targets by the library's native
    /// (canonical) export name. The consumer HIR uses the projected surface.
    #[must_use]
    pub fn imported_export_surface_name(
        &self,
        namespace: LexSymbol,
        canonical: LexSymbol,
    ) -> LexSymbol {
        self.file_interfaces
            .get(&namespace)
            .and_then(|interface| interface.canonical_exports.get(&canonical).copied())
            .unwrap_or(canonical)
    }

    /// Recover the canonical imported method identity for a consumer-locale
    /// spelling and receiver definition.
    #[must_use]
    pub fn imported_method_canonical_name(
        &self,
        def_id: DefId,
        surface_name: LexSymbol,
    ) -> Option<LexSymbol> {
        self.file_interfaces
            .values()
            .flat_map(|interface| interface.exports.values())
            .find_map(|export| {
                let (export_def_id, methods) = match export {
                    ImportedFileExport::Type(ty) => (ty.def_id?, &ty.methods),
                    ImportedFileExport::Struct(strukt) => (strukt.def_id, &strukt.methods),
                    ImportedFileExport::Function(..) => return None,
                };
                (export_def_id == def_id)
                    .then(|| {
                        methods
                            .iter()
                            .find(|method| {
                                method.surface_name == surface_name || method.name == surface_name
                            })
                            .map(|method| method.name)
                    })
                    .flatten()
            })
    }

    /// Mark a namespace binding as used from type or value namespace syntax.
    pub fn mark_namespace_used(&mut self, namespace: LexSymbol) {
        self.used_namespaces.insert(namespace);
    }

    /// Return whether a namespace binding was referenced outside the import declaration.
    #[must_use]
    pub fn namespace_used(&self, namespace: LexSymbol) -> bool {
        self.used_namespaces.contains(&namespace)
    }

    /// Insert an existing definition into the current lexical scope.
    ///
    /// # Errors
    ///
    /// Returns the duplicate `name` symbol if the name is already bound in the
    /// current scope.
    pub fn bind_name_in_current_scope(
        &mut self,
        name: LexSymbol,
        def_id: DefId,
        kind: SymbolKind,
    ) -> Result<(), LexSymbol> {
        let previous = self.lookup_shadow.get(&name).copied();
        let scope = &mut self.scopes[self.current_scope.0 as usize];
        if let Some(existing) = scope.symbols.get(&name).copied() {
            // A value binding (function or module-level value) may shadow a
            // reader-pack type spelling in the same frame: the pack words are
            // contextual, not reserved (`fn tensor(...)` under any locale).
            // The type spelling stays reachable from type syntax through
            // `shadowed_types`; a user-declared type of the same name is still
            // a duplicate.
            if matches!(kind, SymbolKind::Function | SymbolKind::Local)
                && existing.kind == SymbolKind::TypeAlias
                && self.locale_pack_type_aliases.contains_key(&existing.def_id)
            {
                scope.shadowed_types.insert(name, existing);
                scope.symbols.insert(name, ScopeEntry { def_id, kind });
                scope.shadow_deltas.push((name, previous));
                self.lookup_shadow.insert(name, def_id);
                return Ok(());
            }

            // A module import lives in the value namespace and may shadow a
            // reader-pack type spelling that occupies the same name in the
            // same scope frame (e.g. `importa ex "norma:json" json`
            // vs the en pack's `json` type). Type resolution still reaches
            // builtin types through the registry fallback, so the module
            // binding replaces the type entry without losing the type.
            if !(kind == SymbolKind::Module && existing.kind.is_type()) {
                return Err(name);
            }
        }
        scope.symbols.insert(name, ScopeEntry { def_id, kind });
        scope.shadow_deltas.push((name, previous));
        // Innermost binding wins for value lookup.
        self.lookup_shadow.insert(name, def_id);
        Ok(())
    }

    /// Return the reserved builtin `forma` genus id when registered.
    #[must_use]
    pub fn builtin_forma_def_id(&self) -> Option<DefId> {
        self.builtin_forma_def_id
    }

    /// Return the builtin `forma.template` field symbol when registered.
    #[must_use]
    pub fn builtin_forma_template_field(&self) -> Option<LexSymbol> {
        self.builtin_forma_template_field
    }

    /// Return the builtin `forma.params` field symbol when registered.
    #[must_use]
    pub fn builtin_forma_params_field(&self) -> Option<LexSymbol> {
        self.builtin_forma_params_field
    }

    /// Record the builtin `forma` genus identity and field symbols during
    /// builtin registration. User code must not call this; it is owned by
    /// [`crate::builtins::forma::register`].
    pub(crate) fn register_builtin_forma(
        &mut self,
        def_id: DefId,
        template_field: LexSymbol,
        params_field: LexSymbol,
    ) {
        self.builtin_forma_def_id = Some(def_id);
        self.builtin_forma_template_field = Some(template_field);
        self.builtin_forma_params_field = Some(params_field);
    }

    #[must_use]
    pub fn builtin_scrinium_def_id(&self) -> Option<DefId> {
        self.builtin_scrinium_def_id
    }

    #[must_use]
    pub fn builtin_status_def_id(&self) -> Option<DefId> {
        self.builtin_status_def_id
    }

    #[must_use]
    pub fn builtin_sermo_def_id(&self) -> Option<DefId> {
        self.builtin_sermo_def_id
    }

    #[must_use]
    pub fn builtin_meus_def_id(&self) -> Option<DefId> {
        self.builtin_meus_def_id
    }

    #[must_use]
    pub fn builtin_tuus_def_id(&self) -> Option<DefId> {
        self.builtin_tuus_def_id
    }

    /// Builtin `trapped<R, T>` genus definition id (f9d8a611).
    #[must_use]
    pub fn builtin_trapped_def_id(&self) -> Option<DefId> {
        self.builtin_trapped_def_id
    }

    /// Whether the program applied the builtin `trapped<R, T>` genus: a
    /// `trapped<...>` type position, or a `capta` boundary (which applies it
    /// implicitly through its re-typed result). Drives HIR item injection.
    #[must_use]
    pub fn builtin_trapped_applied(&self) -> bool {
        self.builtin_trapped_applied
    }

    /// Record the builtin `trapped` genus definition id at registration.
    pub(crate) fn register_builtin_trapped_def_id(&mut self, def_id: DefId) {
        self.builtin_trapped_def_id = Some(def_id);
    }

    /// Record that the program applies the builtin `trapped<R, T>` genus.
    pub(crate) fn mark_builtin_trapped_applied(&mut self) {
        self.builtin_trapped_applied = true;
    }

    pub(crate) fn register_builtin_scrinium(
        &mut self,
        def_id: DefId,
        field_symbols: Vec<LexSymbol>,
    ) {
        self.builtin_scrinium_def_id = Some(def_id);
        self.builtin_scrinium_field_symbols = field_symbols;
    }

    pub(crate) fn builtin_scrinium_field_symbols(&self) -> &[LexSymbol] {
        &self.builtin_scrinium_field_symbols
    }

    pub(crate) fn register_builtin_status(&mut self, def_id: DefId) {
        self.builtin_status_def_id = Some(def_id);
    }

    pub(crate) fn register_builtin_sermo(&mut self, def_id: DefId) {
        self.builtin_sermo_def_id = Some(def_id);
    }

    pub(crate) fn register_builtin_meus(&mut self, def_id: DefId) {
        self.builtin_meus_def_id = Some(def_id);
    }

    pub(crate) fn register_builtin_tuus(&mut self, def_id: DefId) {
        self.builtin_tuus_def_id = Some(def_id);
    }

    pub(crate) fn register_builtin_status_variant(
        &mut self,
        enum_def_id: DefId,
        variant_def_id: DefId,
        variant_name: LexSymbol,
    ) {
        self.register_variant_parent(variant_def_id, enum_def_id);
        self.builtin_status_variants
            .push((variant_def_id, variant_name));
    }

    pub(crate) fn builtin_status_variants(&self, enum_def_id: DefId) -> Vec<(DefId, LexSymbol)> {
        if self.builtin_status_def_id == Some(enum_def_id) {
            self.builtin_status_variants.clone()
        } else {
            Vec::new()
        }
    }

    pub(crate) fn register_variant_parent(&mut self, variant_def_id: DefId, enum_def_id: DefId) {
        self.variant_parents.insert(variant_def_id, enum_def_id);
        if let Some(symbol) = self.symbols.get(&variant_def_id) {
            self.variants_by_parent_name
                .entry((enum_def_id, symbol.name))
                .or_insert(variant_def_id);
            self.variants_by_name
                .entry(symbol.name)
                .or_default()
                .push(variant_def_id);
        }
    }

    /// Register a variant under its parent union/enum identity.
    ///
    /// Variant identity is scoped by the parent (`parent::variant`): the symbol
    /// record is stored in the resolver symbol table and linked through
    /// [`Self::register_variant_parent`], but the bare name is NOT entered into
    /// any scope frame. Two unions in one module may therefore declare the same
    /// variant name without colliding. Same-parent duplicates still fail
    /// (per-union uniqueness), which keeps SEM005 at the collection site.
    ///
    /// Returns `None` when the parent already owns a variant with this name.
    pub fn define_variant(
        &mut self,
        parent_def_id: DefId,
        name: LexSymbol,
        span: Span,
    ) -> Option<DefId> {
        if self.variant_child(parent_def_id, name).is_some() {
            return None;
        }
        let def_id = self.fresh_def_id();
        self.insert_variant(def_id, parent_def_id, name, span);
        Some(def_id)
    }

    /// Register a compiler-injected builtin variant under its parent identity.
    ///
    /// Same symbol and per-union bookkeeping as [`Self::define_variant`], but
    /// the identity comes from the builtin low range
    /// ([`Self::fresh_builtin_def_id`]), so builtin registration never burns a
    /// user id and user declarations keep starting at [`USER_DEF_ID_BASE`].
    ///
    /// Returns `None` when the parent already owns a variant with this name.
    pub fn define_builtin_variant(
        &mut self,
        parent_def_id: DefId,
        name: LexSymbol,
        span: Span,
    ) -> Option<DefId> {
        if self.variant_child(parent_def_id, name).is_some() {
            return None;
        }
        let def_id = self.fresh_builtin_def_id();
        self.insert_variant(def_id, parent_def_id, name, span);
        Some(def_id)
    }

    /// Insert one variant row. The caller owns identity allocation so the user
    /// and builtin counters stay disjoint.
    fn insert_variant(&mut self, def_id: DefId, parent_def_id: DefId, name: LexSymbol, span: Span) {
        self.symbols.insert(
            def_id,
            Symbol {
                def_id,
                name,
                kind: SymbolKind::Variant,
                ty: None,
                mutable: false,
                span,
            },
        );
        self.variant_parents.insert(def_id, parent_def_id);
        self.variants_by_parent_name
            .entry((parent_def_id, name))
            .or_insert(def_id);
        self.variants_by_name.entry(name).or_default().push(def_id);
    }

    #[must_use]
    pub fn variant_child(&self, enum_def_id: DefId, variant_name: LexSymbol) -> Option<DefId> {
        // Imported enums' variant membership is the `imported_enum_variants`
        // table, not the parent link: a variant whose payload cannot be
        // remapped stays in the table (the commune projection arms it from the
        // row) but is flagged `payload_unresolved` and must not resolve as a
        // member here — the package-MIR merge depends on that exclusion to
        // fail closed on the unknown payload identity.
        if let Some(variants) = self.imported_enum_variants.get(&enum_def_id) {
            return variants
                .iter()
                .find(|variant| variant.name == variant_name && !variant.payload_unresolved)
                .map(|variant| variant.def_id);
        }
        self.variants_by_parent_name
            .get(&(enum_def_id, variant_name))
            .copied()
    }

    /// Resolve a bare variant name against the per-union variant namespace.
    ///
    /// Returns the variant definition when exactly one visible union defines
    /// the name, an ambiguity marker when two or more do, and `None` when no
    /// union does. Per-union uniqueness means each union owns at most one
    /// variant of a given name, so the count is also the number of defining
    /// unions. The bare scope table is deliberately NOT consulted here:
    /// callers resolve ordinary names through [`Self::lookup`] first so
    /// locals, functions, and builtins keep shadowing variant names.
    ///
    /// The current unit's OWN unions shadow imported namespaces' unions: a
    /// local discretio's variant resolves bare even when an imported union
    /// defines the same name — the imported variant is reachable only through
    /// its qualified namespace, so it must not collapse the local bare name
    /// into a false `variant_ambiguous` (gradus tensor.fab / shape.fab
    /// `ElementaMismatch` shape). Imported unions' variants participate in
    /// bare resolution only when no local union defines the name; two or more
    /// defining unions remain an explicit ambiguity, never a silent pick.
    #[must_use]
    pub fn lookup_variant_bare(&self, name: LexSymbol) -> BareVariantLookup {
        let mut local_variant: Option<DefId> = None;
        let mut local_unions = 0usize;
        let mut single: Option<DefId> = None;
        let mut defining_unions = 0usize;
        for &variant_def_id in self.variants_by_name.get(&name).into_iter().flatten() {
            let Some(symbol) = self.symbols.get(&variant_def_id) else {
                continue;
            };
            if symbol.kind == SymbolKind::Variant && symbol.name == name {
                defining_unions += 1;
                single = Some(variant_def_id);
                let Some(&parent_def_id) = self.variant_parents.get(&variant_def_id) else {
                    continue;
                };
                if !self.imported_enum_variants.contains_key(&parent_def_id) {
                    local_unions += 1;
                    local_variant = Some(variant_def_id);
                }
            }
        }
        // A local union's variant is the only bare-visible one: imported
        // unions' variants are reachable only qualified, so they never turn a
        // local bare name into an ambiguity. The loop guarantees the
        // single-union arm also carries the defining local variant.
        if local_unions > 0 {
            return match local_variant {
                Some(def_id) if local_unions == 1 => BareVariantLookup::Single(def_id),
                _ => BareVariantLookup::Ambiguous {
                    defining_unions: local_unions,
                },
            };
        }
        // The loop sets `single` on every increment of `defining_unions`, so
        // the count-1 arm structurally carries the defining variant: binding
        // the Some case here needs no panicking call to bridge the Option.
        match (defining_unions, single) {
            (0, _) => BareVariantLookup::None,
            (1, Some(def_id)) => BareVariantLookup::Single(def_id),
            _ => BareVariantLookup::Ambiguous { defining_unions },
        }
    }

    /// Variant lists, in declaration order, for enums that have no local HIR
    /// item: imported enums and the builtin status enum. Exhaustiveness needs
    /// the closed variant set of every enum a `discerne` can scrutinize, and
    /// local HIR only carries the enums this unit declares.
    ///
    /// An imported variant whose payload failed consumer-side remap is left
    /// out, matching [`Self::variant_child`]: the consumer cannot name it in a
    /// pattern, so coverage is proven over the partial remap and codegen
    /// synthesizes the fallback arm.
    #[must_use]
    pub fn non_local_enum_variants(&self) -> Vec<(DefId, Vec<DefId>)> {
        let mut lists: Vec<(DefId, Vec<DefId>)> = self
            .imported_enum_variants
            .iter()
            .map(|(&enum_def, variants)| {
                let nameable = variants
                    .iter()
                    .filter(|variant| !variant.payload_unresolved)
                    .map(|variant| variant.def_id);
                (enum_def, nameable.collect())
            })
            .collect();
        if let Some(status) = self.builtin_status_def_id {
            lists.push((
                status,
                self.builtin_status_variants
                    .iter()
                    .map(|(def_id, _)| *def_id)
                    .collect(),
            ));
        }
        lists
    }

    /// Every variant → parent-enum definition pair known to this resolver
    /// (local enums and imported enum variants). Used by the package-MIR
    /// merge to remap a source unit's variant definitions to the consumer
    /// analysis's identities.
    #[must_use]
    pub fn variant_parents_all(&self) -> Vec<(DefId, DefId)> {
        self.variant_parents
            .iter()
            .map(|(&variant, &parent)| (variant, parent))
            .collect()
    }

    pub(crate) fn register_gpu_builtin(
        &mut self,
        def_id: DefId,
        builtin: radix_types::MirKernelBuiltin,
    ) {
        if self.gpu_builtins.insert(def_id, builtin).is_none() {
            self.gpu_builtins_order.push((def_id, builtin));
        } else if let Some(slot) = self
            .gpu_builtins_order
            .iter_mut()
            .find(|(candidate, _)| *candidate == def_id)
        {
            slot.1 = builtin;
        }
    }

    #[must_use]
    pub fn gpu_builtin(&self, def_id: DefId) -> Option<radix_types::MirKernelBuiltin> {
        self.gpu_builtins.get(&def_id).copied()
    }

    #[must_use]
    pub fn gpu_builtins(&self) -> &[(DefId, radix_types::MirKernelBuiltin)] {
        &self.gpu_builtins_order
    }

    /// Allocate a fresh definition identity for a user declaration.
    ///
    /// User ids start at [`USER_DEF_ID_BASE`] so builtin registration cannot
    /// shift them. Allocation is separate from [`Self::define`] so passes can
    /// prepare symbol records before inserting them into the current scope.
    pub fn fresh_def_id(&mut self) -> DefId {
        let id = DefId(self.next_def_id);
        self.next_def_id += 1;
        id
    }

    /// Allocate a fresh definition identity for a compiler-injected builtin.
    ///
    /// Builtin ids occupy the low range below [`USER_DEF_ID_BASE`] and are
    /// assigned sequentially during builtin registration.
    pub fn fresh_builtin_def_id(&mut self) -> DefId {
        let id = DefId(self.next_builtin_def_id);
        self.next_builtin_def_id += 1;
        id
    }

    /// Enter a child scope and make it current until [`Self::exit_scope`].
    ///
    /// # Panics
    ///
    /// Panics if the number of live scopes exceeds `u32::MAX`, since the new
    /// scope cannot be addressed by [`ScopeId`].
    pub fn enter_scope(&mut self, kind: ScopeKind) -> ScopeId {
        let id = ScopeId(u32::try_from(self.scopes.len()).expect("scope stack fits u32"));
        let scope = Scope::new(id, Some(self.current_scope), kind);
        self.scopes.push(scope);
        self.current_scope = id;
        // Shadow stays valid: the new frame is empty, so nearest bindings are unchanged.
        id
    }

    /// Exit the current scope, staying at global if there is no parent.
    pub fn exit_scope(&mut self) {
        let dropped_scope = self.current_scope;
        if let Some(parent) = self.scopes[dropped_scope.0 as usize].parent {
            self.current_scope = parent;
            let deltas = std::mem::take(&mut self.scopes[dropped_scope.0 as usize].shadow_deltas);
            for (name, previous) in deltas.into_iter().rev() {
                match previous {
                    Some(def_id) => {
                        self.lookup_shadow.insert(name, def_id);
                    }
                    None => {
                        self.lookup_shadow.remove(&name);
                    }
                }
            }
        }
    }

    /// Define a symbol in the current scope.
    ///
    /// Duplicate names are rejected only within the current lexical frame;
    /// shadowing outer scopes remains representable and is handled by pass
    /// policy when it wants to warn.
    ///
    /// # Errors
    ///
    /// Returns the duplicate `name` symbol if the name is already bound in the
    /// current scope.
    pub fn define(&mut self, symbol: Symbol) -> Result<DefId, LexSymbol> {
        let def_id = symbol.def_id;
        self.bind_name_in_current_scope(symbol.name, def_id, symbol.kind)?;
        self.symbols.insert(def_id, symbol);
        Ok(def_id)
    }

    /// Register a compiler-managed symbol without binding its name in any scope.
    ///
    /// Used for the body-visible `magnitudo` parameters minted during HIR
    /// lowering: their names resolve through the lowerer's lexical scopes, and
    /// the symbols-map entry exists only so `check_path` can read the stored
    /// type by `DefId`. Unlike [`Self::define`], a same-name binding elsewhere
    /// is not an error — each minted parameter owns a fresh def.
    pub fn register_unbound_symbol(&mut self, symbol: Symbol) -> DefId {
        let def_id = symbol.def_id;
        self.symbols.insert(def_id, symbol);
        def_id
    }

    /// Look up the nearest visible definition for a source name.
    #[must_use]
    pub fn lookup(&self, name: LexSymbol) -> Option<DefId> {
        self.lookup_shadow.get(&name).copied()
    }

    /// Record that a builtin-claimed expression is shadowed by a value binding.
    pub fn record_shadowed_builtin(&mut self, node: radix_syntax::NodeId, def_id: DefId) {
        self.shadowed_builtins.insert(node, def_id);
    }

    /// Return the value binding that shadows a builtin-claimed expression, if any.
    #[must_use]
    pub fn shadowed_builtin(&self, node: radix_syntax::NodeId) -> Option<DefId> {
        self.shadowed_builtins.get(&node).copied()
    }

    /// Record per-position `iuncta` labels for a binding.
    pub fn record_tuple_element_labels(&mut self, def_id: DefId, labels: Vec<Option<LexSymbol>>) {
        self.tuple_element_labels.insert(def_id, labels);
    }

    /// Per-position `iuncta` labels for a binding, if an annotation was recorded.
    #[must_use]
    pub fn tuple_element_labels(&self, def_id: DefId) -> Option<&[Option<LexSymbol>]> {
        self.tuple_element_labels.get(&def_id).map(Vec::as_slice)
    }

    /// Record the closed-set member names a binding's annotation names.
    pub fn record_subject_member_set(&mut self, def_id: DefId, members: Vec<String>) {
        self.subject_member_sets.insert(def_id, members);
    }

    /// Closed-set member names recorded for a binding, if an annotation was seen.
    #[must_use]
    pub fn subject_member_set(&self, def_id: DefId) -> Option<&[String]> {
        self.subject_member_sets.get(&def_id).map(Vec::as_slice)
    }

    /// Return visible value-namespace names from the current lexical scope.
    ///
    /// Served from the nearest-name shadow: one entry per visible spelling,
    /// filtered to value kinds (type bindings that win a name are excluded).
    pub(crate) fn visible_value_names(&self) -> Vec<LexSymbol> {
        self.lookup_shadow
            .iter()
            .filter_map(|(&name, &def_id)| {
                self.symbols
                    .get(&def_id)
                    .is_some_and(|symbol| symbol.kind.is_value_name())
                    .then_some(name)
            })
            .collect()
    }

    /// Look up the nearest visible type-bearing definition for a source name.
    ///
    /// Type syntax is a distinct namespace from value syntax. A local value can
    /// shadow a value lookup without making `name { ... }` or `fixum name ...`
    /// stop referring to an outer genus, implendum, or alias in type position.
    ///
    /// One map probe per scope frame: `ScopeEntry.kind` avoids a second
    /// `symbols` table hash for the type check.
    #[must_use]
    pub fn lookup_type(&self, name: LexSymbol) -> Option<DefId> {
        let mut scope_id = Some(self.current_scope);

        while let Some(id) = scope_id {
            let scope = &self.scopes[id.0 as usize];
            if let Some(entry) = scope.symbols.get(&name)
                && entry.kind.is_type()
            {
                return Some(entry.def_id);
            }
            if let Some(entry) = scope.shadowed_types.get(&name) {
                return Some(entry.def_id);
            }
            scope_id = scope.parent;
        }

        None
    }

    /// Get symbol metadata by definition identity.
    #[must_use]
    pub fn get_symbol(&self, def_id: DefId) -> Option<&Symbol> {
        self.symbols.get(&def_id)
    }

    /// Get mutable symbol metadata by definition identity.
    pub fn get_symbol_mut(&mut self, def_id: DefId) -> Option<&mut Symbol> {
        self.symbols.get_mut(&def_id)
    }

    /// Record ordered generic parameter kinds for one declaration identity.
    pub fn register_generic_param_kinds(
        &mut self,
        def_id: DefId,
        kinds: impl IntoIterator<Item = radix_syntax::GenericParamKind>,
    ) {
        self.generic_param_kinds
            .insert(def_id, kinds.into_iter().collect());
    }

    /// Return ordered generic parameter kinds for a declaration identity.
    #[must_use]
    pub fn generic_param_kinds(&self, def_id: DefId) -> Option<&[radix_syntax::GenericParamKind]> {
        self.generic_param_kinds.get(&def_id).map(Vec::as_slice)
    }

    /// Record ordered generic descriptors, including declaration defaults, for
    /// a named type identity.
    pub fn register_generic_param_descriptors(
        &mut self,
        def_id: DefId,
        descriptors: impl IntoIterator<Item = radix_types::GenericParamDescriptor>,
    ) {
        self.generic_param_descriptors
            .insert(def_id, descriptors.into_iter().collect());
    }

    /// Return ordered generic descriptors for a named type identity.
    #[must_use]
    pub fn generic_param_descriptors(
        &self,
        def_id: DefId,
    ) -> Option<&[radix_types::GenericParamDescriptor]> {
        self.generic_param_descriptors
            .get(&def_id)
            .map(Vec::as_slice)
    }

    /// Register the resolved relational schema metadata for a schema
    /// definition.
    pub fn register_schema(&mut self, def_id: DefId, schema: crate::SchemaDef) {
        self.schemas.insert(def_id, schema);
    }

    /// Return the resolved relational schema metadata for a definition, if any.
    #[must_use]
    pub fn schema_def(&self, def_id: DefId) -> Option<&crate::SchemaDef> {
        self.schemas.get(&def_id)
    }

    /// Record one locale-pack `[types]` surface as a builtin `TypeAlias` def.
    pub fn record_locale_pack_type_alias(&mut self, def_id: DefId, name: LexSymbol) {
        self.locale_pack_type_aliases.insert(def_id, name);
    }

    /// Builtin `TypeAlias` defs installed from the reader-locale pack.
    pub fn locale_pack_type_alias_defs(&self) -> impl Iterator<Item = DefId> + '_ {
        self.locale_pack_type_aliases.keys().copied()
    }

    /// Surface spellings for locale-pack `TypeAlias` defs, for emit-table names.
    #[must_use]
    pub fn locale_pack_type_alias_names(
        &self,
        interner: &radix_lexer::Interner,
    ) -> FxHashMap<DefId, String> {
        self.locale_pack_type_aliases
            .iter()
            .map(|(&def_id, &name)| (def_id, interner.resolve(name).to_owned()))
            .collect()
    }

    /// Return the current lexical scope.
    #[must_use]
    pub fn current(&self) -> &Scope {
        &self.scopes[self.current_scope.0 as usize]
    }

    /// Return whether the current context is inside a loop in this function.
    ///
    /// `perge` targets the nearest loop (ruling R3): a plain `fac` block is
    /// [`ScopeKind::Breakable`], not a loop, so the search passes through it.
    /// A function or closure boundary stops the search.
    #[must_use]
    pub fn in_loop(&self) -> bool {
        let mut scope_id = Some(self.current_scope);

        while let Some(id) = scope_id {
            let scope = &self.scopes[id.0 as usize];
            if scope.kind == ScopeKind::Loop {
                return true;
            }
            if scope.kind == ScopeKind::Function {
                return false;
            }
            scope_id = scope.parent;
        }

        false
    }

    /// Return whether `rumpe` may target an enclosing breakable scope.
    ///
    /// Loops and breakable blocks both qualify; `perge` still requires [`in_loop`].
    #[must_use]
    pub fn in_breakable(&self) -> bool {
        let mut scope_id = Some(self.current_scope);

        while let Some(id) = scope_id {
            let scope = &self.scopes[id.0 as usize];
            if matches!(scope.kind, ScopeKind::Loop | ScopeKind::Breakable) {
                return true;
            }
            if scope.kind == ScopeKind::Function {
                return false;
            }
            scope_id = scope.parent;
        }

        false
    }

    /// Return whether the current context is inside a function scope.
    #[must_use]
    pub fn in_function(&self) -> bool {
        let mut scope_id = Some(self.current_scope);

        while let Some(id) = scope_id {
            let scope = &self.scopes[id.0 as usize];
            if scope.kind == ScopeKind::Function {
                return true;
            }
            scope_id = scope.parent;
        }

        false
    }

    /// Reconstruct resolver import state from a serialized snapshot.
    ///
    /// Builtin `DefId` values are re-derived from compiler constants — NOT
    /// deserialized from the snapshot. Walk-state fields (`scopes`,
    /// `current_scope`, `lookup_shadow`) and GPU builtin registrations are
    /// initialized empty/default.
    #[cfg(feature = "hir-fhir")]
    #[must_use]
    pub fn from_snapshot(snapshot: radix_hir_fhir::ResolverSnapshot) -> Self {
        use radix_hir_fhir::FileExportSnapshot;

        let conversion_rows = snapshot.conversion_rows.into_iter().fold(
            FxHashMap::default(),
            |mut rows, snapshot| {
                rows.entry(ConversionPair::new(snapshot.source, snapshot.destination))
                    .or_insert_with(Vec::new)
                    .push(ConversionRow {
                        pair: ConversionPair::new(snapshot.source, snapshot.destination),
                        fn_def_id: snapshot.fn_def_id,
                        span: snapshot.span,
                        // Snapshots carry no scope walk-state; every restored
                        // row is visible from the reconstructed global scope.
                        scope: ScopeId(0),
                        builtin: snapshot.builtin,
                    });
                rows
            },
        );

        // The FHIR snapshot is the codegen-facing surface and carries export
        // names without tiers; reconstruction marks every name `@ publica`.
        // The `@ interna` fail-closed tier gate is enforced during live import
        // resolution, not on artifact round-trips (no new imports resolve
        // against a reconstructed resolver).
        let namespace_exports: FxHashMap<LexSymbol, FxHashMap<LexSymbol, VisibilityTier>> =
            snapshot
                .namespace_exports
                .into_iter()
                .map(|(ns, members)| {
                    (
                        ns,
                        members
                            .into_iter()
                            .map(|member| (member, VisibilityTier::Publica))
                            .collect(),
                    )
                })
                .collect();

        let file_interfaces: FxHashMap<LexSymbol, ImportedFileInterface> = snapshot
            .file_interfaces
            .into_iter()
            .map(|(ns, iface)| {
                let unavailable = iface.unavailable;
                let const_values = iface.const_values;
                let exports: FxHashMap<LexSymbol, ImportedFileExport> = iface
                    .exports
                    .into_iter()
                    .map(|(member, export)| {
                        let imported = match export {
                            FileExportSnapshot::Function(sig) => {
                                // The wire snapshot predates the axis flag and
                                // must not grow a schema field for it
                                // (RESOLVER_SCHEMA is frozen); restored
                                // interfaces keep all-false flags.
                                ImportedFileExport::Function(sig, Vec::new())
                            }
                            FileExportSnapshot::Type { ty, def_id } => {
                                ImportedFileExport::Type(ImportedFileType {
                                    ty,
                                    def_id,
                                    methods: Vec::new(),
                                    // Artifact snapshots do not carry analysis-
                                    // time declaration kinds; restored
                                    // interfaces keep the undirected fallback.
                                    decl_param_kinds: Vec::new(),
                                })
                            }
                            FileExportSnapshot::Struct {
                                def_id,
                                ty,
                                fields,
                                methods,
                                is_json,
                                implements,
                            } => {
                                let imported_fields: Vec<ImportedFileStructField> = fields
                                    .into_iter()
                                    .map(|field| ImportedFileStructField {
                                        name: field.name,
                                        ty: field.ty,
                                        optional: field.optional,
                                        required: field.required,
                                        modifier: field.modifier,
                                        visibility: field.visibility,
                                        init: field.init,
                                    })
                                    .collect();
                                let imported_methods = methods
                                    .into_iter()
                                    .map(|method| ImportedFileInterfaceMethod {
                                        def_id: method.def_id,
                                        name: method.name,
                                        surface_name: method.surface_name,
                                        sig: method.sig,
                                        // Restored artifact route: the wire
                                        // snapshot carries no axis flag.
                                        axis: Vec::new(),
                                        visibility: method.visibility,
                                        unavailable: method
                                            .unavailable
                                            .into_iter()
                                            .map(interface_unavailable_from_wire)
                                            .collect(),
                                    })
                                    .collect();
                                ImportedFileExport::Struct(ImportedFileStruct {
                                    is_json,
                                    implements,
                                    def_id,
                                    ty,
                                    fields: imported_fields,
                                    methods: imported_methods,
                                    // Artifact snapshots do not carry
                                    // declaration params; applied-projection
                                    // metadata stays degraded on this route.
                                    type_params: Vec::new(),
                                })
                            }
                        };
                        (member, imported)
                    })
                    .collect();
                (
                    ns,
                    ImportedFileInterface {
                        exports,
                        tensor_edge_policy_requirements: FxHashMap::default(),
                        canonical_exports: FxHashMap::default(),
                        strict_members: false,
                        identity: None,
                        const_members: FxHashSet::default(),
                        const_values: const_values
                            .into_iter()
                            .map(|(member, value)| (member, interface_const_value_from_wire(value)))
                            .collect(),
                        unavailable: unavailable
                            .into_iter()
                            .map(|(member, gates)| {
                                (
                                    member,
                                    gates
                                        .into_iter()
                                        .map(interface_unavailable_from_wire)
                                        .collect(),
                                )
                            })
                            .collect(),
                        device_contracts: FxHashMap::default(),
                    },
                )
            })
            .collect();

        let used_namespaces: FxHashSet<LexSymbol> = snapshot.used_namespaces.into_iter().collect();

        let imported_nominal_types: FxHashMap<(InterfaceNominalKind, LexSymbol), TypeId> = snapshot
            .imported_nominal_types
            .into_iter()
            .map(|((kind, name), ty)| ((interface_nominal_kind_from_wire(kind), name), ty))
            .collect();

        let ambiguous_imported_nominal_types: FxHashSet<(InterfaceNominalKind, LexSymbol)> =
            snapshot
                .ambiguous_imported_nominal_types
                .into_iter()
                .map(|(kind, name)| (interface_nominal_kind_from_wire(kind), name))
                .collect();

        let mut resolver = Self {
            namespace_exports,
            file_interfaces,
            imported_nominal_types,
            ambiguous_imported_nominal_types,
            conversion_rows,
            used_namespaces,
            ..Self::new()
        };
        // Module bindings are what a `module.NAME` read resolves through to
        // reach the namespace's file interface. The names are not bound in any
        // scope: no resolution runs against a restored resolver.
        for (def_id, name) in snapshot.module_bindings {
            resolver.register_unbound_symbol(Symbol {
                def_id,
                name,
                kind: SymbolKind::Module,
                ty: None,
                mutable: false,
                span: Span::default(),
            });
        }
        resolver
    }

    /// Produce a serializable snapshot of resolver import state for the HIR
    /// artifact wire format.
    ///
    /// Collects the import-facing fields codegen reads from the resolver:
    /// namespace exports, file interfaces, used namespaces, imported nominal
    /// types, and the conversion coherence registry rows. Runtime-only state
    /// — scopes, shadow map, builtins, GPU builtins, insertion-order mirrors —
    /// is excluded and rebuilt during loading.
    #[cfg(feature = "hir-fhir")]
    #[must_use]
    pub fn to_snapshot(&self) -> radix_hir_fhir::ResolverSnapshot {
        use radix_hir_fhir::{
            FileExportSnapshot, FileInterfaceSnapshot, ResolverSnapshot, StructFieldSnapshot,
            StructMethodSnapshot,
        };

        let mut namespace_exports: Vec<_> = self
            .namespace_exports
            .iter()
            .map(|(&ns, members)| {
                let mut members: Vec<_> = members.keys().copied().collect();
                members.sort_by_key(|member| member.0);
                (ns, members)
            })
            .collect();
        namespace_exports.sort_by_key(|(ns, _)| ns.0);

        let mut file_interfaces: Vec<_> = self
            .file_interfaces
            .iter()
            .map(|(&ns, imported)| {
                let mut exports: Vec<_> = imported
                    .exports
                    .iter()
                    .map(|(&member, export)| {
                        let snapshot = match export {
                            ImportedFileExport::Function(sig, _) => {
                                FileExportSnapshot::Function(sig.clone())
                            }
                            ImportedFileExport::Type(ty) => FileExportSnapshot::Type {
                                ty: ty.ty,
                                def_id: ty.def_id,
                            },
                            ImportedFileExport::Struct(strukt) => FileExportSnapshot::Struct {
                                is_json: strukt.is_json,
                                implements: strukt.implements.clone(),
                                def_id: strukt.def_id,
                                ty: strukt.ty,
                                fields: strukt
                                    .fields
                                    .iter()
                                    .map(|field| StructFieldSnapshot {
                                        name: field.name,
                                        ty: field.ty,
                                        optional: field.optional,
                                        required: field.required,
                                        modifier: field.modifier,
                                        visibility: field.visibility,
                                        init: field.init.clone(),
                                    })
                                    .collect(),
                                methods: strukt
                                    .methods
                                    .iter()
                                    .map(|method| StructMethodSnapshot {
                                        def_id: method.def_id,
                                        name: method.name,
                                        surface_name: method.surface_name,
                                        sig: method.sig.clone(),
                                        visibility: method.visibility,
                                        unavailable: method
                                            .unavailable
                                            .iter()
                                            .map(interface_unavailable_to_wire)
                                            .collect(),
                                    })
                                    .collect(),
                            },
                        };
                        (member, snapshot)
                    })
                    .collect();
                // The export map is a hash map; the wire order must not be.
                exports.sort_by_key(|(member, _)| member.0);
                let mut unavailable: Vec<_> = imported
                    .unavailable
                    .iter()
                    .map(|(&member, gates)| {
                        (
                            member,
                            gates
                                .iter()
                                .map(interface_unavailable_to_wire)
                                .collect::<Vec<_>>(),
                        )
                    })
                    .collect();
                unavailable.sort_by_key(|(member, _)| member.0);
                let mut const_values: Vec<_> = imported
                    .const_values
                    .iter()
                    .map(|(&member, value)| (member, interface_const_value_to_wire(value)))
                    .collect();
                const_values.sort_by_key(|(member, _)| member.0);
                (
                    ns,
                    FileInterfaceSnapshot {
                        exports,
                        unavailable,
                        const_values,
                    },
                )
            })
            .collect();
        file_interfaces.sort_by_key(|(ns, _)| ns.0);

        let mut used_namespaces: Vec<_> = self.used_namespaces.iter().copied().collect();
        used_namespaces.sort_by_key(|namespace| namespace.0);

        let mut imported_nominal_types: Vec<_> = self
            .imported_nominal_types
            .iter()
            .map(|(&(kind, name), &ty)| ((interface_nominal_kind_to_wire(kind), name), ty))
            .collect();
        imported_nominal_types
            .sort_by_key(|((kind, name), _)| (nominal_kind_wire_sort_key(*kind), name.0));

        let mut ambiguous_imported_nominal_types: Vec<(radix_hir_fhir::NominalKind, LexSymbol)> =
            self.ambiguous_imported_nominal_types
                .iter()
                .map(|&(kind, name)| (interface_nominal_kind_to_wire(kind), name))
                .collect();
        ambiguous_imported_nominal_types
            .sort_by_key(|(kind, name)| (nominal_kind_wire_sort_key(*kind), name.0));

        let mut conversion_rows = self
            .conversion_rows
            .values()
            .flat_map(|rows| {
                rows.iter()
                    .map(|row| radix_hir_fhir::ConversionRowSnapshot {
                        source: row.pair.source,
                        destination: row.pair.destination,
                        fn_def_id: row.fn_def_id,
                        span: row.span,
                        builtin: row.builtin,
                    })
            })
            .collect::<Vec<_>>();
        conversion_rows.sort_by_key(|row| {
            (
                row.destination.0,
                row.source.0,
                row.builtin,
                row.fn_def_id.map_or(0, |def_id| def_id.0),
            )
        });

        let mut module_bindings: Vec<_> = self
            .symbols
            .values()
            .filter(|symbol| symbol.kind == SymbolKind::Module)
            .map(|symbol| (symbol.def_id, symbol.name))
            .collect();
        module_bindings.sort_by_key(|(def_id, _)| def_id.0);

        ResolverSnapshot {
            namespace_exports,
            file_interfaces,
            used_namespaces,
            imported_nominal_types,
            ambiguous_imported_nominal_types,
            conversion_rows,
            module_bindings,
        }
    }
}

impl Default for Resolver {
    fn default() -> Self {
        Self::new()
    }
}
