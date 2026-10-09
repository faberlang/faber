//! Faber `regex` runtime carrier.
//!
//! The conversion `textus ↦ regex` is the one place a pattern is compiled or
//! rejected (RD-1, RD-9): [`Regex::new`] validates and compiles once, and every
//! operation on the resulting value is total. A rejected pattern fails with a
//! [`RegexError`] whose leading token is a stable construct id (RD-4), never
//! the engine's prose. The engine is the Rust `regex` crate (RD-8); the ids
//! come from the `regex-syntax` error kinds of the same parser.
//!
//! The operations on a compiled [`Regex`] and on a [`Match`] are the free
//! functions of this module (`faber::regex::find`, `faber::regex::split`, ...):
//! generated code spells them through the canonical runtime-helper table. All
//! offsets are code points, half-open; the byte-to-code-point map is one
//! running count per bulk call.

use std::sync::Arc;

use regex_syntax::ast::{self, Ast, ErrorKind};

/// RE2's repetition count cap; counts above it are `syntax`.
const MAX_REPEAT_COUNT: u32 = 1000;

/// Pattern carrier for Faber `regex`: the pattern text and its compiled
/// program, built once at construction.
#[derive(Clone)]
pub struct Regex {
    pattern: String,
    compiled: regex::Regex,
    /// `(name, group index)` for every named group, shared by the matches.
    names: Arc<[(String, usize)]>,
}

/// One match of a [`Regex`]: the matched text, its half-open code-point span,
/// and the capture groups. The compiler gives the value no source-level type
/// name (RD-3); programs hold it in an inferred binding and read it through
/// [`match_text`], [`match_start`], [`match_end`], [`match_group`] and
/// [`match_named`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Match {
    text: String,
    start: usize,
    end: usize,
    /// Group 0 is the whole match; a group that did not take part is `None`.
    groups: Vec<Option<String>>,
    names: Arc<[(String, usize)]>,
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
        let names = compiled
            .capture_names()
            .enumerate()
            .filter_map(|(index, name)| name.map(|name| (name.to_owned(), index)))
            .collect();
        Ok(Self {
            pattern: pattern.to_owned(),
            compiled,
            names,
        })
    }

    #[must_use]
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// One engine capture set as a [`Match`] whose whole match starts at code
    /// point `start`.
    fn match_value(&self, captures: &regex::Captures<'_>, start: usize) -> Match {
        let whole = captures.get(0).map_or("", |found| found.as_str());
        Match {
            text: whole.to_owned(),
            start,
            end: start + whole.chars().count(),
            groups: captures
                .iter()
                .map(|group| group.map(|found| found.as_str().to_owned()))
                .collect(),
            names: Arc::clone(&self.names),
        }
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

/// The failure text of an unrecovered `textus ↦ regex` conversion: the text
/// `Result::unwrap` printed before generated code routed it through the trap
/// module.
const COMPILE_TRAP: &str = "called `Result::unwrap()` on an `Err` value";

/// The failure text of a regex literal the compiler should have validated.
const LITERAL_TRAP: &str = "regex literal validated";

/// Compile `pattern`, trapping when it is rejected: the unrecovered form of the
/// failable `textus ↦ regex` conversion.
///
/// # Panics
///
/// With the `Result::unwrap` text and the rejection when [`Regex::new`]
/// rejects `pattern`.
#[must_use]
#[track_caller]
pub fn compile_or_trap(pattern: &str) -> Regex {
    crate::trap::ok(Regex::new(pattern), crate::Trap::Other(COMPILE_TRAP))
}

/// Compile a pattern the compiler wrote as a regex literal, trapping when it
/// is rejected. A rejected literal is a compiler defect, not a program error.
///
/// # Panics
///
/// With `regex literal validated` and the rejection when [`Regex::new`]
/// rejects `pattern`.
#[must_use]
#[track_caller]
pub fn literal_or_trap(pattern: &str) -> Regex {
    crate::trap::ok(Regex::new(pattern), crate::Trap::Other(LITERAL_TRAP))
}

/// Whether the pattern matches somewhere in `text`.
///
/// G12 `consentit`: the pattern was validated and compiled at construction,
/// so the verb is total and never recompiles.
#[must_use]
pub fn matches(regex: &Regex, text: &str) -> bool {
    regex.compiled.is_match(text)
}

/// The leftmost-first match in `text`, or `None` when the pattern matches
/// nowhere. Offsets are code points; the byte offsets the engine reports are
/// mapped once, here.
#[must_use]
pub fn find(regex: &Regex, text: &str) -> Option<Match> {
    let captures = regex.compiled.captures(text)?;
    let start = text[..captures.get(0)?.start()].chars().count();
    Some(regex.match_value(&captures, start))
}

/// Every non-overlapping match in `text`, left to right, in one pass.
///
/// The engine's iteration skips an empty match that starts where the previous
/// match ended (RE2's rule), so `a*` over `baaac` yields the spans (0,0) (1,4)
/// (5,5). The byte-to-code-point map is one running count over the haystack:
/// matches arrive in ascending order, so each step counts only the characters
/// since the previous match end.
#[must_use]
pub fn find_all(regex: &Regex, text: &str) -> Vec<Match> {
    let mut found = Vec::new();
    let mut counted_bytes = 0;
    let mut counted_points = 0;
    for captures in regex.compiled.captures_iter(text) {
        let Some(whole) = captures.get(0) else {
            continue;
        };
        let start = counted_points + text[counted_bytes..whole.start()].chars().count();
        let value = regex.match_value(&captures, start);
        counted_points = value.end;
        counted_bytes = whole.end();
        found.push(value);
    }
    found
}

/// The pieces of `text` between the iteration's matches: leading and trailing
/// empty pieces are kept, adjacent separators give an empty piece, and capture
/// groups in the pattern are not spliced into the output.
#[must_use]
pub fn split(regex: &Regex, text: &str) -> Vec<String> {
    regex.compiled.split(text).map(str::to_owned).collect()
}

/// `text` with every non-overlapping match replaced by `replacement`, taken
/// literally (`$1`, `${name}` and `\1` stay text), in one pass over the same
/// iteration as [`find_all`].
#[must_use]
pub fn replace(regex: &Regex, text: &str, replacement: &str) -> String {
    regex
        .compiled
        .replace_all(text, regex::NoExpand(replacement))
        .into_owned()
}

/// `text` with every match of the [`find_all`] iteration replaced by what
/// `replacer` returns for it. Each result is taken literally and is never
/// re-scanned: the output is assembled from the unmatched text and the
/// results only.
#[must_use]
pub fn replace_with<R: AsRef<str>>(
    regex: &Regex,
    text: &str,
    replacer: impl Fn(Match) -> R,
) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut counted_points = 0;
    for captures in regex.compiled.captures_iter(text) {
        let Some(whole) = captures.get(0) else {
            continue;
        };
        let start = counted_points + text[last..whole.start()].chars().count();
        let value = regex.match_value(&captures, start);
        counted_points = value.end;
        out.push_str(&text[last..whole.start()]);
        out.push_str(replacer(value).as_ref());
        last = whole.end();
    }
    out.push_str(&text[last..]);
    out
}

/// A pattern text matching `text` literally: backslash-escapes exactly the
/// metacharacters `\ . + * ? ( ) | [ ] { } ^ $ # & - ~` and leaves every other
/// character (including non-ASCII text) unchanged.
#[must_use]
pub fn escape(text: &str) -> String {
    regex_syntax::escape(text)
}

/// The matched text.
#[must_use]
pub fn match_text(found: &Match) -> String {
    found.text.clone()
}

/// The code-point offset where the match starts.
#[must_use]
pub fn match_start(found: &Match) -> i64 {
    i64::try_from(found.start).unwrap_or(i64::MAX)
}

/// The code-point offset where the match ends (exclusive).
#[must_use]
pub fn match_end(found: &Match) -> i64 {
    i64::try_from(found.end).unwrap_or(i64::MAX)
}

/// The text of capture group `index` (0 is the whole match); `None` when the
/// index is out of range or the group did not take part in the match.
#[must_use]
pub fn match_group(found: &Match, index: i64) -> Option<String> {
    let index = usize::try_from(index).ok()?;
    found.groups.get(index)?.clone()
}

/// The text of the group called `name`; `None` when the name is unknown or the
/// group did not take part in the match.
#[must_use]
pub fn match_named(found: &Match, name: &str) -> Option<String> {
    let (_, index) = found.names.iter().find(|(known, _)| known == name)?;
    found.groups.get(*index)?.clone()
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
