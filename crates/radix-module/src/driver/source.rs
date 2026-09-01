//! Source text identity and byte-position indexing.
//!
//! Diagnostics are produced by compiler phases in byte offsets, but humans and
//! editors need stable file names, line numbers, and displayable line excerpts.
//! [`SourceFile`] is the driver-side adapter between those two views: it stores
//! source text with its origin and precomputes line starts once so every later
//! diagnostic lookup can be cheap and deterministic.
//!
//! INVARIANTS
//! ==========
//! - `line_starts[0]` is always `0`, even for empty input.
//! - Line starts are byte offsets into `content` (the peeled Faber body), not
//!   `raw_content`.
//! - Displayed line and column numbers are one-based and body-relative.
//! - Inline sources keep an empty path but still carry a user-facing name for
//!   diagnostics, REPLs, and tests.
//!
//! PERFORMANCE
//! ===========
//! Position conversion uses a binary search over precomputed line starts. That
//! keeps repeated diagnostics proportional to `log(lines)` instead of rescanning
//! the full source for every span.

use super::file_frontmatter::{FileFrontmatter, parse_file_frontmatter};
use super::frontmatter::split_frontmatter;
use crate::diagnostics::{Diagnostic, DiagnosticPhase};
use std::path::PathBuf;

/// Errors while constructing a [`SourceFile`] from raw text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceLoadError {
    Frontmatter(String),
    Toml(String),
}

impl std::fmt::Display for SourceLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Frontmatter(message) | Self::Toml(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SourceLoadError {}

/// Peeled Faber body plus parsed frontmatter metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct PeeledSource<'a> {
    pub body: &'a str,
    pub frontmatter: Option<FileFrontmatter>,
    pub body_byte_offset: u32,
}

/// Build the standard frontmatter load diagnostic (`PARSE052`).
#[must_use]
pub fn source_load_diagnostic(file: &str, error: &SourceLoadError) -> Diagnostic {
    Diagnostic::error(error.to_string())
        .with_phase(DiagnosticPhase::Parse)
        .with_code("PARSE052")
        .with_args(vec![
            crate::diagnostics::DiagnosticArg::new("issue", "invalid_frontmatter"),
            crate::diagnostics::DiagnosticArg::new(
                "help",
                "frontmatter must open with +++ on line 1 and contain valid TOML",
            ),
        ])
        .with_file(file.to_owned())
}

/// Peel optional frontmatter from raw source without constructing a [`SourceFile`].
///
/// # Errors
///
/// Returns [`SourceLoadError::Frontmatter`] when frontmatter delimiters are malformed,
/// or [`SourceLoadError::Toml`] when frontmatter TOML content is invalid.
///
/// # Panics
///
/// Panics if the body byte offset exceeds `u32::MAX` (source files larger than 4 GB).
pub fn peel_raw_source<'a>(name: &str, raw: &'a str) -> Result<PeeledSource<'a>, SourceLoadError> {
    let split = split_frontmatter(raw).map_err(|error| SourceLoadError::Frontmatter(format!("{name}: {error}")))?;

    let frontmatter = if let Some(text) = split.frontmatter_text {
        Some(parse_file_frontmatter(text).map_err(|error| SourceLoadError::Toml(format!("{name}: {error}")))?)
    } else {
        None
    };

    Ok(PeeledSource { body: split.body, frontmatter, body_byte_offset: split.body_byte_offset })
}

/// Source text plus the lookup table needed for diagnostic rendering.
#[derive(Debug)]
pub struct SourceFile {
    /// Filesystem origin for disk-backed sources; empty for inline sources.
    pub path: PathBuf,

    /// User-facing name used in diagnostics.
    pub name: String,

    /// Peeled Faber body passed to the lexer and parser.
    pub content: String,

    /// Original source text before optional frontmatter peeling.
    pub raw_content: String,

    /// Parsed frontmatter when a `+++` block was present on line 1.
    pub frontmatter: Option<FileFrontmatter>,

    /// Byte offset in `raw_content` where `content` begins.
    pub body_byte_offset: u32,

    line_starts: Vec<u32>,
}

impl SourceFile {
    /// Build a disk-backed source file from its path and raw contents.
    ///
    /// The display name is derived from the final path component so diagnostics
    /// stay readable even when callers pass absolute paths.
    ///
    /// # Errors
    ///
    /// Returns [`SourceLoadError`] when frontmatter peeling or TOML parsing fails.
    ///
    /// # Panics
    ///
    /// Panics if the source file peak after frontmatter exceeds `u32::MAX` bytes (4 GB+).
    // public constructor: callers choose move vs clone
    pub fn new(path: PathBuf, raw_content: String) -> Result<Self, SourceLoadError> {
        let name = path
            .file_name()
            .map_or_else(|| "<unknown>".to_owned(), |s| s.to_string_lossy().into_owned());

        Self::from_raw(path, name, raw_content)
    }

    /// Build an inline source file with no filesystem path.
    ///
    /// Inline inputs are used by string compilation, tests, and tool surfaces
    /// where diagnostics still need a stable source name.
    ///
    /// # Errors
    ///
    /// Returns [`SourceLoadError`] when frontmatter peeling or TOML parsing fails.
    ///
    /// # Panics
    ///
    /// Panics if the peeled body exceeds `u32::MAX` bytes (4 GB+).
    // public constructor: callers choose move vs clone
    pub fn inline(name: impl Into<String>, raw_content: String) -> Result<Self, SourceLoadError> {
        Self::from_raw(PathBuf::new(), name.into(), raw_content)
    }

    /// Construct a source file from raw text, peeling optional frontmatter first.
    ///
    /// # Errors
    ///
    /// Returns [`SourceLoadError`] when frontmatter peeling or TOML parsing fails.
    ///
    /// # Panics
    ///
    /// Panics if the peeled body exceeds `u32::MAX` bytes (4 GB+).
    // public constructor: callers choose move vs clone
    pub fn from_raw(path: PathBuf, name: String, raw_content: String) -> Result<Self, SourceLoadError> {
        let peeled = peel_raw_source(&name, &raw_content)?;
        let body_byte_offset = peeled.body_byte_offset;
        let frontmatter = peeled.frontmatter;
        let content = peeled.body.to_owned();
        let line_starts = compute_line_starts(&content);

        Ok(Self { path, name, content, raw_content, frontmatter, body_byte_offset, line_starts })
    }

    /// Convert a body-relative byte offset to one-based line and column numbers.
    ///
    /// Offsets beyond the final recorded line start are anchored to the last
    /// known line. The column is byte-based to match compiler spans.
    ///
    /// # Panics
    ///
    /// Panics if the line index exceeds `u32::MAX` (source files larger than 4 GB).
    #[must_use]
    pub fn offset_to_line_col(&self, offset: u32) -> (u32, u32) {
        let line = self
            .line_starts
            .partition_point(|&start| start <= offset)
            .saturating_sub(1);

        let line_start = self.line_starts.get(line).copied().unwrap_or(0);
        let column = offset - line_start;

        (
            u32::try_from(line).expect("line index fits u32 (4 GB source limit)") + 1,
            column + 1,
        )
    }

    /// Return one display line by one-based line number from the peeled body.
    ///
    /// The returned slice trims the trailing line feed so diagnostic renderers
    /// can place carets without carrying source line endings into output.
    ///
    /// # Panics
    ///
    /// Panics if internal line-start offsets are inconsistent with the peeled
    /// body length (an invariant violation; should not occur in normal use).
    #[must_use]
    pub fn line_content(&self, line: u32) -> Option<&str> {
        let line_idx = line.checked_sub(1)? as usize;
        let start = *self.line_starts.get(line_idx)? as usize;
        let end = self
            .line_starts
            .get(line_idx + 1)
            .map_or(self.content.len(), |&e| e as usize);

        Some(self.content[start..end].trim_end_matches('\n'))
    }
}

/// Compute byte offsets of each line start for binary-search lookup.
///
/// EDGE: `\r\n` input records the byte after `\n`; any preceding `\r` remains
/// part of the line slice because display trimming only removes the line feed.
///
/// # Panics
///
/// Panics if a line start offset exceeds `u32::MAX` (source files larger than 4 GB).
fn compute_line_starts(content: &str) -> Vec<u32> {
    let mut starts = vec![0];

    for (i, c) in content.char_indices() {
        if c == '\n' {
            starts.push(u32::try_from(i + 1).expect("line start offset fits u32 (4 GB source limit)"));
        }
    }

    starts
}

#[cfg(test)]
#[path = "source_test.rs"]
mod tests;
