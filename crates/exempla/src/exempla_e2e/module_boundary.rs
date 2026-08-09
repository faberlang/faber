//! Module-boundary full profile: emit-lane parity on generated consumers.
//!
//! Full-profile counterpart of the cheap profile in
//! `radix/crates/radix-module-boundary` (goal
//! `radix/docs/factory/module-boundary-corpus-harness/goal.md`). The cheap
//! profile measures parse/check/MIR/stepper parity in-process; this mode
//! measures the **emit lanes** on the generated consumer: for each corpus
//! file (and the defect-sprint fixture seeds), emit the file standalone and
//! emit a generated consumer that imports it, then assert emission parity or
//! a named ledger row (goal AC5).
//!
//! Wave 1 is the rust lane only (head-cto D4 framing, fire-21/22
//! consolidation — ONE context). The emit lanes follow the existing matrix
//! structure ([`mir_target_matrix`] / [`conversio_target_matrix`]: rows =
//! corpus files, columns = emit lanes); the ts/go/swift and MIR target lanes
//! are wired in the lane table but opt-in behind [`ACTIVE_LANES`].
//!
//! Invocations (both opt-in; nothing here runs in the default suite):
//!
//! ```text
//! # narrow wave-1 proof (Hand closeout):
//! cargo test -p exempla --lib exempla_module_boundary_rust_lane -- --ignored --nocapture
//! # full-corpus walk (auditor-owned at audit boundaries, delivery MB-U4 gates):
//! cargo test -p exempla --lib exempla_module_boundary_e2e -- --ignored --nocapture
//! ```

use radix::codegen::{OutputMode, Target};
use radix::driver::{Config, Session};
use radix::Output;
use radix_module_boundary::engine::generate_consumer;
use radix_module_boundary::ledger::Disposition;
use radix_module_boundary::parity::{measure_standalone, Layer, Mode, ModeMeasurement};
use radix_module_boundary::walk::{
    corpus_entries, corpus_policy_for, policy_for, seed_sources, CorpusClass, SeedSource,
};
use std::path::{Path, PathBuf};

/// Emit lanes in matrix-column order. Wave 1: rust only is ACTIVE; the other
/// lanes are wired (each declares its emit target) but opt-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EmitLane {
    Rust,
    // Experimental HIR lanes — wired, opt-in (wave 2+).
    TypeScript,
    Go,
    Swift,
    // MIR target lanes — wired, opt-in (wave 2+; their emit surface is the
    // MIR emitters, e.g. `radix::mir::emit_wasm_text_and_binary_probe_with_context`).
    MirWasmText,
    MirWasm,
    MirSexp,
    MirLlvmText,
    MirMetalText,
    MirWgslText,
}

impl EmitLane {
    /// Stable lane name used in the ROWS report.
    fn name(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::TypeScript => "ts",
            Self::Go => "go",
            Self::Swift => "swift",
            Self::MirWasmText => "mir-wasm-text",
            Self::MirWasm => "mir-wasm",
            Self::MirSexp => "mir-sexp",
            Self::MirLlvmText => "mir-llvm-text",
            Self::MirMetalText => "mir-metal-text",
            Self::MirWgslText => "mir-wgsl-text",
        }
    }

    /// The HIR emit target for HIR-backed lanes (MIR lanes return `None`;
    /// they are emitted from validated MIR in a later wave).
    #[allow(dead_code)]
    fn hir_target(self) -> Option<Target> {
        match self {
            Self::Rust => Some(Target::HirRust),
            Self::TypeScript => Some(Target::HirTypeScript),
            Self::Go => Some(Target::HirGo),
            Self::Swift => Some(Target::HirSwift),
            _ => None,
        }
    }
}

/// Wave 1 active lanes (head-cto D4 ONE-context framing: rust lane first).
/// The remaining lanes stay wired-opt-in until a later wave measures them.
const ACTIVE_LANES: &[EmitLane] = &[EmitLane::Rust];

/// One seed: a corpus file or a defect-sprint fixture, with its generated
/// consumer shape (mirrors the cheap profile's [`SeedSource`] /
/// `CorpusEntry`).
struct Seed {
    /// Ledger row identity: corpus-relative path or `fixtures/<name>`.
    key: String,
    /// Source file for the standalone emit.
    file: PathBuf,
    /// Corpus import-boundary classification (fixture seeds have none).
    class: Option<CorpusClass>,
    /// Temp-root-relative module path the consumer imports.
    module_rel: String,
    /// Consumer-side import alias.
    alias: String,
    /// Extra `import from ...` lines in the consumer (multi-namespace seeds).
    extra_imports: String,
    /// Extra top-level consumer declarations exposing the import-boundary
    /// surface.
    extra_body: String,
    /// Copy same-directory `.fab` siblings into the temp root (relative
    /// imports / sibling-typed public surfaces need their dependency module).
    copy_same_dir: bool,
    /// The cheap-profile fixture policy key (fixture seeds only).
    fixture_key: Option<&'static str>,
}

fn seed_from_source(source: SeedSource) -> Seed {
    Seed {
        key: source.key.to_owned(),
        file: source.file.clone(),
        class: None,
        module_rel: source.module_rel.to_owned(),
        alias: source.alias.to_owned(),
        extra_imports: source.extra_imports.to_owned(),
        extra_body: source.extra_body.to_owned(),
        copy_same_dir: source.copy_same_dir,
        fixture_key: Some(source.key),
    }
}

fn seed_from_entry(entry: &radix_module_boundary::walk::CorpusEntry) -> Seed {
    Seed {
        key: entry.rel.clone(),
        file: entry.file.clone(),
        class: Some(entry.class),
        module_rel: entry.rel.trim_end_matches(".fab").to_owned(),
        alias: "m".to_owned(),
        extra_imports: String::new(),
        extra_body: String::new(),
        copy_same_dir: entry.has_same_dir_import,
        fixture_key: None,
    }
}

/// The defect-sprint fixture seeds (the cheap profile's seed subset minus the
/// corpus trio, which the corpus walk already covers).
fn fixture_seeds() -> Vec<Seed> {
    seed_sources()
        .into_iter()
        .filter(|source| source.key.starts_with("fixtures/"))
        .map(seed_from_source)
        .collect()
}

/// Every corpus entry plus the defect-sprint fixtures (the full walk).
fn all_seeds() -> Vec<Seed> {
    let mut seeds: Vec<Seed> = corpus_entries().iter().map(seed_from_entry).collect();
    seeds.extend(fixture_seeds());
    seeds
}

/// The narrow wave-1 proof set: the full seed subset (importa pair + curata +
/// the seven fixtures) plus one contract-reject corpus entry and one negative
/// exemplum (by-design floor).
fn narrow_seeds() -> Vec<Seed> {
    let mut seeds: Vec<Seed> = seed_sources().into_iter().map(seed_from_source).collect();
    let corpus = corpus_entries();
    for rel in [
        "adfirma/adfirma.fab",
        "gpu-core-types/atomic-element-reject.fab",
    ] {
        if let Some(entry) = corpus.iter().find(|entry| entry.rel == rel) {
            seeds.push(seed_from_entry(entry));
        }
    }
    seeds
}

/// Structured outcome of one emit.
#[derive(Debug, Clone, PartialEq, Eq)]
enum EmitOutcome {
    Ok,
    Failed { codes: Vec<String> },
}

/// Emit-lane known-defect rows. The cheap-profile ledger covers
/// parse/check/mir/stepper; the emit lane pins its own rows (collapse-oracle:
/// when the underlying fix lands, the row flips to parity).
struct EmitLaneDefect {
    key: &'static str,
    defect_id: &'static str,
    recheck_trigger: &'static str,
    pinned_codes: &'static [&'static str],
    reason: &'static str,
}

/// Wave-1 emit-lane known-defect rows (rust lane).
const EMIT_LANE_KNOWN_DEFECTS: &[EmitLaneDefect] = &[EmitLaneDefect {
    key: "fixtures/lib_mat.fab",
    defect_id: "codegen001-sibling-typed-import",
    recheck_trigger: "rust codegen resolves definition ids through sibling-typed imports (bug-4 family)",
    pinned_codes: &["CODEGEN001"],
    reason: "MB-U4 seed: the generated consumer imports lib_mat, whose public Mesh references the \
             sibling lib_graph Object3D through the import. Analysis is parity (cheap profile), but \
             the rust emit of the importing consumer fails `internal: definition id … could not be \
             resolved during code generation` — the fix:codegen001 family reproduced at the emit lane.",
}];

/// One (seed, lane) emit row.
#[derive(Debug)]
struct EmitLaneRow {
    key: String,
    lane: EmitLane,
    expected: Disposition,
    pinned_codes: Vec<String>,
    reason: String,
    standalone: EmitOutcome,
    imported: EmitOutcome,
}

/// Expected emit-lane disposition for one seed: the cheap profile's
/// imported-check policy is the floor because emission follows analysis.
/// Known-defect seeds pin the expected imported-emit failure codes; the
/// emit-lane table overrides parity rows that the cheap profile's layers
/// cannot see (codegen-layer defects).
fn expected_emit_disposition(
    seed: &Seed,
    standalone: &ModeMeasurement,
) -> (Disposition, Vec<String>, String) {
    let policy = match seed.fixture_key {
        Some(key) => policy_for(key, Mode::Imported, Layer::Check),
        None => corpus_policy_for(
            seed.class.expect("corpus seed carries a class"),
            standalone,
            Mode::Imported,
            Layer::Check,
        ),
    };
    if policy.disposition == Disposition::Parity {
        if let Some(defect) = EMIT_LANE_KNOWN_DEFECTS
            .iter()
            .find(|defect| defect.key == seed.key)
        {
            return (
                Disposition::KnownDefect,
                defect.pinned_codes.iter().map(|code| (*code).to_owned()).collect(),
                defect.reason.to_owned(),
            );
        }
    }
    (policy.disposition, policy.pinned_codes, policy.reason)
}

/// Measure one (seed, lane) row. Wave 1 wires only the rust lane; any
/// activated lane without its measurement fails fast (the lane table stays
/// honest about what wave 2+ must fill in).
fn measure_row(session: &Session, seed: &Seed, lane: EmitLane) -> EmitLaneRow {
    match lane {
        EmitLane::Rust => measure_rust_row(session, seed),
        _ => panic!(
            "lane {} is wired opt-in for wave 2+; only rust is active in wave 1 (head-cto D4)",
            lane.name()
        ),
    }
}

fn measure_rust_row(session: &Session, seed: &Seed) -> EmitLaneRow {
    let standalone_measurement = measure_standalone(session, &seed.file);
    let (expected, pinned_codes, reason) = expected_emit_disposition(seed, &standalone_measurement);
    let standalone = emit_rust_outcome(&seed.file, Some(&seed.key));
    let imported = measure_imported_emit(seed);
    EmitLaneRow {
        key: seed.key.clone(),
        lane: EmitLane::Rust,
        expected,
        pinned_codes,
        reason,
        standalone,
        imported,
    }
}

/// Generate the consumer for one seed and emit it through the rust lane.
fn measure_imported_emit(seed: &Seed) -> EmitOutcome {
    let generated = generate_consumer(
        &seed.file,
        &seed.module_rel,
        &seed.alias,
        &seed.extra_imports,
        &seed.extra_body,
        seed.copy_same_dir,
    );
    emit_rust_outcome(&generated.consumer_path, None)
}

/// Emit one source file through the rust lane, mirroring the rust harness's
/// compile branching (rust.rs): kernel imports are script-mode-only,
/// `norma:*` imports take the package compile path, the `importa/importa.fab`
/// pair needs its sibling directory, everything else goes through the
/// single-file CLI path with the reader-locale pack. `corpus_rel` is the
/// corpus-relative path (`None` for generated consumers and fixture files
/// outside the corpus walk).
fn emit_rust_outcome(file: &Path, corpus_rel: Option<&str>) -> EmitOutcome {
    let imports = import_paths_of(file);
    if imports
        .iter()
        .any(|path| radix::kernel::is_kernel_import_path(path))
    {
        return EmitOutcome::Failed {
            codes: vec!["kernel_import_script_mode_only".to_owned()],
        };
    }
    if imports.iter().any(|path| path.starts_with("norma:")) {
        let config = Config::default().with_target(Target::HirRust);
        return outcome_from_compile(&faber_cli::package::compile_package(&config, file));
    }
    // The rust module emission resolves relative provider imports against the
    // process CWD (the analysis stage uses the source file's parent), so any
    // file whose imports are `./…` must be emitted from its own directory.
    // This subsumes the importa-pair special case (rust.rs).
    if corpus_rel == Some("importa/importa.fab") || imports.iter().any(|p| p.starts_with("./")) {
        return emit_from_own_dir(file, corpus_rel == Some("importa/importa.fab"));
    }
    outcome_from_compile(&compile_cli_rust(file))
}

/// Single-file CLI rust emit with the reader-locale pack (the route the
/// exempla rust harness uses for ordinary corpus files).
fn compile_cli_rust(file: &Path) -> radix::CompileResult {
    let input = vec![file.display().to_string()];
    let code_pack = faber_cli::package::locale_pack_for_emit(&input, None)
        .ok()
        .flatten();
    radix::tool::compile_cli_path_with_locale_pack(
        file,
        false,
        Target::HirRust,
        code_pack.as_ref(),
        OutputMode::Application,
        None,
    )
}

/// Emit a file with the process CWD set to its own directory: the rust module
/// emission resolves relative provider imports (`./…`) against the process
/// CWD, so the entry directory is the resolution base (mirror of the importa
/// pair special case in rust.rs). The importa pair keeps its proven
/// `compile_cli_path` route; everything else takes the locale-pack CLI route.
fn emit_from_own_dir(file: &Path, importa_pair: bool) -> EmitOutcome {
    let Ok(previous) = std::env::current_dir() else {
        return EmitOutcome::Failed {
            codes: vec!["no-current-dir".to_owned()],
        };
    };
    let Some(entry_dir) = file.parent() else {
        return EmitOutcome::Failed {
            codes: vec!["no-parent-dir".to_owned()],
        };
    };
    if std::env::set_current_dir(entry_dir).is_err() {
        return EmitOutcome::Failed {
            codes: vec!["cannot-enter-entry-dir".to_owned()],
        };
    }
    let result = if importa_pair {
        radix::tool::compile_cli_path(file, false, Target::HirRust)
    } else {
        compile_cli_rust(file)
    };
    let _ = std::env::set_current_dir(&previous);
    outcome_from_compile(&result)
}

/// Capture a compile result as a structured emit outcome (reader-locale rule:
/// compare codes, not rendered prose).
fn outcome_from_compile(result: &radix::CompileResult) -> EmitOutcome {
    match result.output {
        Some(Output::Rust(_)) => EmitOutcome::Ok,
        Some(_) => EmitOutcome::Failed {
            codes: vec!["unexpected-output-kind".to_owned()],
        },
        None => {
            let mut codes: Vec<String> = result
                .diagnostics
                .iter()
                .filter(|diag| diag.is_error())
                .filter_map(|diag| diag.code.map(str::to_owned))
                .collect();
            codes.sort();
            codes.dedup();
            if codes.is_empty() {
                codes.push("emit-failed".to_owned());
            }
            EmitOutcome::Failed { codes }
        }
    }
}

/// Import paths declared by a source file (mirror of the rust harness's
/// `parsed_import_paths`), used only to route the standalone emit between the
/// single-file CLI path and the package/library path.
fn import_paths_of(file: &Path) -> Vec<String> {
    let Ok(source) = std::fs::read_to_string(file) else {
        return Vec::new();
    };
    let name = file.display().to_string();
    let Ok(peeled) = radix::driver::peel_raw_source(&name, &source) else {
        return Vec::new();
    };
    let input = vec![file.display().to_string()];
    let code_pack = faber_cli::package::locale_pack_for_emit(&input, None)
        .ok()
        .flatten();
    let lex_result = match code_pack.as_ref() {
        Some(pack) => radix::lexer::lex_with_locale_pack(peeled.body, pack),
        None => radix::lexer::lex(peeled.body),
    };
    if !lex_result.success() {
        return Vec::new();
    }
    let parse_result = radix::parser::parse(lex_result);
    if !parse_result.success() {
        return Vec::new();
    }
    let radix::parser::ParseResult {
        program, interner, ..
    } = parse_result;
    let Some(program) = program else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for stmt in &program.statements {
        let radix::syntax::StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        let import_path = interner.resolve(decl.path);
        paths.push(import_path.to_owned());
    }
    paths
}

/// Fail-closed verification: a parity row must emit parity (standalone Ok and
/// imported consumer Ok); a known-defect row must reproduce its pinned
/// imported-emit failure while the standalone file stays clean; by-design and
/// contract-reject rows are recorded dispositions (the cheap-profile checker
/// applies the same policy without a live-parity guard).
fn verify_rows(rows: &[EmitLaneRow]) -> Vec<String> {
    let mut failures = Vec::new();
    for row in rows {
        match row.expected {
            Disposition::Parity => {
                if row.standalone != EmitOutcome::Ok {
                    failures.push(format!(
                        "{}|{}: parity expected but standalone emit failed ({:?})",
                        row.key,
                        row.lane.name(),
                        row.standalone
                    ));
                }
                if row.imported != EmitOutcome::Ok {
                    failures.push(format!(
                        "{}|{}: parity expected but imported consumer emit failed ({:?})",
                        row.key,
                        row.lane.name(),
                        row.imported
                    ));
                }
            }
            Disposition::KnownDefect => {
                if row.standalone != EmitOutcome::Ok {
                    failures.push(format!(
                        "{}|{}: known-defect seed standalone emit must stay clean, got {:?}",
                        row.key,
                        row.lane.name(),
                        row.standalone
                    ));
                }
                match &row.imported {
                    EmitOutcome::Failed { codes } => {
                        let reproduced = row
                            .pinned_codes
                            .iter()
                            .any(|pinned| codes.iter().any(|got| got == pinned));
                        if !reproduced {
                            failures.push(format!(
                                "{}|{}: known-defect row did not reproduce pinned codes {:?} (imported emit failed with {:?})",
                                row.key,
                                row.lane.name(),
                                row.pinned_codes,
                                codes
                            ));
                        }
                    }
                    EmitOutcome::Ok => failures.push(format!(
                        "{}|{}: known-defect row stale — imported consumer emit succeeded, expected failure with {:?}",
                        row.key,
                        row.lane.name(),
                        row.pinned_codes
                    )),
                }
            }
            Disposition::CapabilityGap
            | Disposition::ContractReject
            | Disposition::ByDesign => {}
        }
    }
    failures
}

fn disposition_name(disposition: Disposition) -> &'static str {
    match disposition {
        Disposition::Parity => "parity",
        Disposition::KnownDefect => "known-defect",
        Disposition::CapabilityGap => "capability-gap",
        Disposition::ContractReject => "contract-reject",
        Disposition::ByDesign => "by-design",
    }
}

fn outcome_name(outcome: &EmitOutcome) -> String {
    match outcome {
        EmitOutcome::Ok => "ok".to_owned(),
        EmitOutcome::Failed { codes } => format!("fail:{}", codes.join(",")),
    }
}

fn print_report(rows: &[EmitLaneRow]) {
    let total = rows.len();
    let parity = rows
        .iter()
        .filter(|row| row.expected == Disposition::Parity)
        .count();
    let known = rows
        .iter()
        .filter(|row| row.expected == Disposition::KnownDefect)
        .count();
    let contract = rows
        .iter()
        .filter(|row| row.expected == Disposition::ContractReject)
        .count();
    let by_design = rows
        .iter()
        .filter(|row| row.expected == Disposition::ByDesign)
        .count();
    let standalone_ok = rows
        .iter()
        .filter(|row| row.standalone == EmitOutcome::Ok)
        .count();
    let imported_ok = rows
        .iter()
        .filter(|row| row.imported == EmitOutcome::Ok)
        .count();
    eprintln!("Module-boundary full profile (emit lanes; wave 1 rust — ONE context per head-cto D4):");
    eprintln!(
        "  seeds: {total}  (parity {parity}, known-defect {known}, contract-reject {contract}, by-design {by_design})"
    );
    eprintln!(
        "  emit: standalone ok {standalone_ok}/{total}; imported consumer ok {imported_ok}/{total}"
    );
    eprintln!(
        "  active lanes: {}",
        ACTIVE_LANES
            .iter()
            .map(|lane| lane.name())
            .collect::<Vec<_>>()
            .join(", ")
    );
    eprintln!(
        "  wired opt-in (wave 2+): ts/go/swift + MIR target lanes follow the existing matrix structure"
    );
    println!("ROWS");
    for row in rows {
        println!(
            "{}|{}|{}|{}|{}",
            row.key,
            row.lane.name(),
            disposition_name(row.expected),
            outcome_name(&row.standalone),
            outcome_name(&row.imported)
        );
    }
    let explained = rows
        .iter()
        .filter(|row| row.expected != Disposition::Parity && !row.reason.is_empty())
        .collect::<Vec<_>>();
    if !explained.is_empty() {
        println!("REASONS");
        for row in explained {
            println!("{}|{}|{}", row.key, row.lane.name(), row.reason);
        }
    }
}

/// Session for the module-boundary mode: corpus fixtures declare reader
/// locales, so the stdlib reader packs must resolve (matching the wasm lane's
/// session posture).
fn module_boundary_session() -> Session {
    Session::new(
        Config::default()
            .with_target(Target::HirRust)
            .with_stdlib(crate::paths::radix_stdlib_dir()),
    )
}

/// Narrow rust-lane proof: the seed subset plus one contract-reject and one
/// negative exemplum (by-design). This is the MB-U4 Hand closeout invocation;
/// the full-corpus walk is auditor-owned.
///
/// ```text
/// cargo test -p exempla --lib exempla_module_boundary_rust_lane -- --ignored --nocapture
/// ```
#[test]
#[ignore = "module-boundary rust-lane proof; run: cargo test -p exempla --lib exempla_module_boundary_rust_lane -- --ignored --nocapture"]
fn exempla_module_boundary_rust_lane() {
    let session = module_boundary_session();
    let seeds = narrow_seeds();
    let rows: Vec<EmitLaneRow> = ACTIVE_LANES
        .iter()
        .flat_map(|lane| seeds.iter().map(|seed| measure_row(&session, seed, *lane)))
        .collect();
    print_report(&rows);
    let failures = verify_rows(&rows);
    assert!(
        failures.is_empty(),
        "module-boundary rust-lane divergences:\n{}",
        failures.join("\n")
    );
}

/// Full-profile walk: every corpus entry plus the defect-sprint fixtures,
/// rust lane. Auditor-owned at audit boundaries (delivery MB-U4 gates) — the
/// Hand closeout is [`exempla_module_boundary_rust_lane`].
///
/// ```text
/// cargo test -p exempla --lib exempla_module_boundary_e2e -- --ignored --nocapture
/// ```
#[test]
#[ignore = "slow module-boundary full walk; run: cargo test -p exempla --lib exempla_module_boundary_e2e -- --ignored --nocapture"]
fn exempla_module_boundary_e2e() {
    let session = module_boundary_session();
    let seeds = all_seeds();
    let rows: Vec<EmitLaneRow> = ACTIVE_LANES
        .iter()
        .flat_map(|lane| seeds.iter().map(|seed| measure_row(&session, seed, *lane)))
        .collect();
    print_report(&rows);
    let failures = verify_rows(&rows);
    assert!(
        failures.is_empty(),
        "module-boundary full-profile divergences:\n{}",
        failures.join("\n")
    );
}
