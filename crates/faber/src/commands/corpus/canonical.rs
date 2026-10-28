//! Canonical generated Rust: formatted members and one batched Clippy check.

use super::{Case, Judged, Outcome, SUITE_STEP_TIMEOUT, ms, run_bounded, summary_status};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::Instant;

struct Job<'a> {
    index: usize,
    case: &'a Case,
    name: String,
    emit_ms: f64,
    format_ms: f64,
    started: Instant,
}

pub(super) fn execute(
    cases: &[Case],
    suite: &Path,
    runtime: &Path,
    faber: &Path,
    discover_ms: f64,
    started: Instant,
) {
    let workspace = suite.join("target/corpus/canonical");
    fs::create_dir_all(&workspace).unwrap_or_else(|error| {
        super::fail_cant_create(&format!("cannot create canonical workspace: {error}"))
    });
    let mut rows = vec![None; cases.len()];
    let mut jobs = Vec::new();
    let mut members = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let case_started = Instant::now();
        let Some(outcome) = case.target.outcome.judged() else {
            rows[index] = Some(json!({"schema":1,"type":"case","case":case.id,
                "target":"canonical","status":"not-applicable","reason":case.target.reason,
                "timing_ms":{"total":ms(case_started)}}));
            continue;
        };
        let argv = vec![
            faber.display().to_string(),
            "emit".to_owned(),
            "-t".to_owned(),
            "rust".to_owned(),
            "--format".to_owned(),
            case.input.display().to_string(),
        ];
        let emitting = Instant::now();
        let emitted = run_bounded(
            Command::new(faber).args(&argv[1..]).current_dir(&case.dir),
            Stdio::null(),
            case.setup_timeout(),
        );
        let emit_ms = ms(emitting);
        let emitted = match emitted {
            Ok(output) => output,
            Err(error) => {
                rows[index] = Some(row(
                    case,
                    "fail",
                    "emit",
                    Some(error.describe("Rust emit")),
                    Some(&argv),
                    None,
                    emit_ms,
                    0.0,
                    case_started,
                ));
                continue;
            }
        };
        if case.target.phase.as_deref() == Some("emit") {
            let (status, reason) = judge_emit(case, outcome, &emitted);
            rows[index] = Some(row(
                case,
                status,
                "emit",
                reason,
                Some(&argv),
                emitted.status.code(),
                emit_ms,
                0.0,
                case_started,
            ));
            continue;
        }
        if !emitted.status.success() {
            rows[index] = Some(row(
                case,
                "fail",
                "emit",
                Some(format!(
                    "Rust emit failed: {}",
                    String::from_utf8_lossy(&emitted.stderr)
                )),
                Some(&argv),
                emitted.status.code(),
                emit_ms,
                0.0,
                case_started,
            ));
            continue;
        }
        let name = format!("canonical_case_{index:04}");
        let member = workspace.join(&name);
        let source = member.join("src/main.rs");
        let manifest = member.join("Cargo.toml");
        let mut code = String::from_utf8_lossy(&emitted.stdout).into_owned();
        if !code.contains("fn main") {
            code.push_str("\nfn main() {}\n");
        }
        let runtime_value = toml::Value::String(runtime.display().to_string());
        let needs_regex = code.contains("faber::Regex");
        let needs_tokio = code.contains("tokio::") || code.contains("block_on_tokio");
        let mut features = Vec::new();
        if needs_regex {
            features.push("\"regex\"");
        }
        if needs_tokio {
            features.push("\"tokio\"");
        }
        let feature_list = if features.is_empty() {
            String::new()
        } else {
            format!(", features = [{}]", features.join(", "))
        };
        let manifest_text = format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[dependencies]\nfaber = {{ path = {runtime_value}, default-features = false{feature_list} }}\n"
        );
        let manifest_text = if needs_tokio {
            format!(
                "{manifest_text}tokio = {{ version = \"1\", features = [\"rt\", \"net\", \"time\"] }}\n"
            )
        } else {
            manifest_text
        };
        let written = source
            .parent()
            .ok_or_else(|| std::io::Error::other("source has no parent directory"))
            .and_then(fs::create_dir_all)
            .and_then(|()| write_if_changed(&source, code.as_bytes()))
            .and_then(|()| write_if_changed(&manifest, manifest_text.as_bytes()));
        if let Err(error) = written {
            super::fail_cant_create(&format!("cannot write canonical member: {error}"));
        }
        let formatting = Instant::now();
        let fmt_argv = vec![
            "rustfmt".to_owned(),
            "--edition".to_owned(),
            "2021".to_owned(),
            "--check".to_owned(),
            source.display().to_string(),
        ];
        let checked = run_bounded(
            Command::new("rustfmt").args(&fmt_argv[1..]),
            Stdio::null(),
            case.setup_timeout(),
        );
        let format_ms = ms(formatting);
        match checked {
            Ok(output) => {
                if let Some((status, reason)) = judge_format(case, &output) {
                    rows[index] = Some(row(
                        case,
                        status,
                        "format",
                        reason,
                        Some(&fmt_argv),
                        output.status.code(),
                        emit_ms,
                        format_ms,
                        case_started,
                    ));
                    continue;
                }
            }
            Err(error) => {
                rows[index] = Some(row(
                    case,
                    "fail",
                    "format",
                    Some(error.describe("rustfmt")),
                    Some(&fmt_argv),
                    None,
                    emit_ms,
                    format_ms,
                    case_started,
                ));
                continue;
            }
        }
        members.push(name.clone());
        jobs.push(Job {
            index,
            case,
            name,
            emit_ms,
            format_ms,
            started: case_started,
        });
    }
    let mut lint_ms = 0.0;
    let mut lint_command = None;
    let mut lint_exit = None;
    let mut diagnostics: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    let mut artifacts = BTreeSet::new();
    if !members.is_empty() {
        let manifest = workspace.join("Cargo.toml");
        let members_text = members
            .iter()
            .map(|name| format!("    \"{name}\",\n"))
            .collect::<String>();
        let root_text = format!("[workspace]\nresolver = \"2\"\nmembers = [\n{members_text}]\n");
        write_if_changed(&manifest, root_text.as_bytes()).unwrap_or_else(|error| {
            super::fail_cant_create(&format!("cannot write canonical workspace: {error}"))
        });
        let argv = vec![
            "cargo".to_owned(),
            "clippy".to_owned(),
            "--workspace".to_owned(),
            "--keep-going".to_owned(),
            "--message-format=json".to_owned(),
            "--manifest-path".to_owned(),
            manifest.display().to_string(),
            "--".to_owned(),
            "-W".to_owned(),
            "clippy::pedantic".to_owned(),
            "-A".to_owned(),
            "clippy::never_loop".to_owned(),
            "-A".to_owned(),
            "clippy::eq_op".to_owned(),
            "-A".to_owned(),
            "clippy::approx_constant".to_owned(),
        ];
        lint_command = Some(json!({"argv":argv,"cwd":workspace}));
        let linting = Instant::now();
        match run_bounded(
            Command::new("cargo")
                .args(&argv[1..])
                .current_dir(&workspace),
            Stdio::null(),
            SUITE_STEP_TIMEOUT,
        ) {
            Ok(output) => {
                lint_ms = ms(linting);
                lint_exit = output.status.code();
                for line in String::from_utf8_lossy(&output.stdout).lines() {
                    let Ok(message) = serde_json::from_str::<Value>(line) else {
                        continue;
                    };
                    match message["reason"].as_str() {
                        Some("compiler-artifact") => {
                            if let Some(name) = message["target"]["name"].as_str() {
                                artifacts.insert(name.to_owned());
                            }
                        }
                        Some("compiler-message") => {
                            if let (Some(name), Some(level), Some(rendered)) = (
                                message["target"]["name"].as_str(),
                                message["message"]["level"].as_str(),
                                message["message"]["rendered"].as_str(),
                            ) {
                                diagnostics
                                    .entry(name.to_owned())
                                    .or_default()
                                    .push((level.to_owned(), rendered.to_owned()));
                            }
                        }
                        Some("build-finished") => {}
                        _ => {}
                    }
                }
                if !output.status.success() {
                    eprint!("{}", String::from_utf8_lossy(&output.stderr));
                }
            }
            Err(error) => {
                for job in &jobs {
                    rows[job.index] = Some(row(
                        job.case,
                        "fail",
                        "lint",
                        Some(error.describe("Cargo Clippy")),
                        None,
                        None,
                        job.emit_ms,
                        job.format_ms,
                        job.started,
                    ));
                }
            }
        }
    }
    for job in jobs {
        if rows[job.index].is_some() {
            continue;
        }
        let messages = diagnostics.get(&job.name);
        let errors = messages
            .into_iter()
            .flatten()
            .filter(|(level, _)| level == "error")
            .map(|(_, rendered)| rendered.as_str())
            .collect::<Vec<_>>();
        let warnings = messages
            .into_iter()
            .flatten()
            .filter(|(level, _)| level == "warning")
            .count();
        let diagnostic = errors.join("\n");
        let expects_build_failure = matches!(job.case.target.outcome, Outcome::KnownFail)
            && job.case.target.phase.as_deref() == Some("build");
        let expected_build_failure = expects_build_failure
            && job
                .case
                .known_fragment()
                .is_ok_and(|fragment| diagnostic.contains(fragment));
        let (status, reason) = if expected_build_failure {
            ("expected-fail", None)
        } else if expects_build_failure && artifacts.contains(&job.name) {
            (
                "fail",
                Some("unexpected pass; update target expectation".to_owned()),
            )
        } else if expects_build_failure {
            (
                "fail",
                Some(format!("expected build diagnostic changed: {diagnostic}")),
            )
        } else if !diagnostic.is_empty() {
            ("fail", Some(diagnostic))
        } else if !artifacts.contains(&job.name) {
            (
                "fail",
                Some("Cargo Clippy produced no artifact for this case".to_owned()),
            )
        } else {
            ("pass", None)
        };
        let mut value = row(
            job.case,
            status,
            "lint",
            reason,
            None,
            lint_exit,
            job.emit_ms,
            job.format_ms,
            job.started,
        );
        value["warnings"] = json!(warnings);
        rows[job.index] = Some(value);
    }
    let mut passed = 0;
    let mut rejected = 0;
    let mut expected_failures = 0;
    let mut failed = 0;
    let mut not_applicable = 0;
    for (index, row) in rows.into_iter().enumerate() {
        let Some(mut value) = row else {
            super::fail_software(&format!(
                "no result recorded for canonical case {}",
                cases[index].id
            ))
        };
        if value["status"] == "expected-fail" {
            value["status"] = json!(cases[index].relabel_expected_fail("expected-fail"));
        }
        match value["status"].as_str() {
            Some("pass") => passed += 1,
            Some("rejected") => rejected += 1,
            Some("expected-fail") => expected_failures += 1,
            Some("not-applicable") => not_applicable += 1,
            _ => failed += 1,
        }
        println!("{value}");
    }
    let status = summary_status(passed + rejected, expected_failures, failed);
    println!(
        "{}",
        json!({"schema":1,"type":"summary","target":"canonical",
        "cases":cases.len(),"passed":passed,"rejected":rejected,"expected_failures":expected_failures,
        "not_applicable":not_applicable,"failed":failed,"status":status,
        "lint":{"command":lint_command,"exit":lint_exit,"cargo_invocations":usize::from(!members.is_empty())},
        "timing_ms":{"discover":discover_ms,"lint":lint_ms,"total":ms(started)}})
    );
    if status != "pass" {
        std::process::exit(1);
    }
}

/// `None` means rustfmt accepted the member and the case does not pin a format failure,
/// so the case proceeds to the lint step.
fn judge_format(case: &Case, output: &Output) -> Option<(&'static str, Option<String>)> {
    let expects_failure = matches!(case.target.outcome, Outcome::KnownFail)
        && case.target.phase.as_deref() == Some("format");
    if output.status.success() {
        return expects_failure.then(|| {
            (
                "fail",
                Some("unexpected pass; update target expectation".to_owned()),
            )
        });
    }
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    if !expects_failure {
        return Some((
            "fail",
            Some(format!(
                "generated Rust is not rustfmt-clean: {diagnostics}"
            )),
        ));
    }
    let fragment = match case.known_fragment() {
        Ok(fragment) => fragment,
        Err(reason) => return Some(("fail", Some(reason))),
    };
    Some(if diagnostics.contains(fragment) {
        ("expected-fail", None)
    } else {
        (
            "fail",
            Some(format!("expected format diagnostic changed: {diagnostics}")),
        )
    })
}

fn judge_emit(case: &Case, outcome: Judged, emitted: &Output) -> (&'static str, Option<String>) {
    let diagnostics = format!(
        "{}{}",
        String::from_utf8_lossy(&emitted.stderr),
        String::from_utf8_lossy(&emitted.stdout)
    );
    match outcome {
        Judged::KnownFail => {
            // The exit status decides first: stdout carries the emitted program.
            if emitted.status.success() {
                return (
                    "fail",
                    Some("unexpected pass; update target expectation".to_owned()),
                );
            }
            let fragment = match case.known_fragment() {
                Ok(fragment) => fragment,
                Err(reason) => return ("fail", Some(reason)),
            };
            if diagnostics.contains(fragment) {
                ("expected-fail", None)
            } else {
                (
                    "fail",
                    Some(format!("expected emit diagnostic changed: {diagnostics}")),
                )
            }
        }
        Judged::Pass => {
            let expected = match case.expected() {
                Ok(expected) => expected,
                Err(reason) => return ("fail", Some(reason)),
            };
            let fragment_found = case
                .target
                .diagnostic_contains
                .as_deref()
                .is_none_or(|fragment| diagnostics.contains(fragment));
            if super::exit_matches(expected, &emitted.status) && fragment_found {
                ("pass", None)
            } else {
                (
                    "fail",
                    Some(format!("emit rejection changed: {diagnostics}")),
                )
            }
        }
    }
}

fn write_if_changed(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    match fs::read(path) {
        Ok(old) if old == bytes => Ok(()),
        _ => fs::write(path, bytes),
    }
}

#[allow(clippy::too_many_arguments)]
fn row(
    case: &Case,
    status: &str,
    phase: &str,
    reason: Option<String>,
    argv: Option<&Vec<String>>,
    exit: Option<i32>,
    emit_ms: f64,
    format_ms: f64,
    started: Instant,
) -> Value {
    json!({"schema":1,"type":"case","case":case.id,"target":"canonical",
        "status":status,"phase":phase,"reason":reason,"exit":exit,
        "command":argv.map(|argv| json!({"argv":argv,"cwd":case.dir})),
        "timing_ms":{"emit":emit_ms,"format":format_ms,"total":ms(started)}})
}
