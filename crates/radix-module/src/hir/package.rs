//! FHIR **package** build and load (source-independent package reconstruction).
//!
//! This module is the compiler-side owner of the package envelope contract:
//! [`build_package`] assembles a [`FhirPackage`] from analyzed units plus
//! package metadata (the wire types and codec live in `radix-hir-fhir`), and
//! [`load_package`] reconstructs a [`LoadedHirPackage`] whose units are full
//! [`AnalyzedModule`]s — re-entering the compiler at the codegen boundary with
//! no lexer, parser, resolver, semantic-analysis, or lowering pass.
//!
//! [`load_package`] never treats unit-local `DefId`/`HirId`/`TypeId`/`Symbol`
//! values as package-global; cross-unit references travel as module/export
//! identities (the module table + serialized file interfaces inside each
//! unit's resolver snapshot).

use crate::driver::AnalyzedModule;
use radix_hir_fhir::{FhirPackageModule, SCHEMA_VERSION};

pub use radix_hir_fhir::{
    decode_package, encode_package, DecodedFhirPackage, FhirPackage, FhirPackageError, LibraryImportWire,
    LocalLinkWire, PackageDependencyWire, PackageIdentityWire, PACKAGE_SCHEMA_VERSION,
};

/// One analyzed unit input for package assembly.
///
/// All strings are package-relative or provider-qualified — never absolute
/// checkout paths (contract gate).
pub struct PackageUnitInput<'a> {
    /// Package-root-relative canonical path (e.g. `src/main.fab`).
    pub relative_path: String,
    pub module_segments: Vec<String>,
    pub is_entry: bool,
    /// Public export names of this module (source-free surface for codegen
    /// assembly).
    pub export_names: Vec<String>,
    /// Local import links (binding → target module path), the explicit link
    /// table captured at analysis time.
    pub local_links: Vec<LocalLinkWire>,
    /// Library imports referenced by this module (binding/package/module).
    pub library_imports: Vec<LibraryImportWire>,
    /// SHA-256 hex of the original source bytes — also stored as the unit
    /// artifact's `SourceIdentity.content_hash` and re-checked on decode.
    pub content_hash: String,
    pub analysis: &'a AnalyzedModule,
}

/// Assemble a [`FhirPackage`] from analyzed units and package metadata.
///
/// Deterministic by construction: modules are sorted by canonical relative
/// path, export names and library imports are sorted, and dependencies are
/// sorted by name. Each unit is serialized through the existing unit path
/// (`crate::hir::serialize::from_analyzed` + `radix_hir_fhir::encode`), so
/// package work never duplicates the unit artifact contract.
///
/// # Errors
///
/// Returns [`FhirPackageError`] when package identity, module ordering,
/// dependency metadata, or unit serialization is invalid.
pub fn build_package(
    identity: PackageIdentityWire,
    entry_path: &str,
    entry_frontmatter: Option<String>,
    mut modules: Vec<PackageUnitInput<'_>>,
    mut dependencies: Vec<PackageDependencyWire>,
) -> Result<FhirPackage, FhirPackageError> {
    modules.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));

    let mut wire_modules = Vec::with_capacity(modules.len());
    for input in modules {
        let mut artifact =
            crate::hir::serialize::from_analyzed(input.analysis, &input.content_hash, &input.relative_path);
        // Package artifacts are deterministic interchange: `analysis_stamp`
        // is a live-process run counter (monotonic per process), not content.
        // Reset it so rebuilding the same source yields byte-identical
        // packages — required for stable package content hashes in the store
        // and the deterministic-serialization contract gate. Loaded units
        // reconstruct the stamp via `AnalysisStamp::from_persisted(0)`; the
        // stamp is only meaningful while fields of one run are assembled
        // in-process, which package loading does not do.
        artifact.analysis_stamp = 0;
        let mut export_names = input.export_names;
        export_names.sort();
        export_names.dedup();
        let mut local_links = input.local_links;
        local_links.sort_by(|left, right| left.binding.cmp(&right.binding));
        let mut library_imports = input.library_imports;
        library_imports.sort_by(|left, right| {
            (&left.binding, &left.package, &left.module).cmp(&(&right.binding, &right.package, &right.module))
        });
        wire_modules.push(FhirPackageModule {
            path: input.relative_path,
            module_segments: input.module_segments,
            is_entry: input.is_entry,
            export_names,
            local_links,
            library_imports,
            source_hash: input.content_hash,
            unit_schema_version: SCHEMA_VERSION,
            unit: radix_hir_fhir::encode(&artifact).map_err(FhirPackageError::Malformed)?,
        });
    }

    dependencies.sort_by(|left, right| left.name.cmp(&right.name));

    Ok(FhirPackage {
        package_schema_version: PACKAGE_SCHEMA_VERSION,
        identity,
        entry_path: entry_path.to_owned(),
        entry_frontmatter,
        modules: wire_modules,
        dependencies,
    })
}

/// One module of a loaded package: envelope metadata plus a fully
/// reconstructed [`AnalyzedModule`].
pub struct LoadedHirModule {
    pub relative_path: String,
    pub module_segments: Vec<String>,
    pub is_entry: bool,
    pub export_names: Vec<String>,
    pub local_links: Vec<LocalLinkWire>,
    pub library_imports: Vec<LibraryImportWire>,
    pub unit: AnalyzedModule,
}

/// A source-independently loaded FHIR package.
pub struct LoadedHirPackage {
    pub identity: PackageIdentityWire,
    pub entry_path: String,
    pub entry_frontmatter: Option<String>,
    pub modules: Vec<LoadedHirModule>,
    pub dependencies: Vec<PackageDependencyWire>,
}

/// Load a FHIR package from `postcard` bytes.
///
/// The format crate's [`decode_package`] runs the package version gate, the
/// per-unit gates (unit schema version + F3 + U2.3 referential integrity +
/// source-hash consistency), and the package-level module/dependency checks;
/// this function only reconstructs arenas per unit via
/// [`crate::hir::artifact::reconstruct_unit`] (U2.2), the same path the
/// single-unit loader uses. Never panics on the load path (F7) and never
/// invokes frontend phases.
///
/// # Errors
///
/// Returns [`FhirPackageError`] when package decoding, validation, or unit
/// reconstruction fails.
pub fn load_package(bytes: &[u8]) -> Result<LoadedHirPackage, FhirPackageError> {
    let decoded = decode_package(bytes)?;
    let mut modules = Vec::with_capacity(decoded.modules.len());
    for module in decoded.modules {
        let unit = crate::hir::artifact::reconstruct_unit(module.unit).map_err(|error| {
            FhirPackageError::MalformedUnit { module: module.path.clone(), detail: error.to_string() }
        })?;
        modules.push(LoadedHirModule {
            relative_path: module.path,
            module_segments: module.module_segments,
            is_entry: module.is_entry,
            export_names: module.export_names,
            local_links: module.local_links,
            library_imports: module.library_imports,
            unit,
        });
    }
    Ok(LoadedHirPackage {
        identity: decoded.identity,
        entry_path: decoded.entry_path,
        entry_frontmatter: decoded.entry_frontmatter,
        modules,
        dependencies: decoded.dependencies,
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "package_test.rs"]
mod tests;
