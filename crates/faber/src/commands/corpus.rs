//! Verify explicit corpus cases with the MIR runner.

use crate::cli::CorpusArgs;
use faber_cli::package;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Instant;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuiteManifest {
    version: u32,
    cases_dir: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseManifest {
    version: u32,
    input: PathBuf,
    #[serde(default)]
    args: Vec<String>,
    stdin: Option<PathBuf>,
    expect: Option<Expected>,
    targets: BTreeMap<String, TargetExpectation>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expected {
    #[serde(default)]
    exit: i32,
    stdout: Option<PathBuf>,
    stderr: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetExpectation {
    outcome: Outcome,
    phase: Option<String>,
    diagnostic_contains: Option<String>,
    reason: Option<String>,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Outcome {
    Pass,
    KnownFail,
    NotApplicable,
}

struct Case {
    id: String,
    dir: PathBuf,
    input: PathBuf,
    args: Vec<String>,
    stdin: Option<PathBuf>,
    expect: Option<Expected>,
    target: TargetExpectation,
}

pub(super) fn cmd_corpus(args: &CorpusArgs) {
    let started = Instant::now();
    if args.target != "runner" {
        fail(&format!("target {} is not implemented", args.target));
    }
    let input = package::absolutize_path(&args.input);
    let discovering = Instant::now();
    let cases = if input.is_dir() && input.join("corpus.toml").is_file() {
        if args.expected.is_some() {
            fail("--expected applies only to a single input");
        }
        load_suite(&input, &args.target).unwrap_or_else(|message| fail(&message))
    } else {
        vec![single_case(&input, args.expected.as_deref()).unwrap_or_else(|message| fail(&message))]
    };
    let discover_ms = ms(discovering);
    let faber = std::env::current_exe()
        .unwrap_or_else(|error| fail(&format!("cannot locate faber executable: {error}")));

    let mut passed = 0;
    let mut expected_failures = 0;
    let mut not_applicable = 0;
    let mut failed = 0;
    for case in &cases {
        let result = run_case(case, &faber);
        match result["status"].as_str() {
            Some("pass") => passed += 1,
            Some("expected-fail") => expected_failures += 1,
            Some("not-applicable") => not_applicable += 1,
            _ => failed += 1,
        }
        println!("{result}");
    }
    println!(
        "{}",
        json!({
            "schema": 1,
            "type": "summary",
            "target": args.target,
            "cases": cases.len(),
            "passed": passed,
            "expected_failures": expected_failures,
            "not_applicable": not_applicable,
            "failed": failed,
            "timing_ms": {"discover": discover_ms, "total": ms(started)}
        })
    );
    if failed != 0 {
        std::process::exit(1);
    }
}

fn single_case(input: &Path, explicit: Option<&Path>) -> Result<Case, String> {
    let stdout = if let Some(path) = explicit {
        package::absolutize_path(path)
    } else if input.is_file() && input.extension().is_some_and(|ext| ext == "fab") {
        input.with_extension("expected")
    } else {
        package::discover_package(input)
            .map_err(|diagnostic| diagnostic.message)?
            .entry
            .with_extension("expected")
    };
    let dir = if input.is_dir() {
        input.to_path_buf()
    } else {
        input
            .parent()
            .ok_or_else(|| format!("input has no parent: {}", input.display()))?
            .to_path_buf()
    };
    Ok(Case {
        id: input.display().to_string(),
        dir,
        input: input.to_path_buf(),
        args: Vec::new(),
        stdin: None,
        expect: Some(Expected {
            exit: 0,
            stdout: Some(stdout),
            stderr: None,
        }),
        target: TargetExpectation {
            outcome: Outcome::Pass,
            phase: None,
            diagnostic_contains: None,
            reason: None,
        },
    })
}

fn load_suite(root: &Path, target: &str) -> Result<Vec<Case>, String> {
    let manifest_path = root.join("corpus.toml");
    let text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let suite: SuiteManifest = toml::from_str(&text)
        .map_err(|error| format!("invalid {}: {error}", manifest_path.display()))?;
    if suite.version != 1 {
        return Err(format!("unsupported corpus.toml version {}", suite.version));
    }
    let cases_root = relative_path(root, &suite.cases_dir)?;
    if !cases_root.is_dir() {
        return Err(format!(
            "cases directory is missing: {}",
            cases_root.display()
        ));
    }
    let mut manifests = Vec::new();
    find_cases(&cases_root, &mut manifests)?;
    manifests.sort();
    if manifests.is_empty() {
        return Err(format!("no case.toml files under {}", cases_root.display()));
    }
    manifests
        .iter()
        .map(|path| load_case(path, &cases_root, target))
        .collect()
}

fn find_cases(dir: &Path, manifests: &mut Vec<PathBuf>) -> Result<(), String> {
    let marker = dir.join("case.toml");
    if marker.is_file() {
        manifests.push(marker);
        return Ok(());
    }
    for entry in
        fs::read_dir(dir).map_err(|error| format!("cannot read {}: {error}", dir.display()))?
    {
        let entry = entry.map_err(|error| format!("cannot read case entry: {error}"))?;
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot read case type: {error}"))?;
        if file_type.is_dir() {
            find_cases(&entry.path(), manifests)?;
        } else if file_type.is_file()
            && entry
                .path()
                .extension()
                .is_some_and(|ext| ext == "fab" || ext == "expected")
        {
            return Err(format!(
                "source or golden outside a case directory: {}",
                entry.path().display()
            ));
        }
    }
    Ok(())
}

fn load_case(manifest_path: &Path, cases_root: &Path, target: &str) -> Result<Case, String> {
    let text = fs::read_to_string(manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let mut manifest: CaseManifest = toml::from_str(&text)
        .map_err(|error| format!("invalid {}: {error}", manifest_path.display()))?;
    if manifest.version != 1 {
        return Err(format!(
            "unsupported case.toml version {} in {}",
            manifest.version,
            manifest_path.display()
        ));
    }
    let dir = manifest_path.parent().expect("case.toml has a parent");
    let id = dir
        .strip_prefix(cases_root)
        .expect("case is inside cases root")
        .to_string_lossy()
        .replace('\\', "/");
    let expectation = manifest
        .targets
        .remove(target)
        .ok_or_else(|| format!("{id}: target {target} has no expectation in case.toml"))?;
    match expectation.outcome {
        Outcome::Pass
            if manifest
                .expect
                .as_ref()
                .is_none_or(|item| item.stdout.is_none()) =>
        {
            return Err(format!("{id}: a passing case requires expect.stdout"));
        }
        Outcome::KnownFail
            if expectation.phase.as_deref() != Some("run")
                || expectation
                    .diagnostic_contains
                    .as_ref()
                    .is_none_or(String::is_empty) =>
        {
            return Err(format!(
                "{id}: known-fail requires phase = \"run\" and diagnostic_contains"
            ));
        }
        Outcome::NotApplicable if expectation.reason.as_ref().is_none_or(String::is_empty) => {
            return Err(format!("{id}: not-applicable requires a reason"));
        }
        _ => {}
    }
    let input = relative_path(dir, &manifest.input)?;
    let stdin = manifest
        .stdin
        .as_deref()
        .map(|path| relative_path(dir, path))
        .transpose()?;
    let expect = manifest
        .expect
        .map(|item| {
            Ok::<Expected, String>(Expected {
                exit: item.exit,
                stdout: item
                    .stdout
                    .as_deref()
                    .map(|path| relative_path(dir, path))
                    .transpose()?,
                stderr: item
                    .stderr
                    .as_deref()
                    .map(|path| relative_path(dir, path))
                    .transpose()?,
            })
        })
        .transpose()?;
    Ok(Case {
        id,
        dir: dir.to_path_buf(),
        input,
        args: manifest.args,
        stdin,
        expect,
        target: expectation,
    })
}

fn relative_path(root: &Path, path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!("case path must stay relative: {}", path.display()));
    }
    Ok(root.join(path))
}

fn run_case(case: &Case, faber: &Path) -> Value {
    let started = Instant::now();
    if matches!(case.target.outcome, Outcome::NotApplicable) {
        return json!({
            "schema": 1, "type": "case", "case": case.id, "target": "runner",
            "status": "not-applicable", "reason": case.target.reason,
            "timing_ms": {"read": 0.0, "run": 0.0, "compare": 0.0, "total": ms(started)}
        });
    }
    let reading = Instant::now();
    let expected_stdout = match case
        .expect
        .as_ref()
        .and_then(|item| item.stdout.as_ref())
        .map(fs::read)
        .transpose()
    {
        Ok(bytes) => bytes,
        Err(error) => {
            return case_error(case, format!("cannot read stdout golden: {error}"), started);
        }
    };
    let expected_stderr = match case
        .expect
        .as_ref()
        .and_then(|item| item.stderr.as_ref())
        .map(fs::read)
        .transpose()
    {
        Ok(bytes) => bytes,
        Err(error) => {
            return case_error(case, format!("cannot read stderr golden: {error}"), started);
        }
    };
    let stdin = match case.stdin.as_ref().map(fs::File::open).transpose() {
        Ok(file) => file,
        Err(error) => {
            return case_error(case, format!("cannot read stdin fixture: {error}"), started);
        }
    };
    let read_ms = ms(reading);
    let mut argv = vec![
        faber.display().to_string(),
        "run".to_owned(),
        case.input.display().to_string(),
    ];
    if !case.args.is_empty() {
        argv.push("--".to_owned());
        argv.extend(case.args.iter().cloned());
    }
    let mut command = Command::new(faber);
    command.arg("run").arg(&case.input).current_dir(&case.dir);
    if !case.args.is_empty() {
        command.arg("--").args(&case.args);
    }
    command.stdin(stdin.map_or_else(Stdio::null, Stdio::from));
    let running = Instant::now();
    let output = match command.output() {
        Ok(output) => output,
        Err(error) => return case_error(case, format!("cannot start runner: {error}"), started),
    };
    let run_ms = ms(running);
    let comparing = Instant::now();
    let (status, reason) = match case.target.outcome {
        Outcome::Pass => {
            let expected = case.expect.as_ref().expect("validated passing expectation");
            if output.status.code() != Some(expected.exit) {
                (
                    "fail",
                    Some(format!(
                        "exit mismatch: expected {}, got {}",
                        expected.exit, output.status
                    )),
                )
            } else if expected_stdout.as_deref() != Some(output.stdout.as_slice()) {
                ("fail", Some("stdout mismatch".to_owned()))
            } else if output.stderr != expected_stderr.unwrap_or_default() {
                ("fail", Some("stderr mismatch".to_owned()))
            } else {
                ("pass", None)
            }
        }
        Outcome::KnownFail => {
            let fragment = case
                .target
                .diagnostic_contains
                .as_deref()
                .expect("validated known failure diagnostic");
            if output.status.success() {
                (
                    "fail",
                    Some("unexpected pass; update target expectation".to_owned()),
                )
            } else if !String::from_utf8_lossy(&output.stderr).contains(fragment) {
                ("fail", Some("known failure diagnostic changed".to_owned()))
            } else {
                ("expected-fail", None)
            }
        }
        Outcome::NotApplicable => unreachable!(),
    };
    let compare_ms = ms(comparing);
    json!({
        "schema": 1,
        "type": "case",
        "case": case.id,
        "target": "runner",
        "status": status,
        "reason": reason,
        "command": {"argv": argv, "cwd": case.dir},
        "exit": output.status.code(),
        "timing_ms": {"read": read_ms, "run": run_ms, "compare": compare_ms, "total": ms(started)}
    })
}

fn case_error(case: &Case, reason: String, started: Instant) -> Value {
    json!({
        "schema": 1, "type": "case", "case": case.id, "target": "runner",
        "status": "fail", "reason": reason,
        "timing_ms": {"total": ms(started)}
    })
}

fn ms(started: Instant) -> f64 {
    (started.elapsed().as_secs_f64() * 1_000_000.0).round() / 1_000.0
}

fn fail(message: &str) -> ! {
    println!(
        "{}",
        json!({"schema": 1, "type": "error", "message": message})
    );
    std::process::exit(1);
}
