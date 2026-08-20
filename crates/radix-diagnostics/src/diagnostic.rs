//! Core diagnostic data model for the Radix compiler.
//!
//! Owns the transport types and typed phase-error kinds shared across compiler
//! phases. Phase-specific error conversion and catalogs live outside this crate
//! to keep the foundation dependency-free.

use std::fmt;
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::Span;

/// Named fact attached to a diagnostic template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiagnosticArg {
    /// Stable template argument name.
    pub name: &'static str,

    /// Source, keyword, type, or computed value used by the template.
    pub value: String,
}

impl DiagnosticArg {
    #[must_use]
    pub fn new(name: &'static str, value: impl Into<String>) -> Self {
        Self {
            name,
            value: value.into(),
        }
    }
}

/// User-visible severity and compilation policy for a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    /// Fatal diagnostic that should prevent successful compilation.
    Error,

    /// Non-fatal diagnostic for suspicious code or discouraged usage.
    Warning,

    /// Advisory note that explains context without indicating a fault.
    Info,
}

/// Compiler phase that produced or normalized a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticPhase {
    /// Filesystem or source-loading failure.
    Io,

    /// Lexer/tokenization failure.
    Lex,

    /// Parser/grammar failure.
    Parse,

    /// Name resolution failure.
    Resolve,

    /// AST-to-HIR lowering failure.
    Lower,

    /// Type checking failure.
    Typecheck,

    /// Coarse semantic analysis failure when no narrower pass is exposed.
    Analysis,

    /// MIR lowering, validation, or inspection failure.
    Mir,

    /// Target code generation failure.
    Codegen,

    /// CLI/tooling policy failure outside a compiler phase.
    Tool,
}

impl fmt::Display for DiagnosticPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            DiagnosticPhase::Io => "io",
            DiagnosticPhase::Lex => "lex",
            DiagnosticPhase::Parse => "parse",
            DiagnosticPhase::Resolve => "resolve",
            DiagnosticPhase::Lower => "lower",
            DiagnosticPhase::Typecheck => "typecheck",
            DiagnosticPhase::Analysis => "analysis",
            DiagnosticPhase::Mir => "mir",
            DiagnosticPhase::Codegen => "codegen",
            DiagnosticPhase::Tool => "tool",
        };
        f.write_str(name)
    }
}

/// Lexical failure classes produced before parsing begins.
///
/// These variants describe tokenization failures only. Syntax-level decisions
/// such as "unexpected token" or "missing expression" are intentionally not
/// modeled here. Owned by the diagnostics foundation so [`Diagnostic`] can
/// carry the typed kind without a dependency cycle on phase crates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LexErrorKind {
    /// String literal reached newline or EOF before its closing delimiter.
    UnterminatedString,

    /// Numeric literal text could not be parsed under its declared radix/form.
    InvalidNumber,

    /// Escape processing rejected a string escape sequence.
    InvalidEscape,

    /// Character had no token meaning in the current lexer mode.
    UnexpectedCharacter,

    /// Line-start `§` file directives were removed in favor of `+++` frontmatter.
    RemovedSectionDirective,

    /// C-style `//` or `/* */` comments are not part of the Faber lexical contract.
    InvalidCommentSyntax,

    /// `#` appeared after other tokens on the same logical line.
    InlineComment,
}

/// Parser diagnostic taxonomy.
///
/// Variants are intentionally grouped by the part of the grammar that failed.
/// Add new variants when a category unlocks clearer recovery, tests, or user
/// messaging; otherwise prefer an existing bucket with a precise message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParseErrorKind {
    // Lexer passthrough - allows treating lex/parse errors uniformly
    LexError,

    // General errors - fallback for uncommon cases
    Expected,
    Unexpected,

    // Declaration errors - structural issues in top-level constructs
    DuplicateModifier,
    InvalidAnnotation,
    InvalidDeclaration,
    InvalidModifier,
    InvalidParameter,
    InvalidTypeParameter,
    MissingClassBody,
    MissingClassName,
    MissingFunctionBody,
    MissingFunctionName,

    // Statement errors - control flow and block structure issues
    InvalidCasuArm,
    InvalidCasuValue,
    InvalidPattern,
    InvalidStatement,
    MissingBlock,
    /// `∴` used where a compact statement body requires `ergo`.
    ThereforeWrongContext,
    /// `ergo` used where a compact clausura requires `∴`.
    ErgoWrongContext,
    MissingCondition,

    // Expression errors - malformed expressions and operators
    InvalidAssignmentTarget,
    InvalidCallArgument,
    InvalidExpression,
    InvalidLiteral,
    InvalidMemberAccess,
    InvalidOperator,
    UnterminatedGroup,

    // Type annotation errors
    InvalidType,
    InvalidTypeAnnotation,
    UnterminatedTypeParams,

    // Import errors
    InvalidImport,

    // Comment/trivia errors
    /// A `LineComment` token was not claimed by a legal attachment boundary
    /// (forward statement/member trivia, block end, or program end).
    InvalidCommentPlacement,
}

/// Non-fatal semantic findings that share the normal diagnostic transport.
///
/// These variants communicate quality, portability, or intent issues without
/// causing a result to fail unless a caller layers on a stricter
/// warnings-as-errors policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarningKind {
    UnusedVariable,
    UnusedImport,
    UnusedFunction,
    UnreachableCode,
    UnnecessaryCast,
    TargetNoop,
    UnusedMutRefParam,
    UnusedMoveParam,
    ExplicitIgnotumAnnotation,
    /// An unguarded wildcard/binding arm absorbed the null constituent of a
    /// nullable scrutinee (`Option(_)` or a union containing `nihil`),
    /// silently treating null as a value.
    WildcardAbsorbsNull,
    UnnecessaryVaria,
    /// A provider/file-interface export could not be remapped and was skipped;
    /// sibling exports in the same namespace may still be usable.
    FileInterfaceExportSkipped,
    /// A postfix `curata <ident>` names an allocator contract the compiler
    /// does not recognize. Accepted at warning level until allocator
    /// threading ships.
    UnrecognizedCurataAllocator,
    /// A monomorphic `_` hole solved to a multi-member union. The source
    /// still looks monomorphic while the inferred type is a sum of
    /// alternatives; the warning makes that cross-shape explicit.
    MonomorphicHoleSolvedToUnion,
    /// A union `∪` hole solved to a single inhabitant type. The sum-shaped
    /// slot collapsed to one member; the warning makes that cross-shape
    /// explicit.
    UnionHoleSolvedMonomorphically,
    /// A string-typed `+` chain of 3+ operands contains at least one
    /// string-literal operand; a `§` string template would read better.
    PreferStringTemplate,
    /// A local binding declared `mut`. Grammar allows it (borrowed-into-local);
    /// the lint discourages the form in favor of a `mut` parameter (OQ-3).
    DiscouragedMutLocal,
}

/// Semantic diagnostic categories used by passes and command policy.
///
/// Variants are intentionally phase-oriented rather than renderer-oriented:
/// renderers should use the message/help text for display, while compiler
/// policy can match the kind when it needs to distinguish unresolved names,
/// type failures, ownership failures, or warnings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticErrorKind {
    // Name resolution
    UndefinedVariable,
    UndefinedType,
    UndefinedFunction,
    UndefinedMember,
    DuplicateDefinition,
    ImportNotFound,
    CircularDependency,
    LoweringError,
    CliValidation,
    InvalidAnnotation,
    /// A declaration mixes distinct visibility tiers (`@ publica @ privata`).
    ConflictingVisibility,
    ShadowedVariable,

    // Type errors
    TypeMismatch,
    InvalidOperandTypes,
    NotCallable,
    WrongArity,
    MissingTypeAnnotation,
    InvalidCast,
    InvalidConversion,
    UnavailableDeclaration,
    AirPurityViolation,

    // Assignment
    ImmutableAssignment,
    InvalidAssignmentTarget,
    UseBeforeInit,

    // Control flow
    BreakOutsideBreakable,
    ContinueOutsideLoop,
    ReturnOutsideFunction,
    MissingReturn,

    // Pattern matching
    NonExhaustiveMatch,
    UnreachablePattern,
    DuplicatePattern,
    OmniaRequiresEnum,
    OmniaForbidsCatchall,

    // Borrowing (for Rust target)
    UseAfterMove,
    BorrowOfMoved,
    MutableBorrowConflict,
    CannotMoveOut,
    LifetimeMismatch,
    AssignToImmutableBorrow,
    ModeMismatch,

    // Warnings
    Warning(WarningKind),
}

impl SemanticErrorKind {
    /// Return whether permissive check mode may report this hard error as a softer result.
    ///
    /// The downgrade set is restricted to missing symbols/imports because those
    /// can arise while users explore incomplete files. Type, control-flow, and
    /// lowering failures still indicate that the current program shape is known
    /// to be invalid.
    #[must_use]
    pub fn is_permissive_check_downgrade(self) -> bool {
        matches!(
            self,
            SemanticErrorKind::UndefinedVariable
                | SemanticErrorKind::UndefinedType
                | SemanticErrorKind::UndefinedFunction
                | SemanticErrorKind::UndefinedMember
                | SemanticErrorKind::ImportNotFound
        )
    }
}

/// Machine-readable phase error kind preserved through conversion.
///
/// Catalog `code` strings remain for human-readable docs and renderers. This
/// enum is the programmatic identity carrier so consumers do not reconstruct
/// kinds from Debug text or argument strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhaseErrorKind {
    Lex(LexErrorKind),
    Parse(ParseErrorKind),
    Semantic(SemanticErrorKind),
}

/// Reportable compiler message after phase-specific errors are normalized.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    /// Compilation policy and visual priority for the message.
    pub severity: Severity,

    /// Compiler phase that owns the diagnostic.
    pub phase: DiagnosticPhase,

    /// Compact diagnostic identity or minimal tooling message.
    pub message: String,

    /// Stable catalog code used by docs, tests, and external tooling.
    pub code: Option<&'static str>,

    /// Typed phase error kind when conversion preserved one; absent for
    /// uncoded tooling messages and phases that have not yet adopted typed kinds.
    pub error_kind: Option<PhaseErrorKind>,

    /// Named facts used by reader-pack templates and documentation lookup.
    pub args: Vec<DiagnosticArg>,

    /// Source filename or path to display in reports.
    pub file: String,

    /// Byte span in `file`; absent when the error is not tied to source text.
    pub span: Option<Span>,

    /// Full source line containing `span`, captured for plain renderers.
    pub source_line: Option<String>,

    /// Actionable guidance shown after the primary message.
    pub help: Option<String>,
}

impl Diagnostic {
    #[must_use]
    pub fn new(severity: Severity, message: impl Into<String>) -> Self {
        Self {
            severity,
            phase: DiagnosticPhase::Tool,
            message: message.into(),
            code: None,
            error_kind: None,
            args: Vec::new(),
            file: String::new(),
            span: None,
            source_line: None,
            help: None,
        }
    }

    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self::new(Severity::Error, message)
    }

    #[must_use]
    pub fn warning(message: impl Into<String>) -> Self {
        Self::new(Severity::Warning, message)
    }

    #[must_use]
    pub fn with_phase(mut self, phase: DiagnosticPhase) -> Self {
        self.phase = phase;
        self
    }

    #[must_use]
    pub fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }

    #[must_use]
    pub fn with_error_kind(mut self, kind: PhaseErrorKind) -> Self {
        self.error_kind = Some(kind);
        self
    }

    #[must_use]
    pub fn with_lex_kind(mut self, kind: LexErrorKind) -> Self {
        self.error_kind = Some(PhaseErrorKind::Lex(kind));
        self
    }

    #[must_use]
    pub fn lex_kind(&self) -> Option<LexErrorKind> {
        match self.error_kind {
            Some(PhaseErrorKind::Lex(kind)) => Some(kind),
            _ => None,
        }
    }

    #[must_use]
    pub fn with_parse_kind(mut self, kind: ParseErrorKind) -> Self {
        self.error_kind = Some(PhaseErrorKind::Parse(kind));
        self
    }

    #[must_use]
    pub fn parse_kind(&self) -> Option<ParseErrorKind> {
        match self.error_kind {
            Some(PhaseErrorKind::Parse(kind)) => Some(kind),
            _ => None,
        }
    }

    #[must_use]
    pub fn with_semantic_kind(mut self, kind: SemanticErrorKind) -> Self {
        self.error_kind = Some(PhaseErrorKind::Semantic(kind));
        self
    }

    #[must_use]
    pub fn semantic_kind(&self) -> Option<SemanticErrorKind> {
        match self.error_kind {
            Some(PhaseErrorKind::Semantic(kind)) => Some(kind),
            _ => None,
        }
    }

    #[must_use]
    pub fn with_args(mut self, args: Vec<DiagnosticArg>) -> Self {
        self.args = args;
        self
    }

    #[must_use]
    pub fn with_arg(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.args.push(DiagnosticArg::new(name, value));
        self
    }

    /// Override severity when command policy intentionally downgrades a fault.
    #[must_use]
    pub fn with_severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    #[must_use]
    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = file.into();
        self
    }

    #[must_use]
    pub fn with_span(mut self, span: Span) -> Self {
        self.span = Some(span);
        self
    }

    #[must_use]
    pub fn with_source_line(mut self, line: impl Into<String>) -> Self {
        self.source_line = Some(line.into());
        self
    }

    /// Attach file, span, and containing source line for phase conversions.
    #[must_use]
    pub fn with_source_context(mut self, file: &str, source: &str, span: Span) -> Self {
        file.clone_into(&mut self.file);
        self.span = Some(span);
        self.source_line = Some(get_line_at_offset(source, span.start as usize));
        self
    }

    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    /// Sub-variant selected inside a stable diagnostic code, when present.
    #[must_use]
    pub fn issue(&self) -> Option<&str> {
        self.args
            .iter()
            .find(|arg| arg.name == "issue")
            .map(|arg| arg.value.as_str())
    }

    /// Lookup key for documentation and explain surfaces.
    #[must_use]
    pub fn lookup_key(&self) -> Option<String> {
        self.code.map(|code| match self.issue() {
            Some(issue) => format!("{code}:{issue}"),
            None => code.to_owned(),
        })
    }

    /// Compact code label used in default CLI output.
    #[must_use]
    pub fn display_label(&self) -> Option<String> {
        self.lookup_key()
    }

    #[must_use]
    pub fn is_error(&self) -> bool {
        self.severity == Severity::Error
    }

    /// Convert a filesystem read failure into a path-scoped diagnostic.
    #[must_use]
    pub fn io_error(path: &Path, err: &std::io::Error) -> Self {
        Self::error(format!("cannot read '{}': {}", path.display(), err))
            .with_phase(DiagnosticPhase::Io)
            .with_file(path.display().to_string())
    }

    /// Create a backend diagnostic when generation fails outside source spans.
    #[must_use]
    pub fn codegen_error(message: &str) -> Self {
        Self::error(format!("code generation failed: {message}"))
            .with_phase(DiagnosticPhase::Codegen)
            .with_code("CODEGEN001")
    }
}

/// Build the compact diagnostic identity string used as the default message.
#[must_use]
pub fn diagnostic_identity(code: &str, args: &[DiagnosticArg]) -> String {
    match parse_arg_value(args, "issue") {
        Some(issue) if !issue.is_empty() => {
            // WARN014: plain check one-liners use identity only — fold export
            // names so hollow skip is not silent about *which* export died.
            if issue == "file_interface_export_skipped" {
                let namespace =
                    parse_arg_value(args, "namespace").unwrap_or_else(|| "?".to_owned());
                let export = parse_arg_value(args, "export").unwrap_or_else(|| "?".to_owned());
                return format!("{code}.{issue}:{namespace}.{export}");
            }
            format!("{code}.{issue}")
        }
        _ => code.to_owned(),
    }
}

#[must_use]
pub fn parse_arg_value(args: &[DiagnosticArg], name: &str) -> Option<String> {
    args.iter()
        .find(|arg| arg.name == name)
        .map(|arg| arg.value.clone())
}

#[must_use]
pub fn get_line_at_offset(source: &str, offset: usize) -> String {
    let offset = nearest_char_boundary(source, offset);
    let start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = source[offset..]
        .find('\n')
        .map_or(source.len(), |i| offset + i);
    source[start..end].to_owned()
}

/// Snap a byte offset down to the nearest UTF-8 char boundary.
#[must_use]
pub fn nearest_char_boundary(source: &str, offset: usize) -> usize {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}
