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
            .map(|segment| segment.to_owned())
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
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
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
}
