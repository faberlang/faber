//! Faber-owned target-source formatting and lint post-processing.
//!
//! These helpers intentionally live in the Faber product CLI, not Radix.
//! Radix emits target artifacts. Faber may optionally run target-specific
//! developer tools as part of user-facing build/emit workflows.

use radix::module::Output;
use radix::module::codegen::Target;
use radix::services::DiagnosticMode;
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

    let code = postprocess_code(radix::services::output_code(output), target, format, linter);
    write_artifact_contents(path, code);
}

/// Drop LOCALE002 wrong-locale notes for sources that declare a locale.
///
/// WHY: the package and eval routes assemble their diagnostics inside the
/// compiler driver, whose per-module analysis sees only the peeled body — the
/// `+++` frontmatter is gone by the time the note is produced, so a source
/// that declares `locale` can still draw a note claiming it has none. The
/// note is advisory and the SEM errors already report the misread symbols, so
/// suppression is the honest fix. A locale selected only by manifest or
/// `--locale` (invisible in the file) is not detected here and keeps the note.
#[must_use]
pub fn drop_declared_locale002_notes(
    diagnostics: Vec<radix::module::diagnostics::Diagnostic>,
) -> Vec<radix::module::diagnostics::Diagnostic> {
    let mut checked_files: std::collections::BTreeMap<String, bool> =
        std::collections::BTreeMap::new();
    diagnostics
        .into_iter()
        .filter(|diagnostic| {
            if diagnostic.code != Some("LOCALE002")
                || diagnostic.issue().as_deref() != Some("source_reads_as_other_locale")
            {
                return true;
            }
            let declares = checked_files
                .entry(diagnostic.file.clone())
                .or_insert_with(|| file_declares_locale(&diagnostic.file));
            !*declares
        })
        .collect()
}

fn file_declares_locale(file: &str) -> bool {
    let path = std::path::Path::new(file);
    if !path.is_file() {
        return false;
    }
    std::fs::read_to_string(path)
        .map(|source| radix::services::source_declares_locale(file, &source))
        .unwrap_or(false)
}

/// Print diagnostics using Faber-owned presentation.
pub fn print_diagnostics(
    diagnostics: &[radix::module::diagnostics::Diagnostic],
    mode: DiagnosticMode,
    diagnostics_locale_pack: Option<&radix::module::locale::LocalePack>,
) {
    if mode != DiagnosticMode::Diagnostics {
        for diag in diagnostics {
            eprintln!("{}", radix::module::diagnostics::render_plain(diag));
        }
        return;
    }
    if diagnostics.is_empty() {
        return;
    }
    match diagnostics_locale_pack {
        Some(pack) => match radix::module::diagnostics::render_expanded_diagnostics_with_pack(
            diagnostics,
            pack,
        ) {
            Ok(rendered) => eprintln!("{rendered}"),
            Err(err) => {
                eprintln!("error: failed to render reader-pack diagnostics: {err}");
                std::process::exit(1);
            }
        },
        None => eprintln!(
            "{}",
            radix::module::diagnostics::render_expanded_diagnostics(diagnostics)
        ),
    }
}

/// Write a text artifact, creating parent directories as needed.
pub fn write_text_artifact(path: &Path, text: &str) {
    ensure_artifact_parent(path);
    write_artifact_contents(path, text);
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
        Target::HirGo => run_formatter("gofmt", &[], code),
        Target::HirFaber
        | Target::HirSwift
        | Target::HirPython
        | Target::HirHaskell
        | Target::HirPascal
        | Target::HirCobol
        | Target::HirFortran
        | Target::HirOdin
        | Target::HirC
        | Target::HirFhir
        | Target::MirStepper
        | Target::MirWasm
        | Target::MirLlvm
        | Target::MirLlvmHost
        | Target::MirClif
        | Target::MirMetal
        | Target::MirWgsl
        | Target::MirSexp
        | Target::MirBasic
        | Target::MirScena
        | Target::MirFmir
        | Target::MirFmirBinary
        | Target::MirFmirBundle
        | Target::MirAmd => Ok(code.to_string()),
        Target::HirTypeScript => format_typescript_code(code),
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
        Target::HirFaber
        | Target::HirGo
        | Target::HirSwift
        | Target::HirPython
        | Target::HirHaskell
        | Target::HirPascal
        | Target::HirCobol
        | Target::HirFortran
        | Target::HirOdin
        | Target::HirC
        | Target::HirFhir
        | Target::MirStepper
        | Target::MirWasm
        | Target::MirLlvm
        | Target::MirLlvmHost
        | Target::MirClif
        | Target::MirMetal
        | Target::MirWgsl
        | Target::MirSexp
        | Target::MirBasic
        | Target::MirScena
        | Target::MirFmir
        | Target::MirFmirBinary
        | Target::MirFmirBundle
        | Target::MirAmd => Ok(code.to_string()),
        Target::HirTypeScript => lint_typescript_code(code),
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

    let eslint_output = run_formatter(
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
    )?;

    parse_eslint_output(code, &eslint_output)
}

fn parse_eslint_output(code: &str, eslint_output: &str) -> Result<String, String> {
    let results = serde_json::from_str::<serde_json::Value>(eslint_output.trim())
        .map_err(|err| format!("eslint returned invalid JSON: {err}"))?;
    let results = results
        .as_array()
        .ok_or_else(|| "eslint JSON output must be an array".to_string())?;
    let result = match results.as_slice() {
        [] => return Err("eslint JSON output contained no file result".to_string()),
        [result] => result,
        _ => {
            return Err(format!(
                "eslint JSON output contained {} file results; expected exactly one",
                results.len()
            ));
        }
    };
    let result = result
        .as_object()
        .ok_or_else(|| "eslint JSON file result must be an object".to_string())?;

    match result.get("output") {
        Some(output) => output
            .as_str()
            .map(ToOwned::to_owned)
            .ok_or_else(|| "eslint JSON output field was not a string".to_string()),
        // ESLint omits `output` when --fix-dry-run made no changes. In that
        // case the original source is already the fixed source.
        None => Ok(code.to_owned()),
    }
}

fn lint_rust_code(code: &str) -> Result<String, String> {
    use std::process::Command;

    let (temp_dir, main_rs) = write_temp_rust_project_in(&std::env::temp_dir(), code)?;

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

/// Create the lint temp root through the shared exclusive-create retry
/// mechanism — never `create_dir_all` on a predictable name, so a preplanted
/// directory or symlink at a candidate name is neither written through nor
/// clobbered.
fn write_temp_rust_project_in(
    parent: &std::path::Path,
    code: &str,
) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    use std::fs;

    let temp_dir = crate::package::unique_temp_sibling(parent, "faber-lint")
        .map_err(|err| format!("failed to create temp src dir: {}", err.message))?;
    let src_dir = temp_dir.join("src");

    fs::create_dir(&src_dir).map_err(|e| format!("failed to create temp src dir: {e}"))?;

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

#[cfg(test)]
#[path = "postprocess_test.rs"]
mod tests;
