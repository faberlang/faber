//! Native `clif` product build (goal `radix-mir-clif`, unit C4b).
//!
//! The compiler emits one relocatable object for the host triple
//! ([`radix::module::Output::ClifObject`]); this module writes it and links
//! it, through the system C linker, against the `runtime/clif` archive. The
//! link line carries the platform dead-strip flag: without it the same
//! inputs measured 13 MB and pulled the TLS stack into every program.
//!
//! The archive comes from the checkout's `runtime/clif` crate, built once
//! with cargo (an embedded prebuilt archive replaces this at release,
//! unit C20). A missing checkout, cargo or linker fails closed with a named
//! issue; there is no fallback route.

use radix::module::diagnostics::Diagnostic;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The linker driver. Faber's native route needs only a C linker.
const LINKER: &str = "cc";

/// File name of the runtime archive on this host.
const ARCHIVE_NAME: &str = "libfaber_rt_clif.a";

fn issue(message: impl Into<String>, issue: &str) -> Diagnostic {
    crate::package_diagnostic_error(message).with_arg("issue", issue)
}

/// The own checkout's root, two levels above this crate's manifest directory.
fn checkout_root() -> PathBuf {
    let own = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    fs::canonicalize(&own).unwrap_or(own)
}

/// Platform arguments after the object and archive: dead-strip plus the
/// system libraries a Rust staticlib needs.
fn platform_link_args() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["-Wl,-dead_strip"]
    } else {
        &["-Wl,--gc-sections", "-lm", "-ldl", "-lpthread"]
    }
}

/// Build (or reuse) the `runtime/clif` archive and return its path.
fn ensure_runtime_archive(release: bool) -> Result<PathBuf, Diagnostic> {
    let root = checkout_root();
    let manifest = root.join("runtime").join("clif").join("Cargo.toml");
    if !manifest.is_file() {
        return Err(issue(
            format!(
                "the clif runtime crate is not available at {} (an installed faber embeds it at release)",
                manifest.display()
            ),
            "clif_runtime_archive_unavailable",
        ));
    }
    let target_dir = root.join("target");
    let mut build = Command::new("cargo");
    build
        .args(["build", "--locked", "--manifest-path"])
        .arg(&manifest)
        .arg("--target-dir")
        .arg(&target_dir);
    if release {
        build.arg("--release");
    }
    let output = build.output().map_err(|error| {
        issue(
            format!("cannot run cargo to build the clif runtime: {error}"),
            "clif_runtime_archive_unavailable",
        )
    })?;
    if !output.status.success() {
        return Err(issue(
            format!(
                "building the clif runtime failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
            "clif_runtime_archive_unavailable",
        ));
    }
    let archive = target_dir
        .join(if release { "release" } else { "debug" })
        .join(ARCHIVE_NAME);
    if archive.is_file() {
        Ok(archive)
    } else {
        Err(issue(
            format!(
                "the clif runtime archive is missing at {}",
                archive.display()
            ),
            "clif_runtime_archive_unavailable",
        ))
    }
}

/// Write `object` under `out_dir` and link it into an executable named after
/// `stem`. Returns the executable path.
///
/// # Errors
///
/// `clif_runtime_archive_unavailable` when the runtime archive cannot be
/// built; `clif_link_failed` when the linker is missing or rejects the link.
pub fn link_executable(
    object: &[u8],
    out_dir: &Path,
    stem: &str,
    release: bool,
) -> Result<PathBuf, Diagnostic> {
    fs::create_dir_all(out_dir).map_err(|error| {
        issue(
            format!("failed to create '{}': {error}", out_dir.display()),
            "clif_link_failed",
        )
        .with_arg(
            crate::exit_status::CLASS_ARG,
            crate::exit_status::CLASS_CANT_CREATE,
        )
    })?;
    let object_path = out_dir.join(format!("{stem}.o"));
    fs::write(&object_path, object).map_err(|error| {
        issue(
            format!("failed to write '{}': {error}", object_path.display()),
            "clif_link_failed",
        )
        .with_arg(
            crate::exit_status::CLASS_ARG,
            crate::exit_status::CLASS_CANT_CREATE,
        )
    })?;
    let archive = ensure_runtime_archive(release)?;
    let executable = out_dir.join(stem);
    let output = Command::new(LINKER)
        .arg(&object_path)
        .arg(&archive)
        .args(platform_link_args())
        .arg("-o")
        .arg(&executable)
        .output()
        .map_err(|error| {
            issue(
                format!("cannot run the linker '{LINKER}': {error}"),
                "clif_link_failed",
            )
        })?;
    if !output.status.success() {
        return Err(issue(
            format!(
                "linking with '{LINKER}' failed: {}",
                String::from_utf8_lossy(&output.stderr)
            ),
            "clif_link_failed",
        ));
    }
    Ok(executable)
}
