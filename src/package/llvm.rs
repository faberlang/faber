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

use radix::diagnostics::Diagnostic;
use radix::driver::Config;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::compile::{analyze_package, AnalyzedPackage, AnalyzedPackageUnit};
use super::library::{with_library_cached_analysis_mut, LibraryInterfaceCache};
use super::{library_resolver_for_package, LibraryImportBinding};
use crate::library::LibraryResolver;

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
///
/// `allow(dead_code)`: the faber binary inlines the package modules, so fields
/// only the pairwise exempla harness (lib consumer) and the product builder
/// read are flagged by the bin target's dead-code analysis.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PackageLlvmModule {
    /// Source unit path in the package graph.
    pub unit_path: PathBuf,
    /// Canonical module segments for this unit (not filesystem spelling alone).
    pub module_segments: Vec<String>,
    /// Whether this unit is the package's single entry unit.
    pub is_entry: bool,
    /// Whether this module is a selected flat Norma unit (S8.5 Norma graph:
    /// transitive used modules only, resolved through the Faber package
    /// graph's library resolver — never a harness-side re-resolution).
    pub is_norma: bool,
    /// The emitted `.ll` file for this unit.
    pub llvm_path: PathBuf,
}

/// Inspectable link manifest (Package Compilation Contract).
///
/// `allow(dead_code)`: the faber binary inlines the package modules, so fields
/// only the pairwise exempla harness (lib consumer) and the product builder
/// read are flagged by the bin target's dead-code analysis.
#[allow(dead_code)]
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
///
/// `allow(dead_code)`: the faber binary inlines the package modules, so fields
/// only the pairwise exempla harness (lib consumer) and the product builder
/// read are flagged by the bin target's dead-code analysis.
#[allow(dead_code)]
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
    build_package_llvm_from_graph(config, &mut package, options)
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
    config: &Config,
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
            is_norma: false,
            llvm_path,
        });
    }

    // S8.5 Norma graph: selected flat Norma units (transitive used modules
    // only, resolved exactly as Rust package compile does through the Faber
    // package graph's library resolver — no new resolver) emit one `.ll`
    // module per unit. Each module defines its exported functions under the
    // canonical `__faber_external_product_norma_module_…_func_…` identities
    // the entry (and any sibling Norma module) declares and calls.
    emit_selected_norma_modules(config, package, &options.output_dir, &product, &mut modules)?;

    let entry_module = modules
        .iter()
        .find(|module| module.is_entry)
        .ok_or_else(|| {
            vec![crate::package_diagnostic_error(
                "internal invariant: select_entry_unit guaranteed exactly one entry unit",
            )]
        })?
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
    let output = options.output_dir.join(&product);
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

/// Deterministic `.ll` file name for a selected Norma unit: the module path is
/// prefixed with `norma` so the file is never confused with a package sibling
/// module that happens to share the same terminal segments.
fn norma_module_file_name(
    output_dir: &Path,
    product: &str,
    index: usize,
    module_segments: &[String],
) -> PathBuf {
    let segments = if module_segments.is_empty() {
        "root".to_owned()
    } else {
        module_segments.join("-")
    };
    output_dir.join(format!("{index:03}-{product}-norma-{segments}.ll"))
}

/// Emit one `.ll` module per selected Norma unit (S8.5 Norma graph).
///
/// Selection = the transitive closure of `norma:*` imports carried by the
/// package graph (`expanded_library_imports`, dependencies first, deduped by
/// identity) — exactly the modules a Rust package compile selects. Each module
/// is analyzed through the package graph's library cache, lowered through MIR
/// with the canonical `(product = "norma", module_path, item)` package
/// identities attached (so its exported functions define
/// `__faber_external_product_norma_module_…_func_…`), and emitted in library
/// mode (no `__faber_program_entry_v1`). No `.ll` text concatenation and no
/// secondary import parser.
///
/// Carried provider modules (the S8.5 closed set — solum/tempus/toml/valor/
/// json) are NOT emitted: their functions resolve to the versioned runtime
/// intrinsics at the call site, so their compiled bodies would only pull the
/// unimplemented legacy sermo/convert dialect into the link.
///
/// Whether a `norma:*` library import names a carried provider module in the
/// S8.5 closed set. Matched by provider family (first module segment) so
/// nested modules such as `norma:solum/path` are covered the same way: every
/// provider family body lives in the Rust runtime and is never referenced
/// through a compiled Faber module.
fn is_carried_provider_module(package: &str, module_segments: &[String]) -> bool {
    package == "norma"
        && module_segments.first().is_some_and(|segment| {
            matches!(
                segment.as_str(),
                "solum" | "tempus" | "toml" | "valor" | "json"
            )
        })
}

fn emit_selected_norma_modules(
    config: &Config,
    package: &AnalyzedPackage,
    output_dir: &Path,
    product: &str,
    modules: &mut Vec<PackageLlvmModule>,
) -> Result<(), Vec<Diagnostic>> {
    let library_resolver = library_resolver_for_package(config, &package.spec.package_root)?;
    let mut library_cache = LibraryInterfaceCache::with_config(config);
    let mut seen = BTreeSet::new();
    let mut index = modules.len();
    for unit in &package.units {
        for import in &unit.expanded_library_imports {
            let key = (
                import.module.package.clone(),
                import.module.module_path.join("/"),
            );
            if !seen.insert(key) {
                continue;
            }
            // S8.5 carried-provider closed set: `norma:*` provider modules
            // whose functions the LLVM host lane resolves to versioned runtime
            // intrinsics (or native leaves) — solum/tempus/toml/valor/json
            // (the same closed set `radix-mir-llvm/src/host.rs`
            // `solum_call_kind` / `provider_carrier_kind` /
            // `is_tempus_wait_call` recognize). Their bodies genuinely live in
            // the Rust runtime (package-provider-abi-design.md D-PA2), so the
            // builder must not compile the provider module: a compiled body
            // lowers its `ad` routes to the legacy
            // `__faber_runtime_sermo_*` / convert runtime dialect the host
            // archive does not implement (L26 solum-lege-generic regression).
            // The entry (and any sibling unit) resolves the same provider
            // functions to their versioned `__faber_rt_v1_*` intrinsics — the
            // merged package-MIR probe behavior.
            if is_carried_provider_module(&import.module.package, &import.module.module_path) {
                continue;
            }
            let module_segments = import.module.module_path.clone();
            let llvm_path = norma_module_file_name(output_dir, product, index, &module_segments);
            emit_one_norma_module(import, &library_resolver, &mut library_cache, &llvm_path)?;
            modules.push(PackageLlvmModule {
                unit_path: import.module.interface_path.clone(),
                module_segments,
                is_entry: false,
                is_norma: true,
                llvm_path,
            });
            index += 1;
        }
    }
    Ok(())
}

/// Analyze, lower, and emit ONE selected Norma unit as a library-mode `.ll`
/// module. The mutable-analysis access stays inside [`with_library_cached_analysis_mut`]
/// so the lowered borrow cannot leak out of the cache.
fn emit_one_norma_module(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    llvm_path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let module_segments = import.module.module_path.clone();
    with_library_cached_analysis_mut(
        import,
        library_resolver,
        library_cache,
        |analysis, _cache| {
            // The canonical Norma identity: `(norma, module_path, item)`. The
            // package graph resolved the module to this interface path; the
            // attached facts make MIR record the same identity the consumer's
            // `record_library_item_identity` produced for its call sites.
            analysis.package_import_identities = Some(radix::driver::PackageImportIdentities {
                product: "norma".to_owned(),
                module_segments: module_segments.clone(),
                imports: std::collections::BTreeMap::new(),
            });
            let lowered =
                radix::mir::lower_analyzed_unit_with_context(analysis).map_err(|errors| {
                    radix::codegen::CodegenError {
                        message: errors
                            .iter()
                            .map(|error| error.message.as_str())
                            .collect::<Vec<_>>()
                            .join(" | "),
                        args: Vec::new(),
                    }
                })?;
            let llvm = radix::mir::emit_llvm_text_probe_library_module(
                &lowered.validated,
                &lowered.interner,
            )
            .map_err(|error| radix::codegen::CodegenError {
                message: format!("{}:{}", error.category, error.shape),
                args: Vec::new(),
            })?;
            std::fs::write(llvm_path, llvm).map_err(|error| radix::codegen::CodegenError {
                message: format!(
                    "cannot write package LLVM module {}: {error}",
                    llvm_path.display()
                ),
                args: Vec::new(),
            })
        },
    )
    .map_err(|diagnostic| vec![diagnostic])
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
    // S8.2/S8.6: a CLI-bearing entry unit emits through the versioned static
    // descriptor adapter lane — the product CLI lane (never a harness-side
    // reparse). The descriptor carries the exit policy (Fixed/Binding/Field)
    // and the runtime derives the process code; the legacy fixed-code emission
    // seam is dropped for CLI entries. Ordinary (non-CLI) entries keep the
    // device-roles lane with no explicit exit code.
    let cli_adapter = unit
        .analysis
        .cli_program
        .as_ref()
        .map(radix::cli_descriptor::build_cli_adapter_plan);
    let lowered = if is_entry && unit.analysis.cli_program.is_some() {
        radix::mir::lower_analyzed_unit_with_cli_adapter_with_context(&mut unit.analysis)
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
        match cli_adapter {
            Some(plan) => radix::mir::emit_llvm_text_probe_with_cli_adapter(
                &device_roles,
                &lowered.validated,
                &lowered.interner,
                plan,
            ),
            None => radix::mir::emit_llvm_text_probe_with_device_roles_and_exit(
                &device_roles,
                &lowered.validated,
                &lowered.interner,
                None,
            ),
        }
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
