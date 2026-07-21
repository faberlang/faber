//! LLVM exempla e2e harness: tiered emission, external verification, and runtime probes.
//!
//! Classifies each exemplum through frontend analysis, MIR lowering, LLVM IR
//! emission, and optional llvm-as/opt verification. When an external toolchain is
//! available, Tier C/D link and run emitted modules and compare captured output
//! against sibling `*.expected` files.

use super::common::{
    collect_exempla_files, command_available, format_ceiling_line, format_diagnostic_messages,
    format_tier_line, make_temp_root,
};
use super::llvm_runtime::{LlvmRunBucket, LlvmRunProbe};
use super::oracle::rust_oracle;
use super::rust;
use radix::codegen::Target;
use radix::driver::Session;
use radix::Config;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum LlvmTier {
    SourceReadable,
    FrontendAnalyzed,
    MirLowered,
    /// Tier A — LLVM IR text emitted.
    LlvmEmitted,
    /// Tier B — external llvm-as/opt accepts the module.
    LlvmVerifierValid,
    /// Tier C — linked binary runs `incipit` via external toolchain.
    LlvmRunnable,
    /// Tier D — captured output matches sibling `*.expected`.
    LlvmOutputChecked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LlvmEmissionBucket {
    FrontendFailed,
    MirLoweringFailed,
    Emitted,
    Unsupported,
    EmissionFailed,
    OutputWriteFailed,
    VerifierValid,
    VerifierFailed,
}

#[derive(Debug, Clone, Copy)]
enum LlvmVerifier {
    LlvmAs,
    Opt,
}

#[derive(Debug, Clone)]
pub(super) struct LlvmToolchain {
    verifier: Option<LlvmVerifier>,
    verifier_version: Option<String>,
}

impl LlvmToolchain {
    pub(super) fn is_available(&self) -> bool {
        self.verifier.is_some()
    }
}

// Tier floors and ceilings are ratcheted by the MIR LLVM baseline ledger.
const EXPECTED_FRONTEND_ANALYZED_FLOOR: usize = 235;
const EXPECTED_MIR_LOWERED_FLOOR: usize = 209;
const EXPECTED_LLVM_EMITTED_FLOOR: usize = 204;
const EXPECTED_LLVM_VERIFIER_VALID_FLOOR: usize = 204;
const EXPECTED_LLVM_RUNNABLE_FLOOR: usize = 48;
const EXPECTED_LLVM_OUTPUT_CHECKED_FLOOR: usize = 8;
/// Maximum exempla that may still hit explicit unsupported diagnostics (lower is better).
/// WHY: ratcheted 5 → 6 on 2026-07-02 to admit `conversio/fallibilis.fab`, a new
/// exemplum hitting the existing `try_call` MIR-to-LLVM gap. Ratcheted 6 → 9 on
/// 2026-07-05 for indexed GPU core type examples (`f16`, matrix, atomic) that
/// intentionally document current LLVM target rejection. This is a counted debt
/// budget, not a fix; Stage 8 (failable-call-cfg) and later GPU core backend
/// stages own ratcheting it back down. See docs/factory/mir-llvm/baseline-ledger.md.
const EXPECTED_UNSUPPORTED_DIAGNOSTIC_CEILING: usize = 9;

#[derive(Debug)]
pub(super) struct LlvmE2eResult {
    pub(super) path: PathBuf,
    pub(super) tier: LlvmTier,
    pub(super) bucket: LlvmEmissionBucket,
    pub(super) reason: String,
    pub(super) run_probe: Option<LlvmRunProbe>,
}

#[test]
#[ignore = "slow llvm e2e; run: cargo test -p exempla --test e2e_harness exempla_llvm_e2e -- --ignored --nocapture"]
fn exempla_llvm_e2e() {
    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);
    assert!(
        !exempla.is_empty(),
        "LLVM e2e harness found no exempla files"
    );

    let session = Session::new(Config::default().with_target(Target::LlvmText));
    let temp_root = make_temp_root();
    let toolchain = detect_llvm_toolchain();
    let mut results = Vec::with_capacity(exempla.len());

    for (idx, file) in exempla.iter().enumerate() {
        results.push(classify_llvm_exemplum(
            &session, file, idx, &temp_root, &toolchain,
        ));
    }

    print_llvm_e2e_report(&results, &toolchain);
    assert_llvm_staging_gates(&results);
    warn_llvm_host_floors(&results);
}

pub(super) fn detect_llvm_toolchain() -> LlvmToolchain {
    let verifier = if command_available("llvm-as", &["--version"]) {
        Some(LlvmVerifier::LlvmAs)
    } else if command_available("opt", &["--version"]) {
        Some(LlvmVerifier::Opt)
    } else {
        None
    };
    let verifier_version = verifier.map(llvm_verifier_version);

    LlvmToolchain {
        verifier,
        verifier_version,
    }
}

fn llvm_verifier_version(verifier: LlvmVerifier) -> String {
    let output = match verifier {
        LlvmVerifier::LlvmAs => Command::new("llvm-as").arg("--version").output(),
        LlvmVerifier::Opt => Command::new("opt").arg("--version").output(),
    };
    let Ok(output) = output else {
        return "version unavailable".to_owned();
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .unwrap_or("version unavailable")
        .to_owned()
}

pub(super) fn classify_llvm_exemplum(
    session: &Session,
    file: &Path,
    idx: usize,
    temp_root: &Path,
    toolchain: &LlvmToolchain,
) -> LlvmE2eResult {
    let source = match fs::read_to_string(file) {
        Ok(source) => source,
        Err(err) => {
            return llvm_result(
                file,
                LlvmTier::SourceReadable,
                LlvmEmissionBucket::OutputWriteFailed,
                format!("cannot read source: {err}"),
            );
        }
    };
    match rust::uses_package_library_import_for(file) {
        Ok(true) => return classify_package_llvm_exemplum(file, idx, temp_root, toolchain),
        Ok(false) => {}
        Err(reason) => {
            return llvm_result(
                file,
                LlvmTier::SourceReadable,
                LlvmEmissionBucket::FrontendFailed,
                format!("cannot inspect imports: {reason}"),
            );
        }
    }
    let mut analysis =
        match radix::driver::analyze_source(session, &file.display().to_string(), &source) {
            Ok(analysis) => analysis,
            Err(diagnostics) => {
                return llvm_result(
                    file,
                    LlvmTier::SourceReadable,
                    LlvmEmissionBucket::FrontendFailed,
                    format!(
                        "frontend failed: {}",
                        format_diagnostic_messages(&diagnostics)
                    ),
                );
            }
        };

    let device_roles = radix::mir::device_roles_from_hir(&analysis.hir);
    let cli_program = analysis.cli_program.is_some();
    let mir = match if cli_program {
        radix::mir::lower_analyzed_unit_allowing_cli_entry_with_context(&mut analysis)
    } else {
        radix::mir::lower_analyzed_unit_with_context(&mut analysis)
    } {
        Ok(mir) => mir,
        Err(errors) => {
            let cli_record_pending = cli_program
                && errors.iter().all(|error| {
                    error.issue == "invalid_mir_record_aggregate_is_missing_required_field"
                });
            return llvm_result(
                file,
                LlvmTier::FrontendAnalyzed,
                LlvmEmissionBucket::MirLoweringFailed,
                format!(
                    "{}MIR lowering failed: {}",
                    if cli_record_pending {
                        "CLI runtime record binding pending; "
                    } else {
                        ""
                    },
                    errors
                        .iter()
                        .map(|error| error.issue.clone())
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
            );
        }
    };

    let llvm = match radix::mir::emit_llvm_text_probe_with_device_roles(
        &device_roles,
        &mir.program,
        &mir.validation,
        &mir.interner,
    ) {
        Ok(llvm) => llvm,
        Err(error) if error.category == "unsupported-mir-shape" => {
            return llvm_result(
                file,
                LlvmTier::MirLowered,
                LlvmEmissionBucket::Unsupported,
                format!(
                    "LLVM emission unsupported: {}:{}",
                    error.category, error.shape
                ),
            );
        }
        Err(error) => {
            return llvm_result(
                file,
                LlvmTier::MirLowered,
                LlvmEmissionBucket::EmissionFailed,
                format!("LLVM emission failed: {}:{}", error.category, error.shape),
            );
        }
    };

    classify_emitted_llvm(file, idx, temp_root, toolchain, llvm)
}

fn classify_package_llvm_exemplum(
    file: &Path,
    idx: usize,
    temp_root: &Path,
    toolchain: &LlvmToolchain,
) -> LlvmE2eResult {
    let config = radix::Config::default().with_target(Target::LlvmText);
    let emitted = faber_cli::package::with_lowered_package_mir(&config, file, |lowered| {
        let Some(interner) = lowered.validation.interner else {
            return Err("package MIR validation context has no interner".to_owned());
        };
        radix::mir::emit_llvm_text_probe_with_context(
            &lowered.program,
            &lowered.validation,
            interner,
        )
        .map_err(|error| format!("{}:{}", error.category, error.shape))
    });
    let llvm = match emitted {
        Err(diagnostics) => {
            let mir_failed = diagnostics
                .iter()
                .any(|diagnostic| diagnostic.phase == radix::diagnostics::DiagnosticPhase::Mir);
            return llvm_result(
                file,
                if mir_failed {
                    LlvmTier::FrontendAnalyzed
                } else {
                    LlvmTier::SourceReadable
                },
                if mir_failed {
                    LlvmEmissionBucket::MirLoweringFailed
                } else {
                    LlvmEmissionBucket::FrontendFailed
                },
                format!(
                    "package analysis/MIR failed: {}",
                    format_diagnostic_messages(&diagnostics)
                ),
            );
        }
        Ok(Err(reason)) if reason.starts_with("unsupported-mir-shape:") => {
            return llvm_result(
                file,
                LlvmTier::MirLowered,
                LlvmEmissionBucket::Unsupported,
                format!("LLVM emission unsupported: {reason}"),
            );
        }
        Ok(Err(reason)) => {
            return llvm_result(
                file,
                LlvmTier::MirLowered,
                LlvmEmissionBucket::EmissionFailed,
                format!("LLVM emission failed: {reason}"),
            );
        }
        Ok(Ok(llvm)) => llvm,
    };
    classify_emitted_llvm(file, idx, temp_root, toolchain, llvm)
}

fn classify_emitted_llvm(
    file: &Path,
    idx: usize,
    temp_root: &Path,
    toolchain: &LlvmToolchain,
    llvm: String,
) -> LlvmE2eResult {
    let stem = file
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("exemplum");
    let llvm_file = temp_root.join(format!("{idx:03}-{stem}.ll"));
    if let Err(err) = fs::write(&llvm_file, &llvm) {
        return llvm_result(
            file,
            LlvmTier::LlvmEmitted,
            LlvmEmissionBucket::OutputWriteFailed,
            format!("cannot write LLVM output: {err}"),
        );
    }

    if let Some(verifier) = toolchain.verifier {
        match verify_llvm(verifier, &llvm_file) {
            Ok(()) => {
                let run_probe = super::llvm_runtime::run_llvm_exemplum(
                    &llvm_file,
                    temp_root,
                    &format!("{idx:03}-{stem}"),
                    file,
                );
                return classify_llvm_run_tier(
                    file,
                    &llvm_file,
                    verifier,
                    LlvmEmissionBucket::VerifierValid,
                    run_probe,
                );
            }
            Err(reason) => {
                return llvm_result(
                    file,
                    LlvmTier::LlvmEmitted,
                    LlvmEmissionBucket::VerifierFailed,
                    format!(
                        "LLVM text emitted to {}; verifier failed: {reason}",
                        llvm_file.display()
                    ),
                );
            }
        }
    }

    llvm_result(
        file,
        LlvmTier::LlvmEmitted,
        LlvmEmissionBucket::Emitted,
        format!(
            "LLVM text emitted to {}; verifier unavailable",
            llvm_file.display()
        ),
    )
}

fn classify_llvm_run_tier(
    file: &Path,
    llvm_file: &Path,
    verifier: LlvmVerifier,
    bucket: LlvmEmissionBucket,
    run_probe: LlvmRunProbe,
) -> LlvmE2eResult {
    let verified_reason = format!(
        "LLVM text emitted and verified with {} at {}",
        verifier.command(),
        llvm_file.display()
    );
    let mut result = match run_probe.bucket {
        LlvmRunBucket::OutputMatched => llvm_result(
            file,
            LlvmTier::LlvmOutputChecked,
            bucket,
            format!("{verified_reason}; {}", run_probe.reason),
        ),
        LlvmRunBucket::Runnable => llvm_result(
            file,
            LlvmTier::LlvmRunnable,
            bucket,
            format!("{verified_reason}; {}", run_probe.reason),
        ),
        LlvmRunBucket::ToolchainMissing => llvm_result(
            file,
            LlvmTier::LlvmVerifierValid,
            bucket,
            format!("{verified_reason}; {}", run_probe.reason),
        ),
        LlvmRunBucket::LinkFailed | LlvmRunBucket::RunFailed => llvm_result(
            file,
            LlvmTier::LlvmVerifierValid,
            bucket,
            format!("{verified_reason}; tier C failed: {}", run_probe.reason),
        ),
    };
    result.run_probe = Some(run_probe);
    result
}

fn llvm_result(
    file: &Path,
    tier: LlvmTier,
    bucket: LlvmEmissionBucket,
    reason: String,
) -> LlvmE2eResult {
    LlvmE2eResult {
        path: file.to_path_buf(),
        tier,
        bucket,
        reason,
        run_probe: None,
    }
}

fn verify_llvm(verifier: LlvmVerifier, llvm_file: &Path) -> Result<(), String> {
    let mut command = match verifier {
        LlvmVerifier::LlvmAs => {
            let mut command = Command::new("llvm-as");
            command
                .arg("-o")
                .arg(if cfg!(windows) { "NUL" } else { "/dev/null" })
                .arg(llvm_file);
            command
        }
        LlvmVerifier::Opt => {
            let mut command = Command::new("opt");
            command.arg("-disable-output").arg(llvm_file);
            command
        }
    };
    let output = super::common::command_output_with_timeout(&mut command, Duration::from_secs(120))
        .map_err(|err| format!("cannot execute LLVM verifier: {err}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

fn print_llvm_e2e_report(results: &[LlvmE2eResult], toolchain: &LlvmToolchain) {
    let total = results.len();
    eprintln!("LLVM e2e toolchain:");
    eprintln!(
        "  verifier: {}",
        match toolchain.verifier {
            Some(verifier) => match &toolchain.verifier_version {
                Some(version) => format!("{} ({version})", verifier.command()),
                None => verifier.command().to_owned(),
            },
            None => "unavailable (llvm-as/opt not found)".to_owned(),
        }
    );
    eprintln!("  tier C/D runner: coherent external LLVM tools + Rust LLVM host runtime");
    eprintln!("LLVM e2e exempla (tiers A–D):");
    eprintln!(
        "{}",
        format_tier_line(
            "frontend analyzed",
            count_llvm_tier(results, LlvmTier::FrontendAnalyzed),
            total,
            EXPECTED_FRONTEND_ANALYZED_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "MIR lowered",
            count_llvm_tier(results, LlvmTier::MirLowered),
            total,
            EXPECTED_MIR_LOWERED_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "LLVM emitted",
            count_llvm_tier(results, LlvmTier::LlvmEmitted),
            total,
            EXPECTED_LLVM_EMITTED_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "tier B verifier-valid",
            count_llvm_tier(results, LlvmTier::LlvmVerifierValid),
            total,
            EXPECTED_LLVM_VERIFIER_VALID_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "tier C runnable",
            count_llvm_tier(results, LlvmTier::LlvmRunnable),
            total,
            EXPECTED_LLVM_RUNNABLE_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "tier D output-checked",
            count_llvm_tier(results, LlvmTier::LlvmOutputChecked),
            total,
            EXPECTED_LLVM_OUTPUT_CHECKED_FLOOR,
        )
    );
    eprintln!(
        "  frontend failed: {}",
        count_emission_bucket(results, LlvmEmissionBucket::FrontendFailed)
    );
    eprintln!(
        "  MIR lowering failed: {}",
        count_emission_bucket(results, LlvmEmissionBucket::MirLoweringFailed)
    );
    eprintln!(
        "{}",
        format_ceiling_line(
            "unsupported diagnostic",
            count_emission_bucket(results, LlvmEmissionBucket::Unsupported),
            EXPECTED_UNSUPPORTED_DIAGNOSTIC_CEILING,
        )
    );
    eprintln!(
        "  emission failed: {}",
        count_emission_bucket(results, LlvmEmissionBucket::EmissionFailed)
    );
    eprintln!(
        "  output write failed: {}",
        count_emission_bucket(results, LlvmEmissionBucket::OutputWriteFailed)
    );
    eprintln!(
        "  verifier failed: {}",
        count_emission_bucket(results, LlvmEmissionBucket::VerifierFailed)
    );

    for result in results
        .iter()
        .filter(|result| result.tier < LlvmTier::LlvmEmitted)
    {
        eprintln!(
            "[llvm:{:?}] {} :: {}",
            result.tier,
            result.path.display(),
            result.reason
        );
    }

    let corpus_dir = crate::paths::corpus_dir();
    for result in results {
        let relative = result
            .path
            .strip_prefix(&corpus_dir)
            .unwrap_or(&result.path);
        let oracle = rust_oracle(&result.path);
        eprintln!(
            "[llvm-baseline] {}\trust_executable={}\trust_oracle={oracle:?}\ttier={:?}\tbucket={:?}\t{}",
            relative.display(),
            oracle.is_executable(),
            result.tier,
            result.bucket,
            result.reason.replace('\n', " ")
        );
    }
}

fn count_llvm_tier(results: &[LlvmE2eResult], tier: LlvmTier) -> usize {
    results.iter().filter(|result| result.tier >= tier).count()
}

fn count_emission_bucket(results: &[LlvmE2eResult], bucket: LlvmEmissionBucket) -> usize {
    results
        .iter()
        .filter(|result| result.bucket == bucket)
        .count()
}

/// Campaign staging gates (verification plan step 3): Tier A/B floors + unsupported ceiling.
fn assert_llvm_staging_gates(results: &[LlvmE2eResult]) {
    let frontend = count_llvm_tier(results, LlvmTier::FrontendAnalyzed);
    let mir = count_llvm_tier(results, LlvmTier::MirLowered);
    let llvm = count_llvm_tier(results, LlvmTier::LlvmEmitted);
    let verifier = count_llvm_tier(results, LlvmTier::LlvmVerifierValid);
    let unsupported = count_emission_bucket(results, LlvmEmissionBucket::Unsupported);

    let mut regressions = [
        (
            "frontend analyzed",
            frontend,
            EXPECTED_FRONTEND_ANALYZED_FLOOR,
        ),
        ("MIR lowered", mir, EXPECTED_MIR_LOWERED_FLOOR),
        ("LLVM emitted", llvm, EXPECTED_LLVM_EMITTED_FLOOR),
        (
            "LLVM verifier-valid",
            verifier,
            EXPECTED_LLVM_VERIFIER_VALID_FLOOR,
        ),
    ]
    .into_iter()
    .filter_map(|(label, actual, expected)| {
        (actual < expected).then_some(format!(
            "{label} expected at least {expected}, got {actual}"
        ))
    })
    .collect::<Vec<_>>();
    if unsupported > EXPECTED_UNSUPPORTED_DIAGNOSTIC_CEILING {
        regressions.push(format!(
            "unsupported diagnostic expected at most {}, got {unsupported}",
            EXPECTED_UNSUPPORTED_DIAGNOSTIC_CEILING
        ));
    }

    assert!(
        regressions.is_empty(),
        "unexpected LLVM staging gate regressions:\n{}",
        regressions.join("\n")
    );
}

/// Tier C/D are non-gating (Stage 6 deferred); warn when host link/run floors dip.
fn warn_llvm_host_floors(results: &[LlvmE2eResult]) {
    let runnable = count_llvm_tier(results, LlvmTier::LlvmRunnable);
    let output_checked = count_llvm_tier(results, LlvmTier::LlvmOutputChecked);
    if runnable < EXPECTED_LLVM_RUNNABLE_FLOOR {
        eprintln!(
            "LLVM e2e warning: tier C runnable {runnable} below informational floor {}",
            EXPECTED_LLVM_RUNNABLE_FLOOR
        );
    }
    if output_checked < EXPECTED_LLVM_OUTPUT_CHECKED_FLOOR {
        eprintln!(
            "LLVM e2e warning: tier D output-checked {output_checked} below informational floor {}",
            EXPECTED_LLVM_OUTPUT_CHECKED_FLOOR
        );
    }
}

impl LlvmVerifier {
    fn command(self) -> &'static str {
        match self {
            LlvmVerifier::LlvmAs => "llvm-as",
            LlvmVerifier::Opt => "opt -disable-output",
        }
    }
}
