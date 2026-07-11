use std::path::{Component, Path};

use radix::driver::FileFrontmatter;

pub(super) fn module_segments_for_file(
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
        }
    }

    parts
        .into_iter()
        .map(|segment| sanitize_rust_module_ident(&segment))
        .collect()
}

/// Map a filesystem or frontmatter path segment to a valid Rust module identifier.
///
/// Worktree packet slugs (`faber-hir-v1`) and other non-ident path characters must
/// not appear raw in `pub mod …` emission — Rust rejects `-` inside idents.
///
/// Policy:
/// - keep ASCII alphanumeric and `_`
/// - every other character (including `-`) becomes `_`
/// - empty / all-separator → `m`
/// - leading digit → prefix `m_`
pub(crate) fn sanitize_rust_module_ident(segment: &str) -> String {
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

#[cfg(test)]
#[path = "modules_test.rs"]
mod tests;
