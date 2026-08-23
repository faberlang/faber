//! End-to-end aspect-report integration over the AIR corpus exempla
//! (`corpus/aspect/air-aspect-{admit,decline}.fab`).
//!
//! Proves the report surface recognizes the proven AIR aspect kinds from real
//! Faber source through the shared analysis path: admitted shapes produce
//! `UnrollEquivalence`/`GatePurity` records with their route/term details,
//! declined shapes produce none.  Recognition is the machine-checked-structural
//! surface only — these tests assert no proof verdict.

use super::{report_hir_aspects_with_interner, HirAspectDetail, HirAspectKind, HirAspectRecordKind, HirAspectReport};
use crate::codegen::Target;
use crate::driver::{analyze_source, AnalyzeRequest};
use crate::test_support::{corpus_file, test_session};

fn report_for_exemplum(rel: &str) -> HirAspectReport {
    let path = corpus_file(rel);
    let source = std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("read {path:?}: {err}"));
    let session = test_session(Target::HirRust);
    let unit = analyze_source(&session, rel, &source, AnalyzeRequest::default())
        .unwrap_or_else(|diagnostics| panic!("analyze {rel} failed: {diagnostics:?}"));
    report_hir_aspects_with_interner(&unit.hir, &unit.types, &unit.interner)
}

fn air_records(report: &HirAspectReport, kind: HirAspectKind) -> Vec<&super::HirAspectRecord> {
    report
        .records()
        .iter()
        .filter(|record| record.aspect == kind)
        .collect()
}

#[test]
fn admit_exemplum_surfaces_unroll_equivalence_and_gate_purity() {
    let report = report_for_exemplum("aspect/air-aspect-admit.fab");

    // Line 1 admit: one literal-range itera ab — route Ab, lanes in source order.
    let unroll = air_records(&report, HirAspectKind::UnrollEquivalence);
    assert_eq!(unroll.len(), 1, "exactly one unroll record: {unroll:?}");
    assert_eq!(unroll[0].record_kind, HirAspectRecordKind::Recognized);
    match &unroll[0].detail {
        HirAspectDetail::UnrollEquivalence { route, lanes } => {
            assert_eq!(*route, super::UnrollRoute::Ab);
            assert_eq!(lanes, &[0, 1, 2, 3]);
        }
        other => panic!("unroll detail: {other:?}"),
    }

    // Line 3 admit: the pure let/return function carries a GatePurity term;
    // every other top-level function declines silently (no record).
    let purity = air_records(&report, HirAspectKind::GatePurity);
    assert_eq!(purity.len(), 1, "exactly one gate-purity record: {purity:?}");
    assert_eq!(purity[0].record_kind, HirAspectRecordKind::Recognized);
    assert!(
        matches!(purity[0].detail, HirAspectDetail::GatePurity { .. }),
        "gate-purity detail: {:?}",
        purity[0].detail
    );
}

#[test]
fn decline_exemplum_emits_no_air_obligations() {
    let report = report_for_exemplum("aspect/air-aspect-decline.fab");

    // Non-literal range and binary-arithmetic return: both aspect kinds
    // decline, so neither appears in any record kind.
    assert!(
        air_records(&report, HirAspectKind::UnrollEquivalence).is_empty(),
        "declined unroll shape must emit no record"
    );
    assert!(
        air_records(&report, HirAspectKind::GatePurity).is_empty(),
        "declined purity shape must emit no record"
    );
}
