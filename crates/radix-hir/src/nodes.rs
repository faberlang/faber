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

use radix_lexer::{Span, Symbol, Token};
use radix_syntax::{CallablePosture, GenericParamKind};
use radix_types::TypeId;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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

/// Lowered program split into declarations and executable entry code.
///
/// HIR makes the source/package distinction explicit: declarations are always
/// top-level items, while implicit top-level execution and explicit entry forms
/// become one optional entry block.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirProgram {
    /// Top-level declarations available to later semantic and backend phases.
    pub items: Vec<HirItem>,

    /// Executable entry block from `incipit` or package-level statements.
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
    /// Provider namespace selected by the source import.
    pub provider: LibraryProvider,

    /// Module path inside the provider.
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
    /// Local binding definition.
    pub local_def_id: DefId,

    /// Provider/module identity.
    pub identity: LibraryIdentity,

    /// Rust runtime module supplied by library metadata, when this import is
    /// backed by a target runtime crate instead of generated local declarations.
    pub rust_runtime_module: Option<String>,

    /// Faber method name to Rust runtime symbol mappings supplied by interface
    /// metadata.
    pub rust_runtime_methods: BTreeMap<String, String>,
}

/// Provenance for a declaration imported from a library interface.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryItem {
    /// Definition of the imported declaration in this source unit.
    pub def_id: DefId,

    /// Provider/module identity.
    pub identity: LibraryIdentity,

    /// Exported library name before local aliasing.
    pub exported_name: String,

    /// Imported declaration kind.
    pub kind: LibraryItemKind,

    /// Whether an imported function declares an alternate-exit channel.
    pub is_failable: bool,

    /// Whether an imported function returns a future that must be awaited.
    pub is_async: bool,

    /// Rust runtime type supplied by library metadata for declarations whose
    /// Faber interface is a contract over a target runtime type.
    pub rust_runtime_type: Option<String>,

    /// Whether Rust should omit the generated declaration because the runtime
    /// crate owns the concrete surface.
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
    pub exports: FxHashMap<DefId, rustc_hash::FxHashSet<String>>,

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
    /// Node identifier for item-level analysis annotations.
    pub id: HirId,

    /// Definition identifier produced by name resolution.
    pub def_id: DefId,

    /// Declaration payload.
    pub kind: HirItemKind,

    /// Source location for diagnostics about the whole declaration.
    pub span: Span,
}

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

    /// Token payload for unpromoted `AnnotationKind::Statement` families (Stage 4 retires this).
    pub raw_args: Vec<Token>,

    /// Resolved annotation-contract definition when this application named a contract.
    ///
    /// Compiler-owned families leave this `None`. Target/framework consumers should
    /// match on `contract_id` when present instead of reparsing family strings.
    pub contract_id: Option<DefId>,

    /// Full source span of the annotation.
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
            contract_id: None,
            span,
        }
    }
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

/// Function signature and optional lowered body.
///
/// Function HIR is shared by top-level functions and methods. A missing body
/// means the source declared a signature-only surface, such as an interface
/// method. A missing return type means the source did not force one and
/// typecheck must infer or validate the effective return.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)]
pub struct HirFunction {
    /// Interned source name of the callable.
    pub name: Symbol,

    /// Generic type parameters introduced by the signature.
    pub type_params: Vec<HirTypeParam>,

    /// Ordinary call parameters, in declaration order.
    pub params: Vec<HirParam>,

    /// CLI argument bundle synthesized for mounted command entrypoints.
    pub cli_args: Option<HirParam>,

    /// Return type; None until typecheck infers it.
    pub ret_ty: Option<TypeId>,

    /// Recoverable alternate-exit type declared with `⇥`.
    pub err_ty: Option<TypeId>,

    /// Function body; None for interface methods.
    pub body: Option<HirBlock>,

    /// Canonical callable posture (four-cell identity).
    pub posture: CallablePosture,

    /// Whether the function is async (`fiet` / `futura`).
    pub is_async: bool,

    /// Whether the function is a generator (`fiunt`/`fient` / `cursor`).
    pub is_generator: bool,

    /// Whether the function is a CUDA kernel entry (`nucleum`).
    pub is_kernel: bool,

    /// Whether the function is a CUDA device helper (`machina`).
    pub is_device: bool,

    /// Whether the function is a graphics vertex shader entry (`@ vertex`).
    /// Set from annotation family; MIR lowering for vertex shaders is not yet
    /// implemented — see `device_role_for_function` in `mir/lower.rs`.
    pub is_vertex: bool,

    /// Whether the function is a graphics fragment shader entry (`@ fragment`).
    /// Set from annotation family; MIR lowering for fragment shaders is not yet
    /// implemented.
    pub is_fragment: bool,

    /// Structured metadata for lowered Faber test cases.
    pub test: Option<HirTestMetadata>,

    /// Normalized declaration metadata attached before lowering.
    pub annotations: Vec<HirAnnotation>,
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

    /// Source span for diagnostics about the test declaration.
    pub span: Span,
}

/// Test modifier taxonomy preserved for harness selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirTestModifier {
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

    /// Required capability or environment marker.
    Requirit(Symbol),

    /// Suite-scoped only marker.
    SolumIn(Symbol),
}

/// Generic parameter introduced by an item or callable signature.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirTypeParam {
    /// Definition identity for references to this type parameter.
    pub def_id: DefId,

    /// Type vs compile-time size/index parameter.
    pub kind: GenericParamKind,

    /// Interned source name.
    pub name: Symbol,

    /// Declaration span for diagnostics.
    pub span: Span,

    /// Source-declared admission rule for concrete type arguments.
    pub constraint: HirTypeParamConstraint,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirTypeParamConstraint {
    Any,
    OneOf(Vec<TypeId>),
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
}

/// Function, method, closure, or CLI parameter binding.
///
/// Parameter types are resolved during lowering because Faber parameters are
/// type-first source syntax. Call-site optionality and provider-slot markers are
/// distinct source contracts and are kept as separate flags.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirParam {
    /// Binding definition introduced by this parameter.
    pub def_id: DefId,

    /// Interned parameter name.
    pub name: Symbol,

    /// Declared parameter type.
    pub ty: TypeId,

    /// Ownership/reference mode requested by the signature.
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

    /// Source span for diagnostics about this binding.
    pub span: Span,
}

/// Source ownership/borrowing markers preserved through HIR lowering.
///
/// Semantic analysis maps these to [`crate::semantic::ParamMode`] at the
/// compiler-shaped boundary.
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
}

/// Product type declaration with fields, methods, and declared relationships.
///
/// HIR stores inheritance/interface references as `DefId` values so later
/// checks can reason about relationships without reopening lexical scopes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirStruct {
    /// Interned source name.
    pub name: Symbol,

    /// Generic type parameters declared on the type.
    pub type_params: Vec<HirTypeParam>,

    /// Normalized metadata attached to the genus declaration.
    pub annotations: Vec<HirAnnotation>,

    /// Instance/static fields in source order.
    pub fields: Vec<HirField>,

    /// Methods declared inside the type body.
    pub methods: Vec<HirMethod>,

    /// Resolved base type, if the source declares extension.
    pub extends: Option<DefId>,

    /// Resolved interfaces this type claims to implement.
    pub implements: Vec<DefId>,
}

/// Field declaration inside a product type.
///
/// Field type is already a resolved semantic type reference. Initializer
/// expressions remain HIR because typecheck still owns compatibility and
/// default-value validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirField {
    /// Field definition identity used by field lookup and diagnostics.
    pub def_id: DefId,

    /// Interned field name.
    pub name: Symbol,

    /// Declared field type.
    pub ty: TypeId,

    /// Whether this field belongs to the type rather than instances.
    pub is_static: bool,

    /// Post-name declaration marker: the field is voluntary in object literals / construction.
    pub sponte: bool,

    /// Normalized metadata attached to the field declaration.
    pub annotations: Vec<HirAnnotation>,

    /// Optional source initializer/default expression.
    pub init: Option<HirExpression>,

    /// Declaration span for diagnostics.
    pub span: Span,
}

/// Method declaration paired with receiver metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirMethod {
    /// Method definition identity reached by method lookup.
    pub def_id: DefId,

    /// Shared function payload for the method signature and body.
    pub func: HirFunction,

    /// Receiver mode lowered from method syntax.
    pub receiver: HirReceiver,

    /// Whole-method source span.
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
    /// Interned source name.
    pub name: Symbol,

    /// Generic type parameters declared on the enum.
    pub type_params: Vec<HirTypeParam>,

    /// Variants in source order.
    pub variants: Vec<HirVariant>,
}

/// Enum variant declaration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirVariant {
    /// Variant definition identity.
    pub def_id: DefId,

    /// Interned variant name.
    pub name: Symbol,

    /// Positional or named fields carried by the variant.
    pub fields: Vec<HirVariantField>,

    /// Variant declaration span.
    pub span: Span,
}

/// Field carried by an enum variant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirVariantField {
    /// Field name when the variant uses named payloads.
    pub name: Symbol,

    /// Declared payload type.
    pub ty: TypeId,

    /// Source span for diagnostics.
    pub span: Span,
}

/// Interface declaration whose methods define a callable contract.
///
/// Interface methods intentionally carry signatures only; implementation
/// matching is a later semantic check against these resolved parameter and
/// return type references.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirInterface {
    /// Interned interface name.
    pub name: Symbol,

    /// Generic type parameters declared on the interface.
    pub type_params: Vec<HirTypeParam>,

    /// Required method signatures.
    pub methods: Vec<HirInterfaceMethod>,
}

/// Signature required by an interface.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirInterfaceMethod {
    /// Target availability metadata. Empty means available for every target.
    pub availability: Vec<HirAvailability>,

    /// Interned method name.
    pub name: Symbol,

    /// Required parameter contract.
    pub params: Vec<HirParam>,

    /// Declared return type, or `None` when typecheck must infer/validate absence.
    pub ret_ty: Option<TypeId>,

    /// Declared recoverable alternate-exit type.
    pub err_ty: Option<TypeId>,

    /// Canonical callable posture (four-cell identity).
    pub posture: CallablePosture,

    /// Signature span for diagnostics.
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
    /// Alias name.
    pub name: Symbol,

    /// Generic parameters on the alias declaration.
    pub type_params: Vec<HirTypeParam>,

    /// Target type reference.
    pub ty: TypeId,
}

/// Top-level constant declaration.
///
/// The value remains an expression because constant evaluation and type
/// compatibility are later-phase responsibilities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirConst {
    /// Constant name.
    pub name: Symbol,

    /// Explicit constant type, when declared.
    pub ty: Option<TypeId>,

    /// Lowered initializer expression.
    pub value: HirExpression,

    /// Whether the source declaration permits mutation.
    pub mutable: bool,

    /// Whether the initializer should be awaited.
    pub is_await: bool,
}

/// Import declaration retained after package/library resolution.
///
/// Import items introduce definitions into the local semantic surface. Keeping
/// the declaration in HIR lets later diagnostics and generated metadata refer
/// back to the source import instead of losing that boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirImport {
    /// Interned import path/specifier.
    pub path: Symbol,

    /// Visibility declared for the import.
    pub visibility: radix_syntax::Visibility,

    /// Named items introduced by the import.
    pub items: Vec<HirImportItem>,
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

/// Breakable scope kind for [`HirBlock::breakable`].
///
/// Loops (`dum` / `itera` / `fac … dum`) use expression-level lowering and do
/// not set this field. `fac` statement and closure bodies do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HirBreakableKind {
    /// `fac { }` try body (not the `cape` handler).
    Fac,
}

/// Ordered statement block with an optional trailing expression.
///
/// HIR keeps expression-position blocks and statement-position blocks in one
/// shape. Typecheck decides whether the trailing expression contributes a value
/// in its context.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct HirBlock {
    /// Statements executed in order.
    pub statements: Vec<HirStatement>,

    /// Optional tail expression.
    pub expr: Option<Box<HirExpression>>,

    /// Span covering the block construct.
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
}

/// Statement node with stable analysis identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirStatement {
    /// Statement identity for analysis tables.
    pub id: HirId,

    /// Statement payload.
    pub kind: HirStatementKind,

    /// Source span for diagnostics.
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

    /// Loop break.
    Rumpe,

    /// Loop continue.
    Perge,

    /// Explicit no-op.
    Tacet,

    /// Postfix increment/decrement statement (`place ⊕` / `place ⊖`).
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
    /// Guard condition evaluated before the body.
    pub cond: HirExpression,

    /// Body executed when the guard condition is true.
    pub body: HirBlock,

    /// Source span of the full guard clause.
    pub span: Span,
}

/// Postfix increment or decrement of a mutable `numerus` place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirIncDec {
    /// Assignment target evaluated once for read-modify-write.
    pub target: HirExpression,

    /// Increment (`⊕`) or decrement (`⊖`).
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
    /// Local definition identity.
    pub def_id: DefId,

    /// Interned local name.
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

    /// Expression payload.
    pub kind: HirExpressionKind,

    /// Type established by typecheck, or absent before analysis.
    pub ty: Option<TypeId>,

    /// Source span for diagnostics about this expression.
    pub span: Span,
}

/// Function or method call argument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HirCallArg {
    /// Source field name for named construction forms such as `finge`.
    pub name: Option<Symbol>,

    /// Whether the source argument used `sparge`.
    pub spread: bool,

    /// Lowered argument expression.
    pub expr: HirExpression,

    /// Whole argument span.
    pub span: Span,
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

    /// Interned binding name.
    pub binding_name: Symbol,

    /// Declared or inferred binding type.
    pub binding_ty: Option<TypeId>,

    /// Handler body.
    pub body: HirBlock,

    /// Handler span for diagnostics.
    pub span: Span,
}

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

    /// Literal value.
    Literal(HirLiteral),

    /// Contextual empty value expression: `vacua`.
    Vacua,

    /// Binary operation.
    Binary(HirBinOp, Box<HirExpression>, Box<HirExpression>),

    /// Unary operation.
    Unary(HirUnOp, Box<HirExpression>),

    /// Function call.
    Call(Box<HirExpression>, Vec<TypeId>, Vec<HirCallArg>),

    /// Method call normalized from receiver syntax.
    MethodCall(Box<HirExpression>, Symbol, Vec<TypeId>, Vec<HirCallArg>),

    /// Field access.
    Field(Box<HirExpression>, Symbol),

    /// Index access.
    Index(Box<HirExpression>, Box<HirExpression>),

    /// Optional chaining for null-safe member, index, or call access.
    OptionalChain(Box<HirExpression>, HirOptionalChainKind),

    /// Non-null assertion for member, index, or call access.
    NonNull(Box<HirExpression>, HirNonNullKind),

    /// Block expression.
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

    /// Iteration loop with a binding introduced by the loop head.
    Itera(HirIteraMode, DefId, Symbol, Box<HirExpression>, HirBlock),

    /// Range expression.
    Intervallum {
        start: Box<HirExpression>,
        end: Box<HirExpression>,
        step: Option<Box<HirExpression>>,
        kind: HirRangeKind,
    },

    /// Assignment.
    Assign(Box<HirExpression>, Box<HirExpression>),

    /// Array literal.
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

    /// Assertion expression.
    Adfirma(Box<HirExpression>, Option<Box<HirExpression>>),

    /// Unrecoverable panic expression.
    Panic(Box<HirExpression>),

    /// Recoverable throw expression (`iace`).
    Throw(Box<HirExpression>),

    /// Structured recoverable-error handler from an attached `cape` clause.
    Handled { body: HirBlock, catch: Box<HirCape> },

    /// Closure with parameters, optional return/error types, and expression body.
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
    /// and supports inline recovery values via `⇥`.
    Conversio {
        source: Box<HirExpression>,
        target: HirConversioTarget,

        /// Codegen hint parameters (e.g., `i32`, `Hex` in `↦ numerus<i32, Hex>`).
        /// Stored as raw symbols because these are target-specific hints, not Faber types.
        params: Vec<Symbol>,
        recovery: Option<Box<HirExpression>>,
    },

    /// Expression-form stream `ad` (blockless, evaluates to `sermo`).
    Ad {
        route: Symbol,
        opener: Option<Box<HirExpression>>,
    },

    /// Reference expression.
    Ref(HirRefKind, Box<HirExpression>),

    /// Dereference expression.
    Deref(Box<HirExpression>),

    /// Runtime variant check: `expr est <type>` / `expr non est <type>`.
    /// Lowers to a tag check (`matches!` in Rust) when the LHS is `valor`.
    TypeCheck {
        positive: bool,
        expr: Box<HirExpression>,
        /// Resolved target type whose Valor variant is being checked.
        ty: TypeId,
    },

    /// Error placeholder that keeps the tree traversable after a diagnostic.
    Error,
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
    /// Ordinary element expression.
    Expr(HirExpression),

    /// Spread element expression.
    Spread(HirExpression),
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
    /// Identifier key.
    Ident(Symbol),

    /// String literal key.
    String(Symbol),

    /// Computed key expression.
    Computed(HirExpression),

    /// Spread object expression.
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
    Float(f64),
    String(Symbol),
    Ascii(Symbol),
    Octeti(Symbol),
    /// Compile-time JSON object literal typed `valor`. The payload is the parsed
    /// constant JSON tree carried straight from the AST; duplicate keys are
    /// rejected in the typecheck pass (Goal Stage 3).
    JsonValor(radix_syntax::JsonValue),
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
    Mod,
    Eq,
    NotEq,
    ApproxEq,
    ApproxNotEq,
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
    /// Accepted patterns for this arm.
    pub patterns: Vec<HirPattern>,

    /// Optional guard expression.
    pub guard: Option<HirExpression>,

    /// Arm body expression.
    pub body: HirExpression,

    /// Source span for diagnostics about this arm.
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

    /// Literal pattern.
    Literal(HirLiteral),
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
        /// Source span for diagnostics.
        span: Span,
    },

    /// One boundary newline captured in lexer/parser order.
    Newline {
        /// Source span for diagnostics.
        span: Span,
    },
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
}

/// Back-reference from a presentation owner to the semantic node it annotates.
///
/// Items and statements use [`HirOwnerRef::Hir`]; members use
/// [`HirOwnerRef::Def`]. This keeps the semantic tree trivia-blind — no new
/// fields on `HirItem`, `HirStatement`, `HirMethod`, or `HirField`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

    /// Owner kind within the closed taxonomy.
    pub kind: HirTriviaOwnerKind,

    /// Semantic node back-reference, when the owner annotates one.
    pub semantic: Option<HirOwnerRef>,
}

/// Ordered trivia attached to one source anchor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirTriviaAttachment {
    /// Owner anchor this trivia attaches to.
    pub anchor: HirSourceAnchorId,

    /// Trivia in lexer/parser order (comments and newlines interleaved).
    pub leading: Vec<HirTrivia>,
}

/// The durable, serializable presentation layer for one analyzed unit.
///
/// Lives in the analyzed-unit/artifact envelope — never on `HirProgram` — so
/// semantic consumers stay trivia-blind while the HIR value and its
/// presentation always travel together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirPresentation {
    /// Typed owners keyed by source anchor.
    pub owners: Vec<HirTriviaOwner>,

    /// Ordered trivia attachments keyed by owner anchor.
    pub attachments: Vec<HirTriviaAttachment>,

    /// Structural ends of authored executable blocks.
    pub block_ends: Vec<HirBlockEnd>,

    /// The program's single structural end.
    pub program_end: HirProgramEnd,

    /// Anchor for the explicit entry (`incipit`) leading trivia, when present.
    pub entry_start_anchor: Option<HirSourceAnchorId>,
}

/// Structural end of one authored executable block.
///
/// Carried as [`HirBlock::end`]; never visited as a statement and never
/// lowered to a target no-op.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirBlockEnd {
    /// Source anchor identity of this block end.
    pub id: HirSourceAnchorId,

    /// Source span for diagnostics.
    pub span: Span,
}

/// Structural end of the program.
///
/// Owned by the presentation sidecar — `HirProgram` carries no end field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HirProgramEnd {
    /// Source anchor identity of the program end.
    pub id: HirSourceAnchorId,

    /// Source span for diagnostics.
    pub span: Span,
}
