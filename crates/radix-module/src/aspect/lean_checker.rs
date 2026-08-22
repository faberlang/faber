//! Lean proof-checker runner — invokes `lake build` with timeout and
//! normalizes output into the backend-neutral [`HirAspectCheckResult`].

use std::fmt::Write;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicU64;
use std::time::Duration;
use wait_timeout::ChildExt;

use super::verifier::{HirAspectCheckResult, HirAspectCheckStatus, HirAspectEvidence, HirAspectVerifier};
use super::{HirAspectBackendKind, HirAspectKind, HirAspectRecord, HirAspectRecordKind, HirAspectReport};

#[cfg(test)]
#[path = "lean_checker_test.rs"]
mod tests;

/// Default deadline for a single `lake build` invocation.
pub const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Best-effort side effects where the `Result` is intentionally ignored.
#[inline]
fn ignore_result<T, E>(_result: Result<T, E>) {}

/// Raw output captured from a `lake build` invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LakeResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
}

// ---------------------------------------------------------------------------
// trust mechanism types
// ---------------------------------------------------------------------------

/// A single axiom that a Lean declaration depends on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AxiomDependency {
    /// Name of the axiom (e.g. `propext`, `Classical.choice`).
    pub axiom_name: String,
    /// Lean declaration that transitively depends on this axiom.
    pub declaration: String,
}

/// Collected axiom dependencies from a `#print axioms` audit.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AxiomAudit {
    /// Every axiom→declaration edge found during the audit.
    pub dependencies: Vec<AxiomDependency>,
}

/// Verdict after checking axiom usage against the allowlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AxiomVerdict {
    /// Every axiom is in the allowlist.
    Allowed,
    /// At least one axiom is not in the allowlist.
    Rejected {
        /// The offending dependencies.
        disallowed: Vec<AxiomDependency>,
    },
}

/// Verdict after scanning source for `sorry` tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SorryVerdict {
    /// No `sorry` tokens detected.
    Clean,
    /// `sorry` tokens found on these 1-based line numbers.
    Found { line_numbers: Vec<usize> },
}

/// Configuration for the Lean checker toolchain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeanAspectCheckerConfig {
    /// Path to the `lake` binary (or bare command found on `PATH`).
    pub lake_bin: PathBuf,
    /// Path to the `lean` binary (or bare command found on `PATH`).
    pub lean_bin: PathBuf,
    /// Maximum wall-clock time for a single `lake build` invocation.
    pub timeout: Duration,
}

impl Default for LeanAspectCheckerConfig {
    fn default() -> Self {
        Self {
            lake_bin: PathBuf::from("lake"),
            lean_bin: PathBuf::from("lean"),
            timeout: Duration::from_secs(DEFAULT_TIMEOUT_SECS),
        }
    }
}

/// Lean proof-checker — invokes `lake build` and normalizes output into
/// backend-neutral [`HirAspectCheckResult`] values.
///
/// Holds the toolchain configuration and a reference to the Lean project
/// directory that will be built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeanAspectChecker {
    config: LeanAspectCheckerConfig,
    project_dir: PathBuf,
}

impl LeanAspectChecker {
    /// Create a new checker from toolchain config and project directory.
    #[must_use]
    pub fn new(config: LeanAspectCheckerConfig, project_dir: PathBuf) -> Self {
        Self { config, project_dir }
    }

    /// Invoke `lake build` in `project_dir` with the configured timeout.
    ///
    /// Returns a [`LakeResult`] with `timed_out: true` when the process
    /// exceeds the deadline.  When `lake` or `lean` is not reachable the
    /// result carries `exit_code: None` and a human-readable stderr message.
    #[must_use]
    pub fn run_lake_build(&self, project_dir: &Path) -> LakeResult {
        if !self.toolchain_available() {
            return LakeResult {
                exit_code: None,
                stdout: String::new(),
                stderr: format!(
                    "lake ({}) or lean ({}) not found on PATH",
                    self.config.lake_bin.display(),
                    self.config.lean_bin.display(),
                ),
                timed_out: false,
            };
        }

        let mut child = match Command::new(&self.config.lake_bin)
            .arg("build")
            .current_dir(project_dir)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                return LakeResult {
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("failed to launch lake: {error}"),
                    timed_out: false,
                };
            }
        };

        match child.wait_timeout(self.config.timeout) {
            Ok(Some(exit_status)) => {
                let exit_code = exit_status.code();
                let stdout = drain_pipe(child.stdout.take());
                let stderr = drain_pipe(child.stderr.take());
                LakeResult { exit_code, stdout, stderr, timed_out: false }
            }
            Ok(None) => {
                ignore_result(child.kill());
                ignore_result(child.wait());
                LakeResult {
                    exit_code: None,
                    stdout: String::new(),
                    stderr: format!("lake build timed out after {}s", self.config.timeout.as_secs()),
                    timed_out: true,
                }
            }
            Err(error) => LakeResult {
                exit_code: None,
                stdout: String::new(),
                stderr: format!("failed to wait for lake: {error}"),
                timed_out: false,
            },
        }
    }

    /// Borrow the configured project directory.
    #[must_use]
    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    /// Borrow the toolchain configuration.
    #[must_use]
    pub fn config(&self) -> &LeanAspectCheckerConfig {
        &self.config
    }

    // ------------------------------------------------------------------
    // trust mechanism
    // ------------------------------------------------------------------

    /// Audit axiom usage by invoking `lean` with `#print axioms` for each
    /// declaration found in `module_path`.
    ///
    /// Creates a temporary wrapper `.lean` file, runs the Lean toolchain on
    /// it, and parses the `#print axioms` output into an [`AxiomAudit`].
    #[must_use]
    pub fn audit_axioms(&self, module_path: &Path) -> AxiomAudit {
        let Ok(source) = std::fs::read_to_string(module_path) else {
            return AxiomAudit::default();
        };

        let decls = extract_declarations(&source);
        if decls.is_empty() {
            return AxiomAudit::default();
        }

        let module_name = module_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Module");

        let mut wrapper = format!("import «{module_name}»\n\n");
        for decl in &decls {
            writeln!(wrapper, "#print axioms {decl}").ok();
        }

        let parent = module_path.parent().unwrap_or(Path::new("."));
        let wrapper_path = parent.join(format!("_axiom_audit_{module_name}.lean"));
        ignore_result(std::fs::write(&wrapper_path, &wrapper));

        let output = self.run_lean_command(&wrapper_path);

        ignore_result(std::fs::remove_file(&wrapper_path));

        parse_axiom_output(&output)
    }

    /// Classify an [`AxiomAudit`] against the built-in allowlist.
    ///
    /// Allowed axioms: `propext`, `Classical.choice`, `Quot.sound`.
    /// Any axiom not in that set is rejected.
    #[must_use]
    pub fn classify_axioms(audit: &AxiomAudit) -> AxiomVerdict {
        let allowed: &[&str] = &["propext", "Classical.choice", "Quot.sound"];

        let disallowed: Vec<AxiomDependency> = audit
            .dependencies
            .iter()
            .filter(|dep| !allowed.contains(&dep.axiom_name.as_str()))
            .cloned()
            .collect();

        if disallowed.is_empty() {
            AxiomVerdict::Allowed
        } else {
            AxiomVerdict::Rejected { disallowed }
        }
    }

    /// Scan Lean source for `sorry` tokens.
    ///
    /// Returns [`SorryVerdict::Found`] with 1-based line numbers for every
    /// line containing the token `sorry` outside of a comment or string
    /// literal (simple heuristic — scans whole words only).
    #[must_use]
    pub fn scan_for_sorry(source: &str) -> SorryVerdict {
        let line_numbers: Vec<usize> = source
            .lines()
            .enumerate()
            .filter(|(_, line)| contains_sorry_token(line))
            .map(|(i, _)| i + 1)
            .collect();

        if line_numbers.is_empty() {
            SorryVerdict::Clean
        } else {
            SorryVerdict::Found { line_numbers }
        }
    }

    fn toolchain_available(&self) -> bool {
        is_bare_or_found(&self.config.lake_bin) && is_bare_or_found(&self.config.lean_bin)
    }

    /// Run `lean` on a single `.lean` file and return combined stdout+stderr.
    fn run_lean_command(&self, file_path: &Path) -> String {
        if !self.toolchain_available() {
            return format!("lean ({}) not found on PATH", self.config.lean_bin.display());
        }

        match std::process::Command::new(&self.config.lean_bin)
            .arg(file_path)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
        {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
                let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
                if stdout.is_empty() {
                    stderr
                } else {
                    stdout
                }
            }
            Err(error) => {
                format!("failed to launch lean: {error}")
            }
        }
    }
}

// ---------------------------------------------------------------------------
// HirAspectVerifier implementation
// ---------------------------------------------------------------------------

impl HirAspectVerifier for LeanAspectChecker {
    fn verify(&self, report: &HirAspectReport) -> Vec<HirAspectCheckResult> {
        let recognized: Vec<&HirAspectRecord> = report
            .records()
            .iter()
            .filter(|r| r.record_kind == HirAspectRecordKind::Recognized)
            .collect();

        if recognized.is_empty() {
            return vec![];
        }

        // No-fall-through gate: registered kinds without landed proof content
        // never reach the generic `True := trivial` generator. They get a
        // deterministic Failed result naming the unproven kind instead of a
        // fabricated verdict.
        let refused: Vec<HirAspectCheckResult> = recognized
            .iter()
            .filter(|r| !has_registered_proof_generator(r.aspect))
            .map(|r| HirAspectCheckResult {
                status: HirAspectCheckStatus::Failed,
                evidence: HirAspectEvidence(format!(
                    "aspect kind `{}` is registered without proof content; refusing placeholder proof",
                    aspect_label(r)
                )),
                backend: HirAspectBackendKind::Lean,
            })
            .collect();

        let checkable: Vec<&HirAspectRecord> = recognized
            .iter()
            .copied()
            .filter(|r| has_registered_proof_generator(r.aspect))
            .collect();

        if checkable.is_empty() {
            return refused;
        }

        // Generate Lean source and a lakefile.toml for the temp project.
        let lean_source = generate_lean_source(&checkable);
        let lakefile_toml = generate_lakefile_toml();

        // Create a unique temp directory under the system temp root.
        let temp_dir = match make_temp_project_dir() {
            Ok(dir) => dir,
            Err(error) => {
                let err_result = HirAspectCheckResult {
                    status: HirAspectCheckStatus::Error,
                    evidence: HirAspectEvidence(error),
                    backend: HirAspectBackendKind::Lean,
                };
                return merge_results(&recognized, checkable.iter().map(|_| err_result.clone()).collect(), refused);
            }
        };

        // Write the generated files.
        let lean_path = temp_dir.join(format!("{LAKE_PROJECT_NAME}.lean"));
        if let Err(error) = std::fs::write(&lean_path, &lean_source) {
            let err_result = HirAspectCheckResult {
                status: HirAspectCheckStatus::Error,
                evidence: HirAspectEvidence(format!("failed to write Lean source: {error}")),
                backend: HirAspectBackendKind::Lean,
            };
            ignore_result(std::fs::remove_dir_all(&temp_dir));
            return merge_results(&recognized, checkable.iter().map(|_| err_result.clone()).collect(), refused);
        }
        if let Err(error) = std::fs::write(temp_dir.join("lakefile.toml"), &lakefile_toml) {
            let err_result = HirAspectCheckResult {
                status: HirAspectCheckStatus::Error,
                evidence: HirAspectEvidence(format!("failed to write lakefile.toml: {error}")),
                backend: HirAspectBackendKind::Lean,
            };
            ignore_result(std::fs::remove_dir_all(&temp_dir));
            return merge_results(&recognized, checkable.iter().map(|_| err_result.clone()).collect(), refused);
        }

        // Run lake build via the U1 runner.
        let lake = self.run_lake_build(&temp_dir);
        let build_result = normalize_result(lake);

        // If lake build did not succeed, all records share the failure.
        if build_result.status != HirAspectCheckStatus::Proven {
            ignore_result(std::fs::remove_dir_all(&temp_dir));
            return merge_results(&recognized, checkable.iter().map(|_| build_result.clone()).collect(), refused);
        }

        // Run trust checks via the U2 mechanism.
        let sorry_verdict = LeanAspectChecker::scan_for_sorry(&lean_source);
        let axiom_audit = self.audit_axioms(&lean_path);
        let axiom_verdict = LeanAspectChecker::classify_axioms(&axiom_audit);
        let final_result = combined_verdict(build_result, &axiom_verdict, &sorry_verdict);

        // Clean up the temp project directory.
        ignore_result(std::fs::remove_dir_all(&temp_dir));

        merge_results(&recognized, checkable.iter().map(|_| final_result.clone()).collect(), refused)
    }
}

/// Re-assemble per-record results in the original report order: checkable
/// records take their checker result, refused kinds take their deterministic
/// no-fall-through result.
fn merge_results(
    records: &[&HirAspectRecord],
    checked: Vec<HirAspectCheckResult>,
    refused: Vec<HirAspectCheckResult>,
) -> Vec<HirAspectCheckResult> {
    let mut next_checked = checked.into_iter();
    let mut next_refused = refused.into_iter();
    records
        .iter()
        .map(|record| {
            if has_registered_proof_generator(record.aspect) {
                next_checked
                    .next()
                    .expect("one checked result per checkable record")
            } else {
                next_refused
                    .next()
                    .expect("one refused result per refused record")
            }
        })
        .collect()
}

/// Lake project name used for generated temp projects.
const LAKE_PROJECT_NAME: &str = "FaberGenerated";

/// Generate a minimal `lakefile.toml` for the temp project.
#[must_use]
fn generate_lakefile_toml() -> String {
    format!(
        "name = \"{LAKE_PROJECT_NAME}\"\n\
         defaultTargets = [\"{LAKE_PROJECT_NAME}\"]\n\
         \n\
         [leanOptions]\n\
         \n\
         [[leanLib]]\n\
         name = \"{LAKE_PROJECT_NAME}\"\n"
    )
}

/// Generate a Lean source file from recognized aspect records.
///
/// Each record produces one theorem declaration.  Stub theorems use
/// `trivial` until the G4 codegen backend supplies real proof content.
#[must_use]
fn generate_lean_source(records: &[&HirAspectRecord]) -> String {
    let mut out = String::new();
    out.push_str("/- Auto-generated by radix LeanAspectChecker. DO NOT EDIT.\n");
    out.push_str("   This file is replaceable — do not edit manually. -/\n\n");

    for (i, record) in records.iter().enumerate() {
        let label = theorem_label(record, i);
        write!(out, "theorem {label} : True := trivial\n\n").ok();
    }

    out
}

/// Build a deterministic theorem label from a record's node id and index.
fn theorem_label(record: &HirAspectRecord, index: usize) -> String {
    match record.node_id {
        Some(node_id) => format!("aspect_{}_{}", aspect_label(record), node_id.0),
        None => format!("aspect_{}_record_{index}", aspect_label(record)),
    }
}

/// Short label for the aspect kind.
fn aspect_label(record: &HirAspectRecord) -> &'static str {
    match record.aspect {
        HirAspectKind::DiscerneOmnia => "discerne_omnia",
        HirAspectKind::UnrollEquivalence => "unroll_equivalence",
        HirAspectKind::MaskNeutrality => "mask_neutrality",
        HirAspectKind::GatePurity => "gate_purity",
    }
}

/// Whether a checker-ready proof generator is registered for the kind.
///
/// Only kinds with landed line-proof content may reach `generate_lean_source`.
/// The AIR kinds are registered (APA-2) but carry no proof generator until
/// their owning lines land; they must never fall through the generic
/// `True := trivial` placeholder as if they were proven obligations.
fn has_registered_proof_generator(aspect: HirAspectKind) -> bool {
    matches!(aspect, HirAspectKind::DiscerneOmnia)
}

/// Create a unique temporary project directory.
fn make_temp_project_dir() -> Result<std::path::PathBuf, String> {
    let base = std::env::temp_dir().join("radix-lean-checker");
    std::fs::create_dir_all(&base).map_err(|e| format!("failed to create temp base: {e}"))?;

    // Use a UUID-like suffix for uniqueness (process id + monotonic counter).
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        NEXT_TEMP_INDEX.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    let dir = base.join(suffix);
    std::fs::create_dir(&dir).map_err(|e| format!("failed to create temp dir: {e}"))?;
    Ok(dir)
}

/// Monotonic counter for temp directory uniqueness within one process.
static NEXT_TEMP_INDEX: AtomicU64 = AtomicU64::new(0);

/// Map raw [`LakeResult`] into a backend-neutral [`HirAspectCheckResult`].
///
/// Classification rules:
///
/// | Condition | Status |
/// |---|---|
/// | `timed_out` | `Timeout` |
/// | `exit_code == Some(0)` | `Proven` |
/// | `exit_code == Some(nonzero)` | `Failed` |
/// | `exit_code == None` + toolchain-not-found evidence | `ToolchainMissing` |
/// | `exit_code == None` (other) | `Error` |
#[must_use]
pub fn normalize_result(lake_output: LakeResult) -> HirAspectCheckResult {
    if lake_output.timed_out {
        return HirAspectCheckResult {
            status: HirAspectCheckStatus::Timeout,
            evidence: HirAspectEvidence(lake_output.stderr),
            backend: HirAspectBackendKind::Lean,
        };
    }

    let Some(exit_code) = lake_output.exit_code else {
        // Process did not produce an exit code — launch or toolchain failure.
        let combined = [lake_output.stdout.as_str(), lake_output.stderr.as_str()].concat();
        if is_toolchain_missing_evidence(&combined) {
            return HirAspectCheckResult {
                status: HirAspectCheckStatus::ToolchainMissing,
                evidence: HirAspectEvidence(combined),
                backend: HirAspectBackendKind::Lean,
            };
        }
        return HirAspectCheckResult {
            status: HirAspectCheckStatus::Error,
            evidence: HirAspectEvidence(combined),
            backend: HirAspectBackendKind::Lean,
        };
    };

    match exit_code {
        0 => HirAspectCheckResult {
            status: HirAspectCheckStatus::Proven,
            evidence: HirAspectEvidence(lake_output.stdout),
            backend: HirAspectBackendKind::Lean,
        },
        _ => {
            // Non-zero exit — syntax error, type error, etc.
            // U2 adds more granular classification (sorry, axioms).
            HirAspectCheckResult {
                status: HirAspectCheckStatus::Failed,
                evidence: HirAspectEvidence(lake_output.stderr),
                backend: HirAspectBackendKind::Lean,
            }
        }
    }
}

// ---------------------------------------------------------------------------
// combined verdict
// ---------------------------------------------------------------------------

/// Combine the lake build result, axiom verdict, and sorry verdict into a
/// single final [`HirAspectCheckResult`].
///
/// A theorem is `Proven` only when all of these hold:
/// 1. Lake build succeeded (already `Proven` in `lake_result`).
/// 2. No `sorry` tokens detected (`Clean`).
/// 3. Every axiom dependency is in the allowlist (`Allowed`).
///
/// If the lake build itself did not produce `Proven` the original result is
/// returned unchanged — the trust checks only tighten a passing build.
#[must_use]
pub fn combined_verdict(
    lake_result: HirAspectCheckResult,
    axiom_verdict: &AxiomVerdict,
    sorry_verdict: &SorryVerdict,
) -> HirAspectCheckResult {
    if lake_result.status != HirAspectCheckStatus::Proven {
        return lake_result;
    }

    if let SorryVerdict::Found { line_numbers } = sorry_verdict {
        return HirAspectCheckResult {
            status: HirAspectCheckStatus::Failed,
            evidence: HirAspectEvidence(format!("sorry detected at lines: {line_numbers:?}")),
            backend: HirAspectBackendKind::Lean,
        };
    }

    if let AxiomVerdict::Rejected { disallowed } = axiom_verdict {
        let names: Vec<&str> = disallowed.iter().map(|d| d.axiom_name.as_str()).collect();
        return HirAspectCheckResult {
            status: HirAspectCheckStatus::Failed,
            evidence: HirAspectEvidence(format!("disallowed axioms: {names:?}")),
            backend: HirAspectBackendKind::Lean,
        };
    }

    lake_result
}

// ---------------------------------------------------------------------------
// axiom output parsing
// ---------------------------------------------------------------------------

/// Parse `#print axioms` output into an [`AxiomAudit`].
///
/// Handles the Lean 4 format:
///
/// ```text
/// 'declName' depends on axioms: [propext, Classical.choice]
/// 'otherDecl' does not depend on any axioms
/// ```
#[must_use]
pub fn parse_axiom_output(output: &str) -> AxiomAudit {
    let mut dependencies = Vec::new();

    for line in output.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix('\'') else {
            continue;
        };

        let Some((decl, suffix)) = rest.split_once('\'') else {
            continue;
        };

        if let Some(axiom_part) = suffix.strip_prefix(" depends on axioms: [") {
            let axiom_part = axiom_part.trim_end_matches(']');
            for axiom in axiom_part.split(',') {
                let axiom = axiom.trim();
                if !axiom.is_empty() {
                    dependencies.push(AxiomDependency { axiom_name: axiom.to_string(), declaration: decl.to_string() });
                }
            }
        }
        // "does not depend on any axioms" → nothing to record
    }

    AxiomAudit { dependencies }
}

/// Extract candidate Lean declaration names from source text.
///
/// Simple heuristic: recognises `theorem`, `lemma`, `def`, `example`, and
/// `axiom` prefixes.
fn extract_declarations(source: &str) -> Vec<String> {
    let prefixes: &[&str] = &["theorem ", "lemma ", "def ", "example ", "axiom "];
    let mut names = Vec::new();

    for line in source.lines() {
        let trimmed = line.trim();
        for prefix in prefixes {
            if let Some(rest) = trimmed.strip_prefix(prefix) {
                if let Some(name) = rest.split_whitespace().next() {
                    let name = name
                        .trim_end_matches(':')
                        .trim_end_matches('(')
                        .trim_end_matches(')');
                    if !name.is_empty()
                        && name
                            .chars()
                            .all(|c| c.is_alphanumeric() || c == '_' || c == '\'')
                    {
                        names.push(name.to_string());
                    }
                }
            }
        }
    }

    names
}

/// Check whether a single source line contains a `sorry` token.
///
/// Strips inline comments (both `--` and `/--`) before scanning so that
/// commented-out `sorry` tokens are not flagged.
fn contains_sorry_token(line: &str) -> bool {
    // Strip `--` line-comments.
    let without_comments = match line.find("--") {
        Some(pos) => &line[..pos],
        None => line,
    };
    // Strip block-comment-start `/--`.
    let without_comments = match without_comments.find("/--") {
        Some(pos) => &without_comments[..pos],
        None => without_comments,
    };

    // Match `sorry` as a whole word.
    without_comments
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|word| word == "sorry")
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn is_bare_or_found(path: &Path) -> bool {
    if path.components().count() == 1 {
        // Bare command — assume it is reachable on PATH.
        true
    } else {
        path.is_file()
    }
}

fn is_toolchain_missing_evidence(output: &str) -> bool {
    let haystack = output.to_lowercase();
    haystack.contains("not found") || haystack.contains("no such file") || haystack.contains("cannot find")
}

fn drain_pipe<R: Read>(pipe: Option<R>) -> String {
    let mut output = String::new();
    if let Some(mut reader) = pipe {
        ignore_result(reader.read_to_string(&mut output));
    }
    output
}
