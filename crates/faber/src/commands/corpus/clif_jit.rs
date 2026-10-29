//! The `clif-jit` corpus lane: every case through the in-memory JIT.
//!
//! `faber corpus <suite> --target clif-jit` runs each case with
//! `faber run --target clif-jit`: the same compile route as the `clif` leg,
//! but the program is defined in a JIT module inside the child process and
//! run there — no object file, no linker, no runtime archive. Judging reuses
//! the runner lane's expectations (`parse_case` falls a case without a
//! `clif-jit` pin back to its `runner` pin), so a case the JIT runs but that
//! differs from the expected (runner) result is a **fail** — the parity
//! worklist — while a case the Cranelift lowering refuses never runs: the
//! refusal is reported in the expected-failure bucket (a `rejects` pin stays
//! `rejected`), never a pass, because the compile route — and so the refusal
//! set — is the `clif` leg's own.

use super::{Case, Judged, ms, run_bounded, summary_status};
use serde_json::{Value, json};
use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

#[cfg(test)]
#[path = "clif_jit_test.rs"]
mod clif_jit_test;

/// The one stderr line `faber run --target clif-jit` prints after the compile
/// diagnostics when the lowering refuses the program — and only then: a trap
/// or a program's own stderr never ends the stream with it.
const REFUSAL_SENTINEL: &str = "compilation failed";

pub(super) fn execute(cases: &[Case], faber: &Path, discover_ms: f64, started: Instant) {
    let mut passed = 0;
    let mut rejected = 0;
    let mut expected_failures = 0;
    let mut not_applicable = 0;
    let mut failed = 0;
    // WHY: mirrors the runner lane — each case is an independent `faber run`
    // child, so the suite is bound by the sum of its children; rows keep suite
    // order so the JSONL stream stays stable.
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(4)
        .min(cases.len().max(1));
    let next = AtomicUsize::new(0);
    let mut rows = Vec::with_capacity(cases.len());
    std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            let next = &next;
            handles.push(scope.spawn(move || {
                let mut out = Vec::new();
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(case) = cases.get(index) else {
                        break;
                    };
                    out.push((index, run_case(case, faber)));
                }
                out
            }));
        }
        for handle in handles {
            match handle.join() {
                Ok(found) => rows.extend(found),
                Err(payload) => std::panic::resume_unwind(payload),
            }
        }
    });
    rows.sort_by_key(|(index, _)| *index);
    for (_, result) in rows {
        match result["status"].as_str() {
            Some("pass") => passed += 1,
            Some("rejected") => rejected += 1,
            Some("expected-fail") => expected_failures += 1,
            Some("not-applicable") => not_applicable += 1,
            _ => failed += 1,
        }
        println!("{result}");
    }
    let status = summary_status(passed + rejected, expected_failures, failed);
    // WHY: same contract as the runner lane — harnesses and unit proofs grep
    // leg logs for `failed: <n>`, so the human summary goes to stderr and
    // stdout stays a pure JSON-lines stream.
    eprintln!(
        "corpus clif-jit summary: {} case(s): {passed} passed, {rejected} rejected, {expected_failures} expected-fail, {not_applicable} not-applicable, failed: {failed} [{status}]",
        cases.len()
    );
    println!(
        "{}",
        json!({
            "schema": 1,
            "type": "summary",
            "target": "clif-jit",
            "cases": cases.len(),
            "passed": passed,
            "rejected": rejected,
            "expected_failures": expected_failures,
            "not_applicable": not_applicable,
            "failed": failed,
            "status": status,
            "timing_ms": {"discover": discover_ms, "total": ms(started)}
        })
    );
    if status != "pass" {
        std::process::exit(1);
    }
}

/// Arguments after the `faber` executable for one clif-jit case. Unlike the
/// runner lane no `--device` is pinned: the JIT has no device route, and a
/// device program is refused by the compile step instead.
fn clif_jit_run_args(case: &Case) -> Vec<String> {
    let mut args = vec![
        "run".to_owned(),
        "--target".to_owned(),
        "clif-jit".to_owned(),
        case.input.display().to_string(),
    ];
    if !case.args.is_empty() {
        args.push("--".to_owned());
        args.extend(case.args.iter().cloned());
    }
    args
}

/// Whether the child refused the program at compile: the refusal route prints
/// its diagnostics and then this one closing line.
fn refused_at_compile(output: &Output) -> bool {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .last()
        .is_some_and(|line| line.trim() == REFUSAL_SENTINEL)
}

/// The refusal named for the row: the first compile diagnostic line.
fn refusal_reason(output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let diagnostic = stderr
        .lines()
        .find(|line| line.starts_with("error"))
        .unwrap_or(REFUSAL_SENTINEL);
    format!("clif-jit refuses the program: {diagnostic}")
}

fn run_case(case: &Case, faber: &Path) -> Value {
    let started = Instant::now();
    let Some(outcome) = case.target.outcome.judged() else {
        return json!({
            "schema": 1, "type": "case", "case": case.id, "target": "clif-jit",
            "status": "not-applicable", "reason": case.target.reason,
            "timing_ms": {"read": 0.0, "run": 0.0, "compare": 0.0, "total": ms(started)}
        });
    };
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
    let run_args = clif_jit_run_args(case);
    let mut argv = vec![faber.display().to_string()];
    argv.extend(run_args.iter().cloned());
    let mut command = Command::new(faber);
    command.args(&run_args).current_dir(&case.dir);
    let running = Instant::now();
    let output = match run_bounded(
        &mut command,
        stdin.map_or_else(Stdio::null, Stdio::from),
        case.timeout,
    ) {
        Ok(output) => output,
        Err(error) => return case_error(case, error.describe("clif-jit"), started),
    };
    let run_ms = ms(running);
    let comparing = Instant::now();
    let (status, reason) = if refused_at_compile(&output) {
        judge_refusal(case, outcome, &output)
    } else {
        match outcome {
            Judged::Pass => super::judge_pass(case, expected_stdout, expected_stderr, &output),
            Judged::KnownFail => {
                let (status, reason) = super::judge_known_fail(case, expected_stdout, &output);
                (case.relabel_expected_fail(status), reason)
            }
        }
    };
    let compare_ms = ms(comparing);
    json!({
        "schema": 1,
        "type": "case",
        "case": case.id,
        "target": "clif-jit",
        "status": status,
        "reason": reason,
        "command": {"argv": argv, "cwd": case.dir},
        "exit": output.status.code(),
        "timing_ms": {"read": read_ms, "run": run_ms, "compare": compare_ms, "total": ms(started)}
    })
}

/// Judge a run the Cranelift lowering refused before any program code ran.
///
/// A refusal is never a pass. It lands in the expected-failure bucket — the
/// compile route is the `clif` leg's, so the `clif` lane's refusal policy
/// carries over — but a pinned diagnostic must still name it: a case pinned to
/// fail at run phase (or on its output) did not get that far, so a refusal
/// there is a divergence, not the pinned failure.
fn judge_refusal(case: &Case, outcome: Judged, output: &Output) -> (&'static str, Option<String>) {
    let run_pinned = matches!(
        case.target.failure_kind.as_deref(),
        Some("stdout-mismatch" | "nonzero-exit" | "exit-mismatch")
    );
    if matches!(outcome, Judged::KnownFail) && !run_pinned {
        // A diagnostic pin must still name the refusal: the compile route is
        // the `clif` leg's, so its fragment oracle carries over.
        return match case.known_fragment() {
            Ok(fragment) if String::from_utf8_lossy(&output.stderr).contains(fragment) => {
                (case.relabel_expected_fail("expected-fail"), None)
            }
            Ok(_) => (
                "fail",
                Some(format!(
                    "refused with a different diagnostic than the pinned failure: {}",
                    refusal_reason(output)
                )),
            ),
            Err(reason) => ("fail", Some(reason)),
        };
    }
    if run_pinned {
        // The pin judges the run's own output or exit; a refusal produced
        // neither, so the pinned divergence was never reached.
        return (
            "fail",
            Some(format!(
                "refused where a run-phase failure was pinned: {}",
                refusal_reason(output)
            )),
        );
    }
    (
        case.relabel_expected_fail("expected-fail"),
        Some(refusal_reason(output)),
    )
}

fn case_error(case: &Case, reason: String, started: Instant) -> Value {
    json!({
        "schema": 1, "type": "case", "case": case.id, "target": "clif-jit",
        "status": "fail", "reason": reason,
        "timing_ms": {"total": ms(started)}
    })
}
