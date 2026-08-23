//! HIR aspect recognition and reporting substrate.
//!
//! Aspects are small, named verification targets over checked HIR. This module
//! owns the data-first report model, TLA emission, checker integration, and
//! coverage summaries used before any public CLI behavior is exposed.

mod air_mask_proof;
mod coverage;
pub mod lean_checker;
mod obligation;
mod report;
mod tla;
pub(crate) mod tla_checker;
mod verifier;

pub use air_mask_proof::MaskNeutralityFamily;
pub use coverage::{
    render_hir_aspect_coverage, summarize_hir_aspect_coverage, HirAspectAssumption, HirAspectAssumptionStatus,
    HirAspectCoverageEntry, HirAspectCoverageReason, HirAspectCoverageReport, HirAspectCoverageStatus,
};
pub use lean_checker::{
    combined_verdict, normalize_result, parse_axiom_output, AxiomAudit, AxiomDependency, AxiomVerdict, LakeResult,
    LeanAspectChecker, LeanAspectCheckerConfig, SorryVerdict,
};
pub use obligation::{HirAspectBackendKind, HirAspectObligation, HirAspectObligationId};
pub use report::{
    report_hir_aspects, report_hir_aspects_with_interner, HirAspectDetail, HirAspectKind, HirAspectNoSpanReason,
    HirAspectPrecondition, HirAspectReason, HirAspectRecord, HirAspectRecordKind, HirAspectReport, HirAspectSpan,
};
pub use tla::{emit_tla_modules, HirAspectTlaModule};
pub use tla_checker::{check_tla_modules, TlaAspectChecker, TlaAspectCheckerConfig, TlaAspectVerifier};
pub use verifier::{HirAspectCheckResult, HirAspectCheckStatus, HirAspectEvidence, HirAspectVerifier};
