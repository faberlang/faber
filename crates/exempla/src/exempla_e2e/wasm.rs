use super::common::{
    collect_exempla_files, command_available, format_diagnostic_messages, format_tier_line,
    make_temp_root, normalize_newline, read_expected_stdout,
};
use super::wasm_behavior_fixtures::{behavior_matches, expected_wasm_behavior};
use super::wasm_expectations::WASM_EXPECTED_TIER_FLOORS;
use super::wasm_external::{
    parse_wat_import_sites, probe_wasm_instantiation_stubless, probe_wasm_with_stub_host,
    run_wasm_entry_with_stub_host, validate_wasm_bytes, WasmInstantiationBucket, WasmRunBucket,
};
use radix::codegen::Target;
use radix::driver::Session;
use radix::lexer::Interner;
use radix::Config;
use std::fs;
use std::path::{Path, PathBuf};

/// Wasm exempla e2e tiers aligned with the Rust-parity contract (A–D).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum WasmTier {
    SourceReadable,
    FrontendAnalyzed,
    MirLowered,
    /// Tier A — Wasm bytes emitted in-tree.
    WasmEmitted,
    /// Tier B — external `wasm-tools validate` accepts the module.
    CompileValid,
    /// Tier C — external stub host runs `incipit` without trap.
    Runnable,
    /// Tier D — captured output matches sibling `*.expected` when present.
    OutputChecked,
}

#[derive(Debug)]
struct WasmE2eResult {
    path: PathBuf,
    tier: WasmTier,
    reason: String,
    stubless_bucket: Option<WasmInstantiationBucket>,
    stub_bucket: Option<WasmInstantiationBucket>,
    run_bucket: Option<WasmRunBucket>,
}

#[derive(Debug, Clone, Copy)]
struct WasmToolchain {
    validator_available: bool,
    stub_host_note: &'static str,
}

const EXPECTED_FRONTEND_ANALYZED_FLOOR: usize = 210;
const EXPECTED_MIR_LOWERED_FLOOR: usize = 194;
const EXPECTED_WASM_TIER_A_EMITTED_FLOOR: usize = 184;
const EXPECTED_WASM_TIER_B_COMPILE_VALID_FLOOR: usize = 180;
const EXPECTED_WASM_TIER_C_RUNNABLE_FLOOR: usize = 157;
const EXPECTED_WASM_TIER_D_OUTPUT_CHECKED_FLOOR: usize = 12;

#[test]
#[ignore = "slow wasm e2e; run: cargo test -p exempla --test e2e_harness exempla_wasm_e2e -- --ignored --nocapture"]
fn exempla_wasm_e2e() {
    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);
    assert!(
        !exempla.is_empty(),
        "Wasm e2e harness found no exempla files"
    );

    let session = Session::new(Config::default().with_target(Target::Wasm));
    let temp_root = make_temp_root();
    let toolchain = detect_wasm_toolchain();
    let mut results = Vec::with_capacity(exempla.len());

    for (idx, file) in exempla.iter().enumerate() {
        results.push(classify_wasm_exemplum(&session, file, idx, &temp_root));
    }

    print_wasm_e2e_report(&results, toolchain);
    assert_wasm_per_exemplum_floors(&results);
    assert_wasm_aggregate_floors(&results);
}

fn detect_wasm_toolchain() -> WasmToolchain {
    WasmToolchain {
        validator_available: command_available("wasm-tools", &["--version"]),
        stub_host_note:
            "external `radix-wasm-stub-host` subprocess (build with --features wasm-stub-host)",
    }
}

fn classify_wasm_exemplum(
    session: &Session,
    file: &Path,
    idx: usize,
    temp_root: &Path,
) -> WasmE2eResult {
    let source = match fs::read_to_string(file) {
        Ok(source) => source,
        Err(err) => {
            return wasm_result(
                file,
                WasmTier::SourceReadable,
                format!("cannot read source: {err}"),
                None,
                None,
                None,
            );
        }
    };

    let mut analysis =
        match radix::driver::analyze_source(session, &file.display().to_string(), &source) {
            Ok(analysis) => analysis,
            Err(diagnostics) => {
                return wasm_result(
                    file,
                    WasmTier::SourceReadable,
                    format!(
                        "frontend failed: {}",
                        format_diagnostic_messages(&diagnostics)
                    ),
                    None,
                    None,
                    None,
                );
            }
        };

    let interner = analysis.interner.clone();
    let mir = match radix::mir::lower_analyzed_unit_with_context(&mut analysis) {
        Ok(mir) => mir,
        Err(errors) => {
            return wasm_result(
                file,
                WasmTier::FrontendAnalyzed,
                format!(
                    "MIR lowering failed: {}",
                    errors
                        .iter()
                        .map(|error| error.issue.clone())
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
                None,
                None,
                None,
            );
        }
    };

    let (wat, wasm_bytes) = match radix::mir::emit_wasm_text_and_binary_probe_with_context(
        &mir.program,
        &mir.validation,
        &interner,
    ) {
        Ok(pair) => pair,
        Err(error) => {
            return wasm_result(
                file,
                WasmTier::MirLowered,
                format!("Wasm emission failed: {error}"),
                None,
                None,
                None,
            );
        }
    };

    let stem = file
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("exemplum");
    let wasm_file = temp_root.join(format!("{idx:03}-{stem}.wasm"));
    if let Err(err) = fs::write(&wasm_file, &wasm_bytes) {
        return wasm_result(
            file,
            WasmTier::WasmEmitted,
            format!("cannot write Wasm output: {err}"),
            None,
            None,
            None,
        );
    }

    let imports = parse_wat_import_sites(&wat);

    if let Err(reason) = validate_wasm_bytes(&wasm_file) {
        return wasm_result(file, WasmTier::WasmEmitted, reason, None, None, None);
    }

    let stubless_probe = probe_wasm_instantiation_stubless(&wasm_file, &imports);
    let stub_probe = probe_wasm_with_stub_host(&wasm_file);
    let run_probe = run_wasm_entry_with_stub_host(&wasm_file);
    let exemplum_key = wasm_exemplum_key(file);
    let (tier, reason) = match stub_probe.bucket {
        WasmInstantiationBucket::InstantiateValid => {
            classify_run_tier(file, &exemplum_key, &interner, &stub_probe, &run_probe)
        }
        WasmInstantiationBucket::MissingImport => (
            WasmTier::CompileValid,
            format!(
                "stub host blocked ({}): {}; stubless ({})",
                stub_probe.bucket, stub_probe.reason, stubless_probe.bucket
            ),
        ),
        WasmInstantiationBucket::InstantiationTrap => (
            WasmTier::CompileValid,
            format!(
                "stub host failed ({}): {}; stubless ({})",
                stub_probe.bucket, stub_probe.reason, stubless_probe.bucket
            ),
        ),
        WasmInstantiationBucket::NoRuntime => (
            WasmTier::CompileValid,
            format!(
                "stub host skipped ({}): {}; stubless ({})",
                stub_probe.bucket, stub_probe.reason, stubless_probe.bucket
            ),
        ),
    };

    wasm_result(
        file,
        tier,
        reason,
        Some(stubless_probe.bucket),
        Some(stub_probe.bucket),
        Some(run_probe.bucket),
    )
}

fn classify_run_tier(
    file: &Path,
    exemplum_key: &str,
    interner: &Interner,
    stub_probe: &super::wasm_external::WasmInstantiationProbe,
    run_probe: &super::wasm_external::WasmRunProbe,
) -> (WasmTier, String) {
    match run_probe.bucket {
        WasmRunBucket::Runnable => {
            if let Some(expected_stdout) = read_expected_stdout(file) {
                let captured = diag_events_to_stdout(interner, &run_probe.diag_events);
                if normalize_newline(&captured) == expected_stdout {
                    return (
                        WasmTier::OutputChecked,
                        format!(
                            "{}; output matched .expected ({} lines)",
                            run_probe.reason,
                            expected_stdout.lines().count()
                        ),
                    );
                }
                return (
                    WasmTier::Runnable,
                    format!(
                        "{}; .expected mismatch: got {:?}, want {:?}",
                        run_probe.reason, captured, expected_stdout
                    ),
                );
            }
            if let Some(expected) = expected_wasm_behavior(exemplum_key) {
                if behavior_matches(expected, &run_probe.diag_events) {
                    return (
                        WasmTier::OutputChecked,
                        format!(
                            "{}; behavior matched {} diag events",
                            run_probe.reason,
                            run_probe.diag_events.len()
                        ),
                    );
                }
                return (
                    WasmTier::Runnable,
                    format!(
                        "{}; behavior mismatch: expected {:?}, got {:?}",
                        run_probe.reason, expected, run_probe.diag_events
                    ),
                );
            }
            (
                WasmTier::Runnable,
                format!("{}; {}", stub_probe.reason, run_probe.reason),
            )
        }
        WasmRunBucket::NoEntryExport => (
            WasmTier::CompileValid,
            format!("{}; {}", stub_probe.reason, run_probe.reason),
        ),
        WasmRunBucket::EntryTrap => (
            WasmTier::CompileValid,
            format!("{}; {}", stub_probe.reason, run_probe.reason),
        ),
    }
}

fn diag_events_to_stdout(interner: &Interner, events: &[String]) -> String {
    let mut lines = Vec::new();
    for event in events {
        let Some((kind, index)) = event.split_once(':') else {
            continue;
        };
        if !kind.ends_with("_text") {
            continue;
        }
        let Ok(index) = index.parse::<u32>() else {
            continue;
        };
        let text = interner.resolve(radix::lexer::Symbol(index));
        lines.push(text.to_owned());
    }
    lines.join("\n")
}

fn wasm_result(
    file: &Path,
    tier: WasmTier,
    reason: String,
    stubless_bucket: Option<WasmInstantiationBucket>,
    stub_bucket: Option<WasmInstantiationBucket>,
    run_bucket: Option<WasmRunBucket>,
) -> WasmE2eResult {
    WasmE2eResult {
        path: file.to_path_buf(),
        tier,
        reason,
        stubless_bucket,
        stub_bucket,
        run_bucket,
    }
}

fn print_wasm_e2e_report(results: &[WasmE2eResult], toolchain: WasmToolchain) {
    let total = results.len();
    eprintln!("Wasm e2e toolchain:");
    eprintln!(
        "  tier B validator: {}",
        if toolchain.validator_available {
            "wasm-tools validate (external)"
        } else {
            "unavailable"
        }
    );
    eprintln!("  tier C/D runner: {}", toolchain.stub_host_note);
    eprintln!("Wasm e2e exempla (tiers A–D):");
    eprintln!(
        "{}",
        format_tier_line(
            "tier A emitted",
            count_wasm_tier(results, WasmTier::WasmEmitted),
            total,
            EXPECTED_WASM_TIER_A_EMITTED_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "tier B compile-valid",
            count_wasm_tier(results, WasmTier::CompileValid),
            total,
            EXPECTED_WASM_TIER_B_COMPILE_VALID_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "tier C runnable",
            count_wasm_tier(results, WasmTier::Runnable),
            total,
            EXPECTED_WASM_TIER_C_RUNNABLE_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "tier D output-checked",
            count_wasm_tier(results, WasmTier::OutputChecked),
            total,
            EXPECTED_WASM_TIER_D_OUTPUT_CHECKED_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "frontend analyzed",
            count_wasm_tier(results, WasmTier::FrontendAnalyzed),
            total,
            EXPECTED_FRONTEND_ANALYZED_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "MIR lowered",
            count_wasm_tier(results, WasmTier::MirLowered),
            total,
            EXPECTED_MIR_LOWERED_FLOOR,
        )
    );

    let compile_valid = results
        .iter()
        .filter(|result| result.tier >= WasmTier::CompileValid)
        .collect::<Vec<_>>();
    eprintln!("Wasm instantiation buckets (tier B+ subset, stubless):");
    eprintln!(
        "  missing-import: {}",
        count_stubless_bucket(&compile_valid, WasmInstantiationBucket::MissingImport)
    );
    eprintln!(
        "  instantiation-trap: {}",
        count_stubless_bucket(&compile_valid, WasmInstantiationBucket::InstantiationTrap)
    );
    eprintln!(
        "  instantiate-valid: {}",
        count_stubless_bucket(&compile_valid, WasmInstantiationBucket::InstantiateValid)
    );
    eprintln!(
        "  no-runtime: {}",
        count_stubless_bucket(&compile_valid, WasmInstantiationBucket::NoRuntime)
    );
    eprintln!("Wasm stub-host buckets (tier B+ subset):");
    eprintln!(
        "  instantiate-valid: {}",
        count_stub_bucket(&compile_valid, WasmInstantiationBucket::InstantiateValid)
    );
    eprintln!("Wasm run buckets (tier B+ subset):");
    eprintln!(
        "  runnable: {}",
        count_run_bucket(&compile_valid, WasmRunBucket::Runnable)
    );

    for result in results
        .iter()
        .filter(|result| result.tier < WasmTier::OutputChecked)
    {
        eprintln!(
            "[wasm:{:?}] {} :: {}",
            result.tier,
            result.path.display(),
            result.reason
        );
    }
}

fn count_wasm_tier(results: &[WasmE2eResult], tier: WasmTier) -> usize {
    results.iter().filter(|result| result.tier >= tier).count()
}

fn count_stubless_bucket(results: &[&WasmE2eResult], bucket: WasmInstantiationBucket) -> usize {
    results
        .iter()
        .filter(|result| result.stubless_bucket == Some(bucket))
        .count()
}

fn count_stub_bucket(results: &[&WasmE2eResult], bucket: WasmInstantiationBucket) -> usize {
    results
        .iter()
        .filter(|result| result.stub_bucket == Some(bucket))
        .count()
}

fn count_run_bucket(results: &[&WasmE2eResult], bucket: WasmRunBucket) -> usize {
    results
        .iter()
        .filter(|result| result.run_bucket == Some(bucket))
        .count()
}

fn assert_wasm_per_exemplum_floors(results: &[WasmE2eResult]) {
    let regressions = results
        .iter()
        .filter_map(|result| {
            let expected = expected_wasm_tier(&result.path);
            (result.tier < expected).then_some(format!(
                "{} expected at least {:?}, reached {:?}: {}",
                wasm_exemplum_key(&result.path),
                expected,
                result.tier,
                result.reason
            ))
        })
        .collect::<Vec<_>>();

    assert!(
        regressions.is_empty(),
        "unexpected Wasm per-exemplum tier regressions:\n{}",
        regressions.join("\n")
    );
}

fn assert_wasm_aggregate_floors(results: &[WasmE2eResult]) {
    let frontend = count_wasm_tier(results, WasmTier::FrontendAnalyzed);
    let mir = count_wasm_tier(results, WasmTier::MirLowered);
    let emitted = count_wasm_tier(results, WasmTier::WasmEmitted);
    let compile_valid = count_wasm_tier(results, WasmTier::CompileValid);
    let runnable = count_wasm_tier(results, WasmTier::Runnable);
    let output_checked = count_wasm_tier(results, WasmTier::OutputChecked);

    let regressions = [
        (
            "frontend analyzed",
            frontend,
            EXPECTED_FRONTEND_ANALYZED_FLOOR,
        ),
        ("MIR lowered", mir, EXPECTED_MIR_LOWERED_FLOOR),
        (
            "tier A emitted",
            emitted,
            EXPECTED_WASM_TIER_A_EMITTED_FLOOR,
        ),
        (
            "tier B compile-valid",
            compile_valid,
            EXPECTED_WASM_TIER_B_COMPILE_VALID_FLOOR,
        ),
        (
            "tier C runnable",
            runnable,
            EXPECTED_WASM_TIER_C_RUNNABLE_FLOOR,
        ),
        (
            "tier D output-checked",
            output_checked,
            EXPECTED_WASM_TIER_D_OUTPUT_CHECKED_FLOOR,
        ),
    ]
    .into_iter()
    .filter_map(|(label, actual, expected)| {
        (actual < expected).then_some(format!(
            "{label} expected at least {expected}, got {actual}"
        ))
    })
    .collect::<Vec<_>>();

    assert!(
        regressions.is_empty(),
        "unexpected Wasm aggregate tier regressions:\n{}",
        regressions.join("\n")
    );
}

fn expected_wasm_tier(path: &Path) -> WasmTier {
    let key = wasm_exemplum_key(path);
    WASM_EXPECTED_TIER_FLOORS
        .iter()
        .find_map(|(expected, tier)| (*expected == key).then_some(*tier))
        .unwrap_or(WasmTier::FrontendAnalyzed)
}

fn wasm_exemplum_key(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('\\', "/");
    for marker in ["/examples/corpus/", "/crates/exempla/corpus/"] {
        if let Some(rel) = normalized.split(marker).nth(1) {
            return rel.to_owned();
        }
    }
    if let Ok(rel) = path.strip_prefix(crate::paths::corpus_dir()) {
        return rel.display().to_string().replace('\\', "/");
    }
    normalized
}
