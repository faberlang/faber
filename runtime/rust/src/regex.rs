//! Faber `regex` runtime carrier.
//!
//! The conversion `textus ↦ regex` is the one place a pattern is compiled or
//! rejected (RD-1, RD-9): [`Regex::new`] validates and compiles once, and every
//! operation on the resulting value is total. A rejected pattern fails with a
//! [`RegexError`] whose leading token is a stable construct id (RD-4), never
//! the engine's prose. The engine is the Rust `regex` crate (RD-8); the ids
//! come from the `regex-syntax` error kinds of the same parser.

use regex_syntax::ast::{self, ErrorKind};

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
        if let Err(error) = ast::parse::Parser::new().parse(pattern) {
            return Err(from_ast_error(pattern, &error));
        }
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

fn from_ast_error(pattern: &str, error: &ast::Error) -> RegexError {
    let start = error.span().start.offset;
    let end = error.span().end.offset;
    RegexError {
        construct: construct_id(pattern, error.kind(), start, end),
        offset: pattern.get(..start).map_or(0, |head| head.chars().count()),
        detail: error.kind().to_string(),
    }
}

/// Map a `regex-syntax` error kind to its stable construct id. The parser
/// reports atomic groups, conditionals, numbered recursion and named
/// backreferences as unrecognized flags or escapes; the id is recovered from
/// the character at the error span (a peek, no parse).
fn construct_id(pattern: &str, kind: &ErrorKind, start: usize, end: usize) -> &'static str {
    let at = pattern.get(start..).and_then(|rest| rest.chars().next());
    match kind {
        ErrorKind::UnsupportedBackreference => "backreference",
        ErrorKind::UnsupportedLookAround => {
            if pattern
                .get(start..end)
                .is_some_and(|token| token.contains('<'))
            {
                "lookbehind"
            } else {
                "lookahead"
            }
        }
        ErrorKind::FlagUnrecognized => match at {
            Some('>') => "atomic_group",
            Some('(') => "conditional",
            Some(digit) if digit.is_ascii_digit() => "recursion",
            Some('P')
                if pattern
                    .get(start + 1..)
                    .is_some_and(|rest| rest.starts_with('=')) =>
            {
                "backreference"
            }
            Some(letter) if letter.is_alphabetic() => "unsupported_flag",
            _ => "syntax",
        },
        ErrorKind::EscapeUnrecognized => {
            let named_backreference = pattern
                .get(start..end)
                .is_some_and(|escape| escape.ends_with('k'))
                && pattern
                    .get(end..)
                    .is_some_and(|rest| rest.starts_with(['<', '{', '\'']));
            if named_backreference {
                "backreference"
            } else {
                "syntax"
            }
        }
        _ => "syntax",
    }
}
