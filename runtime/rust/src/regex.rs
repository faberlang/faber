//! Faber `regex` runtime carrier.
//!
//! The conversion `textus ↦ regex` is the one place a pattern is compiled or
//! rejected (RD-1, RD-9): [`Regex::new`] validates and compiles once, and every
//! operation on the resulting value is total. A rejected pattern fails with a
//! [`RegexError`] whose leading token is a stable construct id (RD-4), never
//! the engine's prose. The engine is the Rust `regex` crate (RD-8); the ids
//! come from the `regex-syntax` error kinds of the same parser.

use regex_syntax::ast::{self, Ast, ErrorKind};

/// RE2's repetition count cap; counts above it are `syntax`.
const MAX_REPEAT_COUNT: u32 = 1000;

/// Pattern carrier for Faber `regex`: the pattern text and its compiled
/// program, built once at construction.
#[derive(Clone)]
pub struct Regex {
    pattern: String,
    compiled: regex::Regex,
}

/// Why a pattern was rejected: a stable construct id, the code-point offset of
/// the construct in the pattern, and a short detail.
///
/// The construct ids are `lookahead`, `lookbehind`, `backreference`,
/// `unsupported_flag`, `atomic_group`, `possessive`, `recursion`,
/// `conditional` and `syntax`. [`std::fmt::Display`] renders the payload a
/// failed conversion carries: the id, `": "`, then the detail.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegexError {
    construct: &'static str,
    offset: usize,
    detail: String,
}

impl RegexError {
    /// The stable construct id (the leading token of the failure payload).
    #[must_use]
    pub fn construct(&self) -> &'static str {
        self.construct
    }

    /// Code-point offset of the rejected construct in the pattern.
    #[must_use]
    pub fn offset(&self) -> usize {
        self.offset
    }

    /// Engine detail for the rejection; not stable across engines.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl std::fmt::Display for RegexError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.construct, self.detail)
    }
}

impl std::error::Error for RegexError {}

impl Regex {
    /// Validate and compile `pattern`.
    ///
    /// # Errors
    ///
    /// Returns a [`RegexError`] carrying the construct id when the pattern is
    /// outside the dialect or does not compile.
    pub fn new(pattern: &str) -> Result<Self, RegexError> {
        let parsed = ast::parse::Parser::new()
            .parse(pattern)
            .map_err(|error| from_ast_error(pattern, &error))?;
        ast::visit(&parsed, DialectVisitor { pattern })?;
        let compiled = regex::Regex::new(pattern).map_err(|error| RegexError {
            construct: "syntax",
            offset: 0,
            detail: engine_detail(&error),
        })?;
        Ok(Self {
            pattern: pattern.to_owned(),
            compiled,
        })
    }

    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// Whether the pattern matches `textus` (the Rust `regex` crate
    /// `is_match` semantics).
    ///
    /// G12 `consentit`: the pattern was validated and compiled at
    /// construction, so the verb is total and never recompiles.
    // The owned `String` is the generated-code calling convention:
    // `radix/crates/radix-hir-rust/src/expr/call/intrinsics.rs::generate_regex_method`
    // emits `{ let t: String = …; receiver.consentit(t) }`; accepting `&str`
    // would break emitted programs.
    #[allow(clippy::needless_pass_by_value)]
    #[must_use]
    pub fn consentit(&self, textus: String) -> bool {
        self.compiled.is_match(&textus)
    }
}

impl PartialEq for Regex {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
    }
}

impl Eq for Regex {}

impl std::fmt::Debug for Regex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "regex {:?}", self.pattern)
    }
}

impl std::fmt::Display for Regex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.pattern)
    }
}

/// The last line of the engine's multi-line message (`error: <detail>`).
fn engine_detail(error: &regex::Error) -> String {
    let text = error.to_string();
    let last = text.lines().last().unwrap_or("");
    last.strip_prefix("error: ").unwrap_or(last).to_owned()
}

/// Reject at byte offset `start` of `pattern` with construct `id`.
fn reject(pattern: &str, id: &'static str, start: usize, detail: impl Into<String>) -> RegexError {
    RegexError {
        construct: id,
        offset: pattern.get(..start).map_or(0, |head| head.chars().count()),
        detail: detail.into(),
    }
}

/// Map a `regex-syntax` AST parse error to a construct id. Lookaround and
/// backreferences have their own error kinds; atomic groups, conditionals,
/// recursion and named backreferences surface as unrecognized flags or
/// escapes, so the id comes from the character at the error span (a peek, no
/// parse). The MIR runner maps the same kinds the same way.
fn from_ast_error(pattern: &str, error: &ast::Error) -> RegexError {
    let offset = error.span().start.offset;
    let rest = pattern.get(offset..).unwrap_or("");
    let id = match error.kind() {
        ErrorKind::UnsupportedBackreference => "backreference",
        ErrorKind::UnsupportedLookAround => {
            if rest.starts_with("(?<") {
                "lookbehind"
            } else {
                "lookahead"
            }
        }
        ErrorKind::EscapeUnrecognized if rest.starts_with("\\k") => "backreference",
        ErrorKind::FlagUnrecognized if rest.starts_with("P=") => "backreference",
        ErrorKind::FlagUnrecognized if rest.starts_with("P>") => "recursion",
        ErrorKind::FlagUnrecognized => match rest.chars().next() {
            Some('>') => "atomic_group",
            Some('(') => "conditional",
            Some(c) if c.is_ascii_digit() || c == '&' || c == '+' => "recursion",
            Some(c) if c.is_ascii_alphabetic() => "unsupported_flag",
            _ => "syntax",
        },
        _ => "syntax",
    };
    reject(pattern, id, offset, error.kind().to_string())
}

/// Rejects the constructs the AST accepts but the dialect does not (the
/// portable core of the reference syntax): the `R` and `U` and `u` flags,
/// `\\u` escapes, a repetition of a repetition (possessive quantifiers),
/// counts above 1000, nested classes, class set operations and the
/// non-simple word boundaries.
struct DialectVisitor<'p> {
    pattern: &'p str,
}

impl DialectVisitor<'_> {
    fn check_flags(&self, flags: &ast::Flags) -> Result<(), RegexError> {
        for item in &flags.items {
            let ast::FlagsItemKind::Flag(flag) = &item.kind else {
                continue;
            };
            let start = item.span.start.offset;
            match flag {
                ast::Flag::CaseInsensitive
                | ast::Flag::MultiLine
                | ast::Flag::DotMatchesNewLine
                | ast::Flag::IgnoreWhitespace => {}
                // `R` is a CRLF flag letter to the Rust parser and the
                // recursion spelling `(?R)` to the dialect.
                ast::Flag::CRLF => {
                    return Err(reject(
                        self.pattern,
                        "recursion",
                        start,
                        "recursion is not supported",
                    ));
                }
                ast::Flag::SwapGreed | ast::Flag::Unicode => {
                    return Err(reject(
                        self.pattern,
                        "unsupported_flag",
                        start,
                        format!("flag {flag:?} is not supported"),
                    ));
                }
            }
        }
        Ok(())
    }

    fn check_literal(&self, literal: &ast::Literal) -> Result<(), RegexError> {
        use ast::{
            HexLiteralKind::{UnicodeLong, UnicodeShort},
            LiteralKind,
        };
        match literal.kind {
            LiteralKind::HexFixed(UnicodeShort | UnicodeLong)
            | LiteralKind::HexBrace(UnicodeShort | UnicodeLong) => Err(reject(
                self.pattern,
                "syntax",
                literal.span.start.offset,
                "\\u and \\U escapes are not supported",
            )),
            _ => Ok(()),
        }
    }

    fn check_repetition(&self, repetition: &ast::Repetition) -> Result<(), RegexError> {
        let start = repetition.span.start.offset;
        if matches!(&*repetition.ast, Ast::Repetition(_)) {
            let possessive =
                matches!(repetition.op.kind, ast::RepetitionKind::OneOrMore) && repetition.greedy;
            return Err(if possessive {
                reject(
                    self.pattern,
                    "possessive",
                    start,
                    "possessive quantifiers are not supported",
                )
            } else {
                reject(self.pattern, "syntax", start, "repetition of a repetition")
            });
        }
        let over = match &repetition.op.kind {
            ast::RepetitionKind::Range(
                ast::RepetitionRange::Exactly(n) | ast::RepetitionRange::AtLeast(n),
            ) => *n > MAX_REPEAT_COUNT,
            ast::RepetitionKind::Range(ast::RepetitionRange::Bounded(m, n)) => {
                *m > MAX_REPEAT_COUNT || *n > MAX_REPEAT_COUNT
            }
            _ => false,
        };
        if over {
            return Err(reject(
                self.pattern,
                "syntax",
                start,
                "repetition count exceeds 1000",
            ));
        }
        Ok(())
    }
}

impl ast::Visitor for DialectVisitor<'_> {
    type Output = ();
    type Err = RegexError;

    fn finish(self) -> Result<(), RegexError> {
        Ok(())
    }

    fn visit_pre(&mut self, node: &Ast) -> Result<(), RegexError> {
        match node {
            Ast::Flags(set) => self.check_flags(&set.flags),
            Ast::Group(group) => match &group.kind {
                ast::GroupKind::NonCapturing(flags) => self.check_flags(flags),
                _ => Ok(()),
            },
            Ast::Repetition(repetition) => self.check_repetition(repetition),
            Ast::Literal(literal) => self.check_literal(literal),
            Ast::Assertion(assertion) => match assertion.kind {
                ast::AssertionKind::StartLine
                | ast::AssertionKind::EndLine
                | ast::AssertionKind::StartText
                | ast::AssertionKind::EndText
                | ast::AssertionKind::WordBoundary
                | ast::AssertionKind::NotWordBoundary => Ok(()),
                _ => Err(reject(
                    self.pattern,
                    "syntax",
                    assertion.span.start.offset,
                    "word boundary variant is not supported",
                )),
            },
            _ => Ok(()),
        }
    }

    fn visit_class_set_item_pre(&mut self, item: &ast::ClassSetItem) -> Result<(), RegexError> {
        match item {
            ast::ClassSetItem::Bracketed(class) => Err(reject(
                self.pattern,
                "syntax",
                class.span.start.offset,
                "nested character class",
            )),
            ast::ClassSetItem::Literal(literal) => self.check_literal(literal),
            _ => Ok(()),
        }
    }

    fn visit_class_set_binary_op_pre(
        &mut self,
        op: &ast::ClassSetBinaryOp,
    ) -> Result<(), RegexError> {
        Err(reject(
            self.pattern,
            "syntax",
            op.span.start.offset,
            "character class set operation",
        ))
    }
}
