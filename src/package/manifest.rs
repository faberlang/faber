use std::fs;
use std::path::Path;

use radix::codegen::rust::RustFieldNamePolicy;
use radix::codegen::Target;
use radix::diagnostics::Diagnostic;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FaberManifest {
    /// Package identity used for generated crate metadata and binary naming.
    pub package: ManifestPackage,

    /// Source-layout settings for package graph discovery.
    #[serde(default)]
    pub paths: ManifestPaths,

    /// Build settings accepted by the current package compiler.
    #[serde(default)]
    pub build: ManifestBuild,

    /// Reader-locale settings used to select a source and diagnostic surface.
    #[serde(default)]
    pub reader: ManifestReader,
}

/// `[package]` metadata from `faber.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestPackage {
    /// Human-authored package name; sanitized before it becomes a Cargo name.
    pub name: String,

    /// Package version copied into the generated Cargo manifest.
    #[serde(default = "default_version")]
    pub version: String,

    /// Faber source edition, distinct from the generated Rust edition.
    #[serde(default = "default_edition")]
    pub edition: String,
}

/// `[paths]` metadata that anchors package source discovery.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestPaths {
    /// Directory containing package source files, relative to the manifest.
    #[serde(default = "default_source_path")]
    pub source: String,

    /// Entry module path, relative to `source`.
    #[serde(default = "default_entry_path")]
    pub entry: String,
}

/// `[build]` metadata accepted by the package command surface.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestBuild {
    /// Backend target requested by the package.
    #[serde(default = "default_build_target")]
    pub target: String,

    /// Package output kind; currently only binary crates are supported.
    #[serde(default = "default_build_kind")]
    pub kind: String,

    /// Generated Rust struct-field spelling policy.
    #[serde(default)]
    pub rust_field_names: ManifestRustFieldNames,
}

/// `[reader]` metadata used to select a package-local reader pack.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManifestReader {
    /// Locale id such as `th-TH` or `zh-Hans`.
    pub locale: Option<String>,

    /// Optional reader-pack path relative to the package root.
    pub pack: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManifestRustFieldNames {
    #[default]
    Preserve,
    SnakeCase,
}

impl From<ManifestRustFieldNames> for RustFieldNamePolicy {
    fn from(value: ManifestRustFieldNames) -> Self {
        match value {
            ManifestRustFieldNames::Preserve => RustFieldNamePolicy::Preserve,
            ManifestRustFieldNames::SnakeCase => RustFieldNamePolicy::SnakeCase,
        }
    }
}

impl Default for ManifestPaths {
    fn default() -> Self {
        Self {
            source: default_source_path(),
            entry: default_entry_path(),
        }
    }
}

impl Default for ManifestBuild {
    fn default() -> Self {
        Self {
            target: default_build_target(),
            kind: default_build_kind(),
            rust_field_names: ManifestRustFieldNames::Preserve,
        }
    }
}

fn default_version() -> String {
    "0.1.0".to_owned()
}

fn default_edition() -> String {
    "2026".to_owned()
}

fn default_source_path() -> String {
    "src".to_owned()
}

fn default_entry_path() -> String {
    "main.fab".to_owned()
}

fn default_build_target() -> String {
    "rust".to_owned()
}

fn default_build_kind() -> String {
    "bin".to_owned()
}

pub(super) fn manifest_build_target(target: &str, path: &Path) -> Result<Target, Box<Diagnostic>> {
    match target.trim() {
        "rust" => Ok(Target::Rust),
        "scena" => Ok(Target::Scena),
        "fmir-text" => Ok(Target::FmirText),
        "fmir" => Ok(Target::Fmir),
        "fmir-bin" => Ok(Target::FmirBin),
        unsupported => Err(Box::new(
            Diagnostic::error(format!(
                "faber.toml build.target '{unsupported}' is not supported for package builds"
            ))
            .with_file(path.display().to_string())
            .with_arg("issue", "package_build_target_unsupported")
            .with_arg("target", unsupported.to_owned()),
        )),
    }
}

/// Read and deserialize a `faber.toml` manifest.
///
/// Unknown manifest fields are rejected by the manifest structs so spelling
/// mistakes become diagnostics rather than silently ignored configuration.
pub fn read_manifest(path: &Path) -> Result<FaberManifest, Box<Diagnostic>> {
    let source =
        fs::read_to_string(path).map_err(|err| Box::new(Diagnostic::io_error(path, err)))?;
    toml::from_str::<FaberManifest>(&source).map_err(|err| {
        Box::new(
            Diagnostic::error(format!("invalid faber.toml manifest: {err}"))
                .with_file(path.display().to_string())
                .with_arg("issue", "invalid_package_manifest"),
        )
    })
}

pub(super) fn validate_manifest(
    manifest: &FaberManifest,
    path: &Path,
) -> Result<(), Box<Diagnostic>> {
    if manifest.package.name.trim().is_empty() {
        return Err(Box::new(
            Diagnostic::error("faber.toml package.name must not be empty")
                .with_file(path.display().to_string()),
        ));
    }

    if manifest.package.version.trim().is_empty() {
        return Err(Box::new(
            Diagnostic::error("faber.toml package.version must not be empty")
                .with_file(path.display().to_string()),
        ));
    }

    if manifest.package.edition.trim().is_empty() {
        return Err(Box::new(
            Diagnostic::error("faber.toml package.edition must not be empty")
                .with_file(path.display().to_string()),
        ));
    }

    if manifest.paths.source.trim().is_empty() {
        return Err(Box::new(
            Diagnostic::error("faber.toml paths.source must not be empty")
                .with_file(path.display().to_string()),
        ));
    }

    if manifest.paths.entry.trim().is_empty() {
        return Err(Box::new(
            Diagnostic::error("faber.toml paths.entry must not be empty")
                .with_file(path.display().to_string()),
        ));
    }

    manifest_build_target(&manifest.build.target, path)?;

    if manifest.build.kind != "bin" {
        return Err(Box::new(
            Diagnostic::error(format!(
                "faber.toml build.kind '{}' is not supported yet",
                manifest.build.kind
            ))
            .with_file(path.display().to_string()),
        ));
    }

    if let Some(locale) = manifest.reader.locale.as_deref() {
        if locale.trim().is_empty() {
            return Err(Box::new(
                Diagnostic::error("faber.toml reader.locale must not be empty")
                    .with_file(path.display().to_string()),
            ));
        }
    }

    if let Some(pack) = manifest.reader.pack.as_deref() {
        if pack.trim().is_empty() {
            return Err(Box::new(
                Diagnostic::error("faber.toml reader.pack must not be empty")
                    .with_file(path.display().to_string()),
            ));
        }
        if manifest.reader.locale.is_none() {
            return Err(Box::new(
                Diagnostic::error("faber.toml reader.pack requires reader.locale")
                    .with_file(path.display().to_string()),
            ));
        }
    }

    Ok(())
}
