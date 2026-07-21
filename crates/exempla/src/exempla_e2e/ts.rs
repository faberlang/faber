use super::common::{
    collect_exempla_files, command_available, format_diagnostic_messages, format_tier_line,
    make_temp_root, normalize_newline, read_expected_stdout, TSC_SMOKE_ARGS,
};
use radix::codegen::{self, Target};
use radix::hir::{HirItemKind, HirProgram};
use radix::lexer::Interner;
use radix::{Config, Output};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

// Baseline 2026-07-12: the harness discovers the 292-file sibling
// `examples/corpus` through `exempla::paths`. Historical 305/318-file floors
// are not comparable after the public-corpus split; keep this ratchet tied to
// the measured current corpus instead of silently normalizing denominators.
const EXPECTED_TS_FRONTEND_ANALYZED_FLOOR: usize = 279;
const EXPECTED_TS_EMITTED_FLOOR: usize = 276;
const EXPECTED_TS_TYPECHECK_VALID_FLOOR: usize = 259;
const EXPECTED_TS_RUNNABLE_FLOOR: usize = 256;

#[derive(Debug)]
struct TsE2eResult {
    path: PathBuf,
    frontend_analyzed: bool,
    typescript_emitted: bool,
    formatted: TierState,
    linted: TierState,
    typecheck_valid: TierState,
    runnable: TierState,
    behavior_checked: TierState,
    reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TierState {
    Passed,
    Failed,
    Skipped,
}

const TS_EXPECTED_OUTCOMES: &[ExpectedTsOutcome] = &[
    ExpectedTsOutcome {
        path: "ad/solum-lege-generic.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::SplitOut,
        bucket: "frontend semantic gap",
        reason_contains: "dynamic_receiver_method_type_args",
    },

    ExpectedTsOutcome {
        path: "cli/cli.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "missing type/variant binding",
        reason_contains: "error TS2304: Cannot find name 'args'",
    },
    ExpectedTsOutcome {
        path: "conversio/fallibilis.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "failable / conversio lowering",
        reason_contains: "Property 'error' does not exist on type 'never'",
    },
    ExpectedTsOutcome {
        path: "conversio/valor-genus.fab",
        // Reclassified from the pre-wrapper TypecheckValid row: current TS
        // codegen rejects this JSON-root Valor explicitly, so the lower tier is
        // an evidenced feature gap rather than hidden coverage debt.
        highest_tier: TsHighestTier::FrontendAnalyzed,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "json valor not supported",
        reason_contains: "json is not supported for the TypeScript target",
    },
    ExpectedTsOutcome {
        path: "conversio/valor-tensor.fab",
        // Same explicit JSON-root wrapper gap as valor-genus; do not restore
        // the stale Runnable classification until the wrapper exists.
        highest_tier: TsHighestTier::FrontendAnalyzed,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "json valor not supported",
        reason_contains: "json is not supported for the TypeScript target",
    },
    ExpectedTsOutcome {
        path: "discerne/discerne.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "expression-valued control-flow lowering",
        reason_contains: "error TS2355: A function whose declared type is neither 'undefined', 'void', nor 'any' must return a value",
    },
    ExpectedTsOutcome {
        path: "fac/fac-cape.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "unreachable cape branch narrowing",
        reason_contains: "Property 'error' does not exist on type 'never'",
    },
    ExpectedTsOutcome {
        path: "gpu-core-types/atomic-element-reject.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "atomic_element",
    },
    ExpectedTsOutcome {
        path: "gpu-core-types/f16-bf16-reject.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "unknown_type",
    },
    ExpectedTsOutcome {
        path: "gpu-core-types/matrix-tensor-reject.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "expression_type_mismatch",
    },

    ExpectedTsOutcome {
        path: "importa/default-braced.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::SplitOut,
        bucket: "package HAL split-out",
        reason_contains: "Cannot find module 'norma:chorda' or its corresponding type declarations.",
    },
    ExpectedTsOutcome {
        path: "importa/default-minimal.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::SplitOut,
        bucket: "package HAL split-out",
        reason_contains: "Cannot find module 'norma:chorda' or its corresponding type declarations.",
    },
    ExpectedTsOutcome {
        path: "instans/instans.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::SplitOut,
        bucket: "frontend semantic gap",
        reason_contains: "expression_type_mismatch",
    },
    ExpectedTsOutcome {
        path: "lege/lege.fab",
        highest_tier: TsHighestTier::TypecheckValid,
        kind: ExpectedTsKind::RuntimeFailure,
        bucket: "runtime input provider gap",
        reason_contains: "ReferenceError: prompt is not defined",
    },
    ExpectedTsOutcome {
        path: "protecta/protecta.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "protecta_reserved",
    },
    ExpectedTsOutcome {
        path: "rumpe/fac-dum-rumpe.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "unreachable cape branch narrowing",
        reason_contains: "Property 'error' does not exist on type 'never'",
    },
    ExpectedTsOutcome {
        path: "rumpe/fac-si-rumpe.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "break/continue closure lowering",
        reason_contains: "Jump target cannot cross function boundary",
    },
    ExpectedTsOutcome {
        path: "rumpe/rumpe-top-level-error.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "break_outside_breakable",
    },

    ExpectedTsOutcome {
        path: "sparsa/conversio-reject.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "sparsa_tensor_shape_mismatch",
    },
    ExpectedTsOutcome {
        path: "sparsa/non-numeric-reject.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "sparsa_element_non_numeric",
    },

    ExpectedTsOutcome {
        path: "sub/sub.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "class/genus declaration shape",
        reason_contains: "error TS2612: Property 'nomen' will overwrite the base property in 'Animal'",
    },
    ExpectedTsOutcome {
        path: "tensor/arithmetic-reject.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "tensor_arithmetic_numeric_element_required",
    },
    ExpectedTsOutcome {
        path: "tensor/method-errors.fab",
        highest_tier: TsHighestTier::TypecheckValid,
        kind: ExpectedTsKind::RuntimeBehavior,
        bucket: "expected runtime error behavior",
        reason_contains: "tensor structa element count does not match shape",
    },
    ExpectedTsOutcome {
        path: "typi/sized-family-error.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::CompileFail,
        bucket: "expected compile-fail / frontend policy",
        reason_contains: "float_width_on_numerus",
    },
    ExpectedTsOutcome {
        path: "vector/builtins.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "missing type/variant binding",
        reason_contains: "Cannot find name 'unresolved_def'",
    },
    ExpectedTsOutcome {
        path: "ad/async-solum-leget.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::SplitOut,
        bucket: "frontend semantic gap",
        reason_contains: "dynamic_receiver_method_type_args",
    },
    ExpectedTsOutcome {
        path: "ad/async-tempus-dormiet.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::SplitOut,
        bucket: "package HAL split-out",
        reason_contains: "norma:tempus",
    },
    ExpectedTsOutcome {
        path: "destructura/literal.fab",
        highest_tier: TsHighestTier::FrontendAnalyzed,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "json valor not supported",
        reason_contains: "json is not supported for the TypeScript target",
    },
    ExpectedTsOutcome {
        path: "itera/de.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "tabula index type",
        reason_contains: "Element implicitly has an 'any' type",
    },
    ExpectedTsOutcome {
        path: "json/json.fab",
        highest_tier: TsHighestTier::FrontendRejected,
        kind: ExpectedTsKind::SplitOut,
        bucket: "frontend semantic gap",
        reason_contains: "expression_type_mismatch",
    },
];

#[derive(Debug, Clone, Copy)]
struct TsToolchain {
    formatter: TsFormatter,
    linter: TsLinter,
    typechecker: TsTypechecker,
    runtime: TsRuntime,
}

#[derive(Debug, Clone, Copy)]
struct TsE2eCounts {
    total: usize,
    frontend_analyzed: usize,
    emitted: usize,
    formatted: usize,
    linted: usize,
    typecheck_valid: usize,
    runnable: usize,
    behavior_checked: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TsHighestTier {
    FrontendRejected,
    FrontendAnalyzed,
    TypeScriptEmitted,
    TypecheckValid,
    Runnable,
    RunPass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpectedTsKind {
    CompileFail,
    DeclarationOnly,
    RuntimeFailure,
    RuntimeBehavior,
    BehaviorFailure,
    TrackedGap,
    SplitOut,
}

#[derive(Debug, Clone, Copy)]
struct ExpectedTsOutcome {
    path: &'static str,
    highest_tier: TsHighestTier,
    kind: ExpectedTsKind,
    bucket: &'static str,
    reason_contains: &'static str,
}

#[derive(Debug, Clone, Copy)]
enum TsFormatter {
    Prettier,
    Deno,
    Missing,
}

#[derive(Debug, Clone, Copy)]
enum TsLinter {
    Biome,
    Eslint,
    Missing,
}

#[derive(Debug, Clone, Copy)]
enum TsTypechecker {
    Tsc,
    Deno,
    Missing,
}

#[derive(Debug, Clone, Copy)]
enum TsRuntime {
    NodeViaTsc,
    Deno,
    Missing,
}

struct TsModuleFile {
    relative_ts_path: PathBuf,
    code: String,
}

fn flush_progress() {
    let _ = std::io::Write::flush(&mut std::io::stderr());
}

#[test]
#[ignore = "slow ts e2e; run: cargo test -p exempla --test e2e_harness exempla_ts_e2e -- --ignored --nocapture"]
fn exempla_ts_e2e() {
    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);

    let toolchain = detect_ts_toolchain();
    let session = radix::driver::Session::new(Config::default().with_target(Target::TypeScript));
    let temp_root = make_temp_root();
    let total = exempla.len();
    let mut expected_count = 0usize;

    // Each exemplum is independent: its own case_dir, its own tiers, no shared
    // mutable state across exempla. Run them in parallel across available cores.
    // The Session and Compiler are not Sync by construction here, so the
    // frontend+codegen work happens in the main thread (serial, ~0ms each per
    // the Radix frontend measurements), and only the tier subprocess work
    // (tsc/node/deno — the actual cost) runs concurrently.
    //
    // A job is the per-exemplum inputs needed to run tiers: the emitted TS code
    // (or None if frontend/codegen failed) plus its case_dir and expected stdout.

    struct TsJob {
        idx: usize,
        file: PathBuf,
        relative: String,
        case_dir: PathBuf,
        code: Option<String>,
        // Tier states populated during frontend/codegen (serial phase).
        formatted: TierState,
        linted: TierState,
        typecheck_valid: TierState,
        runnable: TierState,
        behavior_checked: TierState,
        reason: String,
        frontend_analyzed: bool,
        typescript_emitted: bool,
        declaration_only: bool,
        expected: Option<String>,
    }

    // ---- Phase 1: serial frontend + codegen + tier-prep ---------------------
    eprintln!(
        "[ts-e2e] phase 1: frontend + codegen + format/lint over {total} exempla; temp root: {}",
        temp_root.display()
    );
    flush_progress();

    let mut jobs: Vec<TsJob> = Vec::with_capacity(exempla.len());
    for (idx, file) in exempla.iter().enumerate() {
        let relative = file
            .strip_prefix(&exempla_dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| file.display().to_string());

        let expected = read_expected_stdout(file);
        if expected.is_some() {
            expected_count += 1;
        }
        let expected_outcome = expected_ts_outcome(file, &exempla_dir);
        let declaration_only = matches!(
            expected_outcome.map(|outcome| outcome.kind),
            Some(ExpectedTsKind::DeclarationOnly)
        );

        let tc = std::time::Instant::now();
        let source = match fs::read_to_string(file) {
            Ok(source) => source,
            Err(err) => {
                eprintln!("[ts-e2e {idx:03}/{total}] {relative}  read-fail",);
                flush_progress();
                jobs.push(TsJob {
                    idx,
                    file: file.clone(),
                    relative,
                    case_dir: temp_root.clone(),
                    code: None,
                    formatted: TierState::Skipped,
                    linted: TierState::Skipped,
                    typecheck_valid: TierState::Skipped,
                    runnable: TierState::Skipped,
                    behavior_checked: TierState::Skipped,
                    reason: format!("cannot read source: {err}"),
                    frontend_analyzed: false,
                    typescript_emitted: false,
                    declaration_only,
                    expected,
                });
                continue;
            }
        };

        let analysis =
            match radix::driver::analyze_source(&session, &file.display().to_string(), &source) {
                Ok(analysis) => analysis,
                Err(diagnostics) => {
                    eprintln!("[ts-e2e {idx:03}/{total}] {relative}  frontend-fail",);
                    flush_progress();
                    jobs.push(TsJob {
                        idx,
                        file: file.clone(),
                        relative,
                        case_dir: temp_root.clone(),
                        code: None,
                        formatted: TierState::Skipped,
                        linted: TierState::Skipped,
                        typecheck_valid: TierState::Skipped,
                        runnable: TierState::Skipped,
                        behavior_checked: TierState::Skipped,
                        reason: format!(
                            "frontend failed: {}",
                            format_diagnostic_messages(&diagnostics)
                        ),
                        frontend_analyzed: false,
                        typescript_emitted: false,
                        declaration_only,
                        expected,
                    });
                    continue;
                }
            };

        let ts = match codegen::generate(
            Target::TypeScript,
            &analysis.hir,
            &analysis.types,
            &analysis.interner,
        ) {
            Ok(Output::TypeScript(output)) => output.code,
            Ok(_) => {
                eprintln!("[ts-e2e {idx:03}/{total}] {relative}  no-ts-output",);
                flush_progress();
                jobs.push(TsJob {
                    idx,
                    file: file.clone(),
                    relative,
                    case_dir: temp_root.clone(),
                    code: None,
                    formatted: TierState::Skipped,
                    linted: TierState::Skipped,
                    typecheck_valid: TierState::Skipped,
                    runnable: TierState::Skipped,
                    behavior_checked: TierState::Skipped,
                    reason: "compiler did not produce TypeScript output".to_owned(),
                    frontend_analyzed: true,
                    typescript_emitted: false,
                    declaration_only,
                    expected,
                });
                continue;
            }
            Err(err) => {
                eprintln!("[ts-e2e {idx:03}/{total}] {relative}  codegen-fail",);
                flush_progress();
                jobs.push(TsJob {
                    idx,
                    file: file.clone(),
                    relative,
                    case_dir: temp_root.clone(),
                    code: None,
                    formatted: TierState::Skipped,
                    linted: TierState::Skipped,
                    typecheck_valid: TierState::Skipped,
                    runnable: TierState::Skipped,
                    behavior_checked: TierState::Skipped,
                    reason: format!("TypeScript codegen failed: {err}"),
                    frontend_analyzed: true,
                    typescript_emitted: false,
                    declaration_only,
                    expected,
                });
                continue;
            }
        };

        // Format + lint tiers are in-process string transforms (or subprocesses),
        // but they don't depend on case_dir, so run them in phase 1.
        let (formatted, code, _format_reason) = run_ts_format_tier(&toolchain, &ts);
        let (linted, code, _lint_reason) = run_ts_lint_tier(&toolchain, &code);
        let modules = match compile_local_ts_modules(
            &session,
            file,
            &analysis.hir,
            &analysis.interner,
            &toolchain,
        ) {
            Ok(modules) => modules,
            Err(err) => {
                eprintln!("[ts-e2e {idx:03}/{total}] {relative}  module-fail",);
                flush_progress();
                jobs.push(TsJob {
                    idx,
                    file: file.clone(),
                    relative,
                    case_dir: temp_root.clone(),
                    code: None,
                    formatted,
                    linted,
                    typecheck_valid: TierState::Failed,
                    runnable: TierState::Skipped,
                    behavior_checked: TierState::Skipped,
                    reason: format!("cannot emit local TypeScript module graph: {err}"),
                    frontend_analyzed: true,
                    typescript_emitted: true,
                    declaration_only,
                    expected,
                });
                continue;
            }
        };

        let stem = file
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("exemplum");
        let case_dir = temp_root.join(format!("{idx:03}-{stem}"));
        match write_ts_case_files(&case_dir, &code, &modules) {
            Ok(()) => {
                eprintln!(
                    "[ts-e2e {idx:03}/{total}] {relative}  compiled+formatted+linted ({}ms)",
                    tc.elapsed().as_millis()
                );
                flush_progress();
                jobs.push(TsJob {
                    idx,
                    file: file.clone(),
                    relative,
                    case_dir,
                    code: Some(code),
                    formatted,
                    linted,
                    typecheck_valid: TierState::Skipped,
                    runnable: TierState::Skipped,
                    behavior_checked: TierState::Skipped,
                    reason: String::new(),
                    frontend_analyzed: true,
                    typescript_emitted: true,
                    declaration_only,
                    expected,
                });
            }
            Err(err) => {
                eprintln!("[ts-e2e {idx:03}/{total}] {relative}  write-fail",);
                flush_progress();
                jobs.push(TsJob {
                    idx,
                    file: file.clone(),
                    relative,
                    case_dir: temp_root.clone(),
                    code: None,
                    formatted,
                    linted,
                    typecheck_valid: TierState::Failed,
                    runnable: TierState::Skipped,
                    behavior_checked: TierState::Skipped,
                    reason: format!("cannot write TypeScript output: {err}"),
                    frontend_analyzed: true,
                    typescript_emitted: true,
                    declaration_only,
                    expected,
                });
            }
        }
    }

    // ---- Phase 2: parallel tier subprocess work -----------------------------
    // Only run typecheck + runtime tiers here for jobs that emitted TS and have a
    // case_dir. These are the expensive subprocess spawns (tsc + node).
    let tier_jobs: Vec<&TsJob> = jobs
        .iter()
        .filter(|j| j.typescript_emitted && j.code.is_some())
        .collect();
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    eprintln!(
        "[ts-e2e] phase 2: typecheck + run {} exempla across {workers} workers",
        tier_jobs.len()
    );
    flush_progress();
    let corpus_root = &exempla_dir;

    // Results indexed by job idx so we can fold them back into jobs in order.
    let tier_results: std::sync::Mutex<std::collections::HashMap<usize, TierOutcome>> =
        std::sync::Mutex::new(std::collections::HashMap::new());

    struct TierOutcome {
        typecheck_valid: TierState,
        runnable: TierState,
        behavior_checked: TierState,
        reason: String,
    }

    let next = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let next = &next;
            let tier_jobs = &tier_jobs;
            let tier_results = &tier_results;
            scope.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= tier_jobs.len() {
                    break;
                }
                let job = tier_jobs[i];

                let tr = std::time::Instant::now();
                let (typecheck_valid, typecheck_reason) =
                    run_ts_typecheck_tier(&toolchain, &job.case_dir);
                if typecheck_valid != TierState::Passed {
                    eprintln!(
                        "[ts-e2e {0:03}/{total}] {1}  typecheck-fail ({2}ms)",
                        job.idx,
                        job.relative,
                        tr.elapsed().as_millis()
                    );
                    flush_progress();
                    tier_results
                        .lock()
                        .expect("tier_results mutex poisoned")
                        .insert(
                            job.idx,
                            TierOutcome {
                                typecheck_valid,
                                runnable: TierState::Skipped,
                                behavior_checked: TierState::Skipped,
                                reason: typecheck_reason,
                            },
                        );
                    continue;
                }

                if job.declaration_only {
                    eprintln!(
                        "[ts-e2e {0:03}/{total}] {1}  declaration-only ({2}ms)",
                        job.idx,
                        job.relative,
                        tr.elapsed().as_millis()
                    );
                    flush_progress();
                    tier_results
                        .lock()
                        .expect("tier_results mutex poisoned")
                        .insert(
                            job.idx,
                            TierOutcome {
                                typecheck_valid,
                                runnable: TierState::Skipped,
                                behavior_checked: TierState::Skipped,
                                reason: "declaration-only fixture; runtime skipped".to_owned(),
                            },
                        );
                    continue;
                }

                let (runnable, stdout, run_reason) = run_ts_runtime_tier(&toolchain, &job.case_dir);
                if runnable != TierState::Passed {
                    let runtime_behavior_expected = expected_runtime_behavior(
                        expected_ts_outcome(&job.file, corpus_root),
                        &run_reason,
                    );
                    eprintln!(
                        "[ts-e2e {0:03}/{total}] {1}  {2} ({3}ms)",
                        job.idx,
                        job.relative,
                        if runtime_behavior_expected {
                            "expected-run-fail"
                        } else {
                            "run-fail"
                        },
                        tr.elapsed().as_millis()
                    );
                    flush_progress();
                    tier_results
                        .lock()
                        .expect("tier_results mutex poisoned")
                        .insert(
                            job.idx,
                            TierOutcome {
                                typecheck_valid,
                                runnable,
                                behavior_checked: if runtime_behavior_expected {
                                    TierState::Passed
                                } else {
                                    TierState::Skipped
                                },
                                reason: if runtime_behavior_expected {
                                    format!("expected runtime behavior: {run_reason}")
                                } else {
                                    run_reason
                                },
                            },
                        );
                    continue;
                }

                let (behavior_checked, behavior_reason) = match &job.expected {
                    Some(expected) if normalize_newline(&stdout) == *expected => {
                        (TierState::Passed, String::new())
                    }
                    Some(expected) => (
                        TierState::Failed,
                        format!(
                            "stdout mismatch: expected `{expected}`, got `{}`",
                            normalize_newline(&stdout)
                        ),
                    ),
                    None => (TierState::Skipped, "no sibling .expected file".to_owned()),
                };

                let label = if behavior_checked != TierState::Failed {
                    "ok"
                } else {
                    "behavior-fail"
                };
                eprintln!(
                    "[ts-e2e {0:03}/{total}] {1}  typecheck+run {2} ({3}ms)",
                    job.idx,
                    job.relative,
                    label,
                    tr.elapsed().as_millis()
                );
                flush_progress();
                tier_results
                    .lock()
                    .expect("tier_results mutex poisoned")
                    .insert(
                        job.idx,
                        TierOutcome {
                            typecheck_valid,
                            runnable,
                            behavior_checked,
                            reason: behavior_reason,
                        },
                    );
            });
        }
    });

    // Fold tier outcomes back into the ordered results.
    let mut results: Vec<TsE2eResult> = Vec::with_capacity(jobs.len());
    let tier_map = tier_results
        .into_inner()
        .expect("tier_results mutex poisoned");
    for job in jobs {
        let (typecheck_valid, runnable, behavior_checked, tier_reason) =
            if let Some(outcome) = tier_map.get(&job.idx) {
                (
                    outcome.typecheck_valid,
                    outcome.runnable,
                    outcome.behavior_checked,
                    outcome.reason.clone(),
                )
            } else if job.typescript_emitted && job.code.is_none() {
                // write-fail path already set Failed states
                (
                    job.typecheck_valid,
                    job.runnable,
                    job.behavior_checked,
                    job.reason.clone(),
                )
            } else {
                (
                    TierState::Skipped,
                    TierState::Skipped,
                    TierState::Skipped,
                    job.reason.clone(),
                )
            };

        // Match the original harness's reason string: it joined the format/lint
        // skip reasons into every non-clean line, so reconstruct them for the
        // reported reason. Those tiers are Skipped (not Failed) but their skip
        // messages were part of the diagnostic text the baseline report emits.
        // Only when TS was emitted did format/lint actually get a chance to run,
        // so skip reasons only apply on the emitted path (frontend/codegen
        // failures never reached those tiers and the original omits them).
        let (format_reason, lint_reason) = if job.typescript_emitted {
            (
                if matches!(job.formatted, TierState::Skipped) {
                    formatter_skip_reason(&toolchain)
                } else {
                    String::new()
                },
                if matches!(job.linted, TierState::Skipped) {
                    linter_skip_reason(&toolchain)
                } else {
                    String::new()
                },
            )
        } else {
            (String::new(), String::new())
        };
        let reason = join_reasons([format_reason, lint_reason, tier_reason]);

        results.push(TsE2eResult {
            path: job.file,
            frontend_analyzed: job.frontend_analyzed,
            typescript_emitted: job.typescript_emitted,
            formatted: job.formatted,
            linted: job.linted,
            typecheck_valid,
            runnable,
            behavior_checked,
            reason,
        });
    }

    print_ts_e2e_report(&results, &toolchain, expected_count, &exempla_dir);
}

fn detect_ts_toolchain() -> TsToolchain {
    let formatter = if command_available("prettier", &["--version"]) {
        TsFormatter::Prettier
    } else if command_available("deno", &["--version"]) {
        TsFormatter::Deno
    } else {
        TsFormatter::Missing
    };
    let linter = if command_available("biome", &["--version"]) {
        TsLinter::Biome
    } else if command_available("eslint", &["--version"]) {
        TsLinter::Eslint
    } else {
        TsLinter::Missing
    };
    let typechecker = if command_available("tsc", &["--version"]) {
        TsTypechecker::Tsc
    } else if command_available("deno", &["--version"]) {
        TsTypechecker::Deno
    } else {
        TsTypechecker::Missing
    };
    let runtime =
        if matches!(typechecker, TsTypechecker::Tsc) && command_available("node", &["--version"]) {
            TsRuntime::NodeViaTsc
        } else if command_available("deno", &["--version"]) {
            TsRuntime::Deno
        } else {
            TsRuntime::Missing
        };
    TsToolchain {
        formatter,
        linter,
        typechecker,
        runtime,
    }
}

fn run_ts_format_tier(toolchain: &TsToolchain, code: &str) -> (TierState, String, String) {
    match toolchain.formatter {
        TsFormatter::Missing => (
            TierState::Skipped,
            code.to_owned(),
            "formatted skipped: no prettier or deno".to_owned(),
        ),
        TsFormatter::Prettier | TsFormatter::Deno => {
            match radix::tool::format_generated_code(Target::TypeScript, code) {
                Ok(formatted) => (TierState::Passed, formatted, String::new()),
                Err(err) => (
                    TierState::Failed,
                    code.to_owned(),
                    format!("format failed: {err}"),
                ),
            }
        }
    }
}

fn run_ts_lint_tier(toolchain: &TsToolchain, code: &str) -> (TierState, String, String) {
    match toolchain.linter {
        TsLinter::Missing => (
            TierState::Skipped,
            code.to_owned(),
            "linted skipped: no biome or eslint".to_owned(),
        ),
        TsLinter::Biome | TsLinter::Eslint => {
            match radix::tool::lint_generated_code(Target::TypeScript, code) {
                Ok(fixed) => (TierState::Passed, fixed, String::new()),
                Err(err) => (
                    TierState::Failed,
                    code.to_owned(),
                    format!("lint failed: {err}"),
                ),
            }
        }
    }
}

struct LocalTsImport {
    source_path: PathBuf,
    relative_ts_path: PathBuf,
}

fn compile_local_ts_modules(
    session: &radix::driver::Session,
    file: &Path,
    hir: &HirProgram,
    interner: &Interner,
    toolchain: &TsToolchain,
) -> Result<Vec<TsModuleFile>, String> {
    let imports = local_ts_imports(file, hir, interner)?;
    let mut modules = Vec::new();
    for import in imports {
        if modules
            .iter()
            .any(|module: &TsModuleFile| module.relative_ts_path == import.relative_ts_path)
        {
            continue;
        }

        let source = fs::read_to_string(&import.source_path)
            .map_err(|err| format!("cannot read {}: {err}", import.source_path.display()))?;
        let mut analysis = radix::driver::analyze_source(
            session,
            &import.source_path.display().to_string(),
            &source,
        )
        .map_err(|diagnostics| format_diagnostic_messages(&diagnostics))?;
        let namespace = import
            .source_path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| {
                format!(
                    "cannot derive module namespace from {}",
                    import.source_path.display()
                )
            })?;
        let namespace = analysis.interner.intern(namespace);
        let output = codegen::ts::generate_module(
            &analysis.hir,
            &analysis.types,
            &analysis.interner,
            namespace,
        )
        .map_err(|err| err.to_string())?;
        let (formatted, code, format_reason) = run_ts_format_tier(toolchain, &output.code);
        if formatted == TierState::Failed {
            return Err(format_reason);
        }
        let (linted, code, lint_reason) = run_ts_lint_tier(toolchain, &code);
        if linted == TierState::Failed {
            return Err(lint_reason);
        }
        modules.push(TsModuleFile {
            relative_ts_path: import.relative_ts_path,
            code,
        });
    }
    Ok(modules)
}

fn local_ts_imports(
    file: &Path,
    hir: &HirProgram,
    interner: &Interner,
) -> Result<Vec<LocalTsImport>, String> {
    let mut imports = Vec::new();
    for item in &hir.items {
        let HirItemKind::Import(import) = &item.kind else {
            continue;
        };
        let specifier = interner.resolve(import.path);
        if !specifier.starts_with("./") {
            continue;
        }
        let relative_ts_path = local_ts_output_path(specifier)?;
        let mut source_path = file
            .parent()
            .ok_or_else(|| format!("source file has no parent: {}", file.display()))?
            .join(specifier);
        if source_path.extension().is_none() {
            source_path.set_extension("fab");
        }
        imports.push(LocalTsImport {
            source_path,
            relative_ts_path,
        });
    }
    Ok(imports)
}

fn local_ts_output_path(specifier: &str) -> Result<PathBuf, String> {
    let path = Path::new(specifier);
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => out.push(part),
            Component::ParentDir => {
                return Err(format!(
                    "parent-directory TypeScript import is not supported: {specifier}"
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(format!(
                    "absolute TypeScript import is not supported: {specifier}"
                ));
            }
        }
    }
    if out.as_os_str().is_empty() {
        return Err(format!("empty TypeScript import path: {specifier}"));
    }
    if out.extension().is_none() {
        out.set_extension("ts");
    }
    Ok(out)
}

fn write_ts_case_files(
    case_dir: &Path,
    main_code: &str,
    modules: &[TsModuleFile],
) -> std::io::Result<()> {
    fs::create_dir_all(case_dir)?;
    fs::write(case_dir.join("main.ts"), main_code)?;
    for module in modules {
        let path = case_dir.join(&module.relative_ts_path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(path, &module.code)?;
    }
    Ok(())
}

fn run_ts_typecheck_tier(toolchain: &TsToolchain, case_dir: &Path) -> (TierState, String) {
    match toolchain.typechecker {
        TsTypechecker::Missing => (
            TierState::Skipped,
            "typecheck skipped: no tsc or deno".to_owned(),
        ),
        TsTypechecker::Tsc => {
            let mut command = Command::new("tsc");
            command
                .args(TSC_SMOKE_ARGS)
                .arg("main.ts")
                .current_dir(case_dir);
            command_tier(command.output(), "tsc typecheck failed")
        }
        TsTypechecker::Deno => {
            let output = Command::new("deno")
                .args(["check", "main.ts"])
                .current_dir(case_dir)
                .output();
            command_tier(output, "deno check failed")
        }
    }
}

fn run_ts_runtime_tier(toolchain: &TsToolchain, case_dir: &Path) -> (TierState, String, String) {
    match toolchain.runtime {
        TsRuntime::Missing => (
            TierState::Skipped,
            String::new(),
            "runtime skipped: no node+tsc or deno".to_owned(),
        ),
        TsRuntime::NodeViaTsc => {
            let transpile = Command::new("tsc")
                .args(["--target", "ES2022", "--module", "commonjs", "main.ts"])
                .current_dir(case_dir)
                .output();
            let (state, reason) = command_tier(transpile, "tsc transpile failed");
            if state != TierState::Passed {
                return (state, String::new(), reason);
            }
            let run = Command::new("node")
                .arg("main.js")
                .current_dir(case_dir)
                .output();
            match run {
                Ok(run) if run.status.success() => (
                    TierState::Passed,
                    String::from_utf8_lossy(&run.stdout).to_string(),
                    String::new(),
                ),
                Ok(run) => (
                    TierState::Failed,
                    String::from_utf8_lossy(&run.stdout).to_string(),
                    format!("node run failed: {}", command_stderr(&run)),
                ),
                Err(err) => (
                    TierState::Failed,
                    String::new(),
                    format!("cannot execute node: {err}"),
                ),
            }
        }
        TsRuntime::Deno => {
            let run = Command::new("deno")
                .args(["run", "main.ts"])
                .current_dir(case_dir)
                .output();
            match run {
                Ok(run) if run.status.success() => (
                    TierState::Passed,
                    String::from_utf8_lossy(&run.stdout).to_string(),
                    String::new(),
                ),
                Ok(run) => (
                    TierState::Failed,
                    String::from_utf8_lossy(&run.stdout).to_string(),
                    format!("deno run failed: {}", command_stderr(&run)),
                ),
                Err(err) => (
                    TierState::Failed,
                    String::new(),
                    format!("cannot execute deno: {err}"),
                ),
            }
        }
    }
}

fn command_tier(
    output: Result<std::process::Output, std::io::Error>,
    failure_prefix: &str,
) -> (TierState, String) {
    match output {
        Ok(output) if output.status.success() => (TierState::Passed, String::new()),
        Ok(output) => (
            TierState::Failed,
            format!("{failure_prefix}: {}", command_stderr(&output)),
        ),
        Err(err) => (TierState::Failed, format!("cannot execute command: {err}")),
    }
}

/// Skip reason text used by the original harness for the format tier when no
/// tool is present. Mirrors run_ts_format_tier's Missing arm so reported reason
/// strings stay identical to the baseline.
fn formatter_skip_reason(toolchain: &TsToolchain) -> String {
    if matches!(toolchain.formatter, TsFormatter::Missing) {
        "formatted skipped: no prettier or deno".to_owned()
    } else {
        String::new()
    }
}

/// Skip reason text used by the original harness for the lint tier when no
/// tool is present. Mirrors run_ts_lint_tier's Missing arm.
fn linter_skip_reason(toolchain: &TsToolchain) -> String {
    if matches!(toolchain.linter, TsLinter::Missing) {
        "linted skipped: no biome or eslint".to_owned()
    } else {
        String::new()
    }
}

fn join_reasons<const N: usize>(reasons: [String; N]) -> String {
    reasons
        .into_iter()
        .filter(|reason| !reason.is_empty())
        .collect::<Vec<_>>()
        .join(" | ")
}

fn command_stderr(output: &std::process::Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    if stderr.is_empty() {
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    } else {
        stderr
    }
}

impl TsE2eResult {
    fn is_fully_clean(&self) -> bool {
        self.frontend_analyzed
            && self.typescript_emitted
            && !matches!(self.formatted, TierState::Failed)
            && !matches!(self.linted, TierState::Failed)
            && matches!(self.typecheck_valid, TierState::Passed)
            && matches!(self.runnable, TierState::Passed)
            && !matches!(self.behavior_checked, TierState::Failed)
    }
}

fn expected_ts_outcome(file: &Path, exempla_dir: &Path) -> Option<&'static ExpectedTsOutcome> {
    let relative = corpus_relative_path(file, exempla_dir);
    TS_EXPECTED_OUTCOMES
        .iter()
        .find(|expected| expected.path == relative)
}

fn corpus_relative_path(file: &Path, exempla_dir: &Path) -> String {
    file.strip_prefix(exempla_dir)
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| file.display().to_string())
}

fn expected_ts_outcome_matches(result: &TsE2eResult, expected: &ExpectedTsOutcome) -> bool {
    ts_highest_tier(result) == expected.highest_tier
        && result.reason.contains(expected.reason_contains)
}

fn expected_runtime_behavior(expected: Option<&ExpectedTsOutcome>, reason: &str) -> bool {
    matches!(
        expected,
        Some(ExpectedTsOutcome {
            kind: ExpectedTsKind::RuntimeBehavior,
            reason_contains,
            ..
        }) if reason.contains(reason_contains)
    )
}

fn is_stale_expected_ts_outcome(result: &TsE2eResult, expected: &ExpectedTsOutcome) -> bool {
    ts_highest_tier(result) > expected.highest_tier
}

fn ts_highest_tier(result: &TsE2eResult) -> TsHighestTier {
    if !result.frontend_analyzed {
        TsHighestTier::FrontendRejected
    } else if !result.typescript_emitted {
        TsHighestTier::FrontendAnalyzed
    } else if result.typecheck_valid != TierState::Passed {
        TsHighestTier::TypeScriptEmitted
    } else if result.runnable != TierState::Passed {
        TsHighestTier::TypecheckValid
    } else if result.behavior_checked == TierState::Failed {
        TsHighestTier::Runnable
    } else {
        TsHighestTier::RunPass
    }
}

fn ts_highest_tier_label(tier: TsHighestTier) -> &'static str {
    match tier {
        TsHighestTier::FrontendRejected => "frontend rejected",
        TsHighestTier::FrontendAnalyzed => "frontend analyzed",
        TsHighestTier::TypeScriptEmitted => "TypeScript emitted",
        TsHighestTier::TypecheckValid => "typecheck-valid",
        TsHighestTier::Runnable => "runnable",
        TsHighestTier::RunPass => "run pass",
    }
}

fn format_ts_result_paths(results: &[&TsE2eResult], exempla_dir: &Path) -> String {
    results
        .iter()
        .map(|result| corpus_relative_path(&result.path, exempla_dir))
        .collect::<Vec<_>>()
        .join(", ")
}

fn format_ts_stale_paths(
    stale: &[(&TsE2eResult, &ExpectedTsOutcome)],
    exempla_dir: &Path,
) -> String {
    stale
        .iter()
        .map(|(result, expected)| {
            format!(
                "{} ({} -> {})",
                corpus_relative_path(&result.path, exempla_dir),
                ts_highest_tier_label(expected.highest_tier),
                ts_highest_tier_label(ts_highest_tier(result))
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn print_ts_e2e_report(
    results: &[TsE2eResult],
    toolchain: &TsToolchain,
    expected_count: usize,
    exempla_dir: &Path,
) {
    let counts = ts_e2e_counts(results);
    let assessed = results
        .iter()
        .map(|result| (result, expected_ts_outcome(&result.path, exempla_dir)))
        .collect::<Vec<_>>();
    let accepted_expected = assessed
        .iter()
        .filter(|(result, expected)| {
            expected
                .map(|expected| expected_ts_outcome_matches(result, expected))
                .unwrap_or(false)
        })
        .count();
    let expected_runtime_behaviors = assessed
        .iter()
        .filter(|(result, expected)| expected_runtime_behavior(*expected, &result.reason))
        .count();
    let unexpected_failures = assessed
        .iter()
        .filter_map(|(result, expected)| {
            if result.is_fully_clean() {
                return None;
            }
            match expected {
                Some(expected)
                    if expected_ts_outcome_matches(result, expected)
                        || is_stale_expected_ts_outcome(result, expected) =>
                {
                    None
                }
                _ => Some(*result),
            }
        })
        .collect::<Vec<_>>();
    let stale_expected = assessed
        .iter()
        .filter_map(|(result, expected)| {
            expected
                .filter(|expected| is_stale_expected_ts_outcome(result, expected))
                .map(|expected| (*result, expected))
        })
        .collect::<Vec<_>>();
    let missing_expected = TS_EXPECTED_OUTCOMES
        .iter()
        .filter(|expected| {
            !results
                .iter()
                .any(|result| corpus_relative_path(&result.path, exempla_dir) == expected.path)
        })
        .collect::<Vec<_>>();

    eprintln!("TypeScript toolchain:");
    eprintln!("  formatter: {}", formatter_label(toolchain.formatter));
    eprintln!("  linter: {}", linter_label(toolchain.linter));
    eprintln!(
        "  typechecker: {}",
        typechecker_label(toolchain.typechecker)
    );
    eprintln!("  runtime: {}", runtime_label(toolchain.runtime));
    eprintln!("TypeScript e2e exempla:");
    eprintln!(
        "{}",
        format_tier_line(
            "frontend analyzed",
            counts.frontend_analyzed,
            counts.total,
            EXPECTED_TS_FRONTEND_ANALYZED_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "TypeScript emitted",
            counts.emitted,
            counts.total,
            EXPECTED_TS_EMITTED_FLOOR,
        )
    );
    eprintln!(
        "  formatted: {formatted}/{total} ({})",
        formatter_label(toolchain.formatter),
        formatted = counts.formatted,
        total = counts.total,
    );
    eprintln!(
        "  linted: {linted}/{total} ({})",
        linter_label(toolchain.linter),
        linted = counts.linted,
        total = counts.total,
    );
    eprintln!(
        "{}",
        format_tier_line(
            "typecheck-valid",
            counts.typecheck_valid,
            counts.total,
            EXPECTED_TS_TYPECHECK_VALID_FLOOR,
        )
    );
    eprintln!(
        "{}",
        format_tier_line(
            "runnable",
            counts.runnable,
            counts.total,
            EXPECTED_TS_RUNNABLE_FLOOR,
        )
    );
    eprintln!(
        "  behavior-checked: {}/{}",
        counts.behavior_checked, counts.total
    );
    eprintln!("  expected runtime behavior: {expected_runtime_behaviors}");
    eprintln!("Expected-output checks available for {expected_count} exempla files");
    eprintln!(
        "TypeScript e2e ledger: {accepted_expected}/{} expected outcomes accepted, {} unexpected, {} stale, {} missing",
        TS_EXPECTED_OUTCOMES.len(),
        unexpected_failures.len(),
        stale_expected.len(),
        missing_expected.len()
    );

    for fail in results.iter().filter(|result| {
        !result.is_fully_clean()
            && !expected_runtime_behavior(
                expected_ts_outcome(&result.path, exempla_dir),
                &result.reason,
            )
    }) {
        eprintln!("[ts] {} :: {}", fail.path.display(), fail.reason);
    }
    for fail in &unexpected_failures {
        eprintln!("[ts-unexpected] {} :: {}", fail.path.display(), fail.reason);
    }
    for (stale, expected) in &stale_expected {
        eprintln!(
            "[ts-stale] {} :: expected {} at {}, now {}",
            stale.path.display(),
            expected.bucket,
            ts_highest_tier_label(expected.highest_tier),
            ts_highest_tier_label(ts_highest_tier(stale))
        );
    }
    for expected in &missing_expected {
        eprintln!(
            "[ts-missing] {} :: expected metadata path missing from corpus",
            expected.path
        );
    }

    assert_ts_e2e_floor_counts(counts, *toolchain);
    assert!(
        unexpected_failures.is_empty(),
        "unexpected TypeScript e2e failures: {}",
        format_ts_result_paths(&unexpected_failures, exempla_dir)
    );
    assert!(
        stale_expected.is_empty(),
        "stale TypeScript e2e expected metadata: {}",
        format_ts_stale_paths(&stale_expected, exempla_dir)
    );
    assert!(
        missing_expected.is_empty(),
        "TypeScript e2e expected metadata paths are not in corpus: {}",
        missing_expected
            .iter()
            .map(|expected| expected.path)
            .collect::<Vec<_>>()
            .join(", ")
    );
}

fn ts_e2e_counts(results: &[TsE2eResult]) -> TsE2eCounts {
    TsE2eCounts {
        total: results.len(),
        frontend_analyzed: results
            .iter()
            .filter(|result| result.frontend_analyzed)
            .count(),
        emitted: results
            .iter()
            .filter(|result| result.typescript_emitted)
            .count(),
        formatted: results
            .iter()
            .filter(|result| result.formatted == TierState::Passed)
            .count(),
        linted: results
            .iter()
            .filter(|result| result.linted == TierState::Passed)
            .count(),
        typecheck_valid: results
            .iter()
            .filter(|result| result.typecheck_valid == TierState::Passed)
            .count(),
        runnable: results
            .iter()
            .filter(|result| result.runnable == TierState::Passed)
            .count(),
        behavior_checked: results
            .iter()
            .filter(|result| result.behavior_checked == TierState::Passed)
            .count(),
    }
}

fn assert_ts_e2e_floor_counts(counts: TsE2eCounts, toolchain: TsToolchain) {
    assert!(
        counts.frontend_analyzed >= EXPECTED_TS_FRONTEND_ANALYZED_FLOOR,
        "TypeScript e2e frontend-analyzed count regressed: {}/{} below floor {EXPECTED_TS_FRONTEND_ANALYZED_FLOOR}",
        counts.frontend_analyzed,
        counts.total,
    );
    assert!(
        counts.emitted >= EXPECTED_TS_EMITTED_FLOOR,
        "TypeScript e2e emitted count regressed: {}/{} below floor {EXPECTED_TS_EMITTED_FLOOR}",
        counts.emitted,
        counts.total,
    );
    if !matches!(toolchain.typechecker, TsTypechecker::Missing) {
        assert!(
            counts.typecheck_valid >= EXPECTED_TS_TYPECHECK_VALID_FLOOR,
            "TypeScript e2e typecheck-valid count regressed: {}/{} below floor {EXPECTED_TS_TYPECHECK_VALID_FLOOR}",
            counts.typecheck_valid,
            counts.total,
        );
    }
    if !matches!(toolchain.runtime, TsRuntime::Missing) {
        assert!(
            counts.runnable >= EXPECTED_TS_RUNNABLE_FLOOR,
            "TypeScript e2e runnable count regressed: {}/{} below floor {EXPECTED_TS_RUNNABLE_FLOOR}",
            counts.runnable,
            counts.total,
        );
    }
}

fn formatter_label(formatter: TsFormatter) -> &'static str {
    match formatter {
        TsFormatter::Prettier => "prettier --parser typescript",
        TsFormatter::Deno => "deno fmt --ext ts -",
        TsFormatter::Missing => "skipped: no prettier or deno",
    }
}

fn linter_label(linter: TsLinter) -> &'static str {
    match linter {
        TsLinter::Biome => "biome check",
        TsLinter::Eslint => "eslint",
        TsLinter::Missing => "skipped: no biome or eslint",
    }
}

fn typechecker_label(typechecker: TsTypechecker) -> &'static str {
    match typechecker {
        TsTypechecker::Tsc => "tsc --noEmit main.ts",
        TsTypechecker::Deno => "deno check main.ts",
        TsTypechecker::Missing => "skipped: no tsc or deno",
    }
}

fn runtime_label(runtime: TsRuntime) -> &'static str {
    match runtime {
        TsRuntime::NodeViaTsc => "tsc main.ts; node main.js",
        TsRuntime::Deno => "deno run main.ts",
        TsRuntime::Missing => "skipped: no node+tsc or deno",
    }
}

#[test]
fn ts_e2e_floor_constants_accept_current_baseline() {
    assert_ts_e2e_floor_counts(current_floor_counts(), full_ts_toolchain());
}

#[test]
fn ts_expected_outcome_matches_recorded_tier_and_reason() {
    let expected = ExpectedTsOutcome {
        path: "sample.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "sample",
        reason_contains: "error TS2355",
    };
    let result = ts_result_for_test(
        TsHighestTier::TypeScriptEmitted,
        "tsc typecheck failed: main.ts(1,1): error TS2355: missing return",
    );

    assert!(expected_ts_outcome_matches(&result, &expected));
    assert!(!is_stale_expected_ts_outcome(&result, &expected));
}

#[test]
fn ts_expected_runtime_behavior_requires_exact_error_contract() {
    let expected = ExpectedTsOutcome {
        path: "tensor/method-errors.fab",
        highest_tier: TsHighestTier::TypecheckValid,
        kind: ExpectedTsKind::RuntimeBehavior,
        bucket: "expected runtime error behavior",
        reason_contains: "tensor structa element count does not match shape",
    };

    assert!(expected_runtime_behavior(
        Some(&expected),
        "expected runtime behavior: node run failed: Error: tensor structa element count does not match shape",
    ));
    assert!(!expected_runtime_behavior(
        Some(&expected),
        "node run failed: Error: different failure",
    ));
}

#[test]
fn ts_expected_outcome_rejects_stale_higher_tier() {
    let expected = ExpectedTsOutcome {
        path: "sample.fab",
        highest_tier: TsHighestTier::TypeScriptEmitted,
        kind: ExpectedTsKind::TrackedGap,
        bucket: "sample",
        reason_contains: "error TS2355",
    };
    let result = ts_result_for_test(TsHighestTier::RunPass, "");

    assert!(!expected_ts_outcome_matches(&result, &expected));
    assert!(is_stale_expected_ts_outcome(&result, &expected));
}

#[test]
fn ts_expected_outcome_uses_corpus_relative_path() {
    let exempla_dir = Path::new("/tmp/exempla/corpus");
    let file = exempla_dir.join("gpu-core-types/atomic-element-reject.fab");

    let expected = expected_ts_outcome(&file, exempla_dir).expect("expected metadata row");

    assert_eq!(expected.path, "gpu-core-types/atomic-element-reject.fab");
    assert_eq!(expected.kind, ExpectedTsKind::CompileFail);
}

#[test]
fn ts_e2e_floor_constants_accept_missing_optional_tooling() {
    assert_ts_e2e_floor_counts(
        TsE2eCounts {
            typecheck_valid: 0,
            runnable: 0,
            ..current_floor_counts()
        },
        TsToolchain {
            formatter: TsFormatter::Missing,
            linter: TsLinter::Missing,
            typechecker: TsTypechecker::Missing,
            runtime: TsRuntime::Missing,
        },
    );
}

#[test]
#[should_panic(expected = "TypeScript e2e runnable count regressed")]
fn ts_e2e_floor_constants_reject_runnable_regression() {
    assert_ts_e2e_floor_counts(
        TsE2eCounts {
            total: 292,
            frontend_analyzed: EXPECTED_TS_FRONTEND_ANALYZED_FLOOR,
            emitted: EXPECTED_TS_EMITTED_FLOOR,
            formatted: 0,
            linted: 0,
            typecheck_valid: EXPECTED_TS_TYPECHECK_VALID_FLOOR,
            runnable: EXPECTED_TS_RUNNABLE_FLOOR - 1,
            behavior_checked: 241,
        },
        full_ts_toolchain(),
    );
}

fn current_floor_counts() -> TsE2eCounts {
    TsE2eCounts {
        total: 292,
        frontend_analyzed: EXPECTED_TS_FRONTEND_ANALYZED_FLOOR,
        emitted: EXPECTED_TS_EMITTED_FLOOR,
        formatted: 0,
        linted: 0,
        typecheck_valid: EXPECTED_TS_TYPECHECK_VALID_FLOOR,
        runnable: EXPECTED_TS_RUNNABLE_FLOOR,
        behavior_checked: 241,
    }
}

fn full_ts_toolchain() -> TsToolchain {
    TsToolchain {
        formatter: TsFormatter::Prettier,
        linter: TsLinter::Biome,
        typechecker: TsTypechecker::Tsc,
        runtime: TsRuntime::NodeViaTsc,
    }
}

fn ts_result_for_test(tier: TsHighestTier, reason: &str) -> TsE2eResult {
    TsE2eResult {
        path: PathBuf::from("sample.fab"),
        frontend_analyzed: tier >= TsHighestTier::FrontendAnalyzed,
        typescript_emitted: tier >= TsHighestTier::TypeScriptEmitted,
        formatted: TierState::Skipped,
        linted: TierState::Skipped,
        typecheck_valid: if tier >= TsHighestTier::TypecheckValid {
            TierState::Passed
        } else {
            TierState::Failed
        },
        runnable: if tier >= TsHighestTier::Runnable {
            TierState::Passed
        } else {
            TierState::Skipped
        },
        behavior_checked: if tier == TsHighestTier::RunPass {
            TierState::Passed
        } else if tier == TsHighestTier::Runnable {
            TierState::Failed
        } else {
            TierState::Skipped
        },
        reason: reason.to_owned(),
    }
}
