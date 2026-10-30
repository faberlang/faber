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
///
/// Variants use a fact-noun grammar, naming the failed fact or state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LexErrorKind {
    /// String literal reached newline or EOF before its closing delimiter.
    UnterminatedString,

    /// Numeric literal text could not be parsed under its declared radix/form.
    NumberInvalid,

    /// Escape processing rejected a string escape sequence.
    EscapeInvalid,

    /// Character had no token meaning in the current lexer mode.
    CharacterUnexpected,

    /// C-style `//` or `/* */` comments are not part of the Faber lexical contract.
    CommentSyntaxInvalid,

    /// `#` appeared after other tokens on the same logical line.
    InlineComment,
}

/// Parser diagnostic taxonomy.
///
/// Variants are intentionally grouped by the part of the grammar that failed.
/// Add new variants when a category unlocks clearer recovery, tests, or user
/// messaging; otherwise prefer an existing bucket with a precise message.
///
/// Variants use a fact-noun grammar, naming the failed fact or state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ParseErrorKind {
    // Lexer passthrough - allows treating lex/parse errors uniformly
    LexError,

    // General errors - fallback for uncommon cases
    Expected,
    Unexpected,

    // Declaration errors - structural issues in top-level constructs
    ModifierDuplicate,
    AnnotationInvalid,
    DeclarationInvalid,
    ModifierInvalid,
    ParameterInvalid,
    TypeParameterInvalid,
    ClassBodyAbsent,
    ClassNameAbsent,
    FunctionBodyAbsent,
    FunctionNameAbsent,

    // Statement errors - control flow and block structure issues
    CasuArmInvalid,
    CasuValueInvalid,
    PatternInvalid,
    StatementInvalid,
    BlockAbsent,
    /// `∴` used where a compact statement body requires `ergo`.
    ThereforeWrongContext,
    /// `ergo` used where a compact clausura requires `∴`.
    ErgoWrongContext,
    ConditionAbsent,

    // Expression errors - malformed expressions and operators
    AssignmentTargetInvalid,
    CallArgumentInvalid,
    ExpressionInvalid,
    LiteralInvalid,
    MemberAccessInvalid,
    OperatorInvalid,
    UnterminatedGroup,

    // Type annotation errors
    TypeInvalid,
    TypeAnnotationInvalid,
    UnterminatedTypeParams,

    // Import errors
    ImportInvalid,

    // Comment/trivia errors
    /// A `LineComment` token was not claimed by a legal attachment boundary
    /// (forward statement/member trivia, block end, or program end).
    CommentPlacementInvalid,
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
    /// A value binding uses the active locale's empty-collection marker
    /// spelling, making bare marker expressions visually ambiguous.
    EmptyMarkerShadowing,
    /// Scalar indexing on unbounded UTF-8 `textus` walks from the start and
    /// traps when the selected scalar is not representable as one ASCII byte.
    UnboundedTextusIndex,
    /// An `est M` type test names a member its operand union cannot hold —
    /// a statically dead test (U1R).
    TypeTestMemberAbsent,
    /// An all-nominal `∪` whose constituents share no members is store-only
    /// until narrowed (U1R guardrail 3).
    UnionNoCommonMembers,
    /// A same-name method with incompatible signatures across `∪`
    /// constituents silently leaves the common set (U1R guardrail 4).
    UnionMethodSignatureShadowed,
    /// A declared function's cyclomatic complexity exceeded the applicable
    /// budget. Warn-only; promotion is the caller's deny policy.
    ComplexityBudgetExceeded,
    /// A `genus` member carries `@ publica`, which is redundant because members
    /// are public by default (D5.5 interim).
    RedundantMemberVisibility,
    /// A `discerne` whose coverage could not be computed for its pattern
    /// kinds (D4.8): the match is compiled, but exhaustiveness was not
    /// checked, so the author is told rather than left in silence.
    CoverageUnchecked,
    /// A declared identifier starts with `_` (other than the bare `_` hole).
    /// Not Faber style: unmarked top-level declarations are already private
    /// (D5.6). Warn-only; strict builds deny it through `deny_warnings`.
    LeadingUnderscore,
    /// A field of a type on a reference cycle can hold a closable value
    /// (`sermo`, or a type implementing norma's `Closable`). Reference
    /// cycles never reach their last reference, so the value never
    /// auto-closes (D8.3). Warn-only.
    ClosableInCycle,
    /// A kernel's trailing `u32` parameter is bound by the compiler as its
    /// lane id, whatever it is named (gpu-reset rule 12.5). The positional
    /// lane ABI is retiring in favor of a `filum` loop. Warn-only until
    /// `filum` ships on the runner.
    PositionalLaneParam,
    /// A tensor index form `t[i]` inside a kernel whose index the checker
    /// cannot prove in range. The form stays checked (one compare per lane);
    /// the warning suggests the range type (`x ↦ 0‥N`) and `get` (gpu-reset
    /// K11). Warn-only.
    UnprovenKernelIndex,
    /// A `"…"` string literal escapes a quote (`\"`). A guillemet block
    /// `«…»` wraps the same text with no escape needed for `"` or `'`.
    /// Warn-only (operator proposal 4, 2026-10-13).
    PreferGuillemetQuote,
    /// One template literal holds both plain holes (`§`, `§{name}`) and
    /// numbered holes (`§N`). Plain holes count from 0 in their own order and
    /// ignore the numbered ones, which routinely reads a different argument
    /// than the author meant. Warn-only; numbering is unchanged (operator
    /// ruling 2026-10-14, template-hole rule P5).
    MixedTemplateHoles,
}

/// Semantic diagnostic categories used by passes and command policy.
///
/// Variants are intentionally phase-oriented rather than renderer-oriented:
/// renderers should use the message/help text for display, while compiler
/// policy can match the kind when it needs to distinguish unresolved names,
/// type failures, ownership failures, or warnings.
///
/// Variants use a fact-noun grammar, naming the failed fact or state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SemanticErrorKind {
    // Name resolution
    VariableUndefined,
    TypeUndefined,
    FunctionUndefined,
    MemberUndefined,
    DefinitionDuplicate,
    ImportAbsent,
    CircularDependency,
    LoweringError,
    CliValidation,
    AnnotationInvalid,
    /// A declaration mixes distinct visibility tiers (`@ publica @ privata`).
    ConflictingVisibility,
    ShadowedVariable,

    // Type errors
    TypeMismatch,
    OperandTypesInvalid,
    NonCallable,
    ArityMismatch,
    TypeAnnotationAbsent,
    CastInvalid,
    ConversionInvalid,
    UnavailableDeclaration,
    AirPurityViolation,
    /// A module-level constant initializer that is not a compile-time constant,
    /// or whose evaluation overflows or divides by zero (D5.9).
    StaticInitializerInvalid,
    /// A build-time file embed (`insere "path"`, D8.10) that cannot be
    /// resolved or does not fit its declared type.
    InsereInvalid,
    /// A read, write, or call of a `genus` member its visibility does not
    /// admit here (`@ privata` outside the type's own methods, `@ interna`
    /// from another package; D5.3).
    MemberInaccessible,
    /// A `praefixum { … }` build-time initializer (D8.4–D8.6) that is
    /// misplaced, reaches a forbidden effect, fails, or yields a value with
    /// no literal form.
    PraefixumInvalid,

    // Assignment
    ImmutableAssignment,
    AssignmentTargetInvalid,
    UseBeforeInit,

    // Control flow
    BreakOutsideBreakable,
    ContinueOutsideLoop,
    ReturnOutsideFunction,
    ReturnAbsent,

    // Pattern matching
    NonExhaustiveMatch,
    UnreachablePattern,
    PatternDuplicate,
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

    /// A legacy top-level `fixum`/`varia` binding. Module-level mutable
    /// state does not exist; the migration target is a module-level
    /// `fixum T X = e` constant (D5.7). Was WARN030 `top_level_binding` until
    /// the library sweep landed; now an error.
    TopLevelBindingForbidden,

    /// A plain `mut T y ← x` that would share a read-only (`const` or `ref`)
    /// value into a writable name (need, rule 2: `const` is deep).
    /// The legal spellings are `← copy x` and `← own x`.
    ConstSharedIntoVar,

    /// A `copy` acquisition (value-side `copy e`, a `copy`-marked binding or
    /// parameter, or a call-site `copy` echo) whose value transitively holds a
    /// member that cannot be duplicated (a conversation handle or another
    /// closable resource with identity). The deep duplicate would silently
    /// share the resource; the refusal names the member path.
    CopyNonduplicable,

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
            SemanticErrorKind::VariableUndefined
                | SemanticErrorKind::TypeUndefined
                | SemanticErrorKind::FunctionUndefined
                | SemanticErrorKind::MemberUndefined
                | SemanticErrorKind::ImportAbsent
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

    /// One-based physical line in `file` when source context was available.
    ///
    /// Plain `file:N` locations must use this, not [`Span::start`]. Byte offsets
    /// printed as line numbers leak past physical EOF (`main.fab:1846` in a
    /// 93-line file).
    #[serde(default)]
    pub line: Option<u32>,

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
            line: None,
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

    /// Attach file, span, physical line, and containing source line for phase conversions.
    #[must_use]
    pub fn with_source_context(mut self, file: &str, source: &str, span: Span) -> Self {
        file.clone_into(&mut self.file);
        self.span = Some(span);
        self.line = Some(offset_to_line(source, span.start as usize));
        self.source_line = Some(get_line_at_offset(source, span.start as usize));
        self
    }

    /// User-facing `file:line` when source context mapped a physical line.
    ///
    /// Span-only diagnostics fall back to `file:byte` so tests that never saw
    /// source text keep a stable location token.
    #[must_use]
    pub fn display_location(&self) -> String {
        match (self.line, self.span) {
            (Some(line), _) if self.file.is_empty() => line.to_string(),
            (Some(line), _) => format!("{}:{line}", self.file),
            (_, Some(span)) if self.file.is_empty() => span.start.to_string(),
            (_, Some(span)) => format!("{}:{}", self.file, span.start),
            _ => self.file.clone(),
        }
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
    ///
    /// The attempted path belongs to [`Self::file`], so keeping it out of the
    /// message prevents plain renderers from displaying it twice. The named
    /// arguments preserve the path facts for structured renderers and tools.
    #[must_use]
    pub fn io_error(path: &Path, err: &std::io::Error) -> Self {
        let base = path
            .parent()
            .filter(|base| !base.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let artifact = io_artifact(path);
        Self::error(format!(
            "cannot read {artifact} resolved against '{}': {err}",
            base.display()
        ))
        .with_phase(DiagnosticPhase::Io)
        .with_file(path.display().to_string())
        .with_arg("path", path.display().to_string())
        .with_arg("base", base.display().to_string())
        .with_arg("artifact", artifact)
    }

    /// Create a backend diagnostic when generation fails outside source spans.
    #[must_use]
    pub fn codegen_error(message: &str) -> Self {
        Self::error(format!("code generation failed: {message}"))
            .with_phase(DiagnosticPhase::Codegen)
            .with_code("CODEGEN001")
    }
}

fn io_artifact(path: &Path) -> &'static str {
    match (
        path.file_name().and_then(|name| name.to_str()),
        path.extension().and_then(|extension| extension.to_str()),
    ) {
        (Some("faber.toml"), _) => "manifest",
        (Some("pack.toml"), _) => "locale pack",
        (_, Some("fab" | "proba")) => "source file",
        _ => "file",
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

/// Longest unclipped line [`get_line_at_offset`] returns, in characters.
///
/// A hostile single-line input (multi-megabyte, newline-free) must not drive
/// renderer output volume; longer lines clip to this head plus an ellipsis
/// marker. Documented SEC-A1 default — an exact value may be pinned later.
pub const MAX_SOURCE_LINE_CHARS: usize = 400;

const SOURCE_LINE_ELLIPSIS: &str = "…";

/// Source line containing a byte offset, clamped to the source length.
///
/// The offset snaps down to a UTF-8 char boundary, and the returned line
/// excludes its terminating `\n`. A line longer than `MAX_SOURCE_LINE_CHARS`
/// returns that many characters plus an ellipsis marker.
#[must_use]
pub fn get_line_at_offset(source: &str, offset: usize) -> String {
    let offset = nearest_char_boundary(source, offset);
    let start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = source[offset..]
        .find('\n')
        .map_or(source.len(), |i| offset + i);
    let line = &source[start..end];
    match line.char_indices().nth(MAX_SOURCE_LINE_CHARS) {
        Some((cut, _)) => format!("{}{SOURCE_LINE_ELLIPSIS}", &line[..cut]),
        None => line.to_owned(),
    }
}

/// One-based physical line for a byte offset, clamped to the source line count.
///
/// Newline count before the clamped offset is the line index. Offsets past EOF
/// or onto a trailing empty line after a final `\n` stay on the last content
/// line so user-facing `file:N` cannot exceed the physical line count.
#[must_use]
pub fn offset_to_line(source: &str, offset: usize) -> u32 {
    let offset = nearest_char_boundary(source, offset);
    let newlines = source[..offset].bytes().filter(|&b| b == b'\n').count();
    // The line index only exceeds the physical line count when the offset sits
    // past the final `\n`, which is the end of the source: step back one line
    // there instead of counting every line of the file.
    let past_last_line = offset == source.len() && source.ends_with('\n');
    let line = if past_last_line {
        newlines
    } else {
        newlines.saturating_add(1)
    };
    u32::try_from(line).unwrap_or(u32::MAX)
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
