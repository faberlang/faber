//! Keyword spelling registry for lexer and language-tool policy.
//!
//! The scanner delegates normal-mode keyword recognition to this registry. It
//! records which spellings are current keywords, which are annotation names,
//! which are intentionally contextual, and which spellings are aliases for
//! migration or teaching purposes.
//!
//! WHY THIS EXISTS
//! ===============
//! Faber's front end is still moving some words from globally reserved tokens
//! toward parser-owned or metadata-owned contexts. Keeping that intent in a
//! structured registry lets diagnostics and explain output work from one
//! taxonomy instead of rediscovering policy from a large scanner `match`.
//!
//! INVARIANTS
//! ==========
//! - `token_kind: Some(_)` means the current lexer can emit that token in at
//!   least one source mode; `None` means the spelling is tracked metadata and
//!   should lex as an identifier today.
//! - `Alias` entries name their canonical spelling but may still lex as their
//!   own token while migration diagnostics are developed.
//! - Annotation spellings are registry entries, not normal-mode reserved words.
//! - Keyword groups declare source-surface completeness constraints for future
//!   reader packs; missing one member of a group is a pack validation error.

use std::mem;
use std::sync::OnceLock;

use rustc_hash::FxHashMap;

use super::token::TokenKind;

/// Where a keyword spelling is valid or intended to be owned.
///
/// Scope is policy metadata, not automatic lexer behavior. It tells tools and
/// parser work whether a spelling is globally reserved, context-owned,
/// annotation-only, test-owned, or transitional.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordScope {
    /// Reserved anywhere normal-mode keyword recognition runs.
    ///
    /// Transitional: rows that stop emitting a token (`token_kind: None`) must
    /// migrate off `Global` — a tokenless row is not reserved, the parser
    /// claims it by spelling in its grammar slots. The registry test
    /// `tokenless_rows_must_not_be_global` enforces this.
    Global,

    /// Intended for one or more parser contexts, even if currently lexed as a keyword.
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

    /// Import visibility surface.
    ImportVisibility,

    /// Type-parameter modifier surface.
    TypeParameter,

    /// Parameter passing mode.
    ParameterMode,

    /// Import or binding alias marker.
    AliasMarker,

    /// Iteration mode keyword.
    IterationMode,

    /// Predicate-style operator.
    PredicateOperator,

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
    /// (`intra` / `inter` / `per` / `usque` / `ante`).
    RangeOperator,

    /// Statement-initial resource or transfer keyword (`cura` / `tacet`).
    StatementIntroducer,

    /// Marker inside a declaration or dispatch
    /// (`sub` / `implet` / `omnia` / `sponte`).
    DeclarationMarker,

    /// Route expression in expression or statement position (`ad`).
    RouteExpression,
}

/// Keyword grouping used by diagnostics, explain data, and registry consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeywordCategory {
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

    /// Object, self, inheritance, or construction syntax.
    Object,

    /// Type conversion or construction syntax.
    TypeOperation,

    /// User-visible output helper.
    Output,

    /// Program entrypoint surface.
    EntryPoint,

    /// Resource-management syntax.
    Resource,

    /// Endpoint binding syntax.
    Endpoint,

    /// Miscellaneous language surface not yet split into a narrower category.
    Misc,

    /// Range operator word.
    Range,

    /// Test declaration or test modifier syntax.
    Testing,

    /// Nullability or predicate helper.
    Nullability,

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

    /// Token emitted by the current lexer, or `None` for metadata-only entries.
    pub token_kind: Option<TokenKind>,

    /// Ownership and reservation policy for this spelling.
    pub scope: KeywordScope,

    /// Taxonomy bucket used by diagnostics and reference tooling.
    pub category: KeywordCategory,
}

impl KeywordSpec {
    /// Return whether the live lexer currently emits a keyword token here.
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
    /// tokenless (contextual) members like `si`/`sin`/`secus` stay
    /// referenceable after their `TokenKind` rows go `None`.
    pub members: &'static [&'static str],
}

/// Validation issue for the static keyword group declarations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeywordGroupValidationError {
    /// A group was declared without any members.
    EmptyGroup { group: &'static str },

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
const ENTRY_FUNCTION_OR_PARAM_OR_MEMBER: &[KeywordOwner] = &[
    KeywordOwner::EntryModifier,
    KeywordOwner::FunctionModifier,
    KeywordOwner::DeclarationName,
    KeywordOwner::ParameterName,
    KeywordOwner::MemberIdentifier,
];
const ANNOTATION_NAME: &[KeywordOwner] = &[KeywordOwner::AnnotationName];
const TYPE_PARAMETER: &[KeywordOwner] = &[KeywordOwner::TypeParameter];
const GENERIC_PARAMETER: &[KeywordOwner] = &[KeywordOwner::TypeParameter];
const REST_PATTERN: &[KeywordOwner] =
    &[KeywordOwner::RestPattern, KeywordOwner::AnnotationModifier];
const FUNCTION_MODIFIER: &[KeywordOwner] = &[KeywordOwner::FunctionModifier];
const ENTRY_OR_FUNCTION_MODIFIER: &[KeywordOwner] =
    &[KeywordOwner::EntryModifier, KeywordOwner::FunctionModifier];
const PARAMETER_MODE: &[KeywordOwner] = &[KeywordOwner::ParameterMode, KeywordOwner::IterationMode];
const ALIAS_MARKER: &[KeywordOwner] = &[KeywordOwner::AliasMarker];
const ITERATION_MODE: &[KeywordOwner] = &[KeywordOwner::IterationMode];
const SPREAD_EXPRESSION: &[KeywordOwner] = &[KeywordOwner::SpreadExpression];
const PREDICATE_OPERATOR: &[KeywordOwner] = &[KeywordOwner::PredicateOperator];
const PREDICATE_OR_MEMBER: &[KeywordOwner] = &[
    KeywordOwner::PredicateOperator,
    KeywordOwner::MemberIdentifier,
];
const CALLABLE_POSTURE: &[KeywordOwner] = &[KeywordOwner::CallablePosture];
const CONTROL_OPERATION: &[KeywordOwner] = &[KeywordOwner::ControlOperation];
// Batch A type-name slots: `modulus` claims the type-name position;
// `iuncta` claims both the type-name and the constructor-expression position.
const TYPE_NAME: &[KeywordOwner] = &[KeywordOwner::TypeName];
const TYPE_NAME_OR_CONSTRUCTOR: &[KeywordOwner] =
    &[KeywordOwner::TypeName, KeywordOwner::Constructor];
// Batch B builtin slots: `nota`/`mone`/`vide` are claimed in the statement-
// initial scribe slot; `lege`/`lineam`/`scriptum` in the expression-primary
// builtin-call slot.
const SCRIBE_STATEMENT: &[KeywordOwner] = &[KeywordOwner::ScribeStatement];
const BUILTIN_CALL: &[KeywordOwner] = &[KeywordOwner::BuiltinCall];
// Batch C literal slot: `verum`/`falsum`/`nihil` are claimed in expression and
// pattern positions as literal values.
const LITERAL: &[KeywordOwner] = &[KeywordOwner::Literal];
const OPERATOR: &[KeywordOwner] = &[KeywordOwner::Operator];
const DECLARATION_INTRODUCER: &[KeywordOwner] = &[KeywordOwner::DeclarationIntroducer];
const CONTROL_FLOW: &[KeywordOwner] = &[KeywordOwner::ControlFlow];
const EXPRESSION_INTRODUCER: &[KeywordOwner] = &[KeywordOwner::ExpressionIntroducer];
const RANGE_OPERATOR: &[KeywordOwner] = &[KeywordOwner::RangeOperator];
const STATEMENT_INTRODUCER: &[KeywordOwner] = &[KeywordOwner::StatementIntroducer];
const DECLARATION_MARKER: &[KeywordOwner] = &[KeywordOwner::DeclarationMarker];
const ROUTE_EXPRESSION: &[KeywordOwner] = &[KeywordOwner::RouteExpression];
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
const LOOP_GROUP: &[&str] = &["dum", "fac", "itera", "perge", "rumpe"];

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
];

/// Complete registry of current, contextual, alias, and annotation spellings.
pub static KEYWORD_SPECS: &[KeywordSpec] = &[
    // === Keywords: Declarations ===
    KeywordSpec {
        text: "discretio",
        token_kind: None,
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordCategory::Declaration,
    },
    KeywordSpec {
        text: "fixum",
        token_kind: None,
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordCategory::Declaration,
    },
    KeywordSpec {
        text: "functio",
        token_kind: None,
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordCategory::Declaration,
    },
    KeywordSpec {
        text: "genus",
        token_kind: None,
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordCategory::Declaration,
    },
    KeywordSpec {
        text: "iuncta",
        token_kind: None,
        scope: KeywordScope::Contextual(TYPE_NAME_OR_CONSTRUCTOR),
        category: KeywordCategory::Declaration,
    },
    KeywordSpec {
        text: "implendum",
        token_kind: None,
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordCategory::Declaration,
    },
    KeywordSpec {
        text: "importa",
        token_kind: Some(TokenKind::Importa),
        scope: KeywordScope::Global,
        category: KeywordCategory::Declaration,
    },
    KeywordSpec {
        text: "modulus",
        token_kind: None,
        scope: KeywordScope::Contextual(TYPE_NAME),
        category: KeywordCategory::TypeOperation,
    },
    KeywordSpec {
        text: "ordo",
        token_kind: None,
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordCategory::Declaration,
    },
    KeywordSpec {
        text: "sit",
        token_kind: None,
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordCategory::Declaration,
    },
    KeywordSpec {
        text: "typus",
        token_kind: None,
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordCategory::Declaration,
    },
    KeywordSpec {
        text: "varia",
        token_kind: None,
        scope: KeywordScope::Contextual(DECLARATION_INTRODUCER),
        category: KeywordCategory::Declaration,
    },
    // === Keywords: Modifiers ===
    KeywordSpec {
        text: "abstractus",
        token_kind: Some(TokenKind::Abstractus),
        scope: KeywordScope::Contextual(GENUS_HEADER),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "ceteri",
        token_kind: Some(TokenKind::Ceteri),
        scope: KeywordScope::Contextual(REST_PATTERN),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "curata",
        token_kind: Some(TokenKind::Curata),
        scope: KeywordScope::Contextual(FUNCTION_MODIFIER),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "errata",
        token_kind: Some(TokenKind::Errata),
        scope: KeywordScope::Contextual(FUNCTION_MODIFIER),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "exitus",
        token_kind: Some(TokenKind::Exitus),
        scope: KeywordScope::Contextual(ENTRY_OR_FUNCTION_MODIFIER),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "generis",
        token_kind: Some(TokenKind::Generis),
        scope: KeywordScope::Contextual(GENUS_MEMBER),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "iacit",
        token_kind: Some(TokenKind::Iacit),
        scope: KeywordScope::Contextual(FUNCTION_MODIFIER),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "immutata",
        token_kind: Some(TokenKind::Immutata),
        scope: KeywordScope::Contextual(FUNCTION_MODIFIER),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "magnitudo",
        token_kind: Some(TokenKind::Magnitudo),
        scope: KeywordScope::Contextual(GENERIC_PARAMETER),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "nexum",
        token_kind: Some(TokenKind::Nexum),
        scope: KeywordScope::Contextual(GENUS_MEMBER),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "optiones",
        token_kind: Some(TokenKind::Optiones),
        scope: KeywordScope::Contextual(FUNCTION_MODIFIER),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "prae",
        token_kind: Some(TokenKind::Prae),
        scope: KeywordScope::Contextual(TYPE_PARAMETER),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "privata",
        token_kind: Some(TokenKind::Privata),
        scope: KeywordScope::Contextual(IMPORT_VISIBILITY_ANNOTATION_OR_MEMBER),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "protecta",
        token_kind: Some(TokenKind::Protecta),
        scope: KeywordScope::Contextual(ANNOTATION_NAME),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "publica",
        token_kind: Some(TokenKind::Publica),
        scope: KeywordScope::Contextual(IMPORT_VISIBILITY_ANNOTATION_OR_MEMBER),
        category: KeywordCategory::Modifier,
    },
    KeywordSpec {
        text: "sponte",
        token_kind: None,
        scope: KeywordScope::Contextual(DECLARATION_MARKER),
        category: KeywordCategory::Modifier,
    },
    // === Keywords: Control flow ===
    KeywordSpec {
        text: "casu",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordCategory::ControlFlow,
    },
    KeywordSpec {
        text: "ceterum",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordCategory::ControlFlow,
    },
    KeywordSpec {
        text: "custodi",
        token_kind: Some(TokenKind::Custodi),
        scope: KeywordScope::Global,
        category: KeywordCategory::ControlFlow,
    },
    KeywordSpec {
        text: "discerne",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordCategory::ControlFlow,
    },
    KeywordSpec {
        text: "dum",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordCategory::ControlFlow,
    },
    KeywordSpec {
        text: "elige",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordCategory::ControlFlow,
    },
    KeywordSpec {
        text: "ergo",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        // Compact statement-body joint only; clausura uses glyph TokenKind::Therefore.
        category: KeywordCategory::ControlFlow,
    },
    KeywordSpec {
        text: "fac",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordCategory::ControlFlow,
    },
    KeywordSpec {
        text: "itera",
        token_kind: Some(TokenKind::Itera),
        scope: KeywordScope::Global,
        category: KeywordCategory::ControlFlow,
    },
    KeywordSpec {
        text: "secus",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordCategory::ControlFlow,
    },
    KeywordSpec {
        text: "si",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordCategory::ControlFlow,
    },
    KeywordSpec {
        text: "sic",
        token_kind: Some(TokenKind::Sic),
        scope: KeywordScope::Global,
        category: KeywordCategory::ControlFlow,
    },
    KeywordSpec {
        text: "sin",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordCategory::ControlFlow,
    },
    // === Keywords: Transfer ===
    KeywordSpec {
        text: "perge",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordCategory::Transfer,
    },
    KeywordSpec {
        text: "redde",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordCategory::Transfer,
    },
    KeywordSpec {
        text: "rumpe",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_FLOW),
        category: KeywordCategory::Transfer,
    },
    KeywordSpec {
        text: "tacet",
        token_kind: None,
        scope: KeywordScope::Contextual(STATEMENT_INTRODUCER),
        category: KeywordCategory::Transfer,
    },
    // === Keywords: Error handling ===
    KeywordSpec {
        text: "adfirma",
        token_kind: Some(TokenKind::Adfirma),
        scope: KeywordScope::Global,
        category: KeywordCategory::ErrorHandling,
    },
    KeywordSpec {
        text: "cape",
        token_kind: Some(TokenKind::Cape),
        scope: KeywordScope::Global,
        category: KeywordCategory::ErrorHandling,
    },
    KeywordSpec {
        text: "iace",
        token_kind: Some(TokenKind::Iace),
        scope: KeywordScope::Global,
        category: KeywordCategory::ErrorHandling,
    },
    KeywordSpec {
        text: "mori",
        token_kind: Some(TokenKind::Mori),
        scope: KeywordScope::Global,
        category: KeywordCategory::ErrorHandling,
    },
    // === Keywords: Async ===
    KeywordSpec {
        text: "cede",
        token_kind: Some(TokenKind::Cede),
        scope: KeywordScope::Global,
        category: KeywordCategory::Async,
    },
    KeywordSpec {
        text: "clausura",
        token_kind: None,
        scope: KeywordScope::Contextual(EXPRESSION_INTRODUCER),
        category: KeywordCategory::Async,
    },
    // === Contextual callable-posture spellings (token_kind: None) ===
    // WHY: morphology-driven async/stream posture. Recognized by spelling via
    // parser-owned grammar slots (eat_identifier_text); not globally reserved.
    KeywordSpec {
        text: "fiet",
        token_kind: None,
        scope: KeywordScope::Contextual(CALLABLE_POSTURE),
        category: KeywordCategory::Async,
    },
    KeywordSpec {
        text: "fiunt",
        token_kind: None,
        scope: KeywordScope::Contextual(CALLABLE_POSTURE),
        category: KeywordCategory::Async,
    },
    KeywordSpec {
        text: "fient",
        token_kind: None,
        scope: KeywordScope::Contextual(CALLABLE_POSTURE),
        category: KeywordCategory::Async,
    },
    // === Contextual control-operation spellings (token_kind: None) ===
    KeywordSpec {
        text: "figendum",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_OPERATION),
        category: KeywordCategory::Async,
    },
    KeywordSpec {
        text: "variandum",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_OPERATION),
        category: KeywordCategory::Async,
    },
    KeywordSpec {
        text: "reddet",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_OPERATION),
        category: KeywordCategory::Async,
    },
    KeywordSpec {
        text: "tacebit",
        token_kind: None,
        scope: KeywordScope::Contextual(CONTROL_OPERATION),
        category: KeywordCategory::Async,
    },
    // === Keywords: Boolean/null ===
    KeywordSpec {
        text: "falsum",
        token_kind: None,
        scope: KeywordScope::Contextual(LITERAL),
        category: KeywordCategory::Literal,
    },
    KeywordSpec {
        text: "nihil",
        token_kind: None,
        scope: KeywordScope::Contextual(LITERAL),
        category: KeywordCategory::Literal,
    },
    KeywordSpec {
        text: "verum",
        token_kind: None,
        scope: KeywordScope::Contextual(LITERAL),
        category: KeywordCategory::Literal,
    },
    // === Keywords: Logical operators ===
    KeywordSpec {
        text: "aut",
        token_kind: None,
        scope: KeywordScope::Contextual(OPERATOR),
        category: KeywordCategory::Operator,
    },
    KeywordSpec {
        text: "est",
        token_kind: None,
        scope: KeywordScope::Contextual(OPERATOR),
        category: KeywordCategory::Operator,
    },
    KeywordSpec {
        text: "et",
        token_kind: None,
        scope: KeywordScope::Contextual(OPERATOR),
        category: KeywordCategory::Operator,
    },
    KeywordSpec {
        text: "non",
        token_kind: None,
        scope: KeywordScope::Contextual(OPERATOR),
        category: KeywordCategory::Operator,
    },
    KeywordSpec {
        text: "vel",
        token_kind: None,
        scope: KeywordScope::Contextual(OPERATOR),
        category: KeywordCategory::Operator,
    },
    // === Keywords: Objects ===
    KeywordSpec {
        text: "ego",
        token_kind: None,
        scope: KeywordScope::Contextual(EXPRESSION_INTRODUCER),
        category: KeywordCategory::Object,
    },
    KeywordSpec {
        text: "finge",
        token_kind: None,
        scope: KeywordScope::Contextual(EXPRESSION_INTRODUCER),
        category: KeywordCategory::Object,
    },
    KeywordSpec {
        text: "implet",
        token_kind: None,
        scope: KeywordScope::Contextual(GENUS_HEADER),
        category: KeywordCategory::Object,
    },
    KeywordSpec {
        text: "sub",
        token_kind: None,
        scope: KeywordScope::Contextual(GENUS_HEADER),
        category: KeywordCategory::Object,
    },
    // === Keywords: Output ===
    KeywordSpec {
        text: "mone",
        token_kind: None,
        scope: KeywordScope::Contextual(SCRIBE_STATEMENT),
        category: KeywordCategory::Output,
    },
    KeywordSpec {
        text: "nota",
        token_kind: None,
        scope: KeywordScope::Contextual(SCRIBE_STATEMENT),
        category: KeywordCategory::Output,
    },
    KeywordSpec {
        text: "scribe",
        token_kind: Some(TokenKind::Scribe),
        scope: KeywordScope::Alias { canonical: "nota" },
        category: KeywordCategory::Output,
    },
    KeywordSpec {
        text: "vide",
        token_kind: None,
        scope: KeywordScope::Contextual(SCRIBE_STATEMENT),
        category: KeywordCategory::Output,
    },
    // === Keywords: Entry points ===
    KeywordSpec {
        text: "argumenta",
        token_kind: Some(TokenKind::Argumenta),
        scope: KeywordScope::Contextual(ENTRY_FUNCTION_OR_PARAM_OR_MEMBER),
        category: KeywordCategory::EntryPoint,
    },
    KeywordSpec {
        text: "incipiet",
        token_kind: Some(TokenKind::Incipiet),
        scope: KeywordScope::Global,
        category: KeywordCategory::EntryPoint,
    },
    KeywordSpec {
        text: "incipit",
        token_kind: Some(TokenKind::Incipit),
        scope: KeywordScope::Global,
        category: KeywordCategory::EntryPoint,
    },
    // === Keywords: Resource management ===
    KeywordSpec {
        text: "cura",
        token_kind: None,
        scope: KeywordScope::Contextual(STATEMENT_INTRODUCER),
        category: KeywordCategory::Resource,
    },
    // === Keywords: Endpoint ===
    KeywordSpec {
        text: "ad",
        token_kind: None,
        scope: KeywordScope::Contextual(ROUTE_EXPRESSION),
        category: KeywordCategory::Endpoint,
    },
    // === Keywords: Misc ===
    KeywordSpec {
        text: "ab",
        token_kind: None,
        scope: KeywordScope::Contextual(ITERATION_MODE),
        category: KeywordCategory::Misc,
    },
    KeywordSpec {
        text: "de",
        token_kind: Some(TokenKind::De),
        scope: KeywordScope::Contextual(PARAMETER_MODE),
        category: KeywordCategory::Misc,
    },
    KeywordSpec {
        text: "ex",
        token_kind: Some(TokenKind::Ex),
        scope: KeywordScope::Global,
        category: KeywordCategory::Misc,
    },
    KeywordSpec {
        text: "in",
        token_kind: Some(TokenKind::In),
        scope: KeywordScope::Contextual(PARAMETER_MODE),
        category: KeywordCategory::Misc,
    },
    KeywordSpec {
        text: "lege",
        token_kind: None,
        scope: KeywordScope::Contextual(BUILTIN_CALL),
        category: KeywordCategory::Misc,
    },
    KeywordSpec {
        text: "lineam",
        token_kind: None,
        scope: KeywordScope::Contextual(BUILTIN_CALL),
        category: KeywordCategory::Misc,
    },
    KeywordSpec {
        text: "omnia",
        token_kind: None,
        scope: KeywordScope::Contextual(DECLARATION_MARKER),
        category: KeywordCategory::Misc,
    },
    KeywordSpec {
        text: "praefixum",
        token_kind: None,
        scope: KeywordScope::Contextual(EXPRESSION_INTRODUCER),
        category: KeywordCategory::Misc,
    },
    KeywordSpec {
        text: "scriptum",
        token_kind: None,
        scope: KeywordScope::Contextual(BUILTIN_CALL),
        category: KeywordCategory::Misc,
    },
    KeywordSpec {
        text: "sparge",
        token_kind: Some(TokenKind::Sparge),
        scope: KeywordScope::Contextual(SPREAD_EXPRESSION),
        category: KeywordCategory::Misc,
    },
    KeywordSpec {
        text: "ut",
        token_kind: Some(TokenKind::Ut),
        scope: KeywordScope::Contextual(ALIAS_MARKER),
        category: KeywordCategory::Misc,
    },
    // === Keywords: Ranges ===
    KeywordSpec {
        text: "ante",
        token_kind: None,
        scope: KeywordScope::Contextual(RANGE_OPERATOR),
        category: KeywordCategory::Range,
    },
    KeywordSpec {
        text: "inter",
        token_kind: None,
        scope: KeywordScope::Contextual(RANGE_OPERATOR),
        category: KeywordCategory::Range,
    },
    KeywordSpec {
        text: "intra",
        token_kind: None,
        scope: KeywordScope::Contextual(RANGE_OPERATOR),
        category: KeywordCategory::Range,
    },
    KeywordSpec {
        text: "per",
        token_kind: None,
        scope: KeywordScope::Contextual(RANGE_OPERATOR),
        category: KeywordCategory::Range,
    },
    KeywordSpec {
        text: "usque",
        token_kind: None,
        scope: KeywordScope::Contextual(RANGE_OPERATOR),
        category: KeywordCategory::Range,
    },
    // === Keywords: Testing ===
    KeywordSpec {
        text: "fragilis",
        token_kind: Some(TokenKind::Fragilis),
        scope: KeywordScope::TestOwned,
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "futurum",
        token_kind: Some(TokenKind::Futurum),
        scope: KeywordScope::TestOwned,
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "metior",
        token_kind: Some(TokenKind::Metior),
        scope: KeywordScope::Contextual(TEST_ANNOTATION_NAME),
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "omitte",
        token_kind: Some(TokenKind::Omitte),
        scope: KeywordScope::Contextual(TEST_ANNOTATION_NAME),
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "postpara",
        token_kind: Some(TokenKind::Postpara),
        scope: KeywordScope::TestOwned,
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "postparabit",
        token_kind: Some(TokenKind::Postparabit),
        scope: KeywordScope::TestOwned,
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "praepara",
        token_kind: Some(TokenKind::Praepara),
        scope: KeywordScope::TestOwned,
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "praeparabit",
        token_kind: Some(TokenKind::Praeparabit),
        scope: KeywordScope::TestOwned,
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "proba",
        token_kind: Some(TokenKind::Proba),
        scope: KeywordScope::TestOwned,
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "probandum",
        token_kind: Some(TokenKind::Probandum),
        scope: KeywordScope::TestOwned,
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "repete",
        token_kind: Some(TokenKind::Repete),
        scope: KeywordScope::TestOwned,
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "requirit",
        token_kind: Some(TokenKind::Requirit),
        scope: KeywordScope::TestOwned,
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "solum",
        token_kind: Some(TokenKind::Solum),
        scope: KeywordScope::Contextual(TEST_ANNOTATION_NAME),
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "solum_in",
        token_kind: Some(TokenKind::SolumIn),
        scope: KeywordScope::TestOwned,
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "tag",
        token_kind: Some(TokenKind::Tag),
        scope: KeywordScope::Contextual(TEST_ANNOTATION_NAME),
        category: KeywordCategory::Testing,
    },
    KeywordSpec {
        text: "temporis",
        token_kind: Some(TokenKind::Temporis),
        scope: KeywordScope::TestOwned,
        category: KeywordCategory::Testing,
    },
    // === Keywords: Nullability ===
    KeywordSpec {
        text: "negativum",
        token_kind: Some(TokenKind::Negativum),
        scope: KeywordScope::Contextual(PREDICATE_OPERATOR),
        category: KeywordCategory::Nullability,
    },
    KeywordSpec {
        text: "nonnihil",
        token_kind: Some(TokenKind::Nonnihil),
        scope: KeywordScope::Contextual(PREDICATE_OR_MEMBER),
        category: KeywordCategory::Nullability,
    },
    KeywordSpec {
        text: "nonnulla",
        token_kind: Some(TokenKind::Nonnulla),
        scope: KeywordScope::Contextual(PREDICATE_OPERATOR),
        category: KeywordCategory::Nullability,
    },
    KeywordSpec {
        text: "nulla",
        token_kind: Some(TokenKind::Nulla),
        scope: KeywordScope::Contextual(PREDICATE_OPERATOR),
        category: KeywordCategory::Nullability,
    },
    KeywordSpec {
        text: "positivum",
        token_kind: Some(TokenKind::Positivum),
        scope: KeywordScope::Contextual(PREDICATE_OPERATOR),
        category: KeywordCategory::Nullability,
    },
    // === Keywords: Annotation ===
    KeywordSpec {
        text: "brevis",
        token_kind: None,
        scope: KeywordScope::Contextual(ANNOTATION_MODIFIER),
        category: KeywordCategory::Annotation,
    },
    KeywordSpec {
        text: "cli",
        token_kind: None,
        scope: KeywordScope::Annotation,
        category: KeywordCategory::Annotation,
    },
    KeywordSpec {
        text: "cursor",
        token_kind: None,
        scope: KeywordScope::Annotation,
        category: KeywordCategory::Annotation,
    },
    KeywordSpec {
        text: "descriptio",
        token_kind: None,
        scope: KeywordScope::Contextual(ANNOTATION_MODIFIER),
        category: KeywordCategory::Annotation,
    },
    KeywordSpec {
        text: "futura",
        token_kind: None,
        scope: KeywordScope::Annotation,
        category: KeywordCategory::Annotation,
    },
    KeywordSpec {
        text: "imperium",
        token_kind: None,
        scope: KeywordScope::Annotation,
        category: KeywordCategory::Annotation,
    },
    KeywordSpec {
        text: "longum",
        token_kind: None,
        scope: KeywordScope::Contextual(ANNOTATION_MODIFIER),
        category: KeywordCategory::Annotation,
    },
    KeywordSpec {
        text: "operandus",
        token_kind: None,
        scope: KeywordScope::Annotation,
        category: KeywordCategory::Annotation,
    },
    KeywordSpec {
        text: "optio",
        token_kind: None,
        scope: KeywordScope::Annotation,
        category: KeywordCategory::Annotation,
    },
    KeywordSpec {
        text: "ubique",
        token_kind: None,
        scope: KeywordScope::Contextual(ANNOTATION_MODIFIER),
        category: KeywordCategory::Annotation,
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
            FxHashMap::with_capacity_and_hasher(KEYWORD_SPECS.len(), rustc_hash::FxBuildHasher);
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
fn keyword_by_kind() -> &'static FxHashMap<mem::Discriminant<TokenKind>, &'static KeywordSpec> {
    static MAP: OnceLock<FxHashMap<mem::Discriminant<TokenKind>, &'static KeywordSpec>> =
        OnceLock::new();
    MAP.get_or_init(|| {
        let mut map = FxHashMap::default();
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
            errors.push(KeywordGroupValidationError::EmptyGroup { group: group.id });
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

/// Globally reserved spellings that may still name parameters or members.
const GLOBAL_PARAMETER_OR_MEMBER: &[&str] = &["cape", "inter", "scribe", "incipiet", "ex", "ad"];

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
        | KeywordOwner::MemberIdentifier => true,
        KeywordOwner::EntryModifier
        | KeywordOwner::FunctionModifier
        | KeywordOwner::GenusHeader
        | KeywordOwner::GenusMember
        | KeywordOwner::RestPattern
        | KeywordOwner::SpreadExpression
        | KeywordOwner::AnnotationName
        | KeywordOwner::AnnotationModifier
        | KeywordOwner::ImportVisibility
        | KeywordOwner::TypeParameter
        | KeywordOwner::ParameterMode
        | KeywordOwner::AliasMarker
        | KeywordOwner::IterationMode
        | KeywordOwner::PredicateOperator
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
        | KeywordOwner::StatementIntroducer
        | KeywordOwner::DeclarationMarker
        | KeywordOwner::RouteExpression => false,
    }
}

fn keyword_owner_allows_spec(owner: KeywordOwner, spec: &KeywordSpec) -> bool {
    match spec.scope {
        KeywordScope::Contextual(owners) => owners.contains(&owner),
        KeywordScope::TestOwned | KeywordScope::Alias { .. } => {
            keyword_owner_is_identifier_like(owner)
        }
        KeywordScope::Global => {
            keyword_owner_is_identifier_like(owner)
                && GLOBAL_PARAMETER_OR_MEMBER.contains(&spec.text)
        }
        KeywordScope::Annotation => owner == KeywordOwner::AnnotationName,
    }
}

#[cfg(test)]
#[path = "keywords_test.rs"]
mod tests;
