//! Exempla-owned target-source post-processing helpers.
//!
//! These live outside Radix because exempla harnesses execute target-specific
//! formatters and linters as host/toolchain checks. Radix only emits lowered
//! target artifacts.

use radix::codegen::Target;

pub(crate) fn format_generated_code(target: Target, code: &str) -> Result<String, String> {
    match target {
        Target::HirRust => run_formatter("rustfmt", &["--edition", "2021"], code),
        Target::HirFaber => Ok(code.to_string()),
        Target::HirGo => run_formatter("gofmt", &[], code),
        Target::HirSwift => Ok(code.to_string()),
        Target::HirTypeScript => format_typescript_code(code),
        Target::HirFhir => Ok(code.to_string()),
        Target::MirWasmBinary
        | Target::MirWasm
        | Target::MirLlvm
        | Target::MirLlvmHost
        | Target::MirMetal
        | Target::MirWgsl
        | Target::MirSexp
        | Target::MirScena
        | Target::MirFmir
        | Target::MirFmirBinary
        | Target::MirFmirBundle
        | Target::MirStepper
        | Target::MirAmd => Ok(code.to_string()),
    }
}

pub(crate) fn lint_generated_code(target: Target, code: &str) -> Result<String, String> {
    match target {
        Target::HirRust => Ok(code.to_string()),
        Target::HirFaber | Target::HirGo | Target::HirSwift | Target::HirFhir => {
            Ok(code.to_string())
        }
        Target::HirTypeScript => lint_typescript_code(code),
        Target::MirWasmBinary
        | Target::MirWasm
        | Target::MirLlvm
        | Target::MirLlvmHost
        | Target::MirMetal
        | Target::MirWgsl
        | Target::MirSexp
        | Target::MirScena
        | Target::MirFmir
        | Target::MirFmirBinary
        | Target::MirFmirBundle
        | Target::MirStepper
        | Target::MirAmd => Ok(code.to_string()),
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

fn run_formatter(cmd: &str, args: &[&str], code: &str) -> Result<String, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run {cmd}: {e}"))?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| format!("failed to open stdin for {cmd}"))?;
        stdin
            .write_all(code.as_bytes())
            .map_err(|e| format!("failed to send code to {cmd}: {e}"))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("failed to wait for {cmd}: {e}"))?;

    if output.status.success() {
        String::from_utf8(output.stdout).map_err(|e| format!("{cmd} produced non-UTF8 output: {e}"))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(format!(
            "{cmd} exited with status {}: {stderr}",
            output.status
        ))
    }
}

#[cfg(test)]
#[path = "postprocess_test.rs"]
mod tests;
