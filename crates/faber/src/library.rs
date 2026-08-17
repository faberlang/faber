//! Library import resolution for package compilation (faber seam).
//!
//! RP2c: the resolver family (specifier → interface file) moved to
//! `radix-package` with the explicit-input posture — `radix-package` never
//! reads the ambient environment. This module keeps the faber-owned ambient
//! discovery seam — `FABER_LIBRARY_HOME` env reading and the
//! workspace-ancestor probe (CTO memo) — and constructs the explicit
//! resolver inputs the compiler crate requires. The resolver types are supplied
//! by `radix-package`; this module adds the Faber-only ambient discovery seam.

#[cfg(test)]
#[path = "library_test.rs"]
mod tests;

use std::path::{Path, PathBuf};

use radix_program::library::LibraryResolver;

pub(crate) const FABER_LIBRARY_HOME_ENV: &str = "FABER_LIBRARY_HOME";
/// Env flag exercised by library probe tests; production probe path is not wired yet.
#[cfg(test)]
pub(crate) const FABER_ENABLE_WORKSPACE_LIBRARY_PROBE_ENV: &str =
    "FABER_ENABLE_WORKSPACE_LIBRARY_PROBE";

/// Build a resolver from the ambient library home: an explicit
/// `FABER_LIBRARY_HOME`, or the sibling workspace dev layout when present.
pub(crate) fn default_library_resolver() -> LibraryResolver {
    LibraryResolver::with_optional_home(default_library_home())
}

fn default_library_home() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os(FABER_LIBRARY_HOME_ENV) {
        return Some(PathBuf::from(value));
    }

    // Public faber repo is a sibling of norma under faberlang/. Probe
    // workspace automatically in development — no opt-in env var required.
    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."));
    for candidate in workspace_root.ancestors() {
        let norma_src = candidate.join("norma/src");
        if norma_src.is_dir() && norma_src.join("solum.fab").is_file() {
            return Some(candidate.to_path_buf());
        }
    }

    None
}
