//! Reusable package-to-Wasm builder (codex-gap Stage 6 U6-D).
//!
//! Emits one WAT module per package unit through the radix package-aware wasm
//! emitter (`radix::mir::emit_wasm_text_probe_package_aware`), converts each
//! module to wasm bytes, and returns an inspectable link manifest plus the
//! entry + sibling byte modules the product host
//! (`faber-host-wasm::WasmRtV1Host::run_package`) instantiates together.
//!
//! INVARIANTS (mirrors the package-to-LLVM builder in `llvm.rs`):
//! - No WAT text concatenation: every package unit is lowered and emitted
//!   independently into its own module (one module per unit).
//! - No secondary import parser: the builder consumes the package graph from
//!   [`analyze_package`] only. The canonical `(product, module_path, item)`
//!   external identities are the radix tool's
//!   [`radix::tool::package_identity_facts_for_path`] facts (the same source
//!   the CLI compile path uses), never a harness-side re-resolution of
//!   imports.
//! - The entry unit keeps the `incipit` export the product host invokes;
//!   every sibling unit's identity-carrying exported functions gain the
//!   canonical `__faber_external_product_…_module_…_func_…` export the
//!   package-aware emitter's `faber_external` imports resolve against
//!   (host `bind_external_imports`: import field `F` binds to the sibling
//!   export `__faber_{F}`).
//!
//! The builder itself does not instantiate or run modules: running is the
//! caller's job via the returned [`PackageWasmLinkManifest`] byte modules and
//! the product host.

use radix::diagnostics::Diagnostic;
use radix::driver::Config;
use std::path::{Path, PathBuf};

use super::compile::{analyze_package, AnalyzedPackage, AnalyzedPackageUnit};

/// Options for a package-to-Wasm build.
#[derive(Debug, Clone)]
pub struct PackageWasmOptions {
    /// Directory that receives one `.wat` module per package unit (the
    /// inspectable artifact; the byte modules travel in the link manifest).
    pub output_dir: PathBuf,
}

impl PackageWasmOptions {
    /// Options with only an output directory.
    #[must_use]
    pub fn new(output_dir: PathBuf) -> Self {
        Self { output_dir }
    }
}

/// One emitted Wasm module for a package unit (one module per unit).
///
/// `allow(dead_code)`: the faber binary reads the manifest fields directly,
/// so fields only the exempla harness (lib consumer) and the product builder
/// read are flagged by the bin target's dead-code analysis.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PackageWasmModule {
    /// Source unit path in the package graph.
    pub unit_path: PathBuf,
    /// Canonical module segments for this unit (not filesystem spelling alone).
    pub module_segments: Vec<String>,
    /// Whether this unit is the package's single entry unit.
    pub is_entry: bool,
    /// The emitted `.wat` module for this unit.
    pub wat_path: PathBuf,
    /// The emitted package-aware WAT source (canonical sibling exports
    /// applied) for this unit.
    pub wat: String,
    /// The compiled wasm bytes for this module (package-aware emission,
    /// canonical sibling exports applied).
    pub bytes: Vec<u8>,
}

/// Inspectable link manifest for a package-to-Wasm build.
///
/// `allow(dead_code)`: the faber binary inlines the package modules, so fields
/// only the exempla harness (lib consumer) and the product builder read are
/// flagged by the bin target's dead-code analysis.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PackageWasmLinkManifest {
    /// One `.wat` path per unit, deterministic order (package analysis order).
    pub modules: Vec<PathBuf>,
    /// The exactly-one module carrying the `incipit` entry export.
    pub entry_module: PathBuf,
    /// Wasm bytes of the entry module (index 0 for the host).
    pub entry_bytes: Vec<u8>,
    /// Wasm bytes of the sibling modules in dependency-first order (the
    /// order `WasmRtV1Host::run_package` instantiates them).
    pub sibling_bytes: Vec<Vec<u8>>,
    /// The module output directory (the package wasm artifact root).
    pub output: PathBuf,
    /// Canonical Faber target layout binary path
    /// (`<package_root>/target/faber/wasm/bin/<product>`; reserved — the
    /// current run path hosts modules rather than producing one linked file).
    pub binary_path: PathBuf,
}

/// Complete package-to-Wasm build result.
///
/// `allow(dead_code)`: the faber binary inlines the package modules, so fields
/// only the exempla harness (lib consumer) and the product builder read are
/// flagged by the bin target's dead-code analysis.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PackageWasmBuild {
    /// Stable product identity used by the external symbol names
    /// (`__faber_external_product_<product>_…`).
    pub product: String,
    /// Source path of the single entry unit.
    pub entry_unit: PathBuf,
    /// One module per package unit, deterministic order.
    pub modules: Vec<PackageWasmModule>,
    /// The inspectable link manifest (entry + sibling byte modules included).
    pub manifest: PackageWasmLinkManifest,
}

/// Build the package-to-Wasm artifact for `input` through the Faber package
/// graph: one module per resolved unit (entry + package siblings) plus an
/// inspectable link manifest. The entry unit is emitted with its `incipit`
/// export; sibling units gain canonical external-symbol exports.
///
/// # Errors
///
/// Returns package analysis diagnostics, or per-unit MIR lowering / Wasm
/// emission diagnostics.
pub fn build_package_wasm(
    config: &Config,
    input: &Path,
    options: &PackageWasmOptions,
) -> Result<PackageWasmBuild, Vec<Diagnostic>> {
    let mut package = analyze_package(config, input)?;
    build_package_wasm_from_graph(&mut package, options)
}

/// Build the package-to-Wasm artifact from an already-analyzed package graph.
///
/// The graph is mutated so each unit's analysis can be attached its canonical
/// package identities and lowered. Unit order is the package analysis order
/// (deterministic dependency-first), so repeated builds produce identical
/// module lists and symbol names.
pub(crate) fn build_package_wasm_from_graph(
    package: &mut AnalyzedPackage,
    options: &PackageWasmOptions,
) -> Result<PackageWasmBuild, Vec<Diagnostic>> {
    if package
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.is_error())
    {
        return Err(package.diagnostics.clone());
    }
    // Exactly one root unit (Package Compilation Contract "entry unit").
    let entry_index = select_entry_unit(package)?;
    let product = product_identity(package);
    std::fs::create_dir_all(&options.output_dir).map_err(|error| {
        vec![crate::package_diagnostic_error(format!(
            "cannot create package wasm output dir {}: {error}",
            options.output_dir.display()
        ))]
    })?;

    let mut modules = Vec::with_capacity(package.units.len());
    for (index, unit) in package.units.iter_mut().enumerate() {
        let (wat, bytes) = emit_unit_module(unit, entry_index == index)?;
        let wat_path = module_file_name(&options.output_dir, &product, index, unit);
        std::fs::write(&wat_path, &wat).map_err(|error| {
            vec![crate::package_diagnostic_error(format!(
                "cannot write package wasm module {}: {error}",
                wat_path.display()
            ))]
        })?;
        modules.push(PackageWasmModule {
            unit_path: unit.path.clone(),
            module_segments: unit.module_segments.clone(),
            is_entry: unit.is_entry,
            wat_path,
            wat,
            bytes,
        });
    }

    let entry = modules
        .iter()
        .find(|module| module.is_entry)
        .ok_or_else(|| {
            vec![crate::package_diagnostic_error(
                "internal invariant: select_entry_unit guaranteed exactly one entry unit",
            )]
        })?;
    let entry_module = entry.wat_path.clone();
    let entry_bytes = entry.bytes.clone();
    let sibling_bytes = modules
        .iter()
        .filter(|module| !module.is_entry)
        .map(|module| module.bytes.clone())
        .collect::<Vec<_>>();
    let module_list = modules.iter().map(|module| module.wat_path.clone()).collect();
    let binary_path = package
        .spec
        .package_root
        .join("target")
        .join("faber")
        .join("wasm")
        .join("bin")
        .join(&product);
    Ok(PackageWasmBuild {
        product,
        entry_unit: package.units[entry_index].path.clone(),
        modules,
        manifest: PackageWasmLinkManifest {
            modules: module_list,
            entry_module,
            entry_bytes,
            sibling_bytes,
            output: options.output_dir.clone(),
            binary_path,
        },
    })
}

/// Select the single entry unit of the package graph (mirrors the package-MIR
/// driver's `select_entry_unit` and the package-to-LLVM builder).
fn select_entry_unit(package: &AnalyzedPackage) -> Result<usize, Vec<Diagnostic>> {
    let entries = package
        .units
        .iter()
        .enumerate()
        .filter_map(|(index, unit)| unit.is_entry.then_some(index))
        .collect::<Vec<_>>();
    let entry = package.spec.entry.display().to_string();
    match entries.as_slice() {
        [index] => Ok(*index),
        [] => Err(vec![crate::package_diagnostic_error(
            "package wasm build requires exactly one entry unit",
        )
        .with_file(entry)]),
        _ => Err(vec![crate::package_diagnostic_error(
            "package wasm build found multiple entry units",
        )
        .with_file(entry)]),
    }
}

/// Stable product identity for the package: the package root directory name —
/// the same source [`radix::tool::package_identity_facts_for_path`] uses, so
/// the manifest identity matches the emitted `__faber_external_…` symbol
/// names.
fn product_identity(package: &AnalyzedPackage) -> String {
    package
        .spec
        .package_root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "faber-package".to_owned())
}

/// Deterministic `.wat` file name for a unit: zero-padded index in package
/// analysis order + canonical module segments (root unit uses `root`).
fn module_file_name(
    output_dir: &Path,
    product: &str,
    index: usize,
    unit: &AnalyzedPackageUnit,
) -> PathBuf {
    let segments = if unit.module_segments.is_empty() {
        "root".to_owned()
    } else {
        unit.module_segments.join("-")
    };
    output_dir.join(format!("{index:03}-{product}-{segments}.wat"))
}

/// MIR-lowering diagnostic for a package unit.
fn mir_lowering_diag(path: &Path, message: impl Into<String>) -> Diagnostic {
    crate::package_diagnostic_error(message)
        .with_file(path.display().to_string())
        .with_phase(radix::diagnostics::DiagnosticPhase::Mir)
}

/// Wasm emission diagnostic for a package unit.
fn wasm_emission_diag(path: &Path, message: impl Into<String>) -> Diagnostic {
    crate::package_diagnostic_error(message)
        .with_file(path.display().to_string())
        .with_arg("issue", "wasm_emission_failed")
}

/// Attach canonical package identities (if absent), lower, and emit one unit
/// as a package-aware WAT module + wasm bytes. Entry units keep the `incipit`
/// export; siblings are post-processed so identity-carrying exported
/// functions also export the canonical `__faber_external_…` symbol their
/// consumers import.
fn emit_unit_module(
    unit: &mut AnalyzedPackageUnit,
    is_entry: bool,
) -> Result<(String, Vec<u8>), Vec<Diagnostic>> {
    if unit.analysis.package_import_identities.is_none() {
        // D-PA1: the CLI compile path attaches these facts for the root file;
        // the package graph does not, so the builder supplies them per unit so
        // MIR lowering records (product, module_path, item) identities for the
        // sibling's defining exports AND the entry's imported-callee references.
        if let Some(identities) = radix::tool::package_identity_facts_for_path(&unit.path) {
            unit.analysis.package_import_identities = Some(identities);
        }
    }
    let lowered = radix::mir::lower_analyzed_unit_with_context(&mut unit.analysis).map_err(
        |errors| {
            errors
                .iter()
                .map(|error| {
                    mir_lowering_diag(&unit.path, &error.message)
                        .with_arg("issue", error.issue.clone())
                })
                .collect::<Vec<_>>()
        },
    )?;
    let mut wat = radix::mir::emit_wasm_text_probe_package_aware(
        &lowered.validated,
        &lowered.interner,
    )
    .map_err(|error| {
        vec![wasm_emission_diag(
            &unit.path,
            format!("Wasm emission failed: {error}"),
        )]
    })?;
    if !is_entry {
        wat = add_canonical_sibling_exports(&wat, &lowered.validated, &lowered.interner).map_err(
            |diagnostics| {
                diagnostics
                    .into_iter()
                    .map(|diag| diag.with_file(unit.path.display().to_string()))
                    .collect()
            },
        )?;
    }
    let bytes = wat::parse_str(&wat).map_err(|error| {
        vec![wasm_emission_diag(
            &unit.path,
            format!("emitted WAT failed to parse: {error}"),
        )]
    })?;
    Ok((wat, bytes))
}

/// Add the canonical `__faber_external_product_…_module_…_func_…` export to
/// every identity-carrying function definition in a sibling module.
///
/// The radix package-aware emitter declares the consumer's cross-module
/// import on the `faber_external` module and the sibling defines the function
/// under its ordinary name; the product host resolves an import field `F`
/// against the sibling export `__faber_{F}`, so the sibling module must also
/// export the canonical symbol. Iterating the validated MIR functions whose
/// `source` def-id carries a recorded `(product, module_path, item)` identity
/// (the D-PA1 lowering recording) gives exactly the exports consumers name.
fn add_canonical_sibling_exports(
    wat: &str,
    validated: &radix::mir::ValidatedMir<'_>,
    interner: &radix::lexer::Interner,
) -> Result<String, Vec<Diagnostic>> {
    let names = radix::mir::names::MirNames::new(
        validated.program(),
        validated.validation().types,
        interner,
    );
    let mut out = wat.to_owned();
    for function in validated.program().functions {
        let Some(source) = function.source else {
            continue;
        };
        let Some(identity) = validated
            .validation()
            .external_function_identities
            .get(&source)
        else {
            continue;
        };
        let symbol = names.function(function);
        let canonical = format!(
            "__faber_{}",
            radix::mir::names::external_function_import_name(identity)
        );
        let marker = format!("(func ${symbol} (export \"{symbol}\")");
        if !out.contains(&marker) {
            return Err(vec![crate::package_diagnostic_error(format!(
                "package wasm sibling module defines no export marker for function `${symbol}` \
                 with canonical identity `{canonical}`"
            ))
            .with_arg("issue", "package_wasm_sibling_export_marker_missing")]);
        }
        out = out.replacen(
            &marker,
            &format!("{marker} (export \"{canonical}\")"),
            1,
        );
    }
    Ok(out)
}

#[cfg(test)]
#[path = "wasm_test.rs"]
mod tests;
