//! Verify explicit corpus cases against a selected execution target.

#[cfg(feature = "hir-rust")]
mod canonical;
mod external;
#[cfg(feature = "mir-llvm")]
mod gpu;
#[cfg(test)]
#[path = "corpus_runner_test.rs"]
mod runner_test;
#[cfg(feature = "hir-rust")]
mod rust;

use crate::cli::CorpusArgs;
use faber_cli::package;
#[cfg(any(not(feature = "hir-rust"), not(feature = "mir-llvm")))]
use radix::module::codegen::Target;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

/// Bound for one child process of a case: emit, compile, run, or verify.
const CASE_TIMEOUT_SECS: u64 = 120;
/// Bound for one suite-level step shared by every case: a batched Cargo build or a
/// target prepare command. Those compile many members at once and legitimately outlast
/// a single case.
const SUITE_STEP_TIMEOUT: Duration = Duration::from_secs(600);
/// Grace for the drain threads after the process group is killed: a descendant
/// that called `setsid` escapes the group kill and can still hold a pipe.
const DRAIN_GRACE: Duration = Duration::from_secs(2);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuiteManifest {
    version: u32,
    cases_dir: PathBuf,
    #[serde(default)]
    embedded_only: bool,
    #[serde(default)]
    targets: BTreeMap<String, SuiteTarget>,
    #[serde(default)]
    roots: BTreeMap<String, PathBuf>,
    #[serde(default)]
    expectations: BTreeMap<String, BTreeMap<String, TargetExpectation>>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SuiteTarget {
    runtime: Option<PathBuf>,
    mode: Option<String>,
    emit: Option<String>,
    extension: Option<String>,
    #[serde(default)]
    prepare: Vec<String>,
    #[serde(default)]
    build: Vec<String>,
    #[serde(default)]
    run: Vec<String>,
    #[serde(default)]
    files: BTreeMap<PathBuf, String>,
    #[serde(default)]
    links: BTreeMap<PathBuf, String>,
    #[serde(default)]
    stdout_lines: BTreeMap<String, String>,
}

struct LoadedSuite {
    cases: Vec<Case>,
    recipe: Option<SuiteTarget>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseManifest {
    version: u32,
    id: Option<String>,
    group: Option<String>,
    input_root: Option<String>,
    input: PathBuf,
    #[serde(default)]
    args: Vec<String>,
    stdin: Option<PathBuf>,
    expect: Option<RawExpected>,
    #[serde(default)]
    targets: BTreeMap<String, TargetExpectation>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawExpected {
    exit: Option<i32>,
    exit_nonzero: Option<bool>,
    stdout: Option<PathBuf>,
    stderr: Option<PathBuf>,
    stdout_contains: Option<String>,
    stderr_contains: Option<String>,
    check_only: Option<bool>,
}

struct Expected {
    exit: i32,
    exit_nonzero: bool,
    stdout: Option<PathBuf>,
    stderr: Option<PathBuf>,
    stdout_contains: Option<String>,
    stderr_contains: Option<String>,
    check_only: bool,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetExpectation {
    outcome: Outcome,
    phase: Option<String>,
    failure_kind: Option<String>,
    args: Option<Vec<String>>,
    diagnostic_contains: Option<String>,
    reason: Option<String>,
    expect: Option<RawExpected>,
    timeout_secs: Option<u64>,
    known_fail_why: Option<String>,
    #[serde(skip)]
    rejects: bool,
}

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "kebab-case")]
enum Outcome {
    Pass,
    KnownFail,
    NotApplicable,
    Rejects,
}

/// An [`Outcome`] that a target actually judges; `NotApplicable` cases are
/// reported before judging and never reach it.
#[derive(Clone, Copy)]
enum Judged {
    Pass,
    KnownFail,
}

impl Outcome {
    fn judged(self) -> Option<Judged> {
        match self {
            Self::Pass => Some(Judged::Pass),
            Self::KnownFail | Self::Rejects => Some(Judged::KnownFail),
            Self::NotApplicable => None,
        }
    }
}

struct Case {
    id: String,
    dir: PathBuf,
    input: PathBuf,
    args: Vec<String>,
    stdin: Option<PathBuf>,
    expect: Option<Expected>,
    target: TargetExpectation,
    timeout: Duration,
}

impl Case {
    /// Rejections share the known-fail judge but stay a distinct outcome: the
    /// compiled artifact failing on purpose is the tested behavior, not a
    /// known breakage parked in the expected-failure bucket.
    pub(super) fn relabel_expected_fail(&self, status: &'static str) -> &'static str {
        if status == "expected-fail" && self.target.rejects {
            "rejected"
        } else {
            status
        }
    }

    /// The diagnostic fragment a diagnostic known-fail pins. Validation guarantees it;
    /// a miss is reported as a case failure, never a panic.
    fn known_fragment(&self) -> Result<&str, String> {
        self.target
            .diagnostic_contains
            .as_deref()
            .ok_or_else(|| format!("{}: known-fail case has no diagnostic_contains", self.id))
    }

    /// The expectation of a case that runs and compares. Validation guarantees it for
    /// passing cases; a miss is reported as a case failure, never a panic.
    fn expected(&self) -> Result<&Expected, String> {
        self.expect
            .as_ref()
            .ok_or_else(|| format!("{}: case has no expectation to compare", self.id))
    }

    /// Bound for a case's setup phases: emit, build, verify, and format.
    ///
    /// `timeout` (from `timeout_secs`) bounds the run phase only. A case may pin a
    /// short run bound to probe a hanging program; setup phases keep the default
    /// bound, or the case's own bound when it is larger.
    fn setup_timeout(&self) -> Duration {
        self.timeout.max(Duration::from_secs(CASE_TIMEOUT_SECS))
    }
}

/// A child process that did not produce an output within its bound.
enum ChildError {
    Io(io::Error),
    Timeout(Duration),
}

impl ChildError {
    fn describe(&self, what: &str) -> String {
        match self {
            Self::Io(error) => format!("cannot start {what}: {error}"),
            Self::Timeout(limit) => {
                format!("timeout: {what} did not finish within {}s", limit.as_secs())
            }
        }
    }
}

/// Run `command` to completion or kill it at `limit`. The child leads its own process
/// group so an overrun kills its descendants too (a build tool's compiler, a shell's
/// children), not only the direct child.
fn run_bounded(command: &mut Command, stdin: Stdio, limit: Duration) -> Result<Output, ChildError> {
    command
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(command, 0);
    let mut child = command.spawn().map_err(ChildError::Io)?;
    let (drained, receiver) = mpsc::channel();
    drain(child.stdout.take(), 0, drained.clone());
    drain(child.stderr.take(), 1, drained);
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                kill_process_group(child.id());
                child.kill().ok();
                child.wait().ok();
                return Err(ChildError::Timeout(limit));
            }
            Ok(None) => thread::sleep(Duration::from_millis(2)),
            Err(error) => return Err(ChildError::Io(error)),
        }
    };
    // The deadline stays in force past the direct child's exit: a descendant that
    // inherited a pipe keeps it open, so joining the drain threads could block
    // until that descendant exits on its own.
    let mut parts = [None, None];
    if collect_drains(&receiver, &mut parts, deadline) {
        return Ok(Output {
            status,
            stdout: parts[0].take().unwrap_or_default(),
            stderr: parts[1].take().unwrap_or_default(),
        });
    }
    // Pipes still open at the deadline. Kill the group — the leader's pgid stays
    // valid while any member lives — then reap the child and give the drains a
    // short grace. A descendant that called `setsid` escapes the kill and can
    // still hold a pipe, so the detached drain threads are abandoned rather than
    // joined; dropping the receiver makes their final send fail harmlessly.
    kill_process_group(child.id());
    child.wait().ok();
    collect_drains(&receiver, &mut parts, Instant::now() + DRAIN_GRACE);
    Err(ChildError::Timeout(limit))
}

/// Read a child pipe on its own detached thread so a chatty child cannot fill it
/// and stall; the buffer goes to `drained` tagged by stream once EOF is reached.
fn drain(
    pipe: Option<impl Read + Send + 'static>,
    which: usize,
    drained: Sender<(usize, Vec<u8>)>,
) {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            pipe.read_to_end(&mut bytes).ok();
        }
        drained.send((which, bytes)).ok();
    });
}

/// Collect both drain results into `parts`, bounded by `deadline`. Returns whether
/// both streams reported EOF in time.
fn collect_drains(
    receiver: &Receiver<(usize, Vec<u8>)>,
    parts: &mut [Option<Vec<u8>>; 2],
    deadline: Instant,
) -> bool {
    while parts[0].is_none() || parts[1].is_none() {
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        match receiver.recv_timeout(deadline - now) {
            Ok((which, bytes)) => parts[which] = Some(bytes),
            Err(_) => return false,
        }
    }
    true
}

#[cfg(unix)]
fn kill_process_group(leader: u32) {
    Command::new("kill")
        .args(["-KILL", "--"])
        .arg(format!("-{leader}"))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok();
}

#[cfg(not(unix))]
fn kill_process_group(leader: u32) {
    Command::new("taskkill")
        .args(["/F", "/T", "/PID"])
        .arg(leader.to_string())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok();
}

/// One summary status shared by every target. A run where every case is
/// not-applicable proved nothing and must not exit green.
fn summary_status(passed: usize, expected_failures: usize, failed: usize) -> &'static str {
    if failed != 0 {
        "fail"
    } else if passed == 0 && expected_failures == 0 {
        "no-applicable-cases"
    } else {
        "pass"
    }
}

pub(super) fn cmd_corpus(args: &CorpusArgs) {
    let started = Instant::now();
    let input = package::absolutize_path(&args.input);
    let discovering = Instant::now();
    let suite = if input.is_dir() && input.join("corpus.toml").is_file() {
        if args.expected.is_some() {
            fail("--expected applies only to a single input");
        }
        let mut suite = load_suite(&input, &args.target).unwrap_or_else(|message| fail(&message));
        if let Some(filter) = args.filter.as_deref() {
            suite.cases.retain(|case| case.id.contains(filter));
            if suite.cases.is_empty() {
                fail(&format!("no cases match filter {filter:?}"));
            }
        }
        suite
    } else {
        if args.filter.is_some() {
            fail("--filter applies only to a suite");
        }
        if args.target != "runner" {
            fail("non-runner targets require a suite with a target recipe");
        }
        LoadedSuite {
            cases: vec![
                single_case(&input, args.expected.as_deref())
                    .unwrap_or_else(|message| fail(&message)),
            ],
            recipe: None,
        }
    };
    let discover_ms = ms(discovering);
    let faber = std::env::current_exe()
        .unwrap_or_else(|error| fail(&format!("cannot locate faber executable: {error}")));

    if args.target == "rust" {
        #[cfg(not(feature = "hir-rust"))]
        package::target_gate::exit_target_not_in_this_build(Target::HirRust);
        #[cfg(feature = "hir-rust")]
        {
            rust::execute(
                &suite.cases,
                &input,
                suite
                    .recipe
                    .as_ref()
                    .and_then(|item| item.runtime.as_deref())
                    .unwrap_or_else(|| fail("rust target requires [targets.rust] runtime")),
                &faber,
                discover_ms,
                started,
            );
            return;
        }
    }
    if args.target == "canonical" {
        #[cfg(not(feature = "hir-rust"))]
        package::target_gate::exit_target_not_in_this_build(Target::HirRust);
        #[cfg(feature = "hir-rust")]
        {
            canonical::execute(
                &suite.cases,
                &input,
                suite
                    .recipe
                    .as_ref()
                    .and_then(|item| item.runtime.as_deref())
                    .unwrap_or_else(|| {
                        fail("canonical target requires [targets.canonical] runtime")
                    }),
                &faber,
                discover_ms,
                started,
            );
            return;
        }
    }
    if args.target == "gpu" {
        #[cfg(not(feature = "mir-llvm"))]
        package::target_gate::exit_target_not_in_this_build(Target::MirLlvm);
        #[cfg(feature = "mir-llvm")]
        {
            gpu::execute(&suite.cases, &input, discover_ms, started);
            return;
        }
    }
    if args.target != "runner" {
        external::execute(
            &suite.cases,
            &input,
            &args.target,
            suite
                .recipe
                .as_ref()
                .unwrap_or_else(|| fail("external target requires a suite target recipe")),
            &faber,
            discover_ms,
            started,
        );
        return;
    }

    let mut passed = 0;
    let mut rejected = 0;
    let mut expected_failures = 0;
    let mut not_applicable = 0;
    let mut failed = 0;
    for case in &suite.cases {
        let result = run_case(case, &faber);
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
    println!(
        "{}",
        json!({
            "schema": 1,
            "type": "summary",
            "target": args.target,
            "cases": suite.cases.len(),
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
            exit_nonzero: false,
            stdout: Some(stdout),
            stderr: None,
            stdout_contains: None,
            stderr_contains: None,
            check_only: false,
        }),
        target: TargetExpectation {
            outcome: Outcome::Pass,
            phase: None,
            failure_kind: None,
            args: None,
            diagnostic_contains: None,
            reason: None,
            expect: None,
            timeout_secs: None,
            known_fail_why: None,
            rejects: false,
        },
        timeout: Duration::from_secs(CASE_TIMEOUT_SECS),
    })
}

fn load_suite(root: &Path, target: &str) -> Result<LoadedSuite, String> {
    let manifest_path = root.join("corpus.toml");
    let text = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("cannot read {}: {error}", manifest_path.display()))?;
    let suite: SuiteManifest = toml::from_str(&text)
        .map_err(|error| format!("invalid {}: {error}", manifest_path.display()))?;
    if suite.version != 1 {
        return Err(format!("unsupported corpus.toml version {}", suite.version));
    }
    if suite.embedded_only {
        if let Some(name) = package::checkout_runtime_override() {
            return Err(format!(
                "embedded_only suite refuses {name}: the embedded runtime is used instead of the checkout"
            ));
        }
    }
    let roots = resolve_suite_roots(root, suite.roots)?;
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
    let cases = manifests
        .iter()
        .map(|path| load_case(path, &cases_root, target, &roots, &suite.expectations))
        .collect::<Result<Vec<_>, _>>()?;
    let recipe = if target == "runner" {
        None
    } else {
        Some(
            suite
                .targets
                .get(target)
                .ok_or_else(|| format!("suite has no [targets.{target}] recipe"))?
                .clone(),
        )
    };
    let recipe = if let Some(mut recipe) = recipe {
        if target == "gpu" && recipe.mode.as_deref() != Some("gpu") {
            return Err("gpu target requires [targets.gpu] mode = \"gpu\"".to_owned());
        }
        if matches!(target, "rust" | "canonical") && suite.embedded_only {
            if recipe.runtime.is_some() {
                return Err(format!(
                    "embedded_only suite must not set [targets.{target}] runtime in corpus.toml"
                ));
            }
            let path = package::embedded_runtime_root()
                .ok_or_else(|| {
                    format!(
                        "embedded_only {target} target requires an embedded runtime, which is unavailable"
                    )
                })?
                .join("runtime/rust");
            if !path.join("Cargo.toml").is_file() {
                return Err(format!(
                    "embedded_only embedded runtime is missing Cargo.toml: {}",
                    path.display()
                ));
            }
            recipe.runtime = Some(path);
        } else {
            if matches!(target, "rust" | "canonical") && recipe.runtime.is_none() {
                return Err(format!(
                    "{target} target requires [targets.{target}] runtime in corpus.toml"
                ));
            }
            if let Some(configured) = recipe.runtime.as_deref() {
                let path = if configured.is_absolute() {
                    configured.to_path_buf()
                } else {
                    root.join(configured)
                };
                if !path.is_dir() {
                    return Err(format!("target runtime is missing: {}", path.display()));
                }
                if matches!(target, "rust" | "canonical") && !path.join("Cargo.toml").is_file() {
                    return Err(format!("Rust runtime crate is missing: {}", path.display()));
                }
                recipe.runtime = Some(path.canonicalize().map_err(|error| {
                    format!("cannot resolve target runtime {}: {error}", path.display())
                })?);
            }
        }
        Some(recipe)
    } else {
        None
    };
    Ok(LoadedSuite { cases, recipe })
}

fn resolve_suite_roots(
    suite_root: &Path,
    configured: BTreeMap<String, PathBuf>,
) -> Result<BTreeMap<String, PathBuf>, String> {
    configured
        .into_iter()
        .map(|(name, path)| {
            if name.is_empty()
                || !name
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
            {
                return Err(format!("invalid suite input root name {name:?}"));
            }
            let candidate = if path.is_absolute() {
                path
            } else {
                suite_root.join(path)
            };
            let resolved = candidate.canonicalize().map_err(|error| {
                format!(
                    "cannot resolve suite input root {}: {error}",
                    candidate.display()
                )
            })?;
            if !resolved.is_dir() {
                return Err(format!(
                    "suite input root is not a directory: {}",
                    resolved.display()
                ));
            }
            Ok((name, resolved))
        })
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

fn load_case(
    manifest_path: &Path,
    cases_root: &Path,
    target: &str,
    roots: &BTreeMap<String, PathBuf>,
    expectations: &BTreeMap<String, BTreeMap<String, TargetExpectation>>,
) -> Result<Case, String> {
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
    let manifest_dir = manifest_path
        .parent()
        .ok_or_else(|| format!("{} has no parent directory", manifest_path.display()))?;
    let default_id = manifest_dir
        .strip_prefix(cases_root)
        .map_err(|_| {
            format!(
                "{} is not inside the cases root {}",
                manifest_path.display(),
                cases_root.display()
            )
        })?
        .to_string_lossy()
        .replace('\\', "/");
    let id = manifest.id.take().unwrap_or(default_id);
    let rooted = manifest.input_root.is_some();
    let input = if let Some(root_name) = manifest.input_root.as_deref() {
        let input_root = roots
            .get(root_name)
            .ok_or_else(|| format!("{id}: unknown suite input root {root_name:?}"))?;
        let relative = relative_path(Path::new("."), &manifest.input)?;
        let candidate = input_root.join(relative);
        let resolved = candidate.canonicalize().map_err(|error| {
            format!(
                "{id}: cannot resolve input {}: {error}",
                candidate.display()
            )
        })?;
        if !resolved.starts_with(input_root) {
            return Err(format!("{id}: input escapes suite root {root_name:?}"));
        }
        resolved
    } else {
        relative_path(manifest_dir, &manifest.input)?
    };
    let dir = if rooted {
        if input.is_dir() {
            input.clone()
        } else {
            input
                .parent()
                .ok_or_else(|| format!("{id}: input has no parent: {}", input.display()))?
                .to_path_buf()
        }
    } else {
        manifest_dir.to_path_buf()
    };
    let expectation = manifest.targets.remove(target).or_else(|| {
        manifest
            .group
            .as_deref()
            .and_then(|group| expectations.get(group))
            .and_then(|group| group.get(target))
            .cloned()
    });
    let Some(mut expectation) = expectation else {
        return Err(format!(
            "{id}: target {target} has no expectation from the case or group"
        ));
    };
    if matches!(expectation.outcome, Outcome::Rejects) {
        expectation.rejects = true;
        expectation.outcome = Outcome::KnownFail;
    }
    if expectation.rejects && expectation.known_fail_why.is_some() {
        return Err(format!(
            "{id}: known_fail_why is not allowed on a rejects case; a rejection needs no reason"
        ));
    }
    if !expectation.rejects
        && !matches!(expectation.outcome, Outcome::KnownFail)
        && expectation.known_fail_why.is_some()
    {
        return Err(format!(
            "{id}: known_fail_why is only allowed on a known-fail case"
        ));
    }
    if expectation
        .known_fail_why
        .as_deref()
        .is_some_and(|why| why.trim().is_empty())
    {
        return Err(format!("{id}: known_fail_why must not be empty"));
    }
    let base_expect = resolve_expected_patch(manifest.expect.take(), manifest_dir)?;
    let target_expect = resolve_expected_patch(expectation.expect.clone(), manifest_dir)?;
    let expect = if rooted
        || base_expect.is_some()
        || target_expect.is_some()
        || matches!(expectation.outcome, Outcome::Pass)
    {
        let base = base_expect.unwrap_or_default();
        let target = target_expect.unwrap_or_default();
        Some(Expected {
            exit: target.exit.or(base.exit).unwrap_or(0),
            exit_nonzero: target.exit_nonzero.or(base.exit_nonzero).unwrap_or(false),
            stdout: target.stdout.or(base.stdout),
            stderr: target.stderr.or(base.stderr),
            stdout_contains: target.stdout_contains.or(base.stdout_contains),
            stderr_contains: target.stderr_contains.or(base.stderr_contains),
            check_only: target.check_only.or(base.check_only).unwrap_or(false),
        })
    } else {
        None
    };
    match expectation.outcome {
        Outcome::KnownFail
            if !(matches!(expectation.phase.as_deref(), Some("emit" | "build" | "run"))
                || (target == "canonical" && expectation.phase.as_deref() == Some("format")))
                || (target == "runner" && expectation.phase.as_deref() != Some("run"))
                || !matches!(
                    expectation.failure_kind.as_deref().unwrap_or("diagnostic"),
                    "diagnostic" | "stdout-mismatch" | "nonzero-exit"
                )
                || (expectation.failure_kind.as_deref() == Some("stdout-mismatch")
                    && (expectation.phase.as_deref() != Some("run")
                        || expect.as_ref().is_none_or(|item| item.stdout.is_none())))
                || (expectation.failure_kind.as_deref() == Some("nonzero-exit")
                    && (expectation.phase.as_deref() != Some("run")
                        || expect.as_ref().is_none_or(|item| !item.exit_nonzero)))
                || (expectation.failure_kind.as_deref() != Some("stdout-mismatch")
                    && expectation.failure_kind.as_deref() != Some("nonzero-exit")
                    && expectation
                        .diagnostic_contains
                        .as_ref()
                        .is_none_or(String::is_empty)) =>
        {
            return Err(format!(
                "{id}: known-fail requires a supported phase and a stable diagnostic or stdout-mismatch oracle"
            ));
        }
        Outcome::Pass
            if expectation.phase.as_deref() == Some("emit")
                && expect
                    .as_ref()
                    .is_some_and(|item| item.exit != 0 || item.exit_nonzero)
                && expectation
                    .diagnostic_contains
                    .as_ref()
                    .is_none_or(String::is_empty) =>
        {
            return Err(format!(
                "{id}: an expected emit rejection requires nonzero expect.exit and diagnostic_contains"
            ));
        }
        Outcome::Pass
            if expectation.phase.is_some()
                && !matches!(expectation.phase.as_deref(), Some("emit" | "run")) =>
        {
            return Err(format!("{id}: passing case has unsupported phase"));
        }
        Outcome::Pass
            if !matches!(target, "roundtrip" | "mir" | "gpu")
                && expectation.phase.as_deref() != Some("emit")
                && expect.as_ref().is_some_and(|item| {
                    item.stdout.is_none()
                        && item.stderr.is_none()
                        && item.stdout_contains.is_none()
                        && item.stderr_contains.is_none()
                        && !item.check_only
                }) =>
        {
            return Err(format!(
                "{id}: execution pass without output assertions must set expect.check_only = true"
            ));
        }
        Outcome::NotApplicable if expectation.reason.as_ref().is_none_or(String::is_empty) => {
            return Err(format!("{id}: not-applicable requires a reason"));
        }
        _ => {}
    }
    check_executor_support(&id, target, &expectation)?;
    let timeout = match expectation.timeout_secs {
        Some(0) => {
            return Err(format!(
                "{id}: target {target}: timeout_secs must be positive"
            ));
        }
        Some(seconds) => Duration::from_secs(seconds),
        None => Duration::from_secs(CASE_TIMEOUT_SECS),
    };
    let stdin = manifest
        .stdin
        .as_deref()
        .map(|path| relative_path(manifest_dir, path))
        .transpose()?;
    Ok(Case {
        id,
        dir: dir.to_path_buf(),
        input,
        args: expectation.args.clone().unwrap_or(manifest.args),
        stdin,
        expect,
        target: expectation,
        timeout,
    })
}

/// Reject every pin the selected target's executor cannot judge, so the executor never
/// meets a shape it would have to guess at. Executors still report a missing field as a
/// case failure rather than panic, but this is the one place that decides what is legal.
fn check_executor_support(
    id: &str,
    target: &str,
    expectation: &TargetExpectation,
) -> Result<(), String> {
    let kind = expectation.failure_kind.as_deref().unwrap_or("diagnostic");
    let phase = expectation.phase.as_deref();
    let unsupported = |what: &str| Err(format!("{id}: target {target} cannot judge {what}"));
    match (target, expectation.outcome) {
        (_, Outcome::NotApplicable) => Ok(()),
        ("runner", Outcome::KnownFail) if kind == "nonzero-exit" => {
            unsupported("failure_kind nonzero-exit")
        }
        ("runner", Outcome::Pass) if !matches!(phase, None | Some("run")) => {
            unsupported("a passing case with a phase other than run")
        }
        ("rust" | "canonical" | "gpu", Outcome::KnownFail) if kind != "diagnostic" => unsupported(
            &format!("failure_kind {kind}; only diagnostic is supported"),
        ),
        ("canonical", Outcome::KnownFail) if phase == Some("run") => {
            unsupported("a known failure in the run phase; it never runs the program")
        }
        ("canonical", Outcome::Pass) if phase == Some("run") => {
            unsupported("a passing case in the run phase; it never runs the program")
        }
        ("gpu", Outcome::KnownFail) if phase != Some("emit") => {
            unsupported("a known failure outside the emit phase")
        }
        _ => Ok(()),
    }
}

#[derive(Default)]
struct ExpectedPatch {
    exit: Option<i32>,
    exit_nonzero: Option<bool>,
    stdout: Option<PathBuf>,
    stderr: Option<PathBuf>,
    stdout_contains: Option<String>,
    stderr_contains: Option<String>,
    check_only: Option<bool>,
}

fn resolve_expected_patch(
    raw: Option<RawExpected>,
    base: &Path,
) -> Result<Option<ExpectedPatch>, String> {
    raw.map(|item| {
        Ok(ExpectedPatch {
            exit: item.exit,
            exit_nonzero: item.exit_nonzero,
            stdout: item
                .stdout
                .as_deref()
                .map(|path| relative_path(base, path))
                .transpose()?,
            stderr: item
                .stderr
                .as_deref()
                .map(|path| relative_path(base, path))
                .transpose()?,
            stdout_contains: item.stdout_contains,
            stderr_contains: item.stderr_contains,
            check_only: item.check_only,
        })
    })
    .transpose()
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

/// Arguments after the `faber` executable for one runner-target case.
///
/// The runner target pins `--device cpu`: it verifies the MIR runner, so device
/// selection must not depend on the host (`auto` routes kernel programs to Metal on a
/// capable machine and fails closed elsewhere).
fn runner_run_args(case: &Case) -> Vec<String> {
    let mut args = vec![
        "run".to_owned(),
        case.input.display().to_string(),
        "--device".to_owned(),
        "cpu".to_owned(),
    ];
    if !case.args.is_empty() {
        args.push("--".to_owned());
        args.extend(case.args.iter().cloned());
    }
    args
}

fn run_case(case: &Case, faber: &Path) -> Value {
    let started = Instant::now();
    let Some(outcome) = case.target.outcome.judged() else {
        return json!({
            "schema": 1, "type": "case", "case": case.id, "target": "runner",
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
    let run_args = runner_run_args(case);
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
        Err(error) => return case_error(case, error.describe("runner"), started),
    };
    let run_ms = ms(running);
    let comparing = Instant::now();
    let (status, reason) = match outcome {
        Judged::Pass => judge_pass(case, expected_stdout, expected_stderr, &output),
        Judged::KnownFail => {
            let (status, reason) = judge_known_fail(case, expected_stdout, &output);
            (case.relabel_expected_fail(status), reason)
        }
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

fn judge_pass(
    case: &Case,
    expected_stdout: Option<Vec<u8>>,
    expected_stderr: Option<Vec<u8>>,
    output: &Output,
) -> (&'static str, Option<String>) {
    let expected = match case.expected() {
        Ok(expected) => expected,
        Err(reason) => return ("fail", Some(reason)),
    };
    if !exit_matches(expected, &output.status) {
        (
            "fail",
            Some(format!(
                "exit mismatch: expected {}, got {}",
                expected.exit, output.status
            )),
        )
    } else if expected_stdout
        .as_deref()
        .is_some_and(|expected| normalize_stdout(expected) != normalize_stdout(&output.stdout))
    {
        ("fail", Some("stdout mismatch".to_owned()))
    } else if expected.stderr.is_some() && output.stderr != expected_stderr.unwrap_or_default() {
        ("fail", Some("stderr mismatch".to_owned()))
    } else if expected
        .stdout_contains
        .as_ref()
        .is_some_and(|fragment| !String::from_utf8_lossy(&output.stdout).contains(fragment))
    {
        ("fail", Some("stdout diagnostic mismatch".to_owned()))
    } else if expected
        .stderr_contains
        .as_ref()
        .is_some_and(|fragment| !String::from_utf8_lossy(&output.stderr).contains(fragment))
    {
        ("fail", Some("stderr diagnostic mismatch".to_owned()))
    } else {
        ("pass", None)
    }
}

fn judge_known_fail(
    case: &Case,
    expected_stdout: Option<Vec<u8>>,
    output: &Output,
) -> (&'static str, Option<String>) {
    if case.target.failure_kind.as_deref() == Some("stdout-mismatch") {
        let expected = match case.expected() {
            Ok(expected) => expected,
            Err(reason) => return ("fail", Some(reason)),
        };
        let differs = expected_stdout
            .as_deref()
            .is_some_and(|expected| normalize_stdout(expected) != normalize_stdout(&output.stdout));
        if !exit_matches(expected, &output.status) {
            (
                "fail",
                Some(format!(
                    "exit mismatch: expected {}, got {}",
                    expected.exit, output.status
                )),
            )
        } else if differs {
            ("expected-fail", None)
        } else {
            (
                "fail",
                Some("unexpected output match; update target expectation".to_owned()),
            )
        }
    } else {
        let fragment = match case.known_fragment() {
            Ok(fragment) => fragment,
            Err(reason) => return ("fail", Some(reason)),
        };
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
}

fn exit_matches(expected: &Expected, status: &std::process::ExitStatus) -> bool {
    if expected.exit_nonzero {
        !status.success()
    } else {
        status.code() == Some(expected.exit)
    }
}

pub(super) fn normalize_stdout(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .replace("\r\n", "\n")
        .trim_end_matches('\n')
        .to_owned()
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
