use super::common::{
    collect_exempla_files, expected_runtime_failure, format_ceiling_line, format_diagnostics,
    format_result_paths, format_tier_line, is_expected_failure, make_temp_root, normalize_newline,
    read_expected_stdout,
};
use super::types::E2eResult;
use radix::{codegen::Target, tool::compile_cli_path, Output};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

const GO_EXPECTED_FAILURES: &[&str] = &[
    "ad/async-solum-leget.fab",
    "ad/solum-lege-generic.fab",
    "conversio/collectiones.fab",
    "conversio/lista-tensor-shaped.fab",
    "conversio/rectangular-lista-literal-tensor.fab",
    "conversio/tensor.fab",
    "conversio/valor-boxing.fab",
    "conversio/valor-tensor.fab",
    "gpu-core-types/matrix-register.fab",
    "instans/instans.fab",
    "intervallum/algebra.fab",
    "intervallum/conversio.fab",
    "sparsa/conversio.fab",
    "ad/async-tempus-dormiet.fab",
    "tensor/bracket-access.fab",
    "tensor/method-errors.fab",
    "vector/builtins.fab",
    "vector/cross.fab",
    "vector/decl.fab",
    "vector/dot.fab",
    "vector/elementwise.fab",
    "vector/infer.fab",
    "vector/sugar.fab",
    "vector/swizzle.fab",
];
const GO_EXPECTED_RUNTIME_FAILURES: &[(&str, &str)] =
    &[("operatores/numerus-overflow.fab", "panic: numerus overflow")];

const GO_EXPECTED_COMPILE_FAILURES: &[(&str, &str)] = &[
    (
        "annotation-sugar/cli-braced.fab",
        "go_cli_options_unsupported",
    ),
    (
        "annotation-sugar/optio-braced.fab",
        "go_cli_options_unsupported",
    ),
    ("cli/cli.fab", "go_cli_subcommand_unsupported"),
    ("gpu-core-types/atomic-element-reject.fab", "atomic_element"),
    (
        "gpu-core-types/atomic-operations.fab",
        "go_atomic_types_unsupported",
    ),
    ("gpu-core-types/f16-bf16-reject.fab", "unknown_type"),
    ("gpu-core-types/f16-width.fab", "go_type_unsupported"),
    ("conversio/valor-genus.fab", "go_json_unsupported"),
    ("destructura/literal.fab", "go_json_unsupported"),
    ("json/json.fab", "go_json_unsupported"),
    (
        "gpu-core-types/matrix-tensor-reject.fab",
        "expression_type_mismatch",
    ),
    ("optio/optio.fab", "go_cli_options_unsupported"),
    ("protecta/protecta.fab", "protecta_reserved"),
    ("rumpe/rumpe-top-level-error.fab", "break_outside_breakable"),
    ("sparsa/conversio-reject.fab", "expression_type_mismatch"),
    (
        "sparsa/non-numeric-reject.fab",
        "sparsa_element_non_numeric",
    ),
    ("tensor/arithmetic-reject.fab", "expression_type_mismatch"),
    ("typi/sized-family-error.fab", "float_width_on_numerus"),
    ("ubique/ubique.fab", "go_cli_options_unsupported"),
];
const GO_DECLARATION_ONLY_FIXTURES: &[&str] = &[
    "curata/curata.fab",
    "errata/errata.fab",
    "fragilis/fragilis.fab",
    "futurum/futurum.fab",
    "immutata/immutata.fab",
    "meta/requirit.fab",
    "numquam/numquam.fab",
    "omitte/omitte.fab",
    "optiones/optiones.fab",
    "postpara/postpara.fab",
    "postparabit/postparabit.fab",
    "prae/prae.fab",
    "praepara/praepara.fab",
    "praeparabit/praeparabit.fab",
    "proba/proba.fab",
    "probandum/probandum.fab",
    "repete/repete.fab",
    "scalaria/scalaria.fab",
    "solum-in/solum-in.fab",
    "solum/solum.fab",
    "sponte/sponte.fab",
    "tag/tag.fab",
    "temporis/temporis.fab",
    "vector/kernel.fab",
];
const EXPECTED_GO_PASS_FLOOR: usize = 249;
const EXPECTED_GO_ACCEPTED_OUTCOME_FLOOR: usize = 292;
// WHY: Remaining expected failures are tracked Go lowering gaps with
// per-path reopen contracts in docs/factory/go-e2e-failures-matrix/baseline.md.
const EXPECTED_GO_EXPECTED_FAILURE_CEILING: usize = 51;

/// A compiled exemplum awaiting the parallel run phase.
struct GoJob {
    file: PathBuf,
    relative: String,
    run_dir: PathBuf,
    expected: Option<String>,
    run_args: &'static [&'static str],
    expected_exit_code: Option<i32>,
}

#[test]
#[ignore = "slow go e2e; run: cargo test -p exempla --test e2e_harness exempla_go_e2e -- --ignored --nocapture"]
fn exempla_go_e2e() {
    if !super::common::go_available() {
        eprintln!("go not found on PATH; skipping Go exempla end-to-end harness");
        return;
    }

    let exempla_dir = crate::paths::corpus_dir();
    let exempla = collect_exempla_files(&exempla_dir);

    let temp_root = make_temp_root();
    let total = exempla.len();
    let mut results: Vec<E2eResult> = Vec::with_capacity(exempla.len());
    let mut expected_count = 0usize;

    // The harness runs in two phases so the run cost (one `go run` spawn per
    // exemplum) is paid concurrently rather than serially:
    //   1. Faber-compile + gofmt + write each exemplum to a unique .go file.
    //   2. Run every .go file in parallel across available cores.
    //
    // `go vet` is intentionally omitted: it was a best-effort diagnostic whose
    // findings were reported separately and never affected pass/fail (mirrors the
    // Rust harness dropping clippy from the correctness loop). Removing it halves
    // the per-exemplum spawn count.

    // ---- Phase 1: compile + write (serial; Faber compile is ~0ms) -----------
    eprintln!(
        "[go-e2e] phase 1: compile + write {total} exempla; temp root: {}",
        temp_root.display()
    );
    let _ = std::io::Write::flush(&mut std::io::stderr());

    let mut jobs: Vec<GoJob> = Vec::with_capacity(exempla.len());
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
        let compiled = compile_go_exemplum(file);
        let t_compile = tc.elapsed();

        let code = match compiled {
            Ok(code) => {
                if expected_compile_failure(file).is_some() {
                    eprintln!(
                        "[go-e2e {idx:03}/{total}] {relative}  compile={}ms  stale-expected-compile-fail",
                        t_compile.as_millis()
                    );
                    let _ = std::io::Write::flush(&mut std::io::stderr());
                    results.push(E2eResult {
                        path: file.clone(),
                        passed: false,
                        reason: "expected compile failure now compiles".to_owned(),
                    });
                    continue;
                }
                go_source_for_run(file, &code)
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
                        "[go-e2e {idx:03}/{total}] {relative}  compile={}ms  expected-compile-fail",
                        t_compile.as_millis()
                    );
                    let _ = std::io::Write::flush(&mut std::io::stderr());
                    results.push(E2eResult {
                        path: file.clone(),
                        passed,
                        reason,
                    });
                    continue;
                }
                eprintln!(
                    "[go-e2e {idx:03}/{total}] {relative}  compile={}ms  compile-fail",
                    t_compile.as_millis()
                );
                let _ = std::io::Write::flush(&mut std::io::stderr());
                results.push(E2eResult {
                    path: file.clone(),
                    passed: false,
                    reason,
                });
                continue;
            }
        };

        let gofmt = radix::tool::format_generated_code(radix::codegen::Target::Go, &code);
        let code = match gofmt {
            Ok(code) => code,
            Err(err) => {
                eprintln!(
                    "[go-e2e {idx:03}/{total}] {relative}  compile={}ms  gofmt-fail",
                    t_compile.as_millis()
                );
                let _ = std::io::Write::flush(&mut std::io::stderr());
                results.push(E2eResult {
                    path: file.clone(),
                    passed: false,
                    reason: format!("gofmt failed: {err}"),
                });
                continue;
            }
        };

        // Unique directory per exemplum so phase 2 can run module-shaped
        // fixtures concurrently without clobbering each other.
        let stem = file
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("exemplum");
        let run_dir = temp_root.join(format!("{idx:03}-{stem}"));
        if let Err(err) = write_go_job(file, &run_dir, &code) {
            eprintln!(
                "[go-e2e {idx:03}/{total}] {relative}  compile={}ms  write-fail",
                t_compile.as_millis()
            );
            let _ = std::io::Write::flush(&mut std::io::stderr());
            results.push(E2eResult {
                path: file.clone(),
                passed: false,
                reason: format!("cannot write go output: {err}"),
            });
            continue;
        }

        eprintln!(
            "[go-e2e {idx:03}/{total}] {relative}  compile={}ms  wrote",
            t_compile.as_millis()
        );
        let _ = std::io::Write::flush(&mut std::io::stderr());
        jobs.push(GoJob {
            file: file.clone(),
            relative,
            run_dir,
            expected,
            run_args: go_run_args(file),
            expected_exit_code: go_expected_exit_code(file),
        });
    }

    // ---- Phase 2: parallel run ----------------------------------------------
    let jobs_len = jobs.len();
    let workers = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    eprintln!("[go-e2e] phase 2: run {jobs_len} exempla across {workers} workers");
    let _ = std::io::Write::flush(&mut std::io::stderr());

    let next = AtomicUsize::new(0);
    let run_results: std::sync::Mutex<Vec<E2eResult>> =
        std::sync::Mutex::new(Vec::with_capacity(jobs_len));

    std::thread::scope(|scope| {
        for _ in 0..workers {
            // Move shared refs into each worker; eprintln! is thread-safe.
            let next = &next;
            let jobs = &jobs;
            let run_results = &run_results;
            scope.spawn(move || loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                if i >= jobs.len() {
                    break;
                }
                let job = &jobs[i];

                let tr = std::time::Instant::now();
                let go_run = Command::new("go")
                    .arg("run")
                    .arg(".")
                    .args(job.run_args)
                    .current_dir(&job.run_dir)
                    .output();
                let t_run = tr.elapsed();

                let result = match go_run {
                    Ok(go_run)
                        if go_run.status.success()
                            || job
                                .expected_exit_code
                                .is_some_and(|code| go_run.status.code() == Some(code)) =>
                    {
                        if let Some(expected_runtime_failure) =
                            expected_runtime_failure(&job.file, GO_EXPECTED_RUNTIME_FAILURES)
                        {
                            E2eResult {
                                path: job.file.clone(),
                                passed: false,
                                reason: format!(
                                    "expected runtime failure `{expected_runtime_failure}`, but Go run succeeded"
                                ),
                            }
                        } else {
                            let stdout = normalize_newline(&String::from_utf8_lossy(&go_run.stdout));
                            if let Some(expected) = &job.expected {
                                if stdout != *expected {
                                    E2eResult {
                                        path: job.file.clone(),
                                        passed: false,
                                        reason: format!(
                                            "stdout mismatch: expected `{expected}`, got `{stdout}`"
                                        ),
                                    }
                                } else {
                                    E2eResult {
                                        path: job.file.clone(),
                                        passed: true,
                                        reason: String::new(),
                                    }
                                }
                            } else {
                                E2eResult {
                                    path: job.file.clone(),
                                    passed: true,
                                    reason: String::new(),
                                }
                            }
                        }
                    }
                    Ok(go_run) => {
                        let stderr = String::from_utf8_lossy(&go_run.stderr).trim().to_owned();
                        let expected_runtime_failure =
                            expected_runtime_failure(&job.file, GO_EXPECTED_RUNTIME_FAILURES);
                        let passed = expected_runtime_failure
                            .is_some_and(|expected| stderr.contains(expected));
                        E2eResult {
                            path: job.file.clone(),
                            passed,
                            reason: if passed {
                                format!(
                                    "expected runtime failure: {}",
                                    expected_runtime_failure.expect("checked above")
                                )
                            } else {
                                format!("go run failed: {stderr}")
                            },
                        }
                    }
                    Err(err) => E2eResult {
                        path: job.file.clone(),
                        passed: false,
                        reason: format!("cannot execute go: {err}"),
                    },
                };

                let label = if result.passed { "OK" } else { "FAIL" };
                eprintln!(
                    "[go-e2e run] {}  run={}ms  {label}",
                    job.relative,
                    t_run.as_millis()
                );
                let _ = std::io::Write::flush(&mut std::io::stderr());
                run_results
                    .lock()
                    .expect("run_results mutex poisoned")
                    .push(result);
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
    let accepted_count = results
        .iter()
        .filter(|r| r.passed || is_expected_failure(&r.path, GO_EXPECTED_FAILURES))
        .count();
    let pass_count = results
        .iter()
        .filter(|r| r.passed && expected_compile_failure(&r.path).is_none())
        .count();
    let expected_compile_fail_count = results
        .iter()
        .filter(|r| r.passed && expected_compile_failure(&r.path).is_some())
        .count();
    let unaccepted_count = total.saturating_sub(accepted_count);
    let unexpected_failures = results
        .iter()
        .filter(|r| !r.passed && !is_expected_failure(&r.path, GO_EXPECTED_FAILURES))
        .collect::<Vec<_>>();
    let unexpected_passes = results
        .iter()
        .filter(|r| r.passed && is_expected_failure(&r.path, GO_EXPECTED_FAILURES))
        .collect::<Vec<_>>();
    eprintln!(
        "Go e2e exempla: {accepted_count}/{total} accepted outcomes ({pass_count} run, {expected_compile_fail_count} expected compile failures)"
    );
    eprintln!("Expected-output checks enabled for {expected_count} exempla files");
    eprintln!(
        "Unaccepted failures: {unaccepted_count} total, {} unexpected",
        unexpected_failures.len()
    );
    eprintln!(
        "{}",
        format_tier_line("pass", pass_count, total, EXPECTED_GO_PASS_FLOOR)
    );
    eprintln!(
        "{}",
        format_tier_line(
            "accepted",
            accepted_count,
            total,
            EXPECTED_GO_ACCEPTED_OUTCOME_FLOOR
        )
    );
    eprintln!(
        "{}",
        format_ceiling_line(
            "expected failures",
            GO_EXPECTED_FAILURES.len(),
            EXPECTED_GO_EXPECTED_FAILURE_CEILING,
        )
    );

    for result in results.iter().filter(|r| !r.passed) {
        let label = if is_expected_failure(&result.path, GO_EXPECTED_FAILURES) {
            "tracked"
        } else {
            "fail"
        };
        eprintln!("[{label}] {} :: {}", result.path.display(), result.reason);
    }

    assert!(
        pass_count >= EXPECTED_GO_PASS_FLOOR,
        "Go e2e pass count regressed: {pass_count}/{} below floor {EXPECTED_GO_PASS_FLOOR}",
        total,
    );
    assert!(
        accepted_count >= EXPECTED_GO_ACCEPTED_OUTCOME_FLOOR,
        "Go e2e accepted outcomes regressed: {accepted_count}/{total} below floor {EXPECTED_GO_ACCEPTED_OUTCOME_FLOOR}",
    );
    assert!(
        GO_EXPECTED_FAILURES.len() <= EXPECTED_GO_EXPECTED_FAILURE_CEILING,
        "Go e2e expected-failure metadata grew: {} above ceiling {EXPECTED_GO_EXPECTED_FAILURE_CEILING}",
        GO_EXPECTED_FAILURES.len(),
    );
    assert!(
        unexpected_failures.is_empty(),
        "unexpected Go e2e failures: {}",
        format_result_paths(&unexpected_failures)
    );
    assert!(
        unexpected_passes.is_empty(),
        "Go e2e expected failures now pass and should be removed from metadata: {}",
        format_result_paths(&unexpected_passes)
    );
}

fn compile_go_exemplum(file: &Path) -> Result<String, String> {
    let import_paths = parsed_import_paths(file)?;
    reject_package_forbidden_kernel_import(&import_paths)?;

    // WHY: `Compiler::compile` is the source-only API and intentionally does
    // not discover imported file interfaces. Go Tier-1 corpus fixtures are
    // path-backed source programs, so use the canonical single-file tool path
    // that resolves `norma:*` and installs typed HIR import contracts.
    let result = compile_cli_path(file, false, Target::Go);
    match result.output {
        Some(Output::Go(output)) => Ok(output.code),
        Some(_) => Err("compiler did not produce Go output".to_owned()),
        None => {
            let diagnostics = format_diagnostics(&result);
            Err(format!("compile failed: {diagnostics}"))
        }
    }
}

fn write_go_job(file: &Path, run_dir: &Path, code: &str) -> Result<(), String> {
    fs::create_dir_all(run_dir)
        .map_err(|err| format!("cannot create Go run dir {}: {err}", run_dir.display()))?;
    fs::write(run_dir.join("go.mod"), "module smoke\n\ngo 1.21\n")
        .map_err(|err| format!("cannot write go.mod: {err}"))?;

    let imports = local_import_paths(file)?;
    let mut main_code = code.to_owned();
    for import in &imports {
        let package_name = go_package_name_for_import(import)?;
        let module_path = format!("smoke/{package_name}");
        let source_import = format!("import {import:?}");
        if main_code.contains(&source_import) {
            main_code = main_code.replace(&source_import, &format!("import {module_path:?}"));
        } else {
            main_code = insert_go_import(&main_code, &module_path);
        }

        let dependency = resolve_local_import(file, import);
        let dependency_code = compile_go_exemplum(&dependency)?;
        let dependency_code = go_dependency_source_for_run(&dependency_code, &package_name)?;
        let dependency_dir = run_dir.join(&package_name);
        fs::create_dir_all(&dependency_dir).map_err(|err| {
            format!(
                "cannot create dependency dir {}: {err}",
                dependency_dir.display()
            )
        })?;
        fs::write(
            dependency_dir.join(format!("{package_name}.go")),
            dependency_code,
        )
        .map_err(|err| format!("cannot write dependency Go source: {err}"))?;
    }

    fs::write(run_dir.join("main.go"), main_code)
        .map_err(|err| format!("cannot write Go main source: {err}"))?;
    Ok(())
}

fn insert_go_import(code: &str, import_path: &str) -> String {
    let marker = "package main\n";
    let Some(marker_end) = code.find(marker).map(|offset| offset + marker.len()) else {
        return code.to_owned();
    };
    let mut out = String::with_capacity(code.len() + import_path.len() + 12);
    out.push_str(&code[..marker_end]);
    out.push_str("\nimport ");
    out.push_str(&format!("{import_path:?}"));
    out.push('\n');
    out.push_str(&code[marker_end..]);
    out
}

fn go_run_args(file: &Path) -> &'static [&'static str] {
    if file.ends_with("operandus/operandus.fab") {
        // This fixture exercises the required positional CLI operand.
        &["--", "sample-input"]
    } else {
        &[]
    }
}

fn go_expected_exit_code(file: &Path) -> Option<i32> {
    // `exitus` is intentionally a nonzero CLI entry fixture; success means
    // observing its declared exit code, not forcing every process to return 0.
    file.ends_with("exitus/exitus.fab").then_some(1)
}

fn local_import_paths(file: &Path) -> Result<Vec<String>, String> {
    Ok(parsed_import_paths(file)?
        .into_iter()
        .filter(|path| path.starts_with("./") || path.starts_with("../"))
        .collect())
}

fn resolve_local_import(file: &Path, import: &str) -> PathBuf {
    let mut path = file.parent().unwrap_or_else(|| Path::new("")).join(import);
    path.set_extension("fab");
    path
}

fn go_package_name_for_import(import: &str) -> Result<String, String> {
    let name = import
        .rsplit('/')
        .find(|segment| !segment.is_empty() && *segment != "." && *segment != "..")
        .ok_or_else(|| format!("cannot infer Go package name from import {import:?}"))?;
    Ok(sanitize_go_package_name(name))
}

fn sanitize_go_package_name(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        }
    }
    if out.is_empty() || out.as_bytes()[0].is_ascii_digit() {
        out.insert(0, 'm');
    }
    out
}

fn go_dependency_source_for_run(code: &str, package_name: &str) -> Result<String, String> {
    let code = code.replacen("package main", &format!("package {package_name}"), 1);
    let code = strip_main_function(&code)?;
    Ok(export_package_functions(&code))
}

fn export_package_functions(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    for line in code.lines() {
        if let Some(rest) = line.strip_prefix("func ") {
            if !rest.starts_with('(') {
                let mut chars = rest.chars();
                if let Some(first) = chars.next() {
                    if first.is_ascii_lowercase() {
                        out.push_str("func ");
                        out.push(first.to_ascii_uppercase());
                        out.push_str(chars.as_str());
                        out.push('\n');
                        continue;
                    }
                }
            }
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

fn strip_main_function(code: &str) -> Result<String, String> {
    let Some(start) = code.find("\nfunc main() {") else {
        return Ok(code.to_owned());
    };
    let body_start = start + "\nfunc main() {".len();
    let mut depth = 1usize;
    let mut end = body_start;
    for (offset, ch) in code[body_start..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end = body_start + offset + ch.len_utf8();
                    break;
                }
            }
            _ => {}
        }
    }
    if depth != 0 {
        return Err("cannot strip generated dependency main function".to_owned());
    }
    let mut stripped = String::with_capacity(code.len());
    stripped.push_str(&code[..start]);
    stripped.push_str(&code[end..]);
    Ok(stripped)
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

fn expected_compile_failure(path: &Path) -> Option<&'static str> {
    GO_EXPECTED_COMPILE_FAILURES
        .iter()
        .find_map(|(expected_path, expected_message)| {
            path.ends_with(expected_path).then_some(*expected_message)
        })
}

fn go_source_for_run(path: &Path, code: &str) -> String {
    if is_expected_failure(path, GO_DECLARATION_ONLY_FIXTURES) {
        format!("{code}\n\nfunc main() {{}}\n")
    } else {
        code.to_owned()
    }
}

#[test]
fn go_expected_failure_metadata_ceiling_is_ratcheted() {
    assert!(
        GO_EXPECTED_FAILURES.len() <= EXPECTED_GO_EXPECTED_FAILURE_CEILING,
        "Go e2e expected-failure metadata grew: {} above ceiling {EXPECTED_GO_EXPECTED_FAILURE_CEILING}",
        GO_EXPECTED_FAILURES.len(),
    );
}

#[test]
fn go_expected_failure_ledgers_are_disjoint() {
    for (runtime_failure, _) in GO_EXPECTED_RUNTIME_FAILURES {
        assert!(
            !GO_EXPECTED_FAILURES.contains(runtime_failure),
            "{runtime_failure} is listed as both an expected failure and an expected runtime failure",
        );
        assert!(
            !GO_EXPECTED_COMPILE_FAILURES
                .iter()
                .any(|(compile_failure, _)| runtime_failure == compile_failure),
            "{runtime_failure} is listed as both an expected runtime failure and an expected compile failure",
        );
        assert!(
            !GO_DECLARATION_ONLY_FIXTURES.contains(runtime_failure),
            "{runtime_failure} is listed as both an expected runtime failure and a declaration-only fixture",
        );
    }

    for (compile_failure, _) in GO_EXPECTED_COMPILE_FAILURES {
        assert!(
            !GO_EXPECTED_FAILURES.contains(compile_failure),
            "{compile_failure} is listed as both an expected failure and an expected compile failure",
        );
        assert!(
            !GO_DECLARATION_ONLY_FIXTURES.contains(compile_failure),
            "{compile_failure} is listed as both an expected compile failure and a declaration-only fixture",
        );
    }

    for declaration_only in GO_DECLARATION_ONLY_FIXTURES {
        assert!(
            !GO_EXPECTED_FAILURES.contains(declaration_only),
            "{declaration_only} is listed as both an expected failure and a declaration-only fixture",
        );
    }
}

#[test]
fn go_expected_failure_ledgers_reference_current_corpus() {
    let corpus = crate::paths::corpus_dir();
    let missing = GO_EXPECTED_FAILURES
        .iter()
        .copied()
        .chain(GO_EXPECTED_RUNTIME_FAILURES.iter().map(|(path, _)| *path))
        .chain(GO_EXPECTED_COMPILE_FAILURES.iter().map(|(path, _)| *path))
        .chain(GO_DECLARATION_ONLY_FIXTURES.iter().copied())
        .filter(|path| !corpus.join(path).is_file())
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "Go expected-failure metadata references paths outside the public corpus: {}",
        missing.join(", ")
    );
}
