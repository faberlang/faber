//! G6 GO3 — write planned Go package artifacts and invoke `go build`.
//!
//! Layout under `<package>/target/faber/go/`:
//! - `main.go` (entry)
//! - optional sibling `*.go` module files (same `package main`)
//! - `go.mod` module `faber/<package>`
//! - binary at `<package>/target/faber/go/bin/<name>`

use radix::diagnostics::Diagnostic;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::discovery::BuildLayout;

/// On-disk layout for a assembled Go package.
#[derive(Debug, Clone)]
pub(crate) struct GoBuildLayout {
    pub module_root: PathBuf,
    pub binary_path: PathBuf,
    pub package_name: String,
}

impl GoBuildLayout {
    pub(crate) fn from_package(layout: &BuildLayout) -> Self {
        let module_root = layout.package_root.join("target").join("faber").join("go");
        let package_name = layout.binary_name().to_owned();
        let binary_path = module_root.join("bin").join(&package_name);
        Self {
            module_root,
            binary_path,
            package_name,
        }
    }
}

/// Write Go sources + `go.mod` for a single-package product assembly.
pub(crate) fn emit_go_module(
    layout: &GoBuildLayout,
    entry_code: &str,
    modules: &[(String, String)],
) -> Result<(), Diagnostic> {
    fs::create_dir_all(&layout.module_root).map_err(|err| {
        Diagnostic::error(format!(
            "failed to create Go module root '{}': {err}",
            layout.module_root.display()
        ))
        .with_arg("issue", "package_go_emit_failed")
    })?;
    fs::create_dir_all(layout.module_root.join("bin")).map_err(|err| {
        Diagnostic::error(format!(
            "failed to create Go binary dir '{}': {err}",
            layout.module_root.join("bin").display()
        ))
        .with_arg("issue", "package_go_emit_failed")
    })?;

    let main_path = layout.module_root.join("main.go");
    fs::write(&main_path, entry_code).map_err(|err| {
        Diagnostic::error(format!(
            "failed to write '{}': {err}",
            main_path.display()
        ))
        .with_arg("issue", "package_go_emit_failed")
    })?;

    for (file_name, code) in modules {
        let path = layout.module_root.join(file_name);
        fs::write(&path, code).map_err(|err| {
            Diagnostic::error(format!("failed to write '{}': {err}", path.display()))
                .with_arg("issue", "package_go_emit_failed")
        })?;
    }

    let go_mod = format!(
        "module faber/{}\n\ngo 1.22\n",
        sanitize_go_module_segment(&layout.package_name)
    );
    let go_mod_path = layout.module_root.join("go.mod");
    fs::write(&go_mod_path, go_mod).map_err(|err| {
        Diagnostic::error(format!(
            "failed to write '{}': {err}",
            go_mod_path.display()
        ))
        .with_arg("issue", "package_go_emit_failed")
    })?;

    Ok(())
}

/// Invoke `go build` for an emitted module; returns the binary path.
pub(crate) fn invoke_go_build(layout: &GoBuildLayout) -> Result<PathBuf, Diagnostic> {
    if let Some(parent) = layout.binary_path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            Diagnostic::error(format!(
                "failed to create '{}': {err}",
                parent.display()
            ))
            .with_arg("issue", "package_go_build_failed")
        })?;
    }

    let output = Command::new("go")
        .arg("build")
        .arg("-o")
        .arg(&layout.binary_path)
        .arg(".")
        .current_dir(&layout.module_root)
        .output()
        .map_err(|err| {
            Diagnostic::error(format!("failed to execute `go build`: {err}"))
                .with_arg("issue", "package_go_build_failed")
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(Diagnostic::error(format!(
            "go build failed for '{}':\n{stderr}{stdout}",
            layout.module_root.display()
        ))
        .with_arg("issue", "package_go_build_failed"));
    }

    if !layout.binary_path.exists() {
        return Err(Diagnostic::error(format!(
            "go build reported success but binary missing at '{}'",
            layout.binary_path.display()
        ))
        .with_arg("issue", "package_go_build_failed"));
    }

    Ok(layout.binary_path.clone())
}

fn sanitize_go_module_segment(name: &str) -> String {
    name.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

/// Run a built Go binary with forwarded argv.
#[allow(dead_code)] // used by binary `commands/run` (not the lib test surface)
pub(crate) fn run_go_binary(binary: &Path, args: &[String]) -> Result<i32, Diagnostic> {
    let status = Command::new(binary)
        .args(args)
        .status()
        .map_err(|err| {
            Diagnostic::error(format!(
                "failed to execute '{}': {err}",
                binary.display()
            ))
            .with_arg("issue", "package_go_run_failed")
        })?;
    Ok(status.code().unwrap_or(1))
}
