//! Loose-parts test seam for synthetic HIR backend fixtures.
//!
//! Synthetic-HIR fixtures — shapes source-level analysis helpers cannot
//! express, including dangling-`DefId` error proofs — are packed with the
//! driver's artifact-loader constructor (empty resolver/registry side tables)
//! plus [`crate::semantic::analyze_function_facts`], the same fact computation
//! the retired test-only entry performed, then forwarded through the
//! production codegen entry with the driver-default Latin keyword surface.
//!
//! This seam is a constructor plus one forward: no per-target dispatch lives
//! here, so tests exercise the single production chain and its effect seams
//! exactly as package builds do.
//!
//! # Errors
//!
//! Returns [`CodegenError`] as [`generate_from_analyzed_with_options`] does
//! for the synthetic module.

use crate::codegen::{CodegenError, OutputMode, Target, generate_from_analyzed_with_options};
use crate::diagnostics::DiagnosticArg;
use crate::hir::HirModule;
use crate::lexer::Interner;
use crate::locale::{KeywordSurface, latin_locale_pack};
use crate::semantic::TypeTable;

/// Build a minimal analyzed carrier from hand-constructed HIR and dispatch it
/// through the production entry.
///
/// # Errors
///
/// Returns [`CodegenError`] for a malformed type-table snapshot or a backend
/// emission error.
pub fn generate_from_loose_parts(
    target: Target,
    hir: &HirModule,
    types: &TypeTable,
    interner: &Interner,
) -> Result<crate::Output, CodegenError> {
    let type_table = TypeTable::from_snapshot(types.snapshot()).map_err(|message| CodegenError {
        message: format!("type table snapshot round-trip failed: {message}"),
        args: vec![DiagnosticArg::new("issue", "type_table_snapshot_failed")],
    })?;
    let analysis = crate::driver::AnalyzedModule::from_loaded(
        // Synthetic modules never join a real analysis snapshot; stamp 0 is
        // unreachable for analyzed units (`AnalysisStamp::next` starts at 1).
        crate::semantic::AnalysisStamp::from_persisted(0),
        interner.clone(),
        // `TypeTable` is not `Clone`; the snapshot round-trip is the copy
        // path (it can only fail on tables missing primitives that
        // `TypeTable::new()` preloads, so a live table always round-trips).
        type_table,
        crate::semantic::Resolver::default(),
        hir.clone(),
        None,
        crate::hir::LibraryRegistry::default(),
        crate::semantic::AnnotationContractMetadata::default(),
        crate::semantic::analyze_function_facts(hir, types),
        crate::semantic::ResolvedUseIndex::default(),
        crate::semantic::QualifiedIdentityTable::default(),
        crate::semantic::FunctionProvenanceTable::default(),
        crate::semantic::RadixLaneMetadata::default(),
        Vec::new(),
        radix_hir::TensorEdgePolicyTable::default(),
        #[cfg(feature = "mir-core")]
        crate::driver::GraphicsSourceFacts::default(),
        Vec::new(),
    );
    let latin = latin_locale_pack();
    let surface = KeywordSurface::new(&latin);
    generate_from_analyzed_with_options(target, &analysis, &surface, OutputMode::Application, None)
}
