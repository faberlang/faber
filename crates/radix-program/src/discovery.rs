//! Package and build-layout discovery for Faber source packages.
//!
//! Discovery resolves a package entrypoint (from `faber.toml` or a legacy path
//! input) into a [`PackageSpec`] and derives the generated-Cargo
//! [`BuildLayout`]. Inputs must be absolute: the ambient-CWD absolutization
//! seam stays faber-side (`RP2b`, CTO memo) and is applied before these
//! entry points are called.

use std::path::{Path, PathBuf};

use radix_module::diagnostics::Diagnostic;

use crate::MANIFEST_FILE;
use crate::manifest::{
    FaberManifest, manifest_build_target, read_manifest, validate_target_tables,
};
use crate::member_path::resolve_package_member;
use crate::paths::normalize_path;

/// Package entrypoints resolved from either `faber.toml` or legacy path input.
///
/// The compiler keeps package discovery separate from build layout discovery:
/// this type describes the Faber source graph only, not the generated Cargo
/// crate that may later be emitted under `target/faber/`.
#[derive(Clone, Debug)]
pub struct PackageSpec {
    /// Directory containing `faber.toml`, or the legacy package root.
    pub package_root: PathBuf,
    pub source_root: PathBuf,
    pub entry: PathBuf,
    /// Import path templates (`§name/rest`) from `[paths.templates]`, values
    /// resolved relative to [`Self::package_root`].
    pub templates: std::collections::BTreeMap<String, PathBuf>,
    /// Whether discovery read and validated `package_root/faber.toml`. Only
    /// legacy manifestless inputs are `false`; a validated manifest that later
    /// disappears or becomes unreadable is a diagnostic, never a silent default.
    pub manifest_backed: bool,
}

/// Layout for a package build: generated Rust crate under `target/faber/`,
/// Cargo artifacts under sibling `target/debug/` and `target/release/`.
///
/// This model is path-only and is the single source of truth for the package
/// build directory contract. Callers should derive all generated Cargo paths
/// from it instead of rebuilding paths ad hoc.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    #[must_use]
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
#[must_use]
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
        "package".clone_into(&mut s);
    }
    if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        s = format!("p-{s}");
    }
    s
}

type PackageDiscoveryResult = Result<PackageSpec, Box<Diagnostic>>;

fn missing_package_path_diagnostic(path: &Path, base: &Path) -> Box<Diagnostic> {
    let base = if base.as_os_str().is_empty() {
        Path::new(".")
    } else {
        base
    };
    let error = std::io::Error::from_raw_os_error(2);
    Box::new(
        crate::package_diagnostic_error(format!(
            "cannot read manifest at '{}' resolved against '{}': {error}",
            path.display(),
            base.display()
        ))
        .with_phase(radix_module::diagnostics::DiagnosticPhase::Io)
        .with_file(path.display().to_string())
        .with_arg("issue", "package_path_missing")
        .with_arg("path", path.display().to_string())
        .with_arg("base", base.display().to_string())
        .with_arg("artifact", "manifest"),
    )
}

#[must_use]
pub fn is_manifest_backed_or_directory_package_input(input: &Path) -> bool {
    let input = normalize_path(input);
    if input.file_name().and_then(|name| name.to_str()) == Some(MANIFEST_FILE) {
        return true;
    }
    if input.is_dir() {
        return true;
    }
    input.is_file() && nearest_manifest_for_file(&input).is_some()
}

/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn discover_package(input: &Path) -> PackageDiscoveryResult {
    let base = input
        .parent()
        .filter(|base| !base.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    discover_package_with_base(input, base)
}

/// Discover a package while retaining the base used to resolve the CLI path.
///
/// The default [`discover_package`] entry point derives a package-local base
/// for compiler callers. Product callers that absolutize a relative CLI input
/// should pass the original resolution base so missing-path diagnostics explain
/// the user's invocation rather than the normalized path's parent.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn discover_package_with_base(input: &Path, base: &Path) -> PackageDiscoveryResult {
    let input = normalize_path(input);
    if !input.exists() {
        return Err(missing_package_path_diagnostic(&input, base));
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
            templates: std::collections::BTreeMap::new(),
            manifest_backed: false,
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
        templates: std::collections::BTreeMap::new(),
        manifest_backed: false,
    })
}

fn parse_manifest(path: &Path) -> PackageDiscoveryResult {
    let (package_root, manifest, source_root) = read_validated_manifest(path)?;
    let entry = manifest
        .paths
        .entry
        .as_deref()
        .map(|entry| manifest_entry_path(&package_root, &manifest.paths.source, entry, path))
        .transpose()?
        .unwrap_or_else(|| source_root.clone());
    let templates = resolve_path_templates(&package_root, &manifest.paths.templates, path)?;
    Ok(PackageSpec {
        package_root,
        source_root,
        entry,
        templates,
        manifest_backed: true,
    })
}

fn parse_manifest_with_entry(path: &Path, entry: PathBuf) -> PackageDiscoveryResult {
    let (package_root, manifest, source_root) = read_validated_manifest(path)?;
    let templates = resolve_path_templates(&package_root, &manifest.paths.templates, path)?;
    Ok(PackageSpec {
        package_root,
        source_root,
        entry,
        templates,
        manifest_backed: true,
    })
}

/// Read and validate a manifest and resolve its source root.
///
/// Entry and template resolution stay with each caller to preserve diagnostic
/// ordering: `parse_manifest` resolves the entry before templates, while
/// `parse_manifest_with_entry` skips entry resolution entirely.
fn read_validated_manifest(
    path: &Path,
) -> Result<(PathBuf, FaberManifest, PathBuf), Box<Diagnostic>> {
    let manifest = read_manifest(path)?;
    let package_root = path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    validate_compiler_manifest(&manifest, path)?;
    let source_root = resolve_source_root(&package_root, &manifest.paths.source, path)?;
    Ok((package_root, manifest, source_root))
}

/// Compiler-view manifest validation for discovery.
///
/// Mirrors the compiler-relevant checks of faber's product `validate_manifest`
/// (package identity, source-root set, entry, library provider, build kind)
/// with the same diagnostic codes. Product-only policy (`[device]`, `[format]`,
/// `[product]`, `[dispatch]`, locale) stays faber-side and is applied by
/// faber's discovery wrapper after this (`RP2b` seam).
fn validate_compiler_manifest(
    manifest: &FaberManifest,
    path: &Path,
) -> Result<(), Box<Diagnostic>> {
    if manifest.package.name.trim().is_empty() {
        return Err(Box::new(
            crate::package_diagnostic_error("faber.toml package.name must not be empty")
                .with_file(path.display().to_string()),
        ));
    }
    if !is_valid_provider_segment(&manifest.package.name) {
        return Err(Box::new(
            crate::package_diagnostic_error(
                "faber.toml package.name must contain only ASCII letters, numbers, underscore, or hyphen",
            )
            .with_file(path.display().to_string())
            .with_arg("issue", "invalid_package_name"),
        ));
    }

    if manifest.package.version.trim().is_empty() {
        return Err(Box::new(
            crate::package_diagnostic_error("faber.toml package.version must not be empty")
                .with_file(path.display().to_string()),
        ));
    }

    if manifest.package.edition.trim().is_empty() {
        return Err(Box::new(
            crate::package_diagnostic_error("faber.toml package.edition must not be empty")
                .with_file(path.display().to_string()),
        ));
    }

    if manifest.paths.source.trim().is_empty() {
        return Err(Box::new(
            crate::package_diagnostic_error("faber.toml paths.source must not be empty")
                .with_file(path.display().to_string()),
        ));
    }

    // FBR-P1-001: the supported source-root set is exactly `src` (default) and
    // `.` (package root). Everything else — nested roots like
    // `components/faber` or depth-1 customs like `lib` — is rejected loudly
    // until a usage contract for custom source roots is decided. The
    // `manifest_path_for_spec` probes are only provably complete for this set.
    match manifest.paths.source.trim() {
        "src" | "." => {}
        unsupported => {
            return Err(Box::new(
                crate::package_diagnostic_error(format!(
                    "faber.toml paths.source '{unsupported}' is not supported: only \"src\" (the default) and \".\" (the package root) are allowed until a usage contract for custom source roots is decided"
                ))
                .with_file(path.display().to_string())
                .with_arg("issue", "package_member_unsupported_source_root")
                .with_arg("source", unsupported.to_owned()),
            ));
        }
    }

    if let Some(entry) = manifest.paths.entry.as_deref()
        && entry.trim().is_empty()
    {
        return Err(Box::new(
            crate::package_diagnostic_error("faber.toml paths.entry must not be empty")
                .with_file(path.display().to_string()),
        ));
    }

    if let Some(library) = &manifest.library
        && !is_valid_provider_segment(&library.provider)
    {
        return Err(Box::new(
                crate::package_diagnostic_error(
                    "faber.toml library.provider must contain only ASCII letters, numbers, underscore, or hyphen",
                )
                .with_file(path.display().to_string())
                .with_arg("issue", "invalid_library_provider"),
            ));
    }

    match manifest.build.kind.as_str() {
        "bin" => validate_binary_build(manifest, path)?,
        "lib" => validate_library_build(manifest, path)?,
        kind => {
            return Err(Box::new(
                crate::package_diagnostic_error(format!(
                    "faber.toml build.kind '{kind}' is not supported"
                ))
                .with_file(path.display().to_string())
                .with_arg("issue", "package_build_kind_unsupported")
                .with_arg("kind", kind.to_owned()),
            ));
        }
    }

    validate_target_tables(&manifest.target, path)?;

    Ok(())
}

fn validate_binary_build(manifest: &FaberManifest, path: &Path) -> Result<(), Box<Diagnostic>> {
    if manifest.paths.entry.is_none() {
        return Err(Box::new(
            crate::package_diagnostic_error(
                "faber.toml paths.entry is required when build.kind = \"bin\"",
            )
            .with_file(path.display().to_string())
            .with_arg("issue", "missing_binary_entry"),
        ));
    }
    manifest_build_target(manifest.build.target.as_deref(), path)?;
    if !manifest.build.targets.is_empty() {
        return Err(Box::new(
            crate::package_diagnostic_error(
                "faber.toml build.targets is only valid when build.kind = \"lib\"",
            )
            .with_file(path.display().to_string())
            .with_arg("issue", "binary_targets_unsupported"),
        ));
    }
    Ok(())
}

fn validate_library_build(manifest: &FaberManifest, path: &Path) -> Result<(), Box<Diagnostic>> {
    if manifest.library.is_none() {
        return Err(Box::new(
            crate::package_diagnostic_error(
                "faber.toml [library] is required when build.kind = \"lib\"",
            )
            .with_file(path.display().to_string())
            .with_arg("issue", "missing_library_table"),
        ));
    }
    if manifest.build.targets.is_empty() {
        return Err(Box::new(
            crate::package_diagnostic_error(
                "faber.toml build.targets must not be empty when build.kind = \"lib\"",
            )
            .with_file(path.display().to_string())
            .with_arg("issue", "missing_library_targets"),
        ));
    }
    for target in &manifest.build.targets {
        if target.trim().is_empty() {
            return Err(Box::new(
                crate::package_diagnostic_error(
                    "faber.toml build.targets entries must not be empty",
                )
                .with_file(path.display().to_string())
                .with_arg("issue", "empty_library_target"),
            ));
        }
        manifest_build_target(Some(target), path)?;
    }
    Ok(())
}

/// Pure provider-name segment predicate (`RP2c` consolidates the production
/// resolver copy into `library.rs`; discovery keeps its own for now).
fn is_valid_provider_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
}

fn resolve_path_templates(
    package_root: &Path,
    templates: &std::collections::BTreeMap<String, String>,
    manifest_path: &Path,
) -> Result<std::collections::BTreeMap<String, PathBuf>, Box<Diagnostic>> {
    // Templates intentionally escape the package root (e.g. coreutils
    // `[paths.templates] gnu = "../common/gnu"`). They must not use the
    // package-member containment check that rejects `..`.
    let mut resolved = std::collections::BTreeMap::new();
    for (name, relative) in templates {
        if relative.trim().is_empty() {
            return Err(Box::new(
                crate::package_diagnostic_error(format!(
                    "paths.templates.{name} must not be empty"
                ))
                .with_file(manifest_path.display().to_string())
                .with_arg("issue", "template_path_empty"),
            ));
        }
        if Path::new(relative).is_absolute() {
            return Err(Box::new(
                crate::package_diagnostic_error(format!(
                    "paths.templates.{name} must be a relative path, got `{relative}`"
                ))
                .with_file(manifest_path.display().to_string())
                .with_arg("issue", "template_path_absolute"),
            ));
        }
        let path = normalize_path(&package_root.join(relative));
        resolved.insert(name.clone(), path);
    }
    Ok(resolved)
}

/// Resolve `[paths].source` to a source root inside the package.
///
/// The supported set is `"src"` (default) and `"."` (the package root). `"."`
/// is the package root itself, so it short-circuits the member-path machinery
/// (a bare `CurDir` normalizes to an empty path, which `resolve_package_member`
/// correctly rejects as naming nothing). All other values are rejected earlier
/// by [`validate_compiler_manifest`].
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn resolve_source_root(
    package_root: &Path,
    source: &str,
    manifest_path: &Path,
) -> Result<PathBuf, Box<Diagnostic>> {
    if source.trim() == "." {
        return Ok(package_root.to_path_buf());
    }
    resolve_package_member(package_root, source, manifest_path).map_err(Box::new)
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
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn discover_build_layout(input: &Path) -> Result<BuildLayout, Box<Diagnostic>> {
    let display_path = input.to_path_buf();
    let input = normalize_path(input);
    if !input.exists() {
        return Err(Box::new(Diagnostic::io_error(
            &display_path,
            &std::io::Error::from_raw_os_error(2),
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

#[cfg(test)]
#[path = "discovery_test.rs"]
mod tests;
