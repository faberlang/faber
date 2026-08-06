//! Reusable package-to-LLVM graph builder (Stage 8 S8.3/S8.4).
//!
//! Implements the "Package Compilation Contract" of
//! `radix/docs/factory/llvm-host-parity/stage-8-entry-cli-package-delivery.md`:
//! Faber's resolved package graph → ordered unit modules (one `.ll` per unit,
//! deterministic order) + exactly one root entry module + an inspectable link
//! manifest (module list, runtime archive, native flags, output, binary path).
//!
//! INVARIANTS:
//! - No `.ll` text concatenation: every package unit is lowered and emitted
//!   independently into its own module (D11 one-module-per-unit).
//! - No secondary import parser: the builder consumes the package graph from
//!   [`analyze_package`] only. The canonical `(product, module_path, item)`
//!   external identities are the radix tool's
//!   [`radix::tool::package_identity_facts_for_path`] facts (the same source
//!   the CLI compile path uses), never a harness-side re-resolution of
//!   imports.
//! - Exactly one module defines `__faber_program_entry_v1`: the entry unit is
//!   emitted with the host program entry; every sibling unit is emitted in
//!   library mode (no entry).
//!
//! The builder itself does not invoke the linker: linking is the caller's job
//! via the returned [`PackageLlvmLinkManifest`].

use radix::cli::CliExit;
use radix::diagnostics::Diagnostic;
use radix::driver::Config;
use std::path::{Path, PathBuf};

use super::compile::{analyze_package, AnalyzedPackage, AnalyzedPackageUnit};

/// Select the single entry unit of the package graph (mirrors the package-MIR
/// driver's `select_entry_unit`).
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
            "package LLVM build requires exactly one entry unit",
        )
        .with_file(entry)]),
        _ => Err(vec![crate::package_diagnostic_error(
            "package LLVM build found multiple entry units",
        )
        .with_file(entry)]),
    }
}

/// MIR-lowering diagnostic for a package unit.
fn mir_lowering_diag(path: &Path, message: impl Into<String>) -> Diagnostic {
    crate::package_diagnostic_error(message)
        .with_file(path.display().to_string())
        .with_phase(radix::diagnostics::DiagnosticPhase::Mir)
}

/// LLVM emission diagnostic for a package unit.
fn llvm_emission_diag(path: &Path, message: impl Into<String>) -> Diagnostic {
    crate::package_diagnostic_error(message)
        .with_file(path.display().to_string())
        .with_arg("issue", "llvm_emission_failed")
}

/// Options for a package-to-LLVM build.
#[derive(Debug, Clone)]
pub struct PackageLlvmOptions {
    /// Directory that receives one `.ll` module per package unit.
    pub output_dir: PathBuf,
    /// Host LLVM runtime archive recorded in the link manifest (host
    /// triple/profile matched). `None` records no archive; callers that build
    /// the runtime archive (e.g. the exempla harness) pass the archive path.
    pub runtime_archive: Option<PathBuf>,
    /// Native linker flags for the clang link step.
    pub native_flags: Vec<String>,
}

impl PackageLlvmOptions {
    /// Options with only an output directory; archive and flags filled by the
    /// caller (or left empty).
    #[must_use]
    pub fn new(output_dir: PathBuf) -> Self {
        Self {
            output_dir,
            runtime_archive: None,
            native_flags: Vec::new(),
        }
    }

    /// Set the host runtime archive recorded in the link manifest.
    #[must_use]
    pub fn with_runtime_archive(mut self, archive: Option<PathBuf>) -> Self {
        self.runtime_archive = archive;
        self
    }

    /// Set the native linker flags recorded in the link manifest.
    #[must_use]
    pub fn with_native_flags(mut self, flags: Vec<String>) -> Self {
        self.native_flags = flags;
        self
    }
}

/// One emitted LLVM module for a package unit (D11 one-module-per-unit).
#[derive(Debug, Clone)]
pub struct PackageLlvmModule {
    /// Source unit path in the package graph.
    pub unit_path: PathBuf,
    /// Canonical module segments for this unit (not filesystem spelling alone).
    pub module_segments: Vec<String>,
    /// Whether this unit is the package's single entry unit.
    pub is_entry: bool,
    /// The emitted `.ll` file for this unit.
    pub llvm_path: PathBuf,
}

/// Inspectable link manifest (Package Compilation Contract).
#[derive(Debug, Clone)]
pub struct PackageLlvmLinkManifest {
    /// One `.ll` path per unit, deterministic order (package analysis order).
    pub modules: Vec<PathBuf>,
    /// The exactly-one module carrying `__faber_program_entry_v1`.
    pub entry_module: PathBuf,
    /// Host runtime archive used at link time (host triple/profile matched).
    pub runtime_archive: Option<PathBuf>,
    /// Native linker flags for the clang link step.
    pub native_flags: Vec<String>,
    /// Linker output path (the produced binary).
    pub output: PathBuf,
    /// Canonical Faber target layout binary path
    /// (`<package_root>/target/faber/llvm/bin/<product>`).
    pub binary_path: PathBuf,
}

/// Complete package-to-LLVM build result.
#[derive(Debug, Clone)]
pub struct PackageLlvmBuild {
    /// Stable product identity used by the external symbol names
    /// (`__faber_external_product_<product>_…`).
    pub product: String,
    /// Source path of the single entry unit.
    pub entry_unit: PathBuf,
    /// One module per package unit, deterministic order.
    pub modules: Vec<PackageLlvmModule>,
    /// The inspectable link manifest.
    pub manifest: PackageLlvmLinkManifest,
}

/// Build the package-to-LLVM artifact for `input` through the Faber package
/// graph: one `.ll` module per resolved unit plus an inspectable link
/// manifest. The entry unit is emitted with the host program entry; sibling
/// units are emitted in library mode.
///
/// # Errors
///
/// Returns package analysis diagnostics, or per-unit MIR lowering / LLVM
/// emission diagnostics.
pub fn build_package_llvm(
    config: &Config,
    input: &Path,
    options: &PackageLlvmOptions,
) -> Result<PackageLlvmBuild, Vec<Diagnostic>> {
    let mut package = analyze_package(config, input)?;
    build_package_llvm_from_graph(&mut package, options)
}

/// Build the package-to-LLVM artifact from an already-analyzed package graph.
///
/// The graph is mutated so each unit's analysis can be attached its canonical
/// package identities and lowered. Unit order is the package analysis order
/// (deterministic dependency-first), so repeated builds produce identical
/// module lists and symbol names.
///
/// # Errors
///
/// Returns diagnostics when the graph has errors, when the entry unit is not
/// exactly one, or when a unit's MIR lowering / LLVM emission fails.
pub(crate) fn build_package_llvm_from_graph(
    package: &mut AnalyzedPackage,
    options: &PackageLlvmOptions,
) -> Result<PackageLlvmBuild, Vec<Diagnostic>> {
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
            "cannot create package LLVM output dir {}: {error}",
            options.output_dir.display()
        ))]
    })?;

    let mut modules = Vec::with_capacity(package.units.len());
    for (index, unit) in package.units.iter_mut().enumerate() {
        let llvm = emit_unit_module(unit, entry_index == index)?;
        let llvm_path = module_file_name(&options.output_dir, &product, index, unit);
        std::fs::write(&llvm_path, llvm).map_err(|error| {
            vec![crate::package_diagnostic_error(format!(
                "cannot write package LLVM module {}: {error}",
                llvm_path.display()
            ))]
        })?;
        modules.push(PackageLlvmModule {
            unit_path: unit.path.clone(),
            module_segments: unit.module_segments.clone(),
            is_entry: unit.is_entry,
            llvm_path,
        });
    }

    let entry_module = modules
        .iter()
        .find(|module| module.is_entry)
        .expect("select_entry_unit guarantees exactly one entry unit")
        .llvm_path
        .clone();
    let module_list = modules
        .iter()
        .map(|module| module.llvm_path.clone())
        .collect();
    let binary_path = package
        .spec
        .package_root
        .join("target")
        .join("faber")
        .join("llvm")
        .join("bin")
        .join(&product);
    let output = options.output_dir.join(format!("{product}"));
    Ok(PackageLlvmBuild {
        product,
        entry_unit: package.units[entry_index].path.clone(),
        modules,
        manifest: PackageLlvmLinkManifest {
            modules: module_list,
            entry_module,
            runtime_archive: options.runtime_archive.clone(),
            native_flags: options.native_flags.clone(),
            output,
            binary_path,
        },
    })
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

/// Deterministic `.ll` file name for a unit: zero-padded index in package
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
    output_dir.join(format!("{index:03}-{product}-{segments}.ll"))
}

/// Attach canonical package identities (if absent), lower, and emit one unit
/// as an LLVM module. Entry units carry the host program entry; siblings are
/// emitted in library mode (no `__faber_program_entry_v1`).
fn emit_unit_module(
    unit: &mut AnalyzedPackageUnit,
    is_entry: bool,
) -> Result<String, Vec<Diagnostic>> {
    if unit.analysis.package_import_identities.is_none() {
        // D-PA1: the CLI compile path attaches these facts for the root file;
        // the package graph does not, so the builder supplies them per unit so
        // MIR lowering records (product, module_path, item) identities for the
        // sibling's defining exports AND the entry's imported-callee references.
        if let Some(identities) = radix::tool::package_identity_facts_for_path(&unit.path) {
            unit.analysis.package_import_identities = Some(identities);
        }
    }
    let device_roles = radix::mir::device_roles_from_hir(&unit.analysis.hir);
    let exit_code = unit.analysis.cli_program.as_ref().and_then(|program| {
        program.exit.as_ref().and_then(|exit| match exit {
            CliExit::Fixed(code) => Some(*code),
            CliExit::Binding(_) | CliExit::Field { .. } | CliExit::Unsupported => None,
        })
    });
    let lowered = if is_entry && unit.analysis.cli_program.is_some() {
        radix::mir::lower_analyzed_unit_allowing_cli_entry_with_context(&mut unit.analysis)
    } else {
        radix::mir::lower_analyzed_unit_with_context(&mut unit.analysis)
    }
    .map_err(|errors| {
        errors
            .iter()
            .map(|error| {
                mir_lowering_diag(&unit.path, &error.message).with_arg("issue", error.issue.clone())
            })
            .collect::<Vec<_>>()
    })?;
    if is_entry {
        radix::mir::emit_llvm_text_probe_with_device_roles_and_exit(
            &device_roles,
            &lowered.validated,
            &lowered.interner,
            exit_code,
        )
    } else {
        radix::mir::emit_llvm_text_probe_library_module(&lowered.validated, &lowered.interner)
    }
    .map_err(|error| {
        vec![llvm_emission_diag(
            &unit.path,
            format!("LLVM emission failed: {}:{}", error.category, error.shape),
        )]
    })
}

#[cfg(test)]
#[path = "llvm_test.rs"]
mod tests;
