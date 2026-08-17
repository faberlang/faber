//! Shared layout core for the `pretty-v1` policy (FORMAT-PRETTY S3).
//!
//! This module owns the pretty layout constants and the two primitive services
//! every pretty rule builds on:
//!
//! - [`PrettyWriter`] — a column-aware text writer with indentation and trial
//!   modes for width measurement. Indent is a fixed 4-space constant and the
//!   soft width is a fixed 100-column constant; neither is configurable in v1
//!   (delivery decision (b): implementation constants only).
//! - [`LineIndex`] — a source line table used to detect whether a syntax span
//!   crossed lines in the author's source, the preservation signal behind
//!   "already-multiline calls/records are not collapsed".
//!
//! The author (AST+trivia) path is the primary consumer. The HIR-backed
//! `--locale` re-emit path (`radix-hir-faber`) lives in a foundation leaf that
//! cannot depend on the monolith, so it cannot share this module in v1; the
//! gap is recorded in the `forma::pretty` module doc.

/// Indentation width in spaces. Structural and immutable in v1.
pub const INDENT_WIDTH: usize = 4;

/// Soft line-width trigger in columns. A trigger, not a promise: unbreakable
/// lines (strings, URLs, identifiers, precedence-obscuring operators) stay
/// long. Structural and immutable in v1.
pub const SOFT_WIDTH: usize = 100;

/// Which trial policy a [`PrettyWriter`] is running under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TrialMode {
    /// Real emission; writes go to the final buffer.
    None,
    /// Inline trial: any line break invalidates the trial (an inline form may
    /// never contain a newline). Nested lists that need expansion must fail
    /// the trial rather than expand.
    Inline,
    /// First-line trial: rendering stops recording at the first line break so
    /// the caller can measure the width of the first line of a (possibly
    /// multiline) rendering. Nested expansion is allowed.
    FirstLine,
}

/// Column-aware code writer for pretty layout.
///
/// Mirrors [`crate::codegen::CodeWriter`]'s indentation mechanics but tracks
/// the current visual column and supports trial modes, where rendering happens
/// into a scratch buffer so the caller can measure a form before committing to
/// it. Trial writers are cheap; they are created per layout decision and
/// discarded when the decision is "expand".
pub struct PrettyWriter {
    buffer: String,
    depth: usize,
    at_line_start: bool,
    col: usize,
    mode: TrialMode,
    /// Whether an inline trial was invalidated by a line break or by a nested
    /// construct declaring itself un-inlineable.
    failed: bool,
    /// First-line trial: whether recording stopped at the first line break.
    stopped: bool,
}

impl PrettyWriter {
    /// Create an empty writer at the top of a fresh line.
    #[must_use]
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            depth: 0,
            at_line_start: true,
            col: 0,
            mode: TrialMode::None,
            failed: false,
            stopped: false,
        }
    }

    /// Create an inline-trial writer that simulates starting mid-line at `col`.
    ///
    /// The writer has no indentation and is not at a line start; rendering
    /// into it measures an inline form starting at the given column. The trial
    /// fails (see [`PrettyWriter::is_failed`]) when any newline is written or
    /// a nested construct needs expansion.
    #[must_use]
    pub fn trial_at(col: usize) -> Self {
        Self {
            buffer: String::new(),
            depth: 0,
            at_line_start: false,
            col,
            mode: TrialMode::Inline,
            failed: false,
            stopped: false,
        }
    }

    /// Create a first-line-trial writer starting mid-line at `col`.
    ///
    /// Rendering stops recording at the first line break; the caller can then
    /// read [`PrettyWriter::column`] as the width of the first line of a
    /// possibly-multiline form. Never fails on newlines.
    #[must_use]
    pub fn first_line_trial_at(col: usize) -> Self {
        Self {
            buffer: String::new(),
            depth: 0,
            at_line_start: false,
            col,
            mode: TrialMode::FirstLine,
            failed: false,
            stopped: false,
        }
    }

    /// Whether this writer is a trial (scratch measurement) writer.
    #[must_use]
    pub fn is_trial(&self) -> bool {
        self.mode != TrialMode::None
    }

    /// Whether this writer is an inline trial (newlines forbidden).
    #[must_use]
    pub fn is_inline_trial(&self) -> bool {
        self.mode == TrialMode::Inline
    }

    /// Whether an inline trial was invalidated.
    #[must_use]
    pub fn is_failed(&self) -> bool {
        self.failed
    }

    /// Mark an inline trial failed (used by nested constructs that cannot
    /// render inline within a trial).
    pub fn fail(&mut self) {
        self.failed = true;
    }

    /// Current visual column on the current line (indent included).
    ///
    /// For a first-line trial this is the width of the first line.
    #[must_use]
    pub fn column(&self) -> usize {
        self.col
    }

    /// Finish emission and return the accumulated source text.
    #[must_use]
    pub fn finish(self) -> String {
        self.buffer
    }

    fn write_char(&mut self, c: char) {
        if c == '\n' {
            match self.mode {
                TrialMode::None => {
                    self.buffer.push('\n');
                    self.at_line_start = true;
                    self.col = 0;
                }
                TrialMode::Inline => {
                    self.failed = true;
                    self.buffer.push('\n');
                    self.at_line_start = true;
                    self.col = 0;
                }
                TrialMode::FirstLine => {
                    // Stop recording; keep col as the first-line width.
                    self.stopped = true;
                }
            }
        } else {
            if self.mode == TrialMode::FirstLine && self.stopped {
                return;
            }
            if self.at_line_start && self.mode != TrialMode::FirstLine {
                let width = self.depth * INDENT_WIDTH;
                for _ in 0..width {
                    self.buffer.push(' ');
                }
                self.col = width;
                self.at_line_start = false;
            } else if self.at_line_start {
                // First-line trial mid-line start: never a real line start.
                self.at_line_start = false;
            }
            self.buffer.push(c);
            self.col += 1;
        }
    }

    /// Write text verbatim without applying block indentation on embedded newlines.
    ///
    /// Use for literal payloads (`«…»`, `` `…` ``) whose interior newlines must
    /// not pick up the surrounding block indent level. In an inline trial an
    /// embedded newline fails the trial (a multiline literal cannot be part of
    /// an inline form).
    pub fn write_raw(&mut self, s: &str) {
        for c in s.chars() {
            if c == '\n' {
                match self.mode {
                    TrialMode::None => {
                        self.buffer.push('\n');
                        self.at_line_start = true;
                        self.col = 0;
                    }
                    TrialMode::Inline => {
                        self.failed = true;
                        self.buffer.push('\n');
                        self.at_line_start = true;
                        self.col = 0;
                    }
                    TrialMode::FirstLine => {
                        self.stopped = true;
                    }
                }
            } else {
                if self.mode == TrialMode::FirstLine && self.stopped {
                    return;
                }
                self.buffer.push(c);
                self.col += 1;
                self.at_line_start = false;
            }
        }
    }

    /// Write text while applying indentation at each line start.
    pub fn write(&mut self, s: &str) {
        for c in s.chars() {
            self.write_char(c);
        }
    }

    /// Write a line (text + newline).
    pub fn writeln(&mut self, s: &str) {
        self.write(s);
        self.write("\n");
    }

    /// Write an empty line.
    pub fn newline(&mut self) {
        self.write("\n");
    }

    /// Increase indentation for subsequent lines.
    pub fn indent(&mut self) {
        self.depth += 1;
    }

    /// Decrease indentation for subsequent lines (saturating).
    pub fn dedent(&mut self) {
        self.depth = self.depth.saturating_sub(1);
    }

    /// Run a closure with one extra indentation level.
    pub fn indented<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.indent();
        f(self);
        self.dedent();
    }
}

impl Default for PrettyWriter {
    fn default() -> Self {
        Self::new()
    }
}

/// Line table over a peeled source body, used for span line-sensitivity checks.
pub struct LineIndex {
    /// Byte offset of each line start; `starts[0] == 0` always.
    starts: Vec<u32>,
}

impl LineIndex {
    /// Build the line table for a source body.
    #[must_use]
    pub fn new(source: &str) -> Self {
        let mut starts = vec![0u32];
        for (i, c) in source.char_indices() {
            if c == '\n' {
                starts.push(u32::try_from(i + 1).unwrap_or(u32::MAX));
            }
        }
        Self { starts }
    }

    /// Whether a span crosses a line boundary in the source.
    ///
    /// Spans are half-open byte ranges; a newline inside `[start, end)` makes
    /// the construct source-multiline, which the pretty engine preserves
    /// rather than collapsing.
    #[must_use]
    pub fn is_multiline(&self, span: crate::lexer::Span) -> bool {
        if span.is_empty() {
            return false;
        }
        let idx = self.starts.partition_point(|&start| start <= span.start);
        // The line containing `span.start` ends at `next_start - 1` (its
        // terminating newline). The span is source-multiline when that
        // newline lies inside `[start, end)`, i.e. `next_start <= span.end`.
        self.starts
            .get(idx)
            .is_some_and(|&next_start| next_start <= span.end)
    }

    /// One-based line number containing a byte offset.
    #[must_use]
    pub fn line_of(&self, offset: u32) -> u32 {
        let line = self.starts.partition_point(|&start| start <= offset);
        u32::try_from(line).unwrap_or(1).max(1)
    }
}

#[cfg(test)]
#[path = "layout_test.rs"]
mod tests;
