//! Opt-in line wrapping for already-emitted canonical Faber source.
//!
//! The canonical emitter keeps no width limit and this module never changes
//! that: [`wrap_faber_source`] is a separate text pass a caller applies on
//! request (`faber format --width N`). It lexes the emitted text and, where a
//! line is wider than the requested display width, replaces an existing
//! inter-token space with a newline plus continuation indent. It never splits
//! a token, never inserts or removes a token, and never moves a comment.
//!
//! Faber treats spaces, tabs and newlines as trivia except where a production
//! names `NEWLINE` or the parser asks whether a word is on the same line. A
//! break is therefore refused where moving a token to a new line would change
//! what the parser reads:
//!
//! - lines beginning with `@` (unbraced annotation sugar ends at `NEWLINE`);
//! - lines carrying a `columna` (schema columns are `NEWLINE`-separated);
//! - before `si` (the `iace x si cond` / `mori x si cond` guard word must stay
//!   on the value's line), before `ex` (copy-with-changes `} ex source` stays
//!   on the closing brace's line; a line-leading `ex` is the extraction
//!   statement) and before or after `via` (the conversion hint is same-line);
//! - after `redde` (the returned value starts on the `redde` line);
//! - after a comma that separates statement-level lists, which a newline would
//!   end (`scribe a, b`, `ex p fixum a, b`), and after commas outside every
//!   bracket;
//! - inside the first three tokens of a source line, so a declaration head
//!   (`sit T name`, `nexum T name`) is never separated;
//! - before a regex literal body (`regex /p/` reads the body only directly
//!   after the type word or `_`);
//! - before closing brackets and braces and before an opening brace.
//!
//! Frontmatter is not part of the body this pass sees. A token, a protected
//! span or a line made only of refused positions that is wider than the width
//! stays whole and overflows; the width is a goal, not a guarantee.
//!
//! Width is counted in Unicode display columns, indentation included.

use radix_lexer::{
    KeywordPack, KeywordSurfaceMode, LexResult, Token, TokenKind, keyword_spelling_for_token, lex,
    lex_with_locale_pack,
};
use unicode_width::UnicodeWidthStr;

/// Spaces added to a line's own indentation on a continuation line.
const CONTINUATION_INDENT: usize = 4;

/// How many leading tokens of a source line stay together.
const HEAD_TOKENS: usize = 3;

/// Wrap `source` (a canonical Faber body, no frontmatter) to `width` display
/// columns. `pack` is the locale pack the text is written in; `None` reads the
/// canonical Latin surface.
///
/// Returns `source` unchanged when it does not lex cleanly or `width` is zero.
#[must_use]
pub fn wrap_faber_source(source: &str, width: usize, pack: Option<&dyn KeywordPack>) -> String {
    if width == 0 {
        return source.to_owned();
    }
    let lexed = match pack {
        Some(pack) => lex_with_locale_pack(source, pack),
        None => lex(source),
    };
    if !lexed.success() {
        return source.to_owned();
    }
    Wrapper::new(source, &lexed, width).run()
}

/// One token with the whitespace written before it on its line.
struct Piece<'a> {
    /// Whitespace between the previous token (or line start) and this token.
    gap: &'a str,
    /// The token's exact source text.
    text: &'a str,
    /// Whether the gap before this piece may become a line break.
    breakable: bool,
    /// Whether the previous token is a comma (the preferred place to break).
    after_comma: bool,
}

struct Wrapper<'a> {
    source: &'a str,
    lexed: &'a LexResult,
    width: usize,
    out: String,
}

/// Bracket and statement-list state carried across source lines.
#[derive(Default)]
struct Nesting {
    /// Open brackets and braces.
    depth: usize,
    /// Bracket depth at which a statement-level comma list is open.
    list_depth: Option<usize>,
}

impl<'a> Wrapper<'a> {
    fn new(source: &'a str, lexed: &'a LexResult, width: usize) -> Self {
        Self {
            source,
            lexed,
            width,
            out: String::with_capacity(source.len() + source.len() / 8),
        }
    }

    fn run(mut self) -> String {
        let tokens: Vec<&Token> = self
            .lexed
            .tokens
            .iter()
            .filter(|token| token.kind != TokenKind::Eof)
            .collect();
        let mut nesting = Nesting::default();
        let mut cursor = 0usize;
        let mut index = 0usize;
        while index < tokens.len() {
            // One source line: tokens up to the next `Newline` token.
            let line_end = tokens[index..]
                .iter()
                .position(|token| token.kind == TokenKind::Newline)
                .map_or(tokens.len(), |offset| index + offset);
            let line = &tokens[index..line_end];
            // Text between the previous line's end and this line's first token
            // is the line break itself plus any blank lines, copied verbatim.
            if let Some(first) = line.first() {
                // Everything before the line's own indentation is copied as is.
                let first_start = first.span.start as usize;
                let line_start = self.source[..first_start]
                    .rfind('\n')
                    .map_or(0, |newline| newline + 1)
                    .max(cursor);
                self.out.push_str(&self.source[cursor..line_start]);
                self.wrap_line(line, &mut nesting);
                cursor = line.last().map_or(cursor, |last| last.span.end as usize);
            }
            index = line_end + 1;
        }
        self.out.push_str(&self.source[cursor..]);
        self.out
    }

    /// The Latin keyword identity of a token, mirroring the parser's lookup.
    fn latin_key(&self, token: &Token) -> Option<String> {
        match token.kind {
            TokenKind::Ident(symbol) => {
                let text = self.lexed.interner.resolve(symbol);
                if let Some(key) = self.lexed.reader_keyword_latin_keys.get(text) {
                    return Some(key.clone());
                }
                match self.lexed.keyword_surface {
                    KeywordSurfaceMode::RegistryDirect => Some(text.to_owned()),
                    KeywordSurfaceMode::PackAuthoritative => None,
                }
            }
            ref kind => keyword_spelling_for_token(kind).map(str::to_owned),
        }
    }

    /// Classify one source line's tokens into pieces with their break rules.
    fn pieces(&self, line: &[&Token], nesting: &mut Nesting) -> Vec<Piece<'a>> {
        let source: &'a str = self.source;
        let keys: Vec<Option<String>> = line.iter().map(|token| self.latin_key(token)).collect();
        let key_at = |i: usize| keys[i].as_deref();
        // Whole-line refusals.
        let annotation_line = line.first().is_some_and(|t| t.kind == TokenKind::At);
        let schema_line = keys.iter().any(|key| key.as_deref() == Some("columna"));
        // A statement-level list never spans a source line.
        nesting.list_depth = None;

        let mut pieces = Vec::with_capacity(line.len());
        // The first piece's gap is the line's indentation.
        let mut previous_end = line.first().map_or(0, |token| {
            let first = token.span.start as usize;
            source[..first].rfind('\n').map_or(0, |newline| newline + 1)
        });
        for (i, token) in line.iter().enumerate() {
            let start = token.span.start as usize;
            let end = token.span.end as usize;
            let gap = &source[previous_end.min(start)..start];
            let previous = i.checked_sub(1);

            let mut breakable = i >= HEAD_TOKENS
                && !gap.is_empty()
                && gap.chars().all(|c| c == ' ' || c == '\t')
                && !annotation_line
                && !schema_line;
            if breakable {
                let here = key_at(i);
                breakable = !matches!(here, Some("si" | "ex" | "via"))
                    && !matches!(
                        token.kind,
                        TokenKind::RegexString(_)
                            | TokenKind::LBrace
                            | TokenKind::RBrace
                            | TokenKind::RParen
                            | TokenKind::RBracket
                    );
            }
            if breakable && let Some(p) = previous {
                breakable = !matches!(key_at(p), Some("redde" | "via"));
                if breakable && line[p].kind == TokenKind::Comma {
                    // Outside every bracket a comma belongs to a declaration
                    // head or a statement-level list, which a newline ends.
                    breakable = nesting.depth > 0 && nesting.list_depth != Some(nesting.depth);
                }
            }

            // Advance bracket and list state past this token.
            match token.kind {
                TokenKind::LParen
                | TokenKind::LBracket
                | TokenKind::LBrace
                | TokenKind::QuestionBracket
                | TokenKind::QuestionParen
                | TokenKind::BangBracket
                | TokenKind::BangParen => nesting.depth += 1,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    nesting.depth = nesting.depth.saturating_sub(1);
                    if nesting
                        .list_depth
                        .is_some_and(|depth| nesting.depth < depth)
                    {
                        nesting.list_depth = None;
                    }
                }
                _ => {}
            }
            if matches!(key_at(i), Some("scribe" | "vide" | "mone" | "nota" | "ex")) {
                nesting.list_depth = Some(nesting.depth);
            }

            pieces.push(Piece {
                gap,
                text: &source[start..end],
                breakable,
                after_comma: previous.is_some_and(|p| line[p].kind == TokenKind::Comma),
            });
            previous_end = end;
        }
        pieces
    }

    fn wrap_line(&mut self, line: &[&Token], nesting: &mut Nesting) {
        let pieces = self.pieces(line, nesting);
        // A token spanning lines (a multi-line string) keeps its line whole.
        if pieces.iter().any(|piece| piece.text.contains('\n')) {
            for piece in &pieces {
                self.out.push_str(piece.gap);
                self.out.push_str(piece.text);
            }
            return;
        }
        let indent = pieces.first().map_or("", |piece| piece.gap);
        let continuation = format!("{indent}{}", " ".repeat(CONTINUATION_INDENT));
        let width_of = |row: &[usize], at: usize| -> usize {
            let lead = if at == 0 {
                indent
            } else {
                continuation.as_str()
            };
            let mut width = display_width(lead);
            for (k, &p) in row.iter().enumerate() {
                if k > 0 {
                    width += display_width(pieces[p].gap);
                }
                width += display_width(pieces[p].text);
            }
            width
        };

        let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
        for i in 0..pieces.len() {
            let current = rows.len() - 1;
            rows[current].push(i);
            while width_of(&rows[rows.len() - 1], rows.len() - 1) > self.width {
                let at = rows.len() - 1;
                let last = &mut rows[at];
                let candidates: Vec<usize> = (1..last.len())
                    .filter(|&k| pieces[last[k]].breakable)
                    .collect();
                // Prefer the last break after a comma, unless it would leave
                // less than half the width on the line; else the last break.
                let split = candidates
                    .iter()
                    .rev()
                    .find(|&&k| {
                        pieces[last[k]].after_comma && width_of(&last[..k], at) * 2 >= self.width
                    })
                    .or(candidates.last())
                    .copied();
                let Some(split) = split else {
                    break;
                };
                let tail = last.split_off(split);
                rows.push(tail);
            }
        }

        for (at, row) in rows.iter().enumerate() {
            if at > 0 {
                self.out.push('\n');
                self.out.push_str(&continuation);
            }
            for (k, &p) in row.iter().enumerate() {
                if at == 0 || k > 0 {
                    self.out.push_str(pieces[p].gap);
                }
                self.out.push_str(pieces[p].text);
            }
        }
    }
}

fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}
