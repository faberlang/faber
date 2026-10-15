//! Compiled `regex` carrier for the CLIF runtime (a copy of the MIR runner's
//! `regex_program.rs`, RX-2b: the runtime cannot depend on the runner, and the
//! two must agree pattern for pattern, so a change to one is a change to both).
//!
//! The runner's original header follows.
//!
//!
//! `textus ↦ regex` validates and compiles the pattern once, in the
//! conversion. The carrier then holds the compiled program, so every operation
//! on the value is total and never recompiles (goal: the conversion is the one
//! place a pattern is compiled or rejected).
//!
//! The dialect is RE2 (`docs/design/regex-dialect.md`). The native `regex`
//! crate is the engine; the construct ids below are recovered from the engine's
//! own AST parser (`regex_syntax`), never from a Faber-owned parser. A rejected
//! pattern's payload starts with a stable construct id (RD-4), followed by
//! `: ` and detail text. Callers and tests rely on the id, never the detail.

use regex_syntax::ast::{self, Ast, ClassSetItem, Flag, FlagsItemKind, LiteralKind, Visitor};

/// One match: its text, code-point offsets, numbered groups and group names.
#[derive(Debug, Clone, PartialEq)]
pub struct MatchValue {
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub groups: Vec<Option<String>>,
    pub names: Vec<(String, usize)>,
}

impl MatchValue {
    /// Text of group `index`; `None` when out of range or not participating.
    #[must_use]
    pub fn group(&self, index: usize) -> Option<&str> {
        self.groups.get(index)?.as_deref()
    }

    /// Text of the group named `name`; `None` when unknown or not participating.
    #[must_use]
    pub fn named(&self, name: &str) -> Option<&str> {
        let (_, index) = self.names.iter().find(|(known, _)| known == name)?;
        self.group(*index)
    }
}

/// RE2's repetition count cap; counts above it are `syntax`.
const MAX_REPEAT_COUNT: u32 = 1000;

/// A validated, compiled regex program plus the pattern text it came from.
#[derive(Debug)]
pub struct RegexProgram {
    pattern: String,
    compiled: regex::Regex,
}

impl PartialEq for RegexProgram {
    fn eq(&self, other: &Self) -> bool {
        self.pattern == other.pattern
    }
}

impl RegexProgram {
    /// Validate and compile `pattern`. The error is the RD-4 payload: a stable
    /// construct id, then `: ` and engine detail.
    pub fn compile(pattern: &str) -> Result<Self, String> {
        validate(pattern).map_err(|reject| reject.payload())?;
        let compiled = regex::RegexBuilder::new(pattern)
            .build()
            .map_err(|err| Reject::syntax(compile_detail(&err.to_string())).payload())?;
        Ok(Self {
            pattern: pattern.to_owned(),
            compiled,
        })
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn is_match(&self, text: &str) -> bool {
        self.compiled.is_match(text)
    }

    /// The leftmost-first match in `text`, with offsets in code points. The
    /// byte offsets the engine reports are mapped once, here.
    pub fn find(&self, text: &str) -> Option<MatchValue> {
        let captures = self.compiled.captures(text)?;
        let start = text[..captures.get(0)?.start()].chars().count();
        Some(self.match_value(&captures, start))
    }

    /// Every non-overlapping match in `text`, left to right, in one pass. The
    /// engine's iteration skips an empty match that starts where the previous
    /// match ended (RE2's rule, `PIN-EMPTY`), so `a*` over `baaac` yields the
    /// spans (0,0) (1,4) (5,5). The byte-to-code-point map is one running
    /// count over the haystack: matches arrive in ascending order, so each
    /// step counts only the characters since the previous match end.
    pub fn find_all(&self, text: &str) -> Vec<MatchValue> {
        let mut found = Vec::new();
        let mut counted_bytes = 0;
        let mut counted_points = 0;
        for captures in self.compiled.captures_iter(text) {
            let Some(whole) = captures.get(0) else {
                continue;
            };
            let start = counted_points + text[counted_bytes..whole.start()].chars().count();
            let value = self.match_value(&captures, start);
            counted_points = value.end;
            counted_bytes = whole.end();
            found.push(value);
        }
        found
    }

    /// The pieces of `text` between the iteration's matches: leading and
    /// trailing empty pieces are kept, adjacent separators give an empty piece,
    /// and capture groups in the pattern are not spliced into the output
    /// (`PIN-SPLIT`). Byte slices only: no code-point map is needed.
    pub fn split(&self, text: &str) -> Vec<String> {
        self.compiled.split(text).map(str::to_owned).collect()
    }

    /// `text` with every non-overlapping match replaced by `replacement`, taken
    /// literally (`$1`, `${name}` and `\1` stay text), in one pass over the
    /// engine's iteration (the same empty-match rule as `find_all`). Byte
    /// slices only: no code-point map is needed.
    pub fn replace_all_literal(&self, text: &str, replacement: &str) -> String {
        self.compiled
            .replace_all(text, regex::NoExpand(replacement))
            .into_owned()
    }

    /// `text` with the i-th match of the iteration replaced by `replacements[i]`
    /// (the closure form of `muta`, RX-5). Each replacement is taken literally
    /// and is never re-scanned: the output is assembled from the unmatched text
    /// and the replacements only. `None` when the counts disagree, which the
    /// lowering (one replacement per match of the same iteration) never
    /// produces.
    pub fn replace_each(&self, text: &str, replacements: &[String]) -> Option<String> {
        let mut out = String::with_capacity(text.len());
        let mut last = 0;
        let mut taken = replacements.iter();
        for found in self.compiled.find_iter(text) {
            out.push_str(&text[last..found.start()]);
            out.push_str(taken.next()?);
            last = found.end();
        }
        if taken.next().is_some() {
            return None;
        }
        out.push_str(&text[last..]);
        Some(out)
    }

    /// One engine capture set as a `MatchValue` whose whole match starts at
    /// code point `start`.
    fn match_value(&self, captures: &regex::Captures<'_>, start: usize) -> MatchValue {
        let whole = captures.get(0).map_or("", |found| found.as_str());
        let groups = captures
            .iter()
            .map(|group| group.map(|found| found.as_str().to_owned()))
            .collect();
        let names = self
            .compiled
            .capture_names()
            .enumerate()
            .filter_map(|(index, name)| name.map(|name| (name.to_owned(), index)))
            .collect();
        MatchValue {
            text: whole.to_owned(),
            start,
            end: start + whole.chars().count(),
            groups,
            names,
        }
    }
}

/// A pattern text matching `text` literally: the engine's own escaper, which
/// backslash-escapes exactly the metacharacters `\ . + * ? ( ) | [ ] { } ^ $ # & - ~`
/// and leaves every other character (including non-ASCII text) unchanged.
pub fn escape(text: &str) -> String {
    regex_syntax::escape(text)
}

/// A rejected construct: a stable id (RD-4) and engine detail.
struct Reject {
    id: &'static str,
    detail: String,
}

impl Reject {
    fn new(id: &'static str, detail: impl Into<String>) -> Self {
        Self {
            id,
            detail: detail.into(),
        }
    }

    fn syntax(detail: impl Into<String>) -> Self {
        Self::new("syntax", detail)
    }

    fn payload(&self) -> String {
        format!("{}: {}", self.id, self.detail)
    }
}

/// The last `error: …` line of the engine's multi-line message, or the whole
/// message when it has none.
fn compile_detail(message: &str) -> String {
    message
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("error: "))
        .unwrap_or(message)
        .to_owned()
}

fn validate(pattern: &str) -> Result<(), Reject> {
    let parsed = ast::parse::Parser::new()
        .parse(pattern)
        .map_err(|err| classify_parse_error(pattern, &err))?;
    ast::visit(&parsed, DialectVisitor)
}

/// Map a `regex_syntax` AST parse error to a construct id. Lookaround and
/// backreferences have their own error kinds; atomic groups, conditionals,
/// recursion and named backreferences surface as `FlagUnrecognized` or
/// `EscapeUnrecognized`, so the id comes from the character at the error span.
fn classify_parse_error(pattern: &str, err: &ast::Error) -> Reject {
    let detail = err.kind().to_string();
    let offset = err.span().start.offset;
    let rest = pattern.get(offset..).unwrap_or("");
    let at = rest.chars().next();
    let id = match err.kind() {
        ast::ErrorKind::UnsupportedBackreference => "backreference",
        ast::ErrorKind::UnsupportedLookAround => {
            if rest.starts_with("(?<") {
                "lookbehind"
            } else {
                "lookahead"
            }
        }
        ast::ErrorKind::EscapeUnrecognized if rest.starts_with("\\k") => "backreference",
        ast::ErrorKind::FlagUnrecognized if rest.starts_with("P=") => "backreference",
        ast::ErrorKind::FlagUnrecognized if rest.starts_with("P>") => "recursion",
        ast::ErrorKind::FlagUnrecognized => match at {
            Some('>') => "atomic_group",
            Some('(') => "conditional",
            Some(c) if c.is_ascii_digit() || c == '&' || c == '+' => "recursion",
            Some(c) if c.is_ascii_alphabetic() => "unsupported_flag",
            _ => "syntax",
        },
        _ => "syntax",
    };
    Reject::new(id, detail)
}

/// Rejects the constructs the AST accepts but the dialect does not.
struct DialectVisitor;

impl DialectVisitor {
    fn check_flags(flags: &ast::Flags) -> Result<(), Reject> {
        for item in &flags.items {
            if let FlagsItemKind::Flag(flag) = &item.kind {
                match flag {
                    Flag::CaseInsensitive
                    | Flag::MultiLine
                    | Flag::DotMatchesNewLine
                    | Flag::IgnoreWhitespace => {}
                    // `R` is a CRLF flag letter to the Rust parser and the
                    // recursion spelling `(?R)` to the dialect.
                    Flag::CRLF => {
                        return Err(Reject::new("recursion", "recursion is not supported"));
                    }
                    Flag::SwapGreed | Flag::Unicode => {
                        return Err(Reject::new(
                            "unsupported_flag",
                            format!("flag {flag:?} is not supported"),
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    fn check_literal(kind: &LiteralKind) -> Result<(), Reject> {
        match kind {
            LiteralKind::HexFixed(
                ast::HexLiteralKind::UnicodeShort | ast::HexLiteralKind::UnicodeLong,
            )
            | LiteralKind::HexBrace(
                ast::HexLiteralKind::UnicodeShort | ast::HexLiteralKind::UnicodeLong,
            ) => Err(Reject::syntax("\\u and \\U escapes are not supported")),
            _ => Ok(()),
        }
    }

    fn check_repetition(rep: &ast::Repetition) -> Result<(), Reject> {
        if matches!(&*rep.ast, Ast::Repetition(_)) {
            let possessive = matches!(rep.op.kind, ast::RepetitionKind::OneOrMore) && rep.greedy;
            return Err(if possessive {
                Reject::new("possessive", "possessive quantifiers are not supported")
            } else {
                Reject::syntax("repetition of a repetition")
            });
        }
        let over = match &rep.op.kind {
            ast::RepetitionKind::Range(
                ast::RepetitionRange::Exactly(n) | ast::RepetitionRange::AtLeast(n),
            ) => *n > MAX_REPEAT_COUNT,
            ast::RepetitionKind::Range(ast::RepetitionRange::Bounded(m, n)) => {
                *m > MAX_REPEAT_COUNT || *n > MAX_REPEAT_COUNT
            }
            _ => false,
        };
        if over {
            return Err(Reject::syntax("repetition count exceeds 1000"));
        }
        Ok(())
    }
}

impl Visitor for DialectVisitor {
    type Output = ();
    type Err = Reject;

    fn finish(self) -> Result<(), Reject> {
        Ok(())
    }

    fn visit_pre(&mut self, node: &Ast) -> Result<(), Reject> {
        match node {
            Ast::Flags(set) => Self::check_flags(&set.flags),
            Ast::Group(group) => match &group.kind {
                ast::GroupKind::NonCapturing(flags) => Self::check_flags(flags),
                _ => Ok(()),
            },
            Ast::Repetition(rep) => Self::check_repetition(rep),
            Ast::Literal(lit) => Self::check_literal(&lit.kind),
            Ast::Assertion(assertion) => match assertion.kind {
                ast::AssertionKind::StartLine
                | ast::AssertionKind::EndLine
                | ast::AssertionKind::StartText
                | ast::AssertionKind::EndText
                | ast::AssertionKind::WordBoundary
                | ast::AssertionKind::NotWordBoundary => Ok(()),
                _ => Err(Reject::syntax("word boundary variant is not supported")),
            },
            _ => Ok(()),
        }
    }

    fn visit_class_set_item_pre(&mut self, item: &ClassSetItem) -> Result<(), Reject> {
        match item {
            ClassSetItem::Bracketed(_) => Err(Reject::syntax("nested character class")),
            ClassSetItem::Literal(lit) => Self::check_literal(&lit.kind),
            _ => Ok(()),
        }
    }

    fn visit_class_set_binary_op_pre(&mut self, _op: &ast::ClassSetBinaryOp) -> Result<(), Reject> {
        Err(Reject::syntax("character class set operation"))
    }
}
