//! `textus` scalar helpers for generated Rust (single-codepoint cords).

const SCALAR_INVARIANT_EMPTY: &str = "scalar textus invariant: empty";
const SCALAR_INVARIANT_MULTI: &str = "scalar textus invariant: multi-scalar";

/// Unicode scalar value for a compile-time-proven single-scalar `textus`/`ascii` cord.
///
/// WHY: chorda interval predicates compare scalar bounds, not lexicographic cords.
/// Typecheck must prove one scalar before this runs.
///
/// # Panics
///
/// Panics if `s` is empty or contains more than one Unicode scalar value.
#[must_use]
pub fn unicode_scalar_value(s: &str) -> u32 {
    let mut chars = s.chars();
    let scalar = chars.next().expect(SCALAR_INVARIANT_EMPTY);
    debug_assert!(chars.next().is_none(), "{SCALAR_INVARIANT_MULTI}");
    scalar as u32
}
