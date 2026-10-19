//! The one library binding-manifest schema (`[target.<name>].bindings` file).
//!
//! Every consumer of a binding manifest (library verification, the Rust
//! library linker, the TS emitter) reads this single type. The compiler names
//! no library vocabulary: keys are opaque `provider:module.name` routes and
//! values are opaque target spellings that the manifest author owns.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use radix::module::diagnostics::Diagnostic;
use serde::Deserialize;

/// Parsed binding manifest.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BindingManifest {
    /// Declared function route (`provider:module.name`) to its target binding.
    #[serde(default)]
    pub(crate) functions: BTreeMap<String, FunctionBinding>,

    /// Declared type route (`provider:module.Name`) to its opaque target
    /// spelling.
    #[serde(default)]
    pub(crate) types: BTreeMap<String, TypeBinding>,

    /// Declared annotation-contract route (`provider:module.Name`) to its
    /// opaque target spelling (SPIKE: stage words and IO attributes for the
    /// WGSL graphics path; the compiler core reads none of them).
    #[serde(default)]
    pub(crate) annotations: BTreeMap<String, TypeBinding>,

    /// Package-level default runtime shim.
    #[serde(default)]
    pub(crate) shim: Option<ShimBinding>,

    /// Per-stem shim overrides (`[shims.<stem>]`) for packages hosting more
    /// than one binding module. Stems without an entry use `shim`.
    #[serde(default)]
    pub(crate) shims: BTreeMap<String, ShimBinding>,
}

/// One function binding: the target symbol plus optional contract notes.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FunctionBinding {
    pub(crate) symbol: String,

    /// Optional opener-argument note (author-owned documentation text).
    // WHY: schema-owned contract notes; read by contract tooling outside this crate.
    #[allow(dead_code)]
    #[serde(default)]
    pub(crate) opener: Option<String>,

    /// Optional result note (author-owned documentation text).
    #[allow(dead_code)]
    #[serde(default)]
    pub(crate) result: Option<String>,
}

/// One type binding: the opaque target spelling of a declared Faber type.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TypeBinding {
    pub(crate) symbol: String,
}

/// A runtime shim source path, relative to the package root.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ShimBinding {
    pub(crate) path: String,
}

/// Parse manifest text with the unified schema.
///
/// # Errors
/// Returns the TOML error when the text does not match the schema.
pub(crate) fn parse_binding_manifest(source: &str) -> Result<BindingManifest, toml::de::Error> {
    toml::from_str::<BindingManifest>(source)
}

/// Read and parse the manifest at `path`.
///
/// # Errors
/// Returns an I/O diagnostic or an `invalid_binding_manifest` diagnostic.
pub(crate) fn read_binding_manifest(path: &Path) -> Result<BindingManifest, Vec<Diagnostic>> {
    let source = fs::read_to_string(path).map_err(|err| vec![Diagnostic::io_error(path, &err)])?;
    parse_binding_manifest(&source).map_err(|err| {
        vec![
            crate::package_diagnostic_error(format!("invalid binding manifest: {err}"))
                .with_file(path.display().to_string())
                .with_arg("issue", "invalid_binding_manifest"),
        ]
    })
}
