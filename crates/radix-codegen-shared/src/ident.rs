//! Shared identifier sanitation and output-resource naming for text emitters.

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
pub fn output_resource_name(param_name: &str) -> &'static str {
    if param_name == "output" {
        "output_buffer"
    } else {
        "output"
    }
}

#[cfg(test)]
#[path = "ident_test.rs"]
mod tests;
