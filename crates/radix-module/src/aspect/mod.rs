//! HIR aspect recognition and reporting substrate.
//!
//! Aspects are small, named verification targets over checked HIR. This module
//! owns the data-first report model, Lean checker integration, and
//! coverage summaries used before any public CLI behavior is exposed.

mod air_mask_proof;
mod air_purity_proof;
mod air_unroll_proof;
mod coverage;
pub mod lean_checker;
mod obligation;
mod report;
mod verifier;

pub use air_mask_proof::MaskNeutralityFamily;
pub use air_purity_proof::{GateHoleIssue, GatePurityFinding, PurityTerm};
pub use air_unroll_proof::{UnrollRoute, UnrollRouteFacts};
pub use coverage::{
    HirAspectAssumption, HirAspectAssumptionStatus, HirAspectCoverageEntry, HirAspectCoverageReason,
    HirAspectCoverageReport, HirAspectCoverageStatus, render_hir_aspect_coverage, summarize_hir_aspect_coverage,
};
pub use lean_checker::{
    AxiomAudit, AxiomDependency, AxiomVerdict, LakeResult, LeanAspectChecker, LeanAspectCheckerConfig, SorryVerdict,
    combined_verdict, normalize_result, parse_axiom_output,
};
pub use obligation::{HirAspectBackendKind, HirAspectObligation, HirAspectObligationId};
pub use report::{
    HirAspectDetail, HirAspectKind, HirAspectNoSpanReason, HirAspectPrecondition, HirAspectReason, HirAspectRecord,
    HirAspectRecordKind, HirAspectReport, HirAspectSpan, report_hir_aspects, report_hir_aspects_with_interner,
};
pub use verifier::{HirAspectCheckResult, HirAspectCheckStatus, HirAspectEvidence, HirAspectVerifier};
