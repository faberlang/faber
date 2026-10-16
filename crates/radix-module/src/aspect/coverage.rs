//! Aspect coverage summary and rendering.
//!
//! Joins a [`HirAspectReport`] with verifier check results to produce
//! per-obligation coverage entries and recorded assumptions. Owns the
//! human-readable coverage report format consumed by the developer CLI;
//! recognition and checking live in [`report`] and [`lean_checker`]
//! respectively.

use super::verifier::{HirAspectCheckResult, HirAspectCheckStatus};
use super::{
    HirAspectBackendKind, HirAspectKind, HirAspectObligationId, HirAspectReason, HirAspectRecord, HirAspectRecordKind,
    HirAspectReport, HirAspectSpan,
};
use crate::hir::HirId;
use rustc_hash::FxHashMap;

#[cfg(test)]
#[path = "coverage_test.rs"]
mod tests;

const COVERAGE_ENTRIES_HEADER: &str = "aspect coverage\n";
const COVERAGE_ASSUMPTIONS_HEADER: &str = "assumptions\n";
const COVERAGE_NO_ASSUMPTIONS: &str = "- none\n";

#[must_use]
pub fn summarize_hir_aspect_coverage(
    report: &HirAspectReport,
    check_results: &[HirAspectCheckResult],
    backend: HirAspectBackendKind,
) -> HirAspectCoverageReport {
    // Build obligation-ID-keyed map from recognized records matching
    // check results by position (one result per recognized record,
    // in the order they appear in the report).
    let mut checks = FxHashMap::default();
    let mut check_iter = check_results.iter();
    for (index, record) in report.records().iter().enumerate() {
        if record.record_kind == HirAspectRecordKind::Recognized
            && let Some(result) = check_iter.next()
        {
            checks.insert(obligation_id(record, index), result);
        }
    }
    let entries = report
        .records()
        .iter()
        .enumerate()
        .map(|(index, record)| coverage_entry(index, record, &checks, backend))
        .collect();
    HirAspectCoverageReport::new(entries, Vec::new())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirAspectCoverageReport {
    entries: Vec<HirAspectCoverageEntry>,
    assumptions: Vec<HirAspectAssumption>,
}

impl HirAspectCoverageReport {
    // public constructor: sorts and moves Vecs into struct
    pub fn new(mut entries: Vec<HirAspectCoverageEntry>, mut assumptions: Vec<HirAspectAssumption>) -> Self {
        entries.sort_by_key(HirAspectCoverageEntry::sort_key);
        assumptions.sort_by_key(HirAspectAssumption::sort_key);
        Self { entries, assumptions }
    }

    #[must_use]
    pub fn entries(&self) -> &[HirAspectCoverageEntry] {
        &self.entries
    }

    #[must_use]
    pub fn assumptions(&self) -> &[HirAspectAssumption] {
        &self.assumptions
    }

    #[must_use]
    pub fn render_text(&self) -> String {
        render_hir_aspect_coverage(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirAspectCoverageEntry {
    pub aspect: HirAspectKind,
    pub span: HirAspectSpan,
    pub node_id: Option<HirId>,
    pub backend: HirAspectBackendKind,
    pub status: HirAspectCoverageStatus,
    pub reason: HirAspectCoverageReason,
    pub assumption_names: Vec<String>,
}

impl HirAspectCoverageEntry {
    #[must_use]
    // public constructor: moves assumption_names into struct
    pub fn new(
        aspect: HirAspectKind,
        span: HirAspectSpan,
        node_id: Option<HirId>,
        backend: HirAspectBackendKind,
        status: HirAspectCoverageStatus,
        reason: HirAspectCoverageReason,
        assumption_names: Vec<String>,
    ) -> Self {
        Self { aspect, span, node_id, backend, status, reason, assumption_names }
    }

    fn from_record(
        record: &HirAspectRecord,
        backend: HirAspectBackendKind,
        status: HirAspectCoverageStatus,
        reason: HirAspectCoverageReason,
    ) -> Self {
        Self::new(record.aspect, record.span, record.node_id, backend, status, reason, Vec::new())
    }

    fn sort_key(&self) -> HirAspectCoverageEntrySortKey {
        HirAspectCoverageEntrySortKey {
            span_start: self.span.sort_start(),
            span_end: self.span.sort_end(),
            aspect: self.aspect,
            status: self.status,
            node_id: self.node_id.map_or(u32::MAX, |id| id.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HirAspectCoverageStatus {
    Proven,
    Failed,
    Skipped,
    Abstracted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HirAspectCoverageReason {
    CheckerPassed,
    CheckerFailed {
        summary: String,
        counterexample: Vec<String>,
    },
    CheckerUnavailable(String),
    CheckerError(String),
    CheckerTimeout(String),
    MissingCheckerResult {
        id: HirAspectObligationId,
    },
    AspectNotProven(HirAspectReason),
    SkippedCandidate(HirAspectReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HirAspectAssumption {
    pub name: String,
    pub status: HirAspectAssumptionStatus,
    pub summary: String,
}

impl HirAspectAssumption {
    pub fn checked(name: impl Into<String>, summary: impl Into<String>) -> Self {
        Self { name: name.into(), status: HirAspectAssumptionStatus::Checked, summary: summary.into() }
    }

    pub fn trusted(name: impl Into<String>, summary: impl Into<String>) -> Self {
        Self { name: name.into(), status: HirAspectAssumptionStatus::Trusted, summary: summary.into() }
    }

    fn sort_key(&self) -> (HirAspectAssumptionStatus, String) {
        (self.status, self.name.clone())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum HirAspectAssumptionStatus {
    Checked,
    Trusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct HirAspectCoverageEntrySortKey {
    span_start: u32,
    span_end: u32,
    aspect: HirAspectKind,
    status: HirAspectCoverageStatus,
    node_id: u32,
}

fn coverage_entry(
    index: usize,
    record: &HirAspectRecord,
    checks: &FxHashMap<HirAspectObligationId, &HirAspectCheckResult>,
    backend: HirAspectBackendKind,
) -> HirAspectCoverageEntry {
    match record.record_kind {
        HirAspectRecordKind::Recognized => checked_entry(index, record, checks, backend),
        HirAspectRecordKind::NotProven => HirAspectCoverageEntry::from_record(
            record,
            backend,
            HirAspectCoverageStatus::Failed,
            HirAspectCoverageReason::AspectNotProven(record.reason),
        ),
        HirAspectRecordKind::SkippedCandidate => HirAspectCoverageEntry::from_record(
            record,
            backend,
            HirAspectCoverageStatus::Skipped,
            HirAspectCoverageReason::SkippedCandidate(record.reason),
        ),
    }
}

fn checked_entry(
    index: usize,
    record: &HirAspectRecord,
    checks: &FxHashMap<HirAspectObligationId, &HirAspectCheckResult>,
    backend: HirAspectBackendKind,
) -> HirAspectCoverageEntry {
    let obligation_id = obligation_id(record, index);
    let Some(result) = checks.get(&obligation_id) else {
        return HirAspectCoverageEntry::from_record(
            record,
            backend,
            HirAspectCoverageStatus::Abstracted,
            HirAspectCoverageReason::MissingCheckerResult { id: obligation_id },
        );
    };

    let (status, reason) = coverage_from_check_result(result);
    HirAspectCoverageEntry::from_record(record, backend, status, reason)
}

/// Compute a [`HirAspectObligationId`] from a record and its position in the report.
fn obligation_id(record: &HirAspectRecord, index: usize) -> HirAspectObligationId {
    HirAspectObligationId { aspect: record.aspect, node_id: record.node_id, ordinal: index }
}

fn coverage_from_check_result(result: &HirAspectCheckResult) -> (HirAspectCoverageStatus, HirAspectCoverageReason) {
    match result.status {
        HirAspectCheckStatus::Proven => (HirAspectCoverageStatus::Proven, HirAspectCoverageReason::CheckerPassed),
        HirAspectCheckStatus::Failed => (
            HirAspectCoverageStatus::Failed,
            HirAspectCoverageReason::CheckerFailed {
                summary: result.evidence.0.clone(),
                counterexample: result.counterexample.clone(),
            },
        ),
        HirAspectCheckStatus::ToolchainMissing => (
            HirAspectCoverageStatus::Abstracted,
            HirAspectCoverageReason::CheckerUnavailable(result.evidence.0.clone()),
        ),
        HirAspectCheckStatus::Error => (
            HirAspectCoverageStatus::Abstracted,
            HirAspectCoverageReason::CheckerError(result.evidence.0.clone()),
        ),
        HirAspectCheckStatus::Timeout => (
            HirAspectCoverageStatus::Abstracted,
            HirAspectCoverageReason::CheckerTimeout(result.evidence.0.clone()),
        ),
    }
}

#[must_use]
pub fn render_hir_aspect_coverage(report: &HirAspectCoverageReport) -> String {
    let mut out = String::new();
    out.push_str(COVERAGE_ENTRIES_HEADER);
    for entry in report.entries() {
        out.push_str("- ");
        out.push_str(aspect_label(entry.aspect));
        out.push(' ');
        out.push_str(&span_label(entry.span));
        out.push(' ');
        out.push_str(node_label(entry.node_id).as_str());
        out.push_str(": ");
        out.push_str(status_label(entry.status));
        if !entry.assumption_names.is_empty() {
            out.push_str(" under ");
            out.push_str(&entry.assumption_names.join(", "));
        }
        out.push_str(" (");
        out.push_str(&reason_label(&entry.reason));
        out.push_str(")\n");
    }

    out.push_str(COVERAGE_ASSUMPTIONS_HEADER);
    if report.assumptions().is_empty() {
        out.push_str(COVERAGE_NO_ASSUMPTIONS);
    } else {
        for assumption in report.assumptions() {
            out.push_str("- ");
            out.push_str(assumption_status_label(assumption.status));
            out.push(' ');
            out.push_str(&assumption.name);
            out.push_str(": ");
            out.push_str(&assumption.summary);
            out.push('\n');
        }
    }
    out
}

fn aspect_label(aspect: HirAspectKind) -> &'static str {
    match aspect {
        HirAspectKind::DiscerneOmnia => "discerne_omnia",
        HirAspectKind::UnrollEquivalence => "unroll_equivalence",
        HirAspectKind::MaskNeutrality => "mask_neutrality",
        HirAspectKind::GatePurity => "gate_purity",
    }
}

fn status_label(status: HirAspectCoverageStatus) -> &'static str {
    match status {
        HirAspectCoverageStatus::Proven => "proven",
        HirAspectCoverageStatus::Failed => "failed",
        HirAspectCoverageStatus::Skipped => "skipped",
        HirAspectCoverageStatus::Abstracted => "abstracted",
    }
}

fn assumption_status_label(status: HirAspectAssumptionStatus) -> &'static str {
    match status {
        HirAspectAssumptionStatus::Checked => "checked",
        HirAspectAssumptionStatus::Trusted => "trusted",
    }
}

fn reason_label(reason: &HirAspectCoverageReason) -> String {
    match reason {
        HirAspectCoverageReason::CheckerPassed => "checker passed".to_owned(),
        HirAspectCoverageReason::CheckerFailed { summary, counterexample } => {
            if counterexample.is_empty() {
                format!("checker failed: {summary}")
            } else {
                format!("checker failed: {summary}; counterexample: {}", counterexample.join(" | "))
            }
        }
        HirAspectCoverageReason::CheckerUnavailable(summary) => format!("checker unavailable: {summary}"),
        HirAspectCoverageReason::CheckerError(summary) => format!("checker error: {summary}"),
        HirAspectCoverageReason::CheckerTimeout(summary) => format!("checker timed out: {summary}"),
        HirAspectCoverageReason::MissingCheckerResult { id } => {
            format!("missing checker result for obligation {id:?}")
        }
        HirAspectCoverageReason::AspectNotProven(reason) => format!("not proven: {reason:?}"),
        HirAspectCoverageReason::SkippedCandidate(reason) => format!("skipped candidate: {reason:?}"),
    }
}

fn span_label(span: HirAspectSpan) -> String {
    match span {
        HirAspectSpan::Source(span) => format!("span={}..{}", span.start, span.end),
        HirAspectSpan::NoSpan(reason) => format!("span=none:{reason:?}"),
    }
}

fn node_label(node_id: Option<HirId>) -> String {
    node_id.map_or_else(|| "hir=none".to_owned(), |id| format!("hir={}", id.0))
}
