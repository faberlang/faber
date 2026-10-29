//! Keyword spelling registry for lexer and language-tool policy.
//!
//! The registry is the single source of truth for keyword identity. Every
//! keyword — in any reader locale — resolves to a locale-independent identity
//! token via [`lookup_keyword_spec`]; the parser claims keywords by that
//! identity in its grammar slots.
//!
//! WHY THIS EXISTS
//! ===============
//! Keywords are locale items: a reader pack re-spells the same identity for
//! each language, and the lexer never emits keyword tokens — every keyword
//! lexes as an identifier with its latin key recorded, so the parser resolves
//! identity through this registry in any locale. Keeping that intent in a
//! structured registry lets diagnostics and explain output work from one
//! taxonomy instead of rediscovering policy from a large scanner `match`.
//!
//! INVARIANTS
//! ==========
//! - `token_kind: Some(_)` is the keyword's locale-independent **identity**
//!   token. It is never emitted by the lexer (there are no reserved words);
//!   the parser references keywords by this identity.
//! - `scope` records where a keyword is claimed (parser-owned slots, test
//!   surface, annotation names, or aliases). There is no global scope — no
//!   keyword is reserved everywhere.
//! - `Alias` entries name their canonical spelling; the alias spelling is
//!   claimed by identity in the canonical keyword's slots.
//! - Annotation spellings are registry entries, not normal-mode reserved words.
//! - Keyword groups declare source-surface completeness constraints for future
//!   reader packs; missing one member of a group is a pack validation error.

use std::mem;
use std::sync::OnceLock;

use handroll::FxHashMap;

use super::token::TokenKind;

/// Where a keyword spelling is valid or intended to be owned.
///
/// Scope is policy metadata, not automatic lexer behavior. It tells tools and
/// parser work whether a spelling is context-owned, annotation-only,
/// test-owned, or an alias.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordScope {
    /// Intended for one or more parser contexts.
    Contextual(&'static [KeywordOwner]),

    /// Annotation name or annotation-owned metadata after `@`.
    Annotation,

    /// Test syntax surface owned by the test language layer.
    TestOwned,

    /// Accepted spelling whose canonical replacement is named here.
    Alias { canonical: &'static str },
}

/// Parser or tooling surface that owns a contextual keyword spelling.
///
/// These owners let diagnostics describe why a word is special without turning
/// every contextual use into a global language reservation forever.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordOwner {
    /// Entrypoint-level modifier or command metadata.
    EntryModifier,

    /// Function declaration modifier.
    FunctionModifier,

    /// Class-like `genus` header syntax.
    GenusHeader,

    /// Class-like `genus` member syntax.
    GenusMember,

    /// Rest binding in patterns or parameters.
    RestPattern,

    /// Spread expression syntax.
    SpreadExpression,

    /// Annotation name position.
    AnnotationName,

    /// Annotation argument or modifier position.
    AnnotationModifier,

    /// Member name position where keyword-looking words may be accepted.
    MemberIdentifier,

    /// Top-level declaration name (`implendum`, `genus`, `typus`, `ordo`, …).
    DeclarationName,

    /// Parameter or field name after the type expression.
    ParameterName,

    /// Binding target in `fixum`/`varia`/`sit` patterns and captures.
    BindingTarget,

    /// Import visibility surface.
    ImportVisibility,

    /// Field name inside a braced import record (`visibilitas = publica`).
    ///
    /// The record's other field names are the existing identities `ex`,
    /// `nomen`, `ut` and `omnia`; the parser resolves all five through the
    /// reader pack.
    ImportField,

    /// Type-parameter modifier surface.
    TypeParameter,

    /// Parameter passing mode.
    ParameterMode,

    /// Import or binding alias marker.
    AliasMarker,

    /// Iteration mode keyword.
    IterationMode,

    /// Optional callable-posture spelling in a signature slot
    /// (`fiet` / `fiunt` / `fient`).
    CallablePosture,

    /// Await/yield control-operation spelling in a statement or declaration slot
    /// (`figendum` / `variandum` / `reddet` / `tacebit`).
    ControlOperation,

    /// Type-name position in a type expression (`modulus<W>`, `iuncta<T, U>`).
    TypeName,

    /// Constructor-expression position (`iuncta<T, U> [a, b]`).
    Constructor,

    /// Output statement in statement-initial position
    /// (`scribe` / `vide` / `mone` / `nota`).
    ScribeStatement,

    /// Builtin call in expression-primary position
    /// (`lege` / `lineam` / `scriptum`).
    BuiltinCall,

    /// Literal value in expression or pattern position
    /// (`verum` / `falsum` / `nihil`).
    Literal,

    /// Logical or equality operator in expression position
    /// (`aut` / `et` / `non` / `est` / `vel`; `et` also separates
    /// arm-head patterns).
    Operator,

    /// Statement-initial declaration introducer
    /// (`functio` / `genus` / `ordo` / `implendum` / `typus` / `discretio` /
    /// `fixum` / `varia` / `sit`; `fixum`/`varia` also mark mutability in
    /// binding statements and pattern captures).
    DeclarationIntroducer,

    /// Control-flow keyword in statement or expression position
    /// (`si` / `secus` / `sin` / `ergo` / `discerne` / `elige` / `dum` /
    /// `fac` / `casu` / `ceterum` / `rumpe` / `perge` / `redde`; `casu` also
    /// marks pattern guards).
    ControlFlow,

    /// Expression-introducing keyword in primary position
    /// (`ego` / `finge` / `clausura` / `praefixum`).
    ExpressionIntroducer,

    /// Range-operator keyword in expression position
    /// (`per` / `usque` / `ante`).
    RangeOperator,

    /// Retired membership-operator spelling in operator position
    /// (`intra` / `inter`, and every reader pack's spelling of them).
    ///
    /// The words stay recognized only so the parser can answer them with the
    /// migration diagnostic `membership_keyword_retired` (`∈` is the one
    /// spelling); elsewhere they are ordinary identifiers.
    RetiredMembership,

    /// Statement-initial transfer keyword (`tacet`).
    StatementIntroducer,

    /// Marker inside a declaration or dispatch
    /// (`sub` / `implet` / `omnia` / `sponte`).
    DeclarationMarker,

    /// Modifier word directly after `fac` (`omnia` / `machina`).
    FacModifier,

    /// Route expression in expression or statement position (`ad`).
    RouteExpression,

    /// Convert-hint clause after a `↦` target type (`u32 via Hex`, D11.9).
    ConversionHint,

    /// Head of the `reducta via Op ex source …` expression (`reducta`, and
    /// `via` as the mandatory operator introducer).
    ReductionHead,

    /// Operator word after `reducta via` (`summa` / `factum` / `maxima` /
    /// `minima` / `argmaxima` / `argminima` / `omnia` / `quilibet` /
    /// `numeratio` / `prima`).
    ReductionOperator,

    /// Grouping word after the source and optional `apud` clause (`pro`).
    ReductionGroup,

    /// Grouping word in an ordered prefix scan (`pro`).
    ScanGroup,

    /// Head of the `ordinata via Dir ex source …` sort expression
    /// (`ordinata`, with `via` as the mandatory direction introducer).
    SortHead,

    /// Direction word after `ordinata via` (`ascendens` / `descendens`).
    SortDirection,

    /// Head of the ordered `scan` expression.
    ScanHead,

    /// Inclusive/exclusive mode introducer in a scan expression.
    ScanTotal,

    /// Inclusive or exclusive scan mode after `total`.
    ScanTotalMode,

    /// Ordering clause introducer in a scan expression.
    ScanOrder,
}

/// Keyword grouping used by diagnostics, explain data, and registry consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordKind {
    /// Declaration-introducing syntax.
    Declaration,

    /// Declaration or behavior modifier.
    Modifier,

    /// Branching, looping, or pattern-control syntax.
    ControlFlow,

    /// Function or loop transfer syntax.
    Transfer,

    /// Error and assertion syntax.
    ErrorHandling,

    /// Async, generator, or closure-adjacent syntax.
    Async,

    /// Built-in literal spelling.
    Literal,

    /// Word operator.
    Operator,

    /// Object, self, contract, or construction syntax.
    Object,

    /// Type conversion or construction syntax.
    TypeOperation,

    /// User-visible output helper.
    Output,

    /// Program entrypoint surface.
    EntryPoint,

    /// Endpoint binding syntax.
    Endpoint,

    /// Miscellaneous language surface not yet split into a narrower category.
    Misc,

    /// Range operator word.
    Range,

    /// Test declaration or test modifier syntax.
    Testing,

    /// Annotation metadata.
    Annotation,
}

/// Registry row for one known keyword or keyword-like spelling.
///
/// This is a public data contract for tools that need to explain or audit the
/// language surface. It is not a generated lexer table: consumers must still
/// check `token_kind` and scope before assuming a spelling is reserved.
#[derive(Debug, Clone)]
pub struct KeywordSpec {
    /// Source spelling as users write it.
    pub text: &'static str,

    /// Locale-independent keyword identity token (never emitted by the lexer).
    pub token_kind: Option<TokenKind>,

    /// Ownership and reservation policy for this spelling.
    pub scope: KeywordScope,

    /// Taxonomy bucket used by diagnostics and reference tooling.
    pub category: KeywordKind,
}

impl KeywordSpec {
    /// Return whether this keyword has an identity token.
    ///
    /// The lexer never emits keyword tokens, so this is identity presence, not
    /// lexer emission. Tokenless rows such as `vacua` return false.
    #[must_use]
    pub fn currently_lexes_as_keyword(&self) -> bool {
        self.token_kind.is_some()
    }
}

/// Keyword families that must be localized together in a complete reader pack.
#[derive(Debug, Clone)]
pub struct KeywordGroupSpec {
    /// Stable group id used by pack validation diagnostics.
    pub id: &'static str,

    /// Latin keyword keys that belong to the group. Spelling-keyed so
    /// tokenless members stay referenceable without a `TokenKind`.
    pub members: &'static [&'static str],
}

/// Validation issue for the static keyword group declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeywordGroupValidationError {
    /// A group is missing its required members.
    MissingGroupMembers { group: &'static str },

    /// A group references a keyword key that has no registry row.
    UnknownMember { group: &'static str, key: String },
}

const GENUS_HEADER: &[KeywordOwner] = &[KeywordOwner::GenusHeader];
const GENUS_MEMBER: &[KeywordOwner] = &[KeywordOwner::GenusMember];
const IMPORT_VISIBILITY_ANNOTATION_OR_MEMBER: &[KeywordOwner] = &[
    KeywordOwner::ImportVisibility,
    KeywordOwner::AnnotationName,
    KeywordOwner::MemberIdentifier,
];
const IMPORT_FIELD: &[KeywordOwner] = &[KeywordOwner::ImportField];
const ENTRY_FUNCTION_OR_PARAM_OR_MEMBER: &[KeywordOwner] = &[
    KeywordOwner::EntryModifier,
    KeywordOwner::FunctionModifier,
    KeywordOwner::DeclarationName,
    KeywordOwner::ParameterName,
    KeywordOwner::MemberIdentifier,
];
const ANNOTATION_NAME: &[KeywordOwner] = &[KeywordOwner::AnnotationName];
const TYPE_PARAMETER: &[KeywordOwner] = &[KeywordOwner::TypeParameter];
const REST_PATTERN: &[KeywordOwner] =
    &[KeywordOwner::RestPattern, KeywordOwner::AnnotationModifier];
const FUNCTION_MODIFIER: &[KeywordOwner] = &[KeywordOwner::FunctionModifier];
const ENTRY_OR_FUNCTION_MODIFIER: &[KeywordOwner] =
    &[KeywordOwner::EntryModifier, KeywordOwner::FunctionModifier];
const PARAMETER_MODE: &[KeywordOwner] = &[KeywordOwner::ParameterMode, KeywordOwner::IterationMode];
const ALIAS_MARKER: &[KeywordOwner] = &[KeywordOwner::AliasMarker];
const ITERATION_MODE: &[KeywordOwner] = &[KeywordOwner::IterationMode];
const SPREAD_EXPRESSION: &[KeywordOwner] = &[KeywordOwner::SpreadExpression];
const CALLABLE_POSTURE: &[KeywordOwner] = &[KeywordOwner::CallablePosture];
const CONTROL_OPERATION: &[KeywordOwner] = &[KeywordOwner::ControlOperation];
const TYPE_NAME: &[KeywordOwner] = &[KeywordOwner::TypeName];
const TYPE_NAME_OR_CONSTRUCTOR: &[KeywordOwner] =
    &[KeywordOwner::TypeName, KeywordOwner::Constructor];
const SCRIBE_STATEMENT: &[KeywordOwner] = &[KeywordOwner::ScribeStatement];
const BUILTIN_CALL: &[KeywordOwner] = &[KeywordOwner::BuiltinCall];
const LITERAL: &[KeywordOwner] = &[KeywordOwner::Literal];
const OPERATOR: &[KeywordOwner] = &[KeywordOwner::Operator];
const DECLARATION_INTRODUCER: &[KeywordOwner] = &[KeywordOwner::DeclarationIntroducer];
/// The writable-binding cell: `varia` (local, field, binder) and `in`
/// (parameter, argument marker) are one cell, so a pack spells them with one
/// word and the word is claimed in every position either key was.
const MUT_CELL: &[KeywordOwner] = &[
    KeywordOwner::DeclarationIntroducer,
    KeywordOwner::ParameterMode,
    KeywordOwner::IterationMode,
];
const CONTROL_FLOW: &[KeywordOwner] = &[KeywordOwner::ControlFlow];
const EXPRESSION_INTRODUCER: &[KeywordOwner] = &[KeywordOwner::ExpressionIntroducer];
const RANGE_OPERATOR: &[KeywordOwner] = &[KeywordOwner::RangeOperator];
const RETIRED_MEMBERSHIP: &[KeywordOwner] = &[KeywordOwner::RetiredMembership];
const STATEMENT_INTRODUCER: &[KeywordOwner] = &[KeywordOwner::StatementIntroducer];
const DECLARATION_MARKER: &[KeywordOwner] = &[KeywordOwner::DeclarationMarker];
const REDUCTION_HEAD: &[KeywordOwner] = &[KeywordOwner::ReductionHead];
const REDUCTION_OPERATOR: &[KeywordOwner] = &[KeywordOwner::ReductionOperator];
const REDUCTION_SCAN_GROUP: &[KeywordOwner] =
    &[KeywordOwner::ReductionGroup, KeywordOwner::ScanGroup];
const SORT_HEAD: &[KeywordOwner] = &[KeywordOwner::SortHead];
const SORT_DIRECTION: &[KeywordOwner] = &[KeywordOwner::SortDirection];
const SCAN_HEAD: &[KeywordOwner] = &[KeywordOwner::ScanHead];
const SCAN_TOTAL: &[KeywordOwner] = &[KeywordOwner::ScanTotal];
const SCAN_TOTAL_MODE: &[KeywordOwner] = &[KeywordOwner::ScanTotalMode];
const SCAN_ORDER: &[KeywordOwner] = &[KeywordOwner::ScanOrder];
// WHY: `omnia` keeps its declaration-marker owner and `via` keeps its
// convert-hint owner. Each also gains the reduction owner for its new slot.
const DECLARATION_MARKER_OR_REDUCTION_OPERATOR: &[KeywordOwner] = &[
    KeywordOwner::DeclarationMarker,
    KeywordOwner::ReductionOperator,
    KeywordOwner::FacModifier,
];
// WHY: `machina` (en `device`) is claimed only directly after `fac`; everywhere
// else it stays an ordinary identifier, so `let int device ← 1` is legal.
const FAC_MODIFIER: &[KeywordOwner] = &[KeywordOwner::FacModifier];
const CONVERSION_HINT_REDUCTION_SCAN_HEAD: &[KeywordOwner] = &[
    KeywordOwner::ConversionHint,
    KeywordOwner::ReductionHead,
    KeywordOwner::ScanHead,
];
// WHY: `ad` opens the route expression and names the compiler-owned
// `@ ad '<route>'` handler annotation; claiming the annotation slot seals the
// locale surface (`@ ad` under an en reader is `wrong_locale_annotation`).
const ROUTE_EXPRESSION_OR_ANNOTATION: &[KeywordOwner] =
    &[KeywordOwner::RouteExpression, KeywordOwner::AnnotationName];
const ANNOTATION_MODIFIER: &[KeywordOwner] = &[KeywordOwner::AnnotationModifier];
// WHY: test annotation spellings are valid after `@` and as import/decl names
// (`privata solum`), but not as bare type or binding identifiers (`fixum solum x`).
const TEST_ANNOTATION_NAME: &[KeywordOwner] = &[
    KeywordOwner::AnnotationName,
    KeywordOwner::DeclarationName,
    KeywordOwner::ParameterName,
    KeywordOwner::MemberIdentifier,
];

const CONDITIONAL_GROUP: &[&str] = &["si", "sin", "secus"];
const CHOICE_GROUP: &[&str] = &["elige", "casu", "ceterum"];
const PATTERN_CHOICE_GROUP: &[&str] = &["discerne", "casu", "ceterum"];
const TRY_RECOVERY_GROUP: &[&str] = &["cape"];
const LOOP_GROUP: &[&str] = &["dum", "fac", "itera", "apud", "perge", "rumpe"];
const CLOSURE_GROUP: &[&str] = &["clausura", "nucleum"];
const RELATIONAL_VIEW_GROUP: &[&str] = &["schema", "columna"];

/// Complete keyword-group declarations for static Latin source-surface data.
pub static KEYWORD_GROUP_SPECS: &[KeywordGroupSpec] = &[
    KeywordGroupSpec {
        id: "conditional",
        members: CONDITIONAL_GROUP,
    },
    KeywordGroupSpec {
        id: "choice",
        members: CHOICE_GROUP,
    },
    KeywordGroupSpec {
        id: "pattern_choice",
        members: PATTERN_CHOICE_GROUP,
    },
    KeywordGroupSpec {
        id: "try_recovery",
        members: TRY_RECOVERY_GROUP,
    },
    KeywordGroupSpec {
        id: "loop",
        members: LOOP_GROUP,
    },
    KeywordGroupSpec {
        id: "closure",
        members: CLOSURE_GROUP,
    },
    KeywordGroupSpec {
        id: "relational_view",
        members: RELATIONAL_VIEW_GROUP,
    },
];

/// Complete registry of current, contextual, alias, and annotation spellings.
pub static KEYWORD_SPECS: &[KeywordSpec] = &[
    // === Keywords: Declarations ===
    KeywordSpec {
        text: "discretio",
        token_kind: Some(TokenKind::Discretio),
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::Declaration,
    },
    KeywordSpec {
        text: "fixum",
        token_kind: Some(TokenKind::Fixum),
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::Declaration,
    },
    KeywordSpec {
        text: "functio",
        token_kind: Some(TokenKind::Functio),
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::Declaration,
    },
    KeywordSpec {
        text: "genus",
        token_kind: Some(TokenKind::Genus),
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::Declaration,
    },
    KeywordSpec {
        text: "iuncta",
        token_kind: Some(TokenKind::Iuncta),
        scope: KeywordScope::Contextual(TYPE_NAME_OR_CONSTRUCTOR),
        category: KeywordKind::Declaration,
    },
    KeywordSpec {
        text: "implendum",
        token_kind: Some(TokenKind::Implendum),
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::Declaration,
    },
    KeywordSpec {
        text: "importa",
        token_kind: Some(TokenKind::Importa),
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::Declaration,
    },
    // `regio` (en `module`) names the file (D7.7). Tokenless: the parser
    // claims it only as the first declaration of a file; everywhere else the
    // spelling stays an ordinary identifier (F8 contextual keyword).
    KeywordSpec {
        text: "regio",
        token_kind: None,
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::Declaration,
    },
    KeywordSpec {
        text: "modulus",
        token_kind: Some(TokenKind::Modulus),
        scope: KeywordScope::Contextual(TYPE_NAME),
        category: KeywordKind::TypeOperation,
    },
    KeywordSpec {
        text: "ordo",
        token_kind: Some(TokenKind::Ordo),
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::Declaration,
    },
    KeywordSpec {
        text: "sit",
        token_kind: Some(TokenKind::Sit),
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::Declaration,
    },
    KeywordSpec {
        text: "typus",
        token_kind: Some(TokenKind::Typus),
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::Declaration,
    },
    // `schema` introduces a relational heading; `columna` declares one column
    // inside a schema body. Both claim the declaration-introducer slot.
    KeywordSpec {
        text: "columna",
        token_kind: Some(TokenKind::Columna),
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::Declaration,
    },
    KeywordSpec {
        text: "schema",
        token_kind: Some(TokenKind::Schema),
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::Declaration,
    },
    KeywordSpec {
        text: "varia",
        token_kind: Some(TokenKind::Varia),
        scope: KeywordScope::Contextual(MUT_CELL),
        category: KeywordKind::Declaration,
    },
    // === Keywords: Modifiers ===
    KeywordSpec {
        text: "ceteri",
        token_kind: Some(TokenKind::Ceteri),
        scope: KeywordScope::Contextual(REST_PATTERN),
        category: KeywordKind::Modifier,
    },
    KeywordSpec {
        text: "errata",
        token_kind: Some(TokenKind::Errata),
        scope: KeywordScope::Contextual(FUNCTION_MODIFIER),
        category: KeywordKind::Modifier,
    },
    KeywordSpec {
        text: "exitus",
        token_kind: Some(TokenKind::Exitus),
        scope: KeywordScope::Contextual(ENTRY_OR_FUNCTION_MODIFIER),
        category: KeywordKind::Modifier,
    },
    KeywordSpec {
        text: "generis",
        token_kind: Some(TokenKind::Generis),
        scope: KeywordScope::Contextual(GENUS_MEMBER),
        category: KeywordKind::Modifier,
    },
    KeywordSpec {
        text: "iacit",
        token_kind: Some(TokenKind::Iacit),
        scope: KeywordScope::Contextual(FUNCTION_MODIFIER),
        category: KeywordKind::Modifier,
    },
    KeywordSpec {
        text: "immutata",
        token_kind: Some(TokenKind::Immutata),
        scope: KeywordScope::Contextual(FUNCTION_MODIFIER),
        category: KeywordKind::Modifier,
    },
    KeywordSpec {
        text: "interna",
        token_kind: Some(TokenKind::Interna),
        scope: KeywordScope::Contextual(IMPORT_VISIBILITY_ANNOTATION_OR_MEMBER),
        category: KeywordKind::Modifier,
    },
    KeywordSpec {
        text: "magnitudo",
        token_kind: Some(TokenKind::Magnitudo),
        scope: KeywordScope::Contextual(TYPE_PARAMETER),
        category: KeywordKind::Modifier,
    },
    KeywordSpec {
        text: "optiones",
        token_kind: Some(TokenKind::Optiones),
        scope: KeywordScope::Contextual(FUNCTION_MODIFIER),
        category: KeywordKind::Modifier,
    },
    KeywordSpec {
        text: "privata",
        token_kind: Some(TokenKind::Privata),
        scope: KeywordScope::Contextual(IMPORT_VISIBILITY_ANNOTATION_OR_MEMBER),
        category: KeywordKind::Modifier,
    },
    KeywordSpec {
        text: "protecta",
        token_kind: Some(TokenKind::Protecta),
        scope: KeywordScope::Contextual(ANNOTATION_NAME),
        category: KeywordKind::Modifier,
    },
    KeywordSpec {
        text: "publica",
        token_kind: Some(TokenKind::Publica),
        scope: KeywordScope::Contextual(IMPORT_VISIBILITY_ANNOTATION_OR_MEMBER),
        category: KeywordKind::Modifier,
    },
    // `visibilitas` names the visibility field of a braced import record
    // (`import { from = "…", visibility = public }`). It is claimed only in that
    // field-name slot and stays an ordinary identifier everywhere else.
    KeywordSpec {
        text: "visibilitas",
        token_kind: None,
        scope: KeywordScope::Contextual(IMPORT_FIELD),
        category: KeywordKind::Modifier,
    },
    KeywordSpec {
        text: "sponte",
        token_kind: Some(TokenKind::Sponte),
        scope: KeywordScope::Contextual(DECLARATION_MARKER),
        category: KeywordKind::Modifier,
    },
    // === Keywords: Control flow ===
    KeywordSpec {
        text: "apud",
        token_kind: Some(TokenKind::Apud),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ControlFlow,
    },
    KeywordSpec {
        text: "casu",
        token_kind: Some(TokenKind::Casu),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ControlFlow,
    },
    KeywordSpec {
        text: "ceterum",
        token_kind: Some(TokenKind::Ceterum),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ControlFlow,
    },
    KeywordSpec {
        text: "custodi",
        token_kind: Some(TokenKind::Custodi),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ControlFlow,
    },
    KeywordSpec {
        text: "discerne",
        token_kind: Some(TokenKind::Discerne),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ControlFlow,
    },
    KeywordSpec {
        text: "dum",
        token_kind: Some(TokenKind::Dum),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ControlFlow,
    },
    KeywordSpec {
        text: "elige",
        token_kind: Some(TokenKind::Elige),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ControlFlow,
    },
    KeywordSpec {
        text: "ergo",
        token_kind: Some(TokenKind::Ergo),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        // Compact statement-body joint only; clausura uses glyph TokenKind::Therefore.
        category: KeywordKind::ControlFlow,
    },
    KeywordSpec {
        text: "fac",
        token_kind: Some(TokenKind::Fac),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ControlFlow,
    },
    KeywordSpec {
        text: "itera",
        token_kind: Some(TokenKind::Itera),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ControlFlow,
    },
    KeywordSpec {
        text: "secus",
        token_kind: Some(TokenKind::Secus),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ControlFlow,
    },
    KeywordSpec {
        text: "si",
        token_kind: Some(TokenKind::Si),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ControlFlow,
    },
    KeywordSpec {
        text: "sin",
        token_kind: Some(TokenKind::Sin),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ControlFlow,
    },
    // === Keywords: Transfer ===
    KeywordSpec {
        text: "perge",
        token_kind: Some(TokenKind::Perge),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::Transfer,
    },
    KeywordSpec {
        text: "redde",
        token_kind: Some(TokenKind::Redde),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::Transfer,
    },
    KeywordSpec {
        text: "rumpe",
        token_kind: Some(TokenKind::Rumpe),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::Transfer,
    },
    KeywordSpec {
        text: "tacet",
        token_kind: Some(TokenKind::Tacet),
        scope: KeywordScope::Contextual(STATEMENT_INTRODUCER),
        category: KeywordKind::Transfer,
    },
    // === Keywords: Error handling ===
    KeywordSpec {
        text: "adfirma",
        token_kind: Some(TokenKind::Adfirma),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ErrorHandling,
    },
    KeywordSpec {
        text: "cape",
        token_kind: Some(TokenKind::Cape),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ErrorHandling,
    },
    // `trap` is the capture boundary: an expression keyword that reifies the
    // error channel into a value union. Claimed only in expression-primary
    // position, so the spelling stays available as an ordinary identifier
    // everywhere else.
    KeywordSpec {
        text: "capta",
        token_kind: Some(TokenKind::Trap),
        scope: KeywordScope::Contextual(EXPRESSION_INTRODUCER),
        category: KeywordKind::ErrorHandling,
    },
    KeywordSpec {
        text: "iace",
        token_kind: Some(TokenKind::Iace),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ErrorHandling,
    },
    KeywordSpec {
        text: "mori",
        token_kind: Some(TokenKind::Mori),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ErrorHandling,
    },
    KeywordSpec {
        text: "requirit",
        token_kind: Some(TokenKind::Requirit),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ErrorHandling,
    },
    KeywordSpec {
        text: "reice",
        token_kind: Some(TokenKind::Reice),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::ErrorHandling,
    },
    // === Keywords: Async ===
    KeywordSpec {
        text: "cede",
        token_kind: Some(TokenKind::Cede),
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordKind::Async,
    },
    KeywordSpec {
        text: "clausura",
        token_kind: Some(TokenKind::Clausura),
        scope: KeywordScope::Contextual(EXPRESSION_INTRODUCER),
        category: KeywordKind::Async,
    },
    // === Contextual callable-posture spellings ===
    // WHY: morphology-driven async/stream posture; claimed in signature slots,
    // not globally reserved.
    KeywordSpec {
        text: "fiet",
        token_kind: Some(TokenKind::Fiet),
        scope: KeywordScope::Contextual(CALLABLE_POSTURE),
        category: KeywordKind::Async,
    },
    KeywordSpec {
        text: "fiunt",
        token_kind: Some(TokenKind::Fiunt),
        scope: KeywordScope::Contextual(CALLABLE_POSTURE),
        category: KeywordKind::Async,
    },
    KeywordSpec {
        text: "fient",
        token_kind: Some(TokenKind::Fient),
        scope: KeywordScope::Contextual(CALLABLE_POSTURE),
        category: KeywordKind::Async,
    },
    // === Contextual control-operation spellings ===
    KeywordSpec {
        text: "figendum",
        token_kind: Some(TokenKind::Figendum),
        scope: KeywordScope::Contextual(CONTROL_OPERATION),
        category: KeywordKind::Async,
    },
    KeywordSpec {
        text: "variandum",
        token_kind: Some(TokenKind::Variandum),
        scope: KeywordScope::Contextual(CONTROL_OPERATION),
        category: KeywordKind::Async,
    },
    KeywordSpec {
        text: "reddet",
        token_kind: Some(TokenKind::Reddet),
        scope: KeywordScope::Contextual(CONTROL_OPERATION),
        category: KeywordKind::Async,
    },
    KeywordSpec {
        text: "tacebit",
        token_kind: Some(TokenKind::Tacebit),
        scope: KeywordScope::Contextual(CONTROL_OPERATION),
        category: KeywordKind::Async,
    },
    // === Keywords: Boolean/null ===
    KeywordSpec {
        text: "falsum",
        token_kind: Some(TokenKind::Falsum),
        scope: KeywordScope::Contextual(LITERAL),
        category: KeywordKind::Literal,
    },
    // Null value literal. DECISIONS D23.2 splits the null *value* from the
    // null *type*: the value literal is `nulla` (en `null`), the type is
    // `nihil` (en `none`). `nihil` as a value is a named parse error.
    KeywordSpec {
        text: "nulla",
        token_kind: Some(TokenKind::Nihil),
        scope: KeywordScope::Contextual(LITERAL),
        category: KeywordKind::Literal,
    },
    KeywordSpec {
        text: "verum",
        token_kind: Some(TokenKind::Verum),
        scope: KeywordScope::Contextual(LITERAL),
        category: KeywordKind::Literal,
    },
    // `nonnumerus` is tokenless so the parser can claim it only in a literal
    // slot; calls and declaration/member names remain ordinary identifiers.
    KeywordSpec {
        text: "nonnumerus",
        token_kind: None,
        scope: KeywordScope::Contextual(LITERAL),
        category: KeywordKind::Literal,
    },
    // Empty-collection expression marker. Tokenless (lexes as an identifier):
    // the parser claims `vacua` by spelling in its expression slot, so the
    // keyword registry only needs the emit surface for packs. Tokenless rows
    // are inactive, so the strict EBNF vocabulary audit does not require them.
    KeywordSpec {
        text: "vacua",
        token_kind: None,
        scope: KeywordScope::Contextual(EXPRESSION_INTRODUCER),
        category: KeywordKind::Literal,
    },
    // === Keywords: Reduction operators ===
    // `summa` is an operator only after `reducta via`; `filum` names the
    // optional sum carrier. Both remain tokenless and pack-driven.
    KeywordSpec {
        text: "summa",
        token_kind: None,
        scope: KeywordScope::Contextual(REDUCTION_OPERATOR),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "filum",
        token_kind: None,
        scope: KeywordScope::Contextual(EXPRESSION_INTRODUCER),
        category: KeywordKind::Misc,
    },
    // `maxima`/`minima` are reduction operators only after `reducta via`.
    KeywordSpec {
        text: "maxima",
        token_kind: None,
        scope: KeywordScope::Contextual(REDUCTION_OPERATOR),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "minima",
        token_kind: None,
        scope: KeywordScope::Contextual(REDUCTION_OPERATOR),
        category: KeywordKind::Misc,
    },
    // === Keywords: Reduction expression (`reducta via Op ex source`) ===
    // `reducta` heads the expression, `pro` names the optional grouping key
    // after the source and `apud` clause, and the remaining rows are op words
    // claimed after `via`. All are tokenless: each lexes as an ordinary
    // identifier and is claimed by Latin key only in its grammar slot. The op
    // words `summa`, `maxima`, `minima`, `omnia` are the existing rows above.
    KeywordSpec {
        text: "reducta",
        token_kind: None,
        scope: KeywordScope::Contextual(REDUCTION_HEAD),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "pro",
        token_kind: None,
        scope: KeywordScope::Contextual(REDUCTION_SCAN_GROUP),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "factum",
        token_kind: None,
        scope: KeywordScope::Contextual(REDUCTION_OPERATOR),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "numeratio",
        token_kind: None,
        scope: KeywordScope::Contextual(REDUCTION_OPERATOR),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "quilibet",
        token_kind: None,
        scope: KeywordScope::Contextual(REDUCTION_OPERATOR),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "argmaxima",
        token_kind: None,
        scope: KeywordScope::Contextual(REDUCTION_OPERATOR),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "argminima",
        token_kind: None,
        scope: KeywordScope::Contextual(REDUCTION_OPERATOR),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "prima",
        token_kind: None,
        scope: KeywordScope::Contextual(REDUCTION_OPERATOR),
        category: KeywordKind::Misc,
    },
    // === Keywords: Sort expression (`ordinata via Dir ex source`) ===
    // `ordinata` heads the expression and `ascendens` / `descendens` name the
    // direction after `via`. All three are tokenless: each lexes as an
    // ordinary identifier and is claimed by Latin key only in its grammar slot.
    KeywordSpec {
        text: "ordinata",
        token_kind: None,
        scope: KeywordScope::Contextual(SORT_HEAD),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "ascendens",
        token_kind: None,
        scope: KeywordScope::Contextual(SORT_DIRECTION),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "descendens",
        token_kind: None,
        scope: KeywordScope::Contextual(SORT_DIRECTION),
        category: KeywordKind::Misc,
    },
    // === Keywords: Ordered prefix scan ===
    // The scan mode words are shared as source spellings by all reader packs;
    // they remain contextual in the scan grammar slots.
    KeywordSpec {
        text: "scan",
        token_kind: None,
        scope: KeywordScope::Contextual(SCAN_HEAD),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "scan_total",
        token_kind: None,
        scope: KeywordScope::Contextual(SCAN_TOTAL),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "scan_inclusive",
        token_kind: None,
        scope: KeywordScope::Contextual(SCAN_TOTAL_MODE),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "scan_exclusive",
        token_kind: None,
        scope: KeywordScope::Contextual(SCAN_TOTAL_MODE),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "scan_order",
        token_kind: None,
        scope: KeywordScope::Contextual(SCAN_ORDER),
        category: KeywordKind::Misc,
    },
    // === Keywords: Logical operators ===
    KeywordSpec {
        text: "aut",
        token_kind: Some(TokenKind::Aut),
        scope: KeywordScope::Contextual(OPERATOR),
        category: KeywordKind::Operator,
    },
    KeywordSpec {
        text: "est",
        token_kind: Some(TokenKind::Est),
        scope: KeywordScope::Contextual(OPERATOR),
        category: KeywordKind::Operator,
    },
    KeywordSpec {
        text: "et",
        token_kind: Some(TokenKind::Et),
        scope: KeywordScope::Contextual(OPERATOR),
        category: KeywordKind::Operator,
    },
    KeywordSpec {
        text: "non",
        token_kind: Some(TokenKind::Non),
        scope: KeywordScope::Contextual(OPERATOR),
        category: KeywordKind::Operator,
    },
    KeywordSpec {
        text: "vel",
        token_kind: Some(TokenKind::Vel),
        scope: KeywordScope::Contextual(OPERATOR),
        category: KeywordKind::Operator,
    },
    // === Keywords: Objects ===
    KeywordSpec {
        text: "ego",
        token_kind: Some(TokenKind::Ego),
        scope: KeywordScope::Contextual(EXPRESSION_INTRODUCER),
        category: KeywordKind::Object,
    },
    KeywordSpec {
        text: "finge",
        token_kind: Some(TokenKind::Finge),
        scope: KeywordScope::Contextual(EXPRESSION_INTRODUCER),
        category: KeywordKind::Object,
    },
    KeywordSpec {
        text: "implet",
        token_kind: Some(TokenKind::Implet),
        scope: KeywordScope::Contextual(GENUS_HEADER),
        category: KeywordKind::Object,
    },
    // === Keywords: Output ===
    KeywordSpec {
        text: "mone",
        token_kind: Some(TokenKind::Mone),
        scope: KeywordScope::Contextual(SCRIBE_STATEMENT),
        category: KeywordKind::Output,
    },
    KeywordSpec {
        text: "nota",
        token_kind: Some(TokenKind::Nota),
        scope: KeywordScope::Contextual(SCRIBE_STATEMENT),
        category: KeywordKind::Output,
    },
    KeywordSpec {
        text: "scribe",
        token_kind: Some(TokenKind::Scribe),
        scope: KeywordScope::Alias { canonical: "nota" },
        category: KeywordKind::Output,
    },
    KeywordSpec {
        text: "vide",
        token_kind: Some(TokenKind::Vide),
        scope: KeywordScope::Contextual(SCRIBE_STATEMENT),
        category: KeywordKind::Output,
    },
    // === Keywords: Entry points ===
    KeywordSpec {
        text: "argumenta",
        token_kind: Some(TokenKind::Argumenta),
        scope: KeywordScope::Contextual(ENTRY_FUNCTION_OR_PARAM_OR_MEMBER),
        category: KeywordKind::EntryPoint,
    },
    KeywordSpec {
        text: "incipiet",
        token_kind: Some(TokenKind::Incipiet),
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::EntryPoint,
    },
    KeywordSpec {
        text: "incipit",
        token_kind: Some(TokenKind::Incipit),
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::EntryPoint,
    },
    // === Keywords: Endpoint ===
    KeywordSpec {
        text: "ad",
        token_kind: Some(TokenKind::Ad),
        scope: KeywordScope::Contextual(ROUTE_EXPRESSION_OR_ANNOTATION),
        category: KeywordKind::Endpoint,
    },
    // === Keywords: Misc ===
    KeywordSpec {
        text: "ab",
        token_kind: Some(TokenKind::Ab),
        scope: KeywordScope::Contextual(ITERATION_MODE),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "de",
        token_kind: Some(TokenKind::De),
        scope: KeywordScope::Contextual(PARAMETER_MODE),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "ex",
        token_kind: Some(TokenKind::Ex),
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "in",
        token_kind: Some(TokenKind::In),
        scope: KeywordScope::Contextual(MUT_CELL),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "own",
        token_kind: Some(TokenKind::Own),
        scope: KeywordScope::Contextual(PARAMETER_MODE),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "copy",
        token_kind: Some(TokenKind::Copy),
        scope: KeywordScope::Contextual(PARAMETER_MODE),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "axis",
        token_kind: Some(TokenKind::Axis),
        scope: KeywordScope::Contextual(PARAMETER_MODE),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "lege",
        token_kind: Some(TokenKind::Lege),
        scope: KeywordScope::Contextual(BUILTIN_CALL),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "lineam",
        token_kind: Some(TokenKind::Lineam),
        scope: KeywordScope::Contextual(BUILTIN_CALL),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "omnia",
        token_kind: Some(TokenKind::Omnia),
        scope: KeywordScope::Contextual(DECLARATION_MARKER_OR_REDUCTION_OPERATOR),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "machina",
        token_kind: Some(TokenKind::Machina),
        scope: KeywordScope::Contextual(FAC_MODIFIER),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "praefixum",
        token_kind: Some(TokenKind::Praefixum),
        scope: KeywordScope::Contextual(EXPRESSION_INTRODUCER),
        category: KeywordKind::Misc,
    },
    // `insere` (en `embed`, D8.10) is claimed in expression-primary position
    // only when directly followed by a string literal; everywhere else the
    // spelling stays an ordinary identifier (F8 contextual keyword). Where an
    // embed is legal (a compile-time initializer) is lowering's decision.
    KeywordSpec {
        text: "insere",
        token_kind: Some(TokenKind::Insere),
        scope: KeywordScope::Contextual(EXPRESSION_INTRODUCER),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "scriptum",
        token_kind: Some(TokenKind::Scriptum),
        scope: KeywordScope::Contextual(BUILTIN_CALL),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "sparge",
        token_kind: Some(TokenKind::Sparge),
        scope: KeywordScope::Contextual(SPREAD_EXPRESSION),
        category: KeywordKind::Misc,
    },
    KeywordSpec {
        text: "ut",
        token_kind: Some(TokenKind::Ut),
        scope: KeywordScope::Contextual(ALIAS_MARKER),
        category: KeywordKind::Misc,
    },
    // `via` (D11.9) opens the convert-hint clause right after a `↦` target
    // type on the same line (`x ↦ u32 via Hex`). Everywhere else the spelling
    // stays an ordinary identifier; it is never reserved.
    KeywordSpec {
        text: "via",
        token_kind: Some(TokenKind::Via),
        scope: KeywordScope::Contextual(CONVERSION_HINT_REDUCTION_SCAN_HEAD),
        category: KeywordKind::Misc,
    },
    // === Keywords: Ranges ===
    KeywordSpec {
        text: "ante",
        token_kind: Some(TokenKind::Ante),
        scope: KeywordScope::Contextual(RANGE_OPERATOR),
        category: KeywordKind::Range,
    },
    // `inter` is a retired membership operator (`∈` replaced it). Tokenless: the
    // parser claims it only in operator position, to emit the migration
    // diagnostic; everywhere else the spelling is an ordinary identifier.
    KeywordSpec {
        text: "inter",
        token_kind: None,
        scope: KeywordScope::Contextual(RETIRED_MEMBERSHIP),
        category: KeywordKind::Range,
    },
    // `intra` is a retired membership operator (`∈` replaced it). Tokenless: the
    // parser claims it only in operator position, to emit the migration
    // diagnostic; everywhere else the spelling is an ordinary identifier.
    KeywordSpec {
        text: "intra",
        token_kind: None,
        scope: KeywordScope::Contextual(RETIRED_MEMBERSHIP),
        category: KeywordKind::Range,
    },
    KeywordSpec {
        text: "per",
        token_kind: Some(TokenKind::Per),
        scope: KeywordScope::Contextual(RANGE_OPERATOR),
        category: KeywordKind::Range,
    },
    KeywordSpec {
        text: "usque",
        token_kind: Some(TokenKind::Usque),
        scope: KeywordScope::Contextual(RANGE_OPERATOR),
        category: KeywordKind::Range,
    },
    // === Keywords: Testing ===
    KeywordSpec {
        text: "erratur",
        token_kind: Some(TokenKind::Erratur),
        scope: KeywordScope::TestOwned,
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "fragilis",
        token_kind: Some(TokenKind::Fragilis),
        scope: KeywordScope::TestOwned,
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "futurum",
        token_kind: Some(TokenKind::Futurum),
        scope: KeywordScope::TestOwned,
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "metior",
        token_kind: Some(TokenKind::Metior),
        scope: KeywordScope::Contextual(TEST_ANNOTATION_NAME),
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "omitte",
        token_kind: Some(TokenKind::Omitte),
        scope: KeywordScope::Contextual(TEST_ANNOTATION_NAME),
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "postpara",
        token_kind: Some(TokenKind::Postpara),
        scope: KeywordScope::TestOwned,
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "postparabit",
        token_kind: Some(TokenKind::Postparabit),
        scope: KeywordScope::TestOwned,
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "praepara",
        token_kind: Some(TokenKind::Praepara),
        scope: KeywordScope::TestOwned,
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "praeparabit",
        token_kind: Some(TokenKind::Praeparabit),
        scope: KeywordScope::TestOwned,
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "proba",
        token_kind: Some(TokenKind::Proba),
        scope: KeywordScope::TestOwned,
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "probandum",
        token_kind: Some(TokenKind::Probandum),
        scope: KeywordScope::TestOwned,
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "repete",
        token_kind: Some(TokenKind::Repete),
        scope: KeywordScope::TestOwned,
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "solum",
        token_kind: Some(TokenKind::Solum),
        scope: KeywordScope::Contextual(TEST_ANNOTATION_NAME),
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "solum_in",
        token_kind: Some(TokenKind::SolumIn),
        scope: KeywordScope::TestOwned,
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "tag",
        token_kind: Some(TokenKind::Tag),
        scope: KeywordScope::Contextual(TEST_ANNOTATION_NAME),
        category: KeywordKind::Testing,
    },
    KeywordSpec {
        text: "temporis",
        token_kind: Some(TokenKind::Temporis),
        scope: KeywordScope::TestOwned,
        category: KeywordKind::Testing,
    },
    // The prefix-predicate rows (`nulla` with D23.4; `positivum`, `negativum`,
    // `nonnihil`, `nonnulla` with KW-1) are gone: `nulla` is the null value
    // literal (the row in the Boolean/null group above).
    // === Keywords: Annotation ===
    KeywordSpec {
        text: "brevis",
        token_kind: Some(TokenKind::Brevis),
        scope: KeywordScope::Contextual(ANNOTATION_MODIFIER),
        category: KeywordKind::Annotation,
    },
    // Annotation-contract family word (`@ annotatio { … }` marks a genus as a
    // compile-time annotation contract). Tokenless like `commune`: the parser
    // claims the pack spelling as a raw annotation name and normalizes it to
    // the canonical `annotatio` family, and annotation-owned status seals the
    // Latin spelling out of translated readers (`wrong_locale_annotation`).
    KeywordSpec {
        text: "annotatio",
        token_kind: None,
        scope: KeywordScope::Contextual(ANNOTATION_NAME),
        category: KeywordKind::Annotation,
    },
    // CLI metadata families (`@ versio "1.0"`, `@ imperia mount`). Tokenless
    // like `commune`: the pack spelling is claimed as a raw annotation name
    // and normalized to the canonical family; annotation ownership seals the
    // Latin spelling out of translated readers.
    KeywordSpec {
        text: "versio",
        token_kind: None,
        scope: KeywordScope::Contextual(ANNOTATION_NAME),
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "imperia",
        token_kind: None,
        scope: KeywordScope::Contextual(ANNOTATION_NAME),
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "cli",
        token_kind: Some(TokenKind::Cli),
        scope: KeywordScope::Annotation,
        category: KeywordKind::Annotation,
    },
    // Shared-field union annotation. Tokenless (lexes as an identifier): the
    // parser claims both locale spellings as raw annotation names and
    // normalizes them to the `commune` family; the registry row exists so
    // reader packs can carry the localized emit surface (en: `shared`)
    // and re-emission projects it back through `contextual_keyword`.
    KeywordSpec {
        text: "commune",
        token_kind: None,
        scope: KeywordScope::Contextual(ANNOTATION_NAME),
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "cursor",
        token_kind: Some(TokenKind::Cursor),
        scope: KeywordScope::Annotation,
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "descriptio",
        token_kind: Some(TokenKind::Descriptio),
        scope: KeywordScope::Contextual(ANNOTATION_MODIFIER),
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "futura",
        token_kind: Some(TokenKind::Futura),
        scope: KeywordScope::Annotation,
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "imperium",
        token_kind: Some(TokenKind::Imperium),
        scope: KeywordScope::Annotation,
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "longum",
        token_kind: Some(TokenKind::Longum),
        scope: KeywordScope::Contextual(ANNOTATION_MODIFIER),
        category: KeywordKind::Annotation,
    },
    // The `@ nucleum` tier words. Claimed only after `@ nucleum`, so `relaxed`
    // and `unsafe` stay ordinary identifiers everywhere else.
    KeywordSpec {
        text: "laxus",
        token_kind: Some(TokenKind::Laxus),
        scope: KeywordScope::Contextual(ANNOTATION_MODIFIER),
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "intutus",
        token_kind: Some(TokenKind::Intutus),
        scope: KeywordScope::Contextual(ANNOTATION_MODIFIER),
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "lane",
        token_kind: Some(TokenKind::Lane),
        scope: KeywordScope::Contextual(ANNOTATION_MODIFIER),
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "operandus",
        token_kind: Some(TokenKind::Operandus),
        scope: KeywordScope::Annotation,
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "optio",
        token_kind: Some(TokenKind::Optio),
        scope: KeywordScope::Annotation,
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "ubique",
        token_kind: Some(TokenKind::Ubique),
        scope: KeywordScope::Contextual(ANNOTATION_MODIFIER),
        category: KeywordKind::Annotation,
    },
    // Compiler-owned annotation families that previously lived only as bare
    // identifiers after `@`. Registered so packs can re-spell them (en: kernel,
    // …) while HIR keeps Latin family ids.
    KeywordSpec {
        text: "conversion",
        token_kind: Some(TokenKind::Conversio),
        scope: KeywordScope::Alias {
            canonical: "conversio",
        },
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "conversio",
        token_kind: Some(TokenKind::Conversio),
        scope: KeywordScope::Annotation,
        category: KeywordKind::Annotation,
    },
    // `nucleum` is the kernel annotation identity.
    KeywordSpec {
        text: "nucleum",
        token_kind: Some(TokenKind::Nucleum),
        scope: KeywordScope::Annotation,
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "vertex",
        token_kind: Some(TokenKind::Vertex),
        scope: KeywordScope::Annotation,
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "fragment",
        token_kind: Some(TokenKind::Fragment),
        scope: KeywordScope::Annotation,
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "nondum",
        token_kind: Some(TokenKind::Nondum),
        scope: KeywordScope::Annotation,
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "verte",
        token_kind: Some(TokenKind::Verte),
        scope: KeywordScope::Annotation,
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "nomen",
        token_kind: Some(TokenKind::Nomen),
        scope: KeywordScope::Contextual(ANNOTATION_MODIFIER),
        category: KeywordKind::Annotation,
    },
    KeywordSpec {
        text: "radix",
        token_kind: Some(TokenKind::Radix),
        scope: KeywordScope::Annotation,
        category: KeywordKind::Annotation,
    },
];

/// Return the keyword spelling registry in deterministic declaration order.
#[must_use]
pub fn keyword_specs() -> &'static [KeywordSpec] {
    KEYWORD_SPECS
}

/// Return keyword group declarations in deterministic order.
#[must_use]
pub fn keyword_group_specs() -> &'static [KeywordGroupSpec] {
    KEYWORD_GROUP_SPECS
}

/// O(1) text → keyword-spec index built once from [`KEYWORD_SPECS`].
fn keyword_by_text() -> &'static FxHashMap<&'static str, &'static KeywordSpec> {
    static MAP: OnceLock<FxHashMap<&'static str, &'static KeywordSpec>> = OnceLock::new();
    MAP.get_or_init(|| {
        let mut map =
            FxHashMap::with_capacity_and_hasher(KEYWORD_SPECS.len(), handroll::FxBuildHasher);
        for spec in KEYWORD_SPECS {
            map.insert(spec.text, spec);
        }
        map
    })
}

/// O(1) token-kind discriminant → keyword-spec index for live keyword rows.
///
/// Keyword tokens are unit variants, so discriminant equality matches the prior
/// `token_kind == kind` linear scan without requiring `TokenKind: Hash`.
///
/// EDGE: two spellings may share one identity (alias `conversion` with canonical
/// `conversio`); the later declaration wins, so a canonical row must follow its
/// alias in [`KEYWORD_SPECS`].
fn keyword_by_kind() -> &'static FxHashMap<mem::Discriminant<TokenKind>, &'static KeywordSpec> {
    static MAP: OnceLock<FxHashMap<mem::Discriminant<TokenKind>, &'static KeywordSpec>> =
        OnceLock::new();
    MAP.get_or_init(|| {
        let mut map =
            FxHashMap::with_capacity_and_hasher(KEYWORD_SPECS.len(), handroll::FxBuildHasher);
        for spec in KEYWORD_SPECS {
            if let Some(kind) = &spec.token_kind {
                map.insert(mem::discriminant(kind), spec);
            }
        }
        map
    })
}

/// Look up registry metadata for an exact source spelling.
#[must_use]
pub fn lookup_keyword_spec(text: &str) -> Option<&'static KeywordSpec> {
    keyword_by_text().get(text).copied()
}

/// Return the live token kind for an exact normal-mode keyword spelling.
///
/// Metadata-only rows stay out of the scanner even though tools may still
/// explain them through the registry.
#[must_use]
pub fn keyword_token_kind(text: &str) -> Option<TokenKind> {
    lookup_keyword_spec(text).and_then(|spec| spec.token_kind.clone())
}

/// Return the canonical spelling for a keyword token kind, if any.
#[must_use]
pub fn keyword_spelling_for_token(kind: &TokenKind) -> Option<&'static str> {
    lookup_keyword_spec_for_token(kind).map(|spec| spec.text)
}

/// Validate that static keyword groups reference live keyword rows.
#[must_use]
pub fn validate_keyword_groups() -> Vec<KeywordGroupValidationError> {
    let mut errors = Vec::new();

    for group in KEYWORD_GROUP_SPECS {
        if group.members.is_empty() {
            errors.push(KeywordGroupValidationError::MissingGroupMembers { group: group.id });
            continue;
        }

        for member in group.members {
            if lookup_keyword_spec(member).is_none() {
                let error = KeywordGroupValidationError::UnknownMember {
                    group: group.id,
                    key: member.to_string(),
                };
                errors.push(error);
            }
        }
    }

    errors
}

/// Return whether `owner` may reinterpret a keyword token as an identifier.
#[must_use]
pub fn keyword_allowed_as_ident(owner: KeywordOwner, kind: &TokenKind) -> Option<&'static str> {
    let spec = lookup_keyword_spec_for_token(kind)?;
    keyword_owner_allows_spec(owner, spec).then_some(spec.text)
}

fn lookup_keyword_spec_for_token(kind: &TokenKind) -> Option<&'static KeywordSpec> {
    keyword_by_kind().get(&mem::discriminant(kind)).copied()
}

fn keyword_owner_is_identifier_like(owner: KeywordOwner) -> bool {
    match owner {
        KeywordOwner::DeclarationName
        | KeywordOwner::ParameterName
        | KeywordOwner::MemberIdentifier
        | KeywordOwner::BindingTarget => true,
        KeywordOwner::EntryModifier
        | KeywordOwner::FunctionModifier
        | KeywordOwner::GenusHeader
        | KeywordOwner::GenusMember
        | KeywordOwner::RestPattern
        | KeywordOwner::SpreadExpression
        | KeywordOwner::AnnotationName
        | KeywordOwner::AnnotationModifier
        | KeywordOwner::ImportVisibility
        | KeywordOwner::ImportField
        | KeywordOwner::TypeParameter
        | KeywordOwner::ParameterMode
        | KeywordOwner::AliasMarker
        | KeywordOwner::IterationMode
        | KeywordOwner::CallablePosture
        | KeywordOwner::ControlOperation
        | KeywordOwner::TypeName
        | KeywordOwner::Constructor
        | KeywordOwner::ScribeStatement
        | KeywordOwner::BuiltinCall
        | KeywordOwner::Literal
        | KeywordOwner::Operator
        | KeywordOwner::DeclarationIntroducer
        | KeywordOwner::ControlFlow
        | KeywordOwner::ExpressionIntroducer
        | KeywordOwner::RangeOperator
        | KeywordOwner::RetiredMembership
        | KeywordOwner::StatementIntroducer
        | KeywordOwner::DeclarationMarker
        | KeywordOwner::FacModifier
        | KeywordOwner::RouteExpression
        | KeywordOwner::ConversionHint
        | KeywordOwner::ReductionHead
        | KeywordOwner::ReductionOperator
        | KeywordOwner::ReductionGroup
        | KeywordOwner::ScanGroup
        | KeywordOwner::SortHead
        | KeywordOwner::SortDirection
        | KeywordOwner::ScanHead
        | KeywordOwner::ScanTotal
        | KeywordOwner::ScanTotalMode
        | KeywordOwner::ScanOrder => false,
    }
}

fn keyword_owner_allows_spec(owner: KeywordOwner, spec: &KeywordSpec) -> bool {
    // Name slots accept every keyword spelling. TypeName stays out.
    if keyword_owner_is_identifier_like(owner) {
        return true;
    }
    match spec.scope {
        KeywordScope::Contextual(owners) => owners.contains(&owner),
        KeywordScope::TestOwned | KeywordScope::Alias { .. } => false,
        KeywordScope::Annotation => owner == KeywordOwner::AnnotationName,
    }
}

#[cfg(test)]
#[path = "keywords_test.rs"]
mod tests;
