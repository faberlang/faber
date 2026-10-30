//! The compiled TypeScript runtime vendored into a built Node project.
//!
//! The runtime ships inside `faber` as TypeScript sources (see
//! `embedded_runtime`). Node refuses to strip types from `.ts` under
//! `node_modules`, so the sources are compiled once per embedded-runtime
//! digest with `tsc` into a cache directory, and each built project gets a
//! plain copy under `node_modules/@faber/runtime`: `.js`, `.d.ts` and a Node
//! package manifest. Never a symlink, never `.ts`, never a checkout path.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use radix::module::diagnostics::Diagnostic;

use super::ts_recipe::require_tool;

const COMPLETE_MARKER: &str = ".faber-ts-runtime-complete";

/// The TypeScript runtime sources inside a repository-shaped embedded root.
pub(crate) fn sources_dir(runtime_root: &Path) -> PathBuf {
    runtime_root.join("runtime").join("typescript")
}

fn runtime_error(message: impl Into<String>) -> Diagnostic {
    crate::package_diagnostic_error(message).with_arg("issue", "package_ts_runtime_failed")
}

/// The compiled runtime for the embedded sources at `runtime_root` (a
/// repository-shaped embedded root), compiling it on first use.
pub(crate) fn compiled_runtime(runtime_root: &Path) -> Result<PathBuf, Diagnostic> {
    static STAGING_SEQ: AtomicU64 = AtomicU64::new(0);

    let name = runtime_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("faber-runtime");
    let parent = runtime_root.parent().unwrap_or(runtime_root);
    let compiled = parent.join(name.replacen("faber-runtime", "faber-ts-runtime", 1));
    if compiled.join(COMPLETE_MARKER).is_file() {
        return Ok(compiled);
    }
    let staging = parent.join(format!(
        ".{}.{}.{}.tmp",
        compiled
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("faber-ts-runtime"),
        std::process::id(),
        STAGING_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    fs::remove_dir_all(&staging).ok();
    let result = compile_into(runtime_root, &staging);
    if let Err(error) = result {
        fs::remove_dir_all(&staging).ok();
        return Err(error);
    }
    if fs::rename(&staging, &compiled).is_err() {
        // A concurrent build finished first: use its directory.
        fs::remove_dir_all(&staging).ok();
        if !compiled.join(COMPLETE_MARKER).is_file() {
            return Err(runtime_error(format!(
                "cannot place the compiled TypeScript runtime at '{}'",
                compiled.display()
            )));
        }
    }
    Ok(compiled)
}

fn compile_into(runtime_root: &Path, staging: &Path) -> Result<(), Diagnostic> {
    let sources = sources_dir(runtime_root);
    let tsc = require_tool(&["tsc".to_owned()])?;
    let output = Command::new(&tsc)
        .arg("-p")
        .arg(sources.join("tsconfig.json"))
        .arg("--outDir")
        .arg(staging)
        .current_dir(&sources)
        .output()
        .map_err(|err| runtime_error(format!("failed to execute `tsc`: {err}")))?;
    if !output.status.success() {
        return Err(runtime_error(format!(
            "`tsc` failed to compile the embedded TypeScript runtime:\n{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    let source_manifest: serde_json::Value = fs::read_to_string(sources.join("package.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let manifest = serde_json::json!({
        "name": source_manifest["name"].as_str().unwrap_or("@faber/runtime"),
        "version": source_manifest["version"].as_str().unwrap_or("0.0.0"),
        "private": true,
        "type": "module",
        "main": "index.js",
        "types": "index.d.ts",
        "exports": {
            ".": { "types": "./index.d.ts", "default": "./index.js" },
            "./*": { "types": "./*.d.ts", "default": "./*.js" }
        }
    });
    let text = serde_json::to_string_pretty(&manifest)
        .map_err(|err| runtime_error(format!("cannot render the runtime package.json: {err}")))?;
    fs::write(staging.join("package.json"), format!("{text}\n"))
        .map_err(|err| runtime_error(format!("cannot write the runtime package.json: {err}")))?;
    fs::write(staging.join(COMPLETE_MARKER), "")
        .map_err(|err| runtime_error(format!("cannot mark the compiled runtime: {err}")))
}

/// Copy the compiled runtime into `<project>/node_modules/@faber/runtime`,
/// replacing any earlier copy.
pub(crate) fn install_runtime(compiled: &Path, project: &Path) -> Result<PathBuf, Diagnostic> {
    let destination = project.join("node_modules/@faber/runtime");
    if destination.exists() {
        fs::remove_dir_all(&destination).map_err(|err| {
            runtime_error(format!("cannot replace '{}': {err}", destination.display()))
        })?;
    }
    copy_tree(compiled, &destination).map_err(|err| {
        runtime_error(format!(
            "cannot copy the TypeScript runtime into '{}': {err}",
            destination.display()
        ))
    })?;
    Ok(destination)
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == COMPLETE_MARKER {
            continue;
        }
        let target = to.join(&name);
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
