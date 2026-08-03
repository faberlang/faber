//! FHIR package assembly, loading, and loaded-package adaptation.
//!
//! The FHIR package format is the portable analyzed-HIR interchange for Faber
//! (contract: [`docs/factory/hir-artifact-fhir/fhir-package-contract.md`] in
//! the radix repo). This module owns the Faber side of the contract:
//!
//! - [`build_package_fhir`] analyzes a package once and writes a deterministic
//!   `.fhirpkg` envelope (per-unit `HirArtifact` records + module/link/export
//!   metadata) without retaining the source checkout;
//! - [`load_package_fhir`] loads the envelope in a fresh process with no
//!   lexer/parser/resolver/analyzer pass;
//! - [`loaded_package_to_analyzed`] rehydrates an [`AnalyzedPackage`] so the
//!   existing Rust/FMIR/canonical-Faber assembly paths consume loaded units —
//!   local imports resolve from the envelope's explicit link table, never
//!   from the filesystem.
//!
//! The format crate (`radix-hir-fhir`) owns the wire types and codec; this
//! module reaches them through `radix::hir::package`.
#![allow(dead_code)] // Binary build/run and lib tests exercise different surfaces.

use super::compile::{analyze_package, AnalyzedPackage, AnalyzedPackageUnit};
use super::import_graph::{resolve_import, ImportResolution};
use super::library_resolver_for_package;
use super::manifest::manifest_for_spec;
use crate::library::{LibraryProviderKind, ResolvedLibraryModule};
use radix::diagnostics::Diagnostic;
use radix::driver::Config;
use radix::hir::package::{
    build_package, encode_package, load_package, LibraryImportWire, LoadedHirModule,
    LoadedHirPackage, LocalLinkWire, PackageDependencyWire, PackageIdentityWire, PackageUnitInput,
};
use radix::hir::HirItemKind;
use radix::lexer::Span;
use radix::syntax::Visibility;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const FHIR_PACKAGE_ARTIFACT_DIR: &str = "faber-fhir";
const FHIR_PACKAGE_FILE: &str = "package.fhirpkg";
const FHIR_PACKAGE_UNLOCKED_LOCK_IDENTITY: &str = "unlocked";

/// Built FHIR package artifact: root directory and the `.fhirpkg` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PackageFhir {
    pub(crate) root: PathBuf,
    pub(crate) package_path: PathBuf,
}

// ---------------------------------------------------------------------------
// Writer
// ---------------------------------------------------------------------------

/// Analyze a package and write it as a deterministic FHIR package artifact.
///
/// Mirrors the FMIR builders: analyze once, assemble the envelope from the
/// existing analysis result, encode, write under `<pkg>/target/faber-fhir/`.
/// The envelope records package identity, entry path + frontmatter, the
/// module table (paths, segments, entry flag, export names), the explicit
/// local link table (binding → target module path), library references
/// (binding/package/module — no absolute paths), dependency coordinates, and
/// ordered per-unit artifacts.
pub(crate) fn build_package_fhir(
    config: &Config,
    input: &Path,
) -> Result<PackageFhir, Vec<Diagnostic>> {
    let package = analyze_package(config, input)?;
    if package
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.is_error())
    {
        return Err(package.diagnostics);
    }
    let package_root = package.spec.package_root.clone();
    let library_resolver = library_resolver_for_package(config, &package_root)?;
    let manifest = manifest_for_spec(&package.spec).map_err(|diagnostic| vec![*diagnostic])?;

    let identity = package_identity(&package, manifest.as_ref());
    let entry_path = relative_path_of(&package_root, &package.spec.entry);
    let entry_frontmatter = package
        .entry_frontmatter
        .as_ref()
        .map(|frontmatter| frontmatter.as_table().to_string());
    let dependencies = dependency_records(&package, manifest.as_ref())?;

    let mut inputs = Vec::with_capacity(package.units.len());
    for unit in &package.units {
        let relative_path = relative_path_of(&package_root, &unit.path);
        let source_bytes = fs::read(&unit.path).map_err(|error| {
            vec![fhir_diag(
                &unit.path,
                format!("could not read source for FHIR package: {error}"),
            )]
        })?;
        let local_links =
            local_links_for_unit(&package, unit, &library_resolver, &package_root)?;
        inputs.push(PackageUnitInput {
            relative_path,
            module_segments: unit.module_segments.clone(),
            is_entry: unit.is_entry,
            export_names: unit.export_names.clone(),
            local_links,
            library_imports: library_imports_for_unit(unit),
            content_hash: sha256_hex(&source_bytes),
            analysis: &unit.analysis,
        });
    }

    let fhir_package = build_package(identity, &entry_path, entry_frontmatter, inputs, dependencies)
        .map_err(|error| {
            vec![fhir_issue_diag(
                input,
                "fhir_package_invalid",
                error.to_string(),
            )]
        })?;
    let bytes = encode_package(&fhir_package).map_err(|error| {
        vec![fhir_diag(
            input,
            format!("could not encode FHIR package: {error}"),
        )]
    })?;

    let artifact_root = package_root.join("target").join(FHIR_PACKAGE_ARTIFACT_DIR);
    fs::create_dir_all(&artifact_root)
        .map_err(|error| vec![fhir_diag(input, error.to_string())])?;
    let package_path = artifact_root.join(FHIR_PACKAGE_FILE);
    fs::write(&package_path, &bytes)
        .map_err(|error| vec![fhir_diag(input, error.to_string())])?;
    Ok(PackageFhir {
        root: artifact_root,
        package_path,
    })
}

/// Package identity from the manifest, or a package-root-derived fallback for
/// legacy manifestless inputs.
fn package_identity(
    package: &AnalyzedPackage,
    manifest: Option<&super::manifest::FaberManifest>,
) -> PackageIdentityWire {
    match manifest {
        Some(manifest) => PackageIdentityWire {
            name: manifest.package.name.clone(),
            version: manifest.package.version.clone(),
            edition: manifest.package.edition.clone(),
        },
        None => PackageIdentityWire {
            name: package
                .spec
                .package_root
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "package".to_owned()),
            version: "0.1.0".to_owned(),
            edition: "2026".to_owned(),
        },
    }
}

/// Dependency coordinates from `faber.toml [dependencies]` plus `faber.lock`
/// identity when a lock record exists (version + lock identity from the lock;
/// checksums are a Stage 5 store concern).
fn dependency_records(
    package: &AnalyzedPackage,
    manifest: Option<&super::manifest::FaberManifest>,
) -> Result<Vec<PackageDependencyWire>, Vec<Diagnostic>> {
    let Some(manifest) = manifest else {
        return Ok(Vec::new());
    };
    let lock = super::lockfile::read_lock(&package.spec.package_root)
        .map_err(|diagnostic| vec![*diagnostic])?;
    let locked_by_name = lock
        .as_ref()
        .map(|lock| {
            lock.packages
                .iter()
                .map(|entry| (entry.name.as_str(), entry))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let mut records = Vec::with_capacity(manifest.dependencies.len());
    for (name, manifest_version) in &manifest.dependencies {
        let locked = locked_by_name.get(name.as_str());
        records.push(PackageDependencyWire {
            name: name.clone(),
            version: locked
                .map(|entry| entry.version.clone())
                .unwrap_or_else(|| manifest_version.clone()),
            lock_identity: locked
                .map(|entry| entry.source.clone())
                .unwrap_or_else(|| FHIR_PACKAGE_UNLOCKED_LOCK_IDENTITY.to_owned()),
            checksum: None,
        });
    }
    Ok(records)
}

/// Local import links for one unit: walk its HIR import declarations, resolve
/// local imports (the same resolution the direct path uses), and record each
/// binding → package-root-relative target module path.
fn local_links_for_unit(
    package: &AnalyzedPackage,
    unit: &AnalyzedPackageUnit,
    library_resolver: &crate::library::LibraryResolver,
    package_root: &Path,
) -> Result<Vec<LocalLinkWire>, Vec<Diagnostic>> {
    let mut links = Vec::new();
    let mut seen = BTreeSet::new();
    for item in &unit.analysis.hir.items {
        let HirItemKind::Import(import) = &item.kind else {
            continue;
        };
        let import_path = unit.analysis.interner.resolve(import.path);
        let ImportResolution::Local(target) =
            resolve_import(&package.spec, library_resolver, &unit.path, import_path)
        else {
            continue;
        };
        let Ok(relative) = target.strip_prefix(package_root) else {
            continue;
        };
        let relative = relative.to_string_lossy().into_owned();
        for import_item in &import.items {
            let binding = unit
                .analysis
                .interner
                .resolve(import_item.alias.unwrap_or(import_item.name))
                .to_owned();
            if seen.insert(binding.clone()) {
                links.push(LocalLinkWire {
                    binding,
                    target: relative.clone(),
                });
            }
        }
    }
    Ok(links)
}

/// Library import references for one unit (binding/package/module — no
/// absolute paths).
fn library_imports_for_unit(unit: &AnalyzedPackageUnit) -> Vec<LibraryImportWire> {
    unit.expanded_library_imports
        .iter()
        .map(|import| LibraryImportWire {
            binding: import.binding.clone(),
            package: import.module.package.clone(),
            module: import.module.module_path.clone(),
        })
        .collect()
}

fn relative_path_of(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

// ---------------------------------------------------------------------------
// Loader
// ---------------------------------------------------------------------------

/// Load a FHIR package artifact source-independently.
///
/// The format crate's decoder runs the package version gate, per-unit gates
/// (unit schema + F3 + U2.3 referential integrity + source-hash consistency),
/// and package-level module/dependency/link checks; the radix loader
/// reconstructs each unit's arenas. Never reparses source.
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

/// Rehydrate an [`AnalyzedPackage`] from a loaded FHIR package.
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
) -> Result<AnalyzedPackage, Vec<Diagnostic>> {
    let package_root = artifact_dir.to_path_buf();
    let spec = super::discovery::PackageSpec {
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
                .map(radix::driver::FileFrontmatter::from_table)
                .map_err(|error| {
                    vec![crate::package_diagnostic_error(format!(
                        "invalid FHIR package entry frontmatter: {error}"
                    ))
                    .with_arg("issue", "fhir_frontmatter_invalid")]
                })
        })
        .transpose()?;

    let units_by_module_path = loaded
        .modules
        .iter()
        .enumerate()
        .map(|(index, module)| (module.relative_path.clone(), index))
        .collect::<BTreeMap<_, _>>();

    // Contract invariant: every library import must be backed by a declared
    // dependency coordinate in the envelope (the dependency record is the
    // store's closure key). An import without a declared dependency fails
    // before codegen with a structured error — never by reparsing source.
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

    // Precompute namespace_exports per module while every module is still
    // borrowable (the loop below consumes `loaded.modules` by value).
    let namespace_exports_all = loaded
        .modules
        .iter()
        .map(|module| namespace_exports_for_loaded(module, &loaded.modules, &units_by_module_path))
        .collect::<Vec<_>>();

    let mut units = Vec::with_capacity(loaded.modules.len());
    for (index, module) in loaded.modules.into_iter().enumerate() {
        let path = package_root.join(&module.relative_path);
        let expanded_library_imports = module
            .library_imports
            .iter()
            .map(|import| super::LibraryImportBinding {
                binding: import.binding.clone(),
                visibility: Visibility::Privata,
                import_span: Span::default(),
                module: ResolvedLibraryModule::new(
                    import.package.clone(),
                    import.module.clone(),
                    package_root.join("libs").join(&import.package),
                    LibraryProviderKind::PackageDependency,
                ),
            })
            .collect();
        units.push(AnalyzedPackageUnit {
            path,
            module_segments: module.module_segments,
            is_entry: module.is_entry,
            analysis: module.unit,
            file_interface: radix::file_interface::FileInterface::new(),
            export_names: module.export_names,
            namespace_exports: namespace_exports_all[index].clone(),
            expanded_library_imports,
        });
    }

    Ok(AnalyzedPackage {
        spec,
        units,
        entry_frontmatter,
        diagnostics: Vec::new(),
        linked_library_crates: BTreeMap::new(),
    })
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
) -> BTreeMap<String, Vec<String>> {
    let mut exports = BTreeMap::new();
    for link in &module.local_links {
        if let Some(&index) = by_path.get(&link.target) {
            exports.insert(link.binding.clone(), modules[index].export_names.clone());
        }
    }
    exports
}

/// Run a loaded FHIR package in-process: reconstruct the package from the
/// envelope, lower to FMIR, and execute with `host` — no Rust, no source.
/// Local imports resolve from the envelope's explicit link table.
pub(crate) fn run_loaded_package_fhir<H: radix::mir::Host + ?Sized>(
    config: &Config,
    loaded: LoadedHirPackage,
    artifact_dir: &Path,
    host: &mut H,
) -> Result<(), Vec<Diagnostic>> {
    let links = loaded_links_by_unit_path(&loaded, artifact_dir);
    let package = loaded_package_to_analyzed(loaded, artifact_dir)?;
    super::mir::run_package_mir_from_loaded(config, package, &links, host)
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
