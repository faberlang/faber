use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

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

    /// Device settings used by the package runner (backend selection, N1.1).
    #[serde(default)]
    pub device: ManifestDevice,

    /// Product packaging recipe owned by faber, not by Radix codegen targets.
    #[serde(default)]
    pub product: Option<ManifestProduct>,

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

    /// Explicit additions to the native host provider selection.
    #[serde(default)]
    pub dispatch: ManifestDispatch,
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

/// `[device]` metadata for package runner backend selection (N1.1), the
/// S1-6 vertical-slice host inputs, and the S5-U5b declared training step
/// count.
///
/// Mirrors `[build] target` for `-t/--target`: the package-level default is
/// overridden by the CLI `--backend` flag; both are overridden by the
/// portable default `auto`.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ManifestDevice {
    /// Backend selection default: `"auto"` | `"metal"` | `"cuda"`.
    /// `None` selects the portable default `auto`.
    #[serde(default)]
    pub backend: Option<String>,

    /// Host input values for the package's device-program input buffers
    /// (S1-6 vertical slice). Keys are the kernel functions' buffer names
    /// (parameter names); values are flat f32 element lists, row-major.
    /// Carried into the packaged FMIR image's canonical device payload so
    /// the ordinary `faber run --backend <metal|cuda>` command can copy them
    /// in at launch.
    #[serde(default)]
    pub inputs: BTreeMap<String, Vec<f64>>,

    /// Declared training step count of the package's RepeatingStep device
    /// program (S5-U5b). Defaults to 100 when absent; validated against the
    /// source training loop's constant bound at construction (a mismatch
    /// fails closed). `None` selects the portable default.
    #[serde(default)]
    pub steps: Option<u32>,
}

/// Configuration for shader artifact packaging.
///
/// When `shaders.source` is set, `faber build` copies pre-compiled shader
/// artifacts from that directory into `dist/generated/` and includes them
/// in the product manifest. This supports the non-goal of requiring the
/// MIR→WGSL compiler pass — reference artifacts produced by U1 are
/// checked into version control under `src/shaders/test-data/` and
/// packaged at build time.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestProductShaders {
    /// Directory (relative to package root) containing pre-compiled shader
    /// artifacts: `kernel.wgsl` and `reflection.json`.
    pub source: String,
}

/// `[product]` browser application packaging metadata.
///
/// This selects a faber-owned product recipe. It deliberately does not add a
/// Radix `web` backend: browser controllers emit TypeScript and packaging owns
/// assets plus product manifests.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestProduct {
    pub kind: ManifestProductKind,
    pub emit: ManifestProductEmit,
    #[serde(default = "default_product_out")]
    pub out: String,
    #[serde(default = "default_product_templates")]
    pub templates: String,
    #[serde(default = "default_product_styles")]
    pub styles: String,
    #[serde(default = "default_product_public")]
    pub public: String,
    #[serde(default = "default_product_assets_manifest")]
    pub assets_manifest: String,
    #[serde(default = "default_product_controllers_json")]
    pub controllers_json: String,
    /// Optional shader artifact packaging config. When present, the build
    /// copies pre-compiled WGSL + reflection from `shaders.source` into
    /// `dist/generated/` and records them in the product manifest (stage 2+).
    #[serde(default)]
    pub shaders: Option<ManifestProductShaders>,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ManifestProductKind {
    BrowserApp,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
pub enum ManifestProductEmit {
    #[serde(rename = "typescript")]
    TypeScript,
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

/// `[dispatch]` package policy for native host providers.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManifestDispatch {
    /// Explicit provider additions; route analysis still supplies inferred
    /// providers, so this list never silently removes a required family.
    pub providers: Vec<String>,
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

fn default_product_out() -> String {
    "dist".to_owned()
}

fn default_product_templates() -> String {
    "pages".to_owned()
}

fn default_product_styles() -> String {
    "styles".to_owned()
}

fn default_product_public() -> String {
    "public".to_owned()
}

fn default_product_assets_manifest() -> String {
    "assets.json".to_owned()
}

fn default_product_controllers_json() -> String {
    "controllers.json".to_owned()
}

/// Map a manifest `[build] target` value (or `None` for the implicit portable
/// default) to a compiler [`Target`].
pub(crate) fn manifest_build_target(
    target: Option<&str>,
    path: &Path,
) -> Result<Target, Box<Diagnostic>> {
    match target.map(str::trim) {
        None => Ok(Target::HirFhir),
        Some("rust") => Ok(Target::HirRust),
        Some("fhir") => Ok(Target::HirFhir),
        Some("ts") | Some("typescript") => Ok(Target::HirTypeScript),
        Some("scena") => Ok(Target::MirScena),
        Some("fmir-text") => Ok(Target::MirFmir),
        Some("fmir") => Ok(Target::MirFmirBinary),
        Some("fmir-bin") => Ok(Target::MirFmirBundle),
        Some("llvm-host") => Ok(Target::MirLlvmHost),
        Some(unsupported) => Err(Box::new(
            crate::package_diagnostic_error(format!(
                "faber.toml build.target '{unsupported}' is not supported for package builds"
            ))
            .with_file(path.display().to_string())
            .with_arg("issue", "package_build_target_unsupported")
            .with_arg("target", unsupported.to_owned()),
        )),
    }
}

/// Map a manifest `[device] backend` value to a backend selection request
/// (N1.1). `None` when the key is absent — the caller applies the portable
/// default `auto`. An unsupported spelling fails closed with a structured
/// diagnostic (never silently ignored).
pub(crate) fn manifest_backend_selection(
    backend: Option<&str>,
    path: &Path,
) -> Result<Option<faber::device::DeviceSelection>, Box<Diagnostic>> {
    match backend.map(str::trim) {
        None => Ok(None),
        Some(spelling) => {
            match faber::device::DeviceSelection::from_spelling(spelling) {
                Some(selection) => Ok(Some(selection)),
                None => Err(Box::new(
                    crate::package_diagnostic_error(format!(
                        "faber.toml device.backend '{spelling}' is not supported; use 'auto', 'metal', or 'cuda'"
                    ))
                    .with_file(path.display().to_string())
                    .with_arg("issue", "package_device_backend_unsupported")
                    .with_arg("backend", spelling.to_owned()),
                )),
            }
        }
    }
}

/// Map a manifest `[device] inputs` map to typed f32 host inputs (S1-6
/// vertical slice). `None` when the key is absent. Values are flat f32
/// element lists; f64 manifest values are converted to f32 (the campaign
/// dtype).
#[must_use]
pub(crate) fn manifest_device_inputs(
    inputs: &BTreeMap<String, Vec<f64>>,
) -> BTreeMap<String, Vec<f32>> {
    inputs
        .iter()
        .map(|(name, values)| {
            (
                name.clone(),
                values.iter().map(|value| *value as f32).collect(),
            )
        })
        .collect()
}

/// Read and deserialize a `faber.toml` manifest.
///
/// Unknown manifest fields are rejected by the manifest structs so spelling
/// mistakes become diagnostics rather than silently ignored configuration.
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

/// Read the validated manifest for a package spec, if the package is
/// manifest-backed.
///
/// Legacy manifestless inputs (file or directory without `faber.toml`) return
/// `Ok(None)`. A spec whose manifest was read and validated during discovery
/// must keep its manifest: a later missing or unreadable manifest is a
/// diagnostic, never a silent `None` default (FBR-P1-001).
pub(super) fn manifest_for_spec(
    spec: &super::discovery::PackageSpec,
) -> Result<Option<FaberManifest>, Box<Diagnostic>> {
    if !spec.manifest_backed {
        return Ok(None);
    }
    let Some(path) = super::frontmatter::manifest_path_for_spec(spec) else {
        return Err(Box::new(
            crate::package_diagnostic_error(
                "package manifest faber.toml is missing after discovery validated it",
            )
            .with_file(
                spec.package_root
                    .join(super::MANIFEST_FILE)
                    .display()
                    .to_string(),
            )
            .with_arg("issue", "package_manifest_missing_after_validation"),
        ));
    };
    read_manifest(&path).map(Some)
}

pub(crate) fn validate_manifest(
    manifest: &FaberManifest,
    path: &Path,
) -> Result<(), Box<Diagnostic>> {
    if manifest.package.name.trim().is_empty() {
        return Err(Box::new(
            crate::package_diagnostic_error("faber.toml package.name must not be empty")
                .with_file(path.display().to_string()),
        ));
    }
    if !crate::library::is_valid_provider_segment(&manifest.package.name) {
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

    if let Some(entry) = manifest.paths.entry.as_deref() {
        if entry.trim().is_empty() {
            return Err(Box::new(
                crate::package_diagnostic_error("faber.toml paths.entry must not be empty")
                    .with_file(path.display().to_string()),
            ));
        }
    }

    if let Some(library) = &manifest.library {
        if !crate::library::is_valid_provider_segment(&library.provider) {
            return Err(Box::new(
                crate::package_diagnostic_error(
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
                crate::package_diagnostic_error(format!(
                    "faber.toml build.kind '{kind}' is not supported"
                ))
                .with_file(path.display().to_string())
                .with_arg("issue", "package_build_kind_unsupported")
                .with_arg("kind", kind.to_owned()),
            ));
        }
    }

    // N1.1: the `[device] backend` selection default is validated at package
    // load so an unsupported spelling fails closed early, never silently
    // ignored.
    manifest_backend_selection(manifest.device.backend.as_deref(), path)?;

    // S5-U5b: the `[device] steps` declared training step count is validated
    // at package load — a zero count is a contradiction (a RepeatingStep
    // program must drive at least one step) and fails closed early, never
    // silently ignored.
    if let Some(steps) = manifest.device.steps {
        if steps == 0 {
            return Err(Box::new(
                crate::package_diagnostic_error("faber.toml device.steps must be at least 1")
                    .with_file(path.display().to_string())
                    .with_arg("issue", "package_device_steps_zero")
                    .with_arg("steps", steps.to_string()),
            ));
        }
    }

    if let Some(product) = &manifest.product {
        validate_product(product, path)?;
    }

    if let Some(locale) = manifest.locale.locale.as_deref() {
        if locale.trim().is_empty() {
            return Err(Box::new(
                crate::package_diagnostic_error("faber.toml locale must not be empty")
                    .with_file(path.display().to_string()),
            ));
        }
    }

    if let Some(pack) = manifest.locale.pack.as_deref() {
        if pack.trim().is_empty() {
            return Err(Box::new(
                crate::package_diagnostic_error("faber.toml locale.pack must not be empty")
                    .with_file(path.display().to_string()),
            ));
        }
        if manifest.locale.locale.is_none() {
            // TODO(locale-rename): LEGACY_READER_MANIFEST_ALIAS — drop "legacy
            // [reader]" from this diagnostic when the serde alias is removed.
            return Err(Box::new(
                crate::package_diagnostic_error(
                    "faber.toml locale.pack requires locale (section [locale] or legacy [reader])",
                )
                .with_file(path.display().to_string()),
            ));
        }
    }

    for (name, version) in &manifest.dependencies {
        if name.trim().is_empty() {
            return Err(Box::new(
                crate::package_diagnostic_error("faber.toml [dependencies] key must not be empty")
                    .with_file(path.display().to_string()),
            ));
        }
        if version.trim().is_empty() {
            return Err(Box::new(
                crate::package_diagnostic_error(format!(
                    "faber.toml dependency `{name}` version must be a non-empty exact string"
                ))
                .with_file(path.display().to_string()),
            ));
        }
    }

    for provider in &manifest.dispatch.providers {
        if provider.trim().is_empty() || !crate::library::is_valid_provider_segment(provider) {
            return Err(Box::new(
                crate::package_diagnostic_error(format!(
                    "faber.toml [dispatch].providers entry `{provider}` is invalid"
                ))
                .with_file(path.display().to_string())
                .with_arg("issue", "invalid_dispatch_provider"),
            ));
        }
    }

    if !manifest.dispatch.providers.is_empty() && manifest.build.target.as_deref() != Some("rust") {
        return Err(Box::new(
            crate::package_diagnostic_error(
                "faber.toml [dispatch] is only supported for the Rust package target",
            )
            .with_file(path.display().to_string())
            .with_arg("issue", "dispatch_target_unsupported"),
        ));
    }

    for (target, config) in &manifest.target {
        if target.trim().is_empty() {
            return Err(Box::new(
                crate::package_diagnostic_error("faber.toml [target] key must not be empty")
                    .with_file(path.display().to_string())
                    .with_arg("issue", "invalid_target_table"),
            ));
        }
        if let Some(bindings) = config.bindings.as_deref() {
            if bindings.trim().is_empty() {
                return Err(Box::new(
                    crate::package_diagnostic_error(
                        "faber.toml target bindings path must not be empty",
                    )
                    .with_file(path.display().to_string())
                    .with_arg("issue", "invalid_target_bindings"),
                ));
            }
        }
        if config.host.is_some() && target != "rust" {
            return Err(Box::new(
                crate::package_diagnostic_error(
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
                    crate::package_diagnostic_error(
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

fn validate_product(product: &ManifestProduct, path: &Path) -> Result<(), Box<Diagnostic>> {
    match product.kind {
        // Only BrowserApp exists today; future kind variants will need their
        // own dispatch here rather than falling through silently.
        ManifestProductKind::BrowserApp => {}
    }
    match product.emit {
        // Only TypeScript exists today; future emit variants will need their
        // own dispatch here rather than falling through silently.
        ManifestProductEmit::TypeScript => {}
    }

    validate_product_path("out", &product.out, path)?;
    validate_product_path("templates", &product.templates, path)?;
    validate_product_path("styles", &product.styles, path)?;
    validate_product_path("public", &product.public, path)?;
    validate_product_path("assets_manifest", &product.assets_manifest, path)?;
    validate_product_path("controllers_json", &product.controllers_json, path)?;
    if let Some(shaders) = &product.shaders {
        validate_product_path("shaders.source", &shaders.source, path)?;
    }
    Ok(())
}

fn validate_product_path(field: &str, value: &str, path: &Path) -> Result<(), Box<Diagnostic>> {
    let trimmed = value.trim();
    if trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed == "."
        || trimmed == ".."
        || trimmed
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err(Box::new(
            crate::package_diagnostic_error(format!(
                "faber.toml product.{field} must be a non-empty relative path without traversal"
            ))
            .with_file(path.display().to_string())
            .with_arg("issue", "invalid_product_path")
            .with_arg("field", field.to_owned()),
        ));
    }
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

#[cfg(test)]
#[path = "manifest_test.rs"]
mod tests;
