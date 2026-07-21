//! External LLVM link-and-run helpers for exempla e2e Tier C/D.

use super::common::{normalize_newline, read_expected_stdout};
use radix::llvm_host::LlvmHostToolchain;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlvmRunBucket {
    ToolchainMissing,
    LinkFailed,
    RunFailed,
    Runnable,
    OutputMatched,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlvmRunProbe {
    pub bucket: LlvmRunBucket,
    pub reason: String,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

impl LlvmRunProbe {
    fn toolchain_missing(reason: impl Into<String>) -> Self {
        Self {
            bucket: LlvmRunBucket::ToolchainMissing,
            reason: reason.into(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
        }
    }

    fn link_failed(reason: impl Into<String>) -> Self {
        Self {
            bucket: LlvmRunBucket::LinkFailed,
            reason: reason.into(),
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
        }
    }

    fn run_failed(
        reason: impl Into<String>,
        stdout: String,
        stderr: String,
        exit_code: Option<i32>,
    ) -> Self {
        Self {
            bucket: LlvmRunBucket::RunFailed,
            reason: reason.into(),
            stdout,
            stderr,
            exit_code,
        }
    }
}

pub fn run_llvm_exemplum(
    llvm_file: &Path,
    temp_root: &Path,
    stem: &str,
    fab_path: &Path,
) -> LlvmRunProbe {
    let toolchain = match llvm_host_toolchain() {
        Ok(toolchain) => toolchain,
        Err(reason) => return LlvmRunProbe::toolchain_missing(format!("tier C skipped: {reason}")),
    };
    let runtime_archive = match llvm_runtime_archive() {
        Ok(path) => path,
        Err(reason) => return LlvmRunProbe::toolchain_missing(reason),
    };

    let binary_file = temp_root.join(format!("{stem}.bin"));
    if let Err(reason) = toolchain.verify(llvm_file) {
        return LlvmRunProbe::link_failed(reason);
    }
    if let Err(reason) = toolchain.link(llvm_file, &runtime_archive, &binary_file) {
        return LlvmRunProbe::link_failed(reason);
    }

    let run = super::common::command_output_with_timeout(
        &mut Command::new(&binary_file),
        Duration::from_secs(10),
    );
    let Ok(run) = run else {
        return LlvmRunProbe::run_failed(
            "cannot execute linked binary",
            String::new(),
            String::new(),
            None,
        );
    };
    let stdout = String::from_utf8_lossy(&run.stdout).to_string();
    if !run.status.success() {
        let stderr = String::from_utf8_lossy(&run.stderr).to_string();
        return LlvmRunProbe::run_failed(
            format!(
                "run failed (status {:?}): {}",
                run.status.code(),
                stderr.trim()
            ),
            stdout,
            stderr,
            run.status.code(),
        );
    }

    let captured = stdout;
    if let Some(expected) = read_expected_stdout(fab_path) {
        if normalize_newline(&captured) == expected {
            return LlvmRunProbe {
                bucket: LlvmRunBucket::OutputMatched,
                reason: "tier D output matched .expected".to_owned(),
                stdout: captured,
                stderr: String::from_utf8_lossy(&run.stderr).to_string(),
                exit_code: run.status.code(),
            };
        }
        return LlvmRunProbe {
            bucket: LlvmRunBucket::Runnable,
            reason: format!(
                "tier C runnable; .expected mismatch: got {captured:?}, want {expected:?}"
            ),
            stdout: captured,
            stderr: String::from_utf8_lossy(&run.stderr).to_string(),
            exit_code: run.status.code(),
        };
    }

    LlvmRunProbe {
        bucket: LlvmRunBucket::Runnable,
        reason: "tier C runnable via external llvm-as + clang".to_owned(),
        stdout: captured,
        stderr: String::from_utf8_lossy(&run.stderr).to_string(),
        exit_code: run.status.code(),
    }
}

fn llvm_host_toolchain() -> Result<LlvmHostToolchain, String> {
    static TOOLCHAIN: OnceLock<Result<LlvmHostToolchain, String>> = OnceLock::new();
    TOOLCHAIN.get_or_init(LlvmHostToolchain::discover).clone()
}

fn llvm_runtime_archive() -> Result<PathBuf, String> {
    static ARCHIVE: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    ARCHIVE.get_or_init(build_llvm_runtime_archive).clone()
}

fn build_llvm_runtime_archive() -> Result<PathBuf, String> {
    let runtime_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../faber-runtime");
    let manifest = runtime_root.join("hosts/llvm/Cargo.toml");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(&manifest);
    let build = super::common::command_output_with_timeout(&mut command, Duration::from_secs(300))
        .map_err(|error| format!("cannot build LLVM host runtime: {error}"))?;
    if !build.status.success() {
        return Err(format!(
            "LLVM host runtime build failed: {}",
            String::from_utf8_lossy(&build.stderr).trim()
        ));
    }
    let archive = runtime_root.join("target/release").join(if cfg!(windows) {
        "faber_host_llvm.lib"
    } else {
        "libfaber_host_llvm.a"
    });
    archive
        .is_file()
        .then_some(archive)
        .ok_or_else(|| "LLVM host runtime archive was not produced".to_owned())
}

#[cfg(test)]
#[path = "llvm_runtime_test.rs"]
mod tests;
