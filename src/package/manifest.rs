use std::collections::BTreeMap;
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

    /// Source-library provider metadata.
    #[serde(default)]
    pub library: Option<ManifestLibrary>,

    /// Build settings accepted by the current package compiler.
    #[serde(default)]
    pub build: ManifestBuild,

    /// Reader-locale settings used to select a source and diagnostic surface.
    #[serde(default)]
    pub reader: ManifestReader,

    /// Target-specific build and binding metadata, e.g. `[target.rust]`.
    #[serde(default)]
    pub target: BTreeMap<String, ManifestTarget>,

    /// Direct exact dependency pins (`name = "version"`). Resolved paths live in `faber.lock`.
    #[serde(default)]
    pub dependencies: BTreeMap<String, String>,
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

    /// Entry module path, relative to `source`; required for binary packages.
    pub entry: Option<String>,
}

/// `[library]` metadata for source-library packages.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestLibrary {
    /// Provider prefix used by imports such as `provider:module/path`.
    pub provider: String,
}

/// `[build]` metadata accepted by the package command surface.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestBuild {
    /// Backend target requested by the package.
    #[serde(default = "default_build_target")]
    pub target: String,

    /// Backend targets supported by a library package.
    #[serde(default)]
    pub targets: Vec<String>,

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

/// `[target.<name>]` metadata for target-specific implementation data.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManifestTarget {
    /// Binding manifest path, relative to the package root.
    pub bindings: Option<String>,

    /// Runtime host policy for generated applications.
    pub host: Option<ManifestRustHost>,

    /// Target dependency pins, e.g. `[target.rust.dependencies]`.
    pub dependencies: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ManifestRustHost {
    Native,
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
            entry: None,
        }
    }
}

impl Default for ManifestBuild {
    fn default() -> Self {
        Self {
            target: default_build_target(),
            targets: Vec::new(),
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

pub(crate) fn validate_manifest(
    manifest: &FaberManifest,
    path: &Path,
) -> Result<(), Box<Diagnostic>> {
    if manifest.package.name.trim().is_empty() {
        return Err(Box::new(
            Diagnostic::error("faber.toml package.name must not be empty")
                .with_file(path.display().to_string()),
        ));
    }
    if !crate::library::is_valid_provider_segment(&manifest.package.name) {
        return Err(Box::new(
            Diagnostic::error(
                "faber.toml package.name must contain only ASCII letters, numbers, underscore, or hyphen",
            )
            .with_file(path.display().to_string())
            .with_arg("issue", "invalid_package_name"),
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

    if let Some(entry) = manifest.paths.entry.as_deref() {
        if entry.trim().is_empty() {
            return Err(Box::new(
                Diagnostic::error("faber.toml paths.entry must not be empty")
                    .with_file(path.display().to_string()),
            ));
        }
    }

    if let Some(library) = &manifest.library {
        if !crate::library::is_valid_provider_segment(&library.provider) {
            return Err(Box::new(
                Diagnostic::error(
                    "faber.toml library.provider must contain only ASCII letters, numbers, underscore, or hyphen",
                )
                .with_file(path.display().to_string())
                .with_arg("issue", "invalid_library_provider"),
            ));
        }
    }

    match manifest.build.kind.as_str() {
        "bin" => validate_binary_build(manifest, path)?,
        "lib" => validate_library_build(manifest, path)?,
        kind => {
            return Err(Box::new(
                Diagnostic::error(format!("faber.toml build.kind '{kind}' is not supported"))
                    .with_file(path.display().to_string())
                    .with_arg("issue", "package_build_kind_unsupported")
                    .with_arg("kind", kind.to_owned()),
            ));
        }
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

    for (name, version) in &manifest.dependencies {
        if name.trim().is_empty() {
            return Err(Box::new(
                Diagnostic::error("faber.toml [dependencies] key must not be empty")
                    .with_file(path.display().to_string()),
            ));
        }
        if version.trim().is_empty() {
            return Err(Box::new(
                Diagnostic::error(format!(
                    "faber.toml dependency `{name}` version must be a non-empty exact string"
                ))
                .with_file(path.display().to_string()),
            ));
        }
    }

    for (target, config) in &manifest.target {
        if target.trim().is_empty() {
            return Err(Box::new(
                Diagnostic::error("faber.toml [target] key must not be empty")
                    .with_file(path.display().to_string())
                    .with_arg("issue", "invalid_target_table"),
            ));
        }
        if let Some(bindings) = config.bindings.as_deref() {
            if bindings.trim().is_empty() {
                return Err(Box::new(
                    Diagnostic::error("faber.toml target bindings path must not be empty")
                        .with_file(path.display().to_string())
                        .with_arg("issue", "invalid_target_bindings"),
                ));
            }
        }
        if config.host.is_some() && target != "rust" {
            return Err(Box::new(
                Diagnostic::error(
                    "faber.toml target host policy is only supported for target.rust",
                )
                .with_file(path.display().to_string())
                .with_arg("issue", "invalid_target_host")
                .with_arg("target", target.to_owned()),
            ));
        }
        for (name, version) in &config.dependencies {
            if name.trim().is_empty() || version.trim().is_empty() {
                return Err(Box::new(
                    Diagnostic::error(
                        "faber.toml target dependency names and versions must be non-empty",
                    )
                    .with_file(path.display().to_string())
                    .with_arg("issue", "invalid_target_dependency"),
                ));
            }
        }
    }

    Ok(())
}

fn validate_binary_build(manifest: &FaberManifest, path: &Path) -> Result<(), Box<Diagnostic>> {
    if manifest.paths.entry.is_none() {
        return Err(Box::new(
            Diagnostic::error("faber.toml paths.entry is required when build.kind = \"bin\"")
                .with_file(path.display().to_string())
                .with_arg("issue", "missing_binary_entry"),
        ));
    }
    manifest_build_target(&manifest.build.target, path)?;
    if !manifest.build.targets.is_empty() {
        return Err(Box::new(
            Diagnostic::error("faber.toml build.targets is only valid when build.kind = \"lib\"")
                .with_file(path.display().to_string())
                .with_arg("issue", "binary_targets_unsupported"),
        ));
    }
    Ok(())
}

fn validate_library_build(manifest: &FaberManifest, path: &Path) -> Result<(), Box<Diagnostic>> {
    if manifest.library.is_none() {
        return Err(Box::new(
            Diagnostic::error("faber.toml [library] is required when build.kind = \"lib\"")
                .with_file(path.display().to_string())
                .with_arg("issue", "missing_library_table"),
        ));
    }
    if manifest.build.targets.is_empty() {
        return Err(Box::new(
            Diagnostic::error(
                "faber.toml build.targets must not be empty when build.kind = \"lib\"",
            )
            .with_file(path.display().to_string())
            .with_arg("issue", "missing_library_targets"),
        ));
    }
    for target in &manifest.build.targets {
        if target.trim().is_empty() {
            return Err(Box::new(
                Diagnostic::error("faber.toml build.targets entries must not be empty")
                    .with_file(path.display().to_string())
                    .with_arg("issue", "empty_library_target"),
            ));
        }
        manifest_build_target(target, path)?;
    }
    Ok(())
}
