//! Product view of the package manifest (`faber.toml`).
//!
//! `RP2a`: the compiler view (package identity, source paths, library identity,
//! build target, locale, dependencies, lock-resolved inputs) moved to
//! `radix-program`. This module keeps the product-only manifest
//! interpretation — `[device]`, `[product]`, `[dispatch]`,
//! product emit — plus the strict one-pass parse (`deny_unknown_fields`)
//! that discovery and product flows validate against (goal §Manifest
//! boundary). Compiler-view types and target selection are consumed directly
//! from `radix-program`; this module retains only the product projection.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use radix::module::codegen::Target;
use radix::module::diagnostics::Diagnostic;
use serde::Deserialize;

// DDPP1-U2 (C2 feature isolation): the shared route-selection request type.
use radix::program::library::is_valid_provider_segment;
use radix::program::manifest::{
    ManifestBuild, ManifestLibrary, ManifestPackage, ManifestPaths, ManifestReader, ManifestTarget,
    manifest_build_target, validate_target_tables,
};
#[cfg(feature = "runner")]
use radix::program::mir::DeviceSelection;

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

    /// Device settings used by the package runner (device selection, N1.1).
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

    /// Check-command settings (`[check]`): the complexity warning budget.
    #[serde(default)]
    pub check: ManifestCheck,

    /// Format-command settings (`[format]`): the opt-in wrap width.
    #[serde(default)]
    pub format: ManifestFormat,
}

/// The `faber run --device` / `[device] device` value set.
///
/// `reference` and `cpu` are explicit choices: `reference` lowers kernels as
/// ordinary functions and runs them single-threaded on the MIR runner; `cpu`
/// runs them on the CPU device session by `[device] threads` / `--threads`
/// workers. Neither is ever substituted for another choice.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceChoice {
    /// Resolve against host capability probes; fail closed when the machine
    /// admits zero or more than one device.
    Auto,
    /// Select the reference route explicitly: plain sequential code on the
    /// MIR runner; the oracle device results are checked against.
    Reference,
    /// Select the CPU device explicitly: processor cores as one device, the
    /// kernels run by `--threads` workers (split over `--partitions` core
    /// sets) through the device session.
    Cpu,
    /// Select Metal explicitly; never silently falls back.
    Metal,
    /// Select CUDA explicitly; never silently falls back.
    Cuda,
}

impl DeviceChoice {
    /// Parse a choice from its stable spelling.
    #[must_use]
    pub fn from_spelling(spelling: &str) -> Option<Self> {
        match spelling {
            "auto" => Some(Self::Auto),
            "reference" => Some(Self::Reference),
            "cpu" => Some(Self::Cpu),
            "metal" => Some(Self::Metal),
            "cuda" => Some(Self::Cuda),
            _ => None,
        }
    }

    /// The CLI spelling of the emit target whose device route this choice
    /// launches under `faber run`, or `None` when the choice runs on the
    /// interpreter alone. Keyed by spelling so the product crate names no
    /// backend variant (open-core boundary).
    ///
    /// `faber run` takes no `--target` and executes no emitted artifact: it
    /// interprets source. A device choice is the only way a target's route
    /// is reached from it (`metal` launches the Metal route, `cuda` the
    /// NVVM/PTX route owned by the LLVM target). `faber targets` derives its
    /// `run` column from this match, so the report cannot claim a target
    /// that `faber run` has no way to select; the match is exhaustive, so a
    /// new choice must decide here.
    #[must_use]
    pub const fn run_target_spelling(self) -> Option<&'static str> {
        match self {
            Self::Auto | Self::Reference | Self::Cpu => None,
            Self::Metal => Some("metal-text"),
            Self::Cuda => Some("llvm-text"),
        }
    }

    /// Stable command and diagnostic spelling.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Reference => "reference",
            Self::Cpu => "cpu",
            Self::Metal => "metal",
            Self::Cuda => "cuda",
        }
    }

    /// The route-selection request for this choice.
    #[must_use]
    #[cfg(feature = "runner")]
    pub const fn selection(self) -> DeviceSelection {
        match self {
            Self::Auto => DeviceSelection::Auto,
            Self::Reference => DeviceSelection::Reference,
            Self::Cpu => DeviceSelection::Cpu,
            Self::Metal => DeviceSelection::Metal,
            Self::Cuda => DeviceSelection::Cuda,
        }
    }
}

/// `[device]` metadata for package runner device selection (N1.1), the
/// S1-6 vertical-slice host inputs, and the S5-U5b declared training step
/// count.
///
/// Mirrors `[locale] locale` / `[build] target`: the manifest key is the CLI
/// flag name. The package-level default is overridden by the CLI `--device`
/// flag; both are overridden by the portable default `auto`.
#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct ManifestDevice {
    /// Device selection default: `"auto"` | `"reference"` | `"cpu"` | `"metal"` | `"cuda"`.
    /// `None` selects the portable default `auto`.
    #[serde(default)]
    pub device: Option<String>,

    /// Retired spelling of [`Self::device`]. Parsed only so [`read_manifest`]
    /// can refuse it with a diagnostic naming the new key; never read
    /// otherwise.
    #[serde(default)]
    pub backend: Option<toml::Value>,

    /// Host input values for the package's device-program input buffers
    /// (S1-6 vertical slice). Keys are the kernel functions' buffer names
    /// (parameter names); values are flat f32 element lists, row-major.
    /// Carried into the packaged FMIR image's canonical device payload so
    /// the ordinary `faber run --device <metal|cuda>` command can copy them
    /// in at launch.
    #[serde(default)]
    pub inputs: BTreeMap<String, Vec<f64>>,

    /// Declared training step count of the package's `RepeatingStep` device
    /// program (S5-U5b). Defaults to 100 when absent; validated against the
    /// source training loop's constant bound at construction (a mismatch
    /// fails closed). `None` selects the portable default.
    #[serde(default)]
    pub steps: Option<u32>,

    /// Worker count of the `cpu` device (gpu-reset rule 16.9): a positive
    /// integer. It applies when the run selects `cpu`; `--threads N` wins
    /// over it, and without either the device uses half the available cores,
    /// at least one. Valid only when `device` is absent or `"cpu"`. Held as a
    /// raw value so a negative or non-integer count is refused with a
    /// diagnostic naming this key ([`manifest_device_threads`]).
    #[serde(default)]
    pub threads: Option<toml::Value>,

    /// Path to NVIDIA `libdevice.10.bc`, linked into CUDA modules that call
    /// libdevice math. A relative path is resolved against the directory of
    /// `faber.toml`. When absent the library is located from `CUDA_HOME`,
    /// `CUDA_PATH`, then the standard CUDA installation (those variables only
    /// locate the toolkit; they never configure the build).
    #[serde(default)]
    pub cuda_libdevice: Option<String>,

    /// The kernel safe switch (operator ruling 2026-10-28): when true, every
    /// kernel runs at the default tier whatever its `@ kernel relaxed` or
    /// `@ kernel unsafe` annotation says. It can only lower a tier. `faber
    /// run --safe` and `faber build --safe` set it for one command; no
    /// environment variable does.
    #[serde(default)]
    pub safe: bool,
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

/// `[dispatch]` package policy for native host providers.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManifestDispatch {
    /// Explicit provider additions; route analysis still supplies inferred
    /// providers, so this list never silently removes a required family.
    pub providers: Vec<String>,
}

/// `[check]` settings for the package check command (product view only).
///
/// The `radix-program` compiler projection tolerates `[check]` as an
/// unknown product table; only this view declares and validates its shape.
/// Layering per key: CLI `--complexity-budget` / `--kernel-complexity-budget`
/// beats the same-named key here, which beats the driver's baked default.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct ManifestCheck {
    /// Cyclomatic-complexity warning budget for `faber check`; `None`
    /// leaves the baked driver default in place.
    pub complexity_budget: Option<u32>,

    /// Advisory kernel-entry complexity warning budget for `faber check`
    /// (`@ nucleum` functions); `None` leaves the baked kernel default in
    /// place. Warns only; never gates kernel admission.
    pub kernel_complexity_budget: Option<u32>,
}

/// `[format]` settings for `faber format` (product view only).
///
/// The `radix-program` compiler projection tolerates `[format]` as an unknown
/// product table; only this view declares and validates its shape. There is
/// no built-in width: with neither the flag nor this key, output is not
/// wrapped. Layering: CLI `--width` beats `width` here.
#[derive(Debug, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ManifestFormat {
    /// Wrap width in display columns for `faber format`; `None` means no
    /// wrapping. Held as a raw value so a zero, negative or non-integer width
    /// is refused with a diagnostic naming this key ([`manifest_format_width`]).
    pub width: Option<toml::Value>,
}

macro_rules! product_path_default {
    ($name:ident, $value:literal) => {
        fn $name() -> String {
            $value.to_owned()
        }
    };
}

product_path_default!(default_product_out, "dist");
product_path_default!(default_product_templates, "pages");
product_path_default!(default_product_styles, "styles");
product_path_default!(default_product_public, "public");
product_path_default!(default_product_assets_manifest, "assets.json");
product_path_default!(default_product_controllers_json, "controllers.json");

/// Read the manifest governing an input path. `Ok(None)` only when no package
/// layout is discoverable or no `faber.toml` exists; a manifest that exists but
/// cannot be read is an error (fail closed).
fn input_manifest(
    input: &Path,
) -> Result<Option<(FaberManifest, std::path::PathBuf)>, Box<Diagnostic>> {
    let Ok(layout) = super::discover_build_layout(input) else {
        return Ok(None);
    };
    if !layout.manifest_path.exists() {
        return Ok(None);
    }
    let manifest = read_manifest(&layout.manifest_path)?;
    Ok(Some((manifest, layout.manifest_path)))
}

/// The one resolver for "what `Target` does this package's `faber.toml`
/// declare?", shared by `faber build`, `faber check`, `faber check-library`
/// and the executed lane. `Ok(None)` means there is no manifest, and each
/// caller then applies its own named no-manifest default. A manifest that
/// names an unreadable or unsupported target is always an error: the declared
/// target is never silently ignored in favor of a default. A manifest naming
/// no target resolves to the implicit portable default.
///
/// # Errors
/// Fails when a discoverable manifest exists but cannot be read, or when it
/// names an unsupported build target.
pub fn manifest_package_build_target(input: &Path) -> Result<Option<Target>, Box<Diagnostic>> {
    let Some((manifest, path)) = input_manifest(input)? else {
        return Ok(None);
    };
    manifest_build_target(manifest.build.target.as_deref(), &path).map(Some)
}

/// Resolve the manifest `[check]` section for an input path.
///
/// `None` when no package layout is discoverable, no manifest exists, or
/// the product-view parse fails — the caller then applies the baked
/// defaults, and the strict product-view validation seam reports the parse
/// failure (fail closed there, not here). Per-budget layering is resolved
/// by the caller: CLI flag beats the section key, which beats the baked
/// default.
pub(crate) fn manifest_check_section(input: &Path) -> Option<ManifestCheck> {
    // Same discovery + read seam as [`input_manifest`]; a read failure here
    // degrades to `None` so the caller applies the baked defaults.
    input_manifest(input)
        .ok()
        .flatten()
        .map(|(manifest, _)| manifest.check)
}

/// The `[locale] locale` a package manifest declares for an input path.
///
/// `None` when no package layout is discoverable, no manifest exists, the
/// manifest cannot be read, or it names no locale: the caller applies the
/// product default. Reference lookup (`faber explain`) uses this to follow the
/// current package's reader locale without requiring a package.
pub(crate) fn manifest_locale(input: &Path) -> Option<String> {
    input_manifest(input)
        .ok()
        .flatten()
        .and_then(|(manifest, _)| manifest.locale.locale)
        .map(|locale| locale.trim().to_owned())
        .filter(|locale| !locale.is_empty())
}

/// Map a manifest `[format] width` value to a wrap width. `None` when the key
/// is absent: no wrapping. A value that is not a positive integer fails closed
/// (never silently ignored).
///
/// # Errors
/// Fails when the width is zero, negative, not an integer, or above `u32::MAX`.
pub fn manifest_format_width(
    format: &ManifestFormat,
    path: &Path,
) -> Result<Option<std::num::NonZeroU32>, Box<Diagnostic>> {
    let Some(value) = format.width.as_ref() else {
        return Ok(None);
    };
    let Some(width) = value
        .as_integer()
        .and_then(|width| u32::try_from(width).ok())
        .and_then(std::num::NonZeroU32::new)
    else {
        return Err(Box::new(
            crate::package_diagnostic_error(format!(
                "faber.toml format.width must be a positive integer, found {value}"
            ))
            .with_file(path.display().to_string())
            .with_arg("issue", "package_format_width_invalid")
            .with_arg("key", "format.width")
            .with_arg("width", value.to_string()),
        ));
    };
    Ok(Some(width))
}

/// Resolve the effective `faber format` wrap width for an input path: CLI
/// `--width N` > manifest `[format] width` > `None` (no wrapping; there is no
/// built-in default). The same precedence as `--device`. An invalid manifest
/// width fails closed even when the flag would win over it.
///
/// # Errors
/// Fails when the discovered manifest cannot be read, or when its `[format]
/// width` value is invalid ([`manifest_format_width`]).
pub fn resolve_format_width(
    cli: Option<std::num::NonZeroU32>,
    input_path: &Path,
) -> Result<Option<std::num::NonZeroU32>, Box<Diagnostic>> {
    let Some((manifest, path)) = input_manifest(input_path)? else {
        return Ok(cli);
    };
    let manifest_width = manifest_format_width(&manifest.format, &path)?;
    Ok(cli.or(manifest_width))
}

/// Map a manifest `[device] device` value to a device choice (N1.1). `None`
/// when the key is absent — the caller applies the portable default `auto`.
/// An unsupported spelling fails closed with a structured diagnostic (never
/// silently ignored).
///
/// # Errors
/// Fails when the manifest names a device spelling outside the supported set
/// (`auto`, `reference`, `cpu`, `metal`, `cuda`).
#[cfg(feature = "runner")]
pub fn manifest_device_choice(
    device: Option<&str>,
    path: &Path,
) -> Result<Option<DeviceChoice>, Box<Diagnostic>> {
    match device.map(str::trim) {
        None => Ok(None),
        Some(spelling) => match DeviceChoice::from_spelling(spelling) {
            Some(choice) => Ok(Some(choice)),
            None => Err(Box::new(
                crate::package_diagnostic_error(format!(
                    "faber.toml device.device '{spelling}' is not supported; use 'auto', 'reference', 'cpu', 'metal', or 'cuda'"
                ))
                .with_file(path.display().to_string())
                .with_arg("issue", "package_device_unsupported")
                .with_arg("key", "device.device")
                .with_arg("device", spelling.to_owned()),
            )),
        }
    }
}

/// Resolve the effective device selection for a route (N1.1): CLI `--device` >
/// manifest `[device] device` > default `auto`. One precedence for every
/// route that takes `--device` (`faber run`, `faber test`). An invalid
/// manifest value fails closed with a structured diagnostic (never silently
/// ignored).
///
/// # Errors
/// Fails when the discovered manifest cannot be read, or when its `[device]
/// device` value is unsupported.
#[cfg(feature = "runner")]
pub fn resolve_device_selection(
    cli: Option<DeviceChoice>,
    input_path: &Path,
) -> Result<DeviceSelection, Box<Diagnostic>> {
    if let Some(device) = cli {
        return Ok(device.selection());
    }
    let Ok(layout) = super::discover_build_layout(input_path) else {
        return Ok(DeviceSelection::Auto);
    };
    if !layout.manifest_path.exists() {
        return Ok(DeviceSelection::Auto);
    }
    let manifest = read_manifest(&layout.manifest_path)?;
    let manifest_device =
        manifest_device_choice(manifest.device.device.as_deref(), &layout.manifest_path)?;
    Ok(manifest_device.map_or(DeviceSelection::Auto, DeviceChoice::selection))
}

/// Map a manifest `[device] threads` value to a CPU worker count (gpu-reset
/// rule 16.9). `None` when the key is absent: the caller applies the device
/// default. A value that is not a positive integer, or a count beside a
/// `[device] device` other than `cpu`, fails closed (never silently ignored).
///
/// # Errors
/// Fails when the count is zero, negative, not an integer or above `u32::MAX`,
/// when `[device] device` is unsupported, or when it names a device other
/// than `cpu`.
#[cfg(feature = "runner")]
pub fn manifest_device_threads(
    device: &ManifestDevice,
    path: &Path,
) -> Result<Option<std::num::NonZeroU32>, Box<Diagnostic>> {
    let Some(value) = device.threads.as_ref() else {
        return Ok(None);
    };
    let Some(count) = value
        .as_integer()
        .and_then(|count| u32::try_from(count).ok())
        .and_then(std::num::NonZeroU32::new)
    else {
        return Err(Box::new(
            crate::package_diagnostic_error(format!(
                "faber.toml device.threads must be a positive integer, found {value}"
            ))
            .with_file(path.display().to_string())
            .with_arg("issue", "package_device_threads_invalid")
            .with_arg("key", "device.threads")
            .with_arg("threads", value.to_string()),
        ));
    };
    if let Some(choice) = manifest_device_choice(device.device.as_deref(), path)?
        && choice != DeviceChoice::Cpu
    {
        let spelling = choice.selection().spelling();
        return Err(Box::new(
            crate::package_diagnostic_error(format!(
                "faber.toml device.threads applies only to the cpu device, but device.device is '{spelling}'"
            ))
            .with_file(path.display().to_string())
            .with_arg("issue", "package_device_threads_requires_cpu")
            .with_arg("key", "device.threads")
            .with_arg("device", spelling),
        ));
    }
    Ok(Some(count))
}

/// The `[device] cuda_libdevice` path of a manifest, resolved against the
/// manifest's directory. `None` when the key is absent.
///
/// # Errors
/// Fails when the key is present but empty.
pub fn manifest_device_cuda_libdevice(
    device: &ManifestDevice,
    manifest_path: &Path,
) -> Result<Option<std::path::PathBuf>, Box<Diagnostic>> {
    let Some(value) = device.cuda_libdevice.as_deref() else {
        return Ok(None);
    };
    if value.trim().is_empty() {
        return Err(Box::new(
            crate::package_diagnostic_error("faber.toml device.cuda_libdevice must not be empty")
                .with_file(manifest_path.display().to_string())
                .with_arg("issue", "package_device_cuda_libdevice_empty")
                .with_arg("key", "device.cuda_libdevice"),
        ));
    }
    let path = std::path::PathBuf::from(value);
    if path.is_absolute() {
        return Ok(Some(path));
    }
    let base = manifest_path.parent().unwrap_or_else(|| Path::new("."));
    Ok(Some(base.join(path)))
}

/// Whether kernels run at the default tier for the package that owns
/// `input_path`: the `--safe` flag, or `[device] safe = true` in its
/// `faber.toml`. A manifest that cannot be read counts as not set here; the
/// package config reports it.
#[must_use]
pub fn resolve_safe_kernels(cli_safe: bool, input_path: &Path) -> bool {
    cli_safe
        || input_manifest(input_path)
            .ok()
            .flatten()
            .is_some_and(|(manifest, _)| manifest.device.safe)
}

/// Resolve the explicit CUDA libdevice path for the package that owns
/// `input_path`: the `[device] cuda_libdevice` key of its discovered
/// `faber.toml`. `None` when there is no manifest or the key is absent.
///
/// # Errors
/// Fails when the discovered manifest cannot be read or the key is invalid.
pub fn resolve_cuda_libdevice(
    input_path: &Path,
) -> Result<Option<std::path::PathBuf>, Box<Diagnostic>> {
    let Ok(layout) = super::discover_build_layout(input_path) else {
        return Ok(None);
    };
    if !layout.manifest_path.exists() {
        return Ok(None);
    }
    let manifest = read_manifest(&layout.manifest_path)?;
    manifest_device_cuda_libdevice(&manifest.device, &layout.manifest_path)
}

/// Resolve the CPU device worker count for a route (gpu-reset rule 16.9):
/// CLI `--threads N` > manifest `[device] threads` > `None` (the device
/// default, half the available cores). One precedence for `faber run` and
/// `faber test`. The flag is valid only when `selection` is the CPU device
/// and fails closed otherwise (`E_THREADS_REQUIRES_CPU`, need); the
/// manifest key is a package default that applies only when the run selects
/// `cpu`, so `--device reference` over a package that sets it runs unchanged.
///
/// # Errors
/// Fails when the flag is given for a selection other than `cpu`, when the
/// discovered manifest cannot be read, or when its `[device] threads` value is
/// invalid ([`manifest_device_threads`]).
#[cfg(feature = "runner")]
pub fn resolve_cpu_threads(
    cli: Option<u32>,
    selection: DeviceSelection,
    input_path: &Path,
) -> Result<Option<std::num::NonZeroU32>, Box<Diagnostic>> {
    if let Some(threads) = cli {
        if selection != DeviceSelection::Cpu {
            return Err(Box::new(
                crate::exit_status::usage_diagnostic_error(
                    "`--threads` is valid only with `--device cpu`",
                )
                .with_arg("issue", "E_THREADS_REQUIRES_CPU")
                .with_arg("device", selection.spelling()),
            ));
        }
        return Ok(std::num::NonZeroU32::new(threads));
    }
    if selection != DeviceSelection::Cpu {
        return Ok(None);
    }
    let Ok(layout) = super::discover_build_layout(input_path) else {
        return Ok(None);
    };
    if !layout.manifest_path.exists() {
        return Ok(None);
    }
    let manifest = read_manifest(&layout.manifest_path)?;
    manifest_device_threads(&manifest.device, &layout.manifest_path)
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
                values
                    .iter()
                    .map(|value| {
                        // cast: manifest inputs are intentionally rounded to the campaign's f32 dtype.
                        *value as f32
                    })
                    .collect(),
            )
        })
        .collect()
}

/// Read and deserialize a `faber.toml` manifest.
///
/// Unknown manifest fields are rejected by the manifest structs so spelling
/// mistakes become diagnostics rather than silently ignored configuration.
///
/// # Errors
/// Fails when the file cannot be read, when the TOML does not parse as a
/// manifest, or when it uses the retired `[device] backend` key.
pub fn read_manifest(path: &Path) -> Result<FaberManifest, Box<Diagnostic>> {
    let source =
        fs::read_to_string(path).map_err(|err| Box::new(Diagnostic::io_error(path, &err)))?;
    let manifest = toml::from_str::<FaberManifest>(&source).map_err(|err| {
        Box::new(
            crate::package_diagnostic_error(format!("invalid faber.toml manifest: {err}"))
                .with_file(path.display().to_string())
                .with_arg("issue", "invalid_package_manifest"),
        )
    })?;
    if manifest.device.backend.is_some() {
        return Err(Box::new(
            crate::package_diagnostic_error(
                "faber.toml [device] backend was renamed: use `device = \"auto\" | \"cpu\" | \"metal\" | \"cuda\"` under [device]",
            )
            .with_file(path.display().to_string())
            .with_arg("issue", "package_device_backend_renamed")
            .with_arg("replacement_key", "device"),
        ));
    }
    Ok(manifest)
}

/// Read the validated manifest for a package spec, if the package is
/// manifest-backed.
///
/// Legacy manifestless inputs (file or directory without `faber.toml`) return
/// `Ok(None)`. A spec whose manifest was read and validated during discovery
/// must keep its manifest: a later missing or unreadable manifest is a
/// diagnostic, never a silent `None` default (FBR-P1-001).
pub(super) fn manifest_for_spec(
    spec: &radix::program::discovery::PackageSpec,
) -> Result<Option<FaberManifest>, Box<Diagnostic>> {
    if !spec.manifest_backed {
        return Ok(None);
    }
    let Some(path) = radix::program::frontmatter::manifest_path_for_spec(spec) else {
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
    validate_package_identity(manifest, path)?;

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

    // N1.1: the `[device] device` selection default is validated at package
    // load so an unsupported spelling fails closed early, never silently
    // ignored.
    #[cfg(feature = "runner")]
    manifest_device_choice(manifest.device.device.as_deref(), path)?;
    // Rule 16.9: the `[device] threads` worker count is validated at package
    // load too, so a bad count fails before any route runs.
    #[cfg(feature = "runner")]
    manifest_device_threads(&manifest.device, path)?;

    // S5-U5b: the `[device] steps` declared training step count is validated
    // at package load — a zero count is a contradiction (a RepeatingStep
    // program must drive at least one step) and fails closed early, never
    // silently ignored.
    if let Some(steps) = manifest.device.steps
        && steps == 0
    {
        return Err(Box::new(
            crate::package_diagnostic_error("faber.toml device.steps must be at least 1")
                .with_file(path.display().to_string())
                .with_arg("issue", "package_device_steps_zero")
                .with_arg("steps", steps.to_string()),
        ));
    }

    manifest_device_cuda_libdevice(&manifest.device, path)?;

    manifest_format_width(&manifest.format, path)?;

    if let Some(product) = &manifest.product {
        validate_product(product, path)?;
    }

    if let Some(locale) = manifest.locale.locale.as_deref()
        && locale.trim().is_empty()
    {
        return Err(Box::new(
            crate::package_diagnostic_error("faber.toml locale must not be empty")
                .with_file(path.display().to_string()),
        ));
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

    validate_dependencies_and_dispatch(manifest, path)?;

    Ok(())
}

fn validate_dependencies_and_dispatch(
    manifest: &FaberManifest,
    path: &Path,
) -> Result<(), Box<Diagnostic>> {
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
        if provider.trim().is_empty() || !is_valid_provider_segment(provider) {
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
        if let Some(bindings) = config.bindings.as_deref()
            && bindings.trim().is_empty()
        {
            return Err(Box::new(
                crate::package_diagnostic_error(
                    "faber.toml target bindings path must not be empty",
                )
                .with_file(path.display().to_string())
                .with_arg("issue", "invalid_target_bindings"),
            ));
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

fn validate_package_identity(manifest: &FaberManifest, path: &Path) -> Result<(), Box<Diagnostic>> {
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
                .with_arg("source", unsupported.to_owned())
                .with_arg("supported_source_roots", "src,.")
                .with_arg("reason", "custom_source_root_usage_contract_pending"),
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
