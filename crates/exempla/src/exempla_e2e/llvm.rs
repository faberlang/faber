//! LLVM exempla e2e harness: tiered emission, external verification, and runtime probes.
//!
//! Classifies each exemplum through the product package-to-LLVM builder
//! (`faber/src/package/llvm.rs`, S8.6): one `.ll` module per package unit plus
//! an inspectable link manifest, optional llvm-as/opt verification, and Tier
//! C/D link-and-run of ALL modules against the host runtime with captured
//! output compared against the Rust oracle and sibling `*.expected` files.
//! The harness keeps no secondary resolver/emitter for entry/CLI/importa/
//! Norma paths: selected `norma:*` units emit through the builder's S8.5
//! per-Norma-unit lane like every other package unit (one module per unit,
//! linked together). The single named non-builder lane is the fallback for
//! corpus stubs the package graph cannot represent at the analysis level — a
//! public export that cannot be snapshotted into a file interface (e.g.
//! `vector/infer.fab`'s inferred `vector<elem, _>` width) — which keeps their
//! historical pairwise classification through the radix single-file lane.

use super::common::{
    collect_exempla_files, command_available, floor_for_corpus, format_ceiling_line,
    format_diagnostic_messages, format_tier_line, make_temp_root,
};
use super::llvm_runtime::{LlvmRunBucket, LlvmRunProbe};
use super::oracle::{normalize_pairwise_output, rust_oracle, RustOracleOutcome};
use radix::codegen::Target;
use radix::driver::Session;
use radix::Config;
use std::collections::BTreeMap;
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

use super::expectations::llvm::{
    EXPECTED_FRONTEND_ANALYZED_FLOOR, EXPECTED_LLVM_EMITTED_FLOOR,
    EXPECTED_LLVM_OUTPUT_CHECKED_FLOOR, EXPECTED_LLVM_RUNNABLE_FLOOR,
    EXPECTED_LLVM_VERIFIER_VALID_FLOOR, EXPECTED_MIR_LOWERED_FLOOR,
    EXPECTED_UNSUPPORTED_DIAGNOSTIC_CEILING,
};

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

    let session = Session::new(Config::default().with_target(Target::MirLlvm));
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
    if let Err(err) = fs::read_to_string(file) {
        return llvm_result(
            file,
            LlvmTier::SourceReadable,
            LlvmEmissionBucket::OutputWriteFailed,
            format!("cannot read source: {err}"),
        );
    }
    // S8.6/S8.8: EVERY fixture — entry, CLI, local-import, and selected-Norma
    // (`norma:*`) — builds through the product package-to-LLVM builder
    // (`faber/src/package/llvm.rs`): the single shared implementation of the
    // package graph → unit modules + link manifest mapping, including the S8.5
    // per-Norma-unit emission. No harness-side resolver or emitter remains;
    // the only non-builder lane is the named non-package-shapeable fallback
    // inside `classify_builder_llvm_exemplum` (an analysis-level rejection of
    // a public export a file interface cannot represent, e.g.
    // `vector/infer.fab`).
    classify_builder_llvm_exemplum(session, file, idx, temp_root, toolchain)
}

/// S8.6: build one fixture through the product package-to-LLVM builder and
/// classify the result — per-unit verifier check, then one link+run of ALL
/// modules with the host runtime archive and the exact Rust-oracle args
/// (S8.1 process argumenta).
fn classify_builder_llvm_exemplum(
    session: &Session,
    file: &Path,
    idx: usize,
    temp_root: &Path,
    toolchain: &LlvmToolchain,
) -> LlvmE2eResult {
    let stem = file
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("exemplum");
    let runtime_archive = match super::llvm_runtime::llvm_runtime_archive() {
        Ok(path) => Some(path),
        Err(reason) => {
            return llvm_result(
                file,
                LlvmTier::SourceReadable,
                LlvmEmissionBucket::OutputWriteFailed,
                format!("LLVM host runtime archive unavailable: {reason}"),
            );
        }
    };
    let options = faber_cli::package::PackageLlvmOptions::new(
        temp_root.join(format!("{idx:03}-{stem}.modules")),
    )
    .with_runtime_archive(runtime_archive);
    // Product-faithful builder config: the product package build
    // (`config_with_locale`) carries no stdlib path, so the library resolver
    // probes the workspace for the `norma` provider home and resolves
    // selected `norma:*` units exactly as Rust package compile does. A
    // caller's `with_dev_stdlib` session config would point the resolver at
    // `radix/stdlib` — which has no `norma` provider repo — so the builder is
    // always called with the product config; the session stays for the
    // single-file fallback lane below.
    let builder_config = radix::Config::default().with_target(Target::MirLlvm);
    let build = match faber_cli::package::build_package_llvm(&builder_config, file, &options) {
        Ok(build) => build,
        Err(diagnostics) => {
            // MIR-lowering and LLVM-emission failures are lane-independent:
            // the single-file path fails them identically, so classify them
            // from the builder diagnostics directly.
            let lane_independent = diagnostics.iter().any(|diagnostic| {
                diagnostic.phase == radix::diagnostics::DiagnosticPhase::Mir
                    || diagnostic.issue() == Some("llvm_emission_failed")
            });
            if lane_independent {
                return classify_builder_failure(file, diagnostics);
            }
            // The package graph rejected the fixture at the analysis level.
            // The NAMED non-package-shapeable exception: a public export that
            // cannot be snapshotted into a file interface (e.g.
            // `vector/infer.fab`'s inferred `vector<elem, _>` width →
            // `IndexInferenceUnsupported`). Such fixtures are not
            // package-shapeable but remain valid single-file programs; run
            // them through the radix single-file lane so their historical
            // pairwise classification is preserved. Any OTHER analysis-level
            // rejection is a genuine package-analysis failure and must surface
            // as such — it is never masked by the fallback lane.
            let non_package_shapeable = diagnostics.iter().any(|diagnostic| {
                diagnostic.phase == radix::diagnostics::DiagnosticPhase::Analysis
                    && diagnostic
                        .message
                        .contains("cannot be represented in a file interface")
            });
            if non_package_shapeable {
                return classify_single_file_llvm_exemplum(
                    session, file, idx, temp_root, toolchain,
                );
            }
            return classify_builder_failure(file, diagnostics);
        }
    };
    classify_built_llvm(file, idx, temp_root, toolchain, &build)
}

/// Single-file compatibility lane for the NAMED non-package-shapeable corpus
/// stubs (analysis-level rejection only): radix `analyze_source` → MIR → LLVM
/// emission → verify/run. Kept ONLY for fixtures the package graph cannot
/// represent — a public export that cannot be snapshotted into a file
/// interface (e.g. `vector/infer.fab`'s inferred `vector<elem, _>` width);
/// entry/CLI/importa/norma fixtures always build through the product
/// package-to-LLVM builder and never take this path.
fn classify_single_file_llvm_exemplum(
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
    // S8.2: for the static-descriptor adapter lane the descriptor carries the
    // exit policy (Fixed/Binding/Field) and the runtime derives the process
    // code; the legacy fixed-code seam stays for non-adapter paths.
    let cli_adapter = analysis
        .cli_program
        .as_ref()
        .map(radix::cli_descriptor::build_cli_adapter_plan);
    let mir = match if analysis.cli_program.is_some() {
        radix::mir::lower_analyzed_unit_with_cli_adapter_with_context(&mut analysis)
    } else {
        radix::mir::lower_analyzed_unit_with_context(&mut analysis)
    } {
        Ok(mir) => mir,
        Err(errors) => {
            return llvm_result(
                file,
                LlvmTier::FrontendAnalyzed,
                LlvmEmissionBucket::MirLoweringFailed,
                format!(
                    "MIR lowering failed: {}",
                    errors
                        .iter()
                        .map(|error| error.issue.clone())
                        .collect::<Vec<_>>()
                        .join(" | ")
                ),
            );
        }
    };

    let llvm = match emit_llvm_for_program(&device_roles, &mir, cli_adapter) {
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

/// Emit the LLVM module for a lowered program: the S8.2 static-descriptor
/// adapter lane for CLI programs, the ordinary roles lane otherwise.
fn emit_llvm_for_program(
    device_roles: &rustc_hash::FxHashMap<radix::hir::DefId, radix::mir::MirDeviceRole>,
    mir: &radix::mir::LoweredMirUnit<'_>,
    cli_adapter: Option<radix::mir::CliAdapterPlan>,
) -> Result<String, radix::mir::MirLlvmTextProbeError> {
    match cli_adapter {
        Some(plan) => radix::mir::emit_llvm_text_probe_with_cli_adapter(
            device_roles,
            &mir.validated,
            &mir.interner,
            plan,
        ),
        None => radix::mir::emit_llvm_text_probe_with_device_roles_and_exit(
            device_roles,
            &mir.validated,
            &mir.interner,
            None,
        ),
    }
}

/// Classify lane-independent builder diagnostics (MIR lowering or LLVM
/// emission failures) into the tiered buckets the pairwise harness and the
/// gap ledger key on. The `unsupported-mir-shape` category keeps its
/// historical `Unsupported` bucket, so the S8.7 ratchet sees identical live
/// classification. (Package-analysis rejections are routed to the single-file
/// fallback by the caller, never here.)
fn classify_builder_failure(
    file: &Path,
    diagnostics: Vec<radix::diagnostics::Diagnostic>,
) -> LlvmE2eResult {
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.issue() == Some("llvm_emission_failed"))
    {
        let unsupported = diagnostics.iter().any(|diagnostic| {
            diagnostic.issue() == Some("llvm_emission_failed")
                && diagnostic.message.contains("unsupported-mir-shape")
        });
        return llvm_result(
            file,
            LlvmTier::MirLowered,
            if unsupported {
                LlvmEmissionBucket::Unsupported
            } else {
                LlvmEmissionBucket::EmissionFailed
            },
            format!(
                "{}: {}",
                if unsupported {
                    "LLVM emission unsupported"
                } else {
                    "LLVM emission failed"
                },
                format_diagnostic_messages(&diagnostics)
            ),
        );
    }
    let mir_failed = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.phase == radix::diagnostics::DiagnosticPhase::Mir);
    llvm_result(
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
            "package LLVM build failed: {}",
            format_diagnostic_messages(&diagnostics)
        ),
    )
}

/// Classify a built package: verify EVERY unit module with the external
/// verifier, then link ALL modules with the host runtime archive in one
/// invocation and run with the exact Rust-oracle args.
fn classify_built_llvm(
    file: &Path,
    idx: usize,
    temp_root: &Path,
    toolchain: &LlvmToolchain,
    build: &faber_cli::package::PackageLlvmBuild,
) -> LlvmE2eResult {
    let stem = file
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("exemplum");
    let modules = &build.manifest.modules;
    if let Some(verifier) = toolchain.verifier {
        for module in modules {
            if let Err(reason) = verify_llvm(verifier, module) {
                return llvm_result(
                    file,
                    LlvmTier::LlvmEmitted,
                    LlvmEmissionBucket::VerifierFailed,
                    format!(
                        "LLVM text emitted to {}; verifier failed: {reason}",
                        module.display()
                    ),
                );
            }
        }
        let oracle_args = rust_oracle(file).run_args();
        let run_probe = super::llvm_runtime::run_llvm_modules_with_args(
            modules,
            temp_root,
            &format!("{idx:03}-{stem}"),
            file,
            oracle_args,
        );
        return classify_llvm_run_tier(
            file,
            &build.manifest.entry_module,
            verifier,
            LlvmEmissionBucket::VerifierValid,
            run_probe,
        );
    }
    llvm_result(
        file,
        LlvmTier::LlvmEmitted,
        LlvmEmissionBucket::Emitted,
        format!(
            "LLVM text emitted to {}; verifier unavailable",
            build.manifest.entry_module.display()
        ),
    )
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
                // S8.1 process argumenta: the LLVM binary receives the exact
                // Rust oracle args so `incipit argumenta` fixtures observe the
                // same process context in both lanes.
                let oracle_args = rust_oracle(file).run_args();
                let run_probe = super::llvm_runtime::run_llvm_exemplum_with_args(
                    &llvm_file,
                    temp_root,
                    &format!("{idx:03}-{stem}"),
                    file,
                    oracle_args,
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
    let total = results.len();
    let frontend = count_llvm_tier(results, LlvmTier::FrontendAnalyzed);
    let mir = count_llvm_tier(results, LlvmTier::MirLowered);
    let llvm = count_llvm_tier(results, LlvmTier::LlvmEmitted);
    let verifier = count_llvm_tier(results, LlvmTier::LlvmVerifierValid);
    let unsupported = count_emission_bucket(results, LlvmEmissionBucket::Unsupported);

    let mut regressions = [
        (
            "frontend analyzed",
            frontend,
            floor_for_corpus(EXPECTED_FRONTEND_ANALYZED_FLOOR, total),
        ),
        (
            "MIR lowered",
            mir,
            floor_for_corpus(EXPECTED_MIR_LOWERED_FLOOR, total),
        ),
        (
            "LLVM emitted",
            llvm,
            floor_for_corpus(EXPECTED_LLVM_EMITTED_FLOOR, total),
        ),
        (
            "LLVM verifier-valid",
            verifier,
            floor_for_corpus(EXPECTED_LLVM_VERIFIER_VALID_FLOOR, total),
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

/// Captured process outcome for one Rust-oracle lane run.
struct Stage8ProcessOutcome {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// S8.6/S8.7/S8.8 ratchet: the Stage 8-owned corpus families — root entry
/// variants (S8.1), CLI programs (S8.2), the local-import package proof
/// (S8.4), and selected-Norma consumers (S8.5) — must pass through the
/// product package-to-LLVM builder (`faber/src/package/llvm.rs`) with
/// oracle-matching exit/stdout in BOTH the Rust oracle lane and the LLVM host
/// lane. Any row here is a Stage 8 gap: the pairwise gap ledger must stay
/// free of these paths.
#[test]
#[ignore = "slow LLVM host build+link+run; run: cargo test -p exempla --lib stage8_entry_cli_package_builder_parity -- --ignored --nocapture"]
fn stage8_entry_cli_package_builder_parity() {
    const STAGE8_FAMILIES: &[&str] = &[
        // S8.1 root entry variants: ordinary, module-scope, args, exit, declaration-only.
        "incipit/incipit.fab",
        "incipit/salve-munde.fab",
        "incipit/functionibus.fab",
        "argumenta/argumenta.fab",
        "exitus/exitus.fab",
        "curata/curata.fab",
        "fragilis/fragilis.fab",
        // S8.2 CLI programs.
        "cli/cli.fab",
        "operandus/operandus.fab",
        "optio/optio.fab",
        // S8.4 local imports (multi-module through the builder).
        "importa/importa.fab",
        // S8.5/S8.8 selected Norma units through the builder: a `norma:chorda`
        // consumer builds entry + one Norma-unit module and runs with
        // oracle-matching exit/stdout (the S8.6 consolidation ratchet).
        "importa/default-minimal.fab",
    ];

    let corpus_root = crate::paths::corpus_dir();
    let paths = STAGE8_FAMILIES
        .iter()
        .map(|rel| corpus_root.join(rel))
        .collect::<Vec<_>>();
    for path in &paths {
        assert!(
            path.is_file(),
            "missing Stage 8 fixture: {}",
            path.display()
        );
        let oracle = rust_oracle(path);
        assert!(
            oracle.is_executable(),
            "Stage 8 fixture {} must be executable",
            path.display()
        );
    }
    let temp_guard = make_temp_root();
    let temp_root = temp_guard.join("stage8-builder-parity");
    std::fs::create_dir_all(&temp_root).expect("cannot create Stage 8 temp root");

    // Rust oracle lane: the behavioral authority for exit/stdout.
    let rust = stage8_rust_lane(&corpus_root, &paths, &temp_root.join("rust"));

    // LLVM host lane: every family fixture routes through the builder.
    let toolchain = detect_llvm_toolchain();
    assert!(
        toolchain.is_available(),
        "Stage 8 builder parity requires llvm-as or opt"
    );
    let session = Session::new(
        Config::default()
            .with_target(Target::MirLlvm)
            .with_dev_stdlib(),
    );
    let mut failures = Vec::new();
    for (idx, path) in paths.iter().enumerate() {
        let relative = path
            .strip_prefix(&corpus_root)
            .expect("corpus path")
            .to_string_lossy()
            .into_owned();
        let result = classify_llvm_exemplum(&session, path, idx, &temp_root, &toolchain);
        let Some(probe) = &result.run_probe else {
            failures.push(format!(
                "{relative}: no LLVM run probe (tier {:?}, bucket {:?}): {}",
                result.tier, result.bucket, result.reason
            ));
            continue;
        };
        if let Err(issue) = check_stage8_pair(
            relative.as_str(),
            rust_oracle(path),
            rust.get(&relative),
            probe,
            path,
        ) {
            failures.push(issue);
        }
    }
    assert!(
        failures.is_empty(),
        "Stage 8 builder parity failures:\n{}",
        failures.join("\n")
    );
}

/// Build and run the Rust oracle lane for a small fixture set (the Stage 8
/// families): per-fixture codegen, one batched workspace build, run with the
/// exact oracle args.
fn stage8_rust_lane(
    corpus_root: &Path,
    paths: &[PathBuf],
    temp_root: &Path,
) -> BTreeMap<String, Stage8ProcessOutcome> {
    let compiler = radix::Compiler::new(radix::Config::default());
    let target = super::common::shared_target_dir(temp_root);
    let mut jobs = Vec::new();
    for (index, path) in paths.iter().enumerate() {
        let relative = path
            .strip_prefix(corpus_root)
            .expect("corpus path")
            .to_string_lossy()
            .into_owned();
        let code = super::rust::compile_rust_exemplum(&compiler, path, corpus_root)
            .unwrap_or_else(|reason| panic!("Rust oracle compile failed for {relative}: {reason}"));
        let code =
            crate::postprocess::format_generated_code(radix::codegen::Target::HirRust, &code)
                .unwrap_or(code);
        let stem = path
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("exemplum");
        let member = format!("rust-{index:03}-{stem}");
        let package = format!("parity_{index:03}_{stem}").replace('-', "_");
        super::common::write_rust_workspace_member(
            &temp_root.join(&member),
            &package,
            &super::rust::rust_member_code(path, &code),
        );
        jobs.push((relative, package, member));
    }
    let members = jobs
        .iter()
        .map(|(_, _, member)| member.clone())
        .collect::<Vec<_>>();
    let manifest = super::common::write_rust_workspace_root(temp_root, &members);
    let mut build = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    build
        .arg("build")
        .arg("--manifest-path")
        .arg(manifest)
        .env("CARGO_TARGET_DIR", &target);
    let status = super::common::command_status_with_timeout(&mut build, Duration::from_secs(900))
        .expect("cannot execute batched Rust oracle build");
    assert!(status.success(), "batched Rust oracle build failed");
    jobs.into_iter()
        .map(|(relative, package, _member)| {
            let mut command = Command::new(target.join(format!("debug/{package}")));
            command.args(rust_oracle(&corpus_root.join(&relative)).run_args());
            let output =
                super::common::command_output_with_timeout(&mut command, Duration::from_secs(20))
                    .unwrap_or_else(|error| panic!("cannot run Rust oracle {relative}: {error}"));
            (
                relative,
                Stage8ProcessOutcome {
                    exit_code: output.status.code(),
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                },
            )
        })
        .collect()
}

/// Oracle + rust-lane + LLVM-lane parity for one Stage 8 family fixture: exit
/// code against the oracle contract and stdout against the sibling `.expected`
/// (when present) or the rust lane (cross-lane equality).
fn check_stage8_pair(
    relative: &str,
    oracle: RustOracleOutcome,
    rust: Option<&Stage8ProcessOutcome>,
    llvm: &LlvmRunProbe,
    fab_path: &Path,
) -> Result<(), String> {
    let Some(rust) = rust else {
        return Err(format!("{relative}: rust oracle outcome missing"));
    };
    let expected_exit = match oracle {
        RustOracleOutcome::RunSuccess { exit_code, .. }
        | RustOracleOutcome::DeclarationOnly { exit_code, .. }
        | RustOracleOutcome::ExpectedNonzeroExit { exit_code, .. } => Some(exit_code),
        RustOracleOutcome::ExpectedRuntimeFailure {
            stderr_contains, ..
        } => {
            if rust.exit_code == Some(0) || !rust.stderr.contains(stderr_contains) {
                return Err(format!(
                    "{relative}: rust lane failed the runtime-failure contract"
                ));
            }
            if llvm.exit_code == Some(0) || !llvm.stderr.contains(stderr_contains) {
                return Err(format!(
                    "{relative}: LLVM lane failed the runtime-failure contract (exit {:?})",
                    llvm.exit_code
                ));
            }
            return Ok(());
        }
        RustOracleOutcome::ExpectedCompileFailure { .. }
        | RustOracleOutcome::ExplicitWrongLane { .. } => {
            return Err(format!("{relative}: unexpected non-executable oracle"));
        }
    };
    if rust.exit_code != expected_exit {
        return Err(format!(
            "{relative}: rust lane exit {:?} != oracle exit {:?}",
            rust.exit_code, expected_exit
        ));
    }
    if llvm.exit_code != expected_exit {
        return Err(format!(
            "{relative}: LLVM lane exit {:?} != oracle exit {:?}",
            llvm.exit_code, expected_exit
        ));
    }
    let rust_stdout = normalize_pairwise_output(&rust.stdout);
    let llvm_stdout = normalize_pairwise_output(&llvm.stdout);
    // Mirror the pairwise harness exactly: the sibling `.expected` file is
    // compared as raw bytes (trailing newline preserved via
    // `normalize_pairwise_output`); without one, the lanes must agree.
    if let Some(expected) = fs::read(fab_path.with_extension("expected")).ok() {
        let expected = normalize_pairwise_output(&String::from_utf8_lossy(&expected));
        if rust_stdout != expected {
            return Err(format!(
                "{relative}: rust lane stdout {rust_stdout:?} != .expected {expected:?}"
            ));
        }
        if llvm_stdout != expected {
            return Err(format!(
                "{relative}: LLVM lane stdout {llvm_stdout:?} != .expected {expected:?}"
            ));
        }
    } else if rust_stdout != llvm_stdout {
        return Err(format!(
            "{relative}: rust stdout {rust_stdout:?} != LLVM stdout {llvm_stdout:?}"
        ));
    }
    Ok(())
}
