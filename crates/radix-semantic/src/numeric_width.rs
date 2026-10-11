//! Re-exports numeric-width types and helpers for the semantic module's public API.
//!
//! This file is the re-export boundary for `NumericWidth`, `WidthError`, and
//! width-marker sugar helpers. The canonical source is `radix-types::numeric_width`.

pub use radix_types::numeric_width::*;

/// Rounding hints of a float or decimal to integer conversion:
/// `x ↦ int via Floor|Ceil|HalfEven|Trunc`.
///
/// Distinct from [`is_conversio_radix_hint`]: these name which integer a
/// fractional value becomes, and only a float or decimal source into an
/// integer-family target takes one (the checker owns the source rule).
#[must_use]
pub fn is_float_rounding_hint(marker: &str) -> bool {
    matches!(marker, "Floor" | "Ceil" | "HalfEven" | "Trunc")
}
