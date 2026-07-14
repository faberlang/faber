use std::path::{Path, PathBuf};

use radix::diagnostics::Diagnostic;

use super::manifest::{read_manifest, validate_manifest};
use super::member_path::resolve_package_member;
use super::paths::{absolutize_path, normalize_path};
use super::MANIFEST_FILE;

/// Package entrypoints resolved from either `faber.toml` or legacy path input.
///
/// The compiler keeps package discovery separate from build layout discovery:
/// this type describes the Faber source graph only, not the generated Cargo
/// crate that may later be emitted under `target/faber/`.
pub(crate) struct PackageSpec {
    /// Directory containing `faber.toml`, or the legacy package root.
    pub(in crate::package) package_root: PathBuf,
    pub(in crate::package) source_root: PathBuf,
    pub(in crate::package) entry: PathBuf,
}

/// Layout for a package build: generated Rust crate under `target/faber/`,
/// Cargo artifacts under sibling `target/debug/` and `target/release/`.
///
/// This model is path-only and is the single source of truth for the package
/// build directory contract. Callers should derive all generated Cargo paths
/// from it instead of rebuilding paths ad hoc.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct BuildLayout {
    /// Directory containing `faber.toml`, or the legacy package root when no
    /// manifest exists.
    pub package_root: PathBuf,

    /// Manifest path for manifest-backed packages; may not exist for legacy
    /// direct-file or directory inputs.
    pub manifest_path: PathBuf,

    /// Root of the generated Rust crate.
    pub generated_crate_root: PathBuf,

    /// Cargo manifest written for the generated Rust crate.
    pub generated_cargo_manifest: PathBuf,

    /// Rust entrypoint written from the assembled package code.
    pub generated_rust_entry: PathBuf,

    /// Cargo target directory shared by generated build/test invocations.
    pub cargo_target_dir: PathBuf,

    /// Expected debug binary path produced by Cargo.
    pub debug_binary: PathBuf,

    /// Expected release binary path produced by Cargo.
    pub release_binary: PathBuf,
}

impl BuildLayout {
    /// Build a layout from an explicit package root directory and the package name
    /// declared in its faber.toml (or a provided name for legacy cases).
    ///
    /// The supplied `package_name` is sanitized for use as a Rust crate/binary name.
    #[allow(dead_code)]
    pub fn from_package_root(root: impl AsRef<Path>, package_name: &str) -> Self {
        let package_root = normalize_path(root.as_ref());
        let manifest_path = package_root.join(MANIFEST_FILE);
        let target_base = package_root.join("target");
        let generated_root = target_base.join("faber");
        let binary = sanitize_crate_name(package_name);

        let debug_binary = target_base.join("debug").join(&binary);
        let release_binary = target_base.join("release").join(&binary);

        Self {
            package_root,
            manifest_path,
            generated_crate_root: generated_root.clone(),
            generated_cargo_manifest: generated_root.join("Cargo.toml"),
            generated_rust_entry: generated_root.join("src").join("main.rs"),
            cargo_target_dir: target_base,
            debug_binary,
            release_binary,
        }
    }

    /// Returns the sanitized name used for the generated binary and crate.
    #[allow(dead_code)]
    pub fn binary_name(&self) -> &str {
        self.debug_binary
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("package")
    }
}

/// Sanitize a Faber package name into a valid Rust/Cargo crate and binary name.
///
/// The policy is intentionally conservative and Cargo-compatible:
/// - lowercase ASCII letters and digits
/// - keep `-` and `_`
/// - other characters become `-`
/// - trim leading/trailing separators
/// - if result empty, fallback to "package"
/// - if starts with a digit, prefix "p-" (Cargo prefers letter or _ start for some contexts)
#[allow(dead_code)]
pub fn sanitize_crate_name(name: &str) -> String {
    if name.trim().is_empty() {
        return "package".to_owned();
    }
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('-');
        }
    }
    let mut s = out.trim_matches(|c: char| c == '-' || c == '_').to_owned();
    if s.is_empty() {
        s = "package".to_owned();
    }
    if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        s = format!("p-{}", s);
    }
    s
}

type PackageDiscoveryResult = Result<PackageSpec, Box<Diagnostic>>;

#[allow(dead_code)] // binary-only `faber run --interpret` route consumes this through `commands`.
pub(crate) fn is_manifest_backed_or_directory_package_input(input: &Path) -> bool {
    let input = absolutize_path(input);
    if input.file_name().and_then(|name| name.to_str()) == Some(MANIFEST_FILE) {
        return true;
    }
    if input.is_dir() {
        return true;
    }
    input.is_file() && nearest_manifest_for_file(&normalize_path(&input)).is_some()
}

pub(crate) fn discover_package(input: &Path) -> PackageDiscoveryResult {
    let display_path = input.to_path_buf();
    let input = absolutize_path(input);
    if !input.exists() {
        return Err(Box::new(Diagnostic::io_error(
            &display_path,
            std::io::Error::from_raw_os_error(2),
        )));
    }

    if input.file_name().and_then(|name| name.to_str()) == Some(MANIFEST_FILE) {
        return parse_manifest(&input);
    }

    if input.is_dir() {
        let root = normalize_path(&input);
        let manifest = root.join(MANIFEST_FILE);
        if manifest.exists() {
            return parse_manifest(&manifest);
        }

        return Ok(PackageSpec {
            package_root: root.clone(),
            entry: root.join("main.fab"),
            source_root: root,
        });
    }

    let entry = normalize_path(&input);
    if let Some(manifest) = nearest_manifest_for_file(&entry) {
        return parse_manifest_with_entry(&manifest, entry);
    }
    let root = entry
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    Ok(PackageSpec {
        package_root: root.clone(),
        source_root: root,
        entry,
    })
}

fn parse_manifest(path: &Path) -> PackageDiscoveryResult {
    let manifest = read_manifest(path)?;
    let package_root = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    validate_manifest(&manifest, path)?;

    let source_root =
        resolve_package_member(&package_root, &manifest.paths.source, path).map_err(Box::new)?;
    let entry = manifest
        .paths
        .entry
        .as_deref()
        .map(|entry| manifest_entry_path(&package_root, &manifest.paths.source, entry, path))
        .transpose()?
        .unwrap_or_else(|| source_root.clone());
    Ok(PackageSpec {
        package_root,
        source_root,
        entry,
    })
}

fn parse_manifest_with_entry(path: &Path, entry: PathBuf) -> PackageDiscoveryResult {
    let manifest = read_manifest(path)?;
    let package_root = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    validate_manifest(&manifest, path)?;

    let source_root =
        resolve_package_member(&package_root, &manifest.paths.source, path).map_err(Box::new)?;
    Ok(PackageSpec {
        package_root,
        source_root,
        entry,
    })
}

fn manifest_entry_path(
    package_root: &Path,
    source: &str,
    entry: &str,
    manifest_path: &Path,
) -> Result<PathBuf, Box<Diagnostic>> {
    if Path::new(entry).is_absolute() {
        return resolve_package_member(package_root, entry, manifest_path).map_err(Box::new);
    }
    let relative = Path::new(source).join(entry);
    let relative = relative.to_string_lossy();
    resolve_package_member(package_root, &relative, manifest_path).map_err(Box::new)
}

/// Discover a `BuildLayout` for the given input (directory, manifest file, or entry file).
///
/// Mirrors the resolution rules of `discover_package`, then adds the package
/// name needed for generated crate and binary paths. Manifest-backed packages
/// use `package.name`; legacy non-manifest inputs fall back to their directory
/// name so old direct-file workflows still have deterministic output paths.
#[allow(dead_code)]
pub fn discover_build_layout(input: &Path) -> Result<BuildLayout, Box<Diagnostic>> {
    let display_path = input.to_path_buf();
    let input = absolutize_path(input);
    if !input.exists() {
        return Err(Box::new(Diagnostic::io_error(
            &display_path,
            std::io::Error::from_raw_os_error(2),
        )));
    }

    if input.file_name().and_then(|name| name.to_str()) == Some(MANIFEST_FILE) {
        let manifest = read_manifest(&input)?;
        let root = normalize_path(input.parent().unwrap_or_else(|| Path::new(".")));
        let name = manifest.package.name.clone();
        return Ok(BuildLayout::from_package_root(root, &name));
    }

    if input.is_dir() {
        let root = normalize_path(&input);
        let manifest = root.join(MANIFEST_FILE);
        if manifest.exists() {
            let m = read_manifest(&manifest)?;
            return Ok(BuildLayout::from_package_root(root, &m.package.name));
        }
        let name = root
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("package")
            .to_owned();
        return Ok(BuildLayout::from_package_root(root, &name));
    }

    let entry = normalize_path(&input);
    if let Some(manifest) = nearest_manifest_for_file(&entry) {
        let m = read_manifest(&manifest)?;
        let root = manifest.parent().unwrap_or_else(|| Path::new("."));
        return Ok(BuildLayout::from_package_root(root, &m.package.name));
    }
    let root = entry
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    let name = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("package")
        .to_owned();
    Ok(BuildLayout::from_package_root(root, &name))
}

fn nearest_manifest_for_file(entry: &Path) -> Option<PathBuf> {
    let mut current = entry.parent();
    while let Some(dir) = current {
        let manifest = dir.join(MANIFEST_FILE);
        if manifest.exists() {
            return Some(manifest);
        }
        current = dir.parent();
    }
    None
}
