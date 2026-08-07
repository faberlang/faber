//! Faber-owned target-source formatting and lint post-processing.
//!
//! These helpers intentionally live in the Faber product CLI, not Radix.
//! Radix emits target artifacts. Faber may optionally run target-specific
//! developer tools as part of user-facing build/emit workflows.

use radix::codegen::Target;
use radix::Output;
use std::path::Path;
use std::{fs, io};

/// Run Faber-owned post-processing over emitted target source.
#[must_use]
pub fn postprocess_code(mut code: String, target: Target, format: bool, linter: bool) -> String {
    if format {
        match format_generated_code(target, &code) {
            Ok(formatted) => code = formatted,
            Err(err) => {
                eprintln!("warning: formatting failed: {err}");
            }
        }
    }

    if linter {
        match lint_generated_code(target, &code) {
            Ok(fixed) => code = fixed,
            Err(err) => {
                eprintln!("warning: linter failed: {err}");
            }
        }
    }

    code
}

/// Write an emitted artifact, applying Faber-owned post-processing for text
/// outputs when requested.
pub fn write_output_artifact(
    path: &Path,
    output: Output,
    target: Target,
    format: bool,
    linter: bool,
) {
    ensure_artifact_parent(path);

    match output {
        Output::Wasm(out) => {
            write_artifact_contents(path, &out.bytes);
        }
        other => {
            let code = postprocess_code(radix::tool::output_code(other), target, format, linter);
            write_artifact_contents(path, code);
        }
    }
}

fn ensure_artifact_parent(path: &Path) {
    if let Some(parent) = path.parent() {
        if parent.as_os_str().is_empty() {
            return;
        }
        fs::create_dir_all(parent).unwrap_or_else(|err| {
            eprintln!(
                "error: failed to create output directory '{}': {}",
                parent.display(),
                err
            );
            std::process::exit(1);
        });
    }
}

fn write_artifact_contents(path: &Path, contents: impl AsRef<[u8]>) {
    match fs::write(path, contents) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::BrokenPipe => {}
        Err(err) => {
            eprintln!("error: failed to write '{}': {}", path.display(), err);
            std::process::exit(1);
        }
    }
}

/// Run the appropriate formatter for generated target code, if available.
///
/// # Errors
///
/// Returns an error string if the formatter binary is not installed, cannot be
/// spawned, or exits with a non-zero status.
pub fn format_generated_code(target: Target, code: &str) -> Result<String, String> {
    match target {
        Target::HirRust => run_formatter("rustfmt", &["--edition", "2021"], code),
        Target::HirFaber => Ok(code.to_string()),
        Target::HirGo => run_formatter("gofmt", &[], code),
        Target::HirSwift => Ok(code.to_string()),
        Target::HirTypeScript => format_typescript_code(code),
        Target::HirFhir => Ok(code.to_string()),
        Target::MirStepper
        | Target::MirWasmBinary
        | Target::MirWasm
        | Target::MirLlvm
        | Target::MirLlvmHost
        | Target::MirMetal
        | Target::MirWgsl
        | Target::MirSexp
        | Target::MirScena
        | Target::MirFmir
        | Target::MirFmirBinary
        | Target::MirFmirBundle => Ok(code.to_string()),
    }
}

/// Run a linter with auto-fix on generated target code where possible.
///
/// # Errors
///
/// Returns an error string if the linter binary is not installed, cannot be
/// spawned, or exits with a non-zero status.
pub fn lint_generated_code(target: Target, code: &str) -> Result<String, String> {
    match target {
        Target::HirRust => lint_rust_code(code),
        Target::HirFaber | Target::HirGo | Target::HirSwift | Target::HirFhir => {
            Ok(code.to_string())
        }
        Target::HirTypeScript => lint_typescript_code(code),
        Target::MirStepper
        | Target::MirWasmBinary
        | Target::MirWasm
        | Target::MirLlvm
        | Target::MirLlvmHost
        | Target::MirMetal
        | Target::MirWgsl
        | Target::MirSexp
        | Target::MirScena
        | Target::MirFmir
        | Target::MirFmirBinary
        | Target::MirFmirBundle => Ok(code.to_string()),
    }
}

fn format_typescript_code(code: &str) -> Result<String, String> {
    if let Ok(formatted) = run_formatter("prettier", &["--parser", "typescript"], code) {
        return Ok(formatted);
    }
    run_formatter("deno", &["fmt", "--ext", "ts", "-"], code)
}

fn lint_typescript_code(code: &str) -> Result<String, String> {
    if let Ok(fixed) = run_formatter(
        "biome",
        &["check", "--apply", "--stdin-file-path", "main.ts"],
        code,
    ) {
        return Ok(fixed);
    }

    run_formatter(
        "eslint",
        &[
            "--fix-dry-run",
            "--stdin",
            "--stdin-filename",
            "main.ts",
            "--format",
            "json",
        ],
        code,
    )
    .map(|_| code.to_string())
}

fn lint_rust_code(code: &str) -> Result<String, String> {
    use std::process::Command;

    let (temp_dir, main_rs) = write_temp_rust_project(code)?;

    let output = Command::new("cargo")
        .args([
            "clippy",
            "--fix",
            "--allow-dirty",
            "--allow-staged",
            "--allow-no-vcs",
            "--quiet",
            "--",
            "-D",
            "warnings",
        ])
        .current_dir(&temp_dir)
        .output();

    match output {
        Ok(output) if output.status.success() => {
            let fixed = std::fs::read_to_string(&main_rs)
                .map_err(|e| format!("failed to read fixed code: {e}"))?;
            cleanup_temp_dir(&temp_dir);
            Ok(fixed)
        }
        Ok(output) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            cleanup_temp_dir(&temp_dir);
            Err(format!(
                "cargo clippy --fix exited with status {}: {stderr}",
                output.status
            ))
        }
        Err(e) => {
            cleanup_temp_dir(&temp_dir);
            Err(format!(
                "failed to run cargo clippy: {e} (is clippy installed?)"
            ))
        }
    }
}

fn write_temp_rust_project(code: &str) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    use std::fs;

    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let temp_dir = std::env::temp_dir().join(format!("faber-lint-{nanos}"));
    let src_dir = temp_dir.join("src");

    fs::create_dir_all(&src_dir).map_err(|e| format!("failed to create temp src dir: {e}"))?;

    let main_rs = src_dir.join("main.rs");
    fs::write(&main_rs, code).map_err(|e| format!("failed to write temp main.rs: {e}"))?;

    let cargo_toml = temp_dir.join("Cargo.toml");
    fs::write(
        &cargo_toml,
        "[package]\nname = \"lint-target\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .map_err(|e| format!("failed to write Cargo.toml: {e}"))?;

    Ok((temp_dir, main_rs))
}

fn cleanup_temp_dir(path: &std::path::Path) {
    if let Err(err) = std::fs::remove_dir_all(path) {
        eprintln!(
            "warning: failed to remove temporary lint directory '{}': {err}",
            path.display()
        );
    }
}

fn run_formatter(cmd: &str, args: &[&str], input: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not spawn {cmd}: {e} (is it installed?)"))?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "failed to open stdin".to_string())?;
        stdin
            .write_all(input.as_bytes())
            .map_err(|e| format!("failed to write to {cmd} stdin: {e}"))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to wait for {cmd}: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{cmd} failed: {}", stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}
