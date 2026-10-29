//! Concrete syntax model for parsed Faber source.
//!
//! This file defines the AST that the parser hands to later compiler phases.
//! It is intentionally a source-shaped model: declarations, expressions, type
//! syntax and annotations are recorded as the user wrote them, with
//! spans and node IDs for later phases to attach diagnostics and side tables.
//!
//! The AST does not decide semantic meaning. Name resolution, inferred types,
//! target compatibility, stdlib translation behavior, exhaustiveness, and data
//! flow live in later compiler phases keyed by these nodes. Keeping the tree
//! syntactic lets the parser preserve edge cases and lets semantic analysis
//! reject bad programs with source-accurate context instead of losing grammar
//! detail during parse.
//!
//! INVARIANTS
//! ==========
//! - Every statement and expression parsed from source carries a [`NodeId`].
//! - `span` fields are parse provenance for diagnostics and source mapping.
//! - [`TypeExpr`] represents written type syntax, not a checked type.
//! - Annotation nodes preserve metadata syntax; interpretation is owned by
//!   lowering, package tooling, and backend-specific codegen.
//! - Legacy compatibility fields remain explicit so callers do not guess when
//!   grammar migrations are incomplete.

use radix_lexer::{Span, Symbol, Token};
use serde::{Deserialize, Serialize};

use super::Trivia;

// =============================================================================
// CORE TYPES
// =============================================================================

/// Parser-assigned identifier for statement and expression nodes.
///
/// `NodeId` is the rendezvous point between the syntax tree and later phase
/// side tables such as inferred types, flow facts, and resolved definitions. It
/// is not globally meaningful outside one parsed program/session, and it should
/// not be serialized as a stable language artifact.
pub type NodeId = u32;

/// Parsed contents of one Faber source unit.
///
/// File-level metadata lives in optional `+++` frontmatter peeled by the driver
/// before lexing. Package mode may assemble many `Program`s, but each value
/// remains the parse result for one file-like source body.
#[derive(Debug)]
pub struct Program {
    pub statements: Vec<Stmt>,

    /// Comments and newlines collected immediately before EOF.
    ///
    /// WHY: A `#` comment run after the last declaration (or in a comment-only
    /// file) has no following statement to attach to; it is retained on the
    /// program end boundary so the post-parse completeness check can claim it.
    pub end_trivia: Vec<Trivia>,
    pub span: Span,

    /// The file's optional `regio` (en `module`) declaration (D7.7).
    ///
    /// At most one, and only as the first declaration of the body (after the
    /// peeled `+++` frontmatter, before any import). It names the file for
    /// default import binding and anchors the module doc comment.
    pub region: Option<RegionDecl>,
}

/// `regio NAME` — a file naming itself (D7.7).
///
/// The declared name is the default binding for a path-inferred import of
/// this file; the comment block directly above it is the module doc (D7.3,
/// D7.6).
#[derive(Debug, Clone)]
pub struct RegionDecl {
    pub name: Ident,
    pub span: Span,

    /// Comments and newlines before the declaration, in source order.
    pub leading_trivia: Vec<Trivia>,
}

// =============================================================================
// Statements
// =============================================================================

#[derive(Debug)]
pub struct Stmt {
    /// Stable handle used by later phases for statement-scoped facts.
    pub id: NodeId,
    pub kind: StmtKind,
    pub span: Span,
    pub annotations: Vec<Annotation>,
    pub leading_trivia: Vec<Trivia>,
}

/// Top-level and block-level statement forms recognized by the parser.
///
/// Variants name grammar categories, not semantic classes. For example,
/// `Import`, `Ad`, and `Incipit` still require package or lowering policy
/// before they become module dependencies, endpoints, or entrypoints.
#[derive(Debug)]
pub enum StmtKind {
    /// `fixum` / `varia` binding.
    Var(VarDecl),
    /// `magnitudo H ← expr` / en `size H ← expr`; syntax only.
    SizeDecl(SizeDecl),
    Func(FuncDecl),
    /// `genus` declaration.
    Class(ClassDecl),
    /// `implendum` declaration.
    Interface(InterfaceDecl),
    /// `typus` alias.
    TypeAlias(TypeAliasDecl),
    Schema(SchemaDecl),
    /// `ordo` enumeration.
    Enum(EnumDecl),
    /// `discretio` tagged union.
    Union(UnionDecl),
    Import(ImportDecl),
    /// `probandum` test suite.
    Probandum(ProbandumDecl),
    /// `proba` test case.
    Proba(ProbaCase),
    /// `ex <expr> fixum|varia <fields>`
    Ex(ExStmt),
    Block(BlockStmt),
    Expr(ExprStmt),
    /// Postfix `↑` / `↓`.
    IncDec(IncDecStmt),
    /// `si` conditional.
    Si(SiStmt),
    /// `dum` loop.
    Dum(DumStmt),
    /// `itera` loop.
    Itera(IteraStmt),
    /// `elige` switch.
    Elige(EligeStmt),
    /// `discerne` pattern match.
    Discerne(DiscerneStmt),
    /// `custodi` guard.
    Custodi(CustodiStmt),
    /// `fac` scoped block.
    Fac(FacStmt),
    /// `redde` return.
    Redde(ReddeStmt),
    /// `redde filum` / `return thread`: end the current lane iteration.
    ReddeFilum(ReddeFilumStmt),
    /// `rumpe` break.
    Rumpe(RumpeStmt),
    /// `perge` continue.
    Perge(PergeStmt),
    /// `iace` throw.
    Iace(IaceStmt),
    /// `mori` panic.
    Mori(MoriStmt),
    /// `tacet` explicit noop.
    Tacet(TacetStmt),
    /// `adfirma` assert.
    Adfirma(AdfirmaStmt),
    /// `requirit` recoverable guard (en surface `require`).
    Require(RequireStmt),
    /// `reice` recoverable reject guard (en surface `reject`).
    Reject(RejectStmt),
    /// `nota` / `vide` / `mone` / `scribe` diagnostics.
    Scribe(ScribeStmt),
    /// `incipit` / `incipiet` entry point.
    Incipit(IncipitStmt),
    /// `reddet future` await-return (`fiet` only).
    Reddet(ReddetStmt),
    /// `tacebit future` await-discard.
    Tacebit(TacebitStmt),
}

// =============================================================================
// Declarations
// =============================================================================

#[derive(Debug)]
pub struct VarDecl {
    /// Binding policy from `fixum` or `varia`.
    pub mutability: AstMutability,

    /// Authored await-binding surface, absent for an immediate initializer.
    pub await_form: Option<AwaitBindingForm>,

    /// Written type annotation, if present; inference is resolved later.
    pub ty: Option<TypeExpr>,
    pub binding: BindingPattern,
    pub init: Option<Box<Expr>>,

    /// Compile-time constant (`const T x = e` / la `fixum`, DECISIONS D5.10):
    /// an immutable binding whose initializer is a compile-time fact, spelled
    /// with `=` rather than `←`. At module level it is the module constant
    /// (st1). Never set together with an await form.
    pub compile_time: bool,
}

/// `const size NAME ← expr` runtime lift or `const size NAME = natural` static size.
#[derive(Debug)]
pub struct SizeDecl {
    pub name: Ident,
    pub init: Box<Expr>,
    /// `=` binds a static natural; `←` lifts a runtime snapshot.
    pub is_static: bool,
}

impl VarDecl {
    /// Whether this declaration awaits its initializer before binding.
    #[must_use]
    pub fn is_await(&self) -> bool {
        self.await_form.is_some()
    }
}

/// Source spelling used to request await-binding behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AwaitBindingForm {
    /// Morphological `figendum` or `variandum` declaration with `←`.
    Morphology,
    /// Ordinary declaration keyword with the `↢` initializer operator.
    Assignment,
}

/// Binding target syntax used by declarations and destructuring forms.
///
/// Patterns only describe where names are introduced. They do not encode
/// resolved field types, tuple/array lengths, or whether a source expression can
/// actually be destructured; those are semantic checks.
#[derive(Debug)]
pub enum BindingPattern {
    Ident(Ident),
    Wildcard(Span),
    Array {
        elements: Vec<BindingPattern>,
        is_rest: Option<Ident>,
        span: Span,
    },
    Object {
        fields: Vec<ExField>,
        is_rest: Option<Ident>,
        span: Span,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AstMutability {
    Immutable, // fixum
    Mutable,   // varia
}

/// Callable posture in the morphology-driven async/stream surface.
///
/// The four cells are the canonical compiler identity for whether a callable
/// completes synchronously or asynchronously and whether it produces one finite
/// result or a stream of results. The source surface spells them in an optional
/// grammar slot after the parameter list: bare (`SyncFinite`), `fiet`
/// (`AsyncFinite`), `fiunt` (`SyncStream`), `fient` (`AsyncStream`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CallablePosture {
    /// Synchronous finite callable (bare signature).
    #[default]
    SyncFinite,
    /// Asynchronous finite callable (`fiet`).
    AsyncFinite,
    /// Synchronous stream callable (`fiunt`).
    SyncStream,
    /// Asynchronous stream callable (`fient`).
    AsyncStream,
}

impl CallablePosture {
    #[must_use]
    pub fn is_async(self) -> bool {
        matches!(self, Self::AsyncFinite | Self::AsyncStream)
    }

    #[must_use]
    pub fn is_stream(self) -> bool {
        matches!(self, Self::SyncStream | Self::AsyncStream)
    }
}

#[derive(Debug)]
pub struct FuncDecl {
    pub name: Ident,
    pub type_params: Vec<TypeParam>,
    pub params: Vec<Param>,

    /// Function-level modifiers that affect CLI/runtime/lowering policy.
    pub modifiers: Vec<FuncModifier>,

    /// Optional callable posture (`fiet`/`fiunt`/`fient`); bare = synchronous finite.
    pub posture: CallablePosture,

    /// Return type annotation. Absence does not imply `vacuum` until checked.
    pub ret: Option<TypeExpr>,

    /// Declared error type for throwing functions, if present.
    pub err: Option<TypeExpr>,

    /// Function body, absent for interface contract declarations.
    pub body: Option<BlockStmt>,
    pub annotations: Vec<Annotation>,
    pub leading_trivia: Vec<Trivia>,
}

/// Kind of a generic parameter in an angle-bracket list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GenericParamKind {
    /// Type parameter (`T`, `U`, …).
    Typus,
    /// Compile-time size/index parameter (`magnitudo N`).
    Magnitudo,
}

#[derive(Debug)]
pub struct TypeParam {
    pub kind: GenericParamKind,
    pub name: Ident,
    /// Declared `=` default. A generic default is a declaration-time fact, so
    /// the variant always matches the declared sort; semantic phases read it
    /// instead of re-deriving the class from the payload shape.
    pub default: Option<GenericParamDefault>,
    /// Contract bounds written `T implet C<T> ∩ D<T>` (DECISIONS D1.1).
    /// Empty when the parameter is unbounded.
    pub bounds: Vec<ContractRefExpr>,
    pub span: Span,
}

/// One contract named by an `implet` clause, with its written type arguments.
///
/// Shared by genus headers (`genus Persona implet Orderable<Persona>`, D1.2)
/// and type-parameter bounds (`T implet Orderable<T>`, D1.1). A generic
/// contract is always written in full; there is no bare-name shorthand.
#[derive(Debug)]
pub struct ContractRefExpr {
    pub name: Ident,
    pub args: Vec<TypeExpr>,
    pub span: Span,
}

/// Source-shaped default on one generic parameter.
#[derive(Debug)]
pub enum GenericParamDefault {
    /// Type default (`T = type_annotation`), legal on `Typus` parameters.
    Typus(Box<TypeExpr>),
    /// Natural-literal default (`magnitudo D = 64`), legal on `Magnitudo`
    /// parameters. Runtime expressions and dependent defaults are not part of
    /// the grammar.
    Magnitudo { value: u64, span: Span },
}

#[derive(Debug)]
pub struct Param {
    /// `sponte` marks a voluntary parameter slot; it is syntax, not nullability.
    pub sponte: bool,

    /// Ownership/borrowing mode from `de`, `in`, or `ex`.
    pub mode: AstParamMode,

    /// `ceteri` rest parameter marker.
    pub is_rest: bool,

    pub ty: TypeExpr,
    pub name: Ident,

    /// Optional external alias introduced by `ut`.
    pub alias: Option<Ident>,
    pub default: Option<Box<Expr>>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AstParamMode {
    /// Default value-passing mode when no ownership marker is written.
    #[default]
    Owned,

    /// `de`: shared borrow-like parameter mode.
    De,

    /// `in`: mutable borrow-like parameter mode.
    In,

    /// `ex`: explicit move mode.
    Ex,

    /// `axis`: the parameter is a number that names an axis. A marker word
    /// before the type (`axis int a`), echoed at the call; not a type.
    Axis,
}

/// Function modifiers parsed from declaration syntax.
///
/// These values are still syntax-level metadata. Package command generation,
/// CLI binding, runtime integration, and codegen decide which combinations are
/// meaningful for a target.
#[derive(Debug)]
pub enum FuncModifier {
    Argumenta(Ident),
    Errata(Ident),
    Exitus(ExitusValue),
    Iacit,
    Immutata,
    Optiones(Ident),
}

#[derive(Debug)]
pub enum ExitusValue {
    Name(Ident),
    Number(i64),
}

#[derive(Debug)]
pub struct ClassDecl {
    pub name: Ident,
    pub type_params: Vec<TypeParam>,
    pub implements: Vec<ContractRefExpr>,
    pub members: Vec<ClassMember>,

    /// Comments and newlines collected immediately before the consuming `}`.
    ///
    /// WHY: A `#` comment run directly before a genus body close has no
    /// following member to attach to; it is retained on the block-end boundary
    /// (mirroring [`BlockStmt::end_trivia`]) so the post-parse completeness
    /// check can claim it. Syntax does not manufacture a member for this
    /// boundary.
    pub end_trivia: Vec<Trivia>,
}

#[derive(Debug)]
pub struct ClassMember {
    pub leading_trivia: Vec<Trivia>,
    pub annotations: Vec<Annotation>,
    pub kind: ClassMemberKind,
    pub span: Span,
}

// WHY: `FuncDecl` carries `leading_trivia`; boxing would ripple through every match arm.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum ClassMemberKind {
    Field(FieldDecl),
    Method(FuncDecl),
}

/// Leading modifier of a `genus` field (DECISIONS D16.1).
///
/// A field declares exactly one: `sit` (en `let`; per instance, set only at
/// construction; the variant keeps its historical `Fixum` name), `varia` (per
/// instance, reassignable) or `fixum` (en `const`; a type-level compile-time
/// constant, read-only, `=` initializer required). The retired `generis` (en
/// `static`) word is `PARSE010 static_member_retired`. `None` is an unmarked
/// field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldModifier {
    Fixum,
    Varia,
    Const,
}

/// Visibility of a `genus` member (DECISIONS D5.2, D5.3).
///
/// Members are public by default. `@ privata` limits a member to the type's
/// own methods; `@ interna` limits it to code in the declaring package. A
/// construction literal may still set any field (D5.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MemberVisibility {
    #[default]
    Publica,
    Interna,
    Privata,
}

#[derive(Debug)]
pub struct FieldDecl {
    pub modifier: Option<FieldModifier>,
    pub sponte: bool, // sponte — voluntary field
    pub ty: TypeExpr,
    pub name: Ident,
    pub init: Option<Box<Expr>>,
}

impl FieldDecl {
    /// Whether the field belongs to the type (a `const` member) rather than instances.
    #[must_use]
    pub fn is_static(&self) -> bool {
        self.modifier == Some(FieldModifier::Const)
    }
}

#[derive(Debug)]
pub struct InterfaceDecl {
    pub name: Ident,
    pub type_params: Vec<TypeParam>,
    pub methods: Vec<InterfaceMethod>,
}

#[derive(Debug)]
pub struct InterfaceMethod {
    pub leading_trivia: Vec<Trivia>,
    pub annotations: Vec<Annotation>,
    pub name: Ident,
    pub params: Vec<Param>,
    pub modifiers: Vec<FuncModifier>,
    /// Optional callable posture (`fiet`/`fiunt`/`fient`); bare = synchronous finite.
    pub posture: CallablePosture,
    pub ret: Option<TypeExpr>,
    pub err: Option<TypeExpr>,
    pub span: Span,
}

#[derive(Debug)]
pub struct TypeAliasDecl {
    pub name: Ident,
    pub type_params: Vec<TypeParam>,
    pub ty: TypeExpr,
    pub annotations: Vec<Annotation>,
}

/// Relational-view schema declaration: `schema Name { columna<T> name [: source] … }`.
///
/// A schema is a partial application-owned relational heading
/// (docs/design/relational-views.md §2): it names only the columns the
/// application needs, and extra source columns stay invisible. Per-column
/// metadata — public name, source alias, Faber type, nullability via
/// `T ∪ nihil` in the type position, and canonical order — is carried
/// source-shaped here and resolved semantically in later phases. The schema
/// name is a type-namespace identity and is never a runtime value.
#[derive(Debug)]
pub struct SchemaDecl {
    /// Schema name (type-namespace identity; never a runtime value).
    pub name: Ident,

    /// Ordered column declarations (canonical column order).
    pub columns: Vec<SchemaColumn>,

    /// Comments and newlines collected immediately before the consuming `}`.
    ///
    /// WHY: A `#` comment run directly before a schema body close has no
    /// following column to attach to; it is retained on the block-end boundary
    /// (mirroring [`ClassDecl::end_trivia`]) so the post-parse completeness
    /// check can claim it.
    pub end_trivia: Vec<Trivia>,
}

/// One `columna<T> publicName [: sourceName]` declaration inside a schema.
///
/// `T` is written type syntax; `T ∪ nihil` spells nullable columns through the
/// type position. The optional `publicName : sourceName` alias row is the
/// source-column mapping (public → source); a missing alias is the identity
/// mapping.
#[derive(Debug)]
pub struct SchemaColumn {
    pub leading_trivia: Vec<Trivia>,

    /// Written Faber type contract (`T` or `T ∪ nihil`).
    pub ty: TypeExpr,

    /// Schema-facing public name; result rows are keyed by it.
    pub name: Ident,

    /// Optional source-column identity via `name : source`; identity mapping
    /// when absent.
    pub source_alias: Option<Ident>,
    pub span: Span,
}

#[derive(Debug)]
pub struct EnumDecl {
    pub name: Ident,
    pub members: Vec<EnumMember>,
}

#[derive(Debug)]
pub struct EnumMember {
    pub leading_trivia: Vec<Trivia>,
    pub name: Ident,
    pub value: Option<EnumValue>,
    pub span: Span,
}

#[derive(Debug)]
pub enum EnumValue {
    Integer(i64),
    String(Symbol),
}

#[derive(Debug)]
pub struct UnionDecl {
    pub name: Ident,
    pub type_params: Vec<TypeParam>,
    /// Declaration-region fields shared by every variant.
    pub shared_fields: Vec<UnionField>,
    pub variants: Vec<Variant>,
}

#[derive(Debug)]
pub struct UnionField {
    pub leading_trivia: Vec<Trivia>,
    pub annotations: Vec<Annotation>,
    pub field: FieldDecl,
    pub span: Span,
}

#[derive(Debug)]
pub struct Variant {
    pub leading_trivia: Vec<Trivia>,
    pub name: Ident,
    pub fields: Vec<VariantField>,
    pub span: Span,
}

#[derive(Debug)]
pub struct VariantField {
    pub ty: TypeExpr,
    pub name: Ident,
    pub span: Span,
}

#[derive(Debug)]
pub struct ImportDecl {
    /// Import specifier exactly as parsed from source.
    pub path: Symbol,

    /// Source visibility marker; package resolution interprets its reach.
    pub visibility: Visibility,
    pub kind: ImportKind,

    /// Target word of a target-native import (`import python "math" m`),
    /// exactly as spelled. `None` for an ordinary Faber import. When set,
    /// `path` is the target's own module specifier, copied verbatim.
    pub target: Option<Ident>,
    pub span: Span,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Visibility {
    /// No re-export; the default when no import marker is written. No longer
    /// spellable as `privata` on an import (VM-U3).
    Privata,

    /// `publica` import visibility marker: re-export (`pub use`).
    Publica,
}

/// Parser-level import binding form.
///
/// The path may later resolve to a local module or a built-in library interface;
/// that boundary is owned outside syntax so parsing does not special-case
/// providers such as `norma`.
#[derive(Debug)]
pub enum ImportKind {
    Named {
        name: Ident,
        alias: Option<Ident>,
        /// True when `name` was inferred from the import path instead of spelled
        /// in source. Semantic collection uses this to require explicit aliases
        /// for ambiguous default bindings.
        inferred: bool,
    },
    Wildcard {
        alias: Ident,
    },
    Selective {
        /// Ordered selective members. The list is never empty; an empty or
        /// wildcard-mixed list is rejected during parse.
        members: Vec<ImportValueBinding>,
    },
}

/// One member of a selective import list: an exported value name plus the
/// caller-owned local binding it is aliased to when one is spelled.
#[derive(Debug)]
pub struct ImportValueBinding {
    /// Exported value name exactly as spelled in source.
    pub name: Ident,
    /// Caller-owned local binding; `None` for an unaliased member.
    pub alias: Option<Ident>,
    pub span: Span,
}

#[derive(Debug)]
pub struct ProbandumDecl {
    pub name: Symbol,

    /// Test-suite modifiers that later test lowering interprets.
    pub modifiers: Vec<ProbaModifier>,
    pub body: ProbandumBody,
    pub span: Span,
}

#[derive(Debug)]
pub struct ProbandumBody {
    pub setup: Vec<PraeparaBlock>,
    pub tests: Vec<ProbaCase>,
    pub nested: Vec<ProbandumDecl>,
}

#[derive(Debug)]
pub struct PraeparaBlock {
    pub kind: PraeparaKind,
    pub all: bool,
    pub body: BlockStmt,
    pub span: Span,
}

#[derive(Debug, Clone, Copy)]
pub enum PraeparaKind {
    Praepara,
    Praeparabit,
    Postpara,
    Postparabit,
}

#[derive(Debug)]
pub struct ProbaCase {
    pub modifiers: Vec<ProbaModifier>,
    pub name: Symbol,
    pub body: BlockStmt,
    pub span: Span,
}

/// Parsed test modifier taxonomy.
///
/// The AST preserves these modifiers as declared; scheduling behavior, target
/// filtering, retries, and timing policy are owned by the test backend.
#[derive(Debug, Clone)]
pub enum ProbaModifier {
    Erratur,
    Fragilis(i64),
    Futurum(Symbol),
    Metior,
    Omitte(Symbol),
    Repete(i64),
    Solum,
    SolumIn(Symbol),
    Tag(Symbol),
    Temporis(i64),
}

// =============================================================================
// Statements (control flow)
// =============================================================================

#[derive(Debug)]
pub struct BlockStmt {
    pub statements: Vec<Stmt>,

    /// Comments and newlines collected immediately before the consuming `}`.
    ///
    /// WHY: A `#` comment run directly before an executable block close has no
    /// following statement to attach to; it is retained on the block-end
    /// boundary so the post-parse completeness check can claim it. Syntax does
    /// not manufacture a `tacet` statement for this boundary.
    pub end_trivia: Vec<Trivia>,

    pub span: Span,
}

#[derive(Debug)]
pub struct ExprStmt {
    pub expr: Box<Expr>,
}

#[derive(Debug)]
pub struct IncDecStmt {
    pub target: Box<Expr>,
    pub op: IncDecOp,
}

#[derive(Debug, Clone, Copy)]
pub enum IncDecOp {
    Inc,
    Dec,
}

#[derive(Debug)]
pub struct SiStmt {
    pub cond: Box<Expr>,
    pub then: IfBody,
    pub catch: Option<CapeClause>,
    pub else_: Option<SecusClause>,
}

#[derive(Debug)]
pub enum IfBody {
    Block(BlockStmt),
    Ergo(Box<Stmt>),
}

#[derive(Debug)]
pub enum SecusClause {
    Sin {
        stmt: Box<SiStmt>,
        /// Leading trivia (comments and blank lines) before the `sin` branch link.
        leading_trivia: Vec<Trivia>,
    },
    Block {
        body: BlockStmt,
        catch: Option<CapeClause>,
        leading_trivia: Vec<Trivia>,
    },
    Stmt {
        stmt: Box<Stmt>,
        catch: Option<CapeClause>,
        leading_trivia: Vec<Trivia>,
    },
}

#[derive(Debug)]
pub struct DumStmt {
    pub cond: Box<Expr>,
    pub body: IfBody,
    pub catch: Option<CapeClause>,
}

#[derive(Debug)]
pub struct IteraStmt {
    pub mode: IteraMode,
    /// Source expressions in source order. `ex` admits the comma-separated
    /// lockstep series and `ab` the comma-separated range-product series;
    /// `de` carries exactly one.
    pub sources: Vec<Expr>,
    /// Coordinate names from the optional `apud`/`at` clause, in source order.
    /// Semantic binding is intentionally deferred to the iteration lowering unit.
    pub apud: Vec<Ident>,
    /// Optional lane binder from the `filum` clause (declared metadata only;
    /// the runner lowers the plain loop).
    pub filum: Option<Ident>,
    pub mutability: AstMutability,
    /// Binder patterns in source order, one per source (enforced in semantic:
    /// patterns bind only the single-source form; the series binds names).
    pub bindings: Vec<BindingPattern>,
    pub body: IfBody,
    pub catch: Option<CapeClause>,
}

#[derive(Debug, Clone, Copy)]
pub enum IteraMode {
    Ex,
    De,
    Ab,
}

#[derive(Debug)]
pub struct EligeStmt {
    pub expr: Box<Expr>,
    pub cases: Vec<CasuCase>,
    pub default: Option<CeterumDefault>,
    pub catch: Option<CapeClause>,
}

#[derive(Debug)]
pub struct CasuCase {
    pub value: Box<Expr>,
    pub body: IfBody,
    pub span: Span,
}

#[derive(Debug)]
pub struct CeterumDefault {
    pub body: IfBody,
    pub span: Span,
}

#[derive(Debug)]
pub struct DiscerneStmt {
    /// `omnia` marker requiring exhaustive semantic coverage.
    pub exhaustive: bool,
    pub subjects: Vec<Expr>,
    pub arms: Vec<CasuArm>,
    pub default: Option<CeterumDefault>,
}

#[derive(Debug)]
pub struct CasuArm {
    pub patterns: Vec<Pattern>,
    pub body: IfBody,
    pub span: Span,
}

/// Pattern syntax accepted inside `discerne`.
///
/// Patterns here identify grammar shape and bound names only. Variant
/// resolution, literal compatibility, and exhaustiveness are checked after name
/// and type information exist.
#[derive(Debug)]
pub enum Pattern {
    Wildcard(Span),
    Ident(Ident, Option<PatternBind>),
    /// A type-pattern expression classified by the parser's type-name lookahead.
    ///
    /// The full type expression is retained so bounded collection and tensor
    /// shape arguments remain available to lowering. Keeping this distinction
    /// in syntax prevents later phases from guessing whether a name was
    /// intended as a new binding or a type predicate.
    Type(TypeExpr, Option<PatternBind>),
    Literal(Literal, Span),
    /// A leading minus over a number literal (`casu -1`, `casu -2.5`,
    /// `casu -∞`). The lexer never produces signed numbers, so the literal
    /// keeps its unsigned magnitude and the pattern owns the sign (D22.2).
    NegatedLiteral(Literal, Span),
    /// Alternatives for one scrutinee, written `casu A aut B`.
    Or(Vec<Pattern>),
    Path(PathPattern),
}

#[derive(Debug)]
pub struct PathPattern {
    pub segments: Vec<Ident>,
    pub bind: Option<PatternBind>,
    pub span: Span,
}

#[derive(Debug)]
pub enum PatternBind {
    /// `ut NAME`: bind the matched value under an alias.
    Alias(Ident),

    /// Destructure and bind payload slots with optional per-slot `ut` aliases.
    Bindings {
        mutability: AstMutability,
        bindings: Vec<ExField>,
    },
}

#[derive(Debug)]
pub struct CustodiStmt {
    pub clauses: Vec<CustodiClause>,
}

#[derive(Debug)]
pub struct CustodiClause {
    pub cond: Box<Expr>,
    pub body: IfBody,
    pub span: Span,
}

/// The modifier slot of a `fac` block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FacModifier {
    /// Plain `fac { … }`.
    None,
    /// `fac omnia` (en `do all`): launches inside defer their faults to the
    /// block's exit.
    Omnia,
    /// `fac machina` (en `do device`): a checked device-resident region.
    Machina,
}

#[derive(Debug)]
pub struct FacStmt {
    /// The word directly after `fac`: `omnia` (en `do all`) or `machina`
    /// (en `do device`). Neither changes the construct's host control flow.
    pub modifier: FacModifier,
    pub body: BlockStmt,
    pub catch: Option<CapeClause>,
    pub while_: Option<Box<Expr>>,
}

#[derive(Debug)]
pub struct ReddeStmt {
    pub value: Option<Box<Expr>>,
}

/// `redde filum` / `return thread` — end the current thread lane.
#[derive(Debug)]
pub struct ReddeFilumStmt {
    /// Parsed so the semantic pass can diagnose the forbidden value explicitly.
    pub value: Option<Box<Expr>>,
}

#[derive(Debug)]
pub struct RumpeStmt {
    pub span: Span,
}

#[derive(Debug)]
pub struct PergeStmt {
    pub span: Span,
}

#[derive(Debug)]
pub struct IaceStmt {
    pub value: Box<Expr>,
}

#[derive(Debug)]
pub struct MoriStmt {
    pub value: Box<Expr>,
}

#[derive(Debug)]
pub struct TacetStmt {
    pub span: Span,
}

/// `reddet future` — await a promise and return its resolved value.
///
/// Valid only inside a `fiet` body. The operand must be a compatible
/// `promissum<T>` or `promissum<T ⇥ E>`.
#[derive(Debug)]
pub struct ReddetStmt {
    pub future: Box<Expr>,
    pub span: Span,
}

/// `tacebit future` — await a promise to completion and discard its success value.
///
/// Valid inside `fiet`, `fient`, and `incipiet` bodies. Discards any success `T`
/// (not only `vacuum`) and propagates a compatible `E`.
#[derive(Debug)]
pub struct TacebitStmt {
    pub future: Box<Expr>,
    pub span: Span,
}

#[derive(Debug)]
pub struct CapeClause {
    pub binding: Ident,
    pub body: BlockStmt,
    pub span: Span,
}

#[derive(Debug)]
pub struct AdfirmaStmt {
    pub cond: Box<Expr>,
    pub message: Option<Box<Expr>>,
}

/// `requirit <cond> iace <err>` — recoverable guard statement (en surface
/// `require <cond> throw <err>`).
///
/// Desugars to `si non (cond) { iace err }` at lowering; the thrown value
/// enters the enclosing function's `⇥ E` channel, unlike `adfirma` (fatal).
#[derive(Debug)]
pub struct RequireStmt {
    pub cond: Box<Expr>,
    pub error: Box<Expr>,
}

/// `reice <cond> iace <err>` — recoverable reject guard statement (en surface
/// `reject <cond> throw <err>`); the boolean opposite of `requirit`.
///
/// Desugars to `si (cond) { iace err }` at lowering; the thrown value enters
/// the enclosing function's `⇥ E` channel, same as `requirit`.
#[derive(Debug)]
pub struct RejectStmt {
    pub cond: Box<Expr>,
    pub error: Box<Expr>,
}

#[derive(Debug)]
pub struct ScribeStmt {
    pub kind: ScribeKind,
    pub args: Vec<Expr>,
}

#[derive(Debug, Clone, Copy)]
pub enum ScribeKind {
    Scribe,
    Vide,
    Mone,
    Nota,
}

#[derive(Debug)]
pub struct IncipitStmt {
    /// Distinguishes `incipiet` from synchronous `incipit`.
    pub is_async: bool,

    /// Entrypoint body in either block or single-statement form.
    pub body: IfBody,
    pub args: Option<Ident>,
    pub exitus: Option<Box<Expr>>,
}

#[derive(Debug)]
pub struct ExStmt {
    pub source: Box<Expr>,
    pub mutability: AstMutability,
    pub fields: Vec<ExField>,
    pub is_rest: Option<Ident>,
    pub span: Span,
}

#[derive(Debug)]
pub struct ExField {
    pub name: Ident,
    pub alias: Option<Ident>,
}

// =============================================================================
// Expressions
// =============================================================================

#[derive(Debug)]
pub struct Expr {
    /// Stable handle used by later phases for expression-scoped facts.
    pub id: NodeId,
    pub kind: ExprKind,
    pub span: Span,
}

/// Value-producing source forms.
///
/// These variants encode parse shape, including sugared forms such as optional
/// chains and collection DSL fragments. Lowering decides which forms normalize
/// to shared HIR operations and which remain target-specific.
#[derive(Debug)]
pub enum ExprKind {
    Ident(Ident),
    Literal(Literal),
    /// Contextual empty value: `vacua`.
    ///
    /// Carries the surface spelling so resolution can prefer a user binding
    /// of the same name (binding-wins).
    Vacua(Ident),
    /// Value-literal keyword written in source: `verum`/`falsum`, `nulla`
    /// (or its `nihil` alias) and `nonnumerus`.
    ///
    /// Carries the surface spelling so resolution can prefer a user binding
    /// of the same name (binding-wins), exactly like [`ExprKind::Vacua`].
    KeywordLiteral(KeywordLiteralExpr),
    Binary(BinaryExpr),
    Unary(UnaryExpr),
    Ternary(TernaryExpr),
    Call(CallExpr),
    Member(MemberExpr),
    Index(IndexExpr),
    /// Postfix gradient selection: `call(...) ∇ [places]?`.
    Gradient(GradientExpr),
    /// Optional chain: `x?.y`, `x?[y]`, `x?(args)`.
    OptionalChain(OptionalChainExpr),
    /// Non-null assertion: `x!.y`, `x![y]`, `x!(args)`.
    NonNull(NonNullExpr),
    Assign(AssignExpr),
    /// Postfix `∷` conversion.
    Verte(VerteExpr),
    /// `finge` variant construction.
    Finge(FingeExpr),
    /// `clausura` closure. Boxed: the variant dominates `ExprKind`'s size
    /// (`clippy::large_enum_variant`); closures are allocated once at parse.
    Clausura(Box<ClausuraExpr>),
    /// `cede` yield carrier.
    Cede(CedeExpr),
    Array(ArrayExpr),
    Object(ObjectExpr),
    /// Range: `x‥y`, `x…y`, `x ante y`, `x usque y`.
    Intervallum(IntervallumExpr),
    /// Runtime conversion: `↦ target`.
    Conversio(ConversioExpr),
    /// `scriptum` interpolated script.
    Scriptum(ScriptumExpr),
    /// `lege` input.
    Lege(LegeExpr),
    /// Expression-form `ad 'route' (opener)?`.
    Ad(Box<AdExpr>),
    /// `praefixum(expr)` comptime.
    Praefixum(PraefixumExpr),
    /// Build-time file embed `insere "path"` (en `embed`, D8.10). Claimed
    /// only as a module-level constant or `generis` field initializer; the
    /// result type follows the declaration (`textus` or `octeti`).
    Insere(InsereExpr),
    /// `ego` self reference.
    Ego(Span),
    Paren(Box<Expr>),
    /// Runtime variant check: `expr est <type>`.
    /// WHY: `est` with a type RHS is semantically a tag check, not value
    /// equality. Separating it from Binary(Is, ...) lets typecheck and codegen
    /// treat it as a variant discrimination (matches!) without overloading the
    /// equality path.
    TypeCheck(TypeCheckExpr),
    /// `iuncta<A, B> [a, b]` tuple construction.
    Iuncta(IunctaExpr),
    /// Extent-bearing type expression used as a value constructor.
    Construct(TypeExpr),
    /// Owned reduction expression: `reducta via Op ex source ...`.
    Reducta(Box<ReductaExpr>),
    /// Ordered prefix expression: `scan via Op [total inclusive|exclusive] ...`.
    Scan(Box<ScanExpr>),
    /// Owned sort expression: `ordinata via ascendens ex source [fixum x { key }]`.
    Sort(Box<SortExpr>),
    /// Copy-into statement: `target ⇇ value` (U+21C7).
    ///
    /// Copies every value of `value` into the existing storage of `target`;
    /// the whole expression is `vacuum`-shaped.
    CopyInto(CopyIntoExpr),
    /// Sink-into statement: `sink ⇜ item` (U+21DC).
    ///
    /// Delivers one `item` to the sinking contract of `sink`; the whole
    /// expression is `vacuum`-shaped. One `⇜` is one write, so the form has no
    /// chain.
    SinkInto(SinkIntoExpr),
    /// Capture boundary: `capta { body }`.
    ///
    /// Reifies the error channel into a value union (`success ∪ errata`). The
    /// trailing expression statement of the body is the success value; there
    /// is no early-success exit form this campaign.
    Trap(Box<TrapExpr>),
    /// Call default: `call ⊥ default` (D17.7).
    ///
    /// Supplies `default` when the failable `call` fails through its `⇥`
    /// error channel. The parser admits only a call operand; semantic analysis
    /// requires the call to be failable.
    CallDefault(CallDefaultExpr),
    /// Format operator: `value ¶ "spec"` (D2.1) → `textus`.
    Format(FormatExpr),
}

/// `call ⊥ default` (D17.7): the call-site default channel.
#[derive(Debug)]
pub struct CallDefaultExpr {
    /// The failable call whose `⇥` failure is replaced.
    pub call: Box<Expr>,
    /// Value evaluated only when `call` fails.
    pub default: Box<Expr>,
}

#[derive(Debug)]
pub struct TrapExpr {
    pub body: BlockStmt,
    pub span: Span,
}

/// Source shape for the `¶` format operator (D2.1, D2.2).
///
/// The spec is a string literal by construction: pass 1 rejects anything
/// else. Pass 2 parses and validates it against the value's type.
#[derive(Debug)]
pub struct FormatExpr {
    /// Value being formatted.
    pub value: Box<Expr>,
    /// Spec literal text, without quotes.
    pub spec: Symbol,
    /// Span of the spec literal.
    pub spec_span: Span,
}

/// Source shape for the `⇇` copy-into statement: `target ⇇ value` copies
/// every value of `value` into the existing storage of `target`.
#[derive(Debug)]
pub struct CopyIntoExpr {
    /// The existing storage that receives the values.
    pub target: Box<Expr>,
    /// The value expression copied into the target.
    pub value: Box<Expr>,
}

/// Source shape for the `⇜` sink-into statement: `sink ⇜ item` delivers one
/// `item` to the sinking contract implemented by `sink`.
#[derive(Debug)]
pub struct SinkIntoExpr {
    /// The sinking receiver that consumes the item.
    pub sink: Box<Expr>,
    /// The item expression delivered to the sink.
    pub item: Box<Expr>,
}

/// Source-owned clauses for the closed contextual reduction family.
#[derive(Debug)]
pub struct ReductaExpr {
    pub op: ReductaOp,
    pub source: Box<Expr>,
    pub apud: Vec<Ident>,
    pub group: Option<Box<Expr>>,
    pub filum: Option<Ident>,
    pub mutability: AstMutability,
    pub binding: Option<Ident>,
    pub body: Option<BlockStmt>,
    pub identity: Option<Box<Expr>>,
}

/// Source-owned clauses for the ordered prefix family. The final `vel` tail
/// remains an ordinary outer coalesce expression; it is never an accumulator
/// identity.
#[derive(Debug)]
pub struct ScanExpr {
    pub op: ReductaOp,
    pub total: ScanTotal,
    pub source: Box<Expr>,
    pub apud: Vec<Ident>,
    pub group: Option<Box<Expr>>,
    pub order: SortDirection,
    pub filum: Ident,
    pub binding: Ident,
    pub body: BlockStmt,
}

/// Whether each output element includes its own source value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanTotal {
    Inclusive,
    Exclusive,
}

/// The ten operations; surface spellings belong to the active locale pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReductaOp {
    Sum,
    Product,
    Max,
    Min,
    ArgMax,
    ArgMin,
    All,
    Any,
    Count,
    /// `prima` (en `first`): the first element whose body predicate holds.
    First,
}

/// Sort expression: direction word, `ex` (values) or `de` (keys), a source and
/// an optional key body in the reduce body form.
#[derive(Debug)]
pub struct SortExpr {
    pub direction: SortDirection,
    pub source_mode: SortSourceMode,
    pub source: Box<Expr>,
    pub mutability: AstMutability,
    pub binding: Option<Ident>,
    pub body: Option<BlockStmt>,
}

/// Sort direction; the surface words belong to the active locale pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortDirection {
    Ascending,
    Descending,
}

/// Which side of a source the sort orders: values (`ex`) or keys (`de`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortSourceMode {
    Values,
    Keys,
}

#[derive(Debug)]
pub struct IunctaExpr {
    pub type_args: Vec<TypeExpr>,
    pub elements: Vec<Expr>,
}

/// A value-literal keyword and the spelling it was written with.
#[derive(Debug)]
pub struct KeywordLiteralExpr {
    pub literal: Literal,
    pub ident: Ident,
}

impl ExprKind {
    /// The literal this form denotes, whether it is a plain literal or a
    /// value-literal keyword that a same-named binding may still shadow.
    #[must_use]
    pub fn literal(&self) -> Option<&Literal> {
        match self {
            Self::Literal(literal) => Some(literal),
            Self::KeywordLiteral(keyword) => Some(&keyword.literal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Ident {
    pub name: Symbol,
    pub span: Span,
}

#[derive(Debug)]
pub enum Literal {
    /// WHY: Ordinary expression integers carry an unsigned u64 magnitude;
    /// negative expressions are unary negation over this magnitude. Signed
    /// consumers convert at their own checked boundaries.
    Int(u64),
    /// Integer literal beyond `u64` (inf track, ruling 34): the cleaned digit
    /// text, decimal or `0x`/`0o`/`0b` prefixed. Parsed exactly where it lands.
    BigInt(Symbol),
    Float(f64),
    /// Decimal literal carrying exact digit text (decimal-widths D1).
    /// `value` re-derives f64 exactly as the `Float` path always did.
    FloatText {
        value: f64,
        text: Symbol,
    },
    String(Symbol),
    Ascii(Symbol),
    Forma(Symbol),
    Octeti(Symbol),
    /// Inline regex literal body (`regex /…/` or `_ /…/`): the raw pattern
    /// text between the slashes, exactly as authored, with no suffix flags.
    Regex(Symbol),
    /// Compile-time JSON object literal. Bare `{ ... }` in expression position
    /// parses to this; the payload is the parsed constant JSON tree, typed
    /// `json` at semantic time unless an explicit `tabula<K,V>` target is supplied.
    JsonValor(JsonValue),
    Bool(bool),
    Nil,
}

/// A member of a compile-time JSON object literal. Keys are always JSON strings;
/// `key_span` is preserved for duplicate-key diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JsonMember {
    pub key_span: Span,
    pub key: Symbol,
    pub value: JsonValue,
}

/// The parsed constant tree of a compile-time JSON literal.
///
/// Mirrors the runtime `faber::Valor` enum but in compile-time AST form. Number
/// variants follow the lexer's existing lexical split (`Integer` vs `Float`),
/// preserving integer precision for large numbers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JsonValue {
    Object(Vec<JsonMember>),
    Array(Vec<JsonValue>),
    String(Symbol),
    Numerus(i64),
    Fractus(f64),
    Bool(bool),
    Null,
}

#[derive(Debug)]
pub struct BinaryExpr {
    pub op: BinOp,
    pub lhs: Box<Expr>,
    pub rhs: Box<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BinOp {
    // Arithmetic
    Add,
    Sub,
    // Infix extrema (minmax-infix-glyphs family; additive precedence)
    Maximum, // ⤒ upper/max
    Minimum, // ⤓ lower/min
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
    // Comparison
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
    // Logical
    And,
    Or,
    // Nullish
    Coalesce,
    // Bitwise
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
    // Identity
    Is,
    IsNot,
    // Range
    InRange,
    Between,
    // Membership glyphs: `∈` / `∉` (the checker picks range or collection)
    Member,
    NotMember,
}

#[derive(Debug)]
pub struct UnaryExpr {
    pub op: UnOp,
    pub operand: Box<Expr>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnOp {
    Neg,           // -
    Not,           // non
    BitNot,        // ¬
    Floor,         // ⌊x⌋
    Ceil,          // ⌈x⌉
    Nearest,       // ⌊x⌉ (ties to the even neighbour)
    RadicalSqrt,   // √ x
    RadicalCbrt,   // ∛ x
    RadicalFourth, // ∜ x
}

/// The value conditional `cond ✓ then ✗ else_` (D4.5), one level only.
#[derive(Debug)]
pub struct TernaryExpr {
    pub cond: Box<Expr>,
    pub then: Box<Expr>,
    pub else_: Box<Expr>,
}

#[derive(Debug)]
pub struct CallExpr {
    pub callee: Box<Expr>,
    pub type_args: Vec<GenericArgExpr>,
    pub args: Vec<Argument>,
}

#[derive(Debug)]
pub struct Argument {
    /// Template-call label, when this actual addresses a named hole.
    ///
    /// Labels are parser-surface metadata and are erased before HIR lowering.
    pub label: Option<Symbol>,
    pub is_spread: bool, // sparge
    /// Call-site access marker (`ref`/`mut`/`own`/`copy`; Latin `de`/`in`/
    /// `own`/`copy`) written before the argument expression, when present.
    pub marker: Option<TypeMode>,

    /// `axis` marker word written before the argument: the number names an
    /// axis. A second kind of marker beside `marker`; the two never combine.
    pub axis: bool,
    pub value: Box<Expr>,
    pub span: Span,
}

#[derive(Debug)]
pub struct MemberExpr {
    pub object: Box<Expr>,
    pub member: Ident,
}

#[derive(Debug)]
pub struct IndexExpr {
    pub object: Box<Expr>,
    pub index: Box<Expr>,
}

/// Postfix gradient-selection expression: `call(...) ∇` or `call(...) ∇ [places]`.
///
/// The gradient glyph is a structural postfix operator, not a prefix unary op.
/// `call` preserves the forward expression as parsed (semantic analysis must
/// require it to resolve to an eligible differentiable forward call); `selectors`
/// preserves the ordered request list, empty for the bare `∇` form. The bracket
/// list is consumed as part of the gradient suffix before the ordinary postfix
/// loop resumes, so later postfix operations apply to the gradient result.
#[derive(Debug)]
pub struct GradientExpr {
    pub call: Box<Expr>,
    pub selectors: Vec<Expr>,
}

#[derive(Debug)]
pub struct OptionalChainExpr {
    pub object: Box<Expr>,
    pub chain: OptionalChainKind,
}

#[derive(Debug)]
pub enum OptionalChainKind {
    Member(Ident),
    Index(Box<Expr>),
    Call(Vec<Argument>),
}

#[derive(Debug)]
pub struct NonNullExpr {
    pub object: Box<Expr>,
    pub chain: NonNullKind,
}

#[derive(Debug)]
pub enum NonNullKind {
    Member(Ident),
    Index(Box<Expr>),
    Call(Vec<Argument>),
}

#[derive(Debug)]
pub struct AssignExpr {
    pub op: AssignOp,
    pub target: Box<Expr>,
    pub value: Box<Expr>,
    /// Value-side `copy` marker written before the right side
    /// (`b ← copy c`): the stored value is a deep duplicate of `c`. Only
    /// `TypeMode::Copy` is recorded here; a declaration initializer's marker
    /// lands on the declared type's mode instead.
    pub value_marker: Option<TypeMode>,
    /// `⊥ fallback` inline recovery, legal only on `↤` (conversion-directed
    /// assignment). The parser attaches the recovery to the nearest `↤` in a
    /// right-associated chain; the operator and recovery travel through the
    /// syntax tree so no erasure happens before the destination type is known.
    pub recovery: Option<Box<Expr>>,
    /// `via HINT` rounding/codegen hint, legal only on `↤` and written before
    /// the `⊥` default (`a ↤ x via Trunc ⊥ 0`). It is the same hint a
    /// `↦ T via HINT` conversion carries, with `T` the assigned cell's type.
    pub via: Option<Ident>,
}

#[derive(Debug, Clone, Copy)]
pub enum AssignOp {
    /// Ordinary assignment `←`.
    Assign,
    /// Conversion-directed assignment `↤` (U+21A4): evaluate the right side,
    /// convert it to the statically known type of the left place through the
    /// `↦` route, then assign. Never normalized into [`AssignOp::Assign`].
    ConversioAssign,
}

#[derive(Debug)]
pub struct VerteExpr {
    pub expr: Box<Expr>,
    pub ty: TypeExpr,
}

#[derive(Debug)]
pub struct FingeExpr {
    /// Qualified variant path: `[Bonum]` for same-module construction, or
    /// `[pkg, Bonum]` when the variant is reached through an owning union or
    /// an import alias (`finge pkg.Bonum { … }`). The terminal segment names
    /// the variant; a prefix segment names the union (same-module or imported).
    pub variant: Vec<Ident>,
    pub fields: Vec<FingeFieldInit>,
    /// `∷` cast — a full type annotation per the EBNF (`typeAnnotation`), so
    /// imported unions ascribe through a qualified name (`∷ pkg.Exitus`).
    pub cast: Option<TypeExpr>,
}

#[derive(Debug)]
pub struct FingeFieldInit {
    pub name: Ident,
    pub value: Box<Expr>,
    pub span: Span,
}

#[derive(Debug)]
pub struct ClausuraExpr {
    pub params: Vec<ClausuraParam>,
    pub ret: Option<TypeExpr>,
    pub err: Option<TypeExpr>,
    pub body: ClausuraBody,
}

#[derive(Debug)]
pub struct ClausuraParam {
    pub ty: TypeExpr,
    pub name: Ident,
    pub span: Span,
}

#[derive(Debug)]
pub enum ClausuraBody {
    Expr(Box<Expr>),
    Block(BlockStmt),
    Fac(FacStmt),
}

#[derive(Debug)]
pub struct CedeExpr {
    pub expr: Box<Expr>,
}

#[derive(Debug)]
pub struct ArrayExpr {
    pub elements: Vec<ArrayElement>,
}

#[derive(Debug)]
pub enum ArrayElement {
    Expr(Box<Expr>),
    Spread(Box<Expr>),
    /// Inference hole written as `_` in an expression shape list. Ordinary
    /// expression `_` keeps its existing name/placeholder rules.
    Infer(Span),
}

#[derive(Debug)]
pub struct ObjectExpr {
    pub fields: Vec<ObjectField>,
}

#[derive(Debug)]
pub struct ObjectField {
    pub key: ObjectKey,
    pub value: Option<Box<Expr>>, // None for shorthand
    pub span: Span,
}

#[derive(Debug)]
pub enum ObjectKey {
    Ident(Ident),
    String(Symbol),
    Computed(Box<Expr>),
    /// Copy-with-changes source (`Genus { … } ex source`, DECISIONS D15.1):
    /// every field the braces omit is copied from this value. Only the
    /// construction-literal `ex` suffix produces it, as the last field.
    Spread(Box<Expr>),
}

#[derive(Debug)]
pub struct IntervallumExpr {
    pub start: Box<Expr>,
    pub end: Box<Expr>,
    pub step: Option<Box<Expr>>,
    pub kind: RangeKind,
}

#[derive(Debug, Clone, Copy)]
pub enum RangeKind {
    Exclusive, // ‥ or ante
    Inclusive, // … or usque
}

#[derive(Debug)]
pub struct ConversioExpr {
    pub expr: Box<Expr>,
    pub target: ConversioTarget,
    /// Convert hint of a `via` clause (`x ↦ u32 via Hex`, D11.9). The hint is
    /// a compile-time identifier that selects the conversion row; it is never
    /// part of the target type. The pre-D11.9 spellings (a second type
    /// argument, a bracketed tail) are parse errors.
    pub via: Option<Ident>,
    pub recovery: Option<Box<Expr>>,
}

/// Target of a conversio expression.
#[derive(Debug)]
pub enum ConversioTarget {
    /// Runtime conversion to a declared type (`↦ numerus`, `↦ lista<T>`, …).
    Explicit(TypeExpr),
    /// Clamp target: inline range (`↦ 0‥10`) or stored `intervallum` value (`↦ fines`).
    Intervallum(Box<Expr>),
}

#[derive(Debug)]
pub struct ScriptumExpr {
    /// Surface spelling that claimed the builtin (`scriptum` or a pack alias).
    pub symbol: Symbol,
    pub template: Symbol,
    pub args: Vec<Expr>,
}

#[derive(Debug)]
pub struct LegeExpr {
    /// Surface spelling that claimed the builtin (`lege`/`lineam` or a pack alias).
    pub symbol: Symbol,
    /// `lineam` marker for line-oriented input.
    pub is_line: bool,

    pub span: Span,
}

/// Expression-form stream `ad` (blockless, evaluates to `sermo`).
#[derive(Debug)]
pub struct AdExpr {
    pub path: Symbol,
    /// Optional opener expression lowered to Request `data` as `valor`.
    pub opener: Option<Box<Expr>>,
    pub span: Span,
}

/// `praefixum { … }`: a constant's build-time initializer body (D8.4). The
/// block runs on the MIR runner during the build and `redde`s the value.
#[derive(Debug)]
pub struct PraefixumExpr {
    pub body: BlockStmt,
}

/// Build-time file embed: `insere "path"` (D8.10).
///
/// The path is a string literal, resolved package-relative by the driver
/// before semantic analysis; the parser records only the authored spelling.
#[derive(Debug)]
pub struct InsereExpr {
    pub path: Symbol,
    pub path_span: Span,
}

/// Runtime variant check: `expr est <type>` / `expr non est <type>`.
///
/// Produced by the parser when `est` is followed by a type expression. When
/// `est` is followed by a value expression, the parser keeps the legacy
/// `Binary(Is, ...)` shape (value equality synonym).
#[derive(Debug)]
pub struct TypeCheckExpr {
    /// `verum` for `est`, `falsum` for `non est`.
    pub positive: bool,
    pub expr: Box<Expr>,
    pub ty: TypeExpr,
}

// =============================================================================
// Types
// =============================================================================

/// One source-preserving generic application argument.
///
/// A bare identifier stays `Name` until the callee's declaration metadata is
/// available. Natural literals, shape tuples, and index holes use `Size`;
/// ordinary type syntax and explicit type holes use `Type` so the shipped
/// marker-hole surface remains distinct from compile-time indices.
#[derive(Debug, Clone)]
pub enum GenericArgExpr {
    Type(TypeExpr),
    Size(FiguraExpr),
    Name(Ident),
    /// A range after the width of a store policy (`saturating<u32, 0‥8>`): the
    /// refinement the store applies inside the explicit width. It is its own
    /// slot, not a type or a size, so `0‥W` stays unambiguous beside both.
    Range(RangeArgExpr),
}

/// The range argument of a ranged store policy (`lo‥hi` or `lo…hi`).
///
/// Both ends are always written. `∞` and `-∞` mean no bound on that side.
#[derive(Debug, Clone)]
pub struct RangeArgExpr {
    pub lo: RangeBoundExpr,
    pub hi: RangeBoundExpr,
    pub kind: RangeKind,
    /// The range was written after the `size` marker (`list<T, size 1…8>`):
    /// it bounds a count, not a value. A bare range bounds a value
    /// (`trapping<u8, 0…255>`), so a list refuses it by name.
    pub sized: bool,
    pub span: Span,
}

/// One written end of a [`RangeArgExpr`].
#[derive(Debug, Clone)]
pub enum RangeBoundExpr {
    /// Integer literal, optionally negated: `8`, `-2`.
    Literal { value: i128, span: Span },
    /// `∞` or `-∞`: no bound on this side.
    Unbounded { negative: bool, span: Span },
    /// A name (`W`), optionally spelled as a size lift (`size W`). A size
    /// bound is not a compile-time integer, so the checker refuses it by name.
    Name {
        name: Ident,
        lifted: bool,
        span: Span,
    },
}

impl RangeBoundExpr {
    /// Source span of the written bound.
    #[must_use]
    pub fn span(&self) -> Span {
        match self {
            Self::Literal { span, .. } | Self::Unbounded { span, .. } | Self::Name { span, .. } => {
                *span
            }
        }
    }
}

/// Authored compression level of one type-expr node.
///
/// Transient syntax fact captured during parsing and consumed by semantic
/// lowering to build authored-form presentation records. Level meaning is
/// interpreted per semantic constructor: `LONG` is the ordinary explicit head;
/// `SHORT` is the bare width (`u32`) or width-prefixed (`lu32`, `tf32[2, 2]`)
/// form where the constructor supports one. Compression never propagates: a
/// parent and each child carry their own level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TypeFormLevel(pub u8);

impl TypeFormLevel {
    /// Ordinary explicit head (`lista<numerus<u32>>`, `tensor<f32, [2, 2]>`).
    pub const LONG: Self = Self(0);
    /// Bare-width or width-prefixed compression (`u32`, `lu32`, `tf32[2, 2]`).
    pub const SHORT: Self = Self(1);
    /// Explicit default-policy head (`trapping<u8>`, la `exactus<u8>`): the
    /// author spelled out the policy word that the bare marker (`u8`) implies.
    /// The type is identical either way; the level only keeps the spelling.
    pub const POLICY: Self = Self(2);
    /// Explicit `wrapping<inf>` / la `modulus<inf>` head. The type is bare `inf`
    /// (nothing to wrap, inf-delivery ruling 32), so the presentation alone
    /// keeps which policy word the author wrote (ruling 49).
    pub const WRAPPING_INF: Self = Self(3);
    /// Explicit `saturating<inf>` / la `saturatus<inf>` head (see [`Self::WRAPPING_INF`]).
    pub const SATURATING_INF: Self = Self(4);
    /// The type slot of a bare `{ field = value }` literal was left out
    /// (implied hole with a default).
    ///
    /// Distinct from an explicit hole (`_ { field = value }`, form [`Self::LONG`]):
    /// the bare form defaults to a record shaped by its fields, while an
    /// explicit hole has no default and fails closed without an expected type.
    pub const BARE_LITERAL: Self = Self(5);
}

#[derive(Debug, Clone)]
pub struct TypeExpr {
    /// Legacy `si` nullability marker.
    ///
    /// This field remains explicit for migration safety, but new nullable value
    /// types are represented as [`TypeExprKind::Union`] with `nihil` as a
    /// member.
    pub nullable: bool,

    /// Access-discipline marker from `de`/`in`/`own`/`copy`, if written in type position.
    pub mode: Option<TypeMode>,

    pub kind: TypeExprKind,
    pub span: Span,

    /// Authored compression level of this node. See [`TypeFormLevel`].
    pub form: TypeFormLevel,

    /// Optional contextual aggregate label at this type-argument position.
    ///
    /// `iuncta` and `ratio` lists use the same carrier, while `None` denotes an
    /// unlabeled slot. Identity consumers read `kind` only; labels are a
    /// checker-side alias layer populated from this field.
    pub iuncta_label: Option<Ident>,
}

/// Access-discipline marker written in type position.
///
/// This is syntax-level intent. Later phases decide whether the selected target
/// or semantic context supports the mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypeMode {
    /// `de` / `ref`: shared reference-like mode.
    De,

    /// `in` / `mut`: mutable reference-like mode.
    In,

    /// `own` / `penes`: exclusive value; transfer moves.
    Own,

    /// `copy` / `exemplum`: duplicate-then-own at acquisition.
    Copy,
}

/// Type-level tensor shape (Figura) in `tensor<T, Figura>`.
#[derive(Debug, Clone)]
pub enum FiguraExpr {
    /// Inference hole: `_`.
    Infer,
    /// Compile-time natural literal (including `0`).
    Literal(u64),
    /// Index variable (`magnitudo N` or repetition in `[N, M]`).
    Var(Ident),
    /// Add a natural constant to an index term (`N + 1`).
    Add(Box<FiguraExpr>, u64),
    /// Subtract a natural constant from an index term (`N - 1`).
    Subtract(Box<FiguraExpr>, u64),
    /// Divide an index term exactly by a positive natural constant (`D / 2`).
    Divide(Box<FiguraExpr>, u64),
    /// Spelled size lift in an extent position: `size <primary>` (la
    /// `magnitudo <primary>`). The lift is a store, so its value is checked
    /// at that extent (size-type Z2).
    Lift(Box<LiftExtent>),
    /// Rank-n shape `[d0, d1, …]`; empty tuple is rank-0 `[]`.
    Tuple(Vec<FiguraExpr>),
}

/// Primary payload of a spelled size lift extent (`size <primary>`).
#[derive(Debug, Clone)]
pub enum LiftExtent {
    /// Natural literal: `size 3` folds statically.
    Literal(u64),
    /// Value or size name: `size n`.
    Name(Ident),
    /// Negative integer literal: `size -1`. Rejected at resolve
    /// (`size_lift_negative`), like the size-declaration lift store.
    Negative { value: u64, span: Span },
    /// Parenthesized figura term: `size (H + 1)`.
    Paren(Box<FiguraExpr>),
    /// Whole shape: `size [4]`, `size [2, 3]` (empty is rank-0 `[]`). Each
    /// dimension is a figura term; the call-argument position this form
    /// serves admits natural-literal dimensions only.
    Shape(Vec<FiguraExpr>),
}

/// Syntactic variants inside [`TypeExpr`].
///
/// This enum is deliberately not the compiler's checked type model. It preserves
/// source spelling such as `_`, generic application, collection syntax, function
/// types, and inline unions so type checking can report errors against the
/// syntax the user wrote.
#[derive(Debug, Clone)]
pub enum TypeExprKind {
    /// Inference hole: `_`.
    Infer,
    /// Union-shaped inference hole: a standalone `∪` in type position. The
    /// hole consumes the whole type expression; it is distinct from `Infer`
    /// (monomorphic hole) and from `Union` (explicit member list).
    UnionHole,
    /// Field-list hole: `*` as the sole argument of `record<...>` (la
    /// `ratio<*>`). It stands for the whole field list, solved from the
    /// shape. The whole `record<*>` spelling is this one node; it has no
    /// `Named` wrapper.
    FieldListHole,
    Named(Ident, Vec<GenericArgExpr>),
    /// Qualified named type: `namespace.Type`.
    Qualified(Vec<Ident>, Vec<GenericArgExpr>),
    /// Indexed tensor: `tensor<T, Figura>`.
    Tensor(Box<TypeExpr>, FiguraExpr),
    /// Indexed sparse tensor: `sparsa<T, Figura>`.
    Sparsa(Box<TypeExpr>, FiguraExpr),
    /// Register-class vector: `vector<T, size N>`.
    Vector(Box<TypeExpr>, FiguraExpr),
    /// Register-class matrix: `matrix<T, [R, C]>`.
    Matrix(Box<TypeExpr>, FiguraExpr),
    /// Storage atomic cell: `atomic<T>`.
    Atomic(Box<TypeExpr>),
    /// Two-channel promise: `promissum<T ⇥ E>`.
    PromissumFailable(Box<TypeExpr>, Box<TypeExpr>),
    Func(FuncTypeExpr),
    /// Inline union: `T ∪ U` (including `T ∪ nihil` for nullables).
    Union(Vec<TypeExpr>),
    /// Inline intersection: `T ∩ U`. Binds tighter than `∪`; nested
    /// intersection members are flattened at parse time like unions.
    Intersection(Vec<TypeExpr>),
}

#[derive(Debug, Clone)]
pub struct FuncTypeExpr {
    pub params: Vec<TypeExpr>,
    pub ret: Box<TypeExpr>,
    pub err: Option<Box<TypeExpr>>,
}

// =============================================================================
// Annotations
// =============================================================================

#[derive(Debug)]
pub struct Annotation {
    pub kind: AnnotationKind,
    pub span: Span,
}

/// Annotation taxonomy preserved by the parser.
///
/// An annotation may describe target translation, CLI/package surfaces, access
/// control, async/cursor behavior, test selection, or raw metadata. Syntax keeps
/// those families distinct without deciding whether a specific declaration may
/// legally use them; that policy belongs to the consumer phase.
#[derive(Debug)]
pub enum AnnotationKind {
    Statement(AnnotationStmt),
    /// `@ radix typus T in textus lista<textus> ...`
    RadixTypusDomain(RadixTypusDomainAnnotation),
    /// `@ radix lane "air" / "mir" / "hir-direct"`
    RadixLane(RadixLaneAnnotation),
    /// `@ nucleum` / `@ nucleum fragment` — typed modifier data, not an untyped argument.
    Nucleum(NucleumAnnotation),
    /// `@ conversio` marker on a standalone function declaration. The ordered
    /// concrete `(source, destination)` pair is derived from the function
    /// signature during semantic enrollment; the marker carries no payload.
    Conversio,
    /// `@ ad '<route>'` — compiler-owned route handler registration on a
    /// top-level `functio` (en `@ call '<route>'`). Legality (placement,
    /// handler shape, reserved routes) is semantic policy, not syntax.
    Ad(AdAnnotation),
    Cli(CliAnnotation),
    Imperium(ImperiumAnnotation),
    Optio(OptioAnnotation),
    Operandus(OperandusAnnotation),
    Futura,
    Cursor,
    Tag,
    Solum,
    Omitte,
    Metior,
    Publica,
    Protecta,
    Privata,
    /// `@ interna` package-internal visibility annotation.
    Interna,
    /// `@ name { field = expr, … }`
    Braced(BracedAnnotation),
}

#[derive(Debug)]
pub struct AnnotationStmt {
    /// Raw annotation name for forms not modeled as structured annotations.
    pub name: Ident,

    /// Token payload preserved for the eventual consumer.
    pub args: Vec<Token>,
}

#[derive(Debug)]
pub struct RadixTypusDomainAnnotation {
    pub param: Ident,
    pub allowed: Vec<TypeExpr>,
}

#[derive(Debug)]
pub struct RadixLaneAnnotation {
    pub lane: Symbol,
}

/// The safety tier of a kernel (operator ruling 2026-10-28,
/// `docs/factory/faber-language-decisions/kernel-math-tiers-ruling-2026-10-28.md`).
///
/// The tiers are ordered and cumulative: `Unsafe` includes everything
/// `Relaxed` allows. They are one three-state choice, never independent
/// flags. The identities are the Latin keyword keys (`laxus`, `intutus`);
/// every reader locale spells them through its pack (en `relaxed` /
/// `unsafe`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KernelTier {
    /// IEEE-correct float math with FMA contraction, reduction reordering and
    /// published-bound transcendentals; unproven index checks stay.
    #[default]
    Default,
    /// Adds approximate `/` and `sqrt`, fast transcendentals, flush-to-zero
    /// and reassociation of ordinary float expressions.
    Relaxed,
    /// Adds removal of the index checks the compiler cannot prove.
    Unsafe,
}

impl KernelTier {
    /// The Latin keyword identity written in the HIR annotation and looked up
    /// in the locale packs; `None` for the default tier, which has no word.
    #[must_use]
    pub const fn identity(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Relaxed => Some("laxus"),
            Self::Unsafe => Some("intutus"),
        }
    }

    /// Resolve a keyword identity back to its tier.
    #[must_use]
    pub fn from_identity(identity: &str) -> Option<Self> {
        match identity {
            "laxus" => Some(Self::Relaxed),
            "intutus" => Some(Self::Unsafe),
            _ => None,
        }
    }

    /// Stable lower-case name used by reports (`default`, `relaxed`,
    /// `unsafe`); this is a report label, not a source spelling.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Relaxed => "relaxed",
            Self::Unsafe => "unsafe",
        }
    }

    /// Whether the tier allows relaxed float math (`Relaxed` and `Unsafe`).
    #[must_use]
    pub const fn allows_relaxed_math(self) -> bool {
        !matches!(self, Self::Default)
    }

    /// Whether the tier allows dropping unproven index checks (`Unsafe`).
    #[must_use]
    pub const fn allows_unchecked_indexing(self) -> bool {
        matches!(self, Self::Unsafe)
    }
}

/// Structured `@ nucleum` annotation, with the optional `fragment` modifier.
/// The modifier is typed data on this family, not an untyped annotation
/// argument; `fragment` remains distinct from graphics `@ fragment`. The
/// removed `lane` binding has no AST shape: the parser rejects its spelling.
#[derive(Debug)]
pub struct NucleumAnnotation {
    /// True when the `fragment` modifier is present (`@ nucleum fragment` or
    /// `@ nucleum { fragment = verum }`).
    pub is_fragment: bool,
    /// The kernel safety tier the author chose (`@ nucleum laxus`,
    /// `@ nucleum intutus`); [`KernelTier::Default`] when no tier word is
    /// written. Only the sugar form carries a tier, and never together with
    /// `fragment`: a fragment composes into an entry and takes that entry's
    /// tier.
    pub tier: KernelTier,
    /// True when the braced record form (`@ nucleum { … }`) was authored;
    /// re-emission keeps the authored brace shape.
    pub braced: bool,
}

/// Structured `@ ad '<route>'` handler registration.
///
/// The route is the single-quoted ASCII literal, the same literal form as
/// expression `ad`. Only exact routes exist; pattern routes are deferred.
#[derive(Debug)]
pub struct AdAnnotation {
    pub route: Symbol,
    pub route_span: Span,
}

#[derive(Debug)]
pub struct CliAnnotation {
    pub name: Symbol,
}

#[derive(Debug)]
pub struct ImperiumAnnotation {
    pub name: Symbol,
}

#[derive(Debug)]
pub struct OptioAnnotation {
    pub binding: Ident,

    /// Optional declared value type. Flags may omit this.
    pub ty: Option<TypeExpr>,
    pub short: Option<Symbol>,
    pub long: Option<Symbol>,
    pub is_flag: bool,
    pub description: Option<Symbol>,

    /// Whether the option applies outside one command scope.
    pub global: bool,
    pub default: Option<Box<Expr>>,
}

#[derive(Debug)]
pub struct OperandusAnnotation {
    /// Whether this positional operand captures the remaining arguments.
    pub is_rest: bool,
    pub ty: TypeExpr,
    pub binding: Ident,
    pub description: Option<Symbol>,

    /// Whether the operand applies outside one command scope.
    pub global: bool,
    pub default: Option<Box<Expr>>,
}

/// Canonical braced annotation: `@ family { field = value, … }`.
#[derive(Debug)]
pub struct BracedAnnotation {
    /// Annotation family identifier (`optio`, `futura`, `cli`, …).
    pub name: Ident,
    pub fields: Vec<AnnotationField>,
}

/// One `name = value` field in a braced annotation body.
#[derive(Debug)]
pub struct AnnotationField {
    pub name: Ident,
    pub value: AnnotationFieldValue,
    pub span: Span,
}

/// Braced-annotation field payload: expression or type position.
#[derive(Debug)]
pub enum AnnotationFieldValue {
    Expr(Box<Expr>),
    Type(TypeExpr),
}
