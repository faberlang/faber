use super::common::{
    cargo_available, collect_exempla_files, format_diagnostics, format_result_paths,
    make_temp_root, normalize_newline, read_expected_stdout, shared_target_dir,
    write_rust_workspace_member, write_rust_workspace_root,
};
use super::oracle::{rust_oracle, RustOracleOutcome};
use super::types::E2eResult;
use radix::{Compiler, Config, Output};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

// Executable-debt budget for rust e2e. KNOWN_FAILURES is the sole accounting
// mechanism: every listed path must fail, and every observed failure must be
// listed. Fixes remove rows and ratchet accepted/pass counts upward. Do not
// raise MAX_KNOWN_FAILURES to absorb drift — reclassify (oracle) or fix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KnownFailureKind {
    FixtureMismatch,
    BuildFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KnownFailure {
    path: &'static str,
    kind: KnownFailureKind,
}

// Ceiling held at 13 for historical budget; live rows shrink as debt clears.
const MAX_KNOWN_FAILURES: usize = 13;
const KNOWN_FAILURES: &[KnownFailure] = &[
    // sermo accipe/scrinium option double-wrap in rust codegen (E0308).
    KnownFailure {
        path: "ad/sermo-live-directional.fab",
        kind: KnownFailureKind::BuildFailure,
    },
];

/// Whether a path is a tracked known Rust build failure (codegen debt).
///
/// The LLVM host parity harness keeps such paths in the executable
/// denominator but records them as `outcome/rust_oracle_missing` instead of
/// panicking on the compile failure (see `llvm_host.rs`).
pub(super) fn is_known_rust_build_failure(path: &Path) -> bool {
    KNOWN_FAILURES.iter().any(|failure| {
        failure.kind == KnownFailureKind::BuildFailure && path.ends_with(failure.path)
    })
}

/// A compiled exemplum awaiting the batched workspace build and run.
struct ExemplumJob {
    idx: usize,
    file: PathBuf,
    relative: String,
    package_name: String,
    dir_rel: String,
    expected: Option<String>,
}

#[test]
#[ignore = "known breakage: instans corpus exemplum fails analysis (SEM010); see need e28474a2"]
fn instans_rust_codegen_preserves_valor_and_textus_error_paths() {
    let exempla_dir = crate::paths::corpus_dir();
    let file = exempla_dir.join("instans/instans.fab");
    let compiler = Compiler::new(Config::default());
    let code = compile_rust_exemplum(&compiler, &file, &exempla_dir).expect("instans Rust codegen");

    let exige = code
        .split("pub(crate) fn exige(")
        .nth(1)
        .and_then(|body| body.split("pub(crate) fn exige_claves(").next())
        .expect("generated exige helper");
    let exige_claves = code
        .split("pub(crate) fn exige_claves(")
        .nth(1)
        .expect("generated exige_claves helper");

    for (name, helper) in [("exige", exige), ("exige_claves", exige_claves)] {
        assert!(
            helper.contains("if child == faber::Valor::Nihil"),
            "{name} must compare Valor to Valor::Nihil:\n{helper}"
        );
        assert!(
            helper.contains("format!(\"missing key: {}\", clavis)"),
            "{name} must format String directly:\n{helper}"
        );
        assert!(
            !helper.contains("child.is_none()"),
            "{name} used Option API on Valor:\n{helper}"
        );
        assert!(
            !helper.contains("(clavis).clone().unwrap()"),
            "{name} unwrapped a String:\n{helper}"
        );
    }
}

#[test]
#[ignore = "slow rust e2e; run: cargo test -p exempla --test e2e_harness exempla_rust_e2e -- --ignored --nocapture"]
fn exempla_rust_e2e() {
    if !cargo_available() {
        eprintln!("cargo not found on PATH; skipping exempla end-to-end harness");
        return;
    }

    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);

    let compiler = Compiler::new(Config::default());
    let temp_root = make_temp_root();
    let shared_target = shared_target_dir(&temp_root);
    let total = exempla.len();
    let mut results: Vec<E2eResult> = Vec::with_capacity(exempla.len());
    let mut expected_count = 0usize;

    // The harness runs in three phases so cargo is invoked exactly once for the whole
    // corpus instead of once per exemplum:
    //   1. Faber-compile + format each exemplum and write it as a workspace member.
    //   2. A single `cargo build` at the workspace root compiles every member into the
    //      shared target dir, amortizing cargo's spawn/fingerprint overhead.
    //   3. Run each built binary and verify stdout / runtime-failure semantics.
    //
    // Per-exemplum progress streams in phases 1 and 3; phase 2 inherits cargo's own
    // `Compiling …` lines so the single build is also observable.

    // ---- Phase 1: compile + write members -------------------------------------
    eprintln!(
        "[rust-e2e] phase 1: compile + write {total} exempla; shared target: {}",
        shared_target.display()
    );
    flush_stderr();

    let mut jobs: Vec<ExemplumJob> = Vec::with_capacity(exempla.len());
    for (idx, file) in exempla.iter().enumerate() {
        let relative = file
            .strip_prefix(&exempla_dir)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| file.display().to_string());

        let expected = read_expected_stdout(file);
        if expected.is_some() {
            expected_count += 1;
        }

        let tc = std::time::Instant::now();
        let compiled = compile_rust_exemplum(&compiler, file, &exempla_dir).map(|code| {
            // Clippy --fix is intentionally omitted (see tool/commands/postprocess.rs):
            // it spun a second temp Cargo crate per exemplum. Canonicality is the
            // rust-canonical RC-003 tier, not an e2e correctness gate.
            radix::tool::format_generated_code(radix::codegen::Target::Rust, &code).unwrap_or(code)
        });
        let t_compile = tc.elapsed();

        match compiled {
            Ok(code) => {
                if expected_compile_failure(file).is_some() {
                    eprintln!(
                        "[rust-e2e {idx:03}/{total}] {relative}  compile={}ms  stale-expected-compile-fail",
                        t_compile.as_millis()
                    );
                    flush_stderr();
                    results.push(E2eResult {
                        path: file.clone(),
                        passed: false,
                        reason: "expected compile failure now compiles".to_owned(),
                    });
                    continue;
                }
                let stem = file
                    .file_stem()
                    .and_then(|n| n.to_str())
                    .unwrap_or("exemplum");
                let dir_rel = format!("{idx:03}-{stem}");
                let project_dir = temp_root.join(&dir_rel);
                let package_name = format!("exemplum-{idx:03}-{stem}").replace('-', "_");
                let member_code = rust_member_code(file, &code);
                write_rust_workspace_member(&project_dir, &package_name, &member_code);
                eprintln!(
                    "[rust-e2e {idx:03}/{total}] {relative}  compile={}ms  wrote",
                    t_compile.as_millis()
                );
                flush_stderr();
                jobs.push(ExemplumJob {
                    idx,
                    file: file.clone(),
                    relative,
                    package_name,
                    dir_rel,
                    expected,
                });
            }
            Err(reason) => {
                if let Some(expected) = expected_compile_failure(file) {
                    let passed = reason.contains(expected);
                    let reason = if passed {
                        format!("expected compile failure: {expected}")
                    } else {
                        format!("expected compile failure containing `{expected}`, got: {reason}")
                    };
                    eprintln!(
                        "[rust-e2e {idx:03}/{total}] {relative}  compile={}ms  expected-compile-fail",
                        t_compile.as_millis()
                    );
                    flush_stderr();
                    results.push(E2eResult {
                        path: file.clone(),
                        passed,
                        reason,
                    });
                    continue;
                }
                eprintln!(
                    "[rust-e2e {idx:03}/{total}] {relative}  compile={}ms  compile-fail",
                    t_compile.as_millis()
                );
                flush_stderr();
                results.push(E2eResult {
                    path: file.clone(),
                    passed: false,
                    reason,
                });
            }
        }
    }

    // ---- Phase 2: single batched workspace build ------------------------------
    let member_paths: Vec<String> = jobs.iter().map(|j| j.dir_rel.clone()).collect();
    let root_manifest = write_rust_workspace_root(&temp_root, &member_paths);
    eprintln!(
        "[rust-e2e] phase 2: single cargo build over {} members (stdout/stderr inherited)",
        jobs.len()
    );
    flush_stderr();

    let tb = std::time::Instant::now();
    let build_status = Command::new("cargo")
        .arg("build")
        .arg("--keep-going")
        .arg("--manifest-path")
        .arg(&root_manifest)
        .env("CARGO_TARGET_DIR", &shared_target)
        .status();
    let t_build = tb.elapsed();
    let build_ok = matches!(&build_status, Ok(s) if s.success());
    eprintln!(
        "[rust-e2e] build phase complete: {}ms (cargo exit success={build_ok}; non-zero is expected when some members fail to compile)",
        t_build.as_millis()
    );
    flush_stderr();

    // ---- Phase 3: parallel run + verify ---------------------------------------
    let jobs_len = jobs.len();
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    eprintln!("[rust-e2e] phase 3: run + verify {jobs_len} built exempla across {workers} workers");
    flush_stderr();

    let next = AtomicUsize::new(0);
    let run_results = std::sync::Mutex::new(Vec::with_capacity(jobs_len));

    std::thread::scope(|scope| {
        for _ in 0..workers {
            let next = &next;
            let jobs = &jobs;
            let shared_target = &shared_target;
            let run_results = &run_results;
            scope.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= jobs.len() {
                    break;
                }
                let job = &jobs[i];
                let bin_file = shared_target.join(format!("debug/{}", job.package_name));
                let tr = std::time::Instant::now();

                if !bin_file.exists() {
                    let result = E2eResult {
                        path: job.file.clone(),
                        passed: false,
                        reason: "cargo build did not produce binary".to_owned(),
                    };
                    eprintln!(
                        "[rust-e2e {idx:03}/{total}] {relative}  run=0ms  build-fail (no binary)",
                        idx = job.idx,
                        relative = job.relative,
                    );
                    flush_stderr();
                    push_run_result(run_results, result);
                    continue;
                }

                let run = match Command::new(&bin_file)
                    .args(rust_oracle(&job.file).run_args())
                    .output()
                {
                    Ok(run) => run,
                    Err(err) => {
                        let t_run = tr.elapsed();
                        let result = E2eResult {
                            path: job.file.clone(),
                            passed: false,
                            reason: format!("cannot run binary: {err}"),
                        };
                        eprintln!(
                            "[rust-e2e {idx:03}/{total}] {relative}  run={run_ms}ms  FAIL (spawn)",
                            idx = job.idx,
                            relative = job.relative,
                            run_ms = t_run.as_millis(),
                        );
                        flush_stderr();
                        push_run_result(run_results, result);
                        continue;
                    }
                };
                let t_run = tr.elapsed();

                let (passed, reason) = classify_run(&job.file, &run, job.expected.as_deref());
                let label = if passed { "OK" } else { "FAIL" };
                eprintln!(
                    "[rust-e2e {idx:03}/{total}] {relative}  run={run_ms}ms  {label}",
                    idx = job.idx,
                    relative = job.relative,
                    run_ms = t_run.as_millis(),
                );
                if !passed {
                    eprintln!("            reason: {reason}");
                }
                flush_stderr();
                push_run_result(
                    run_results,
                    E2eResult {
                        path: job.file.clone(),
                        passed,
                        reason,
                    },
                );
            });
        }
    });

    results.extend(
        run_results
            .into_inner()
            .expect("run_results mutex poisoned"),
    );

    // ---- Summary --------------------------------------------------------------
    let total = results.len();
    let accepted_count = results.iter().filter(|r| r.passed).count();
    let pass_count = results
        .iter()
        .filter(|r| r.passed && expected_compile_failure(&r.path).is_none())
        .count();
    let expected_compile_fail_count = results
        .iter()
        .filter(|r| r.passed && expected_compile_failure(&r.path).is_some())
        .count();
    let fail_count = total.saturating_sub(accepted_count);
    let run_success_count = results
        .iter()
        .filter(|result| {
            matches!(
                rust_oracle(&result.path),
                RustOracleOutcome::RunSuccess { .. }
            )
        })
        .count();
    let declaration_only_count = results
        .iter()
        .filter(|result| {
            matches!(
                rust_oracle(&result.path),
                RustOracleOutcome::DeclarationOnly { .. }
            )
        })
        .count();
    let runtime_failure_count = results
        .iter()
        .filter(|result| {
            matches!(
                rust_oracle(&result.path),
                RustOracleOutcome::ExpectedRuntimeFailure { .. }
            )
        })
        .count();
    let nonzero_exit_count = results
        .iter()
        .filter(|result| {
            matches!(
                rust_oracle(&result.path),
                RustOracleOutcome::ExpectedNonzeroExit { .. }
            )
        })
        .count();
    let compile_failure_count = results
        .iter()
        .filter(|result| {
            matches!(
                rust_oracle(&result.path),
                RustOracleOutcome::ExpectedCompileFailure { .. }
            )
        })
        .count();
    let wrong_lane_count = results
        .iter()
        .filter(|result| {
            matches!(
                rust_oracle(&result.path),
                RustOracleOutcome::ExplicitWrongLane { .. }
            )
        })
        .count();
    let executable_denominator = results
        .iter()
        .filter(|result| rust_oracle(&result.path).is_executable())
        .count();

    let observed_failures = results.iter().filter(|r| !r.passed).collect::<Vec<_>>();
    let failure_paths = observed_failures
        .iter()
        .map(|result| relative_exemplum_path(&result.path, &exempla_dir))
        .collect::<Vec<_>>();
    let known_failure_count = observed_failures
        .iter()
        .filter(|result| known_failure(&result.path, &exempla_dir).is_some())
        .count();
    let unknown_failures: Vec<&E2eResult> = observed_failures
        .iter()
        .filter(|result| known_failure(&result.path, &exempla_dir).is_none())
        .copied()
        .collect();
    let present_paths: Vec<String> = exempla
        .iter()
        .map(|path| relative_exemplum_path(path, &exempla_dir))
        .collect();
    // Known-failure rows for files not in this corpus (e.g. radix/corpus scaffold)
    // are not stale — only rows present in the corpus but no longer failing.
    let stale_known_failures: Vec<&str> = KNOWN_FAILURES
        .iter()
        .filter(|entry| {
            present_paths.iter().any(|path| path == entry.path)
                && !failure_paths.iter().any(|path| path == entry.path)
        })
        .map(|entry| entry.path)
        .collect();
    let applicable_known_failures = KNOWN_FAILURES
        .iter()
        .filter(|entry| present_paths.iter().any(|path| path == entry.path))
        .count();
    let known_fixture_drift_count = observed_failures
        .iter()
        .filter(|result| {
            matches!(
                known_failure(&result.path, &exempla_dir),
                Some(KnownFailure {
                    kind: KnownFailureKind::FixtureMismatch,
                    ..
                })
            )
        })
        .count();
    let known_build_failure_count = observed_failures
        .iter()
        .filter(|result| {
            matches!(
                known_failure(&result.path, &exempla_dir),
                Some(KnownFailure {
                    kind: KnownFailureKind::BuildFailure,
                    ..
                })
            )
        })
        .count();

    let summary = format!(
        "Rust e2e exempla: {accepted_count}/{total} accepted outcomes ({pass_count} run, {expected_compile_fail_count} expected compile failures)"
    );
    eprintln!("{summary}");
    println!("{summary}");
    eprintln!("Expected-output checks enabled for {expected_count} exempla files");
    eprintln!(
        "Rust oracle inventory: run-success={run_success_count}, declaration-only={declaration_only_count}, runtime-failure={runtime_failure_count}, nonzero-exit={nonzero_exit_count}, compile-failure={compile_failure_count}, wrong-lane={wrong_lane_count}, R={executable_denominator}"
    );
    eprintln!(
        "Failures: {fail_count} total, {known_failure_count} ledger-known, {} unlisted",
        unknown_failures.len()
    );
    eprintln!("pass: {pass_count}/{total}; accepted: {accepted_count}/{total}");
    eprintln!(
        "known failure ledger: {known_failure_count}/{applicable_known_failures} present in corpus (fixture drifts: {known_fixture_drift_count}, build failures: {known_build_failure_count}; full ledger {})",
        KNOWN_FAILURES.len()
    );

    for fail in results.iter().filter(|r| !r.passed) {
        eprintln!("[fail] {} :: {}", fail.path.display(), fail.reason);
    }

    if !unknown_failures.is_empty() {
        eprintln!(
            "unlisted failure list: {}",
            format_result_paths(&unknown_failures)
        );
    }

    assert!(
        KNOWN_FAILURES.len() <= MAX_KNOWN_FAILURES,
        "known failure ledger expanded: {}/{} rows",
        KNOWN_FAILURES.len(),
        MAX_KNOWN_FAILURES
    );
    assert!(
        unknown_failures.is_empty(),
        "unlisted Rust e2e failures: {}",
        format_result_paths(&unknown_failures)
    );
    assert!(
        stale_known_failures.is_empty(),
        "stale Rust e2e known-failure rows: {}",
        stale_known_failures.join(", ")
    );
    assert_eq!(
        known_failure_count, applicable_known_failures,
        "Rust e2e failure ledger does not match observed failures for files present in this corpus"
    );
}

fn relative_exemplum_path(path: &Path, exempla_dir: &Path) -> String {
    path.strip_prefix(exempla_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn known_failure(path: &Path, exempla_dir: &Path) -> Option<KnownFailure> {
    let relative = relative_exemplum_path(path, exempla_dir);
    KNOWN_FAILURES
        .iter()
        .find(|entry| entry.path == relative)
        .copied()
}

fn flush_stderr() {
    let _ = std::io::Write::flush(&mut std::io::stderr());
}

fn push_run_result(results: &std::sync::Mutex<Vec<E2eResult>>, result: E2eResult) {
    results
        .lock()
        .expect("run_results mutex poisoned")
        .push(result);
}

/// Classify a run's outcome against runtime-failure expectations and stdout fixtures.
///
/// Encapsulates the pass/fail semantics previously inlined in the loop so the three
/// phase bodies stay readable. Returns `(passed, reason)`.
fn classify_run(
    file: &Path,
    run: &std::process::Output,
    expected_stdout: Option<&str>,
) -> (bool, String) {
    let oracle = rust_oracle(file);
    if let RustOracleOutcome::ExpectedNonzeroExit { exit_code, .. } = oracle {
        if run.status.code() != Some(exit_code) {
            return (
                false,
                format!(
                    "expected exit code {exit_code}, got {:?}: {}",
                    run.status.code(),
                    String::from_utf8_lossy(&run.stderr).trim()
                ),
            );
        }
    } else if !run.status.success() {
        let stderr = String::from_utf8_lossy(&run.stderr).trim().to_owned();
        if let RustOracleOutcome::ExpectedRuntimeFailure {
            stderr_contains, ..
        } = oracle
        {
            if stderr.contains(stderr_contains) {
                return (true, format!("expected runtime failure: {stderr_contains}"));
            }
        }
        return (false, format!("binary failed: {stderr}"));
    }

    if !run.status.success() {
        if let RustOracleOutcome::ExpectedRuntimeFailure {
            stderr_contains, ..
        } = oracle
        {
            return (
                false,
                format!("expected runtime failure containing `{stderr_contains}`, but binary exit was expected"),
            );
        }
    }
    if let RustOracleOutcome::ExpectedRuntimeFailure {
        stderr_contains, ..
    } = oracle
    {
        return (
            false,
            format!(
                "expected runtime failure containing `{stderr_contains}`, but binary succeeded"
            ),
        );
    }

    if let Some(expected) = expected_stdout {
        let stdout = normalize_newline(&String::from_utf8_lossy(&run.stdout));
        if stdout != expected {
            return (
                false,
                format!("stdout mismatch: expected `{expected}`, got `{stdout}`"),
            );
        }
    }

    (true, String::new())
}

pub(super) fn compile_rust_exemplum(
    compiler: &Compiler,
    file: &Path,
    exempla_dir: &Path,
) -> Result<String, String> {
    let import_paths = parsed_import_paths(file)?;
    reject_package_forbidden_kernel_import(&import_paths)?;
    if uses_package_library_import(&import_paths) {
        return compile_package_library_exemplum(file);
    }

    // importa/importa.fab imports a sibling auxilium.fab; single-file CLI emit
    // cannot resolve the relative sibling (the base path is the process CWD),
    // so compile the entry + sibling as one package unit. Restored from the
    // pre-3c72cfb harness special case (see task e69dec39).
    if file
        .strip_prefix(exempla_dir)
        .is_ok_and(|relative| relative == Path::new("importa/importa.fab"))
    {
        return compile_importa_package_exemplum(compiler, file);
    }

    // Prefer the CLI compile path so relative `importa ex "./…"` siblings get an
    // import contract (namespace + file interfaces) and emit a linked module —
    // plain `Compiler::compile` leaves a bare `use crate::auxilium::…` with no
    // module body.
    let _ = compiler;
    let result = radix::tool::compile_cli_path(file, false, radix::codegen::Target::Rust);
    match result.output {
        Some(Output::Rust(output)) => Ok(output.code),
        Some(_) => Err("compiler did not produce Rust output".to_owned()),
        None => {
            let diagnostics = format_diagnostics(&result);
            Err(format!("compile failed: {diagnostics}"))
        }
    }
}

fn parsed_import_paths(file: &Path) -> Result<Vec<String>, String> {
    let source = fs::read_to_string(file)
        .map_err(|err| format!("cannot read exemplum {}: {err}", file.display()))?;
    let name = file.display().to_string();
    let peeled = match radix::driver::peel_raw_source(&name, &source) {
        Ok(peeled) => peeled,
        Err(_) => return Ok(Vec::new()),
    };
    let lex_result = radix::lexer::lex(peeled.body);
    if !lex_result.success() {
        return Ok(Vec::new());
    }
    let parse_result = radix::parser::parse(lex_result);
    if !parse_result.success() {
        return Ok(Vec::new());
    }

    let radix::parser::ParseResult {
        program, interner, ..
    } = parse_result;
    let Some(program) = program else {
        return Ok(Vec::new());
    };
    let mut paths = Vec::new();
    for stmt in &program.statements {
        let radix::syntax::StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        let import_path = interner.resolve(decl.path);
        paths.push(import_path.to_owned());
    }

    Ok(paths)
}

pub(super) fn uses_package_library_import_for(file: &Path) -> Result<bool, String> {
    parsed_import_paths(file).map(|paths| uses_package_library_import(&paths))
}

fn reject_package_forbidden_kernel_import(import_paths: &[String]) -> Result<(), String> {
    for import_path in import_paths {
        if radix::kernel::is_kernel_import_path(import_path) {
            return Err(format!(
                "compile failed: kernel_import_script_mode_only (`{import_path}`)"
            ));
        }
    }

    Ok(())
}

fn uses_package_library_import(import_paths: &[String]) -> bool {
    import_paths.iter().any(|path| path.starts_with("norma:"))
}

fn compile_package_library_exemplum(file: &Path) -> Result<String, String> {
    let config = radix::driver::Config::default().with_target(radix::codegen::Target::Rust);
    let result = faber_cli::package::compile_package(&config, file);
    match result.output {
        Some(Output::Rust(output)) => Ok(output.code),
        Some(_) => Err("package compiler did not produce Rust output".to_owned()),
        None => {
            let diagnostics = format_diagnostics(&result);
            Err(format!("compile failed: {diagnostics}"))
        }
    }
}

/// Package-compile `importa/importa.fab` with its sibling `auxilium.fab`.
///
/// Single-file CLI emit resolves relative `importa ex "./…"` siblings against
/// the process CWD and requires the import contract to type the imported call,
/// so the CLI compile runs from the entry's own directory (restored after).
/// This special case replaces the pre-3c72cfb package-codegen path, which the
/// current radix codegen no longer types correctly without the import contract.
fn compile_importa_package_exemplum(_compiler: &Compiler, entry: &Path) -> Result<String, String> {
    let previous_cwd = std::env::current_dir()
        .map_err(|err| format!("cannot read working directory for importa compile: {err}"))?;
    let entry_dir = entry
        .parent()
        .ok_or_else(|| "package entry has no parent directory".to_owned())?;
    std::env::set_current_dir(entry_dir).map_err(|err| {
        format!(
            "cannot enter {} for importa compile: {err}",
            entry_dir.display()
        )
    })?;
    let result = radix::tool::compile_cli_path(entry, false, radix::codegen::Target::Rust);
    std::env::set_current_dir(&previous_cwd)
        .map_err(|err| format!("cannot restore working directory after importa compile: {err}"))?;
    match result.output {
        Some(Output::Rust(output)) => Ok(output.code),
        Some(_) => Err("compiler did not produce Rust output".to_owned()),
        None => {
            let diagnostics = format_diagnostics(&result);
            Err(format!("compile failed: {diagnostics}"))
        }
    }
}

fn expected_compile_failure(path: &Path) -> Option<&'static str> {
    rust_oracle(path).expected_compile_issue()
}

pub(super) fn rust_member_code(path: &Path, code: &str) -> String {
    if matches!(rust_oracle(path), RustOracleOutcome::DeclarationOnly { .. }) {
        format!("{code}\n\nfn main() {{}}\n")
    } else {
        code.to_owned()
    }
}

#[test]
fn rust_expected_failure_ledgers_are_disjoint() {
    for file in collect_exempla_files(&crate::paths::corpus_dir()) {
        let oracle = rust_oracle(&file);
        assert_eq!(
            oracle.is_executable(),
            oracle.expected_compile_issue().is_none(),
            "{} has an ambiguous Rust oracle classification",
            file.display()
        );
    }
}
