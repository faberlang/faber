//! Data model for the high-level intermediate representation.
//!
//! These structs are the compiler's shared contract after AST lowering. They
//! keep source-level structure recognizable enough for diagnostics and
//! language-aware analysis, while removing grammar trivia that later passes
//! should not depend on. Declarations and bindings are addressed by `DefId`;
//! expression and statement nodes carry `HirId` so analysis results can attach
//! to stable nodes without mutating the tree shape.
//!
//! HIR does not mean "fully typed." A `TypeId` in a syntactic type position is
//! the semantic reference produced from source. Optional `TypeId` fields on
//! expressions, locals, and returns are deliberately left open for typecheck to
//! fill or validate. Codegen must treat missing type information as an upstream
//! analysis failure, not as an invitation to guess.
//!
//! INVARIANTS
//! ==========
//! - `DefId` values refer to declarations, parameters, locals, pattern bindings,
//!   imports, fields, methods, and other named semantic definitions.
//! - `HirId` values identify annotatable HIR nodes, especially statements and
//!   expressions.
//! - `Span` values point back to the source construct that should receive a
//!   diagnostic, even when lowering normalized the syntax around it.
//! - `TypeId` is a reference into the semantic type arena; it is not backend
//!   layout information.
//! - Variants should preserve source-level intent. Lower MIR/control-flow and
//!   target-specific lowering belong in later phases.

use handroll::{FxHashMap, FxHashSet};
use radix_lexer::{Span, Symbol, Token};
use radix_syntax::{
    CallablePosture, FieldModifier, GenericParamKind, JsonValue, TypeFormLevel, Visibility,
};
use radix_types::{
    ContractRef, GenericParamDescriptor, GenericParamSort, IndexId, SemanticGenericParamDefault,
    TypeId, TypeParamConstraint,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// An optional wire value is still a required field: explicit `None` is not
// equivalent to a missing contract fact. `deserialize_with` prevents serde's
// implicit missing-field default for Option without changing either wire shape.
fn required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

/// Stable identity for a named semantic definition.
///
/// Most `DefId` values come from name resolution; lowering also allocates
/// synthetic ids for locals and bindings introduced below the resolver's scope.
/// Later passes should carry these ids instead of re-resolving textual names.
/// One definition may own many HIR nodes: a function has one `DefId`, while its
/// body contains many `HirId` values.
pub use radix_types::DefId;

/// Stable identity for an annotatable HIR node.
///
/// Analysis tables use this ID to attach type facts, flow facts, and diagnostics
/// to statements and expressions without making those facts fields on every HIR
/// variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HirId(pub u32);

// ---------------------------------------------------------------------------
// Program and library surface
// ---------------------------------------------------------------------------

/// HIR for one source module: declarations, import bindings, and one optional
/// executable entry block.
///
/// `entry` is the executable block of this source module (`incipit` or implicit
/// top-level statements). It is not a package or program root.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirModule {
    pub items: Vec<HirItem>,

    /// Executable block of this source module (`incipit` or implicit top-level
    /// statements). Not a package or program root.
    pub entry: Option<HirBlock>,

    /// Normalized metadata attached to the `incipit` statement.
    pub entry_annotations: Vec<HirAnnotation>,

    /// `argumenta` binding when the entry declares CLI args (`incipit argumenta args`).
    pub entry_args_name: Option<Symbol>,

    /// Whether the entry uses async `incipiet` spelling.
    pub entry_is_async: bool,
}

/// Provider identity for an imported library module.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LibraryProvider {
    /// Built-in library bundled with this Faber distribution.
    Builtin(String),

    /// Reserved shape for future package-backed providers.
    Package(String),
}

/// Target-neutral identity of a resolved library import.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LibraryIdentity {
    pub provider: LibraryProvider,
    pub module_path: Vec<String>,
}

/// Kind of declaration imported from a library interface.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LibraryItemKind {
    /// Behavioral contract imported from an `implendum`.
    Interface,

    /// Callable declaration imported from a `functio`.
    Function,

    /// Type alias imported from an `alias`.
    TypeAlias,

    /// Product type imported from a `genus`.
    Struct,

    /// Sum type imported from an enum-like declaration.
    Enum,

    /// Constant imported from a top-level constant declaration.
    Const,
}

/// Provenance for a local binding introduced by a library import.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryBinding {
    pub local_def_id: DefId,
    pub identity: LibraryIdentity,

    /// Rust runtime module when this import is backed by a target runtime crate
    /// instead of generated local declarations.
    pub rust_runtime_module: Option<String>,

    /// Faber method name to Rust runtime symbol mappings from interface metadata.
    pub rust_runtime_methods: BTreeMap<String, String>,
}

/// Provenance for a declaration imported from a library interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryItem {
    pub def_id: DefId,
    pub identity: LibraryIdentity,

    /// Exported library name before local aliasing.
    pub exported_name: String,
    pub kind: LibraryItemKind,

    /// Whether an imported function declares an alternate-exit channel.
    pub is_failable: bool,

    /// Whether an imported function returns a future that must be awaited.
    pub is_async: bool,

    /// Rust runtime type when the Faber interface is a contract over a target
    /// runtime type.
    pub rust_runtime_type: Option<String>,

    /// Omit the generated Rust declaration because the runtime crate owns it.
    pub elide_rust_decl: bool,
}

/// Side table carrying library provenance across analysis and codegen.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryRegistry {
    /// Imported module bindings keyed by their local `DefId`.
    pub bindings: FxHashMap<DefId, LibraryBinding>,

    /// Imported library declarations keyed by their own `DefId`.
    pub items: FxHashMap<DefId, LibraryItem>,

    /// Public member names exposed by library namespace bindings.
    pub exports: FxHashMap<DefId, FxHashSet<String>>,

    /// Explicit `publica` child imports exported by a library namespace.
    pub reexports: FxHashMap<(DefId, String), String>,
}

/// Top-level declaration with shared identity and diagnostic context.
///
/// The item-level `DefId` is the definition reached by paths that name this
/// declaration. The item-level `HirId` exists for analyses that attach facts to
/// the declaration node itself rather than to its body expressions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirItem {
    /// Item-level analysis identity, distinct from the declaration's `DefId`.
    pub id: HirId,

    /// Definition reached by paths that name this declaration.
    pub def_id: DefId,

    /// Source-level annotations attached to this top-level declaration.
    pub annotations: Vec<HirAnnotation>,
    pub kind: HirItemKind,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Declarations
// ---------------------------------------------------------------------------

/// Source-level declaration taxonomy preserved after lowering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirItemKind {
    /// Concrete or declared function.
    Function(Box<HirFunction>),

    /// Genus-like product type with fields and methods.
    Struct(HirStruct),

    /// Sum type with named variants.
    Enum(HirEnum),

    /// Behavioral contract with method signatures but no bodies.
    Interface(HirInterface),

    /// Alias from a source name to a resolved semantic type.
    TypeAlias(HirTypeAlias),

    /// Relational-view schema declaration: partial, ordered, type-level.
    Schema(HirSchema),

    /// Top-level constant whose value remains an expression for typecheck.
    Constant(HirConst),

    /// Import declaration retained for diagnostics and public surface tracking.
    Import(HirImport),
}

/// Normalized declaration metadata after lowering.
///
/// All `@` surfaces lower to this record. Downstream passes should prefer
/// `HirAnnotation` over reparsing AST `AnnotationKind` variants or token payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirAnnotation {
    /// Annotation family (`optio`, `futura`, `radix`, …).
    pub family: Symbol,

    /// Canonical field records for structured and promoted families.
    pub fields: Vec<HirAnnotationField>,

    /// Token payload for unpromoted `AnnotationKind::Statement` families.
    pub raw_args: Vec<Token>,

    /// Whether the source authored the braced record form (`@ futura { }`)
    /// as opposed to the bare marker (`@ futura`). Empty-body re-emission
    /// keeps the authored shape; the serde default preserves braced output
    /// for artifacts serialized before the flag existed.
    #[serde(default = "annotation_brace_authored_default")]
    pub brace_authored: bool,

    /// Resolved annotation-contract definition when this application named a contract.
    ///
    /// Compiler-owned families leave this `None`. Target/framework consumers should
    /// match on `contract_id` when present instead of reparsing family strings.
    pub contract_id: Option<DefId>,
    pub span: Span,
}

impl HirAnnotation {
    /// Build a non-contract annotation with empty raw args.
    #[must_use]
    pub fn with_fields(family: Symbol, fields: Vec<HirAnnotationField>, span: Span) -> Self {
        Self {
            family,
            fields,
            raw_args: Vec::new(),
            brace_authored: true,
            contract_id: None,
            span,
        }
    }

    /// The route of a compiler-owned `@ ad '<route>'` handler annotation, or
    /// `None` for every other family. Lowering records the route as the
    /// family `ad` with one `route` string field.
    #[must_use]
    pub fn ad_route(&self, interner: &radix_lexer::Interner) -> Option<Symbol> {
        if interner.resolve(self.family) != "ad" {
            return None;
        }
        self.fields.iter().find_map(|field| match field.value {
            HirAnnotationValue::String(route) if interner.resolve(field.name) == "route" => {
                Some(route)
            }
            _ => None,
        })
    }
}

impl HirAnnotation {
    /// The safety tier of an `@ nucleum` annotation: the tier word the author
    /// wrote, or [`KernelTier::Default`] for every other family and for an
    /// annotation with no tier. Lowering records the tier as one `tier`
    /// field holding its Latin keyword identity.
    #[must_use]
    pub fn kernel_tier(&self, interner: &radix_lexer::Interner) -> radix_syntax::KernelTier {
        if interner.resolve(self.family) != "nucleum" {
            return radix_syntax::KernelTier::Default;
        }
        self.fields
            .iter()
            .find_map(|field| match field.value {
                HirAnnotationValue::Symbol(identity) if interner.resolve(field.name) == "tier" => {
                    radix_syntax::KernelTier::from_identity(interner.resolve(identity))
                }
                _ => None,
            })
            .unwrap_or_default()
    }
}

/// The declared kernel tier of a function from its annotations.
#[must_use]
pub fn kernel_tier_of(
    annotations: &[HirAnnotation],
    interner: &radix_lexer::Interner,
) -> radix_syntax::KernelTier {
    annotations
        .iter()
        .map(|annotation| annotation.kernel_tier(interner))
        .max()
        .unwrap_or_default()
}

fn annotation_brace_authored_default() -> bool {
    true
}

/// One named field in a normalized annotation record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirAnnotationField {
    pub name: Symbol,
    pub value: HirAnnotationValue,
    pub span: Span,
}

/// Field payload for normalized annotations.
///
/// Compiler-owned families use `Expr`/`Type`/`Symbol`/`String`/`Bool` as today.
/// Resolved annotation contracts prefer the explicit constant arms (`Int`,
/// `Float`, `Nihil`) so consumers do not re-walk expression trees.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirAnnotationValue {
    Expr(Box<HirExpression>),
    Type(TypeId),
    Symbol(Symbol),
    String(Symbol),
    Bool(bool),
    /// Integer constant from a validated annotation contract payload.
    Int(i64),
    /// Floating constant from a validated annotation contract payload.
    Float(f64),
    /// Explicit `nihil` for optional contract fields.
    Nihil,
}

/// Nucleus device classification for `@ nucleum` items.
///
/// Classification only, not a launch authority: since the DFV2-4 device-call
/// composition contract, launchability is decided by visibility (`@ nucleum
/// @ publica` is an entry; an unmarked `@ nucleum` is a module-internal
/// helper), and composition keys on device callers calling device callees
/// rather than on this role. `Fragment` survives as the never-launchable
/// composition-sugar marker the survivor scan proves standalone symbols
/// against. Graphics `@ fragment` remains [`HirFunction::is_fragment`] and is
/// not this type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NucleumRole {
    /// `@ nucleum` — device item; dispatch/reflection/`DeviceProgram`
    /// eligibility is additionally gated by visibility at the consumer.
    Entry,
    /// `@ nucleum fragment` — composition sugar, never a standalone dispatch
    /// stage.
    Fragment,
}

/// Function signature and optional lowered body.
///
/// Function HIR is shared by top-level functions and methods. A missing body
/// means the source declared a signature-only surface, such as an interface
/// method. A missing return type means the source did not force one and
/// typecheck must infer or validate the effective return.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct HirFunction {
    pub name: Symbol,
    pub type_params: Vec<HirTypeParam>,
    pub params: Vec<HirParam>,

    /// CLI argument bundle synthesized for mounted command entrypoints.
    pub cli_args: Option<HirParam>,

    /// Return type; `None` until typecheck infers it.
    pub ret_ty: Option<TypeId>,

    /// Recoverable alternate-exit type declared with `⇥`.
    pub err_ty: Option<TypeId>,

    /// Function body; `None` for interface methods.
    pub body: Option<HirBlock>,

    /// Canonical callable posture (four-cell identity).
    pub posture: CallablePosture,

    /// Async callable (`fiet` / `futura`).
    pub is_async: bool,

    /// Generator callable (`fiunt`/`fient` / `cursor`).
    pub is_generator: bool,

    /// Device classification (`@ nucleum`), shared by every device lane; the
    /// [`NucleumRole::Entry`] half. Launch eligibility is the visibility
    /// gate, not this bit.
    pub is_kernel: bool,

    /// `@ nucleum fragment` composition sugar; the never-launchable
    /// [`NucleumRole::Fragment`] half. Mutually exclusive with
    /// [`Self::is_kernel`] and with graphics stage flags.
    pub nucleum_fragment: bool,

    /// Graphics vertex shader entry (`@ vertex`).
    pub is_vertex: bool,

    /// Graphics fragment shader entry (`@ fragment`).
    pub is_fragment: bool,

    /// Structured metadata for lowered Faber test cases.
    pub test: Option<HirTestMetadata>,
}

impl HirFunction {
    /// Exclusive nucleus classification, or `None` when unset or when both
    /// bits are set. Device classification only — never consult this as a
    /// launch or composition authority.
    #[must_use]
    pub fn nucleum_role(&self) -> Option<NucleumRole> {
        match (self.is_kernel, self.nucleum_fragment) {
            (true, false) => Some(NucleumRole::Entry),
            (false, true) => Some(NucleumRole::Fragment),
            _ => None,
        }
    }
}

/// Lowered test metadata attached to the generated test function item.
///
/// The function still behaves like a normal HIR function for traversal and
/// typecheck. This side channel preserves source-level test taxonomy for
/// harness/codegen decisions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirTestMetadata {
    /// Display/test name after lowering.
    pub name: Symbol,

    /// Suite nesting path used by generated test output.
    pub suite_path: Vec<Symbol>,

    /// Source modifiers that affect scheduling or selection.
    pub modifiers: Vec<HirTestModifier>,
    pub span: Span,
}

/// Test modifier taxonomy preserved for harness selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirTestModifier {
    /// Coarse expected-error marker; the proba runner owns its strict-xfail policy.
    Erratur,

    /// Skip marker with the source reason/label.
    Omitte(Symbol),

    /// Async/future marker with source payload.
    Futurum(Symbol),

    /// Run only this test.
    Solum,

    /// Arbitrary tag used by test selection.
    Tag(Symbol),

    /// Timeout or duration-like modifier, stored as the parsed numeric value.
    Temporis(i64),

    /// Measurement/benchmark marker.
    Metior,

    /// Repeat-count modifier.
    Repete(i64),

    /// Allowed flake/failure tolerance marker.
    Fragilis(i64),

    /// Suite-scoped only marker.
    SolumIn(Symbol),
}

/// Generic parameter introduced by an item or callable signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirTypeParam {
    pub def_id: DefId,

    /// Type vs compile-time size/index parameter.
    pub kind: GenericParamKind,
    pub name: Symbol,
    pub span: Span,

    /// Source-declared admission rule for concrete type arguments.
    pub constraint: HirTypeParamConstraint,

    /// Declaration-time `=` default; the variant always matches `kind` because
    /// the parser enforces the pairing at the source spelling.
    pub default: Option<SemanticGenericParamDefault>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirTypeParamConstraint {
    Any,
    OneOf(Vec<TypeId>),
    /// Contract bounds written `T implet C<…> ∩ D<…>` (DECISIONS D1.1).
    Implet(Vec<ContractRef>),
}

impl HirTypeParam {
    /// Whether this parameter lowers to a backend type parameter.
    #[must_use]
    pub fn is_typus(&self) -> bool {
        self.kind == GenericParamKind::Typus
    }

    /// Type parameters only — size/index parameters are compile-time facts for a later phase.
    pub fn typus_params(params: &[Self]) -> impl Iterator<Item = &Self> {
        params.iter().filter(|param| param.is_typus())
    }

    /// Semantic descriptor for this declared parameter.
    #[must_use]
    pub fn descriptor(&self) -> GenericParamDescriptor {
        GenericParamDescriptor {
            name: self.name,
            sort: match self.kind {
                GenericParamKind::Typus => GenericParamSort::Typus,
                GenericParamKind::Magnitudo => GenericParamSort::Magnitudo,
            },
            constraint: match &self.constraint {
                HirTypeParamConstraint::Any => TypeParamConstraint::Any,
                HirTypeParamConstraint::OneOf(allowed) => {
                    TypeParamConstraint::OneOf(allowed.clone())
                }
                HirTypeParamConstraint::Implet(bounds) => {
                    TypeParamConstraint::Implet(bounds.clone())
                }
            },
            default: self.default.clone(),
        }
    }

    /// Ordered semantic descriptors for one declaration's generic row.
    #[must_use]
    pub fn descriptors(params: &[Self]) -> Vec<GenericParamDescriptor> {
        params.iter().map(Self::descriptor).collect()
    }
}

/// Function, method, closure, or CLI parameter binding.
///
/// Parameter types are resolved during lowering because Faber parameters are
/// type-first source syntax. Call-site optionality and provider-slot markers are
/// distinct source contracts and are kept as separate flags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirParam {
    pub def_id: DefId,
    pub name: Symbol,
    pub ty: TypeId,
    pub mode: HirParamMode,

    /// Whether callers may omit this argument.
    ///
    /// This includes source `sponte` parameters and parameters with defaults;
    /// `sponte` below records only the explicit source marker.
    pub optional: bool,

    /// Post-name declaration marker: the slot is voluntary (not required from provider).
    pub sponte: bool,

    /// Optional source-level default supplied with `vel`.
    pub default: Option<HirExpression>,
    pub span: Span,
}

/// Source ownership/borrowing markers preserved through HIR lowering.
///
/// Semantic analysis maps these to [`radix_types::SemanticParamMode`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum HirParamMode {
    /// Default value-passing mode when no ownership marker is written.
    Owned,

    /// `de`: shared reference parameter.
    De,

    /// `in`: mutable reference parameter.
    In,

    /// `ex`: explicit move parameter.
    Ex,

    /// `axis`: an owned number that names an axis. Passed by value like
    /// `Owned`; the marker word is signature presentation, not a type.
    Axis,
}

/// Product type declaration with fields, methods, and declared contracts.
///
/// HIR stores interface references as `DefId` values so later checks can
/// reason about conformance without reopening lexical scopes. Class
/// inheritance does not exist (DECISIONS D8.1): shared behaviour is a contract
/// (`implet`), shared data is composition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirStruct {
    pub name: Symbol,
    pub type_params: Vec<HirTypeParam>,

    /// Instance/static fields in source order.
    pub fields: Vec<HirField>,
    pub methods: Vec<HirMethod>,

    /// Resolved contracts this type declares in its `implet` clause, each
    /// with its type arguments (`implet Orderable<Persona>`, D1.2).
    pub implements: Vec<ContractRef>,
}

/// Field declaration inside a product type.
///
/// Field type is already a resolved semantic type reference. Initializer
/// expressions remain HIR because typecheck still owns compatibility and
/// default-value validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirField {
    pub def_id: DefId,
    pub name: Symbol,
    pub ty: TypeId,

    /// Authored leading modifier (DECISIONS D16.1): `sit` (read-only per
    /// instance), `varia`, or `fixum` (a type-level constant, not per instance). `None` is an
    /// unmarked field, legal until the D5c tightening.
    pub modifier: Option<FieldModifier>,

    /// Post-name declaration marker: the field is voluntary in object literals / construction.
    pub sponte: bool,
    pub annotations: Vec<HirAnnotation>,

    /// Optional source initializer/default expression.
    pub init: Option<HirExpression>,
    pub span: Span,
}

impl HirField {
    /// Whether this field belongs to the type (a `fixum` constant member) rather than instances.
    #[must_use]
    pub fn is_static(&self) -> bool {
        self.modifier == Some(FieldModifier::Const)
    }

    /// Whether this field is `sit` (read-only): set only by construction (D16.3).
    #[must_use]
    pub fn is_fixum(&self) -> bool {
        self.modifier == Some(FieldModifier::Fixum)
    }
}

/// Method declaration paired with receiver metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirMethod {
    /// Method definition identity reached by method lookup.
    pub def_id: DefId,

    /// Shared function payload for the method signature and body.
    pub func: HirFunction,

    /// Source-level annotations attached to this member declaration.
    pub annotations: Vec<HirAnnotation>,

    /// Receiver mode lowered from method syntax.
    pub receiver: HirReceiver,
    pub span: Span,
}

/// Receiver mode for methods after source lowering.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum HirReceiver {
    /// Static method with no instance receiver.
    None,

    /// Shared receiver.
    Ref,

    /// Mutable receiver.
    MutRef,

    /// Owned receiver.
    Owned,
}

/// Sum type declaration with resolved variant definitions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirEnum {
    pub name: Symbol,
    pub type_params: Vec<HirTypeParam>,
    /// Fields declared in the union's `@ commune` region.
    ///
    /// Backends prepend these fields to every variant payload. Keeping the
    /// declaration separate from variant-owned fields lets emitters recognize
    /// unconditional member access without inferring a shared contract from
    /// coincidentally identical variant shapes.
    #[serde(default)]
    pub commune_fields: Vec<HirVariantField>,
    pub variants: Vec<HirVariant>,
}

/// Enum variant declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirVariant {
    pub def_id: DefId,
    pub name: Symbol,

    /// Positional or named fields carried by the variant.
    pub fields: Vec<HirVariantField>,
    pub span: Span,
}

/// Field carried by an enum variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirVariantField {
    /// Field name when the variant uses named payloads.
    pub name: Symbol,
    pub ty: TypeId,
    pub span: Span,
}

/// Interface declaration whose methods define a callable contract.
///
/// Interface methods intentionally carry signatures only; implementation
/// matching is a later semantic check against these resolved parameter and
/// return type references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirInterface {
    pub name: Symbol,
    pub type_params: Vec<HirTypeParam>,

    /// Required method signatures.
    pub methods: Vec<HirInterfaceMethod>,
}

/// Signature required by an interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirInterfaceMethod {
    /// Contract-method identity. A call through a type-parameter bound
    /// (D1.1) targets this id until monomorphization picks the witness's
    /// own method.
    pub def_id: DefId,
    /// Target availability metadata. Empty means available for every target.
    pub availability: Vec<HirAvailability>,
    pub name: Symbol,
    pub params: Vec<HirParam>,

    /// Declared return type, or `None` when typecheck must infer/validate absence.
    pub ret_ty: Option<TypeId>,

    /// Declared recoverable alternate-exit type.
    pub err_ty: Option<TypeId>,

    /// Canonical callable posture (four-cell identity).
    pub posture: CallablePosture,
    pub span: Span,
}

/// Availability gate attached to a declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirAvailability {
    /// Declaration exists in the source interface, but is not implemented yet.
    Nondum {
        /// Target token such as `rs`, when the gate is target-specific.
        target: Option<Symbol>,

        /// Optional human explanation from the annotation string payload.
        reason: Option<Symbol>,
    },
}

/// Source alias from a name to a resolved semantic type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirTypeAlias {
    pub name: Symbol,
    pub type_params: Vec<HirTypeParam>,
    pub ty: TypeId,
}

/// Relational-view schema declaration: `schema Name { columna<T> name [: source] … }`.
///
/// A schema is a partial application-owned relational heading
/// (docs/design/relational-views.md §2): it names only the columns the
/// application needs, and extra source columns stay invisible. The name is a
/// type-namespace identity and is **never** lowered to MIR or any backend
/// leaf. Each column carries five-field metadata (public name, source mapping,
/// Faber type, required/nullability, canonical order) as an ordered row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirSchema {
    /// Schema name (type-namespace identity; never a runtime value).
    pub name: Symbol,

    /// Ordered per-column metadata in canonical source order.
    pub columns: Vec<HirSchemaColumn>,
}

/// One ordered column of a lowered schema — the five-field metadata row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirSchemaColumn {
    /// Schema-facing public name; result rows are keyed by it.
    pub name: Symbol,

    /// Resolved source-column identity: the `name : source` alias row, or the
    /// public name when the alias is absent (identity mapping).
    pub source: Symbol,

    /// Faber type contract as written (`T` or `T ∪ nihil` in the type
    /// position).
    pub ty: TypeId,

    /// Whether the column is nullable — derived from `T ∪ nihil` in the type
    /// position (the lowered type is `Option<T>`).
    pub nullable: bool,

    /// Canonical column order (source order index).
    pub order: u32,
}

/// Top-level constant declaration.
///
/// The value remains an expression because constant evaluation and type
/// compatibility are later-phase responsibilities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirConst {
    pub name: Symbol,

    /// Explicit constant type, when declared.
    pub ty: Option<TypeId>,
    pub value: HirExpression,

    /// Whether the source declaration permits mutation.
    pub mutable: bool,

    /// Whether the initializer should be awaited.
    pub is_await: bool,

    /// Present for a module-level constant (`const T X = e` / la `fixum`,
    /// DECISIONS D5.8-D5.10, st1); absent for a top-level `fixum`/`varia` store
    /// (SEM062). After semantic analysis its `value` is the compile-time
    /// folded constant.
    #[serde(default)]
    pub statica: Option<HirStatic>,
}

/// Module-constant facts for a top-level [`HirConst`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirStatic {
    /// The authored initializer, kept only when constant folding replaced
    /// `value`. Faber re-emission prints it so formatting never rewrites the
    /// source; every other consumer reads the folded `value`.
    pub authored: Option<Box<HirExpression>>,
}

/// Import declaration retained after package/library resolution.
///
/// Import items introduce definitions into the local semantic surface. Keeping
/// the declaration in HIR lets later diagnostics and generated metadata refer
/// back to the source import instead of losing that boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirImport {
    pub path: Symbol,
    pub visibility: Visibility,

    /// Named items introduced by the import.
    pub items: Vec<HirImportItem>,

    /// EOT-2 selective import (`importa ex "…" fixum member [ut local], …`).
    ///
    /// When true, `items` are ordered caller-owned value bindings (functions
    /// and consts), not namespace members: `name` is the exported spelling and
    /// `alias` the caller-owned local when one was spelled. Legacy namespace
    /// imports keep `false` and their existing item shape.
    #[serde(default)]
    pub selective: bool,

    /// Target word of a target-native import (`import python "math" m`).
    ///
    /// When set, `path` is the target's own module specifier, copied
    /// verbatim, and every item binds a trusted `ignotum` value. The program
    /// is bound to that target.
    #[serde(default)]
    pub target: Option<Symbol>,
}

/// One binding introduced by an import declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirImportItem {
    /// Definition identity of the imported binding in this program.
    pub def_id: DefId,

    /// Exported/source name.
    pub name: Symbol,

    /// Local alias, when the import renames the item.
    pub alias: Option<Symbol>,
}

// ---------------------------------------------------------------------------
// Blocks and statements
// ---------------------------------------------------------------------------

/// Breakable scope kind for [`HirBlock::breakable`].
///
/// Loops (`dum` / `itera` / `fac … dum`) use expression-level lowering and do
/// not set this field. `fac` statement and closure bodies do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirBreakableKind {
    /// `fac { }` try body (not the `cape` handler).
    Fac,
    /// `fac omnia { }` try body: same breakable semantics as [`Fac`]; the
    /// marker is surface-only and rides HIR so the re-emitter round-trips it.
    FacOmnia,
    /// Marker on the synthesized loop block of `fac omnia { } … dum cond`. It
    /// is **not** a breakable scope ([`HirBreakableKind::is_scope`] is false):
    /// the loop keeps plain `fac … dum` semantics in every backend and the
    /// marker only lets the re-emitter print `omnia`.
    FacOmniaLoop,
    /// `fac machina { }` (en `do device`) try body: a checked device-resident
    /// region. It is a breakable scope like [`Fac`] and lowers exactly as
    /// [`FacOmnia`] in V0.
    FacMachina,
    /// Marker on the synthesized loop block of `fac machina { } … dum cond`;
    /// like [`HirBreakableKind::FacOmniaLoop`] it is not a breakable scope.
    FacMachinaLoop,
}

impl HirBreakableKind {
    /// Whether the block is a breakable scope that `rumpe` exits (every kind
    /// except the loop markers [`HirBreakableKind::FacOmniaLoop`] and
    /// [`HirBreakableKind::FacMachinaLoop`]).
    #[must_use]
    pub fn is_scope(self) -> bool {
        !matches!(self, Self::FacOmniaLoop | Self::FacMachinaLoop)
    }

    /// Latin keyword key of the modifier the authored `fac` carried, for the
    /// re-emitter (`omnia` or `machina`); `None` for a plain `fac`.
    #[must_use]
    pub fn fac_modifier_key(self) -> Option<&'static str> {
        match self {
            Self::Fac => None,
            Self::FacOmnia | Self::FacOmniaLoop => Some("omnia"),
            Self::FacMachina | Self::FacMachinaLoop => Some("machina"),
        }
    }

    /// Whether launches inside the block defer their faults to the block's
    /// exit (`omnia` and `machina` alike).
    #[must_use]
    pub fn defers_faults(self) -> bool {
        self.fac_modifier_key().is_some()
    }

    /// Whether the block is a device region (`fac machina`).
    #[must_use]
    pub fn is_device_region(self) -> bool {
        matches!(self, Self::FacMachina | Self::FacMachinaLoop)
    }

    /// Whether the kind is one of the synthesized loop-block markers.
    #[must_use]
    pub fn is_loop_marker(self) -> bool {
        !self.is_scope()
    }
}

/// Ordered statement block with an optional trailing expression.
///
/// HIR keeps expression-position blocks and statement-position blocks in one
/// shape. Typecheck decides whether the trailing expression contributes a value
/// in its context.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HirBlock {
    pub statements: Vec<HirStatement>,

    /// Optional tail expression.
    pub expr: Option<Box<HirExpression>>,
    pub span: Span,

    /// When set, `rumpe` may exit this block before its syntactic end.
    pub breakable: Option<HirBreakableKind>,

    /// Structural end anchor for an authored executable block.
    ///
    /// Present only when the block was lowered from an authored executable
    /// `BlockStmt` boundary; compiler-synthesized blocks carry no authored
    /// end. The end is presentation structure, not a statement: visitors must
    /// never count it or recurse into it, and it never lowers to a no-op.
    pub end: Option<HirBlockEnd>,
}

impl HirBlock {
    /// Block with no breakable scope (default for most lowered blocks).
    #[must_use]
    pub fn simple(
        statements: Vec<HirStatement>,
        expr: Option<Box<HirExpression>>,
        span: Span,
    ) -> Self {
        Self {
            statements,
            expr,
            span,
            breakable: None,
            end: None,
        }
    }

    /// Split a [`HirExpressionKind::Loop`] block into its `fac … dum` parts.
    ///
    /// `fac { body } dum c` is the only producer of `Loop`: it lowers to the
    /// body statements followed by a synthesized test `si non c { rumpe }`.
    /// Returns `(body, test, c)` when the block ends in that test. Ruling R3:
    /// `perge` in the body lands on `test`, never on the body head.
    #[must_use]
    pub fn fac_dum_parts(&self) -> Option<(&[HirStatement], &HirStatement, &HirExpression)> {
        let (test, body) = self.statements.split_last()?;
        let HirStatementKind::Expr(expr) = &test.kind else {
            return None;
        };
        let HirExpressionKind::Si {
            cond,
            then_block,
            then_catch: None,
            else_block,
        } = &expr.kind
        else {
            return None;
        };
        if else_block
            .as_ref()
            .is_some_and(|block| !block.statements.is_empty() || block.expr.is_some())
        {
            return None;
        }
        let HirExpressionKind::Unary(HirUnOp::Not, inner) = &cond.kind else {
            return None;
        };
        if then_block.expr.is_some()
            || !matches!(then_block.statements.as_slice(), [only] if matches!(only.kind, HirStatementKind::Rumpe))
        {
            return None;
        }
        Some((body, test, inner))
    }
}

/// Statement node with stable analysis identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirStatement {
    /// Statement identity for analysis tables.
    pub id: HirId,
    pub kind: HirStatementKind,
    pub span: Span,
}

/// Statement taxonomy after syntax normalization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirStatementKind {
    /// Local binding declaration.
    Local(HirLocal),

    /// Expression used for effects.
    Expr(HirExpression),

    /// Function return with optional value.
    Redde(Option<HirExpression>),

    /// End the current lane iteration, without returning from the function.
    ReddeFilum(Option<HirExpression>),

    /// Loop break.
    Rumpe,

    /// Loop continue.
    Perge,

    /// Explicit no-op.
    Tacet,

    /// Postfix increment/decrement statement (`place ↑` / `place ↓`).
    IncDec(HirIncDec),

    /// Source-ordered guard section (`custodi { si ... }`).
    Custodi(HirCustodi),
}

/// Guard section made of sequential early-exit clauses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirCustodi {
    pub clauses: Vec<HirCustodiClause>,
}

/// One `si` clause inside a `custodi` section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirCustodiClause {
    pub cond: HirExpression,

    /// Body executed when the guard condition is true.
    pub body: HirBlock,
    pub span: Span,
}

/// Postfix increment or decrement of a mutable `numerus` place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirIncDec {
    /// Assignment target evaluated once for read-modify-write.
    pub target: HirExpression,

    /// Increment (`↑`) or decrement (`↓`).
    pub op: HirIncDecOp,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum HirIncDecOp {
    Inc,
    Dec,
}

/// Local binding introduced inside a block.
///
/// The optional type distinguishes explicit annotations from inference. The
/// initializer remains an expression so typecheck can establish the final local
/// type from annotation, initializer, or both.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirLocal {
    pub def_id: DefId,
    pub name: Symbol,

    /// Explicit local type annotation, if present.
    pub ty: Option<TypeId>,

    /// Initializer expression, if present.
    pub init: Option<HirExpression>,

    /// Whether the source declaration permits mutation.
    pub mutable: bool,

    /// Whether the initializer should be awaited.
    pub is_await: bool,

    /// Binding is initialized by runtime/entry effects before any user statement.
    pub runtime_provided: bool,

    /// Set by typecheck on an await-binding whose awaited value narrows into
    /// the declared slot (d11-6 §3(a) row 2). The stored value is the awaited
    /// result, which has no expression node, so the store rides the local:
    /// after the await, the value is checked against the slot under its
    /// policy (trap, `wrapping` reduce, `saturating` clamp). `None` when the
    /// result is assignable to the slot.
    #[serde(deserialize_with = "required_option")]
    pub await_store: Option<HirStoreSite>,

    /// Source declared this binding with the `size` (la `magnitudo`) form.
    /// Presentation-only: the Faber re-emitter prints the declaration
    /// spelling instead of an ordinary immutable binding.
    #[serde(default)]
    pub size_decl: bool,
    /// Static `const size N = 3` (compile-time natural) versus the runtime
    /// `←` lift; only meaningful when `size_decl` is set. Serde-defaulted so
    /// older interface snapshots keep loading.
    #[serde(default)]
    pub size_static: bool,
}

impl HirLocal {
    /// The stored value of a narrowing await-binding, as one expression a
    /// backend can emit through its ordinary checked-narrowing path:
    /// `Conversio { implicit, source: Reddet(init) }` targeting the slot.
    /// `None` when the local carries no [`HirLocal::await_store`].
    ///
    /// The synthetic nodes reuse the initializer's own node unchanged; their
    /// ids are outside the range the lowering assigns, so no per-node plan
    /// keyed on an id matches them.
    #[must_use]
    pub fn await_store_init(&self, types: &radix_types::TypeTable) -> Option<HirExpression> {
        let site = self.await_store.clone()?;
        let init = self.init.as_ref()?;
        let slot = self.ty?;
        let future = init.ty?;
        let awaited = match types.get(future) {
            radix_types::Type::Promissum(inner)
            | radix_types::Type::PromissumFailable(inner, _) => *inner,
            _ => return None,
        };
        let awaited_expr = HirExpression {
            id: HirId(u32::MAX),
            kind: HirExpressionKind::Reddet(Box::new(init.clone())),
            ty: Some(awaited),
            span: init.span,
        };
        Some(HirExpression {
            id: HirId(u32::MAX - 1),
            kind: HirExpressionKind::Conversio {
                source: Box::new(awaited_expr),
                target: HirConversioTarget::Type(slot),
                params: Vec::new(),
                recovery: None,
                route: None,
                implicit: Some(site),
            },
            ty: Some(slot),
            span: init.span,
        })
    }
}

/// Expression node with stable identity, optional type facts, and source span.
///
/// The `ty` slot is analysis-owned. Lowering may seed it when the expression
/// syntax carries an unavoidable type reference, but downstream consumers should
/// expect `None` until typecheck has run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirExpression {
    /// Expression identity for analysis tables.
    pub id: HirId,
    pub kind: HirExpressionKind,

    /// Type established by typecheck, or absent before analysis.
    pub ty: Option<TypeId>,
    pub span: Span,
}

impl HirExpression {
    /// Whether evaluating this expression can have no observable effect —
    /// no call, no trap, no allocation-visible state: a literal, a resolved
    /// path, or a unary operator over a literal (`-1`).
    ///
    /// WHY: short-circuiting operators (`vel`) may evaluate such an operand
    /// eagerly without changing program meaning, which keeps backends free
    /// to emit a single select/`unwrap_or` for the common constant fallback
    /// instead of a branch.
    #[must_use]
    pub fn is_effect_free_operand(&self) -> bool {
        match &self.kind {
            HirExpressionKind::Literal(_) | HirExpressionKind::Path(_) => true,
            HirExpressionKind::Unary(_, operand) => {
                matches!(operand.kind, HirExpressionKind::Literal(_))
            }
            _ => false,
        }
    }
}

/// Key expressions of a grouped reduction's `per` / `pro` clause.
///
/// WHY: `per [i, j]` is a bracketed coordinate list and `per k` is the
/// one-key case, so `per k` and `per [k]` name the same single key. A spread
/// element yields its inner expression; typecheck refuses it, so later passes
/// only see plain keys.
#[must_use]
pub fn reduction_group_keys(group: &HirExpression) -> Vec<&HirExpression> {
    match &group.kind {
        HirExpressionKind::Array(elements) => elements
            .iter()
            .filter_map(|element| match element {
                HirArrayElement::Expr(expr) | HirArrayElement::Spread(expr) => Some(expr),
                HirArrayElement::Infer(_) => None,
            })
            .collect(),
        _ => vec![group],
    }
}

/// Indices into `coordinates` of the coordinates a grouped reduction reduces.
///
/// WHY: a source coordinate is KEPT when it appears as a plain key, every
/// other coordinate is REDUCED. A bare coordinate name such as `pro [i]`
/// lowers to `HirExpressionKind::Path(DefId)`; a composed key (an index
/// expression, a field read) names no coordinate and keeps them all. The
/// returned indices are ascending.
#[must_use]
pub fn reduction_reduced_coordinates(
    coordinates: &[(DefId, Symbol)],
    group: &HirExpression,
) -> Vec<usize> {
    let keys = reduction_group_keys(group);
    coordinates
        .iter()
        .enumerate()
        .filter_map(|(index, (coordinate, _name))| {
            let kept = keys.iter().any(|key| {
                matches!(
                    key.kind,
                    HirExpressionKind::Path(key_def) if key_def == *coordinate
                )
            });
            (!kept).then_some(index)
        })
        .collect()
}

/// Function or method call argument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirCallArg {
    /// Source field name for named construction forms such as `finge`.
    pub name: Option<Symbol>,

    /// Whether the source argument used `sparge`.
    pub spread: bool,

    /// Call-site access marker written on this argument, when present.
    #[serde(default)]
    pub marker: Option<HirArgMarker>,
    pub expr: HirExpression,
    pub span: Span,
}

/// Access marker written at a call argument (`ref x`, `mut x`, `own x`, `copy x`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirArgMarker {
    /// `ref` / `de`: the explicit spelling of the read-only default.
    Ref,
    /// `mut` / `in`.
    Mut,
    /// `own`.
    Own,
    /// `copy`.
    Copy,
    /// `axis`: the number names an axis. A second kind of marker (what a
    /// number means, not how a value is passed).
    Axis,
}

/// Recoverable error handler binding and body.
///
/// `cape` introduces a binding for the recovered value. HIR records that binding
/// as a normal definition so visitors and typecheck see the handler scope
/// without special-case name resolution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirCape {
    /// Definition introduced for the caught value.
    pub binding_def_id: DefId,
    pub binding_name: Symbol,

    /// Declared or inferred binding type.
    pub binding_ty: Option<TypeId>,
    pub body: HirBlock,
    pub span: Span,
}

// ---------------------------------------------------------------------------
// Expressions
// ---------------------------------------------------------------------------

/// Expression taxonomy after AST-to-HIR normalization.
///
/// Variants preserve language intent rather than backend strategy. For example,
/// optional chaining, non-null assertions, conversion, and recoverable handling
/// remain explicit here so typecheck can enforce Faber semantics before MIR or
/// codegen chooses a target representation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirExpressionKind {
    /// Resolved path to a definition.
    Path(DefId),
    Literal(HirLiteral),

    /// Contextual empty value: `vacua`.
    Vacua,
    Binary(HirBinOp, Box<HirExpression>, Box<HirExpression>),
    Unary(HirUnOp, Box<HirExpression>),
    Call(
        Box<HirExpression>,
        Vec<TypeId>,
        Vec<HirCallArg>,
        /// Declaration-owned conversion arm selected for function-level propagation.
        Option<DefId>,
    ),

    /// Postfix gradient selection: `call(...) ∇` or `call(...) ∇ [places]`.
    ///
    /// Dedicated node, not a unary operation: it preserves the forward
    /// expression and the ordered selector expressions whose adjoints are
    /// requested. The bare form has an empty selector list (every eligible
    /// argument, in call order). Semantic analysis resolves the call to an
    /// eligible differentiable forward and the selectors to direct call
    /// argument places.
    Gradient {
        call: Box<HirExpression>,
        selected_arguments: Vec<HirExpression>,
    },

    /// Method call normalized from receiver syntax.
    MethodCall(
        Box<HirExpression>,
        Symbol,
        Vec<TypeId>,
        Vec<HirCallArg>,
        /// Declaration-owned conversion arm selected for function-level propagation.
        Option<DefId>,
    ),
    Field(Box<HirExpression>, Symbol),
    Index(Box<HirExpression>, Box<HirExpression>),

    /// Optional chaining for null-safe member, index, or call access.
    OptionalChain(Box<HirExpression>, HirOptionalChainKind),

    /// Non-null assertion for member, index, or call access.
    NonNull(Box<HirExpression>, HirNonNullKind),

    Block(HirBlock),

    /// Conditional expression or statement form.
    Si {
        cond: Box<HirExpression>,
        then_block: HirBlock,
        then_catch: Option<Box<HirCape>>,
        else_block: Option<HirBlock>,
    },

    /// Pattern or value dispatch (`discerne`, or `elige` normalized to one subject).
    ///
    /// `exhaustive` carries the source-level `omnia` marker: when set, the
    /// exhaustiveness pass forbids catchall arms and requires a single enum
    /// scrutinee so every variant is named explicitly. `elige` always sets
    /// this to `false` (value dispatch has no variant set to enumerate).
    Discerne {
        scrutinees: Vec<HirExpression>,
        arms: Vec<HirCasuArm>,
        exhaustive: bool,
    },

    /// Infinite loop.
    Loop(HirBlock),

    /// While loop.
    Dum(Box<HirExpression>, HirBlock),

    /// Iteration loop with coordinate and element bindings introduced by the loop head.
    Itera {
        /// Iteration source mode.
        mode: HirIteraMode,
        /// Coordinate bindings from the optional `apud` clause.
        coordinates: Vec<(DefId, Symbol)>,
        /// Optional lane binder from the `filum` clause. Declared metadata
        /// only: the runner lowers the plain loop and ignores the binder.
        filum: Option<(DefId, Symbol)>,
        /// Element binding definition.
        binding: DefId,
        /// Element binding name.
        binding_name: Symbol,
        /// Iterable expression.
        iter: Box<HirExpression>,
        /// Loop body.
        body: HirBlock,
        /// Whether the source declared the element binder `varia`.
        mutable: bool,
    },

    /// Lockstep `itera ex` walk over N >= 2 aligned collections.
    ///
    /// N == 1 lowers through [`HirExpressionKind::Itera`] unchanged; this node
    /// exists only for the comma-separated source series with its matching
    /// name-binder series. Length equality is a fail-closed law: statically
    /// known lengths are compared at typecheck, dynamic lengths are guarded at
    /// the MIR walk admission — the walk never truncates to the minimum.
    IteraLockstep {
        /// Aligned source expressions, one per binder.
        sources: Vec<HirExpression>,
        /// Coordinate binding from the optional shared `apud [i]` clause
        /// (at most one; the count law is enforced at typecheck).
        coordinates: Vec<(DefId, Symbol)>,
        /// Element bindings visible in the body, one per source.
        bindings: Vec<(DefId, Symbol)>,
        /// Body evaluated for each aligned element tuple.
        body: HirBlock,
        /// Whether the source declared the element binders `varia`.
        mutable: bool,
    },

    /// Cartesian `itera ab` range-product walk over N >= 2 declared domains.
    ///
    /// N == 1 lowers through [`HirExpressionKind::Itera`] unchanged. The
    /// ranges and binders keep source order: the first range is the
    /// outermost walk, later ranges nest inward (leftmost-outermost). Range
    /// expressions are lowered outside the binder scope, so an extent may
    /// not shadow a binder name (typecheck declines it). Unlike
    /// [`HirExpressionKind::IteraLockstep`] this is a product, never a zip:
    /// no equal-length law applies and no `apud` clause is carried.
    IteraRange {
        /// Range expressions, one per binder, in source order.
        ranges: Vec<HirExpression>,
        /// Bindings visible in the body, one per range, in source order.
        bindings: Vec<(DefId, Symbol)>,
        /// Body evaluated for every element of the Cartesian product.
        body: HirBlock,
        /// Whether the source declared the range binders `varia`.
        mutable: bool,
    },

    /// Range (`intervallum`).
    Intervallum {
        start: Box<HirExpression>,
        end: Box<HirExpression>,
        step: Option<Box<HirExpression>>,
        kind: HirRangeKind,
    },

    Assign(Box<HirExpression>, Box<HirExpression>),

    /// Value-side `copy` marker on an assignment right side (`b ← copy c`):
    /// the value is a deep duplicate of `expr`. Declaration initializers carry
    /// the marker as the declared type's access mode instead, so this node
    /// only ever appears as the source of an [`HirExpressionKind::Assign`].
    Marked {
        marker: HirArgMarker,
        expr: Box<HirExpression>,
    },

    /// Conversion-directed assignment (`↤`, U+21A4).
    ///
    /// `target ↤ source` evaluates `source`, converts it to the statically
    /// known type of `target` through the `↦` conversion route, then stores
    /// once. `recovery` is the optional `⊥ fallback`, legal only on this form;
    /// the fallback satisfies the same target-type rule as explicit `↦`
    /// recovery. Lowering preserves the operator (never erasing it into
    /// [`HirExpressionKind::Assign`]); typecheck resolves the destination type
    /// from the left place through the ordinary lvalue rules and records it on
    /// this node's `ty`.
    ConversioAssign {
        target: Box<HirExpression>,
        source: Box<HirExpression>,
        recovery: Option<Box<HirExpression>>,
        /// `via HINT` of `a ↤ x via HINT ⊥ d`: the same hint symbols a
        /// `↦ T via HINT` [`HirExpressionKind::Conversio`] carries in `params`
        /// (empty when no `via` is written).
        #[serde(default)]
        params: Vec<Symbol>,
    },

    Array(Vec<HirArrayElement>),

    /// Struct literal whose type name is already resolved.
    Struct(DefId, Vec<(Symbol, HirExpression)>),

    /// Tuple expression, including multiple-return carriers.
    ///
    /// The optional `Vec<TypeId>` holds explicit type arguments lowered from an
    /// `iuncta<A, B>` construction site. When present and no contextual expected
    /// type is available, typecheck uses these instead of synthesizing from
    /// element expressions.
    Tuple(Vec<HirExpression>, Option<Vec<TypeId>>),

    /// Diagnostic output expression.
    Scribe(HirScribeKind, Vec<HirExpression>),

    /// String interpolation expression (`scriptum`).
    Scriptum(Symbol, Vec<HirExpression>),

    /// Read one line from the active input stream (`lege` / `lineam`).
    ReadLine(bool),

    /// Compile-time evaluation marker (`praefixum`).
    Praefixum(Box<HirExpression>),

    /// Build-time file embed (`insere "path"`, D8.10) carrying the authored
    /// path. Legal only inside a compile-time initializer; its type comes
    /// from the slot it fills (`textus` or `octeti`). The post-typecheck
    /// `insere_fold` pass replaces it with the file's literal, so no backend
    /// ever sees this node.
    Insere(Symbol),

    /// Assertion expression.
    Adfirma(Box<HirExpression>, Option<Box<HirExpression>>),

    /// Unrecoverable panic expression.
    Panic(Box<HirExpression>),

    /// Recoverable throw expression (`iace`).
    Throw(Box<HirExpression>),

    /// Structured recoverable-error handler from an attached `cape` clause.
    Handled {
        body: HirBlock,
        catch: Box<HirCape>,
    },

    /// Capture boundary (`capta { ... }`): reifies the error channel into a
    /// value union. The block's trailing expression is the success value;
    /// every alternate exit inside the body (failable call, `iace`) wraps
    /// into the trap carrier instead of propagating. Not sugar over
    /// `Handled` — do/catch is derivable as trap+match, not the reverse.
    Trap(Box<HirBlock>),

    /// Call default `call ⊥ default` (D17.7), desugared through the trap
    /// machinery: `body` is a trap body whose trailing expression is the
    /// failable call. On success the expression is the call's value; when
    /// the call fails through its `⇥` channel, `default` is evaluated (only
    /// then) and becomes the value. `mori` and runtime traps are never
    /// caught. The expression type is the call's success type.
    CallDefault {
        body: Box<HirBlock>,
        default: Box<HirExpression>,
        /// Error channel type the body's failure carries, recorded by
        /// typecheck; backends use it to type the discarded error.
        #[serde(default)]
        err_ty: Option<TypeId>,
    },

    /// Closure with parameters, optional return/error types, and expression
    /// body.
    Clausura(
        Vec<HirParam>,
        Option<TypeId>,
        Option<TypeId>,
        Box<HirExpression>,
    ),

    /// Legacy serialized/imported context-dependent `cede` carrier. New source
    /// lowers `cede value` as `Yield`, while awaited operations use `Reddet`,
    /// `Tacebit`, or await locals.
    Cede(Box<HirExpression>),

    /// `reddet future` — await a promise and return its resolved value (fiet only).
    Reddet(Box<HirExpression>),

    /// `tacebit future` — await a promise to completion and discard the success value.
    Tacebit(Box<HirExpression>),

    /// `cede value` — yield one item from a fiunt/fient stream.
    Yield(Box<HirExpression>),

    /// Static type-ascription expression (via ∷).
    ///
    /// Typed constructors and object literals may still lower through this
    /// internal carrier so backends can reuse target-shaped aggregate logic.
    Verte {
        source: Box<HirExpression>,
        target: TypeId,

        /// Extracted object fields for map/struct construction.
        /// Present when source is an object literal being constructed into a Map or Struct.
        entries: Option<Vec<HirObjectField>>,
    },

    /// Runtime value conversion (`↦ target`).
    ///
    /// Unlike Verte (compile-time cast), this performs actual parsing/conversion
    /// and supports inline recovery values via `⊥`.
    Conversio {
        source: Box<HirExpression>,
        target: HirConversioTarget,

        /// Codegen hint parameters (e.g., `i32`, `Hex` in `↦ i32 via Hex`).
        /// Stored as raw symbols because these are target-specific hints, not Faber types.
        params: Vec<Symbol>,
        recovery: Option<Box<HirExpression>>,

        /// Non-primitive conversion row selected by typecheck, when one
        /// applies: a built-in `ordo` row (D9.4) or a registered
        /// `@ conversio` function (D9.3). `None` means the backend's own
        /// scalar/structural conversion rules decide the lowering.
        #[serde(deserialize_with = "required_option")]
        route: Option<HirConversioRoute>,

        /// `Some` marks an **implicit store node** inserted by typecheck where
        /// an integer value lands in a narrower slot (d11-6 ruling 17(b)).
        /// The node's target type is the slot; the slot's overflow policy
        /// decides the outcome: a trapping slot traps (never routed to `⇥`
        /// or `cape`), a `wrapping` slot reduces, a `saturating` slot clamps.
        /// Authored `↦` sites carry `None` and stay recoverable.
        #[serde(deserialize_with = "required_option")]
        implicit: Option<HirStoreSite>,
    },

    /// Implicit lossless widening of a bounded integer into the unbounded
    /// `inf` (inf-delivery ruling 40, row 1), inserted by typecheck at every
    /// position where a bounded-integer expression lands in an `inf`-typed
    /// slot or is joined to an `inf` operand. Never authored: the Faber
    /// re-emitter prints only the operand.
    ///
    /// `operand` keeps its REAL bounded type (`operand.ty == Some(from)`);
    /// the node itself is typed `to` (`inf`). Backends convert from `from`
    /// without guessing the operand's carrier from side tables. Total: it
    /// never fails and never traps.
    Widen {
        operand: Box<HirExpression>,
        from: TypeId,
        to: TypeId,
    },

    /// Expression-form stream `ad` (blockless, evaluates to `sermo`).
    Ad {
        route: Symbol,
        opener: Option<Box<HirExpression>>,
    },

    Ref(HirRefKind, Box<HirExpression>),
    Deref(Box<HirExpression>),

    /// Runtime variant check: `expr est <type>` / `expr non est <type>`.
    /// Lowers to a tag check (`matches!` in Rust) when the LHS is `valor`.
    TypeCheck {
        positive: bool,
        expr: Box<HirExpression>,
        /// Resolved target type whose Valor variant is being checked.
        ty: TypeId,
    },

    /// Copy-into: `target ⇇ value` (U+21C7).
    ///
    /// Copies every value of `value` into the existing storage of `target`;
    /// the storage identity of `target` is preserved (`←` only rebinds names).
    /// The expression is `vacuum`-shaped. Typecheck owns the tensor-storage,
    /// exact-type and aliasing rules; lowering evaluates `value` once.
    CopyInto {
        target: Box<HirExpression>,
        value: Box<HirExpression>,
    },

    /// Sink-into: `sink ⇜ item` (U+21DC).
    ///
    /// The operator form of `sink.sink(item)`. A failed write dies here, while
    /// the method form reports it. The expression is a statement with no value
    /// (`vacuum`-shaped). Typecheck owns its rules 3 and 5-8; lowering
    /// evaluates `sink` then `item`, the `item` exactly once.
    SinkInto {
        sink: Box<HirExpression>,
        item: Box<HirExpression>,
    },

    /// Format operator `value ¶ "spec"` (D2.1) → `textus`.
    ///
    /// `spec` is the literal's text. Typecheck parses and validates it against
    /// the value's type through `radix_types::format_spec`; backends re-parse
    /// the same text with the same parser.
    Format {
        value: Box<HirExpression>,
        spec: Symbol,
    },

    /// Error placeholder that keeps the tree traversable after a diagnostic.
    Error,
    /// Closed reduction family. Body and binder are jointly present or absent.
    Reduction {
        op: HirReductionOp,
        source: Box<HirExpression>,
        coordinates: Vec<(DefId, Symbol)>,
        group: Option<Box<HirExpression>>,
        filum: Option<(DefId, Symbol)>,
        binding: Option<(DefId, Symbol)>,
        body: Option<HirBlock>,
        identity: Option<Box<HirExpression>>,
        mutable: bool,
    },
    /// Source-shaped ordered prefix scan. `group_domain` is populated only
    /// after typecheck proves each grouping key lies in a finite integer range.
    Scan {
        op: HirReductionOp,
        total: HirScanTotal,
        source: Box<HirExpression>,
        coordinates: Vec<(DefId, Symbol)>,
        group: Option<Box<HirExpression>>,
        group_domain: Option<HirScanGroupDomain>,
        order: HirSortDirection,
        filum: (DefId, Symbol),
        binding: (DefId, Symbol),
        body: HirBlock,
    },
    /// Sort expression `sort via asc|desc from|ref source [body]`.
    ///
    /// Source-shaped like `Reduction`: `binding` and `body` are jointly
    /// present or absent, and the body returns the sort key. Typecheck owns
    /// the source, key-order and direction rules; the result is a new list.
    Sort {
        direction: HirSortDirection,
        source_mode: HirSortSource,
        source: Box<HirExpression>,
        binding: Option<(DefId, Symbol)>,
        body: Option<HirBlock>,
        mutable: bool,
    },
}

/// Sort direction; locale packs own the words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirSortDirection {
    Ascending,
    Descending,
}

impl HirSortDirection {
    #[must_use]
    pub const fn keyword_key(self) -> &'static str {
        match self {
            Self::Ascending => "ascendens",
            Self::Descending => "descendens",
        }
    }
}

/// Which side of the source a sort orders: values (`ex`) or keys (`de`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirSortSource {
    Values,
    Keys,
}

impl HirSortSource {
    #[must_use]
    pub const fn keyword_key(self) -> &'static str {
        match self {
            Self::Values => "ex",
            Self::Keys => "de",
        }
    }
}

/// Reduction identities are stable; locale packs own their words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirReductionOp {
    Sum,
    Product,
    Max,
    Min,
    ArgMax,
    ArgMin,
    All,
    Any,
    Count,
    /// The first element whose body predicate holds (`prima`); `T ∪ nihil`.
    First,
}

/// Whether a prefix output includes its own input element.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirScanTotal {
    Inclusive,
    Exclusive,
}

impl HirScanTotal {
    #[must_use]
    pub const fn keyword_key(self) -> &'static str {
        match self {
            Self::Inclusive => "scan_inclusive",
            Self::Exclusive => "scan_exclusive",
        }
    }
}

/// Compiler-proven finite domain for a grouped scan's key components.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirScanGroupDomain {
    pub key_domains: Vec<HirScanKeyDomain>,
}

/// Finite integer domain proven for one grouped-scan key component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirScanKeyDomain {
    /// Inclusive range proved from a literal or source-coordinate extent.
    /// `max < min` represents an empty coordinate extent.
    IntegerRange { min: i128, max: i128 },
    /// The complete finite domain of a sized integer type. Device planners
    /// may reject a width whose dense state exceeds their resource limit.
    IntegerWidth(radix_types::NumericWidth),
}

impl HirReductionOp {
    #[must_use]
    pub const fn keyword_key(self) -> &'static str {
        match self {
            Self::Sum => "summa",
            Self::Product => "factum",
            Self::Max => "maxima",
            Self::Min => "minima",
            Self::ArgMax => "argmaxima",
            Self::ArgMin => "argminima",
            Self::All => "omnia",
            Self::Any => "quilibet",
            Self::Count => "numeratio",
            Self::First => "prima",
        }
    }
}

/// Diagnostic output channel selected by source syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirScribeKind {
    Nota,
    Vide,
    Mone,
    Scribe,
}

/// Array literal element, including spread syntax.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirArrayElement {
    Expr(HirExpression),
    Spread(HirExpression),
    /// Structural expression-shape inference hole written as `_`.
    Infer(Span),
}

/// Object/map construction field used by literals and conversion forms.
///
/// `value: None` represents shorthand forms where the key also names the value
/// binding; typecheck is responsible for validating that lookup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirObjectField {
    /// Key syntax preserved for diagnostics and codegen.
    pub key: HirObjectKey,

    /// Explicit value expression, or absent for shorthand.
    pub value: Option<HirExpression>,
}

/// Object key taxonomy after lowering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirObjectKey {
    Ident(Symbol),
    String(Symbol),

    /// Computed key expression.
    Computed(HirExpression),

    /// Copy-with-changes source (`Genus { … } ex source`, DECISIONS D15.1):
    /// every field the entries omit is copied, shallowly, from this value.
    /// Produced only by the construction-literal `ex` suffix, as the last
    /// entry of a struct construction.
    Spread(HirExpression),
}

/// One optional-chain operation after the base expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirOptionalChainKind {
    /// Null-safe member access.
    Member(Symbol),

    /// Null-safe index access.
    Index(Box<HirExpression>),

    /// Null-safe call.
    Call(Vec<HirCallArg>),
}

/// One non-null assertion operation after the base expression.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirNonNullKind {
    /// Assert only: the node's value is the asserted payload of the base.
    ///
    /// Lowering produces this as the receiver of `base!.method(...)`, so the
    /// call is an ordinary `MethodCall` whose receiver traps on `nihil`.
    Value,

    /// Assert then access a member.
    Member(Symbol),

    /// Assert then index.
    Index(Box<HirExpression>),

    /// Assert then call.
    Call(Vec<HirCallArg>),
}

/// Iteration source mode lowered from `itera` syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirIteraMode {
    /// Iterate values from a collection.
    Ex,

    /// Iterate entries/keys from an object-like source.
    De,

    /// Iterate numeric ranges from an `ab` loop source.
    Ab,
}

/// Target of a runtime `↦` conversion after lowering.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirConversioTarget {
    /// Declared destination type (`↦ numerus`, `↦ lista<T>`, …).
    Type(TypeId),
    /// Clamp constraint: inline range (`↦ 0‥10`) or stored `intervallum` operand (`↦ fines`).
    Intervallum(Box<HirExpression>),
}

/// Store position of the d11-6 store table (ruling 21) that an implicit store
/// node stands for. Names the row so the runtime trap can say where the value
/// was being stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirStoreKind {
    /// `fixum`/`varia T x ← e`.
    Declaration,
    /// `place ← e`.
    Assignment,
    /// Call, method or transfer argument.
    Argument,
    /// Parameter default `vel e`.
    ParameterDefault,
    /// `redde e` / `reddet e`, or a closure body against its declared return.
    Return,
    /// `cede e` against the generator element type.
    Yield,
    /// `iace e` against the `⇥` type.
    Throw,
    /// `T { f = e }` field, `finge` variant field or positional variant field.
    Field,
    /// Collection element, `tabula` key or value.
    Element,
    /// `iuncta<T1, T2>[a, b]` position.
    TuplePosition,
    /// The default of `x ↦ T ⊥ d`.
    ConversionDefault,
    /// `sit T x ↢ future`, `figendum T x ← e`: the awaited value.
    AwaitBinding,
}

/// Where an implicit store lands: the position kind, the slot's name when it
/// has one, and — for a slot whose type was inferred (`_`) — the span of the
/// expression the type was inferred from (ruling 19(i)).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirStoreSite {
    pub kind: HirStoreKind,
    pub slot: Option<Symbol>,
    pub inferred_from: Option<Span>,
    /// The source text of that expression, when the analysis had the source.
    pub inferred_text: Option<Box<str>>,
}

impl HirStoreSite {
    /// How a trap message names the inferring expression: its quoted source
    /// text, else its byte range; `None` when the slot was not inferred.
    /// Shared so every backend words the note like the runner.
    #[must_use]
    pub fn inferred_origin(&self) -> Option<String> {
        let from = self.inferred_from?;
        Some(self.inferred_text.as_deref().map_or_else(
            || format!("the expression at bytes {}..{}", from.start, from.end),
            |text| format!("`{text}`"),
        ))
    }
}

/// Conversion row resolved by typecheck for one `↦` site.
///
/// Carried on the expression so every backend lowers the same decision; the
/// Faber re-emitter ignores it and keeps the authored `↦`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirConversioRoute {
    /// D9.4: built-in `ordo` row. `members` lists every member in declaration
    /// order with its resolved discriminant.
    Ordo {
        row: HirOrdoRow,
        members: Vec<HirOrdoRouteMember>,
    },
    /// D9.3: registered `@ conversio` function for the direct (source,
    /// destination) pair. `failable` when the function declares `⇥ E`; its
    /// failure then propagates, or yields the site's `⊥` default.
    Function { def_id: DefId, failable: bool },
}

impl HirConversioRoute {
    /// The call a [`HirConversioRoute::Function`] route stands for:
    /// `function(source)`, carrying the conversion expression's identity and
    /// result type. Backends emit it through their ordinary call path.
    #[must_use]
    pub fn function_call(
        def_id: DefId,
        source: &HirExpression,
        conversio: &HirExpression,
    ) -> HirExpression {
        let span = conversio.span;
        HirExpression {
            id: conversio.id,
            kind: HirExpressionKind::Call(
                Box::new(HirExpression {
                    id: conversio.id,
                    kind: HirExpressionKind::Path(def_id),
                    ty: None,
                    span,
                }),
                Vec::new(),
                vec![HirCallArg {
                    name: None,
                    spread: false,
                    marker: None,
                    expr: source.clone(),
                    span: source.span,
                }],
                None,
            ),
            ty: conversio.ty,
            span,
        }
    }
}

/// Direction of a built-in `ordo` conversion row (D9.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirOrdoRow {
    /// `Ordo ↦ numerus`: the member's discriminant. Infallible.
    ToNumerus,
    /// `numerus ↦ Ordo`: the first member whose discriminant equals the
    /// value. Failable when no member matches.
    FromNumerus,
    /// `Ordo ↦ textus`: the member name. Infallible.
    ToTextus,
}

/// One `ordo` member as a conversion row sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirOrdoRouteMember {
    pub variant: DefId,
    pub name: Symbol,
    /// Resolved integer discriminant: the authored number, or the previous
    /// member's plus one (the first member defaults to 0). `None` on the
    /// `ToTextus` row for a string-valued member, and on the `FromNumerus`
    /// row for a member whose discriminant an earlier member already has
    /// (the first match wins, so it is never selected).
    pub value: Option<i64>,
}

/// Range endpoint inclusion policy.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum HirRangeKind {
    /// End is excluded.
    Exclusive,

    /// End is included.
    Inclusive,
}

/// Literal values preserved directly in HIR.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirLiteral {
    /// WHY: Ordinary expression integers carry an unsigned u64 magnitude;
    /// negative expressions are unary negation over this magnitude. MIR and
    /// AIR lower through explicit checked conversions until their own
    /// unsigned carriers land.
    Int(u64),
    /// Integer literal beyond `u64` (inf track, ruling 34): its exact value as
    /// canonical decimal digits (no sign, no separators, no radix prefix). An
    /// untyped exact constant that only an `inf` slot (or a value-joining
    /// `inf` operand) can hold. Self-contained so constant folding needs no
    /// interner.
    BigInt(Box<str>),
    Float(f64),
    /// Decimal literal carrying exact digit text (decimal-widths D1, R0 fix).
    /// `value` re-derives f64 exactly as the `Float` path always did; `text`
    /// preserves the decimal spelling for scaled-integer decimal widths.
    FloatText {
        value: f64,
        text: Symbol,
    },
    String(Symbol),
    Ascii(Symbol),
    Octeti(Symbol),
    /// Compile-time JSON object literal typed `valor`. The payload is the parsed
    /// constant JSON tree carried straight from the AST; duplicate keys are
    /// rejected in the typecheck pass.
    JsonValor(JsonValue),
    Regex(Symbol, Option<Symbol>),
    Bool(bool),
    Nil,
}

/// Binary operator taxonomy after lexical/operator resolution.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum HirBinOp {
    Add,
    Sub,
    Mul,
    Div,
    TrueDiv, // ÷ true division (always f64 on integers)
    Mod,
    // Glyph products (tensor-glyphs family; multiplicative precedence)
    Dot,           // · inner product (dot / matvec / matmul)
    Cross,         // × cross product (3-vectors)
    TensorProduct, // ⊗ outer / Kronecker product
    Hadamard,      // ⊙ Hadamard / elementwise product
    HadamardDiv,   // ⊘ Hadamard / elementwise divide
    Eq,
    NotEq,
    ApproxEq,
    ApproxNotEq,
    FuzzyEq,
    FuzzyNotEq,
    StrictEq,
    StrictNotEq,
    Lt,
    Gt,
    LtEq,
    GtEq,
    And,
    Or,
    Coalesce,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    Is,
    IsNot,
    InRange,
    Between,
    NotInRange,
    NotBetween,
}

impl HirBinOp {
    /// Return whether this is one of the tensor-glyphs product operators
    /// (`·` `×` `⊗` `⊙` `⊘`). Lanes that have not implemented the glyph surface
    /// fail closed on these before any lowering or emission.
    #[must_use]
    pub fn is_glyph_product(&self) -> bool {
        matches!(
            self,
            Self::Dot | Self::Cross | Self::TensorProduct | Self::Hadamard | Self::HadamardDiv
        )
    }

    /// Unicode spelling of a glyph operator: the glyph products and `÷` true
    /// division (named in diagnostics); `None` for ops with no glyph spelling.
    #[must_use]
    pub fn glyph_spelling(&self) -> Option<&'static str> {
        match self {
            Self::Dot => Some("·"),
            Self::Cross => Some("×"),
            Self::TensorProduct => Some("⊗"),
            Self::Hadamard => Some("⊙"),
            Self::HadamardDiv => Some("⊘"),
            Self::TrueDiv => Some("÷"),
            _ => None,
        }
    }
}

/// Unary operator taxonomy after lexical/operator resolution.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum HirUnOp {
    Neg,
    Not,
    BitNot,
}

/// Reference expression mode.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum HirRefKind {
    /// Shared borrow/reference.
    Shared,

    /// Mutable borrow/reference.
    Mutable,
}

/// One `casu` arm in a `discerne` expression.
///
/// Patterns introduce definitions and are therefore part of the HIR semantic
/// surface, not parser-only syntax.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirCasuArm {
    pub patterns: Vec<HirPattern>,
    pub body: HirExpression,
    pub span: Span,
}

/// Pattern taxonomy for `discerne` and related binding positions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum HirPattern {
    /// Wildcard pattern that binds nothing.
    Wildcard,

    /// New binding introduced by the pattern.
    Binding(DefId, Symbol),

    /// Alias binding layered over another pattern.
    Alias(DefId, Symbol, Box<HirPattern>),

    /// Resolved enum/variant pattern with nested subpatterns.
    Variant(DefId, Vec<HirPattern>),

    /// A type predicate pattern. `candidate` preserves a same-spelled variant
    /// so semantic checking can apply the scrutinee-directed discrimination
    /// rule before emission; `binding` names the narrowed value, if any.
    TypeTest {
        ty: TypeId,
        candidate: Option<DefId>,
        binding: Option<(DefId, Symbol)>,
    },

    /// Alternatives for one subject pattern, written `casu A aut B`.
    ///
    /// Every alternative shares one binding environment; semantic checking
    /// rejects alternatives that do not bind the same names and types.
    Or(Vec<HirPattern>),

    /// Literal pattern.
    Literal(HirLiteral),

    /// Negated number literal pattern (`casu -1`, `casu -2.5`, `casu -∞`).
    ///
    /// WHY: `HirLiteral::Int` carries an unsigned magnitude, so the sign lives
    /// on the pattern. The payload is always `Int`, `Float`, or `FloatText`
    /// holding the non-negative magnitude (DECISIONS D22.2).
    NegatedLiteral(HirLiteral),
}

// ---------------------------------------------------------------------------
// Presentation sidecar and structural end anchors
// ---------------------------------------------------------------------------
//
// A separate ID namespace plus a serializable presentation sidecar give
// line-start comments and structural ends stable owners without making comment
// content executable or visible to semantic passes. Semantic nodes stay
// trivia-blind: the sidecar back-references existing `HirId`/`DefId` values
// instead of gaining new fields. The single structural carrier in the
// semantic tree is `HirBlock::end`; it is a distinct anchor type, never a
// statement, and is not visited by ordinary visitors.

/// Stable identity for one legal source-anchor boundary in one analyzed unit.
///
/// A dedicated namespace, never overloaded with [`HirId`]. Spans remain
/// diagnostic/source-map aids; the anchor is the attachment identity.
///
/// `Hash` mirrors [`HirId`]: presentation consumers key deterministic maps by
/// anchor identity.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HirSourceAnchorId(pub u32);

/// One piece of ordered trivia attached to a source anchor.
///
/// Raw comment prose is preserved verbatim; the anchor is the identity, spans
/// are diagnostic aids only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirTrivia {
    /// One line-start comment (`# ...`) with its stored text.
    CommentLine {
        /// Interned comment payload, preserved verbatim.
        text: Symbol,
        span: Span,
    },

    /// One boundary newline captured in lexer/parser order.
    Newline { span: Span },
}

/// Closed taxonomy of presentation owner kinds for one schema version.
///
/// Adding a future legal comment position means extending this taxonomy along
/// with the syntax boundary, lowering rule, render slot, and tests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirTriviaOwnerKind {
    /// A top-level item (`HirItem`).
    Item,

    /// An executable statement (`HirStatement`).
    Statement,

    /// A genus field or method (`HirField` / `HirMethod`).
    Member,

    /// The explicit program entry (`incipit`).
    Entry,

    /// An executable block's structural end.
    BlockEnd,

    /// The program's structural end.
    ProgramEnd,

    /// The file's `regio` declaration (module doc owner, D7.7).
    Region,
}

/// Back-reference from a presentation owner to the semantic node it annotates.
///
/// Items and statements use [`HirOwnerRef::Hir`]; members use
/// [`HirOwnerRef::Def`]. This keeps the semantic tree trivia-blind — no new
/// fields on `HirItem`, `HirStatement`, `HirMethod`, or `HirField`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HirOwnerRef {
    /// An annotatable HIR node: `HirItem.id` or `HirStatement.id`.
    Hir(HirId),

    /// A named semantic definition: `HirMethod.def_id` or `HirField.def_id`.
    Def(DefId),
}

/// One typed presentation owner keyed by its source anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirTriviaOwner {
    /// Anchor identity for this owner (the attachment key).
    pub id: HirSourceAnchorId,
    pub kind: HirTriviaOwnerKind,

    /// Semantic node back-reference, when the owner annotates one.
    pub semantic: Option<HirOwnerRef>,
}

/// Ordered trivia attached to one source anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirTriviaAttachment {
    pub anchor: HirSourceAnchorId,

    /// Trivia in lexer/parser order (comments and newlines interleaved).
    pub leading: Vec<HirTrivia>,
}

/// The durable, serializable presentation layer for one analyzed unit.
///
/// Lives in the analyzed-unit/artifact envelope — never on `HirModule` — so
/// semantic consumers stay trivia-blind while the HIR value and its
/// presentation always travel together.
///
/// Wire note: the artifact codec is positional `postcard`, so a field added
/// here shifts every later byte and no field may carry `#[serde(default)]` to
/// suggest compatibility. A layout change is caught by the derived wire-shape
/// hash in `radix-hir-fhir` and must ship with a `SCHEMA_VERSION` bump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirPresentation {
    pub owners: Vec<HirTriviaOwner>,
    pub attachments: Vec<HirTriviaAttachment>,

    /// Structural ends of authored executable blocks.
    pub block_ends: Vec<HirBlockEnd>,

    /// The program's single structural end.
    pub program_end: HirModuleEnd,

    /// Anchor for the explicit entry (`incipit`) leading trivia, when present.
    pub entry_start_anchor: Option<HirSourceAnchorId>,

    /// Whether the entry block came from an explicit `incipit` statement
    /// (versus implicit top-level statements). Emission fidelity: only an
    /// explicit entry re-emits the `incipit` wrapper.
    pub entry_explicit: bool,

    /// Ids of `Si` expressions lowered from `requirit`/`reice` guard
    /// statements. Emission re-sugars exactly these back to guard form;
    /// structurally identical authored conditionals stay braced.
    pub guard_si_ids: Vec<HirId>,

    /// Ids of `Discerne` expressions lowered from `elige` (en `switch`)
    /// statements. Emission re-sugars exactly these back to the `elige`
    /// vocabulary with a `ceterum` default arm; authored `discerne`
    /// statements keep the `discerne`/`casu _` spelling.
    pub elige_expr_ids: Vec<HirId>,

    /// Ids of `MethodCall` expressions lowered from a source-only glyph form:
    /// the `ᵀ` transpose suffix and the rounding delimiters (`⌊x⌋`, `⌈x⌉`,
    /// `⌊x⌉`). Typecheck rewrites the glyph marker to the canonical intrinsic
    /// and keeps the node id; Faber re-emission prints the glyph form for
    /// exactly these ids instead of the method call (`.transpose()`, `.floor()`).
    /// The id only marks a glyph-authored method twin: emission picks `ᵀ` or the
    /// rounding delimiters from the rewritten twin's name.
    pub glyph_ids: Vec<HirId>,

    /// Ids of the outer `MethodCall` expressions lowered from an authored
    /// `∜` fourth root. `∜ x` and authored `√ √ x` lower to the same nested
    /// `.sqrt().sqrt()` shape, so this dedicated set is the only record that
    /// the outer call came from the single glyph; Faber re-emission prints
    /// `∜` for exactly these ids. Defaulted so older serialized HIR loads.
    pub radical_quarta_ids: Vec<HirId>,

    /// Ids of the expressions typecheck produced by desugaring a numeric
    /// twin method (`a.add(2)` means `a ← a + 2`, `x.added(y)` is `x + y`;
    /// the rewrite keeps the call's node id on the operator, the assignment,
    /// or the effect-hoisted block). Faber re-emission prints the authored
    /// method spelling in the source locale for exactly these ids, so
    /// `faber format` stays surface-faithful instead of respelling the call
    /// as an operator, an assignment, or a block with temporaries.
    pub numeric_twin_ids: Vec<HirId>,

    /// Ids of the expressions typecheck produced by rewriting an authored
    /// `empty` (`vacua`) at a dense tensor, vector, or matrix slot into the
    /// typed zero (`Type {}`) HIR. The rewrite keeps the literal's node id;
    /// Faber re-emission prints `empty`/`vacua` for exactly these ids, so
    /// `faber format` stays surface-faithful instead of printing `T {}`.
    pub empty_dense_ids: Vec<HirId>,

    /// Ids of the expressions typecheck produced by desugaring an authored
    /// inferred constructor `_ { … }` (entries at a map or `json` slot, `_ {}`
    /// at a map or dense tensor slot, fields at a genus or record slot). The
    /// rewrite keeps the hole node's id; Faber re-emission prints the hole
    /// for exactly these ids, so `faber format` stays surface-faithful instead
    /// of printing the typed constructor or bare literal it became.
    pub inferred_constructor_ids: Vec<HirId>,

    /// Ids of the expressions typecheck produced by expanding an authored
    /// `t.sum(axis n)` / `t.mean(axis n)` (and the max/min twins) into the
    /// grouped reduction expansion. The rewrite keeps the call's node id;
    /// Faber re-emission prints the authored method call for exactly these
    /// ids, so `faber format` stays surface-faithful instead of printing the
    /// expansion.
    pub axis_sugar: Vec<HirAxisSugar>,

    /// Initializer-expression ids of wildcard-bound declarations
    /// (`sit T _ ← expr`), which lower to bare expression statements.
    /// Emission restores the authored discard binding for exactly these.
    pub discard_binding_stmts: Vec<HirDiscardBinding>,

    /// Authored destructuring declarations (`fixum [a, rest r] ← xs`,
    /// `fixum {x, y} ← p`, and destructuring `itera` binders), which lower to
    /// a run of ordinary locals. Emission re-sugars exactly these runs back to
    /// the authored pattern.
    pub destructure_bindings: Vec<HirDestructureBinding>,

    /// Definitions whose awaited initializer was authored with `↢` rather
    /// than the `figendum` or `variandum` morphology.
    pub await_assignment_bindings: Vec<DefId>,

    /// Locals declared as compile-time constants (`fixum T x = e`, DECISIONS
    /// D5.10). They are ordinary immutable locals in the HIR; this list marks
    /// the ones whose initializer is a compile-time fact so typecheck applies
    /// the `=` store rule, the constant pass evaluates them, and Faber
    /// re-emission prints `=` rather than `←`.
    pub compile_time_locals: Vec<DefId>,

    /// Authored `ordo` enumerations, keyed by the enum's def id. Emission
    /// re-sugars exactly these with the `ordo` vocabulary and member
    /// discriminants; authored `discretio` unions are unaffected.
    pub ordo_enums: Vec<HirOrdoEnum>,

    /// Authored qualified variant paths (`module.Union.VARIANT`) carried
    /// through lowering when the HIR retains only the resolved variant id.
    /// The source path is presentation data: semantic identity remains the
    /// `DefId`, while Faber re-emission uses this spelling to avoid silently
    /// shortening an authored path.
    pub variant_paths: Vec<HirVariantPath>,

    /// Authored type occurrences, one record per authored root type position.
    /// Each record carries the full recursive form tree for that site; nested
    /// nodes are children of the root form, not separate entries. Two sites
    /// with the same semantic `TypeId` keep independent forms. Missing
    /// records mean synthesized/hand-built HIR and emit canonically; present
    /// records are validated fail-closed at emission and in FHIR.
    pub authored_types: Vec<HirAuthoredTypeUse>,

    /// Authored by-label member reads (`p.gw`) that lowering rewrote to the
    /// constant-index node for the same position. Emission re-sugars exactly
    /// these ids back to the label spelling; `p[1]` keeps its index form.
    pub tuple_label_accesses: Vec<HirTupleLabelAccess>,

    /// Authored build-time embeds (`insere "path"`, D8.10) whose initializer
    /// lowering folded to a literal. Faber re-emission prints the embed for
    /// exactly these expression ids instead of the embedded bytes.
    pub insere_exprs: Vec<HirInsere>,

    /// Template applications (`"…"(…)` and `` `…`(…) ``) whose authored text
    /// holds named holes (`§{label}`). Lowering erases the labels to the
    /// positional `§` form and reorders labeled actuals into positional
    /// slots; Faber re-emission prints the authored text and the actual labels
    /// for exactly these expression ids.
    pub template_holes: Vec<HirTemplateHoles>,

    /// The file's `regio` (en `module`) declaration, when authored (D7.7).
    /// Emission re-renders it first; its doc block is the module doc.
    pub region: Option<HirRegion>,

    /// Function modifier bindings (`errors e`, `options o`, `exit code`),
    /// in authored order. Each lowers to a runtime-provided local with no
    /// source statement; this records which modifier introduced it so Faber
    /// re-emission writes the modifier back into the signature.
    pub function_modifier_bindings: Vec<HirFunctionModifierBinding>,
}

/// Which function modifier introduced a runtime-provided binding local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HirFunctionModifierKind {
    /// `errata <name>` (en `errors`): the typed error-channel binding.
    Errata,

    /// `optiones <name>` (en `options`): the CLI options binding.
    Optiones,

    /// `exitus <name>` (en `exit`): the named exit-code binding.
    Exitus,
}

/// One authored function modifier binding, keyed by its runtime-provided local.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirFunctionModifierBinding {
    /// The runtime-provided `HirLocal` the modifier introduced.
    pub local: DefId,
    pub kind: HirFunctionModifierKind,
}

/// The authored form of a template application that names its holes.
///
/// `expr` is the application's own expression id (the `Scriptum` node, or the
/// `forma` capture's outer node). `template` is the authored literal text with
/// its `§{label}` holes, which the lowered node no longer carries.
/// `labeled_actuals` lists the actuals in authored order as
/// `(label, positional slot)`; it is empty when the actuals were positional.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirTemplateHoles {
    pub expr: HirId,
    pub template: Symbol,
    pub labeled_actuals: Vec<(Symbol, u32)>,
}

/// One authored build-time embed: the folded literal's id and its path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirInsere {
    pub expr: HirId,
    pub path: Symbol,
}

/// An authored `regio NAME` file-name declaration (D7.7).
///
/// The name is already applied at the import sites that default to it; this
/// record keeps the declaration for re-emission and carries the module
/// documentation anchor (D7.3, D7.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirRegion {
    pub name: Symbol,
    pub span: Span,

    /// Owner of the declaration's leading trivia, when it has any.
    pub anchor: Option<HirSourceAnchorId>,

    /// Module doc: the comment block directly above the declaration with no
    /// blank line between (D7.6), one entry per `#` line, in source order.
    /// Empty when the file has no module doc.
    pub doc: Vec<Symbol>,
}

/// Authored type-form facts for one source type position.
///
/// `site` keys the occurrence by stable semantic owner plus closed type slot;
/// `form` is the recursive authored tree whose witnesses resolve into the
/// canonical type tables.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirAuthoredTypeUse {
    pub site: HirTypeSite,
    pub form: HirTypeForm,
}

/// One authored by-label member read on a labeled `iuncta` binding.
///
/// Lowering rewrites `p.gw` to the same constant-index node as `p[1]`, so the
/// label survives only as presentation data. Emission re-sugars exactly the
/// recorded ids back to `.gw`; every other index read keeps its `[i]`
/// spelling. `expr_id` is the rewritten index expression's `HirId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirTupleLabelAccess {
    pub expr_id: HirId,

    /// Authored member name, as it appeared before the rewrite.
    pub label: Symbol,
}

impl HirPresentation {
    /// Authored form recorded at `site`, if any. Linear by design: authored
    /// type sites are sparse relative to expressions, and the Faber emitter
    /// builds its own map when it needs repeated lookups.
    #[must_use]
    pub fn authored_type_at(&self, site: &HirTypeSite) -> Option<&HirTypeForm> {
        self.authored_types
            .iter()
            .find(|use_| &use_.site == site)
            .map(|use_| &use_.form)
    }
}

/// Stable identity of one authored type occurrence.
///
/// Never keyed by `TypeId` (distinct occurrences share interned types),
/// spans (diagnostics data, not identity), traversal order, or rendered
/// strings (locale conversion changes built-in spellings).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HirTypeSite {
    pub owner: HirOwnerRef,
    pub slot: HirTypeSlot,
}

/// Closed set of type positions within an owner. The set is derived from the
/// source-site census; new authored positions gain named variants here and
/// nowhere else.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HirTypeSlot {
    /// Declared type of a local/const binding inside a function body.
    Declared,
    /// Body type of a `typus` alias declaration.
    AliasBody,
    /// Declared parameter type; the payload is the parameter ordinal.
    Parameter(u32),
    Return,
    /// Alternate-exit (`⇥`) type.
    Error,
    /// Named aggregate or union-variant field; payload is the field ordinal.
    Field(u32),
    /// Type-valued annotation field.
    AnnotationField {
        annotation: u32,
        field: u32,
    },
    /// Clausura parameter; payload is the parameter ordinal.
    ClosureParameter(u32),
    /// Explicit `iuncta` construction type argument.
    TupleTypeArgument(u32),
    /// Explicit ascription target (`∷`).
    AscriptionTarget,
    /// Explicit conversion (`↦`) target type.
    ConversionTarget,
    /// Authored type of a typed zero `Type {}`; the owner is the zero
    /// expression, which lowering synthesizes into a `crea` call or a `Verte`.
    TypedZero,
    /// Runtime type check / type-test target.
    TypeCheckTarget,
    /// Typed pattern position.
    PatternType,
    /// Type spelling of a wildcard/discard binding.
    DiscardBinding,
}

/// One semantic-level authored type node.
///
/// Not a second type system: no inference, unification, assignability,
/// layout, or alias expansion happens here. `ty` is the finalized semantic
/// witness for this node in the canonical tables; `head` and `args` record
/// how the author spelled that fact at this occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirTypeForm {
    /// Finalized semantic type witnessed by this authored node.
    pub ty: TypeId,
    pub head: HirTypeFormHead,
    pub args: Vec<HirTypeFormArg>,

    /// Authored element label for this node, when the source spelled one —
    /// the `gx` of `iuncta<gx: int, gw: int>`. Mirrors the parser's
    /// per-position `iuncta_label`, which rides the labeled child node; the
    /// absence of a label is the unlabeled form.
    ///
    /// Semantic identity stays label-blind (`Type::Tuple` is positional), so
    /// the label is presentation data: emission restores the authored
    /// spelling, and no pass may read it as a type fact.
    #[serde(default)]
    pub label: Option<Symbol>,
}

/// How one authored type node was spelled.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirTypeFormHead {
    /// A built-in semantic constructor written directly. The level supplies
    /// the presentation (long head vs bare-width/prefixed compression); the
    /// witness `ty` supplies the constructor and width.
    Semantic(TypeFormLevel),
    /// A user-defined alias or nominal path written by its authored spelling.
    /// The source argument list is intentionally independent of the canonical
    /// expansion's child list (a generic alias may take one source argument
    /// and expand to several canonical children); validation proves the
    /// instantiation equivalent to the site's canonical type.
    Named { def_id: DefId, path: Vec<Symbol> },
    /// An imported type alias written by its authored path (`decl.Quad`).
    ///
    /// A file-interface import carries an alias as its resolved body and no
    /// definition of its own, so the alias has no `DefId` for a `Named` head to
    /// name. The witness `ty` is the alias body; the path is the authored
    /// spelling, with no authored arguments (an applied imported alias keeps
    /// the semantic head).
    ImportedAlias { path: Vec<Symbol> },
    /// Authored `_` inference hole.
    Infer,
    /// Authored standalone `∪` hole.
    UnionHole,
}

/// One authored child of a type-form node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirTypeFormArg {
    Type(Box<HirTypeForm>),
    /// Tensor-family figura argument resolved to its canonical index.
    Size(IndexId),
    /// Authored generic name argument (e.g. `lista<numerus>`).
    Name(Symbol),
    /// Authored range argument of a ranged integer store policy. Unlike a
    /// resolved `Type::RangedInt`, this retains the written glyph and bounds.
    Range(HirRangeArgument),
}

/// Source spelling of a range argument on a store-policy type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirRangeArgument {
    pub lo: HirRangeBound,
    pub hi: HirRangeBound,
    /// `true` for the inclusive `…` glyph, `false` for half-open `‥`.
    pub inclusive: bool,
}

/// One authored endpoint of a store-policy range.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirRangeBound {
    /// The authored decimal spelling of an integer endpoint.
    Literal(String),
    Unbounded {
        negative: bool,
    },
    Name {
        name: Symbol,
        lifted: bool,
    },
}

/// Authored source path for one resolved enum or union variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirVariantPath {
    pub def_id: DefId,
    pub path: Vec<Symbol>,
}

/// Authored facts for one `ordo` enumeration member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirOrdoMember {
    pub def_id: DefId,

    /// Authored discriminant, when the member spelled one.
    pub value: Option<HirOrdoValue>,
}

/// An `ordo` member discriminant literal (`= 3`, `= "text"`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirOrdoValue {
    Integer(i64),
    String(Symbol),
}

/// Authored `ordo` enumeration shape carried for emission fidelity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirOrdoEnum {
    pub def_id: DefId,
    pub members: Vec<HirOrdoMember>,
}

/// Authored facts for one wildcard-bound declaration (`sit T _ ← expr`).
///
/// Keyed by the initializer expression id; the lowered statement keeps no
/// `HirLocal`, so these facts ride the presentation sidecar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirDiscardBinding {
    pub expr_id: HirId,

    /// Authored annotation type, when the declaration spelled one.
    pub ty: Option<TypeId>,

    pub mutable: bool,
    pub is_await: bool,
}

/// Authored axis-reduction sugar for one rewritten method call.
///
/// `id` is the node id of the expression typecheck expanded from the authored
/// call; `axis` and `method` are the authored axis and reduction the expansion
/// replaced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirAxisSugar {
    pub id: HirId,
    pub axis: u32,
    pub method: HirAxisMethod,
}

/// Reduction method of one authored axis-sugar call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirAxisMethod {
    Sum,
    Mean,
    Max,
    Min,
}

/// Authored facts for one destructuring declaration.
///
/// Lowering expands the pattern into consecutive statements: an optional
/// hidden subject local (the initializer evaluated once), then one local per
/// bound element or field, then the rest local. `stmt_ids` lists exactly that
/// run, so emission can replace it with the authored pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirDestructureBinding {
    /// Lowered statement ids of the expansion, in order. Never empty.
    pub stmt_ids: Vec<HirId>,

    /// Whether `stmt_ids[0]` is the hidden subject local. Otherwise the
    /// subject is a side-effect-free place read by every expanded local.
    pub hoisted: bool,

    pub pattern: HirDestructurePattern,
    pub mutable: bool,
    pub is_await: bool,
}

/// Authored shape of a flat destructuring pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirDestructurePattern {
    /// `[a, _, b, rest r]`; `None` is a `_` hole.
    Array {
        elements: Vec<Option<Symbol>>,
        rest: Option<Symbol>,
    },
    /// `{x, y ut z, rest r}` as `(field, alias)` pairs.
    Object {
        fields: Vec<(Symbol, Option<Symbol>)>,
        rest: Option<Symbol>,
    },
}

/// Structural end of one authored executable block.
///
/// Carried as [`HirBlock::end`]; never visited as a statement and never
/// lowered to a target no-op.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirBlockEnd {
    pub id: HirSourceAnchorId,
    pub span: Span,
}

/// Structural end of one source module.
///
/// Owned by the presentation sidecar — `HirModule` carries no end field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirModuleEnd {
    pub id: HirSourceAnchorId,
    pub span: Span,
}
