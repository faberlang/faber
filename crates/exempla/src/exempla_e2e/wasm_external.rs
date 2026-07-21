//! External Wasm validation and stub-host execution for the exempla e2e harness.
//!
//! Tier B uses `wasm-tools validate` on module bytes. Tier C/D invoke the
//! optional `radix-wasm-stub-host` binary (built with `--features wasm-stub-host`)
//! via subprocess — never an in-process `wasmtime` link inside `radix`.

use super::common::command_available;
use std::fmt::{self, Display, Formatter};
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmInstantiationBucket {
    NoRuntime,
    MissingImport,
    InstantiationTrap,
    InstantiateValid,
}

impl Display for WasmInstantiationBucket {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoRuntime => write!(f, "no-runtime"),
            Self::MissingImport => write!(f, "missing-import"),
            Self::InstantiationTrap => write!(f, "instantiation-trap"),
            Self::InstantiateValid => write!(f, "instantiate-valid"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WasmRunBucket {
    NoEntryExport,
    EntryTrap,
    Runnable,
}

impl Display for WasmRunBucket {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoEntryExport => write!(f, "no-entry-export"),
            Self::EntryTrap => write!(f, "entry-trap"),
            Self::Runnable => write!(f, "runnable"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmImportSite {
    pub module: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmInstantiationProbe {
    pub bucket: WasmInstantiationBucket,
    pub reason: String,
    pub imports: Vec<WasmImportSite>,
}

impl WasmInstantiationProbe {
    fn with_imports(
        bucket: WasmInstantiationBucket,
        reason: impl Into<String>,
        imports: &[WasmImportSite],
    ) -> Self {
        Self {
            bucket,
            reason: reason.into(),
            imports: imports.to_vec(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasmRunProbe {
    pub bucket: WasmRunBucket,
    pub reason: String,
    pub diag_events: Vec<String>,
}

pub fn parse_wat_import_sites(wat: &str) -> Vec<WasmImportSite> {
    let mut imports = Vec::new();
    let mut cursor = 0usize;
    while let Some(start) = wat[cursor..].find("(import ") {
        let absolute = cursor + start;
        let line_end = wat[absolute..]
            .find('\n')
            .map(|offset| absolute + offset)
            .unwrap_or(wat.len());
        let line = &wat[absolute..line_end];
        if let Some((module, name)) = parse_import_line(line) {
            imports.push(WasmImportSite { module, name });
        }
        cursor = line_end;
    }
    imports
}

fn parse_import_line(line: &str) -> Option<(String, String)> {
    let parts: Vec<_> = line.split('"').collect();
    if parts.len() < 4 {
        return None;
    }
    Some((parts[1].to_owned(), parts[3].to_owned()))
}

pub fn validate_wasm_bytes(wasm_file: &Path) -> Result<(), String> {
    if command_available("wasm-tools", &["--version"]) {
        let output = Command::new("wasm-tools")
            .arg("validate")
            .arg(wasm_file)
            .output()
            .map_err(|err| format!("cannot execute wasm-tools validate: {err}"))?;
        if output.status.success() {
            return Ok(());
        }
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Err("compile validation skipped: wasm-tools not on PATH".to_owned())
}

pub fn probe_wasm_instantiation_stubless(
    wasm_file: &Path,
    imports: &[WasmImportSite],
) -> WasmInstantiationProbe {
    if !imports.is_empty() {
        return WasmInstantiationProbe::with_imports(
            WasmInstantiationBucket::MissingImport,
            format!(
                "stubless host requires unresolved imports: {}",
                summarize_imports(imports)
            ),
            imports,
        );
    }
    if command_available("wasmtime", &["--version"]) {
        let output = Command::new("wasmtime")
            .arg("validate")
            .arg(wasm_file)
            .output();
        match output {
            Ok(output) if output.status.success() => WasmInstantiationProbe::with_imports(
                WasmInstantiationBucket::InstantiateValid,
                "wasmtime validate accepted import-free module",
                imports,
            ),
            Ok(output) => WasmInstantiationProbe::with_imports(
                WasmInstantiationBucket::InstantiationTrap,
                format!(
                    "wasmtime validate failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
                imports,
            ),
            Err(err) => WasmInstantiationProbe::with_imports(
                WasmInstantiationBucket::NoRuntime,
                format!("wasmtime validate unavailable: {err}"),
                imports,
            ),
        }
    } else {
        WasmInstantiationProbe::with_imports(
            WasmInstantiationBucket::NoRuntime,
            "stubless runtime skipped: wasmtime CLI not on PATH",
            imports,
        )
    }
}

pub fn probe_wasm_with_stub_host(wasm_file: &Path) -> WasmInstantiationProbe {
    match run_stub_host(wasm_file, false) {
        Ok(report) => WasmInstantiationProbe {
            bucket: report.instantiate,
            reason: report.instantiate_reason,
            imports: report.imports,
        },
        Err(reason) => WasmInstantiationProbe {
            bucket: WasmInstantiationBucket::NoRuntime,
            reason,
            imports: Vec::new(),
        },
    }
}

pub fn run_wasm_entry_with_stub_host(wasm_file: &Path) -> WasmRunProbe {
    match run_stub_host(wasm_file, true) {
        Ok(report) => WasmRunProbe {
            bucket: report.run,
            reason: report.run_reason,
            diag_events: report.diag_events,
        },
        Err(reason) => WasmRunProbe {
            bucket: WasmRunBucket::EntryTrap,
            reason,
            diag_events: Vec::new(),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StubHostReport {
    instantiate: WasmInstantiationBucket,
    instantiate_reason: String,
    run: WasmRunBucket,
    run_reason: String,
    diag_events: Vec<String>,
    imports: Vec<WasmImportSite>,
}

fn run_stub_host(wasm_file: &Path, invoke_entry: bool) -> Result<StubHostReport, String> {
    let host = locate_stub_host_binary()?;
    let mut command = Command::new(&host);
    command.arg(wasm_file);
    if invoke_entry {
        command.arg("--invoke-entry");
    }
    let output = command
        .output()
        .map_err(|err| format!("cannot execute stub host `{}`: {err}", host.display()))?;
    if !output.status.success() {
        return Err(format!(
            "stub host failed (status {:?}): {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    parse_stub_host_report(&String::from_utf8_lossy(&output.stdout))
}

fn locate_stub_host_binary() -> Result<std::path::PathBuf, String> {
    // WHY: the e2e harness is `#[ignore]` and run infrequently, so we bootstrap
    // the feature-gated stub host on demand rather than requiring the operator
    // to remember a separate build step. `RADIX_WASM_STUB_HOST` overrides this.
    if let Ok(path) = std::env::var("RADIX_WASM_STUB_HOST") {
        let path = std::path::PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(format!(
            "RADIX_WASM_STUB_HOST is not a file: {}",
            path.display()
        ));
    }

    // Stub host lives in the private radix workspace (sibling of faber).
    let radix_root = crate::paths::faberlang_home()
        .map(|home| home.join("radix"))
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../radix")
        });
    for profile in ["debug", "release"] {
        let candidate = radix_root.join(format!("target/{profile}/radix-wasm-stub-host"));
        if candidate.is_file() {
            return Ok(candidate);
        }
    }

    let build = Command::new("cargo")
        .current_dir(&radix_root)
        .args([
            "build",
            "-q",
            "-p",
            "radix",
            "--bin",
            "radix-wasm-stub-host",
            "--features",
            "wasm-stub-host",
        ])
        .status()
        .map_err(|err| format!("cannot build radix-wasm-stub-host: {err}"))?;
    if !build.success() {
        return Err(
            "failed to build radix-wasm-stub-host (enable wasm-stub-host feature)".to_owned(),
        );
    }
    let candidate = radix_root.join("target/debug/radix-wasm-stub-host");
    if candidate.is_file() {
        return Ok(candidate);
    }
    Err("radix-wasm-stub-host binary missing after build".to_owned())
}

fn parse_stub_host_report(stdout: &str) -> Result<StubHostReport, String> {
    let mut instantiate = WasmInstantiationBucket::NoRuntime;
    let mut instantiate_reason = String::new();
    let mut run = WasmRunBucket::EntryTrap;
    let mut run_reason = String::new();
    let mut diag_events = Vec::new();
    let mut imports = Vec::new();

    for line in stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(value) = line.strip_prefix("instantiate=") {
            instantiate = parse_instantiate_bucket(value)?;
            continue;
        }
        if let Some(value) = line.strip_prefix("instantiate_reason=") {
            instantiate_reason = value.to_owned();
            continue;
        }
        if let Some(value) = line.strip_prefix("run=") {
            run = parse_run_bucket(value)?;
            continue;
        }
        if let Some(value) = line.strip_prefix("run_reason=") {
            run_reason = value.to_owned();
            continue;
        }
        if let Some(value) = line.strip_prefix("diag=") {
            diag_events.push(value.to_owned());
            continue;
        }
        if let Some(value) = line.strip_prefix("import=") {
            if let Some((module, name)) = value.split_once("::") {
                imports.push(WasmImportSite {
                    module: module.to_owned(),
                    name: name.to_owned(),
                });
            }
        }
    }

    if instantiate_reason.is_empty() {
        instantiate_reason = instantiate.to_string();
    }
    if run_reason.is_empty() {
        run_reason = run.to_string();
    }

    Ok(StubHostReport {
        instantiate,
        instantiate_reason,
        run,
        run_reason,
        diag_events,
        imports,
    })
}

fn parse_instantiate_bucket(value: &str) -> Result<WasmInstantiationBucket, String> {
    match value {
        "instantiate-valid" => Ok(WasmInstantiationBucket::InstantiateValid),
        "missing-import" => Ok(WasmInstantiationBucket::MissingImport),
        "instantiation-trap" => Ok(WasmInstantiationBucket::InstantiationTrap),
        "no-runtime" => Ok(WasmInstantiationBucket::NoRuntime),
        other => Err(format!("unknown instantiate bucket `{other}`")),
    }
}

fn parse_run_bucket(value: &str) -> Result<WasmRunBucket, String> {
    match value {
        "runnable" => Ok(WasmRunBucket::Runnable),
        "no-entry-export" => Ok(WasmRunBucket::NoEntryExport),
        "entry-trap" => Ok(WasmRunBucket::EntryTrap),
        other => Err(format!("unknown run bucket `{other}`")),
    }
}

fn summarize_imports(imports: &[WasmImportSite]) -> String {
    imports
        .iter()
        .map(|import| format!("{}::{}", import.module, import.name))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
#[path = "wasm_external_test.rs"]
mod tests;
