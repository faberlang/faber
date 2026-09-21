//! Shared identifier sanitation and output-resource naming for text emitters.

#[must_use]
pub fn sanitize_ident(raw: &str, fallback: &str) -> String {
    let mut out = String::new();
    for (idx, ch) in raw.chars().enumerate() {
        let valid = ch == '_' || ch.is_ascii_alphanumeric();
        if idx == 0 && ch.is_ascii_digit() {
            out.push('_');
        }
        out.push(if valid { ch } else { '_' });
    }
    if out.is_empty() {
        fallback.to_owned()
    } else {
        out
    }
}

/// Map a filesystem, frontmatter, or import-path segment to a valid Rust
/// module identifier.
///
/// This is the **single module-naming policy** for Rust emission: the package
/// assembler's `pub mod` names (radix-program) and the Rust backend's
/// `use crate::<segment>::…` path lines (radix-hir-rust) both derive through
/// this function, so a use line can neither inject foreign Rust text nor
/// dangle away from the module tree the assembler actually generated (S-B2).
///
/// Policy:
/// - keep ASCII alphanumeric and `_`
/// - every other character (including `-`) becomes `_`
/// - empty / all-separator → `m`
/// - leading digit → prefix `m_`
#[must_use]
pub fn sanitize_rust_module_ident(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len().max(1));
    for c in segment.chars() {
        if c.is_ascii_alphanumeric() || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    let mut ident = if trimmed.is_empty() {
        "m".to_owned()
    } else {
        trimmed.to_owned()
    };
    if ident.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        ident = format!("m_{ident}");
    }
    ident
}

/// Returns the output resource name, avoiding collision with the kernel input
/// parameter when the parameter happens to be named `output`.
#[must_use]
pub fn output_resource_name(param_name: &str) -> &'static str {
    if param_name == "output" {
        "output_buffer"
    } else {
        "output"
    }
}

/// Candidate local names for structured value-join emission, in preference
/// order (register row S06-1: defined once here, consumed by both the Metal
/// and the WGSL structured-CFG emitter).
pub const VALUE_JOIN_CANDIDATES: &[&str] =
    &["faber_joined", "faber_joined_value", "faber_joined_scalar"];

/// Fallback value-join local when every [`VALUE_JOIN_CANDIDATES`] entry is
/// claimed by a kernel binding.
pub const VALUE_JOIN_FALLBACK: &str = "faber_joined_result";

/// Candidate local names for scalar-loop emission, in preference order
/// (register row S06-1: same claim law as [`VALUE_JOIN_CANDIDATES`]).
pub const SCALAR_LOOP_CANDIDATES: &[&str] =
    &["faber_loop", "faber_loop_value", "faber_loop_scalar"];

/// Fallback scalar-loop local when every [`SCALAR_LOOP_CANDIDATES`] entry is
/// claimed by a kernel binding.
pub const SCALAR_LOOP_FALLBACK: &str = "faber_loop_result";

/// The first candidate name not claimed by a kernel binding, so an emitted
/// local never shadows the kernel parameter, output buffer, or index.
///
/// This is the single candidate-claim law for structured-CFG emission
/// (register row S06-1): the Metal `unclaimed_name` and WGSL `unused_ident`
/// copies were byte-identical under two names and are now one function, so
/// the claim rule cannot drift between the device backends.
#[must_use]
pub fn unclaimed_name(
    candidates: &[&'static str],
    param_name: &str,
    output_buffer: &str,
    index_expr: &str,
    fallback: &'static str,
) -> &'static str {
    candidates
        .iter()
        .copied()
        .find(|candidate| {
            *candidate != param_name && *candidate != output_buffer && *candidate != index_expr
        })
        .unwrap_or(fallback)
}

#[cfg(test)]
#[path = "ident_test.rs"]
mod tests;
