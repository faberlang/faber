//! Tests for HIR aspect coverage summarization and rendering.
//! Proves: coverage merges checker results with report records; rendering
//! is deterministic; counterexamples appear in failed-checker output;
//! skipped records stay out of success.

use super::{
    HirAspectAssumption, HirAspectCoverageEntry, HirAspectCoverageReason, HirAspectCoverageReport,
    HirAspectCoverageStatus, render_hir_aspect_coverage, summarize_hir_aspect_coverage,
};
use crate::aspect::verifier::{HirAspectCheckResult, HirAspectCheckStatus, HirAspectEvidence};
use crate::aspect::{
    HirAspectBackendKind, HirAspectKind, HirAspectObligationId, HirAspectPrecondition, HirAspectReason,
    HirAspectRecord, HirAspectReport, HirAspectSpan,
};
use crate::hir::HirId;
use crate::lexer::Span;

fn span(start: u32, end: u32) -> HirAspectSpan {
    HirAspectSpan::Source(Span::new(start, end))
}

fn recognized(id: u32, start: u32) -> HirAspectRecord {
    HirAspectRecord::recognized(HirAspectKind::DiscerneOmnia, span(start, start + 5), Some(HirId(id)))
}

fn obligation_id(aspect: HirAspectKind, hir_id: u32, ordinal: usize) -> HirAspectObligationId {
    HirAspectObligationId { aspect, node_id: Some(HirId(hir_id)), ordinal }
}

fn check(status: HirAspectCheckStatus, summary: &str) -> HirAspectCheckResult {
    HirAspectCheckResult {
        status,
        evidence: HirAspectEvidence(summary.to_owned()),
        counterexample: Vec::new(),
        backend: HirAspectBackendKind::Tla,
    }
}

#[test]
fn hir_aspect_coverage_maps_checker_results_without_hiding_absence() {
    let report = HirAspectReport::from_records(
        HirAspectPrecondition::CheckedHir,
        vec![
            recognized(1, 10),
            recognized(2, 20),
            recognized(3, 30),
            recognized(4, 40),
            recognized(5, 50),
            HirAspectRecord::not_proven(
                HirAspectKind::DiscerneOmnia,
                span(60, 65),
                Some(HirId(6)),
                HirAspectReason::MissingExplicitVariants,
            ),
            HirAspectRecord::skipped_candidate(
                HirAspectKind::DiscerneOmnia,
                span(70, 75),
                Some(HirId(7)),
                HirAspectReason::NonEnumScrutinee,
            ),
        ],
    );
    let checks_list = vec![
        check(HirAspectCheckStatus::Proven, "ok"),
        HirAspectCheckResult {
            status: HirAspectCheckStatus::Failed,
            evidence: HirAspectEvidence("Error: Invariant Totality is violated.".to_owned()),
            counterexample: vec!["State 1".to_owned()],
            backend: HirAspectBackendKind::Tla,
        },
        check(
            HirAspectCheckStatus::ToolchainMissing,
            "the TLC jar path does not point to a file",
        ),
        check(HirAspectCheckStatus::Error, "unexpected TLC output"),
    ];

    let coverage = summarize_hir_aspect_coverage(&report, &checks_list, HirAspectBackendKind::Tla);

    assert_eq!(
        coverage
            .entries()
            .iter()
            .map(|entry| entry.status)
            .collect::<Vec<_>>(),
        vec![
            HirAspectCoverageStatus::Proven,
            HirAspectCoverageStatus::Failed,
            HirAspectCoverageStatus::Abstracted,
            HirAspectCoverageStatus::Abstracted,
            HirAspectCoverageStatus::Abstracted,
            HirAspectCoverageStatus::Failed,
            HirAspectCoverageStatus::Skipped,
        ]
    );
    assert_eq!(coverage.entries()[0].reason, HirAspectCoverageReason::CheckerPassed);
    assert!(matches!(
        coverage.entries()[2].reason,
        HirAspectCoverageReason::CheckerUnavailable(_)
    ));
    assert!(matches!(coverage.entries()[3].reason, HirAspectCoverageReason::CheckerError(_)));
    assert_eq!(
        coverage.entries()[4].reason,
        HirAspectCoverageReason::MissingCheckerResult { id: obligation_id(HirAspectKind::DiscerneOmnia, 5, 4) }
    );
    assert_eq!(
        coverage.entries()[5].reason,
        HirAspectCoverageReason::AspectNotProven(HirAspectReason::MissingExplicitVariants)
    );
    assert_eq!(
        coverage.entries()[6].reason,
        HirAspectCoverageReason::SkippedCandidate(HirAspectReason::NonEnumScrutinee)
    );
}

#[test]
fn hir_aspect_coverage_renders_checked_and_trusted_assumptions_distinctly() {
    let coverage = HirAspectCoverageReport::new(
        vec![HirAspectCoverageEntry::new(
            HirAspectKind::DiscerneOmnia,
            span(10, 15),
            Some(HirId(1)),
            HirAspectBackendKind::Tla,
            HirAspectCoverageStatus::Proven,
            HirAspectCoverageReason::CheckerPassed,
            vec!["finite_enum_domain".to_owned()],
        )],
        vec![
            HirAspectAssumption::trusted("external_io_pure", "operator body is outside checked HIR"),
            HirAspectAssumption::checked("finite_enum_domain", "enum variants came from checked HIR"),
        ],
    );

    let rendered = render_hir_aspect_coverage(&coverage);

    assert_eq!(
        rendered,
        "aspect coverage\n\
- discerne_omnia span=10..15 hir=1: proven under finite_enum_domain (checker passed)\n\
assumptions\n\
- checked finite_enum_domain: enum variants came from checked HIR\n\
- trusted external_io_pure: operator body is outside checked HIR\n"
    );
}

#[test]
fn hir_aspect_coverage_rendering_is_deterministic() {
    let first = HirAspectCoverageReport::new(
        vec![
            HirAspectCoverageEntry::new(
                HirAspectKind::DiscerneOmnia,
                span(30, 35),
                Some(HirId(3)),
                HirAspectBackendKind::Tla,
                HirAspectCoverageStatus::Skipped,
                HirAspectCoverageReason::SkippedCandidate(HirAspectReason::MissingScrutineeType),
                Vec::new(),
            ),
            HirAspectCoverageEntry::new(
                HirAspectKind::DiscerneOmnia,
                span(10, 15),
                Some(HirId(1)),
                HirAspectBackendKind::Tla,
                HirAspectCoverageStatus::Proven,
                HirAspectCoverageReason::CheckerPassed,
                Vec::new(),
            ),
        ],
        vec![
            HirAspectAssumption::trusted("z_trusted", "trusted"),
            HirAspectAssumption::checked("a_checked", "checked"),
        ],
    );
    let second = first.clone();

    assert_eq!(first.render_text(), second.render_text());
    assert!(first.render_text().find("span=10..15").unwrap() < first.render_text().find("span=30..35").unwrap());
    assert!(
        first.render_text().find("checked a_checked").unwrap() < first.render_text().find("trusted z_trusted").unwrap()
    );
}

#[test]
fn hir_aspect_coverage_failed_checker_render_includes_counterexample() {
    let coverage = HirAspectCoverageReport::new(
        vec![HirAspectCoverageEntry::new(
            HirAspectKind::DiscerneOmnia,
            span(10, 15),
            Some(HirId(1)),
            HirAspectBackendKind::Tla,
            HirAspectCoverageStatus::Failed,
            HirAspectCoverageReason::CheckerFailed {
                summary: "Error: Invariant Totality is violated.".to_owned(),
                counterexample: vec!["State 1".to_owned(), "State 2".to_owned()],
            },
            Vec::new(),
        )],
        Vec::new(),
    );

    assert!(
        coverage
            .render_text()
            .contains("counterexample: State 1 | State 2")
    );
    assert_eq!(coverage.entries()[0].status, HirAspectCoverageStatus::Failed);
}

#[test]
fn hir_aspect_coverage_keeps_skipped_records_out_of_success() {
    let report = HirAspectReport::from_records(
        HirAspectPrecondition::CheckedHir,
        vec![HirAspectRecord::skipped_candidate(
            HirAspectKind::DiscerneOmnia,
            span(10, 15),
            Some(HirId(1)),
            HirAspectReason::MultipleScrutinees,
        )],
    );

    let coverage = summarize_hir_aspect_coverage(&report, &[], HirAspectBackendKind::Tla);

    assert_eq!(coverage.entries()[0].status, HirAspectCoverageStatus::Skipped);
    assert_eq!(
        coverage.entries()[0].reason,
        HirAspectCoverageReason::SkippedCandidate(HirAspectReason::MultipleScrutinees)
    );
}

#[test]
fn hir_aspect_coverage_empty_report_has_no_entries() {
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, vec![]);
    let coverage = summarize_hir_aspect_coverage(&report, &[], HirAspectBackendKind::Tla);

    assert_eq!(coverage.entries().len(), 0);
}

#[test]
fn hir_aspect_coverage_empty_checks_list_maps_all_to_abstracted() {
    let report = HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, vec![recognized(1, 10)]);
    let coverage = summarize_hir_aspect_coverage(&report, &[], HirAspectBackendKind::Tla);

    assert_eq!(coverage.entries().len(), 1);
    assert_eq!(coverage.entries()[0].status, HirAspectCoverageStatus::Abstracted);
}

#[test]
fn hir_aspect_coverage_reports_timeout_distinctly_from_checker_error() {
    let report =
        HirAspectReport::from_records(HirAspectPrecondition::CheckedHir, vec![recognized(1, 10), recognized(2, 20)]);
    let checks_list = vec![
        check(HirAspectCheckStatus::Timeout, "lake build timed out after 30s"),
        check(HirAspectCheckStatus::Error, "unexpected output"),
    ];

    let coverage = summarize_hir_aspect_coverage(&report, &checks_list, HirAspectBackendKind::Tla);

    assert_eq!(coverage.entries()[0].status, HirAspectCoverageStatus::Abstracted);
    assert!(matches!(
        coverage.entries()[0].reason,
        HirAspectCoverageReason::CheckerTimeout(_)
    ));
    assert_eq!(coverage.entries()[1].status, HirAspectCoverageStatus::Abstracted);
    assert!(matches!(coverage.entries()[1].reason, HirAspectCoverageReason::CheckerError(_)));
    let rendered = coverage.render_text();
    assert!(rendered.contains("checker timed out: lake build timed out after 30s"));
    assert!(rendered.contains("checker error: unexpected output"));
}
