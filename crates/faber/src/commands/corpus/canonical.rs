//! Canonical generated Rust: formatted members and one batched Clippy check.

use super::{Case, Outcome, fail, ms};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;
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
    fs::create_dir_all(&workspace)
        .unwrap_or_else(|error| fail(&format!("cannot create canonical workspace: {error}")));
    let mut rows = vec![None; cases.len()];
    let mut jobs = Vec::new();
    let mut members = Vec::new();
    for (index, case) in cases.iter().enumerate() {
        let case_started = Instant::now();
        if matches!(case.target.outcome, Outcome::NotApplicable) {
            rows[index] = Some(json!({"schema":1,"type":"case","case":case.id,
                "target":"canonical","status":"not-applicable","reason":case.target.reason,
                "timing_ms":{"total":ms(case_started)}}));
            continue;
        }
        let argv = vec![
            faber.display().to_string(),
            "emit".to_owned(),
            "-t".to_owned(),
            "rust".to_owned(),
            "--format".to_owned(),
            case.input.display().to_string(),
        ];
        let emitting = Instant::now();
        let emitted = Command::new(faber)
            .args(&argv[1..])
            .current_dir(&case.dir)
            .output();
        let emit_ms = ms(emitting);
        let emitted = match emitted {
            Ok(output) => output,
            Err(error) => {
                rows[index] = Some(row(
                    case,
                    "fail",
                    "emit",
                    Some(format!("cannot start Rust emit: {error}")),
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
            let expected = case.expect.as_ref().expect("validated expected rejection");
            let fragment = case
                .target
                .diagnostic_contains
                .as_deref()
                .expect("validated diagnostic");
            let diagnostics = String::from_utf8_lossy(&emitted.stderr);
            let status =
                if emitted.status.code() == Some(expected.exit) && diagnostics.contains(fragment) {
                    "pass"
                } else {
                    "fail"
                };
            rows[index] = Some(row(
                case,
                status,
                "emit",
                (status == "fail").then(|| format!("emit rejection changed: {diagnostics}")),
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
        let manifest_text = format!(
            "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nfaber = {{ path = {runtime_value}, default-features = false }}\n"
        );
        let written = fs::create_dir_all(source.parent().expect("source parent"))
            .and_then(|()| write_if_changed(&source, code.as_bytes()))
            .and_then(|()| write_if_changed(&manifest, manifest_text.as_bytes()));
        if let Err(error) = written {
            rows[index] = Some(row(
                case,
                "fail",
                "setup",
                Some(format!("cannot write canonical member: {error}")),
                Some(&argv),
                None,
                emit_ms,
                0.0,
                case_started,
            ));
            continue;
        }
        let formatting = Instant::now();
        let fmt_argv = vec![
            "rustfmt".to_owned(),
            "--edition".to_owned(),
            "2024".to_owned(),
            "--check".to_owned(),
            source.display().to_string(),
        ];
        let checked = Command::new("rustfmt").args(&fmt_argv[1..]).output();
        let format_ms = ms(formatting);
        match checked {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                rows[index] = Some(row(
                    case,
                    "fail",
                    "format",
                    Some(format!(
                        "generated Rust is not rustfmt-clean: {}{}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    )),
                    Some(&fmt_argv),
                    output.status.code(),
                    emit_ms,
                    format_ms,
                    case_started,
                ));
                continue;
            }
            Err(error) => {
                rows[index] = Some(row(
                    case,
                    "fail",
                    "format",
                    Some(format!("cannot start rustfmt: {error}")),
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
    let mut finished = None;
    if !members.is_empty() {
        let manifest = workspace.join("Cargo.toml");
        let members_text = members
            .iter()
            .map(|name| format!("    \"{name}\",\n"))
            .collect::<String>();
        let root_text = format!("[workspace]\nresolver = \"2\"\nmembers = [\n{members_text}]\n");
        write_if_changed(&manifest, root_text.as_bytes())
            .unwrap_or_else(|error| fail(&format!("cannot write canonical workspace: {error}")));
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
        match Command::new("cargo")
            .args(&argv[1..])
            .current_dir(&workspace)
            .output()
        {
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
                        Some("build-finished") => finished = message["success"].as_bool(),
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
                        Some(format!("cannot start Cargo Clippy: {error}")),
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
        let (status, reason) = if !errors.is_empty() {
            ("fail", Some(errors.join("\n")))
        } else if !artifacts.contains(&job.name) || finished != Some(true) {
            (
                "fail",
                Some("Cargo Clippy did not finish a successful check for this case".to_owned()),
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
    let mut failed = 0;
    let mut not_applicable = 0;
    for value in rows.into_iter().flatten() {
        match value["status"].as_str() {
            Some("pass") => passed += 1,
            Some("not-applicable") => not_applicable += 1,
            _ => failed += 1,
        }
        println!("{value}");
    }
    println!(
        "{}",
        json!({"schema":1,"type":"summary","target":"canonical",
        "cases":cases.len(),"passed":passed,"expected_failures":0,
        "not_applicable":not_applicable,"failed":failed,
        "lint":{"command":lint_command,"exit":lint_exit,"cargo_invocations":usize::from(!members.is_empty())},
        "timing_ms":{"discover":discover_ms,"lint":lint_ms,"total":ms(started)}})
    );
    if failed != 0 {
        std::process::exit(1);
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
