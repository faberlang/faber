//! Library import resolution for package compilation (faber seam).
//!
//! `RP2c`: the resolver family (specifier → interface file) moved to
//! `radix-program` with the explicit-input posture — `radix-program` never
//! reads the ambient environment. This module keeps the faber-owned ambient
//! discovery seam — `FABER_LIBRARY_HOME` env reading and the Norma library
//! embedded in the binary — and constructs the explicit resolver inputs the
//! compiler crate requires.

#[cfg(test)]
#[path = "library_test.rs"]
mod tests;

use std::path::{Path, PathBuf};

use radix::program::library::LibraryResolver;

use crate::package::embedded_runtime::embedded_runtime_root;

pub const FABER_LIBRARY_HOME_ENV: &str = "FABER_LIBRARY_HOME";

/// Well-known sibling library names doctor always reports (found or not).
///
/// The faber lib artifact does not call doctor/check-library; the binary does.
pub const KNOWN_LIBRARIES: &[&str] = &["norma", "triga", "gradus", "tela"];

/// How the library home was selected. Env wins; otherwise the `lib/` the
/// binary materializes from its embedded Norma sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryHomeSource {
    Env,
    Embedded,
}

impl LibraryHomeSource {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::Env => "env",
            Self::Embedded => "embedded",
        }
    }
}

/// Resolved library home plus the provenance `faber doctor` reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedLibraryHome {
    pub path: PathBuf,
    pub source: LibraryHomeSource,
}

/// Identity derived from the import specifier's provider segment.
///
/// WHY: `LibraryProvider` is not a manifest fact. `norma` stays
/// `Builtin("norma")` even when a checkout has `cista.toml` or `faber.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecifierIdentity {
    Builtin,
    Package,
}

impl SpecifierIdentity {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Package => "package",
        }
    }
}

/// On-disk availability at a provider checkout. Presence only — never a parse
/// of `cista.toml` (v1 does not read that schema).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryAvailability {
    FaberPackage,
    CistaSourcePackage,
    NotFound,
}

impl LibraryAvailability {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::FaberPackage => "faber-package",
            Self::CistaSourcePackage => "cista-source-package",
            Self::NotFound => "not-found",
        }
    }
}

/// Build a resolver from the ambient library home: an explicit
/// `FABER_LIBRARY_HOME`, or the embedded `lib/` (holds `norma/`).
pub(crate) fn default_library_resolver() -> LibraryResolver {
    LibraryResolver::with_optional_home(default_library_home())
}

pub(crate) fn default_library_home() -> Option<PathBuf> {
    resolved_library_home().map(|home| home.path)
}

/// Ambient library home with provenance for doctor / check-library reporting.
pub fn resolved_library_home() -> Option<ResolvedLibraryHome> {
    if let Some(value) = std::env::var_os(FABER_LIBRARY_HOME_ENV) {
        return Some(ResolvedLibraryHome {
            path: PathBuf::from(value),
            source: LibraryHomeSource::Env,
        });
    }

    // Embedded home: `lib/` (holds `norma/`) under the materialized runtime
    // root, so a released or relocated binary needs no checkout.
    let embedded = embedded_runtime_root()?.join("lib");
    embedded
        .join("norma")
        .is_dir()
        .then_some(ResolvedLibraryHome {
            path: embedded,
            source: LibraryHomeSource::Embedded,
        })
}

/// Identity from the provider name in an import specifier (`norma:fs`).
///
/// Never inspects a manifest. Passing `faber.toml` / `cista.toml` metadata
/// here would be the U3 identity-guardrail defect.
pub fn specifier_identity(provider: &str) -> SpecifierIdentity {
    if provider == "norma" {
        SpecifierIdentity::Builtin
    } else {
        SpecifierIdentity::Package
    }
}

/// Classify a provider checkout without parsing Cista schema.
pub fn library_availability(provider_repo: &Path) -> LibraryAvailability {
    if !provider_repo.is_dir() {
        return LibraryAvailability::NotFound;
    }
    if provider_repo.join("faber.toml").is_file() {
        return LibraryAvailability::FaberPackage;
    }
    // Presence of `cista.toml` only — never a schema parse.
    if provider_repo.join("cista.toml").is_file() {
        return LibraryAvailability::CistaSourcePackage;
    }
    LibraryAvailability::NotFound
}

/// Resolver root rule for a provider checkout: `faber.toml` else `<repo>/src`.
///
/// Presence only — this does not parse `cista.toml`. `None` means the checkout
/// has neither a Faber manifest nor a `src` directory.
pub fn library_root_rule(provider_repo: &Path) -> Option<LibraryRootRule> {
    if provider_repo.join("faber.toml").is_file() {
        return Some(LibraryRootRule::FaberToml);
    }
    if provider_repo.join("src").is_dir() {
        return Some(LibraryRootRule::SrcFallback);
    }
    None
}

/// Which resolver root rule selected the checkout source tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibraryRootRule {
    FaberToml,
    SrcFallback,
}

impl LibraryRootRule {
    pub fn as_key(self) -> &'static str {
        match self {
            Self::FaberToml => "faber.toml",
            Self::SrcFallback => "src-fallback",
        }
    }
}
