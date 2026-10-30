//! Suite-configured target toolchains and compiler inspection lanes.

use super::{Case, Outcome, SUITE_STEP_TIMEOUT, SuiteTarget, ms, run_bounded, summary_status};
use serde_json::{Value, json};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

/// One diagnostic as the corpus lanes report it: `code:issue` identity first,
/// then the message, so a `rejects` row pinned on `diagnostic_contains` can
/// match the diagnostic identity the API and roundtrip lanes surface.
pub(super) fn emit_diagnostic_summary(diagnostic: &radix::Diagnostic) -> String {
    match diagnostic.lookup_key() {
        Some(label) => format!("{label}: {}", diagnostic.message),
        None => diagnostic.message.clone(),
    }
}

struct Paths<'a> {
    suite: &'a Path,
    workspace: &'a Path,
    runtime: Option<&'a Path>,
    case_dir: &'a Path,
    source: &'a Path,
    artifact: &'a Path,
}

pub(super) fn execute(
    cases: &[Case],
    suite: &Path,
    target: &str,
    recipe: &SuiteTarget,
    faber: &Path,
    discover_ms: f64,
    started: Instant,
) {
    if !target
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        super::fail_usage("target name must contain only letters, digits, '-' or '_'");
    }
    let mode = recipe.mode.as_deref().unwrap_or("external");
    if !matches!(
        mode,
        "external" | "build" | "emit" | "roundtrip" | "mir" | "none"
    ) {
        super::fail_config(&format!("unsupported target mode {mode}"));
    }
    if matches!(mode, "external" | "emit") && recipe.emit.is_none() {
        super::fail_config(&format!("[targets.{target}] requires emit"));
    }
    if recipe.product && recipe.emit.is_none() {
        super::fail_config(&format!("[targets.{target}] product requires emit"));
    }
    if mode == "external" && (recipe.extension.is_none() || recipe.run.is_empty()) {
        super::fail_config(&format!("[targets.{target}] requires extension and run"));
    }
    if mode == "build" && (recipe.extension.is_none() || recipe.build.is_empty()) {
        super::fail_config(&format!("[targets.{target}] requires extension and build"));
    }
    let workspace = suite.join("target/corpus").join(target);
    if cases
        .iter()
        .any(|case| !matches!(case.target.outcome, Outcome::NotApplicable))
    {
        fs::create_dir_all(&workspace).unwrap_or_else(|error| {
            super::fail_cant_create(&format!("cannot create target workspace: {error}"))
        });
    }
    if mode == "roundtrip" {
        execute_roundtrip(cases, target, discover_ms, started);
        return;
    }
    let preparation = Instant::now();
    let prepare_paths = Paths {
        suite,
        workspace: &workspace,
        runtime: recipe.runtime.as_deref(),
        case_dir: &workspace,
        source: &workspace,
        artifact: &workspace,
    };
    let prepare = if recipe.prepare.is_empty()
        || cases
            .iter()
            .all(|case| matches!(case.target.outcome, Outcome::NotApplicable))
    {
        None
    } else {
        Some(run_command(
            &recipe.prepare,
            &prepare_paths,
            &workspace,
            None,
            SUITE_STEP_TIMEOUT,
        ))
    };
    let prepare_ms = ms(preparation);
    let mut passed = 0;
    let mut rejected = 0;
    let mut expected_failures = 0;
    let mut not_applicable = 0;
    let mut failed = 0;
    let mut build_ms = 0.0;
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(4)
        .min(cases.len().max(1));
    let next = AtomicUsize::new(0);
    let mut output_rows = Vec::with_capacity(cases.len());
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            let next = &next;
            let prepare = prepare.as_ref();
            let workspace = &workspace;
            handles.push(scope.spawn(move || {
                let mut rows = Vec::new();
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(case) = cases.get(index) else {
                        break;
                    };
                    let (row, case_build_ms) = run_case_row(
                        case, index, target, mode, recipe, faber, suite, workspace, prepare,
                    );
                    rows.push((index, row, case_build_ms));
                }
                rows
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(rows) => output_rows.extend(rows),
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
    });
    output_rows.sort_by_key(|(index, _, _)| *index);
    for (index, mut row, case_build_ms) in output_rows {
        build_ms += case_build_ms;
        if row["status"] == "expected-fail" {
            row["status"] = json!(cases[index].relabel_expected_fail("expected-fail"));
        }
        match row["status"].as_str() {
            Some("pass") => passed += 1,
            Some("rejected") => rejected += 1,
            Some("expected-fail") => expected_failures += 1,
            Some("not-applicable") => not_applicable += 1,
            _ => failed += 1,
        }
        println!("{row}");
    }
    let status = summary_status(passed + rejected, expected_failures, failed);
    // WHY: harnesses and unit proofs grep leg logs for `failed: <n>`; the
    // compact JSON summary cannot carry that literal. stdout is a JSON-lines
    // stream that parsers consume line by line, so the human summary goes to
    // stderr and stdout stays pure JSON.
    eprintln!(
        "corpus {target} summary: {} case(s): {passed} passed, {rejected} rejected, {expected_failures} expected-fail, {not_applicable} not-applicable, failed: {failed} [{status}]",
        cases.len()
    );
    println!(
        "{}",
        json!({
            "schema":1,"type":"summary","target":target,"cases":cases.len(),
            "passed":passed,"rejected":rejected,"expected_failures":expected_failures,
            "not_applicable":not_applicable,"failed":failed,
            "workers":workers,
            "status":status,
            "prepare_command":prepare.as_ref().and_then(|item| item.as_ref().ok())
                .map(|item| json!({"argv":&item.0,"cwd":workspace})),
            "timing_ms":{"discover":discover_ms,"prepare":prepare_ms,"build":build_ms,"total":ms(started)}
        })
    );
    if status != "pass" {
        std::process::exit(1);
    }
}

#[cfg(not(feature = "hir-faber"))]
fn execute_roundtrip(_cases: &[Case], target: &str, _discover_ms: f64, _started: Instant) {
    super::fail_unavailable(&format!(
        "[targets.{target}] roundtrip requires a faber build with feature `hir-faber`"
    ));
}

#[cfg(feature = "hir-faber")]
fn execute_roundtrip(cases: &[Case], target: &str, discover_ms: f64, started: Instant) {
    use super::Judged;
    use radix::module::driver::reemit_faber;
    use radix::module::driver::{Config, Session};
    use radix::module::locale::latin_locale_pack;

    let first_session = Session::new(Config::default());
    let emit_pack = latin_locale_pack();
    let second_session =
        Session::new(Config::default().with_resolved_locale_pack(emit_pack.clone()));
    let mut passed = 0;
    let mut rejected = 0;
    let mut expected_failures = 0;
    let mut not_applicable = 0;
    let mut failed = 0;
    for case in cases {
        let case_started = Instant::now();
        let (status, phase, reason) = if let Some(outcome) = case.target.outcome.judged() {
            let source = fs::read_to_string(&case.input);
            let first = source.as_ref().map(|source| {
                reemit_faber(
                    &first_session,
                    &emit_pack,
                    &case.input.display().to_string(),
                    source,
                )
            });
            let failure = match first {
                Err(error) => Some(format!("cannot read source: {error}")),
                Ok(first) => match first.output.as_ref() {
                    None => Some(format!(
                        "first Faber compile failed: {}",
                        first
                            .diagnostics
                            .iter()
                            .map(emit_diagnostic_summary)
                            .collect::<Vec<_>>()
                            .join(" | ")
                    )),
                    Some(first_output) => {
                        let first_code = &first_output.code;
                        let second = reemit_faber(
                            &second_session,
                            &emit_pack,
                            &case.input.display().to_string(),
                            first_code,
                        );
                        match second.output.as_ref() {
                            None => Some(format!(
                                "second Faber compile failed: {}",
                                second
                                    .diagnostics
                                    .iter()
                                    .map(emit_diagnostic_summary)
                                    .collect::<Vec<_>>()
                                    .join(" | ")
                            )),
                            Some(second_output) if first_code != &second_output.code => {
                                Some("faber emit did not stabilize after one round-trip".to_owned())
                            }
                            Some(_)
                                if source.as_ref().is_ok_and(|source| source.trim().is_empty()) =>
                            {
                                Some("source file was unexpectedly empty".to_owned())
                            }
                            Some(_) => None,
                        }
                    }
                },
            };
            let pinned_later = case
                .target
                .phase
                .as_deref()
                .filter(|phase| matches!(outcome, Judged::KnownFail) && *phase != "emit");
            match (outcome, failure) {
                (Judged::KnownFail, Some(reason)) if pinned_later.is_some() => {
                    failed += 1;
                    (
                        "fail",
                        Some("emit"),
                        Some(format!(
                            "emit failed before expected {} phase: {reason}",
                            pinned_later.unwrap_or("run")
                        )),
                    )
                }
                (Judged::KnownFail, None) if pinned_later.is_some() => {
                    let status = case.relabel_expected_fail("expected-fail");
                    if status == "rejected" {
                        rejected += 1;
                    } else {
                        expected_failures += 1;
                    }
                    (
                        status,
                        Some("emit"),
                        Some(format!(
                            "known failure pinned at {} phase; roundtrip executes emit only",
                            pinned_later.unwrap_or("run")
                        )),
                    )
                }
                (Judged::Pass, None) => {
                    passed += 1;
                    ("pass", Some("emit"), None)
                }
                (Judged::Pass, Some(reason)) => {
                    failed += 1;
                    ("fail", Some("emit"), Some(reason))
                }
                (Judged::KnownFail, Some(reason))
                    if case
                        .target
                        .diagnostic_contains
                        .as_deref()
                        .is_some_and(|fragment| reason.contains(fragment)) =>
                {
                    let status = case.relabel_expected_fail("expected-fail");
                    if status == "rejected" {
                        rejected += 1;
                    } else {
                        expected_failures += 1;
                    }
                    (status, Some("emit"), None)
                }
                (Judged::KnownFail, Some(reason)) => {
                    failed += 1;
                    (
                        "fail",
                        Some("emit"),
                        Some(format!("known failure changed: {reason}")),
                    )
                }
                (Judged::KnownFail, None) => {
                    failed += 1;
                    (
                        "fail",
                        Some("emit"),
                        Some("unexpected pass; update target expectation".to_owned()),
                    )
                }
            }
        } else {
            not_applicable += 1;
            ("not-applicable", None, case.target.reason.clone())
        };
        println!(
            "{}",
            json!({
                "schema":1,"type":"case","case":case.id,"target":target,
                "status":status,"phase":phase,"reason":reason,
                "timing_ms":{"emit":ms(case_started),"total":ms(case_started)}
            })
        );
    }
    let status = summary_status(passed + rejected, expected_failures, failed);
    println!(
        "{}",
        json!({
            "schema":1,"type":"summary","target":target,"cases":cases.len(),
            "passed":passed,"rejected":rejected,"expected_failures":expected_failures,
            "not_applicable":not_applicable,"failed":failed,
            "workers":1,
            "status":status,
            "timing_ms":{"discover":discover_ms,"prepare":0.0,"build":0.0,"total":ms(started)}
        })
    );
    if status != "pass" {
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_arguments)]
fn run_case_row(
    case: &Case,
    index: usize,
    target: &str,
    mode: &str,
    recipe: &SuiteTarget,
    faber: &Path,
    suite: &Path,
    workspace: &Path,
    prepare: Option<&Result<(Vec<String>, Output), String>>,
) -> (Value, f64) {
    let case_started = Instant::now();
    let mut build_ms = 0.0;
    let mut row = if matches!(case.target.outcome, Outcome::NotApplicable) {
        json!({"schema":1,"type":"case","case":case.id,"target":target,
            "status":"not-applicable","reason":case.target.reason,
            "timing_ms":{"total":ms(case_started)}})
    } else if let Some(Err(reason)) = prepare {
        result(
            case,
            target,
            "fail",
            "prepare",
            Some(reason.clone()),
            None,
            case_started,
        )
    } else if let Some(Ok((argv, output))) = prepare
        .filter(|prepared| matches!(prepared.as_ref(), Ok((_, output)) if !output.status.success()))
    {
        let mut row = result(
            case,
            target,
            "fail",
            "prepare",
            Some(format!(
                "target preparation failed: {}{}",
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            )),
            Some(argv),
            case_started,
        );
        row["command"]["cwd"] = json!(workspace);
        row
    } else {
        run_case(
            case,
            index,
            target,
            mode,
            recipe,
            faber,
            suite,
            workspace,
            case_started,
            &mut build_ms,
        )
    };
    row["timing_ms"]["total"] = json!(ms(case_started));
    (row, build_ms)
}

#[allow(clippy::too_many_arguments)]
fn run_case(
    case: &Case,
    index: usize,
    target: &str,
    mode: &str,
    recipe: &SuiteTarget,
    faber: &Path,
    suite: &Path,
    workspace: &Path,
    started: Instant,
    build_total_ms: &mut f64,
) -> Value {
    if mode == "none" {
        return result(
            case,
            target,
            "fail",
            "setup",
            Some("applicable case has no target recipe".to_owned()),
            None,
            started,
        );
    }
    let case_dir = workspace.join(format!("case_{index:04}"));
    // WHY: a case dir left by an earlier run (or an earlier suite layout) can hold
    // a built artifact the current case never produced, so a failed step would
    // still find something to run. Every case starts from an empty workspace.
    if let Err(error) = fs::remove_dir_all(&case_dir)
        && error.kind() != std::io::ErrorKind::NotFound
    {
        super::fail_cant_create(&format!("cannot clear case workspace: {error}"));
    }
    if let Err(error) = fs::create_dir_all(&case_dir) {
        super::fail_cant_create(&format!("cannot create case workspace: {error}"));
    }
    let extension = recipe.extension.as_deref().unwrap_or("txt");
    if !extension.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return result(
            case,
            target,
            "fail",
            "setup",
            Some("invalid source extension".to_owned()),
            None,
            started,
        );
    }
    let source = case_dir.join(format!("main.{extension}"));
    let mut artifact = case_dir.join("program");
    let emitting = Instant::now();
    // Binary emit (the `clif` object): the API output carries bytes the text
    // channel cannot, so they travel here and replace the text on write.
    #[allow(unused_mut)]
    let mut object_bytes: Option<Vec<u8>> = None;
    let api_target = (!matches!(mode, "mir") && case.input.is_file())
        .then(|| external_target(target))
        .flatten();
    // A package-directory input has no single source file to emit: `faber build`
    // emits and links the package in one step and prints the artifact path, so the
    // file-shaped emit and the recipe build are both replaced by that command.
    let package_build =
        (case.input.is_dir() || recipe.product) && matches!(mode, "external" | "build");
    // What the package build produced decides which recipe steps still apply: a
    // linked artifact replaces them, an emitted source file or a module set goes
    // through the same recipe build (and run) as a single-file emit.
    let mut package_linked = false;
    let mut package_modules: Vec<PathBuf> = Vec::new();
    let (emitted_code, emit_ms, emit_command) = if package_build {
        let build_args = vec![
            faber.display().to_string(),
            "build".to_owned(),
            "-t".to_owned(),
            recipe.emit.as_deref().unwrap_or("").to_owned(),
            case.input.display().to_string(),
            "--out-dir".to_owned(),
            case_dir.display().to_string(),
        ];
        let built = run_bounded(
            Command::new(faber)
                .args(&build_args[1..])
                .current_dir(&case_dir),
            Stdio::null(),
            // A package build stands in for the recipe build step, so it gets the
            // suite step budget rather than the per-case run budget.
            SUITE_STEP_TIMEOUT,
        );
        let emit_ms = ms(emitting);
        let output = match built {
            Ok(output) => output,
            Err(error) => {
                return result(
                    case,
                    target,
                    "fail",
                    "build",
                    Some(error.describe("Faber")),
                    Some(&build_args),
                    started,
                );
            }
        };
        // `faber build` performs emission and linking, so a failure belongs to the
        // phase the case pins when that pin is emission; otherwise it is a build.
        let judged_phase = if case.target.phase.as_deref() == Some("emit") {
            "emit"
        } else {
            "build"
        };
        let printed =
            printed_artifact(&output).map(|path| classify_package_product(path, extension));
        // The package build is the whole build phase only when it links the
        // artifact; an emitted source or module set still has the recipe build to
        // run, so a `build` pin is judged there, not here.
        let build_phase_done =
            judged_phase == "emit" || matches!(printed, Some(PackageProduct::Linked(_)) | None);
        if !output.status.success()
            || (build_phase_done && case.target.phase.as_deref() == Some(judged_phase))
        {
            let (status, reason) = judge_phase(case, judged_phase, &output);
            let mut row = result(
                case,
                target,
                status,
                judged_phase,
                reason,
                Some(&build_args),
                started,
            );
            row["exit"] = json!(output.status.code());
            row["timing_ms"]["emit"] = json!(emit_ms);
            return row;
        }
        let mut package_code = String::new();
        let product = printed.and_then(|product| match product {
            PackageProduct::Source(path) => fs::read_to_string(&path).ok().map(|code| {
                package_code = code;
                PackageProduct::Source(path)
            }),
            PackageProduct::Missing => None,
            other => Some(other),
        });
        match product {
            Some(PackageProduct::Linked(path)) => {
                artifact = path;
                package_linked = true;
            }
            Some(PackageProduct::Modules(modules)) => package_modules = modules,
            Some(_) => {}
            None => {
                let mut row = result(
                    case,
                    target,
                    "fail",
                    "build",
                    Some("package build printed no usable artifact".to_owned()),
                    Some(&build_args),
                    started,
                );
                row["exit"] = json!(output.status.code());
                row["timing_ms"]["emit"] = json!(emit_ms);
                return row;
            }
        }
        (
            package_code,
            emit_ms,
            json!({"argv":build_args,"cwd":case_dir}),
        )
    } else if let Some(api_target) = api_target {
        // A `clif` file input that imports sibling modules compiles through
        // the package-MIR object emitter — the route `faber build -t clif`
        // gives it — because a lone-file object carries no linked sibling
        // code and the imported call would decline as CODEGEN001
        // `external definition call`.
        #[cfg(feature = "mir-clif")]
        let compilation = if api_target == radix::module::Target::MirClif
            && faber_cli::package::use_package_compiler(api_target, &case.input, false)
        {
            faber_cli::package::compile_clif_package_input(&case.input)
        } else {
            radix::services::compile_cli_path(&case.input, false, api_target)
        };
        #[cfg(not(feature = "mir-clif"))]
        let compilation = radix::services::compile_cli_path(&case.input, false, api_target);
        let emit_ms = ms(emitting);
        let diagnostics = compilation
            .diagnostics
            .iter()
            .map(emit_diagnostic_summary)
            .collect::<Vec<_>>()
            .join(" | ");
        let emitted_code = match compilation.output {
            Some(output) => {
                #[cfg(feature = "mir-clif")]
                if let radix::module::Output::ClifObject(object) = &output {
                    object_bytes = Some(object.bytes.clone());
                }
                radix::services::output_code(output)
            }
            None => {
                let (status, reason) = judge_api_emit_failure(case, &diagnostics);
                let mut row = result(case, target, status, "emit", reason, None, started);
                row["emit"] = json!({"api_target":api_target.as_str(),"input":case.input});
                row["timing_ms"]["emit"] = json!(emit_ms);
                return row;
            }
        };
        if case.target.phase.as_deref() == Some("emit") {
            let (status, reason) = judge_api_emit_success(case);
            let mut row = result(case, target, status, "emit", reason, None, started);
            row["emit"] = json!({"api_target":api_target.as_str(),"input":case.input});
            row["timing_ms"]["emit"] = json!(emit_ms);
            return row;
        }
        (
            emitted_code,
            emit_ms,
            json!({"api_target":api_target.as_str(),"input":case.input}),
        )
    } else {
        let emit_args = match mode {
            "mir" => vec![
                faber.display().to_string(),
                "mir".to_owned(),
                case.input.display().to_string(),
            ],
            _ => vec![
                faber.display().to_string(),
                "emit".to_owned(),
                "-t".to_owned(),
                recipe.emit.as_deref().unwrap_or("").to_owned(),
                case.input.display().to_string(),
            ],
        };
        let emitted = run_bounded(
            Command::new(faber)
                .args(&emit_args[1..])
                .current_dir(&case.dir),
            Stdio::null(),
            case.setup_timeout(),
        );
        let emit_ms = ms(emitting);
        let output = match emitted {
            Ok(output) => output,
            Err(error) => {
                return result(
                    case,
                    target,
                    "fail",
                    "emit",
                    Some(error.describe("Faber")),
                    Some(&emit_args),
                    started,
                );
            }
        };
        if (case.target.phase.as_deref() == Some("emit") && mode != "roundtrip")
            || !output.status.success()
        {
            let (status, reason) = judge_phase(case, "emit", &output);
            let mut row = result(
                case,
                target,
                status,
                "emit",
                reason,
                Some(&emit_args),
                started,
            );
            row["exit"] = json!(output.status.code());
            row["timing_ms"]["emit"] = json!(emit_ms);
            return row;
        }
        let code = String::from_utf8_lossy(&output.stdout).into_owned();
        (code, emit_ms, json!({"argv":emit_args,"cwd":case.dir}))
    };
    if !package_modules.is_empty() {
        return build_package_modules(
            case,
            target,
            mode,
            recipe,
            &PackageModules {
                suite,
                workspace,
                case_dir: &case_dir,
                modules: &package_modules,
            },
            emit_command,
            emit_ms,
            started,
            build_total_ms,
        );
    }
    if !matches!(mode, "external" | "build") {
        let (status, reason) = match judge_unexecuted_pin(case, "emit") {
            Some((status, reason)) => (status, Some(reason)),
            None => ("pass", None),
        };
        let mut row = result(case, target, status, "emit", reason, None, started);
        row["timing_ms"]["emit"] = json!(emit_ms);
        return row;
    }
    let emitted_code = if target == "go" && !emitted_code.contains("func main()") {
        format!("{emitted_code}\nfunc main() {{}}\n")
    } else {
        emitted_code
    };
    let emitted_bytes = object_bytes.unwrap_or_else(|| emitted_code.clone().into_bytes());
    let paths = Paths {
        suite,
        workspace,
        runtime: recipe.runtime.as_deref(),
        case_dir: &case_dir,
        source: &source,
        artifact: &artifact,
    };
    if !package_linked && let Err(error) = prepare_files(recipe, &paths, &emitted_bytes) {
        return result(case, target, "fail", "setup", Some(error), None, started);
    }
    let mut row = result(case, target, "fail", "build", None, None, started);
    let mut stale_build_pin = false;
    row["emit_command"] = emit_command;
    row["timing_ms"]["emit"] = json!(emit_ms);
    if package_linked {
        // The package build above already emitted and linked the artifact.
    } else if !recipe.build.is_empty() {
        let building = Instant::now();
        let build = run_command(&recipe.build, &paths, &case_dir, None, case.setup_timeout());
        let build_ms = ms(building);
        *build_total_ms += build_ms;
        row["timing_ms"]["build"] = json!(build_ms);
        let (argv, output) = match build {
            Ok(item) => item,
            Err(reason) => {
                row["reason"] = json!(reason);
                return row;
            }
        };
        row["build_command"] = json!({"argv":argv,"cwd":case_dir});
        stale_build_pin = mode != "build"
            && output.status.success()
            && matches!(case.target.outcome, Outcome::KnownFail)
            && case.target.phase.as_deref() == Some("build");
        if (case.target.phase.as_deref() == Some("build") && !stale_build_pin)
            || !output.status.success()
        {
            let (status, reason) = judge_phase(case, "build", &output);
            row["status"] = json!(status);
            row["reason"] = json!(reason);
            row["exit"] = json!(output.status.code());
            return row;
        }
    } else if case.target.phase.as_deref() == Some("build") {
        row["reason"] = json!("case expects build failure but target has no build step");
        return row;
    }
    if mode == "build" {
        let (status, reason) = match judge_unexecuted_pin(case, "build") {
            Some((status, reason)) => (status, json!(reason)),
            None => ("pass", Value::Null),
        };
        row["status"] = json!(status);
        row["phase"] = json!("build");
        row["reason"] = reason;
        return row;
    }
    row["phase"] = json!("run");
    let running = Instant::now();
    let mut run_recipe = if package_linked && !recipe.package_run.is_empty() {
        recipe.package_run.clone()
    } else {
        recipe.run.clone()
    };
    run_recipe.extend(case.args.iter().cloned());
    let run = run_command(
        &run_recipe,
        &paths,
        &case_dir,
        case.stdin.as_deref(),
        case.timeout,
    );
    row["timing_ms"]["run"] = json!(ms(running));
    let (argv, mut output) = match run {
        Ok(item) => item,
        Err(reason) => {
            row["reason"] = json!(reason);
            return row;
        }
    };
    normalize_stdout_lines(&mut output, &recipe.stdout_lines);
    row["command"] = json!({"argv":argv,"cwd":case_dir});
    row["exit"] = json!(output.status.code());
    let comparing = Instant::now();
    let (status, reason) = if stale_build_pin {
        judge_stale_build_pin(case, &output)
    } else {
        judge_phase(case, "run", &output)
    };
    row["status"] = json!(status);
    row["reason"] = json!(reason);
    row["timing_ms"]["compare"] = json!(ms(comparing));
    row
}

/// The artifact path `faber build` prints on stdout for a package input: a file
/// (linked binary or emitted source) or a directory (a set of emitted modules).
fn printed_artifact(output: &Output) -> Option<PathBuf> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .rev()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .find(|path| path.exists())
}

/// What a package build left behind, judged against the recipe's source extension.
#[derive(Debug, PartialEq)]
enum PackageProduct {
    /// A linked artifact (native binary): it replaces the recipe build and is run.
    Linked(PathBuf),
    /// One emitted source file in the recipe's language: build and run it as a
    /// single-file emit.
    Source(PathBuf),
    /// A directory of emitted modules in the recipe's language, one build each.
    Modules(Vec<PathBuf>),
    /// A printed path with nothing usable behind it.
    Missing,
}

fn classify_package_product(path: PathBuf, extension: &str) -> PackageProduct {
    let has_extension = |path: &Path| path.extension().is_some_and(|ext| ext == extension);
    if path.is_dir() {
        let mut modules = fs::read_dir(&path)
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.is_file() && has_extension(path))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        modules.sort();
        return if modules.is_empty() {
            PackageProduct::Missing
        } else {
            PackageProduct::Modules(modules)
        };
    }
    if has_extension(&path) {
        PackageProduct::Source(path)
    } else {
        PackageProduct::Linked(path)
    }
}

struct PackageModules<'a> {
    suite: &'a Path,
    workspace: &'a Path,
    case_dir: &'a Path,
    modules: &'a [PathBuf],
}

/// Run the recipe build once per emitted module of a package. A module set has no
/// single program to run, so only `build` mode is judged here; a recipe that
/// would run the result has no way to run a package of modules.
#[allow(clippy::too_many_arguments)]
fn build_package_modules(
    case: &Case,
    target: &str,
    mode: &str,
    recipe: &SuiteTarget,
    set: &PackageModules<'_>,
    emit_command: Value,
    emit_ms: f64,
    started: Instant,
    build_total_ms: &mut f64,
) -> Value {
    let mut row = result(case, target, "fail", "build", None, None, started);
    row["emit_command"] = emit_command;
    row["timing_ms"]["emit"] = json!(emit_ms);
    if mode != "build" {
        row["reason"] = json!(
            "package build produced a directory of modules, which an executing recipe cannot run"
        );
        return row;
    }
    let mut commands = Vec::new();
    for module in set.modules {
        let artifact = set.case_dir.join(format!(
            "program-{}",
            module
                .file_stem()
                .map_or_else(|| "module".into(), |stem| stem.to_string_lossy())
        ));
        let paths = Paths {
            suite: set.suite,
            workspace: set.workspace,
            runtime: recipe.runtime.as_deref(),
            case_dir: set.case_dir,
            source: module,
            artifact: &artifact,
        };
        let building = Instant::now();
        let build = run_command(
            &recipe.build,
            &paths,
            set.case_dir,
            None,
            case.setup_timeout(),
        );
        *build_total_ms += ms(building);
        let (argv, output) = match build {
            Ok(item) => item,
            Err(reason) => {
                row["reason"] = json!(reason);
                return row;
            }
        };
        commands.push(json!({"argv":argv,"cwd":set.case_dir}));
        if case.target.phase.as_deref() == Some("build") || !output.status.success() {
            let (status, reason) = judge_phase(case, "build", &output);
            row["status"] = json!(status);
            row["reason"] = json!(reason);
            row["exit"] = json!(output.status.code());
            row["build_command"] = json!(commands);
            return row;
        }
    }
    row["build_command"] = json!(commands);
    let (status, reason) = match judge_unexecuted_pin(case, "build") {
        Some((status, reason)) => (status, json!(reason)),
        None => ("pass", Value::Null),
    };
    row["status"] = json!(status);
    row["reason"] = reason;
    row
}

fn external_target(target: &str) -> Option<radix::module::Target> {
    use radix::module::Target;

    Some(match target {
        "go" => Target::HirGo,
        "ts" => Target::HirTypeScript,
        "swift" => Target::HirSwift,
        "wasm" => Target::MirWasm,
        "llvm" => Target::MirLlvm,
        "clif" => Target::MirClif,
        "c" => Target::HirC,
        "sexp" => Target::MirSexp,
        _ => return None,
    })
}

fn judge_api_emit_failure(case: &Case, diagnostics: &str) -> (&'static str, Option<String>) {
    if case
        .target
        .phase
        .as_deref()
        .is_some_and(|phase| phase != "emit")
    {
        return (
            "fail",
            Some(format!(
                "emit failed before expected {} phase: {diagnostics}",
                case.target.phase.as_deref().unwrap_or("run")
            )),
        );
    }
    if matches!(case.target.outcome, Outcome::KnownFail) {
        let fragment = match case.known_fragment() {
            Ok(fragment) => fragment,
            Err(reason) => return ("fail", Some(reason)),
        };
        return if diagnostics.contains(fragment) {
            ("expected-fail", None)
        } else {
            (
                "fail",
                Some(format!("known failure diagnostic changed: {diagnostics}")),
            )
        };
    }
    (
        "fail",
        Some(format!("compiler produced no target output: {diagnostics}")),
    )
}

fn judge_api_emit_success(case: &Case) -> (&'static str, Option<String>) {
    if case.target.phase.as_deref() == Some("emit")
        && matches!(case.target.outcome, Outcome::KnownFail)
    {
        return (
            "fail",
            Some("unexpected pass; update target expectation".to_owned()),
        );
    }
    ("pass", None)
}

fn judge_phase(case: &Case, phase: &str, output: &Output) -> (&'static str, Option<String>) {
    if case
        .target
        .phase
        .as_deref()
        .is_some_and(|expected| expected != phase)
    {
        return (
            "fail",
            Some(format!(
                "{phase} failed before expected {} phase",
                case.target.phase.as_deref().unwrap_or("run")
            )),
        );
    }
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    if matches!(case.target.outcome, Outcome::KnownFail) {
        if let Some(judged) = super::judge_known_run_kind(case, output) {
            return judged;
        }
        if output.status.success() {
            return (
                "fail",
                Some("unexpected pass; update target expectation".to_owned()),
            );
        }
        let fragment = match case.known_fragment() {
            Ok(fragment) => fragment,
            Err(reason) => return ("fail", Some(reason)),
        };
        return if diagnostics.contains(fragment) {
            ("expected-fail", None)
        } else {
            (
                "fail",
                Some(format!("known failure diagnostic changed: {diagnostics}")),
            )
        };
    }
    let expected = case.expect.as_ref();
    if phase == "emit" {
        if let Some(expected) = expected {
            if !super::exit_matches(expected, &output.status) {
                return (
                    "fail",
                    Some(format!(
                        "emit exit mismatch: expected {}, got {}: {diagnostics}",
                        expected.exit, output.status
                    )),
                );
            }
        } else if !output.status.success() {
            return ("fail", Some(format!("emit failed: {diagnostics}")));
        }
        if let Some(fragment) = case.target.diagnostic_contains.as_deref()
            && !diagnostics.contains(fragment)
        {
            return ("fail", Some("expected emit diagnostic changed".to_owned()));
        }
        if let Some(path) = expected
            .filter(|item| item.exit != 0)
            .and_then(|item| item.stdout.as_ref())
        {
            match fs::read(path) {
                Ok(bytes) if bytes == output.stdout => {}
                Ok(_) => return ("fail", Some("emit stdout mismatch".to_owned())),
                Err(error) => return ("fail", Some(format!("cannot read emit golden: {error}"))),
            }
        }
        return ("pass", None);
    }
    if phase == "build" {
        return if output.status.success() {
            (
                "fail",
                Some("unexpected pass; update target expectation".to_owned()),
            )
        } else {
            ("fail", Some(format!("build failed: {diagnostics}")))
        };
    }
    judge_run_output(case, output)
}

/// Compare a program run against the case's expected exit, goldens and fragments,
/// whatever phase the case pins.
fn judge_run_output(case: &Case, output: &Output) -> (&'static str, Option<String>) {
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let expected = match case.expected() {
        Ok(expected) => expected,
        Err(reason) => return ("fail", Some(reason)),
    };
    if !super::exit_matches(expected, &output.status) {
        return (
            "fail",
            Some(format!(
                "exit mismatch: expected {}, got {}: {diagnostics}",
                expected.exit, output.status
            )),
        );
    }
    for (name, path, actual) in [
        ("stdout", expected.stdout.as_ref(), output.stdout.as_slice()),
        ("stderr", expected.stderr.as_ref(), output.stderr.as_slice()),
    ] {
        if let Some(path) = path {
            let golden = match fs::read(path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return ("fail", Some(format!("cannot read {name} golden: {error}")));
                }
            };
            let matches = if name == "stdout" {
                super::normalize_stdout(&golden) == super::normalize_stdout(actual)
            } else {
                golden == actual
            };
            if !matches {
                return ("fail", Some(format!("{name} mismatch")));
            }
        }
    }
    if expected
        .stdout_contains
        .as_ref()
        .is_some_and(|fragment| !String::from_utf8_lossy(&output.stdout).contains(fragment))
    {
        return ("fail", Some("stdout diagnostic mismatch".to_owned()));
    }
    if expected
        .stderr_contains
        .as_ref()
        .is_some_and(|fragment| !String::from_utf8_lossy(&output.stderr).contains(fragment))
    {
        return ("fail", Some("stderr diagnostic mismatch".to_owned()));
    }
    ("pass", None)
}

/// A known-fail pinned at a phase the target mode never executes cannot be judged
/// there: the executed phases all succeeded, so report the pin as held (with the
/// phase that went unchecked) instead of letting it read as a pass.
fn judge_unexecuted_pin(case: &Case, executed: &str) -> Option<(&'static str, String)> {
    let pinned = case.target.phase.as_deref()?;
    let rank = |phase: &str| ["emit", "build", "run"].iter().position(|p| *p == phase);
    if matches!(case.target.outcome, Outcome::KnownFail) && rank(pinned) > rank(executed) {
        return Some((
            "expected-fail",
            format!(
                "known failure pinned at {pinned} phase; this mode executes through {executed} only"
            ),
        ));
    }
    None
}

/// A known-fail pinned at `build` whose build now succeeds: the pin is stale. Judge
/// the run that follows and report its own outcome rather than a bare unexpected pass.
fn judge_stale_build_pin(case: &Case, run: &Output) -> (&'static str, Option<String>) {
    let judged = if case.expect.is_some() {
        judge_run_output(case, run)
    } else if run.status.success() {
        ("pass", None)
    } else {
        (
            "fail",
            Some(format!(
                "exit {}: {}{}",
                run.status,
                String::from_utf8_lossy(&run.stderr),
                String::from_utf8_lossy(&run.stdout)
            )),
        )
    };
    match judged {
        ("pass", _) => (
            "fail",
            Some("unexpected pass; update target expectation".to_owned()),
        ),
        (_, reason) => (
            "fail",
            Some(format!(
                "build pin is stale: build now succeeds; run phase failed: {}; re-pin at phase run",
                reason.unwrap_or_default()
            )),
        ),
    }
}

#[cfg(test)]
#[path = "external_test.rs"]
mod test;

fn result(
    case: &Case,
    target: &str,
    status: &str,
    phase: &str,
    reason: Option<String>,
    argv: Option<&Vec<String>>,
    started: Instant,
) -> Value {
    json!({"schema":1,"type":"case","case":case.id,"target":target,
        "status":status,"phase":phase,"reason":reason,
        "command":argv.map(|argv| json!({"argv":argv,"cwd":case.dir})),
        "timing_ms":{"total":ms(started)}})
}

fn prepare_files(recipe: &SuiteTarget, paths: &Paths<'_>, bytes: &[u8]) -> Result<(), String> {
    write_if_changed(paths.source, bytes)?;
    for (relative, template) in &recipe.files {
        let path = super::relative_path(paths.case_dir, relative)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| -> String {
                super::fail_cant_create(&format!("cannot create {}: {error}", parent.display()))
            })?;
        }
        write_if_changed(&path, expand(template, paths).as_bytes())?;
    }
    for (relative, template) in &recipe.links {
        let path = super::relative_path(paths.case_dir, relative)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| -> String {
                super::fail_cant_create(&format!("cannot create {}: {error}", parent.display()))
            })?;
        }
        let destination = PathBuf::from(expand(template, paths));
        if !destination.exists() {
            return Err(format!(
                "runtime link target is missing: {}",
                destination.display()
            ));
        }
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let old = fs::read_link(&path).map_err(|error| error.to_string())?;
                if old == destination {
                    continue;
                }
                fs::remove_file(&path).map_err(|error| -> String {
                    super::fail_cant_create(&format!("cannot replace {}: {error}", path.display()))
                })?;
            }
            Ok(_) => {
                return Err(format!(
                    "runtime link path is not a symlink: {}",
                    path.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.to_string()),
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&destination, &path).map_err(|error| -> String {
            super::fail_cant_create(&format!("cannot link {}: {error}", path.display()))
        })?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&destination, &path)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Write a staged case file unless it already holds `bytes`. A file that
/// cannot be written is an infrastructure failure of the corpus run, not a
/// case outcome: it exits [`exit_status::CANT_CREATE`].
fn write_if_changed(path: &Path, bytes: &[u8]) -> Result<(), String> {
    match fs::read(path) {
        Ok(old) if old == bytes => Ok(()),
        _ => fs::write(path, bytes).map_err(|error| -> String {
            super::fail_cant_create(&format!("cannot write {}: {error}", path.display()))
        }),
    }
}

fn normalize_stdout_lines(
    output: &mut Output,
    replacements: &std::collections::BTreeMap<String, String>,
) {
    if replacements.is_empty() {
        return;
    }
    let Ok(stdout) = std::str::from_utf8(&output.stdout) else {
        return;
    };
    let mut result = String::with_capacity(stdout.len());
    for segment in stdout.split_inclusive('\n') {
        let (line, ending) = segment
            .strip_suffix('\n')
            .map_or((segment, ""), |line| (line, "\n"));
        result.push_str(replacements.get(line).map_or(line, String::as_str));
        result.push_str(ending);
    }
    output.stdout = result.into_bytes();
}

fn expand(template: &str, paths: &Paths<'_>) -> String {
    let runtime = paths
        .runtime
        .map(|path| path.display().to_string())
        .unwrap_or_default();
    template
        .replace("{suite}", &paths.suite.display().to_string())
        .replace("{workspace}", &paths.workspace.display().to_string())
        .replace("{runtime}", &runtime)
        .replace("{case_dir}", &paths.case_dir.display().to_string())
        .replace("{source}", &paths.source.display().to_string())
        .replace("{artifact}", &paths.artifact.display().to_string())
}

fn run_command(
    argv: &[String],
    paths: &Paths<'_>,
    cwd: &Path,
    stdin: Option<&Path>,
    limit: Duration,
) -> Result<(Vec<String>, Output), String> {
    let mut expanded = Vec::new();
    for arg in argv {
        let value = expand(arg, paths);
        if let Some((parent, suffix)) = value.rsplit_once("/*") {
            let mut matches = fs::read_dir(parent)
                .map_err(|error| format!("cannot expand {value}: {error}"))?
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| {
                    path.is_file()
                        && path
                            .file_name()
                            .is_some_and(|name| name.to_string_lossy().ends_with(suffix))
                })
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            matches.sort();
            if matches.is_empty() {
                return Err(format!("no files match {value}"));
            }
            expanded.extend(matches);
        } else {
            expanded.push(value);
        }
    }
    let Some(program) = expanded.first() else {
        return Err("empty target command".to_owned());
    };
    let input = stdin
        .map(fs::File::open)
        .transpose()
        .map_err(|error| format!("cannot read stdin fixture: {error}"))?;
    let output = run_bounded(
        Command::new(program).args(&expanded[1..]).current_dir(cwd),
        input.map_or_else(Stdio::null, Stdio::from),
        limit,
    )
    .map_err(|error| error.describe(program))?;
    Ok((expanded, output))
}
