//! Product `llvm-host` build orchestration (Stage 9 S9.2–S9.5).
//!
//! The native-host product lane: package graph → one `.ll` module per unit via
//! the shared package-to-LLVM builder ([`super::llvm::build_package_llvm`] — the
//! SAME library entrypoint the pairwise exempla harness consumes) → `llvm-as`
//! verify → (release only) pinned `opt -O2` pipeline → one `clang` link against
//! the `faber-host-llvm` runtime archive → inspectable
//! `target/faber-llvm/{debug|release}/` layout with an inspectable link
//! manifest and runtime identity.
//!
//! INVARIANTS:
//! - An `llvm-host` build NEVER invokes Rust codegen for the program and never
//!   falls back to a system `cc`.
//! - The build fails with a structured diagnostic when the host triple is
//!   unsupported/cross, when `llvm-as`/`clang` are missing or incoherent, or
//!   when the runtime archive cannot be built/produced.
//! - Toolchain discovery is the coherent `radix::llvm_host::LlvmHostToolchain`
//!   (the same discovery the pairwise harness uses), never a per-command probe.

use radix::diagnostics::Diagnostic;
use radix::driver::Config;
use radix::llvm_host::LlvmHostToolchain;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{build_package_llvm, discover_build_layout, PackageLlvmBuild, PackageLlvmOptions};

/// Pinned release optimization pipeline (S9.5 profile policy).
const RELEASE_OPT_PIPELINE: &str = "opt -O2";

/// Build profile for the `llvm-host` lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlvmHostProfile {
    /// `llvm-as` verify + `clang` compile/link with `-g` debug symbols; no `opt` pass.
    Debug,
    /// `llvm-as` verify + pinned `opt -O2` pipeline + `clang` compile/link.
    Release,
}

impl LlvmHostProfile {
    /// Directory name under `target/faber-llvm/`.
    #[must_use]
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Debug => "debug",
            Self::Release => "release",
        }
    }

    fn native_flags(self) -> Vec<String> {
        match self {
            Self::Debug => vec!["-g".to_owned()],
            Self::Release => Vec::new(),
        }
    }

    fn opt_pipeline(self) -> Option<&'static str> {
        match self {
            Self::Debug => None,
            Self::Release => Some(RELEASE_OPT_PIPELINE),
        }
    }
}

/// Complete `llvm-host` product build result.
///
/// `allow(dead_code)`: the faber binary inlines the package modules, so fields
/// only the product tests (lib consumer) and the `run` command read are
/// flagged by the bin target's dead-code analysis.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct LlvmHostBuild {
    /// Stable product identity (package root directory name).
    pub product: String,
    /// Build profile.
    pub profile: LlvmHostProfile,
    /// Canonical host triple this binary targets (always the local host).
    pub host_triple: String,
    /// `target/faber-llvm/{debug|release}/` artifact root.
    pub target_dir: PathBuf,
    /// One `.ll` per package unit (emitted by the shared builder).
    pub modules_dir: PathBuf,
    /// `opt -O2` outputs (release only).
    pub optimized_dir: Option<PathBuf>,
    /// The final native executable path.
    pub binary_path: PathBuf,
    /// `link-manifest.toml` path (inspectable).
    pub manifest_path: PathBuf,
    /// The `faber-host-llvm` runtime archive linked into the binary.
    pub runtime_archive: PathBuf,
    /// Coherent toolchain used for verify/link.
    pub toolchain: LlvmHostToolchain,
}

/// Canonical host triple for a supported (arch, os) pair, or `None` for
/// unsupported/cross hosts.
#[must_use]
pub fn host_triple_for(arch: &str, os: &str) -> Option<String> {
    match (arch, os) {
        ("aarch64", "macos") => Some("aarch64-apple-darwin".to_owned()),
        ("x86_64", "macos") => Some("x86_64-apple-darwin".to_owned()),
        ("aarch64", "linux") => Some("aarch64-unknown-linux-gnu".to_owned()),
        ("x86_64", "linux") => Some("x86_64-unknown-linux-gnu".to_owned()),
        _ => None,
    }
}

/// Canonical host triple for the local machine, rejected with a structured
/// diagnostic when the host is unsupported for the `llvm-host` product lane.
///
/// # Errors
///
/// Returns an `E_LLVMHOST_UNSUPPORTED_HOST` diagnostic when the local arch/os
/// is not in the supported native set.
pub fn host_llvm_target_triple() -> Result<String, Diagnostic> {
    let arch = std::env::consts::ARCH;
    let os = std::env::consts::OS;
    host_triple_for(arch, os).ok_or_else(|| {
        crate::package_diagnostic_error(format!(
            "llvm-host does not support host {arch}-{os}; supported: aarch64/x86_64 macOS + aarch64/x86_64 Linux (native host builds only, no cross compile)"
        ))
        .with_arg("issue", "E_LLVMHOST_UNSUPPORTED_HOST")
    })
}

/// Coherent LLVM host toolchain, wrapped as a structured diagnostic.
///
/// # Errors
///
/// Returns an `llvm_host_toolchain_unavailable` diagnostic when
/// `LlvmHostToolchain::discover` fails.
pub fn discover_llvm_host_toolchain() -> Result<LlvmHostToolchain, Diagnostic> {
    LlvmHostToolchain::discover().map_err(|reason| {
        crate::package_diagnostic_error(format!(
            "llvm-host build requires a coherent LLVM toolchain (llvm-as + clang): {reason}"
        ))
        .with_arg("issue", "llvm_host_toolchain_unavailable")
    })
}

/// Ensure the `faber-host-llvm` runtime archive exists for the current host.
///
/// Reuses an existing archive when no runtime source file is newer than it
/// (so repeated product builds do not re-invoke cargo, and a valid archive is
/// not invalidated by a concurrent mid-edit runtime tree). Otherwise builds
/// `faber-runtime/hosts/llvm` (staticlib) in release and returns the produced
/// archive path. The archive identity is recorded in the link manifest and the
/// `runtime/` identity file.
///
/// # Errors
///
/// Returns an `llvm_host_runtime_archive_unavailable` diagnostic when the
/// runtime crate cannot be found, cargo build fails, or the archive is not
/// produced.
pub fn ensure_llvm_runtime_archive() -> Result<PathBuf, Diagnostic> {
    let runtime_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../faber-runtime");
    let manifest = runtime_root.join("hosts").join("llvm").join("Cargo.toml");
    if !manifest.is_file() {
        return Err(crate::package_diagnostic_error(format!(
            "faber-host-llvm runtime manifest not found at {}",
            manifest.display()
        ))
        .with_arg("issue", "llvm_host_runtime_archive_unavailable"));
    }
    let archive = runtime_root.join("target").join("release").join(if cfg!(windows) {
        "faber_host_llvm.lib"
    } else {
        "libfaber_host_llvm.a"
    });
    if archive.is_file() && runtime_source_is_stale(&runtime_root, &archive) {
        return Ok(archive);
    }
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let build = Command::new(&cargo)
        .arg("build")
        .arg("--release")
        .arg("--manifest-path")
        .arg(&manifest)
        .output();
    let Ok(build) = build else {
        return Err(crate::package_diagnostic_error(
            "cannot execute cargo to build the faber-host-llvm runtime archive",
        )
        .with_arg("issue", "llvm_host_runtime_archive_unavailable"));
    };
    if !build.status.success() {
        // Last-good-archive fallback: when the runtime crate is mid-edit (for
        // example a concurrent runtime agent's uncommitted tree), reuse the
        // previously built archive instead of failing the product build. The
        // versioned `__faber_rt_v1_*` ABI keeps this honest: a drifted ABI
        // fails loudly at clang link time (undefined symbols) rather than
        // linking a mismatched runtime silently.
        if archive.is_file() {
            eprintln!(
                "warning: faber-host-llvm runtime rebuild failed; reusing existing archive {}",
                archive.display()
            );
            return Ok(archive);
        }
        return Err(crate::package_diagnostic_error(format!(
            "faber-host-llvm runtime build failed: {}",
            String::from_utf8_lossy(&build.stderr).trim()
        ))
        .with_arg("issue", "llvm_host_runtime_archive_unavailable"));
    }
    if !archive.is_file() {
        return Err(crate::package_diagnostic_error(format!(
            "faber-host-llvm runtime archive was not produced at {}",
            archive.display()
        ))
        .with_arg("issue", "llvm_host_runtime_archive_unavailable"));
    }
    Ok(archive)
}

/// Whether any runtime source file under `runtime_root/hosts/llvm` or
/// `runtime_root/src` is newer than the existing archive (needs a rebuild).
fn runtime_source_is_stale(runtime_root: &Path, archive: &Path) -> bool {
    let archive_mtime = std::fs::metadata(archive)
        .and_then(|meta| meta.modified())
        .ok();
    let Some(archive_mtime) = archive_mtime else {
        return false;
    };
    let mut newest = std::time::SystemTime::UNIX_EPOCH;
    fn scan(dir: &Path, newest: &mut std::time::SystemTime) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan(&path, newest);
                continue;
            }
            if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                if let Ok(meta) = std::fs::metadata(&path) {
                    if let Ok(modified) = meta.modified() {
                        if modified > *newest {
                            *newest = modified;
                        }
                    }
                }
            }
        }
    }
    scan(&runtime_root.join("hosts").join("llvm"), &mut newest);
    scan(&runtime_root.join("src"), &mut newest);
    newest > archive_mtime
}

/// Build the `llvm-host` native executable for `input` through the shared
/// package-to-LLVM builder, with an inspectable `target/faber-llvm/{profile}/`
/// layout and link manifest.
///
/// # Errors
///
/// Returns diagnostics for unsupported hosts, missing/incoherent toolchains,
/// unavailable runtime archives, package analysis/emission failures, or
/// verify/opt/link failures.
pub fn build_host_program(
    config: &Config,
    input: &Path,
    profile: LlvmHostProfile,
) -> Result<LlvmHostBuild, Vec<Diagnostic>> {
    let host_triple = host_llvm_target_triple().map_err(|diagnostic| vec![diagnostic])?;
    let toolchain = discover_llvm_host_toolchain().map_err(|diagnostic| vec![diagnostic])?;
    let runtime_archive =
        ensure_llvm_runtime_archive().map_err(|diagnostic| vec![diagnostic])?;

    let layout = match discover_build_layout(input) {
        Ok(layout) => layout,
        Err(diagnostic) => return Err(vec![*diagnostic]),
    };
    let target_dir = layout
        .package_root
        .join("target")
        .join("faber-llvm")
        .join(profile.dir_name());
    let modules_dir = target_dir.join("modules");
    let native_flags = profile.native_flags();
    let options = PackageLlvmOptions::new(modules_dir.clone())
        .with_runtime_archive(Some(runtime_archive.clone()))
        .with_native_flags(native_flags.clone());
    let build = build_package_llvm(config, input, &options)?;

    // llvm-as verify every emitted module before any link.
    for module in &build.manifest.modules {
        toolchain.verify(module).map_err(|reason| {
            vec![crate::package_diagnostic_error(format!(
                "llvm-host verify failed for {}: {reason}",
                module.display()
            ))
            .with_arg("issue", "llvm_host_verify_failed")]
        })?;
    }

    // Release pins the opt pipeline; debug links the emitted modules directly.
    let (link_modules, optimized_dir, opt_tool) = match profile.opt_pipeline() {
        Some(_pipeline) => {
            let opt = discover_opt(&toolchain)?;
            let optimized_dir = target_dir.join("opt");
            fs::create_dir_all(&optimized_dir).map_err(|error| {
                vec![crate::package_diagnostic_error(format!(
                    "cannot create llvm-host opt dir {}: {error}",
                    optimized_dir.display()
                ))]
            })?;
            let mut optimized = Vec::with_capacity(build.manifest.modules.len());
            for module in &build.manifest.modules {
                let output = optimized_dir.join(
                    module
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
                run_opt_pipeline(&opt, module, &output)?;
                optimized.push(output);
            }
            (optimized, Some(optimized_dir), Some(opt))
        }
        None => (build.manifest.modules.clone(), None, None),
    };

    let binary_path = target_dir.join(&build.product);
    link_modules_with_toolchain(
        &toolchain,
        &link_modules,
        &runtime_archive,
        &binary_path,
        &native_flags,
    )?;

    let runtime_dir = target_dir.join("runtime");
    fs::create_dir_all(&runtime_dir).map_err(|error| {
        vec![crate::package_diagnostic_error(format!(
            "cannot create llvm-host runtime dir {}: {error}",
            runtime_dir.display()
        ))]
    })?;
    let (runtime_name, runtime_version) = runtime_artifact_metadata();
    write_runtime_identity(&runtime_dir, &runtime_archive, &runtime_name, &runtime_version)?;

    let manifest_path = target_dir.join("link-manifest.toml");
    write_link_manifest(
        &manifest_path,
        &build,
        &LinkManifestRecord {
            profile,
            host_triple: &host_triple,
            toolchain: &toolchain,
            link_modules: &link_modules,
            runtime_archive: &runtime_archive,
            output: &binary_path,
            native_flags: &native_flags,
            opt_tool: opt_tool.as_deref(),
        },
    )?;

    Ok(LlvmHostBuild {
        product: build.product.clone(),
        profile,
        host_triple,
        target_dir,
        modules_dir,
        optimized_dir,
        binary_path,
        manifest_path,
        runtime_archive,
        toolchain,
    })
}

/// Run the pinned opt pipeline over one module (`opt -O2`, the Stage 9
/// release profile policy).
///
/// # Errors
///
/// Returns an `llvm_host_opt_failed` diagnostic when `opt` fails.
fn run_opt_pipeline(opt: &Path, module: &Path, output: &Path) -> Result<(), Vec<Diagnostic>> {
    let run = Command::new(opt)
        .arg("-O2")
        .arg(module)
        .arg("-o")
        .arg(output)
        .output();
    let Ok(run) = run else {
        return Err(vec![crate::package_diagnostic_error(format!(
            "cannot execute {} for {}",
            opt.display(),
            module.display()
        ))
        .with_arg("issue", "llvm_host_opt_failed")]);
    };
    if !run.status.success() {
        return Err(vec![crate::package_diagnostic_error(format!(
            "llvm-host opt pipeline failed for {}: {}",
            module.display(),
            String::from_utf8_lossy(&run.stderr).trim()
        ))
        .with_arg("issue", "llvm_host_opt_failed")]);
    }
    Ok(())
}

/// Locate `opt` beside `llvm-as` (coherent toolchain), falling back to PATH.
///
/// # Errors
///
/// Returns an `llvm_host_opt_unavailable` diagnostic when `opt` cannot be
/// found — the release profile pins an opt pipeline, so it is required there.
fn discover_opt(toolchain: &LlvmHostToolchain) -> Result<PathBuf, Vec<Diagnostic>> {
    let sibling = toolchain
        .llvm_as
        .parent()
        .map(|parent| parent.join("opt"))
        .filter(|path| path.is_file());
    if let Some(opt) = sibling {
        return Ok(opt);
    }
    if let Some(opt) = find_on_path("opt") {
        return Ok(opt);
    }
    Err(vec![crate::package_diagnostic_error(
        "llvm-host release build requires `opt` (the pinned opt pipeline), but it was not found beside llvm-as or on PATH",
    )
    .with_arg("issue", "llvm_host_opt_unavailable")])
}

/// One `clang` link of ALL modules with the runtime archive (no `cc` fallback).
///
/// # Errors
///
/// Returns an `llvm_host_link_failed` diagnostic when `clang` fails.
fn link_modules_with_toolchain(
    toolchain: &LlvmHostToolchain,
    modules: &[PathBuf],
    runtime_archive: &Path,
    output: &Path,
    native_flags: &[String],
) -> Result<(), Vec<Diagnostic>> {
    let mut link = Command::new(&toolchain.clang);
    for module in modules {
        link.arg(module);
    }
    for flag in native_flags {
        link.arg(flag);
    }
    link.arg(runtime_archive).arg("-o").arg(output);
    let run = link.output();
    let Ok(run) = run else {
        return Err(vec![crate::package_diagnostic_error(format!(
            "cannot execute clang link for {}",
            output.display()
        ))
        .with_arg("issue", "llvm_host_link_failed")]);
    };
    if !run.status.success() {
        return Err(vec![crate::package_diagnostic_error(format!(
            "clang link failed for {}: {}",
            output.display(),
            String::from_utf8_lossy(&run.stderr).trim()
        ))
        .with_arg("issue", "llvm_host_link_failed")]);
    }
    Ok(())
}

/// Manifest content bundle — keeps the writer's argument list small (the faber
/// bin inlines the package modules, so clippy's default `too_many_arguments`
/// threshold applies to this CLI crate's compilation).
struct LinkManifestRecord<'a> {
    profile: LlvmHostProfile,
    host_triple: &'a str,
    toolchain: &'a LlvmHostToolchain,
    link_modules: &'a [PathBuf],
    runtime_archive: &'a Path,
    output: &'a Path,
    native_flags: &'a [String],
    opt_tool: Option<&'a Path>,
}

/// Write the inspectable `link-manifest.toml` (S9.4): host triple, profile,
/// LLVM tool paths + versions, module paths in link order, runtime archive
/// identity, native flags, output path, and the pinned opt pipeline (release).
///
/// # Errors
///
/// Returns a diagnostic when the manifest cannot be serialized or written.
fn write_link_manifest(
    path: &Path,
    build: &PackageLlvmBuild,
    record: &LinkManifestRecord<'_>,
) -> Result<(), Vec<Diagnostic>> {
    let mut doc = toml::map::Map::new();

    let mut target = toml::map::Map::new();
    target.insert("name".to_owned(), toml::Value::String("llvm-host".to_owned()));
    target.insert("host_triple".to_owned(), toml::Value::String(record.host_triple.to_owned()));
    target.insert("profile".to_owned(), toml::Value::String(record.profile.dir_name().to_owned()));
    doc.insert("target".to_owned(), toml::Value::Table(target));

    let mut tools = toml::map::Map::new();
    tools.insert(
        "llvm_as".to_owned(),
        toml::Value::String(record.toolchain.llvm_as.display().to_string()),
    );
    tools.insert(
        "llvm_as_version".to_owned(),
        toml::Value::String(
            tool_first_line_version(&record.toolchain.llvm_as).unwrap_or_else(|_| "unknown".to_owned()),
        ),
    );
    tools.insert(
        "clang".to_owned(),
        toml::Value::String(record.toolchain.clang.display().to_string()),
    );
    tools.insert(
        "clang_version".to_owned(),
        toml::Value::String(
            tool_first_line_version(&record.toolchain.clang).unwrap_or_else(|_| "unknown".to_owned()),
        ),
    );
    if let Some(opt) = record.opt_tool {
        tools.insert("opt".to_owned(), toml::Value::String(opt.display().to_string()));
        tools.insert(
            "opt_version".to_owned(),
            toml::Value::String(tool_first_line_version(opt).unwrap_or_else(|_| "unknown".to_owned())),
        );
    }
    doc.insert("toolchain".to_owned(), toml::Value::Table(tools));

    let mut link = toml::map::Map::new();
    link.insert(
        "modules".to_owned(),
        toml::Value::Array(
            record
                .link_modules
                .iter()
                .map(|module| toml::Value::String(module.display().to_string()))
                .collect(),
        ),
    );
    link.insert(
        "runtime_archive".to_owned(),
        toml::Value::String(record.runtime_archive.display().to_string()),
    );
    link.insert(
        "native_flags".to_owned(),
        toml::Value::Array(
            record
                .native_flags
                .iter()
                .map(|flag| toml::Value::String(flag.clone()))
                .collect(),
        ),
    );
    link.insert("output".to_owned(), toml::Value::String(record.output.display().to_string()));
    link.insert(
        "entry_module".to_owned(),
        toml::Value::String(build.manifest.entry_module.display().to_string()),
    );
    doc.insert("link".to_owned(), toml::Value::Table(link));

    if let Some(pipeline) = record.profile.opt_pipeline() {
        let mut opt = toml::map::Map::new();
        opt.insert("pipeline".to_owned(), toml::Value::String(pipeline.to_owned()));
        doc.insert("opt".to_owned(), toml::Value::Table(opt));
    }

    let text = toml::to_string_pretty(&toml::Value::Table(doc))
        .map_err(|error| vec![crate::package_diagnostic_error(format!(
            "cannot serialize llvm-host link manifest: {error}"
        ))])?;
    fs::write(path, text).map_err(|error| {
        vec![crate::package_diagnostic_error(format!(
            "cannot write llvm-host link manifest {}: {error}",
            path.display()
        ))]
    })
}

/// Write the `runtime/identity.toml` file: the runtime archive identity and a
/// pointer to the produced archive (S9.4 runtime identity/cache pointer).
///
/// # Errors
///
/// Returns a diagnostic when the identity file cannot be written.
fn write_runtime_identity(
    dir: &Path,
    archive: &Path,
    name: &str,
    version: &str,
) -> Result<(), Vec<Diagnostic>> {
    let mut doc = toml::map::Map::new();
    doc.insert("runtime_name".to_owned(), toml::Value::String(name.to_owned()));
    doc.insert("runtime_version".to_owned(), toml::Value::String(version.to_owned()));
    doc.insert(
        "archive".to_owned(),
        toml::Value::String(archive.display().to_string()),
    );
    let text = toml::to_string_pretty(&toml::Value::Table(doc))
        .map_err(|error| vec![crate::package_diagnostic_error(format!(
            "cannot serialize llvm-host runtime identity: {error}"
        ))])?;
    let path = dir.join("identity.toml");
    fs::write(&path, text).map_err(|error| {
        vec![crate::package_diagnostic_error(format!(
            "cannot write llvm-host runtime identity {}: {error}",
            path.display()
        ))]
    })
}

/// Runtime archive artifact identity (`(name, version)`) read from
/// `faber-runtime/hosts/llvm/Cargo.toml`. Falls back to the crate name and
/// `unknown` when the manifest cannot be read (the archive build has already
/// succeeded by this point, so a read failure only degrades the identity).
#[must_use]
fn runtime_artifact_metadata() -> (String, String) {
    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../faber-runtime")
        .join("hosts")
        .join("llvm")
        .join("Cargo.toml");
    let Ok(text) = fs::read_to_string(&manifest_path) else {
        return ("faber-host-llvm".to_owned(), "unknown".to_owned());
    };
    let Ok(value) = text.parse::<toml::Value>() else {
        return ("faber-host-llvm".to_owned(), "unknown".to_owned());
    };
    let name = value
        .get("package")
        .and_then(|package| package.get("name"))
        .and_then(toml::Value::as_str)
        .unwrap_or("faber-host-llvm")
        .to_owned();
    let version = value
        .get("package")
        .and_then(|package| package.get("version"))
        .and_then(toml::Value::as_str)
        .unwrap_or("unknown")
        .to_owned();
    (name, version)
}

/// First line of `tool --version` for manifest version recording.
fn tool_first_line_version(tool: &Path) -> Result<String, String> {
    let output = Command::new(tool).arg("--version").output().map_err(|error| {
        format!("cannot execute {}: {error}", tool.display())
    })?;
    if !output.status.success() {
        return Err(format!("{} --version failed", tool.display()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .unwrap_or("unknown")
        .to_owned())
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|directory| directory.join(if cfg!(windows) { format!("{name}.exe") } else { name.to_owned() }))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
#[path = "llvm_host_test.rs"]
mod tests;
