//! Conversio (`↦`) coverage matrix harness.
//!
//! The conversio analog of [`hir_target_matrix`] and [`mir_target_matrix`]. The
//! universe is the type-family cartesian product (not the exempla corpus), so
//! this harness is a thin emitter: it iterates every family pair × target and
//! prints the [`classify_conversio_coverage`](radix::codegen::conversio_coverage)
//! verdict as TSV. `scripta/generate-conversio-matrix.py` renders that TSV into
//! `CONVERSIO_MATRIX.md`.
//!
//! Same honesty scope as the HIR/MIR target matrices: the classifier predicts
//! dispatch tiers without emitting code. Drift risk mirrors
//! `classify_hir_coverage`; non-regression floors land once the first green
//! baseline is committed (goal "Later" phase).

use radix::codegen::conversio_coverage::{
    classify_conversio_coverage, ConversioCoverageTarget, ConversioTypeFamily,
};
use radix::driver::{analyze_source, Config, Session};
use radix::mir::{
    classify_mir_coverage, device_roles_from_hir, lower_analyzed_unit_with_context,
    MirCoverageTarget, MirDeviceContext,
};
use std::path::PathBuf;

/// Resolve the conversio-matrix fixture root (`examples/conversio-matrix/`).
fn fixture_root() -> PathBuf {
    // corpus_dir() = examples/corpus; the matrix fixtures are a sibling tree.
    crate::paths::corpus_dir()
        .parent()
        .map(|examples| examples.join("conversio-matrix"))
        .unwrap_or_else(|| PathBuf::from("conversio-matrix"))
}

/// One measured frontend outcome for a fixture.
struct Outcome {
    path: PathBuf,
    accepted: bool,
    issues: Vec<String>,
}

fn measure_fixture(session: &Session, path: &std::path::Path) -> Outcome {
    let source = std::fs::read_to_string(path).unwrap_or_else(|e| format!("# read error: {e}"));
    let name = path.display().to_string();
    let issues = match analyze_source(session, &name, &source) {
        Ok(unit) => unit
            .diagnostics
            .iter()
            .filter(|d| d.is_error())
            .filter_map(|d| d.issue().map(|s| s.to_owned()))
            .collect::<Vec<_>>(),
        Err(diagnostics) => diagnostics
            .iter()
            .filter(|d| d.is_error())
            .filter_map(|d| d.issue().map(|s| s.to_owned()))
            .collect::<Vec<_>>(),
    };
    Outcome {
        path: path.to_path_buf(),
        accepted: issues.is_empty(),
        issues,
    }
}

/// Evaluation experiment: run the real frontend over the sample fixtures,
/// then lower accepted fixtures to MIR and run the real `wasm-text` emitter
/// to measure the MIR `✓`/`—` tier from real artifacts.
///
/// Proves the full measurement loop (✕ via frontend, ✓/— via real emit) before
/// scaling to the full family cartesian product.
///
/// ```text
/// cargo test -p exempla --lib conversio_matrix_eval_experiment -- --ignored --nocapture
/// ```
#[test]
#[ignore = "experiment: run: cargo test -p exempla --lib conversio_matrix_eval_experiment -- --ignored --nocapture"]
fn conversio_matrix_eval_experiment() {
    let session = Session::new(Config::default());
    let root = fixture_root();
    let samples = [
        "numerus/fractus.fab",
        "fractus/numerus.fab",
        "textus/ascii.fab",
        "numerus/octeti.fab",
    ];
    println!(
        "conversio-matrix evaluation experiment (root: {})",
        root.display()
    );
    for rel in samples {
        let path = root.join(rel);
        let outcome = measure_fixture(&session, &path);
        if !outcome.accepted {
            println!("  {:<24} ✕ REJECTED   issues={:?}", rel, outcome.issues);
            continue;
        }
        let mir_tier = measure_wasm_text(&session, &path);
        println!("  {:<24} accepted    wasm-text={}", rel, mir_tier);
    }
    println!("done");
}

/// Lower an accepted fixture to MIR and run the real wasm-text probe.
/// Returns the measured MIR tier: `✓` (probe emitted) or `—` (probe errored).
fn measure_wasm_text(session: &Session, path: &std::path::Path) -> &'static str {
    let source = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(_) => return "— (read failed)",
    };
    let name = path.display().to_string();
    let mut analysis = match analyze_source(session, &name, &source) {
        Ok(unit) if !unit.diagnostics.iter().any(|d| d.is_error()) => unit,
        _ => return "— (frontend rejected)",
    };
    let device_roles = device_roles_from_hir(&analysis.hir);
    let lowered = match lower_analyzed_unit_with_context(&mut analysis) {
        Ok(l) => l,
        Err(_) => return "— (mir lowering failed)",
    };
    let mut device = MirDeviceContext::from_hir_roles(device_roles);
    device.attach_program(&lowered.program);
    let verdict = classify_mir_coverage(
        MirCoverageTarget::WasmText,
        &lowered.program,
        &lowered.validation,
        &device,
        &lowered.interner,
    );
    if verdict.is_capable() {
        "✓"
    } else {
        "—"
    }
}

/// Emit machine-readable rows for the renderer (`ROWS` section).
///
/// One row per (src, tgt, target): `src<TAB>tgt<TAB>target<TAB>verdict`.
///
/// ```text
/// cargo test -p exempla --lib emit_conversio_target_matrix -- --ignored --nocapture
/// ```
#[test]
#[ignore = "maintenance conversio matrix emit; run: cargo test -p exempla --lib emit_conversio_target_matrix -- --ignored --nocapture"]
fn emit_conversio_target_matrix() {
    // Leading newline keeps the ROWS marker on its own line when libtest has
    // already printed `test … ` without a trailing newline under --nocapture.
    print!("\n");
    println!("ROWS");
    for &src in ConversioTypeFamily::ALL {
        for &tgt in ConversioTypeFamily::ALL {
            for &target in ConversioCoverageTarget::ALL {
                let verdict = classify_conversio_coverage(src, tgt, target);
                println!(
                    "{}\t{}\t{}\t{}",
                    src.name(),
                    tgt.name(),
                    target.name(),
                    verdict.name()
                );
            }
        }
    }
}
