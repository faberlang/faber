//! Filesystem/frontmatter module-segment derivation for package sources.

use std::path::{Component, Path};

use radix_module::driver::FileFrontmatter;

pub fn module_segments_for_file(
    source_root: &Path,
    file: &Path,
    frontmatter: Option<&FileFrontmatter>,
) -> Vec<String> {
    if let Some(group) = frontmatter.and_then(|fm| fm.group()) {
        let segments = group
            .split('.')
            .filter(|segment| !segment.is_empty())
            .map(sanitize_rust_module_ident)
            .collect::<Vec<_>>();
        if !segments.is_empty() {
            return segments;
        }
    }

    module_segments(source_root, file)
}

fn module_segments(source_root: &Path, file: &Path) -> Vec<String> {
    let relative = file.strip_prefix(source_root).unwrap_or(file);
    let mut parts: Vec<String> = relative
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();

    if let Some(last) = parts.last_mut() {
        if last == "main.fab" || last == "mod.fab" {
            parts.pop();
        } else if let Some(stripped) = last.strip_suffix(".fab") {
            *last = stripped.to_string();
        } else if let Some(stripped) = last.strip_suffix(".proba") {
            // Keep test modules distinct from product `name.fab` → `name`.
            // `math.proba` becomes module `math_proba`, never colliding with `math`.
            *last = format!("{stripped}_proba");
        }
    }

    parts
        .into_iter()
        .map(|segment| sanitize_rust_module_ident(&segment))
        .collect()
}

/// Single module-naming policy for the Rust module tree and use-line
/// emission (S-B2): `pub mod` names here and `use crate::<segment>::…`
/// path segments in the Rust HIR backend derive from one definition, so a
/// use line can neither inject foreign Rust text nor dangle away from the
/// module tree this crate generates. Policy doc lives on the definition in
/// `radix_codegen_shared::ident`.
pub use radix_codegen_shared::sanitize_rust_module_ident;

#[cfg(test)]
#[path = "modules_test.rs"]
mod tests;
