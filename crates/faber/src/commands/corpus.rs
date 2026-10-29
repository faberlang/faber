//! Verify explicit corpus cases against a selected execution target.

#[cfg(feature = "hir-rust")]
mod canonical;
#[cfg(feature = "mir-clif")]
mod clif_jit;
mod external;
#[cfg(feature = "mir-llvm")]
mod gpu;
#[cfg(test)]
#[path = "corpus_runner_test.rs"]
mod runner_test;
#[cfg(feature = "hir-rust")]
mod rust;
mod selection;
#[cfg(test)]
#[path = "corpus/selection_test.rs"]
mod selection_test;

use crate::cli::CorpusArgs;
use faber_cli::exit_status;
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
use std::sync::atomic::{AtomicUsize, Ordering};
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
    /// Domain name to its categories; the partition `--domain`/`--category` select by.
    #[serde(default)]
    domains: BTreeMap<String, Vec<String>>,
    /// Category of each product package, which has no corpus frontmatter. The
    /// generator reads it; the loader only checks the categories are in the table.
    #[serde(default)]
    product_categories: BTreeMap<String, String>,
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
    /// Run command for a package case whose `faber build` printed a linked
    /// artifact; without it the case runs `run` like a single-file emit.
    #[serde(default)]
    package_run: Vec<String>,
    /// Build every input, single files included, through the product command
    /// `faber build -t <emit>` (one step that emits, links and prints the
    /// linked artifact), then run that artifact with `run`. The lane then
    /// exercises the same code as a user's build instead of a hand-written
    /// emit-and-link recipe.
    #[serde(default)]
    product: bool,
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
    domains: Option<selection::DomainTable>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaseManifest {
    version: u32,
    id: Option<String>,
    group: Option<String>,
    category: Option<String>,
    domain: Option<String>,
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
    category: Option<String>,
    domain: Option<String>,
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
    // until that descendant exits on its own. A parent that only observes the exit
    // after the deadline still needs a fresh grace floor for in-flight drain sends;
    // with the deadline still ahead of that floor the bound is unchanged.
    let mut parts = [None, None];
    let collected = deadline.max(Instant::now() + DRAIN_GRACE);
    if collect_drains(&receiver, &mut parts, collected) {
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
///
/// The receiver is polled before the clock: a buffered result must never be
/// discarded because the parent was scheduled late. `recv_timeout` with a zero
/// remaining bound still delivers a buffered message, so results the drains
/// already sent are collected even after the deadline passed, while a stream
/// that has genuinely not reached EOF still times out.
fn collect_drains(
    receiver: &Receiver<(usize, Vec<u8>)>,
    parts: &mut [Option<Vec<u8>>; 2],
    deadline: Instant,
) -> bool {
    while parts[0].is_none() || parts[1].is_none() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(remaining) {
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
            fail_usage("--expected applies only to a single input");
        }
        let mut suite = load_suite(&input, &args.target).unwrap_or_else(|error| error.exit());
        let selectors = selection::Selectors {
            domains: args.domain.clone(),
            categories: args.category.clone(),
            cases: args.case.clone(),
            filter: args.filter.clone(),
        };
        suite.cases = selection::select(suite.cases, suite.domains.as_ref(), &selectors)
            .unwrap_or_else(|error| match error {
                selection::SelectionError::Usage(message)
                | selection::SelectionError::Empty(message) => fail_usage(&message),
            });
        suite
    } else {
        let named = [
            ("--filter", args.filter.is_some()),
            ("--domain", !args.domain.is_empty()),
            ("--category", !args.category.is_empty()),
            ("--case", !args.case.is_empty()),
        ];
        if let Some((flag, _)) = named.iter().find(|(_, given)| *given) {
            fail_usage(&format!("{flag} applies only to a suite"));
        }
        // The built-in executors (`runner`, `clif-jit`) run the input through
        // this faber binary itself; every other target is recipe-driven.
        if !matches!(args.target.as_str(), "runner" | "clif-jit") {
            fail_usage("non-runner targets require a suite with a target recipe");
        }
        LoadedSuite {
            cases: vec![
                single_case(&input, args.expected.as_deref()).unwrap_or_else(|error| error.exit()),
            ],
            recipe: None,
            domains: None,
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
                    .unwrap_or_else(|| fail_config("rust target requires [targets.rust] runtime")),
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
                        fail_config("canonical target requires [targets.canonical] runtime")
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
    if args.target == "clif-jit" {
        #[cfg(not(feature = "mir-clif"))]
        fail_unavailable("clif-jit corpus target requires a faber build with feature `mir-clif`");
        #[cfg(feature = "mir-clif")]
        {
            clif_jit::execute(&suite.cases, &faber, discover_ms, started);
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
                .unwrap_or_else(|| fail_config("external target requires a suite target recipe")),
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
    // WHY: each case is an independent `faber run` child, so the suite is bound by
    // the sum of its children rather than by this process. The launcher takes a
    // small fixed share of the host: five concurrent lanes already saturate the
    // machine, and a wider fan-out only adds contention. Rows are collected by
    // case index and printed in suite order, so the JSONL stream stays stable.
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(4)
        .min(suite.cases.len().max(1));
    let next = AtomicUsize::new(0);
    let mut rows = Vec::with_capacity(suite.cases.len());
    thread::scope(|scope| {
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            let next = &next;
            let cases = &suite.cases;
            let faber = &faber;
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
    // WHY: harnesses and unit proofs grep leg logs for `failed: <n>`; the
    // compact JSON summary cannot carry that literal. stdout is a JSON-lines
    // stream that parsers consume line by line, so the human summary goes to
    // stderr and stdout stays pure JSON.
    eprintln!(
        "corpus {} summary: {} case(s): {passed} passed, {rejected} rejected, {expected_failures} expected-fail, {not_applicable} not-applicable, failed: {failed} [{status}]",
        args.target,
        suite.cases.len()
    );
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

fn single_case(input: &Path, explicit: Option<&Path>) -> Result<Case, SuiteError> {
    let stdout = if let Some(path) = explicit {
        package::absolutize_path(path)
    } else if input.is_file() && input.extension().is_some_and(|ext| ext == "fab") {
        input.with_extension("expected")
    } else {
        package::discover_package(input)
            .map_err(|diagnostic| {
                SuiteError::with_status(
                    exit_status::status_for_diagnostic(&diagnostic),
                    diagnostic.message,
                )
            })?
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
        category: None,
        domain: None,
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

fn load_suite(root: &Path, target: &str) -> Result<LoadedSuite, SuiteError> {
    let manifest_path = root.join("corpus.toml");
    let text = fs::read_to_string(&manifest_path).map_err(|error| {
        SuiteError::with_status(
            exit_status::NO_INPUT,
            format!("cannot read {}: {error}", manifest_path.display()),
        )
    })?;
    let suite: SuiteManifest = toml::from_str(&text)
        .map_err(|error| format!("invalid {}: {error}", manifest_path.display()))?;
    if suite.version != 1 {
        return Err(format!("unsupported corpus.toml version {}", suite.version).into());
    }
    // A target the suite declares no recipe for is a bad invocation of a valid
    // suite, found before any case is read: every other target needs a recipe.
    // The `clif-jit` lane, like `runner`, is a built-in executor that needs no
    // recipe — it runs each case through this faber's own JIT.
    if !matches!(target, "runner" | "clif-jit") && !suite.targets.contains_key(target) {
        return Err(SuiteError::with_status(
            exit_status::USAGE,
            format!("suite has no [targets.{target}] recipe"),
        ));
    }
    let roots = resolve_suite_roots(root, suite.roots)?;
    let cases_root = relative_path(root, &suite.cases_dir)?;
    if !cases_root.is_dir() {
        return Err(format!("cases directory is missing: {}", cases_root.display()).into());
    }
    let mut manifests = Vec::new();
    find_cases(&cases_root, &mut manifests)?;
    manifests.sort();
    if manifests.is_empty() {
        return Err(format!("no case.toml files under {}", cases_root.display()).into());
    }
    let mut cases = manifests
        .iter()
        .map(|path| load_case(path, &cases_root, target, &roots, &suite.expectations))
        .collect::<Result<Vec<_>, _>>()?;
    let domains = selection::DomainTable::from_manifest(suite.domains)
        .map_err(|message| format!("invalid {}: {message}", manifest_path.display()))?;
    for (case_id, category) in &suite.product_categories {
        if !domains
            .as_ref()
            .is_some_and(|table| table.has_category(category))
        {
            return Err(format!(
                "invalid {}: product_categories {case_id}: category {category:?} is in no domain",
                manifest_path.display()
            )
            .into());
        }
    }
    selection::DomainTable::tag_cases(domains.as_ref(), &mut cases)?;
    let recipe = if matches!(target, "runner" | "clif-jit") {
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
            return Err("gpu target requires [targets.gpu] mode = \"gpu\""
                .to_owned()
                .into());
        }
        if matches!(target, "rust" | "canonical") && suite.embedded_only {
            if recipe.runtime.is_some() {
                return Err(format!(
                    "embedded_only suite must not set [targets.{target}] runtime in corpus.toml"
                )
                .into());
            }
            let path = package::embedded_runtime_root()
                .ok_or_else(|| {
                    SuiteError::with_status(
                        exit_status::UNAVAILABLE,
                        format!(
                        "embedded_only {target} target requires an embedded runtime, which is unavailable"
                    ),
                    )
                })?
                .join("runtime/rust");
            if !path.join("Cargo.toml").is_file() {
                return Err(format!(
                    "embedded_only embedded runtime is missing Cargo.toml: {}",
                    path.display()
                )
                .into());
            }
            recipe.runtime = Some(path);
        } else {
            if matches!(target, "rust" | "canonical") && recipe.runtime.is_none() {
                return Err(format!(
                    "{target} target requires [targets.{target}] runtime in corpus.toml"
                )
                .into());
            }
            if let Some(configured) = recipe.runtime.as_deref() {
                let path = if configured.is_absolute() {
                    configured.to_path_buf()
                } else {
                    root.join(configured)
                };
                if !path.is_dir() {
                    return Err(format!("target runtime is missing: {}", path.display()).into());
                }
                if matches!(target, "rust" | "canonical") && !path.join("Cargo.toml").is_file() {
                    return Err(format!("Rust runtime crate is missing: {}", path.display()).into());
                }
                recipe.runtime = Some(path.canonicalize().map_err(|error| {
                    SuiteError::with_status(
                        exit_status::NO_INPUT,
                        format!("cannot resolve target runtime {}: {error}", path.display()),
                    )
                })?);
            }
        }
        Some(recipe)
    } else {
        None
    };
    Ok(LoadedSuite {
        cases,
        recipe,
        domains,
    })
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

fn find_cases(dir: &Path, manifests: &mut Vec<PathBuf>) -> Result<(), SuiteError> {
    let marker = dir.join("case.toml");
    if marker.is_file() {
        manifests.push(marker);
        return Ok(());
    }
    let unreadable = |message: String| SuiteError::with_status(exit_status::NO_INPUT, message);
    for entry in fs::read_dir(dir)
        .map_err(|error| unreadable(format!("cannot read {}: {error}", dir.display())))?
    {
        let entry =
            entry.map_err(|error| unreadable(format!("cannot read case entry: {error}")))?;
        let file_type = entry
            .file_type()
            .map_err(|error| unreadable(format!("cannot read case type: {error}")))?;
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
            )
            .into());
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
) -> Result<Case, SuiteError> {
    let text = fs::read_to_string(manifest_path).map_err(|error| {
        SuiteError::with_status(
            exit_status::NO_INPUT,
            format!("cannot read {}: {error}", manifest_path.display()),
        )
    })?;
    Ok(parse_case(
        &text,
        manifest_path,
        cases_root,
        target,
        roots,
        expectations,
    )?)
}

fn parse_case(
    text: &str,
    manifest_path: &Path,
    cases_root: &Path,
    target: &str,
    roots: &BTreeMap<String, PathBuf>,
    expectations: &BTreeMap<String, BTreeMap<String, TargetExpectation>>,
) -> Result<Case, String> {
    let mut manifest: CaseManifest = toml::from_str(text)
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
    let expectation = manifest
        .targets
        .remove(target)
        .or_else(|| {
            // The `clif-jit` lane judges the runner's own expectations: a case
            // without a `clif-jit` pin falls back to its `runner` pin.
            (target == "clif-jit")
                .then(|| manifest.targets.remove("runner"))
                .flatten()
        })
        .or_else(|| {
            manifest
                .group
                .as_deref()
                .and_then(|group| expectations.get(group))
                .and_then(|group| {
                    group.get(target).or_else(|| {
                        (target == "clif-jit")
                            .then(|| group.get("runner"))
                            .flatten()
                    })
                })
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
                || (runs_program_in_memory(target)
                    && expectation.phase.as_deref() != Some("run"))
                || !matches!(
                    expectation.failure_kind.as_deref().unwrap_or("diagnostic"),
                    "diagnostic" | "stdout-mismatch" | "nonzero-exit" | "exit-mismatch"
                )
                || (expectation.failure_kind.as_deref() == Some("stdout-mismatch")
                    && (expectation.phase.as_deref() != Some("run")
                        || expect.as_ref().is_none_or(|item| item.stdout.is_none())))
                || (expectation.failure_kind.as_deref() == Some("nonzero-exit")
                    && (expectation.phase.as_deref() != Some("run")
                        || expect.as_ref().is_none_or(|item| !item.exit_nonzero)))
                || (expectation.failure_kind.as_deref() == Some("exit-mismatch")
                    && (expectation.phase.as_deref() != Some("run")
                        || expect
                            .as_ref()
                            .is_none_or(|item| item.exit == 0 || item.exit_nonzero)))
                || (!matches!(
                    expectation.failure_kind.as_deref(),
                    Some("stdout-mismatch" | "nonzero-exit" | "exit-mismatch")
                ) && expectation
                    .diagnostic_contains
                    .as_ref()
                    .is_none_or(String::is_empty)) =>
        {
            return Err(format!(
                "{id}: known-fail requires a supported phase and a stable diagnostic, stdout-mismatch, nonzero-exit or exit-mismatch oracle"
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
        category: manifest.category.take(),
        domain: manifest.domain.take(),
        dir: dir.to_path_buf(),
        input,
        args: expectation.args.clone().unwrap_or(manifest.args),
        stdin,
        expect,
        target: expectation,
        timeout,
    })
}

/// Targets whose corpus lane runs the case's program in memory through this
/// faber binary: the interpreter (`runner`) and the Cranelift JIT
/// (`clif-jit`), which judges the runner's own expectations. Both take
/// runner-shaped pins: a run-phase outcome judged from the run itself.
fn runs_program_in_memory(target: &str) -> bool {
    matches!(target, "runner" | "clif-jit")
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
        ("runner" | "clif-jit", Outcome::KnownFail)
            if matches!(kind, "nonzero-exit" | "exit-mismatch") =>
        {
            unsupported(&format!("failure_kind {kind}"))
        }
        ("runner" | "clif-jit", Outcome::Pass) if !matches!(phase, None | Some("run")) => {
            unsupported("a passing case with a phase other than run")
        }
        ("canonical" | "gpu", Outcome::KnownFail) if kind != "diagnostic" => unsupported(&format!(
            "failure_kind {kind}; only diagnostic is supported"
        )),
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
/// The runner target pins `--device reference`: it verifies the sequential MIR
/// runner, so device selection must not depend on the host (`auto` routes kernel
/// programs to Metal on a capable machine and fails closed elsewhere, and `cpu`
/// is the multi-worker CPU device).
fn runner_run_args(case: &Case) -> Vec<String> {
    let mut args = vec![
        "run".to_owned(),
        case.input.display().to_string(),
        "--device".to_owned(),
        "reference".to_owned(),
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
    // A library-shaped case has no entry function, so the runner has nothing to
    // start from. Its standard is the runner's equivalent of "it builds": the
    // program checks and lowers to valid MIR, judged from `check --lower`.
    let library = if is_library_shaped(case, &output) {
        let lower_args = runner_lower_args(case);
        let mut lower = Command::new(faber);
        lower.args(&lower_args).current_dir(&case.dir);
        Some(match run_bounded(&mut lower, Stdio::null(), case.timeout) {
            Ok(lowered) => (lower_args, lowered),
            Err(error) => return case_error(case, error.describe("runner"), started),
        })
    } else {
        None
    };
    let (status, reason) = match (&library, outcome) {
        (Some((_, lowered)), Judged::Pass) => judge_lower_pass(lowered),
        (Some((_, lowered)), Judged::KnownFail) => {
            let (status, reason) = judge_known_fail(case, expected_stdout, lowered);
            (case.relabel_expected_fail(status), reason)
        }
        (None, Judged::Pass) => judge_pass(case, expected_stdout, expected_stderr, &output),
        (None, Judged::KnownFail) => {
            let (status, reason) = judge_known_fail(case, expected_stdout, &output);
            (case.relabel_expected_fail(status), reason)
        }
    };
    let compare_ms = ms(comparing);
    let mut row = json!({
        "schema": 1,
        "type": "case",
        "case": case.id,
        "target": "runner",
        "status": status,
        "reason": reason,
        "command": {"argv": argv, "cwd": case.dir},
        "exit": output.status.code(),
        "timing_ms": {"read": read_ms, "run": run_ms, "compare": compare_ms, "total": ms(started)}
    });
    if let Some((lower_args, lowered)) = &library {
        let mut lower_argv = vec![faber.display().to_string()];
        lower_argv.extend(lower_args.iter().cloned());
        row["phase"] = json!("lower");
        row["lower_command"] = json!({"argv": lower_argv, "cwd": case.dir});
        row["lower_exit"] = json!(lowered.status.code());
    }
    row
}

/// Arguments after the `faber` executable for the library-shaped standard: check the
/// program and lower it to validated MIR without executing anything.
fn runner_lower_args(case: &Case) -> Vec<String> {
    vec![
        "check".to_owned(),
        "--lower".to_owned(),
        case.input.display().to_string(),
    ]
}

/// True when `run` refused the case only because the program has no entry function
/// and the case asserts no output (`check_only`), so there is nothing to execute and
/// nothing to compare. A rejection pin is never library-shaped: it expects `run` to
/// fail for its own reason.
fn is_library_shaped(case: &Case, output: &Output) -> bool {
    !case.target.rejects
        && case.expect.as_ref().is_some_and(|item| item.check_only)
        && !output.status.success()
        && String::from_utf8_lossy(&output.stderr).contains(radix_mir::names::NO_ENTRY_FUNCTION)
}

/// The library-shaped standard: a case that checks must also lower. `check --lower`
/// exits zero only when analysis passed and the MIR validator accepted the lowering.
fn judge_lower_pass(lowered: &Output) -> (&'static str, Option<String>) {
    if lowered.status.success() {
        ("pass", None)
    } else {
        (
            "fail",
            Some("library-shaped case checks but does not lower to valid MIR".to_owned()),
        )
    }
}

fn judge_output_expectations(
    expected: &Expected,
    expected_stdout: Option<&[u8]>,
    expected_stderr: Option<&[u8]>,
    output: &Output,
) -> Option<&'static str> {
    if expected_stdout
        .is_some_and(|expected| normalize_stdout(expected) != normalize_stdout(&output.stdout))
    {
        Some("stdout mismatch")
    } else if expected.stderr.is_some()
        && output.stderr.as_slice() != expected_stderr.unwrap_or_default()
    {
        Some("stderr mismatch")
    } else if expected
        .stdout_contains
        .as_ref()
        .is_some_and(|fragment| !String::from_utf8_lossy(&output.stdout).contains(fragment))
    {
        Some("stdout diagnostic mismatch")
    } else if expected
        .stderr_contains
        .as_ref()
        .is_some_and(|fragment| !String::from_utf8_lossy(&output.stderr).contains(fragment))
    {
        Some("stderr diagnostic mismatch")
    } else {
        None
    }
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
    } else if let Some(reason) = judge_output_expectations(
        expected,
        expected_stdout.as_deref(),
        expected_stderr.as_deref(),
        output,
    ) {
        ("fail", Some(reason.to_owned()))
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

/// Judge a known-fail pin whose oracle is the program's own run (`stdout-mismatch`,
/// `nonzero-exit`, `exit-mismatch`) rather than a diagnostic fragment. `None` means the pin is a
/// diagnostic pin and the caller judges it. Shared by every executor that runs the
/// built program, so one definition decides what each run-phase kind means.
fn judge_known_run_kind(case: &Case, output: &Output) -> Option<(&'static str, Option<String>)> {
    match case.target.failure_kind.as_deref() {
        Some("stdout-mismatch") => {
            let expected = match case.expected() {
                Ok(expected) => expected,
                Err(reason) => return Some(("fail", Some(reason))),
            };
            if !exit_matches(expected, &output.status) {
                return Some((
                    "fail",
                    Some(format!(
                        "exit mismatch: expected {}, got {}",
                        expected.exit, output.status
                    )),
                ));
            }
            let Some(path) = expected.stdout.as_ref() else {
                return Some((
                    "fail",
                    Some(format!("{}: stdout-mismatch has no stdout golden", case.id)),
                ));
            };
            let golden = match fs::read(path) {
                Ok(bytes) => bytes,
                Err(error) => {
                    return Some(("fail", Some(format!("cannot read stdout golden: {error}"))));
                }
            };
            Some(
                if normalize_stdout(&golden) != normalize_stdout(&output.stdout) {
                    ("expected-fail", None)
                } else {
                    (
                        "fail",
                        Some("unexpected output match; update target expectation".to_owned()),
                    )
                },
            )
        }
        Some("nonzero-exit") => {
            let expected = match case.expected() {
                Ok(expected) => expected,
                Err(reason) => return Some(("fail", Some(reason))),
            };
            Some(if exit_matches(expected, &output.status) {
                ("expected-fail", None)
            } else {
                (
                    "fail",
                    Some(format!(
                        "known nonzero exit changed: expected nonzero, got {}",
                        output.status
                    )),
                )
            })
        }
        Some("exit-mismatch") => {
            let expected = match case.expected() {
                Ok(expected) => expected,
                Err(reason) => return Some(("fail", Some(reason))),
            };
            if exit_matches(expected, &output.status) {
                return Some((
                    "fail",
                    Some("unexpected exit match; update target expectation".to_owned()),
                ));
            }
            let stdout = match expected.stdout.as_ref().map(fs::read).transpose() {
                Ok(bytes) => bytes,
                Err(error) => {
                    return Some(("fail", Some(format!("cannot read stdout golden: {error}"))));
                }
            };
            let stderr = match expected.stderr.as_ref().map(fs::read).transpose() {
                Ok(bytes) => bytes,
                Err(error) => {
                    return Some(("fail", Some(format!("cannot read stderr golden: {error}"))));
                }
            };
            if let Some(reason) =
                judge_output_expectations(expected, stdout.as_deref(), stderr.as_deref(), output)
            {
                return Some(("fail", Some(reason.to_owned())));
            }
            Some(("expected-fail", None))
        }
        _ => None,
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

/// A corpus suite that could not be loaded, and the exit status that names
/// why: a bad `corpus.toml` or case file is [`exit_status::CONFIG`] (the
/// default for a message), an unreadable one [`exit_status::NO_INPUT`], a
/// target the suite has no recipe for [`exit_status::USAGE`].
struct SuiteError {
    status: i32,
    message: String,
}

impl SuiteError {
    fn with_status(status: i32, message: String) -> Self {
        Self { status, message }
    }

    fn exit(&self) -> ! {
        fail_with(self.status, &self.message)
    }
}

impl std::fmt::Display for SuiteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl From<String> for SuiteError {
    fn from(message: String) -> Self {
        Self::with_status(exit_status::CONFIG, message)
    }
}

/// Print the corpus error row on stdout (the JSON-lines stream stays
/// parseable) and exit with `status`.
fn fail_with(status: i32, message: &str) -> ! {
    println!(
        "{}",
        json!({"schema": 1, "type": "error", "message": message})
    );
    std::process::exit(status);
}

/// A failure with no known class.
fn fail(message: &str) -> ! {
    fail_with(exit_status::FAILURE, message)
}

/// A bad corpus invocation: an option or target the suite cannot serve.
fn fail_usage(message: &str) -> ! {
    fail_with(exit_status::USAGE, message)
}

/// A bad suite, case or target recipe configuration.
fn fail_config(message: &str) -> ! {
    fail_with(exit_status::CONFIG, message)
}

/// A feature, executor or tool this binary or host does not have.
fn fail_unavailable(message: &str) -> ! {
    fail_with(exit_status::UNAVAILABLE, message)
}

/// A corpus workspace or staged artifact that cannot be written.
fn fail_cant_create(message: &str) -> ! {
    fail_with(exit_status::CANT_CREATE, message)
}

/// An internal inconsistency in the corpus run itself.
fn fail_software(message: &str) -> ! {
    fail_with(exit_status::SOFTWARE, message)
}
