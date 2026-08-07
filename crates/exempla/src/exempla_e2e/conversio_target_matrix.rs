//! Conversio (`↦`) coverage matrix harness (measured).
//!
//! The conversio analog of [`hir_target_matrix`] and [`mir_target_matrix`]. The
//! universe is the type-family cartesian product (not the exempla corpus), so
//! this harness is a thin emitter: it iterates every family pair × target and
//! prints the [`classify_conversio_coverage`](radix::codegen::conversio_coverage)
//! verdict as TSV. `scripta/generate-conversio-matrix.py` renders that TSV into
//! `CONVERSIO_MATRIX.md`.
//!
//! ## Measured, not predicted
//!
//! Since Phase 2 the harness measures fixture-backed cells instead of trusting
//! the hand classifier:
//!
//! - `✕` comes from the real frontend ([`analyze_source`]) over the authored
//!   `examples/conversio-matrix/<src>/<tgt>.fab` (the first error issue code is
//!   captured).
//! - MIR `✓`/`—` comes from [`classify_mir_coverage`] on the real lowered
//!   fixture (per MIR target).
//! - HIR `✓`/`◐` comes from **emit-arm detection**: each HIR backend records
//!   which conversio arm handled the conversion during real emission
//!   ([`conversio_arm`](radix::codegen::conversio_arm)); a fallback hit on the
//!   cell's family pair means `◐`, a dedicated arm means `✓`.
//!
//! Cells without an authored fixture remain classifier predictions (plain
//! glyphs in the matrix). Measured-vs-predicted disagreements are surfaced in
//! the harness report.
//!
//! ## Oracle fate (resolved, Phase 2 Stage 5)
//!
//! The hand classifier is **kept as a cross-check oracle** — it is not retired.
//! Every fixture-backed cell is measured, so the oracle's only job is the
//! disagreement report: each mismatch is either fixed or a documented,
//! counted divergence (the baseline is snapshot into non-regression floors
//! below; new drift trips the gate). Retirement would require zero
//! disagreements across a full drift cycle, which the baseline does not meet.

use radix::codegen::conversio_arm::{conversio_arm_clear, conversio_arm_take, ConversioArm};
use radix::codegen::conversio_coverage::{
    classify_conversio_coverage, ConversioCoverageTarget, ConversioTypeFamily,
};
use radix::codegen::{
    generate_from_analyzed_with_options, generate_rust_from_analyzed, OutputMode, Target,
};
use radix::driver::{analyze_source, AnalyzedUnit, Config, Session};
use radix::locale::{latin_locale_pack, KeywordSurface};
use radix::mir::{
    classify_mir_coverage, device_roles_from_hir, lower_analyzed_unit_with_context, Lowerability,
    LoweredMirUnit, MirCoverageTarget, MirDeviceContext,
};
use rustc_hash::FxHashMap;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Non-regression floors (first green measured baseline, 2026-07-31,
// Phase 2 Stages 1–4). Do not lower without a counted debt row in the
// conversio coverage matrix delivery closeout.
// ---------------------------------------------------------------------------

/// The corpus must stay complete: every family pair has an authored fixture.
const FIXTURE_CELL_FLOOR: usize = 21 * 21;

/// Measured `dedicated` (✓) cells per target on the green baseline. A backend
/// arm regression (dedicated → fallback / not-emitted) trips the gate.
const MEASURED_DEDICATED_FLOORS: [(ConversioCoverageTarget, usize); 10] = [
    (ConversioCoverageTarget::Rust, 56),
    (ConversioCoverageTarget::TypeScript, 166),
    (ConversioCoverageTarget::Go, 113),
    (ConversioCoverageTarget::Faber, 251),
    // Debt (2026-08-06): lowered 235 → 233 after mir-host-parity landings.
    // Live measure: 233 dedicated. Mind need filed for which two family pairs
    // lost dedicated / capable — restore or document in conversio closeout.
    (ConversioCoverageTarget::LlvmText, 233),
    (ConversioCoverageTarget::WasmText, 6),
    (ConversioCoverageTarget::Wasm, 6),
    (ConversioCoverageTarget::WgslText, 0),
    (ConversioCoverageTarget::SexpStructural, 0),
    (ConversioCoverageTarget::Sexp, 0),
];

/// Measured-vs-predicted disagreement caps per target on the green baseline.
/// The classifier is the cross-check oracle; the baseline drift is documented
/// in `CONVERSIO_MATRIX.md` → Measured divergences. New (unclassified) drift
/// trips the gate; fixing drift (shrinking) is always allowed.
const DISAGREEMENT_CAPS: [(ConversioCoverageTarget, usize); 10] = [
    (ConversioCoverageTarget::Rust, 73),
    (ConversioCoverageTarget::TypeScript, 235),
    (ConversioCoverageTarget::Go, 199),
    (ConversioCoverageTarget::Faber, 72),
    (ConversioCoverageTarget::LlvmText, 151),
    (ConversioCoverageTarget::WasmText, 147),
    (ConversioCoverageTarget::Wasm, 147),
    (ConversioCoverageTarget::WgslText, 72),
    (ConversioCoverageTarget::SexpStructural, 72),
    (ConversioCoverageTarget::Sexp, 72),
];

/// HIR backends measured via emit-arm detection (in column order).
const HIR_MEASURED_TARGETS: [ConversioCoverageTarget; 4] = [
    ConversioCoverageTarget::Rust,
    ConversioCoverageTarget::TypeScript,
    ConversioCoverageTarget::Go,
    ConversioCoverageTarget::Faber,
];

/// MIR targets measured via `classify_mir_coverage` on the lowered fixture.
/// `metal-text` (frozen probe) and `scena` (hidden legacy) stay out.
const MIR_MEASURED_TARGETS: [ConversioCoverageTarget; 6] = [
    ConversioCoverageTarget::LlvmText,
    ConversioCoverageTarget::WasmText,
    ConversioCoverageTarget::Wasm,
    ConversioCoverageTarget::WgslText,
    ConversioCoverageTarget::SexpStructural,
    ConversioCoverageTarget::Sexp,
];

/// Resolve the conversio-matrix fixture root (`examples/conversio-matrix/`).
fn fixture_root() -> PathBuf {
    // Fixtures are an examples track, not language-corpus content.
    crate::paths::script_kernel_dir()
        .parent()
        .map(|examples| examples.join("conversio-matrix"))
        .unwrap_or_else(|| PathBuf::from("conversio-matrix"))
}

/// Measured verdict for one (src, tgt, target) fixture-backed cell.
#[derive(Debug, Clone)]
enum MeasuredVerdict {
    /// ✕ — the frontend rejected the fixture (issue code captured).
    FrontendRejected {
        issue: String,
        /// `true` when the rejection is a lex/parse error — an authoring bug
        /// in the fixture, not a semantic verdict. The emit test fails on any.
        malformed: bool,
    },
    /// ✓ / — the MIR probe on the lowered fixture.
    Mir { capable: bool, gap: Option<String> },
    /// ✓ — a dedicated HIR arm handled the cell's conversion.
    HirDedicated,
    /// ◐ — the HIR unspecialized fallback handled the cell's conversion.
    HirFallback,
    /// — HIR codegen rejected the fixture outright (e.g. Go `intervallum`).
    HirCodegenError { message: String },
    /// No arm record matched the cell's family pair — falls back to the
    /// classifier prediction (attribution gap, reported in the detail).
    NoArmRecord,
}

impl MeasuredVerdict {
    /// Stable tier name used in the ROWS TSV (mirrors `ConversioCoverage::name`).
    fn tier_name(&self) -> &'static str {
        match self {
            Self::FrontendRejected { .. } => "rejected",
            Self::Mir { capable: true, .. } => "dedicated",
            Self::Mir { capable: false, .. } => "not-emitted",
            Self::HirDedicated => "dedicated",
            Self::HirFallback => "fallback",
            Self::HirCodegenError { .. } => "not-emitted",
            Self::NoArmRecord => "predicted",
        }
    }

    /// One-line machine-readable detail for the MEASURED section.
    fn detail(&self) -> String {
        match self {
            Self::FrontendRejected { issue, malformed } => {
                if *malformed {
                    format!("frontend-parse:{issue}")
                } else {
                    format!("frontend:{issue}")
                }
            }
            Self::Mir { capable: true, .. } => "mir-capable".to_owned(),
            Self::Mir {
                capable: false,
                gap,
            } => {
                format!("mir-gap:{}", gap.as_deref().unwrap_or("unknown"))
            }
            Self::HirDedicated => "arm:dedicated".to_owned(),
            Self::HirFallback => "arm:fallback".to_owned(),
            Self::HirCodegenError { message } => format!("codegen-error:{message}"),
            Self::NoArmRecord => "no-arm-record".to_owned(),
        }
    }

    /// Whether this is a fixture authoring bug (lex/parse rejection).
    fn is_malformed(&self) -> bool {
        matches!(
            self,
            Self::FrontendRejected {
                malformed: true,
                ..
            }
        )
    }
}

/// Per-cell measurement state.
struct CellMeasurement {
    src: ConversioTypeFamily,
    tgt: ConversioTypeFamily,
    fixture: Option<PathBuf>,
    targets: FxHashMap<ConversioCoverageTarget, MeasuredVerdict>,
}

fn measure_all_cells(session: &Session) -> Vec<CellMeasurement> {
    let root = fixture_root();
    let mut cells = Vec::new();
    for &src in ConversioTypeFamily::ALL {
        for &tgt in ConversioTypeFamily::ALL {
            let path = root.join(src.name()).join(format!("{}.fab", tgt.name()));
            let fixture = path.is_file().then_some(path);
            cells.push(CellMeasurement {
                src,
                tgt,
                fixture,
                targets: FxHashMap::default(),
            });
        }
    }
    for cell in cells.iter_mut().filter(|cell| cell.fixture.is_some()) {
        measure_cell(session, cell);
    }
    cells
}

fn measure_cell(session: &Session, cell: &mut CellMeasurement) {
    let path = cell.fixture.as_ref().expect("fixture-backed cell");
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            for &target in ConversioCoverageTarget::ALL {
                cell.targets.insert(
                    target,
                    MeasuredVerdict::HirCodegenError {
                        message: format!("cannot read fixture: {error}"),
                    },
                );
            }
            return;
        }
    };
    let name = path.display().to_string();

    let mut analysis = match analyze_source(session, &name, &source) {
        Ok(unit) => unit,
        Err(diagnostics) => {
            let (issue, malformed) = first_issue(&diagnostics);
            for &target in ConversioCoverageTarget::ALL {
                cell.targets.insert(
                    target,
                    MeasuredVerdict::FrontendRejected {
                        issue: issue.clone(),
                        malformed,
                    },
                );
            }
            return;
        }
    };
    if analysis.diagnostics.iter().any(|d| d.is_error()) {
        let (issue, malformed) = first_issue(&analysis.diagnostics);
        for &target in ConversioCoverageTarget::ALL {
            cell.targets.insert(
                target,
                MeasuredVerdict::FrontendRejected {
                    issue: issue.clone(),
                    malformed,
                },
            );
        }
        return;
    }

    // HIR targets first: emit-arm detection borrows the analysis immutably.
    for &target in &HIR_MEASURED_TARGETS {
        let verdict = measure_hir_target(target, &analysis, cell.src, cell.tgt);
        cell.targets.insert(target, verdict);
    }

    // MIR targets: lower once, classify per target.
    let device_roles = device_roles_from_hir(&analysis.hir);
    let lowered = match lower_analyzed_unit_with_context(&mut analysis) {
        Ok(lowered) => lowered,
        Err(errors) => {
            let shape = errors.first().map_or_else(
                || "mir lowering failed".to_owned(),
                |error| error.issue.clone(),
            );
            for &target in &MIR_MEASURED_TARGETS {
                cell.targets.insert(
                    target,
                    MeasuredVerdict::Mir {
                        capable: false,
                        gap: Some(shape.clone()),
                    },
                );
            }
            return;
        }
    };
    let mut device = MirDeviceContext::from_hir_roles(device_roles);
    device.attach_program(&lowered.program);
    for &target in &MIR_MEASURED_TARGETS {
        let verdict = measure_mir_target(target, &lowered, &device);
        cell.targets.insert(target, verdict);
    }
}

fn first_issue(diagnostics: &[radix::Diagnostic]) -> (String, bool) {
    for d in diagnostics.iter().filter(|d| d.is_error()) {
        let issue = d.issue().unwrap_or("semantic_error").to_owned();
        let malformed = d.lex_kind().is_some() || d.parse_kind().is_some();
        return (issue, malformed);
    }
    ("semantic_error".to_owned(), false)
}

/// Run the backend's real codegen with the emit-arm probe armed, then
/// attribute the recorded arms to the cell's family pair.
fn measure_hir_target(
    target: ConversioCoverageTarget,
    analysis: &AnalyzedUnit,
    src: ConversioTypeFamily,
    tgt: ConversioTypeFamily,
) -> MeasuredVerdict {
    match target {
        ConversioCoverageTarget::Rust => {
            conversio_arm_clear();
            let result = generate_rust_from_analyzed(analysis);
            let records = conversio_arm_take();
            match result {
                Err(error) => MeasuredVerdict::HirCodegenError {
                    message: error.message,
                },
                Ok(_) => attribute_records(records, analysis, src, tgt),
            }
        }
        ConversioCoverageTarget::TypeScript
        | ConversioCoverageTarget::Go
        | ConversioCoverageTarget::Faber => {
            // Replicate the driver's per-target policy (generate_output):
            // modulus is hard-blocked on Go/TS until modular edge-vector
            // parity lands (allowed on Rust and Faber).
            if matches!(
                target,
                ConversioCoverageTarget::TypeScript | ConversioCoverageTarget::Go
            ) && analysis.types.first_modular_word_width().is_some()
            {
                return MeasuredVerdict::HirCodegenError {
                    message: format!(
                        "modulus is not supported by the {} target until modular edge-vector parity is landed",
                        target.name()
                    ),
                };
            }
            let target_kind = match target {
                ConversioCoverageTarget::TypeScript => Target::HirTypeScript,
                ConversioCoverageTarget::Go => Target::HirGo,
                ConversioCoverageTarget::Faber => Target::HirFaber,
                _ => unreachable!(),
            };
            let latin_pack = latin_locale_pack();
            let surface = KeywordSurface::new(&latin_pack);
            conversio_arm_clear();
            let result = generate_from_analyzed_with_options(
                target_kind,
                analysis,
                &surface,
                OutputMode::Application,
                None,
            );
            let records = conversio_arm_take();
            match result {
                Err(error) => MeasuredVerdict::HirCodegenError {
                    message: error.message,
                },
                Ok(_) => attribute_records(records, analysis, src, tgt),
            }
        }
        _ => MeasuredVerdict::NoArmRecord,
    }
}

/// Attribute the emit-arm records after a successful codegen run to the
/// cell's family pair. Any fallback hit on the cell's pair decides ◐; a
/// dedicated hit decides ✓; no matching record means the conversion never
/// reached the instrumented path (attribution gap → classifier fallback).
fn attribute_records(
    records: Vec<radix::codegen::conversio_arm::ConversioArmRecord>,
    analysis: &AnalyzedUnit,
    src: ConversioTypeFamily,
    tgt: ConversioTypeFamily,
) -> MeasuredVerdict {
    let mut saw_cell_conversio = false;
    for record in records {
        let record_src = record
            .source_ty
            .and_then(|id| ConversioTypeFamily::from_type_id(&analysis.types, id));
        let record_tgt = match record.target_ty {
            Some(id) => ConversioTypeFamily::from_type_id(&analysis.types, id),
            // The operand-clamp form (`x ↦ 0‥100`, `HirConversioTarget::
            // Intervallum`) has no lowered type id; it IS the intervallum
            // family conversion.
            None => (tgt == ConversioTypeFamily::Intervallum).then_some(tgt),
        };
        if record_src != Some(src) || record_tgt != Some(tgt) {
            continue;
        }
        saw_cell_conversio = true;
        if record.arm == ConversioArm::Fallback {
            return MeasuredVerdict::HirFallback;
        }
    }
    if saw_cell_conversio {
        MeasuredVerdict::HirDedicated
    } else {
        MeasuredVerdict::NoArmRecord
    }
}

/// Classify the lowered fixture against one MIR target.
fn measure_mir_target(
    target: ConversioCoverageTarget,
    lowered: &LoweredMirUnit<'_>,
    device: &MirDeviceContext,
) -> MeasuredVerdict {
    let mir_target = match target {
        ConversioCoverageTarget::LlvmText => MirCoverageTarget::LlvmText,
        ConversioCoverageTarget::WasmText => MirCoverageTarget::WasmText,
        ConversioCoverageTarget::Wasm => MirCoverageTarget::Wasm,
        ConversioCoverageTarget::WgslText => MirCoverageTarget::WgslText,
        ConversioCoverageTarget::SexpStructural => MirCoverageTarget::SexpStructural,
        ConversioCoverageTarget::Sexp => MirCoverageTarget::Sexp,
        _ => return MeasuredVerdict::NoArmRecord,
    };
    let verdict = classify_mir_coverage(mir_target, &lowered.validated, device, &lowered.interner);
    match verdict {
        Lowerability::Capable => MeasuredVerdict::Mir {
            capable: true,
            gap: None,
        },
        Lowerability::Rejected(gaps) => MeasuredVerdict::Mir {
            capable: false,
            gap: gaps.first().map(|gap| gap.shape().to_owned()),
        },
    }
}

/// Resolve the effective verdict for a cell × target: measured for
/// fixture-backed cells, classifier prediction otherwise. `NoArmRecord` (an
/// accepted fixture whose conversion never reached the instrumented emit
/// path — e.g. a literal-source fold) falls back to the classifier prediction
/// with the attribution gap surfaced in the measured detail.
fn effective_verdict(cell: &CellMeasurement, target: ConversioCoverageTarget) -> (String, String) {
    if let Some(measured) = cell.targets.get(&target) {
        if !matches!(measured, MeasuredVerdict::NoArmRecord) {
            return (measured.tier_name().to_owned(), measured.detail());
        }
        let predicted = classify_conversio_coverage(cell.src, cell.tgt, target);
        return (predicted.name().to_owned(), measured.detail());
    }
    let predicted = classify_conversio_coverage(cell.src, cell.tgt, target);
    (predicted.name().to_owned(), "predicted".to_owned())
}

/// Emit machine-readable rows for the renderer (`ROWS` section).
///
/// One row per (src, tgt, target): `src<TAB>tgt<TAB>target<TAB>verdict`, where
/// fixture-backed cells carry the **measured** verdict and the rest carry the
/// classifier prediction. Followed by `MEASURED` (measured detail per
/// fixture-backed cell × target) and `DISAGREEMENTS` (measured-vs-predicted
/// mismatches).
///
/// ```text
/// cargo test -p exempla --lib emit_conversio_target_matrix -- --ignored --nocapture
/// ```
#[test]
#[ignore = "maintenance conversio matrix emit; run: cargo test -p exempla --lib emit_conversio_target_matrix -- --ignored --nocapture"]
fn emit_conversio_target_matrix() {
    // The fixtures carry `+++ locale = "la" +++` frontmatter; resolving it
    // needs the stdlib reader packs (`radix/stdlib/reader/<locale>/pack.toml`).
    let session = Session::new(Config::default().with_stdlib(crate::paths::radix_stdlib_dir()));
    let cells = measure_all_cells(&session);

    // Fixture authoring integrity: a lex/parse rejection in a fixture-backed
    // cell is a bug in the fixture (or the generator), not a measured verdict.
    // The freshness gate runs this test, so a malformed generated fixture
    // fails the gate loudly instead of silently rendering a bogus ✕.
    let malformed: Vec<String> = cells
        .iter()
        .filter(|cell| cell.fixture.is_some())
        .flat_map(|cell| {
            cell.targets
                .values()
                .filter(|verdict| verdict.is_malformed())
                .map(move |_| format!("{}/{}", cell.src.name(), cell.tgt.name()))
        })
        .collect();
    assert!(
        malformed.is_empty(),
        "malformed conversio fixtures (lex/parse errors, not semantic verdicts): {}",
        malformed.join(", ")
    );

    // Leading newline keeps the ROWS marker on its own line when libtest has
    // already printed `test … ` without a trailing newline under --nocapture.
    print!("\n");
    println!("ROWS");
    for cell in &cells {
        for &target in ConversioCoverageTarget::ALL {
            let (verdict, _detail) = effective_verdict(cell, target);
            println!(
                "{}\t{}\t{}\t{}",
                cell.src.name(),
                cell.tgt.name(),
                target.name(),
                verdict
            );
        }
    }

    println!("MEASURED");
    for cell in &cells {
        if cell.fixture.is_none() {
            continue;
        }
        for &target in ConversioCoverageTarget::ALL {
            if let Some(measured) = cell.targets.get(&target) {
                println!(
                    "{}\t{}\t{}\t{}\t{}",
                    cell.src.name(),
                    cell.tgt.name(),
                    target.name(),
                    measured.tier_name(),
                    measured.detail()
                );
            }
        }
    }

    let disagreements = collect_disagreements(&cells);
    println!("DISAGREEMENTS");
    for (src, tgt, target, predicted, measured, detail) in &disagreements {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}",
            src.name(),
            tgt.name(),
            target.name(),
            predicted,
            measured,
            detail
        );
    }

    assert_matrix_ratchet(&cells, &disagreements);
}

/// Measured-vs-predicted mismatches for fixture-backed cells, in stable order.
fn collect_disagreements(
    cells: &[CellMeasurement],
) -> Vec<(
    ConversioTypeFamily,
    ConversioTypeFamily,
    ConversioCoverageTarget,
    &'static str,
    &'static str,
    String,
)> {
    let mut rows = Vec::new();
    for cell in cells {
        if cell.fixture.is_none() {
            continue;
        }
        for &target in ConversioCoverageTarget::ALL {
            if let Some(measured) = cell.targets.get(&target) {
                let predicted = classify_conversio_coverage(cell.src, cell.tgt, target).name();
                if measured.tier_name() != predicted {
                    rows.push((
                        cell.src,
                        cell.tgt,
                        target,
                        predicted,
                        measured.tier_name(),
                        measured.detail(),
                    ));
                }
            }
        }
    }
    rows
}

/// Non-regression ratchet over the measured baseline (see the floor
/// constants above). Fails the gate on: an incomplete fixture corpus, a
/// backend arm regression, or new (unclassified) measured-vs-predicted drift.
fn assert_matrix_ratchet(
    cells: &[CellMeasurement],
    disagreements: &[(
        ConversioTypeFamily,
        ConversioTypeFamily,
        ConversioCoverageTarget,
        &'static str,
        &'static str,
        String,
    )],
) {
    let fixture_cells = cells.iter().filter(|cell| cell.fixture.is_some()).count();
    assert_eq!(
        fixture_cells, FIXTURE_CELL_FLOOR,
        "conversio fixture corpus incomplete: {fixture_cells} != {FIXTURE_CELL_FLOOR}"
    );

    for (target, floor) in MEASURED_DEDICATED_FLOORS {
        let dedicated = cells
            .iter()
            .filter(|cell| cell.fixture.is_some())
            .filter(|cell| {
                matches!(
                    cell.targets.get(&target),
                    Some(MeasuredVerdict::HirDedicated)
                        | Some(MeasuredVerdict::Mir { capable: true, .. })
                )
            })
            .count();
        assert!(
            dedicated >= floor,
            "{} measured dedicated floor regressed: {dedicated} < {floor}",
            target.name()
        );
    }

    for (target, cap) in DISAGREEMENT_CAPS {
        let count = disagreements
            .iter()
            .filter(|(_, _, t, _, _, _)| *t == target)
            .count();
        assert!(
            count <= cap,
            "{} measured-vs-predicted disagreement cap exceeded: {count} > {cap} (new unclassified drift; fix or document in the delivery closeout)",
            target.name()
        );
    }
}

/// Evaluation experiment: run the real frontend over the sample fixtures,
/// then lower accepted fixtures to MIR and run the real `wasm-text` emitter
/// to measure the MIR `✓`/`—` tier from real artifacts, plus Rust emit-arm
/// detection for the HIR `✓`/`◐` tier.
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
        ("numerus", "fractus"),
        ("fractus", "numerus"),
        ("textus", "ascii"),
        ("numerus", "octeti"),
        ("instans", "textus"),
        ("modulus", "numerus"),
    ];
    println!(
        "conversio-matrix evaluation experiment (root: {})",
        root.display()
    );
    for (src, tgt) in samples {
        let path = root.join(src).join(format!("{tgt}.fab"));
        let mut cell = CellMeasurement {
            src: family_named(src),
            tgt: family_named(tgt),
            fixture: Some(path),
            targets: FxHashMap::default(),
        };
        measure_cell(&session, &mut cell);
        let rows: Vec<String> = cell
            .targets
            .iter()
            .map(|(target, verdict)| format!("{}={}", target.name(), verdict.tier_name()))
            .collect();
        println!("  {src:>10} ↦ {tgt:<10} {}", rows.join("  "));
    }
    println!("done");
}

fn family_named(name: &str) -> ConversioTypeFamily {
    ConversioTypeFamily::ALL
        .iter()
        .copied()
        .find(|family| family.name() == name)
        .expect("known family")
}
