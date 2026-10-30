//! FHIR package assembly (writer half).
//!
//! The FHIR package format is the portable analyzed-HIR interchange for Faber
//! (contract: [`docs/factory/hir-artifact-fhir/fhir-package-contract.md`] in
//! the radix repo). This module owns the compiler half of the faber FHIR
//! surface: [`build_package_fhir`] analyzes a package once and writes a
//! deterministic `.fhirpkg` envelope (per-unit `HirArtifact` records +
//! module/link/export metadata) without retaining the source checkout.
//!
//! RP5: `build_package_fhir` moved from `crates/faber/src/package/fhir.rs`.
//! The product halves — `load_package_fhir` (in-process load) and
//! `run_loaded_package_fhir` (execution) — stay faber-side and consume the
//! forwarded builder (ownership-freeze §5.3).
//!
//! FPF-5: the `.fhirpkg` write path emits the `MessagePack` envelope (package
//! schema v2); the retired v1 postcard envelope is never written.
//!
//! The format crate (`radix-hir-fhir`) owns the wire types and codec; this
//! module reaches them through `radix_module::hir::package`.
#![allow(dead_code)] // Binary build/run and lib tests exercise different surfaces.

use crate::analyze::{AnalysisSeams, AnalyzedProgram, AnalyzedProgramNode};
use crate::discovery::PackageSpec;
use crate::import_graph::{ImportResolution, resolve_import};
use radix_module::diagnostics::Diagnostic;
use radix_module::driver::Config;
use radix_module::hir::HirItemKind;
use radix_module::hir::package::{
    LibraryImportWire, LocalLinkWire, PackageDependencyWire, PackageIdentityWire, PackageUnitInput,
    build_package_with_nucleum, encode_package_msgpack,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

const FHIR_PACKAGE_ARTIFACT_DIR: &str = "faber-fhir";
const FHIR_PACKAGE_FILE: &str = "package.fhirpkg";
const FHIR_PACKAGE_UNLOCKED_LOCK_IDENTITY: &str = "unlocked";

/// Built FHIR package artifact: root directory and the `.fhirpkg` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageFhir {
    pub root: PathBuf,
    pub package_path: PathBuf,
    /// Canonical MIR sidecar for importable nucleum fragments (goal OQ2).
    pub nucleum_path: Option<PathBuf>,
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
/// ordered per-unit artifacts. FPF-5: encoding is the `MessagePack` package
/// codec (schema v2) — the single writer, no postcard output.
///
/// RP5: moved from `crates/faber/src/package/fhir.rs`; faber product seams
/// (locale packs, manifest validation, library resolver construction) enter
/// through [`AnalysisSeams`].
/// # Errors
/// Returns an error when the input or persisted artifact is invalid.
pub fn build_package_fhir(
    config: &Config,
    input: &Path,
    seams: &AnalysisSeams,
) -> Result<PackageFhir, Vec<Diagnostic>> {
    let mut package = crate::analyze::analyze_package(config, input, seams)?;
    if package
        .diagnostics
        .iter()
        .any(radix_module::Diagnostic::is_error)
    {
        return Err(package.diagnostics);
    }
    let package_root = package.spec.package_root.clone();
    let library_resolver = (seams.resolver_for_package)(config, &package_root)?;
    let manifest = manifest_for_spec(&package.spec).map_err(|diagnostic| vec![*diagnostic])?;

    let identity = package_identity(&package, manifest.as_ref());
    let entry_path = relative_path_of(&package_root, &package.spec.entry);
    let entry_frontmatter = package
        .entry_frontmatter
        .as_ref()
        .map(|frontmatter| frontmatter.as_table().to_string());
    let dependencies = dependency_records(&package, manifest.as_ref())?;

    let mut inputs = Vec::with_capacity(package.nodes.len());
    for unit in package.nodes.values() {
        let relative_path = relative_path_of(&package_root, &unit.path);
        let source_bytes = fs::read(&unit.path).map_err(|error| {
            vec![fhir_issue_diag(
                &unit.path,
                "fhir_source_unreadable",
                format!("could not read source for FHIR package: {error}"),
            )]
        })?;
        let local_links = local_links_for_unit(&package, unit, &library_resolver, &package_root);
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

    let (fhir_package, mut nucleum) = build_package_with_nucleum(
        identity,
        &entry_path,
        entry_frontmatter,
        inputs,
        dependencies,
    )
    .map_err(|error| {
        vec![fhir_issue_diag(
            input,
            "fhir_package_invalid",
            error.to_string(),
        )]
    })?;
    let bytes = encode_package_msgpack(&fhir_package).map_err(|error| {
        vec![fhir_issue_diag(
            input,
            "fhir_encode_failed",
            format!("could not encode FHIR package: {error}"),
        )]
    })?;

    let artifact_root = package_root.join("target").join(FHIR_PACKAGE_ARTIFACT_DIR);
    fs::create_dir_all(&artifact_root).map_err(|error| {
        vec![fhir_issue_diag(
            input,
            "fhir_artifact_dir_failed",
            error.to_string(),
        )]
    })?;
    let package_path = artifact_root.join(FHIR_PACKAGE_FILE);
    fs::write(&package_path, &bytes).map_err(|error| {
        vec![fhir_issue_diag(
            input,
            "fhir_artifact_write_failed",
            error.to_string(),
        )]
    })?;
    crate::nucleum::attach_canonical_mir(&mut nucleum, &mut package.nodes)?;
    let nucleum_path = crate::nucleum::write_nucleum_sidecar(&artifact_root, &nucleum)?;
    Ok(PackageFhir {
        root: artifact_root,
        package_path,
        nucleum_path: Some(nucleum_path),
    })
}

/// Read the compiler-view manifest for a package spec, if the package is
/// manifest-backed (the same `manifest_path_for_spec` probe the loader uses).
fn manifest_for_spec(
    spec: &PackageSpec,
) -> Result<Option<crate::manifest::FaberManifest>, Box<Diagnostic>> {
    if !spec.manifest_backed {
        return Ok(None);
    }
    let Some(path) = crate::frontmatter::manifest_path_for_spec(spec) else {
        return Err(Box::new(
            crate::package_diagnostic_error(
                "package manifest faber.toml is missing after discovery validated it",
            )
            .with_file(
                spec.package_root
                    .join(crate::MANIFEST_FILE)
                    .display()
                    .to_string(),
            )
            .with_arg("issue", "package_manifest_missing_after_validation"),
        ));
    };
    crate::manifest::read_manifest(&path).map(Some)
}

/// Package identity from the manifest, or a package-root-derived fallback for
/// legacy manifestless inputs.
fn package_identity(
    package: &AnalyzedProgram,
    manifest: Option<&crate::manifest::FaberManifest>,
) -> PackageIdentityWire {
    match manifest {
        Some(manifest) => PackageIdentityWire {
            name: manifest.package.name.clone(),
            version: manifest.package.version.clone(),
            edition: manifest.package.edition.clone(),
        },
        None => PackageIdentityWire {
            name: package.spec.package_root.file_name().map_or_else(
                || "package".to_owned(),
                |name| name.to_string_lossy().into_owned(),
            ),
            version: "0.1.0".to_owned(),
            edition: "2026".to_owned(),
        },
    }
}

/// Dependency coordinates from `faber.toml [dependencies]` plus `faber.lock`
/// identity when a lock record exists (version + lock identity from the lock;
/// checksums are a Stage 5 store concern).
fn dependency_records(
    package: &AnalyzedProgram,
    manifest: Option<&crate::manifest::FaberManifest>,
) -> Result<Vec<PackageDependencyWire>, Vec<Diagnostic>> {
    let Some(manifest) = manifest else {
        return Ok(Vec::new());
    };
    let lock = crate::lockfile::read_lock(&package.spec.package_root)
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
            version: locked.map_or_else(|| manifest_version.clone(), |entry| entry.version.clone()),
            lock_identity: locked.map_or_else(
                || FHIR_PACKAGE_UNLOCKED_LOCK_IDENTITY.to_owned(),
                |entry| entry.source.clone(),
            ),
            checksum: None,
        });
    }
    Ok(records)
}

/// Local import links for one unit: walk its HIR import declarations, resolve
/// local imports (the same resolution the direct path uses), and record each
/// binding → package-root-relative target module path.
fn local_links_for_unit(
    package: &AnalyzedProgram,
    unit: &AnalyzedProgramNode,
    library_resolver: &crate::library::LibraryResolver,
    package_root: &Path,
) -> Vec<LocalLinkWire> {
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
    links
}

/// Library import references for one unit (binding/package/module — no
/// absolute paths).
fn library_imports_for_unit(unit: &AnalyzedProgramNode) -> Vec<LibraryImportWire> {
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
    handroll::sha256_hex(bytes)
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

#[cfg(test)]
#[path = "fhir_pkg001_issue_test.rs"]
mod fhir_pkg001_issue_test;
