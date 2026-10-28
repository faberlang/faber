//! Runtime support sources carried in the `faber` binary.
//!
//! `build.rs` embeds every file under `runtime/{rust,go,typescript,swift,llvm}`
//! and `crates/radix-abi` (runtime/llvm depends on it by relative path); a
//! root the tree does not have is not embedded.
//! An installed binary has no repository checkout, so
//! [`embedded_runtime_root`] materializes the table once under a cache
//! directory and returns it as a repository-shaped root: generated manifests
//! then reference `<root>/runtime/rust` as a path dependency.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

include!(concat!(env!("OUT_DIR"), "/runtime_embedded.rs"));

const COMPLETE_MARKER: &str = ".faber-runtime-complete";

/// Repository-shaped root holding the embedded runtime sources, extracted on
/// first use. `None` on any I/O failure (callers fail closed).
pub(crate) fn embedded_runtime_root() -> Option<PathBuf> {
    embedded_runtime_root_in(&cache_dir())
}

/// [`embedded_runtime_root`] under an explicit cache directory.
fn embedded_runtime_root_in(parent: &Path) -> Option<PathBuf> {
    let root = parent.join(format!("faber-runtime-{EMBEDDED_RUNTIME_DIGEST}"));
    if root.join(COMPLETE_MARKER).is_file() {
        return Some(root);
    }
    materialize(parent, &root).ok()?;
    root.join(COMPLETE_MARKER).is_file().then_some(root)
}

pub(crate) fn cache_dir() -> PathBuf {
    let nonempty = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
    if let Some(dir) = nonempty("XDG_CACHE_HOME") {
        return PathBuf::from(dir).join("faber");
    }
    if let Some(home) = nonempty("HOME") {
        return PathBuf::from(home).join(".cache").join("faber");
    }
    std::env::temp_dir().join("faber")
}

/// Write the table into a sibling temp dir, then rename it onto `root`. A lost
/// rename race (the winner's directory already exists) is success.
fn materialize(parent: &Path, root: &Path) -> io::Result<()> {
    // Unique per call: concurrent callers in one process (parallel tests,
    // threads) must not share, write into, or discard each other's staging.
    static STAGING_SEQ: AtomicU64 = AtomicU64::new(0);
    fs::create_dir_all(parent)?;
    let staging = parent.join(format!(
        ".faber-runtime-{EMBEDDED_RUNTIME_DIGEST}.{}.{}.tmp",
        std::process::id(),
        STAGING_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    discard_staging(&staging);
    let written = write_table(&staging).and_then(|()| fs::rename(&staging, root));
    if written.is_err() {
        discard_staging(&staging);
        // Another caller completed first: use its directory.
        if root.join(COMPLETE_MARKER).is_file() {
            return Ok(());
        }
    }
    written
}

/// Remove a staging directory on a best-effort basis. The directory is usually
/// absent, and a failed removal only leaves a stale temp dir; neither changes
/// the materialize outcome, which is decided by the write and the rename.
fn discard_staging(staging: &Path) {
    fs::remove_dir_all(staging).ok();
}

fn write_table(dir: &Path) -> io::Result<()> {
    for (relative, bytes) in EMBEDDED_RUNTIME {
        let path = dir.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes)?;
    }
    fs::write(dir.join(COMPLETE_MARKER), EMBEDDED_RUNTIME_DIGEST)
}

#[cfg(test)]
#[path = "embedded_runtime_test.rs"]
mod tests;
