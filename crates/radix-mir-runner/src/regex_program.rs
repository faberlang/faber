//! Compiled `regex` carrier for the runner (RX-2b).
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

use crate::value::MatchValue;
use regex_syntax::ast::{self, Ast, ClassSetItem, Flag, FlagsItemKind, LiteralKind, Visitor};

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
        let whole = captures.get(0)?;
        let start = text[..whole.start()].chars().count();
        let end = start + whole.as_str().chars().count();
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
        Some(MatchValue {
            text: whole.as_str().to_owned(),
            start,
            end,
            groups,
            names,
        })
    }
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

#[cfg(test)]
#[path = "regex_program_test.rs"]
mod regex_program_test;
