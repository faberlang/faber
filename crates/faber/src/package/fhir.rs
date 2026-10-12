//! FHIR package loading and loaded-package adaptation (product half).
//!
//! The FHIR package format is the portable analyzed-HIR interchange for Faber
//! (contract: [`docs/factory/hir-artifact-fhir/fhir-package-contract.md`] in
//! the radix repo). This module owns the faber product half of the contract:
//!
//! - [`load_package_fhir`] loads a `.fhirpkg` envelope in a fresh process
//!   with no lexer/parser/resolver/analyzer pass (FPF-5: the wire is the
//!   `MessagePack` package envelope, schema v2; retired v1 postcard bytes are
//!   rejected with the named legacy error);
//! - [`loaded_package_to_analyzed`] rehydrates an [`AnalyzedProgram`] so the
//!   existing Rust/FMIR/canonical-Faber assembly paths consume loaded units —
//!   local imports resolve from the envelope's explicit link table, never
//!   from the filesystem;
//! - [`run_loaded_package_fhir`] runs a loaded package in-process.
//!
//! RP5: the writer half ([`radix::program::fhir::build_package_fhir`]) moved to
//! `radix-program/src/fhir.rs` (compiler-owned). This module keeps only the
//! Faber-side loader and execution adapter, which consume the typed artifact
//! through the Faber analysis/MIR seams (ownership-freeze §5.3).
#![allow(dead_code)] // Binary build/run and lib tests exercise different surfaces.

use radix::module::diagnostics::Diagnostic;
use radix::module::driver::Config;
use radix::module::hir::package::{LoadedHirModule, LoadedHirPackage, load_package};
use radix::module::lexer::Span;
use radix::module::semantic::passes::visibility_annotation::VisibilityTier;
use radix::module::syntax::Visibility;
use radix::program::LibraryImportBinding;
use radix::program::analyze::{AnalyzedProgram, AnalyzedProgramNode, ModuleGraph, ModuleId};
use radix::program::discovery::PackageSpec;
use radix::program::library::LibraryResolver;
use radix::program::library::ResolvedLibraryModule;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Load a FHIR package artifact source-independently.
///
/// The format crate's `MessagePack` decoder runs the legacy postcard
/// rejection, the package version gate, per-unit gates (unit schema + F3 +
/// U2.3 referential integrity + source-hash consistency), and package-level
/// module/dependency/link checks; the radix loader reconstructs each unit's
/// arenas. Never reparses source.
pub(crate) fn load_package_fhir(path: &Path) -> Result<LoadedHirPackage, Vec<Diagnostic>> {
    let bytes = fs::read(path).map_err(|error| {
        vec![fhir_diag(
            path,
            format!("could not read FHIR package: {error}"),
        )]
    })?;
    load_package(&bytes).map_err(|error| {
        vec![fhir_issue_diag(
            path,
            "fhir_package_invalid",
            error.to_string(),
        )]
    })
}

// ---------------------------------------------------------------------------
// Loaded-package adapter
// ---------------------------------------------------------------------------

/// Rehydrate an [`AnalyzedProgram`] from a loaded FHIR package.
///
/// Local imports resolve from the envelope's explicit link table (binding →
/// target module path), so the reconstructed package feeds the existing Rust,
/// FMIR, Go, and canonical-Faber assembly paths without any filesystem or
/// source access. Unit `namespace_exports` are rebuilt as
/// `{ binding: target_module.export_names }` — the same values the direct
/// analysis computed.
pub(crate) fn loaded_package_to_analyzed(
    loaded: LoadedHirPackage,
    artifact_dir: &Path,
) -> Result<AnalyzedProgram, Vec<Diagnostic>> {
    let package_root = artifact_dir.to_path_buf();
    let spec = PackageSpec {
        package_root: package_root.clone(),
        source_root: package_root.clone(),
        entry: package_root.join(&loaded.entry_path),
        templates: BTreeMap::new(),
        manifest_backed: true,
    };

    let entry_frontmatter = loaded
        .entry_frontmatter
        .as_deref()
        .map(|text| {
            toml::from_str::<toml::Table>(text)
                .map(radix::module::driver::FileFrontmatter::from_table)
                .map_err(|error| {
                    vec![
                        crate::package_diagnostic_error(format!(
                            "invalid FHIR package entry frontmatter: {error}"
                        ))
                        .with_arg("issue", "fhir_frontmatter_invalid"),
                    ]
                })
        })
        .transpose()?;

    let units_by_module_path = loaded
        .modules
        .iter()
        .enumerate()
        .map(|(index, module)| (module.relative_path.clone(), index))
        .collect::<BTreeMap<_, _>>();

    validate_declared_dependencies(&loaded, &package_root)?;

    // Precompute namespace_exports per module while every module is still
    // borrowable (the loop below consumes `loaded.modules` by value).
    let namespace_exports_all = loaded
        .modules
        .iter()
        .map(|module| namespace_exports_for_loaded(module, &loaded.modules, &units_by_module_path))
        .collect::<Vec<_>>();

    let path_to_id = loaded
        .modules
        .iter()
        .map(|module| {
            (
                package_root.join(&module.relative_path),
                ModuleId::for_spec(&spec, module.module_segments.clone()),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut imports = ModuleGraph::default();
    let mut nodes = BTreeMap::new();
    let mut entry = None;
    for (index, module) in loaded.modules.into_iter().enumerate() {
        let path = package_root.join(&module.relative_path);
        let module_id = ModuleId::for_spec(&spec, module.module_segments.clone());
        let expanded_library_imports = module
            .library_imports
            .iter()
            .map(|import| LibraryImportBinding {
                binding: import.binding.clone(),
                visibility: Visibility::Privata,
                import_span: Span::default(),
                module: ResolvedLibraryModule::new(
                    import.package.clone(),
                    import.module.clone(),
                    package_root.join("libs").join(&import.package),
                ),
            })
            .collect::<Vec<_>>();
        for link in &module.local_links {
            let target_path = package_root.join(&link.target);
            if let Some(to) = path_to_id.get(&target_path) {
                imports.insert_local(module_id.clone(), link.binding.clone(), to.clone());
            }
        }
        for import in &expanded_library_imports {
            imports.insert_library(module_id.clone(), import.clone());
        }
        if module.is_entry {
            entry = Some(module_id.clone());
        }
        nodes.insert(
            module_id,
            AnalyzedProgramNode {
                path,
                module_segments: module.module_segments,
                is_entry: module.is_entry,
                analysis: module.unit,
                file_interface: radix::module::file_interface::FileInterface::new(),
                export_names: module.export_names,
                namespace_exports: namespace_exports_all[index].clone(),
                expanded_library_imports,
            },
        );
    }
    let roots = entry
        .clone()
        .map_or_else(|| nodes.keys().cloned().collect(), |id| vec![id]);

    // No library resolver exists on rehydration: the table holds the
    // package's own handlers only.
    let routes = radix::program::route_table::local_route_table(
        nodes
            .values()
            .map(|node| (node.path.as_path(), &node.analysis)),
        None,
    )?;
    Ok(AnalyzedProgram {
        spec,
        entry,
        roots,
        nodes,
        imports,
        entry_frontmatter,
        diagnostics: Vec::new(),
        library_resolver: LibraryResolver::with_optional_home(None),
        routes,
    })
}

/// Contract invariant: every library import must be backed by a declared
/// dependency coordinate in the envelope (the dependency record is the
/// store's closure key). An import without a declared dependency fails
/// before codegen with a structured error — never by reparsing source.
fn validate_declared_dependencies(
    loaded: &LoadedHirPackage,
    package_root: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let declared_packages = loaded
        .dependencies
        .iter()
        .map(|dependency| dependency.name.as_str())
        .collect::<BTreeSet<_>>();
    for module in &loaded.modules {
        for import in &module.library_imports {
            if !declared_packages.contains(import.package.as_str()) {
                return Err(vec![fhir_issue_diag(
                    &package_root.join(&module.relative_path),
                    "fhir_dependency_unresolved",
                    format!(
                        "FHIR package import `{}` references library `{}` with no declared dependency record",
                        import.binding, import.package
                    ),
                )
                .with_arg("package", import.package.clone())]);
            }
        }
    }
    Ok(())
}

/// Per-unit binding → target module path map for the loaded package, keyed by
/// unit path, consumed by `generate_package_rust`'s link-table sibling
/// resolution.
pub(crate) fn loaded_links_by_unit_path(
    loaded: &LoadedHirPackage,
    artifact_dir: &Path,
) -> BTreeMap<PathBuf, BTreeMap<String, PathBuf>> {
    let mut by_unit = BTreeMap::new();
    for module in &loaded.modules {
        let mut links = BTreeMap::new();
        for link in &module.local_links {
            links.insert(link.binding.clone(), artifact_dir.join(&link.target));
        }
        by_unit.insert(artifact_dir.join(&module.relative_path), links);
    }
    by_unit
}

/// Rebuild a module's `namespace_exports` from the explicit link table:
/// `{ binding: target_module.export_names }` — the same values the direct
/// package analysis computed for local imports.
fn namespace_exports_for_loaded(
    module: &LoadedHirModule,
    modules: &[LoadedHirModule],
    by_path: &BTreeMap<String, usize>,
) -> BTreeMap<String, BTreeMap<String, VisibilityTier>> {
    let mut exports = BTreeMap::new();
    for link in &module.local_links {
        if let Some(&index) = by_path.get(&link.target) {
            // The FHIR-loaded package carries export names without tiers;
            // every name is marked publica (artifact round-trips do not
            // re-run import resolution).
            exports.insert(
                link.binding.clone(),
                modules[index]
                    .export_names
                    .iter()
                    .map(|name| (name.clone(), VisibilityTier::Publica))
                    .collect(),
            );
        }
    }
    exports
}

/// Run a loaded FHIR package in-process: reconstruct the package from the
/// envelope, lower to FMIR, and execute with `host` — no Rust, no source.
/// Local imports resolve from the envelope's explicit link table.
pub(crate) fn run_loaded_package_fhir<H: radix::module::mir::Host + ?Sized>(
    config: &Config,
    loaded: LoadedHirPackage,
    artifact_dir: &Path,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let links = loaded_links_by_unit_path(&loaded, artifact_dir);
    let package = loaded_package_to_analyzed(loaded, artifact_dir)?;
    radix::program::mir::run_package_mir_from_loaded(
        config,
        package,
        &links,
        &super::mir::faber_mir_seams(),
        host,
    )
}

// ---------------------------------------------------------------------------
// Diagnostics
// ---------------------------------------------------------------------------

fn fhir_diag(path: &Path, message: impl Into<String>) -> Diagnostic {
    crate::package_diagnostic_error(message).with_file(path.display().to_string())
}

fn fhir_issue_diag(path: &Path, issue: &'static str, message: impl Into<String>) -> Diagnostic {
    fhir_diag(path, message).with_arg("issue", issue)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "fhir_test.rs"]
mod tests;
