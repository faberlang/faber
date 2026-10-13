//! Nucleum fragment package metadata: body artifacts, cache keys, import
//! fail-closed, and post-composition symbol omission.
//!
//! The FHIR envelope stays the existing unit-HIR interchange. Fragment bodies
//! travel as a canonical MIR sidecar (`nucleum-fragments.bin`) next to
//! `package.fhirpkg` (goal OQ2).

use crate::analyze::{AnalyzedProgram, AnalyzedProgramNode};
use radix_module::diagnostics::{Diagnostic, DiagnosticPhase};
use radix_module::hir::NucleumRole;
use radix_module::hir::package::{
    NucleumPackageError, NucleumProgramMetadata, collect_nucleum_metadata, encode_nucleum_metadata,
};
use std::path::{Path, PathBuf};

/// Sidecar file written beside `package.fhirpkg`.
pub const NUCLEUM_FRAGMENT_ARTIFACT_FILE: &str = "nucleum-fragments.bin";

/// Relative path key for a unit: its file name when present, otherwise the
/// full display path. The same key names the unit in fragment metadata and the
/// canonical MIR sidecar.
fn node_relative_path(node: &AnalyzedProgramNode) -> String {
    node.path.file_name().map_or_else(
        || node.path.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// Collect and validate nucleum metadata for an analyzed package.
///
/// Signature-only fragments fail closed. The concrete fragment call graph
/// must be acyclic after import resolution.
///
/// # Errors
///
/// Returns a package diagnostic when a fragment is signature-only or the
/// call graph is cyclic.
pub fn metadata_for_program(
    program: &AnalyzedProgram,
) -> Result<NucleumProgramMetadata, Diagnostic> {
    let mut metadata = NucleumProgramMetadata::new();
    for node in program.nodes.values() {
        let relative = node_relative_path(node);
        metadata.merge(
            collect_nucleum_metadata(&node.analysis, &relative)
                .map_err(|error| nucleum_diagnostic(&node.path, &error))?,
        );
    }
    metadata
        .require_importable_fragment_bodies()
        .and_then(|()| metadata.require_acyclic())
        .map_err(|error| nucleum_diagnostic(&program.spec.package_root, &error))?;
    Ok(metadata)
}

/// Export names that remain in target symbol tables after composition.
///
/// Nucleum fragments are omitted; ordinary exports and `Entry` kernels stay.
#[must_use]
pub fn target_symbol_exports(program: &AnalyzedProgram) -> Vec<String> {
    let omitted = metadata_for_program(program)
        .map(|metadata| metadata.omitted_target_symbol_names())
        .unwrap_or_default();
    let mut names = program
        .nodes
        .values()
        .flat_map(|node| node.export_names.iter().cloned())
        .filter(|name| !omitted.iter().any(|fragment| fragment == name))
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

/// Attach canonical MIR bodies to metadata for every unit that exports a fragment.
///
/// # Errors
///
/// Returns lowering diagnostics when a fragment unit cannot produce MIR, or a
/// package diagnostic when the canonical MIR bytes are still missing.
pub fn attach_canonical_mir(
    metadata: &mut NucleumProgramMetadata,
    nodes: &mut std::collections::BTreeMap<crate::analyze::ModuleId, AnalyzedProgramNode>,
) -> Result<(), Vec<Diagnostic>> {
    let fragment_paths = metadata
        .items
        .iter()
        .filter(|item| item.role == NucleumRole::Fragment)
        .map(|item| item.relative_path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    if fragment_paths.is_empty() {
        return Ok(());
    }
    for node in nodes.values_mut() {
        let relative = node_relative_path(node);
        if !fragment_paths.contains(&relative) {
            continue;
        }
        let lowered = radix_mir_lower::lower::lower_analyzed_unit(
            node.analysis.application_view(),
            radix_mir_lower::lower::MirLoweringOptions::default(),
        )
        .map_err(|errors| {
            errors
                .into_iter()
                .map(|error| {
                    crate::package_diagnostic_error(error.message)
                        .with_phase(DiagnosticPhase::Analysis)
                        .with_file(node.path.display().to_string())
                        .with_arg("issue", error.issue)
                })
                .collect::<Vec<_>>()
        })?;
        metadata.attach_canonical_mir(lowered.validated.program(), &relative);
    }
    metadata
        .require_canonical_mir()
        .map_err(|error| vec![nucleum_diagnostic(Path::new("nucleum-fragments"), &error)])
}

/// Write the nucleum sidecar next to the FHIR package artifact.
///
/// # Errors
///
/// Returns a package diagnostic when the sidecar cannot be encoded or written.
pub fn write_nucleum_sidecar(
    artifact_root: &Path,
    metadata: &NucleumProgramMetadata,
) -> Result<PathBuf, Vec<Diagnostic>> {
    let path = artifact_root.join(NUCLEUM_FRAGMENT_ARTIFACT_FILE);
    let bytes = encode_nucleum_metadata(metadata).map_err(|error| {
        vec![crate::package_diagnostic_error(format!(
            "could not encode nucleum fragment artifacts: {error}"
        ))]
    })?;
    std::fs::write(&path, bytes).map_err(|error| {
        vec![crate::package_diagnostic_error(format!(
            "could not write nucleum fragment artifacts: {error}"
        ))]
    })?;
    Ok(path)
}

fn nucleum_diagnostic(path: &Path, error: &NucleumPackageError) -> Diagnostic {
    crate::package_diagnostic_error(error.to_string())
        .with_phase(DiagnosticPhase::Analysis)
        .with_file(path.display().to_string())
        .with_arg("issue", error.issue())
        .with_arg("reason", error.reason())
        .with_arg("site", "import")
}

#[cfg(test)]
#[path = "nucleum_test.rs"]
mod tests;
