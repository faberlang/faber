//! Compiler view of the package manifest (`faber.toml`).
//!
//! Owns the compiler-relevant projection of the package manifest: package
//! identity, source paths, library identity, build target, locale,
//! dependencies, and lock-resolved compiler inputs (goal §Manifest boundary).
//! Product-only sections (`[device]`, `[product]`, `[format]`, `[dispatch]`)
//! stay in faber's product view; this projection tolerates them as unknown
//! tables rather than re-declaring product types to preserve one-pass
//! parsing (ownership-freeze, RP0). Faber re-parses the file with
//! `deny_unknown_fields` for its own product surface.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use radix_module::codegen::Target;
use radix_module::diagnostics::Diagnostic;
use serde::Deserialize;

/// Compiler projection of `faber.toml`.
///
/// Carries the fields whose meaning is required to construct and compile a
/// source package. Product-only sections (`[device]`, `[product]`, `[format]`,
/// `[dispatch]`) are not declared here and are tolerated as unknown tables;
/// faber's product view re-parses the file with `deny_unknown_fields`.
#[derive(Debug, Deserialize)]
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

    /// Code-locale settings used to select a source and diagnostic surface.
    ///
    /// Canonical TOML section is `[locale]`.
    ///
    /// TODO(locale-rename): LEGACY — drop `alias = "reader"` after examples and
    /// sibling packages are retagged to `[locale]` (default-en Stage 2 / clean
    /// break). Sweep marker: `LEGACY_READER_MANIFEST_ALIAS`.
    #[serde(default, alias = "reader")]
    pub locale: ManifestReader,

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
    ///
    /// Supported values are `"src"` (the default) and `"."` (the package
    /// root). Any other value — nested roots such as `"components/faber"` or
    /// depth-1 customs such as `"lib"` — is rejected at manifest validation
    /// until a usage contract for custom source roots exists.
    #[serde(default = "default_source_path")]
    pub source: String,

    /// Entry module path, relative to `source`; required for binary packages.
    pub entry: Option<String>,

    /// Directory holding provider checkouts (`<home>/gradus/src`, ...) for
    /// `provider:module` imports; relative to the manifest. The
    /// `--library-home` flag wins.
    #[serde(default)]
    pub library_home: Option<String>,

    /// Named import templates for `§name/rest` forms (values relative to package root).
    ///
    /// Example:
    /// ```toml
    /// [paths.templates]
    /// gnu = "../common/gnu"
    /// ```
    /// then `importa ex "§gnu/argv"` resolves under that directory.
    #[serde(default)]
    pub templates: BTreeMap<String, String>,
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
    /// Backend target requested by the package. `None` selects the implicit
    /// portable default (FHIR package + derived FMIR run) per the FHIR
    /// package delivery; `Some("rust")` keeps the explicit Rust/Cargo route.
    /// `faber init` intentionally leaves this unset so fresh packages are
    /// portable by default.
    #[serde(default)]
    pub target: Option<String>,

    /// Backend targets supported by a library package.
    #[serde(default)]
    pub targets: Vec<String>,

    /// Package output kind; currently only binary crates are supported.
    #[serde(default = "default_build_kind")]
    pub kind: String,

    /// Generated Rust struct-field spelling policy.
    #[serde(default)]
    #[allow(dead_code)]
    // manifest compatibility: accepted even when the Rust target leaf is disabled
    pub rust_field_names: ManifestRustFieldNames,
}

/// `[locale]` metadata for package code-locale selection.
///
/// TODO(locale-rename): LEGACY — type name `ManifestReader` and any remaining
/// `[reader]` prose should go with `LEGACY_READER_MANIFEST_ALIAS` (see field
/// alias on [`FaberManifest::locale`]).
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManifestReader {
    /// Locale id such as `en`, `la`, `th-TH`, or `zh-Hans`.
    pub locale: Option<String>,

    /// Optional locale pack path relative to the package root.
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

impl Default for ManifestPaths {
    fn default() -> Self {
        Self {
            source: default_source_path(),
            entry: None,
            library_home: None,
            templates: BTreeMap::new(),
        }
    }
}

impl Default for ManifestBuild {
    fn default() -> Self {
        Self {
            target: None,
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

fn default_build_kind() -> String {
    "bin".to_owned()
}

/// Whether a [`Target`] has a package build route the product can honour.
///
/// `MirStepper` (the internal MIR interpreter lane) and `MirAmd` (an
/// internal emit-family probe) are compiler-internal targets with no
/// `-t` spelling on the product CLI, so the manifest rejects them by name
/// instead of accepting a build nothing can execute.
fn is_manifest_build_target(target: Target) -> bool {
    !matches!(target, Target::MirStepper | Target::MirAmd)
}

/// The accepted `[build] target` spellings, in [`Target::ALL`] order — the
/// same vocabulary the product CLI accepts for `-t`, derived from the
/// variant universe instead of a second hand-typed list.
fn manifest_build_target_spellings() -> String {
    Target::ALL
        .iter()
        .copied()
        .filter(|target| is_manifest_build_target(*target))
        .map(|target| target.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Map a manifest `[build] target` value (or `None` for the implicit portable
/// default) to a compiler [`Target`].
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn manifest_build_target(target: Option<&str>, path: &Path) -> Result<Target, Box<Diagnostic>> {
    let Some(raw) = target.map(str::trim) else {
        return Ok(Target::HirFhir);
    };
    // `typescript` predates the canonical `ts` spelling; manifests already in
    // the wild keep parsing.
    let spelling = if raw == "typescript" { "ts" } else { raw };
    match Target::from_spelling(spelling) {
        None => Err(Box::new(
            crate::package_diagnostic_error(format!(
                "faber.toml build.target '{raw}' is not a build target; use one of: {}",
                manifest_build_target_spellings()
            ))
            .with_file(path.display().to_string())
            .with_arg("issue", "package_build_target_unsupported")
            .with_arg("target", raw.to_owned())
            .with_arg("accepted", manifest_build_target_spellings()),
        )),
        Some(target) if is_manifest_build_target(target) => Ok(target),
        // Only `MirStepper` / `MirAmd` reach this arm (the guard above takes
        // every eligible target).
        Some(internal) => Err(Box::new(
            crate::package_diagnostic_error(format!(
                "faber.toml build.target '{internal}' is an internal compiler target with no \
                 package build route; use one of: {}",
                manifest_build_target_spellings()
            ))
            .with_file(path.display().to_string())
            .with_arg("issue", "package_build_target_internal")
            .with_arg("target", internal.to_string())
            .with_arg("accepted", manifest_build_target_spellings()),
        )),
    }
}

/// Reject `[target.<key>]` tables whose key is not an accepted target
/// spelling.
///
/// Target tables are looked up by the canonical spelling of the target being
/// emitted, so a table under any other key is never read: its bindings would
/// load as empty and every bound call would fail as unbound with nothing
/// pointing at the manifest. The diagnostic names the key and lists the
/// accepted spellings (the `[build] target` vocabulary). No alias is accepted.
///
/// # Errors
/// Returns the first unknown `[target.<key>]` table as a structured
/// diagnostic.
pub fn validate_target_tables(
    targets: &BTreeMap<String, ManifestTarget>,
    path: &Path,
) -> Result<(), Box<Diagnostic>> {
    let Some(key) = targets
        .keys()
        .find(|key| !Target::from_spelling(key).is_some_and(is_manifest_build_target))
    else {
        return Ok(());
    };
    Err(Box::new(
        crate::package_diagnostic_error(format!(
            "faber.toml [target.{key}] is not a target; use one of: {}",
            manifest_build_target_spellings()
        ))
        .with_file(path.display().to_string())
        .with_arg("issue", "package_target_table_unknown")
        .with_arg("target", key.clone())
        .with_arg("accepted", manifest_build_target_spellings()),
    ))
}

/// Read and deserialize the compiler projection of a `faber.toml` manifest.
///
/// Product-only sections are tolerated (they are faber's product view);
/// unknown fields inside the declared compiler tables still fail closed.
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn read_manifest(path: &Path) -> Result<FaberManifest, Box<Diagnostic>> {
    let source =
        fs::read_to_string(path).map_err(|err| Box::new(Diagnostic::io_error(path, &err)))?;
    toml::from_str::<FaberManifest>(&source).map_err(|err| {
        Box::new(
            crate::package_diagnostic_error(format!("invalid faber.toml manifest: {err}"))
                .with_file(path.display().to_string())
                .with_arg("issue", "invalid_package_manifest"),
        )
    })
}

#[cfg(test)]
#[path = "manifest_test.rs"]
mod tests;
