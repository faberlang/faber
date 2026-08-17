//! Pre-lex `+++` TOML frontmatter splitting.
//!
//! GRAMMAR:
//!   fabFile := frontmatter? program
//!   frontmatter := '+++' NEWLINE tomlBody NEWLINE '+++' NEWLINE?
//!
//! Frontmatter is stripped before lexing. The Faber lexer never tokenizes `+++`
//! delimiters; a line that is only `+++` would otherwise become three `Plus`
//! tokens.
//!
//! INVARIANTS
//! ==========
//! - Opening `+++` must be the entire first line (no leading comments or BOM).
//! - When frontmatter is absent, `body` spans the full source and
//!   `body_byte_offset` is `0`.
//! - `body_byte_offset` is a byte index into the original source text.
//! - An empty Faber body after a well-formed frontmatter block is valid.

const DELIMITER: &str = "+++";
const BOM: char = '\u{FEFF}';

/// Result of peeling optional `+++` frontmatter from a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitFrontmatter<'a> {
    /// Raw TOML between delimiters; `None` when the file has no frontmatter block.
    pub frontmatter_text: Option<&'a str>,
    /// Source passed to the Faber lexer (body after the closing delimiter).
    pub body: &'a str,
    /// Byte offset in the original source where `body` begins.
    pub body_byte_offset: u32,
}

/// Errors while splitting frontmatter delimiters (not TOML schema validation).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrontmatterError {
    ByteOrderMark,
    OpeningDelimiterNotFirstLine,
    Unterminated,
}

impl std::fmt::Display for FrontmatterError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ByteOrderMark => "byte order mark is not allowed before frontmatter",
            Self::OpeningDelimiterNotFirstLine => {
                "opening +++ must be on the first line of the file (comments and whitespace before it are not allowed)"
            }
            Self::Unterminated => "unterminated frontmatter",
        })
    }
}

impl std::error::Error for FrontmatterError {}

/// Split optional `+++` TOML frontmatter from UTF-8 source text.
///
/// # Errors
///
/// Returns [`FrontmatterError`] when a byte order mark precedes frontmatter,
/// the opening `+++` is not on the first line, or frontmatter is unterminated.
///
/// # Panics
///
/// Panics if the body byte offset exceeds `u32::MAX` (source files larger than 4 GB).
pub fn split_frontmatter(source: &str) -> Result<SplitFrontmatter<'_>, FrontmatterError> {
    if source.starts_with(BOM) {
        return Err(FrontmatterError::ByteOrderMark);
    }

    let first_line = source.lines().next();
    if first_line != Some(DELIMITER) {
        if let Some(index) = delimiter_line_index(source) {
            if index > 0 {
                return Err(FrontmatterError::OpeningDelimiterNotFirstLine);
            }
        }

        return Ok(SplitFrontmatter { frontmatter_text: None, body: source, body_byte_offset: 0 });
    }

    let after_open = source
        .char_indices()
        .find(|(_, ch)| *ch == '\n')
        .map_or(source.len(), |(index, _)| index + 1);

    let mut cursor = after_open;
    let mut frontmatter_end = after_open;
    let mut body_start = None;

    for line in source[after_open..].split_inclusive('\n') {
        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');
        if trimmed == DELIMITER {
            body_start = Some(cursor + line.len());
            break;
        }
        cursor += line.len();
        frontmatter_end = cursor;
    }

    let body_start = body_start.ok_or(FrontmatterError::Unterminated)?;
    let frontmatter_text = &source[after_open..frontmatter_end];

    Ok(SplitFrontmatter {
        frontmatter_text: Some(frontmatter_text),
        body: &source[body_start..],
        body_byte_offset: u32::try_from(body_start).expect("frontmatter body offset fits u32 (4 GB source limit)"),
    })
}

fn delimiter_line_index(source: &str) -> Option<usize> {
    source
        .lines()
        .enumerate()
        .find_map(|(index, line)| (line.trim_end_matches('\r') == DELIMITER).then_some(index))
}

#[cfg(test)]
#[path = "frontmatter_test.rs"]
mod tests;
