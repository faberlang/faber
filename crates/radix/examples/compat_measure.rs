//! Measure the public columns of the grammar × target compatibility matrices.
//!
//! ```text
//! cargo run -q -p radix --example compat_measure -- faber runner
//! ```
//!
//! Each argument names one column: `faber` (the HIR application lane, round-trip
//! health of the Faber source emitter) or `runner` (the MIR lane, the in-process
//! runner's run tier). Every `.fab` file under `corpus/` is measured, and one
//! tab-separated line per file and column goes to stdout:
//!
//! ```text
//! <path relative to corpus/>  <column>  <verdict>
//! ```
//!
//! The verdict is `capable`, `gap:<why>`, or not analyzable (`no-hir` for the
//! `faber` lane, `n/a` for the `runner` lane): the exemplum does not get as far
//! as the lane's classifier, so it counts toward neither side of the matrix.
//! `scripta/emit-compat-json.py` turns the lines into
//! `corpus/measurement/compat/<column>.json`.
//!
//! A runner classification that crashes is not a verdict: the measurement stops
//! with an error instead of recording the crash as a gap.

use radix::module::codegen::coverage::{HirCoverageTarget, classify_hir_coverage};
use radix::module::driver::{AnalyzeRequest, Config, Session, analyze_source};
use radix::module::mir::{CapabilityGap, Lowerability, classify_run_tier, lower_analyzed_unit};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const COLUMNS: [&str; 2] = ["faber", "runner"];

fn main() -> ExitCode {
    let columns: Vec<String> = std::env::args().skip(1).collect();
    if columns.is_empty()
        || columns
            .iter()
            .any(|column| !COLUMNS.contains(&column.as_str()))
    {
        eprintln!(
            "usage: compat_measure <column>...   (columns: {})",
            COLUMNS.join(", ")
        );
        return ExitCode::from(2);
    }

    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../corpus");
    let corpus = corpus.canonicalize().unwrap_or(corpus);
    let mut files = Vec::new();
    collect_exempla(&corpus, &mut files);
    files.sort();

    let session = Session::new(Config::default());
    for file in &files {
        let key = file
            .strip_prefix(&corpus)
            .unwrap_or(file)
            .display()
            .to_string()
            .replace('\\', "/");
        for column in &columns {
            let cell = match column.as_str() {
                "faber" => faber_cell(&session, file),
                _ => match runner_cell(&session, file) {
                    Ok(cell) => cell,
                    Err(crash) => {
                        eprintln!("compat_measure: {key} [runner]: {crash}");
                        return ExitCode::FAILURE;
                    }
                },
            };
            println!("{key}\t{column}\t{cell}");
        }
    }
    ExitCode::SUCCESS
}

fn collect_exempla(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_exempla(&path, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("fab") {
            out.push(path);
        }
    }
}

/// HIR lane: one frontend pass, then the Faber source emitter's classifier.
fn faber_cell(session: &Session, file: &Path) -> String {
    let Ok(source) = std::fs::read_to_string(file) else {
        return "no-hir".to_owned();
    };
    let name = file.display().to_string();
    match analyze_source(session, &name, &source, AnalyzeRequest::default()) {
        Ok(unit)
            if !unit
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.is_error()) =>
        {
            match classify_hir_coverage(HirCoverageTarget::Faber, &unit.hir, &unit.types) {
                Lowerability::Capable => "capable".to_owned(),
                Lowerability::Rejected(gaps) => {
                    let slugs: Vec<_> = gaps.iter().map(CapabilityGap::slug).collect();
                    format!("gap:{}", slugs.join(","))
                }
            }
        }
        _ => "no-hir".to_owned(),
    }
}

/// MIR lane: analyze and lower the program, then run its entry in the runner.
fn runner_cell(session: &Session, file: &Path) -> Result<String, String> {
    let Ok(source) = std::fs::read_to_string(file) else {
        return Ok("n/a".to_owned());
    };
    let Ok(analysis) = radix::program::analyze_prepared_program_path(session, file, &source) else {
        return Ok("n/a".to_owned());
    };
    let Ok(lowered) = lower_analyzed_unit(analysis.application_view(), Default::default()) else {
        return Ok("n/a".to_owned());
    };
    match classify_run_tier(&lowered.validated) {
        Lowerability::Capable => Ok("capable".to_owned()),
        Lowerability::Rejected(gaps) => {
            if let Some(CapabilityGap::RuntimePanic { shape, .. }) = gaps
                .iter()
                .find(|gap| matches!(gap, CapabilityGap::RuntimePanic { .. }))
            {
                return Err(format!(
                    "classification crashed, not a capability gap: {shape}"
                ));
            }
            let shape = gaps.first().map_or("rejected", CapabilityGap::shape);
            Ok(format!("gap:{}", shape.replace(['\t', '\n'], " ")))
        }
    }
}
