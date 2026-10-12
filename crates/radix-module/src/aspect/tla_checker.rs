//! External TLC model-checker integration for aspect verification.
//!
//! Runs the TLA+ TLC checker against modules emitted by [`tla`] and parses its
//! output into pass/fail/counter-example results. The checker is an optional
//! developer tool: it shells out to `java -jar` and is skipped when the TLA
//! tools jar is not configured. It never modifies the aspect report itself.

use super::HirAspectBackendKind;
use super::HirAspectReport;
use super::HirAspectTlaModule;
use super::tla::emit_tla_modules;
use super::verifier::{HirAspectCheckResult, HirAspectCheckStatus, HirAspectEvidence, HirAspectVerifier};
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(test)]
#[path = "tla_checker_test.rs"]
mod tests;

const TLC_MAIN_CLASS: &str = "tlc2.TLC";
const TLC_CFG_TEMPLATE: &str = "SPECIFICATION Spec\nINVARIANT Totality\n";
const TLC_SUCCESS_MARKER: &str = "No error has been found";
const TLC_COUNTEREXAMPLE_MARKER: &str = "The following behavior constitutes a counter-example";
/// TLC reports an invariant violation with a status line of the form
/// `Error: Invariant <name> is violated.`. Matching the full status line
/// rather than a bare `Invariant` substring keeps unrelated output (e.g.
/// stack traces that quote the module) from flipping a successful run.
const TLC_INVARIANT_VIOLATION_PREFIX: &str = "Error: Invariant";
const TLC_INVARIANT_VIOLATION_SUFFIX: &str = "is violated.";
const TLC_ERROR_MARKER: &str = "Error:";
const TLC_NO_OUTPUT_SUMMARY: &str = "TLC produced no output";
const TLC_UNRECOGNIZED_OUTPUT_SUMMARY: &str = "TLC output did not contain a recognizable success or failure";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlaAspectCheckerConfig {
    pub java_bin: PathBuf,
    pub tla_tools_jar: PathBuf,
    pub work_dir: PathBuf,
}

impl TlaAspectCheckerConfig {
    /// Construct config from environment variables, returning `None` when
    /// `FABER_TLA_TLC_JAR` is unset.
    ///
    /// # Errors
    ///
    /// Returns an error string if the specified `FABER_TLA_TLC_JAR` or
    /// `FABER_TLA_JAVA_BIN` paths are invalid or cannot be canonicalized.
    // public API: callers choose move vs clone
    pub fn from_env(work_dir: PathBuf) -> Result<Option<Self>, String> {
        let Some(jar) = std::env::var_os("FABER_TLA_TLC_JAR") else {
            return Ok(None);
        };
        let java = std::env::var_os("FABER_TLA_JAVA_BIN").unwrap_or_else(|| "java".into());
        Self::from_env_values(&work_dir, &jar, &java).map(Some)
    }

    fn from_env_values(work_dir: &Path, jar: &OsStr, java: &OsStr) -> Result<Self, String> {
        let jar = PathBuf::from(jar);
        let java = PathBuf::from(java);
        Ok(Self {
            java_bin: normalize_java_bin(&java)?,
            tla_tools_jar: canonicalize_env_path(&jar, "FABER_TLA_TLC_JAR")?,
            work_dir: work_dir.to_path_buf(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TlaAspectChecker {
    config: TlaAspectCheckerConfig,
}

impl TlaAspectChecker {
    #[must_use]
    pub fn new(config: TlaAspectCheckerConfig) -> Self {
        Self { config }
    }

    #[must_use]
    pub fn check_modules(&self, modules: &[HirAspectTlaModule]) -> Vec<HirAspectCheckResult> {
        modules
            .iter()
            .map(|module| self.check_module(module))
            .collect()
    }

    fn check_module(&self, module: &HirAspectTlaModule) -> HirAspectCheckResult {
        if !is_tla_identifier(&module.name) {
            return HirAspectCheckResult::checker_error("invalid TLA module name");
        }

        if let Err(error) = write_module_files(&self.config.work_dir, module) {
            return HirAspectCheckResult::checker_error(&format!("failed to write TLA module: {error}"));
        }

        if !self.config.java_bin_exists() {
            return HirAspectCheckResult::checker_unavailable("java executable is unavailable");
        }
        if !self.config.tla_tools_jar.is_file() {
            return HirAspectCheckResult::checker_unavailable("FABER_TLA_TLC_JAR does not point to a file");
        }

        let output = Command::new(&self.config.java_bin)
            .current_dir(&self.config.work_dir)
            .arg("-cp")
            .arg(&self.config.tla_tools_jar)
            .arg(TLC_MAIN_CLASS)
            .arg("-deadlock")
            .arg("-cleanup")
            .arg(&module.name)
            .output();

        match output {
            Ok(output) => normalize_tlc_output(
                output.status.success(),
                &String::from_utf8_lossy(&output.stdout),
                &String::from_utf8_lossy(&output.stderr),
            ),
            Err(error) => {
                HirAspectCheckResult::checker_unavailable(&format!("failed to launch TLC through java: {error}"))
            }
        }
    }
}

impl TlaAspectCheckerConfig {
    fn java_bin_exists(&self) -> bool {
        is_bare_command(&self.java_bin) || self.java_bin.is_file()
    }
}

#[must_use]
pub fn check_tla_modules(config: TlaAspectCheckerConfig, modules: &[HirAspectTlaModule]) -> Vec<HirAspectCheckResult> {
    TlaAspectChecker::new(config).check_modules(modules)
}

/// TLA+ model-checker verifier — implements [`HirAspectVerifier`] over
/// [`TlaAspectChecker`].
#[derive(Debug, Clone)]
pub struct TlaAspectVerifier {
    config: TlaAspectCheckerConfig,
}

impl TlaAspectVerifier {
    #[must_use]
    pub fn new(config: TlaAspectCheckerConfig) -> Self {
        Self { config }
    }
}

impl HirAspectVerifier for TlaAspectVerifier {
    fn verify(&self, report: &HirAspectReport) -> Vec<HirAspectCheckResult> {
        let modules = emit_tla_modules(report);
        check_tla_modules(self.config.clone(), &modules)
    }
}

impl HirAspectCheckResult {
    fn passed(summary: &str) -> Self {
        Self {
            status: HirAspectCheckStatus::Proven,
            evidence: HirAspectEvidence(summary.to_owned()),
            counterexample: Vec::new(),
            backend: HirAspectBackendKind::Tla,
        }
    }

    fn failed(summary: &str, counterexample: Vec<String>) -> Self {
        Self {
            status: HirAspectCheckStatus::Failed,
            evidence: HirAspectEvidence(summary.to_owned()),
            counterexample,
            backend: HirAspectBackendKind::Tla,
        }
    }

    fn checker_error(summary: &str) -> Self {
        Self {
            status: HirAspectCheckStatus::Error,
            evidence: HirAspectEvidence(summary.to_owned()),
            counterexample: Vec::new(),
            backend: HirAspectBackendKind::Tla,
        }
    }

    fn checker_unavailable(summary: &str) -> Self {
        Self {
            status: HirAspectCheckStatus::ToolchainMissing,
            evidence: HirAspectEvidence(summary.to_owned()),
            counterexample: Vec::new(),
            backend: HirAspectBackendKind::Tla,
        }
    }
}

fn write_module_files(work_dir: &Path, module: &HirAspectTlaModule) -> std::io::Result<()> {
    fs::create_dir_all(work_dir)?;
    fs::write(work_dir.join(format!("{}.tla", module.name)), &module.text)?;
    fs::write(work_dir.join(format!("{}.cfg", module.name)), TLC_CFG_TEMPLATE)?;
    Ok(())
}

fn normalize_tlc_output(exit_success: bool, stdout: &str, stderr: &str) -> HirAspectCheckResult {
    let combined = format!("{stdout}\n{stderr}");

    if !exit_success {
        // TLC exits non-zero both for genuine violations and for toolchain
        // failures. Only output that actually reports a violation (the
        // counter-example trace or the invariant status line) disproves the
        // aspect; anything else on a failed exit is a checker error.
        if contains_failure_markers(&combined) {
            return HirAspectCheckResult::failed(first_relevant_line(&combined), counterexample_lines(&combined));
        }
        return HirAspectCheckResult::checker_error(first_nonempty_line(&combined));
    }

    if combined.contains(TLC_SUCCESS_MARKER) {
        return HirAspectCheckResult::passed("TLC completed with no error");
    }

    // Defensive: a zero-exit run that still printed a violation status line
    // must not surface as success.
    if contains_failure_markers(&combined) {
        return HirAspectCheckResult::failed(first_relevant_line(&combined), counterexample_lines(&combined));
    }

    HirAspectCheckResult::checker_error(TLC_UNRECOGNIZED_OUTPUT_SUMMARY)
}

/// Whether the output reports a genuine model-checking violation: the
/// counter-example trace marker or the precise invariant status line.
fn contains_failure_markers(output: &str) -> bool {
    output.contains(TLC_COUNTEREXAMPLE_MARKER) || output.lines().any(is_tlc_invariant_violation_line)
}

/// TLC's invariant-failure status line format: `Error: Invariant <name> is
/// violated.`. Matching prefix and suffix together is what distinguishes a
/// real violation from incidental `Invariant` mentions in other output.
fn is_tlc_invariant_violation_line(line: &str) -> bool {
    line.trim_start()
        .starts_with(TLC_INVARIANT_VIOLATION_PREFIX)
        && line.contains(TLC_INVARIANT_VIOLATION_SUFFIX)
}

fn normalize_java_bin(path: &Path) -> Result<PathBuf, String> {
    if is_bare_command(path) {
        Ok(path.to_path_buf())
    } else {
        canonicalize_env_path(path, "FABER_TLA_JAVA_BIN")
    }
}

fn canonicalize_env_path(path: &Path, var: &'static str) -> Result<PathBuf, String> {
    fs::canonicalize(path).map_err(|error| format!("{var} path `{}` is invalid: {error}", path.display()))
}

fn is_bare_command(path: &Path) -> bool {
    path.components().count() == 1
}

fn first_relevant_line(output: &str) -> &str {
    output
        .lines()
        .find(|line| is_tlc_invariant_violation_line(line) || line.contains(TLC_ERROR_MARKER))
        .unwrap_or_else(|| first_nonempty_line(output))
}

fn first_nonempty_line(output: &str) -> &str {
    output
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(TLC_NO_OUTPUT_SUMMARY)
}

fn counterexample_lines(output: &str) -> Vec<String> {
    let mut in_trace = false;
    let mut lines = Vec::new();
    for line in output.lines() {
        if line.contains(TLC_COUNTEREXAMPLE_MARKER) {
            in_trace = true;
        }
        if in_trace && !line.trim().is_empty() {
            lines.push(line.to_owned());
        }
    }
    lines
}

fn is_tla_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_') && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}
