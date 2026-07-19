use std::path::{Path, PathBuf};

use radix::codegen::rust::TestSelection as RustTestSelection;
use radix::diagnostics::Diagnostic;
use radix::driver::FileFrontmatter;

use super::{FaberManifest, PackageSpec, MANIFEST_FILE};

pub(super) fn manifest_path_for_spec(spec: &PackageSpec) -> Option<PathBuf> {
    let direct = spec.source_root.join(MANIFEST_FILE);
    if direct.exists() {
        return Some(direct);
    }

    spec.source_root
        .parent()
        .map(|parent| parent.join(MANIFEST_FILE))
        .filter(|path| path.exists())
}

fn frontmatter_manifest_conflict(
    file: &str,
    frontmatter_label: &str,
    frontmatter_value: &str,
    manifest_label: &str,
    manifest_value: &str,
) -> Option<Diagnostic> {
    if frontmatter_value == manifest_value {
        return None;
    }
    Some(
        crate::package_diagnostic_error(format!(
            "frontmatter {frontmatter_label} `{frontmatter_value}` cannot override faber.toml {manifest_label} `{manifest_value}`"
        ))
        .with_file(file.to_owned())
        .with_arg("issue", "frontmatter_manifest_override")
        .with_arg("frontmatter", frontmatter_label)
        .with_arg("frontmatter_value", frontmatter_value)
        .with_arg("manifest", manifest_label)
        .with_arg("manifest_value", manifest_value),
    )
}

pub(super) fn validate_frontmatter_against_manifest(
    path: &Path,
    frontmatter: Option<&FileFrontmatter>,
    manifest: &FaberManifest,
) -> Option<Diagnostic> {
    let frontmatter = frontmatter?;
    let file = path.display().to_string();

    if let Some(target) = frontmatter.build_target() {
        if let Some(diag) = frontmatter_manifest_conflict(
            &file,
            "[build].target",
            target,
            "target",
            &manifest.build.target,
        ) {
            return Some(diag);
        }
    }
    if let Some(kind) = frontmatter.build_kind() {
        if let Some(diag) =
            frontmatter_manifest_conflict(&file, "[build].kind", kind, "kind", &manifest.build.kind)
        {
            return Some(diag);
        }
    }

    if let Some(name) = frontmatter.package_name() {
        if let Some(diag) = frontmatter_manifest_conflict(
            &file,
            "[package].name",
            name,
            "name",
            &manifest.package.name,
        ) {
            return Some(diag);
        }
    }
    if let Some(version) = frontmatter.package_version() {
        if let Some(diag) = frontmatter_manifest_conflict(
            &file,
            "[package].version",
            version,
            "version",
            &manifest.package.version,
        ) {
            return Some(diag);
        }
    }

    if let Some(source) = frontmatter.paths_source() {
        if let Some(diag) = frontmatter_manifest_conflict(
            &file,
            "[paths].source",
            source,
            "paths.source",
            &manifest.paths.source,
        ) {
            return Some(diag);
        }
    }
    if let Some(entry) = frontmatter.paths_entry() {
        if let Some(manifest_entry) = manifest.paths.entry.as_deref() {
            if let Some(diag) = frontmatter_manifest_conflict(
                &file,
                "[paths].entry",
                entry,
                "paths.entry",
                manifest_entry,
            ) {
                return Some(diag);
            }
        }
    }

    None
}

pub(super) fn merge_entry_test_selection(
    cli: Option<&RustTestSelection>,
    entry: Option<&FileFrontmatter>,
) -> Option<RustTestSelection> {
    let mut selection = cli.cloned().unwrap_or_default();
    let cli_overrides =
        selection.name.is_some() || selection.suite.is_some() || selection.tag.is_some();

    if !cli_overrides {
        if let Some(entry) = entry {
            selection.suite = entry.sectio().map(str::to_owned);
            selection.tag = entry.probanda_first_tag();
        }
    }

    (selection.name.is_some() || selection.suite.is_some() || selection.tag.is_some())
        .then_some(selection)
}

#[cfg(test)]
#[path = "frontmatter_test.rs"]
mod tests;
