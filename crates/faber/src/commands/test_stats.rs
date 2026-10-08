//! Binding and sinks for `faber test` diagnostic statistics.
//!
//! The runner owns per-case observations; this module owns the test-run schema
//! and reuses the run-stats meter rows. The artifact is diagnostic-only.

use super::run_stats::{
    self, FaberMetadata, FunctionMetadata, IntrinsicMetadata, MeterMetadata, PackageMetadata,
};
use proba::{CaseOutcome, CaseResult, ProbaReport};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::PathBuf;

pub(crate) const STATS_MARKER: &str = "--- faber-test-stats ---";

/// The binding's `format_version: 1` test-run artifact.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TestStatsArtifact {
    pub(crate) kind: String,
    pub(crate) format_version: u32,
    pub(crate) generated_at: String,
    pub(crate) faber: FaberMetadata,
    pub(crate) package: PackageMetadata,
    pub(crate) route: String,
    pub(crate) run: TestRunMetadata,
    pub(crate) meter: MeterMetadata,
    pub(crate) cases: Vec<TestCaseStats>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TestRunMetadata {
    pub(crate) wall_ms: u64,
    pub(crate) exit: i32,
    pub(crate) passed: usize,
    pub(crate) failed: usize,
    pub(crate) blocked: usize,
    pub(crate) skipped: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct TestCaseStats {
    pub(crate) path: String,
    pub(crate) name: String,
    pub(crate) suite: String,
    pub(crate) status: String,
    pub(crate) duration_ms: u128,
    pub(crate) reason: Option<String>,
    pub(crate) functions: Vec<FunctionMetadata>,
    pub(crate) intrinsics: Vec<IntrinsicMetadata>,
}

impl TestStatsArtifact {
    pub(crate) fn new(
        package: PackageMetadata,
        report: &ProbaReport,
        durations_ms: &HashMap<String, u128>,
        wall_ms: u64,
        exit: i32,
        result_key: impl Fn(&CaseResult) -> String,
    ) -> Self {
        let mut cases = report
            .results
            .iter()
            .map(|result| {
                let key = result_key(result);
                TestCaseStats::from_result(result, durations_ms.get(&key).copied().unwrap_or(0))
            })
            .collect::<Vec<_>>();
        cases.sort_by(|left, right| left.path.cmp(&right.path));
        Self {
            kind: "faber-test-stats".to_owned(),
            format_version: run_stats::FORMAT_VERSION,
            generated_at: run_stats::generated_at(),
            faber: FaberMetadata {
                version: env!("CARGO_PKG_VERSION").to_owned(),
                commit: None,
            },
            package,
            route: run_stats::ROUTE.to_owned(),
            run: TestRunMetadata {
                wall_ms,
                exit,
                passed: report.passed(),
                failed: report.failed(),
                blocked: report.blocked(),
                skipped: report.skipped(),
            },
            meter: MeterMetadata {
                clock: run_stats::CLOCK.to_owned(),
                durations: run_stats::DurationMetadata {
                    method: run_stats::DURATION_METHOD.to_owned(),
                    size: run_stats::DURATION_SIZE,
                    exact: false,
                },
                self_time: run_stats::SELF_TIME.to_owned(),
                note: run_stats::NOTE.to_owned(),
            },
            cases,
        }
    }
}

impl TestCaseStats {
    fn from_result(result: &CaseResult, duration_ms: u128) -> Self {
        let (status, reason) = match &result.outcome {
            CaseOutcome::Passed => ("passed", None),
            CaseOutcome::Failed { message } => ("failed", Some(message.clone())),
            CaseOutcome::Blocked { reason } => ("blocked", Some(reason.clone())),
            CaseOutcome::Skipped { reason } => ("skipped", Some(reason.clone())),
        };
        let stats = result.stats.clone().unwrap_or_default();
        let mut functions = stats
            .functions
            .into_iter()
            .map(FunctionMetadata::from)
            .collect::<Vec<_>>();
        functions.sort_by(|left, right| {
            left.module
                .cmp(&right.module)
                .then_with(|| left.name.cmp(&right.name))
        });
        let mut intrinsics = stats
            .intrinsics
            .into_iter()
            .map(IntrinsicMetadata::from)
            .collect::<Vec<_>>();
        intrinsics.sort_by(|left, right| left.name.cmp(&right.name));
        Self {
            path: format!("{}::{}", result.unit_path.display(), result.display_path()),
            name: result.name.clone(),
            suite: result.suite_path.clone(),
            status: status.to_owned(),
            duration_ms,
            reason,
            functions,
            intrinsics,
        }
    }
}

pub(crate) fn emit(
    artifact: &TestStatsArtifact,
    stdout: bool,
    stats_file: Option<&PathBuf>,
) -> io::Result<()> {
    let json = serde_json::to_string(artifact)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    run_stats::emit_framed(STATS_MARKER, &json, stdout, stats_file)
}
