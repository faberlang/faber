//! Half-open byte ranges in original source buffers.
//!
//! Spans are the compiler's source-location currency. They point at original
//! bytes rather than normalized interner strings so diagnostics can quote
//! exactly what the user wrote.

use serde::{Deserialize, Serialize};

/// Half-open byte range in the original source buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Span {
    /// Inclusive byte offset where the range begins.
    pub start: u32,

    /// Exclusive byte offset where the range ends.
    pub end: u32,
}

impl Span {
    /// Create a span from cursor-derived byte offsets.
    #[must_use]
    pub fn new(start: u32, end: u32) -> Self {
        debug_assert!(start <= end, "invalid span: start > end");
        Self { start, end }
    }

    /// Return the smallest span covering both inputs.
    #[must_use]
    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }

    /// Length of the span in bytes.
    #[must_use]
    pub fn len(&self) -> u32 {
        self.end - self.start
    }

    /// Check if the span is empty (zero-length).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}
