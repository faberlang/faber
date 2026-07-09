//! Faber build lockfile (`faber.lock`) consumption.
//!
//! Absolute paths only. The package manager writes this file; faber never
//! discovers a package store root or environment variable for installs.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use radix::diagnostics::Diagnostic;
use serde::Deserialize;

pub(crate) const LOCK_FILE: &str = "faber.lock";

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct FaberLock {
    #[serde(default, rename = "package")]
    pub packages: Vec<LockedPackage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(dead_code)] // full lock schema is retained for diagnostics and future link work
pub(crate) struct LockedPackage {
    pub name: String,
    pub version: String,
    pub source: String,
    pub package_root: String,
    pub kind: String,
    pub target_language: String,
    pub target_triple: String,
    pub target_manifest: String,
    pub interface_root: String,
    pub artifact: String,
    #[serde(rename = "crate")]
    pub crate_name: String,
    pub rustc: String,
}

impl LockedPackage {
    pub(crate) fn interface_root_path(&self) -> PathBuf {
        PathBuf::from(&self.interface_root)
    }

    pub(crate) fn artifact_path(&self) -> PathBuf {
        PathBuf::from(&self.artifact)
    }

    pub(crate) fn target_manifest_path(&self) -> PathBuf {
        PathBuf::from(&self.target_manifest)
    }
}

/// Read `faber.lock` from a package root when present.
pub(crate) fn read_lock(package_root: &Path) -> Result<Option<FaberLock>, Box<Diagnostic>> {
    let path = package_root.join(LOCK_FILE);
    if !path.is_file() {
        return Ok(None);
    }
    let source =
        fs::read_to_string(&path).map_err(|err| Box::new(Diagnostic::io_error(&path, err)))?;
    let lock = toml::from_str::<FaberLock>(&source).map_err(|err| {
        Box::new(
            Diagnostic::error(format!("invalid {LOCK_FILE}: {err}"))
                .with_file(path.display().to_string())
                .with_arg("issue", "invalid_faber_lock"),
        )
    })?;
    Ok(Some(lock))
}

/// Index locked packages by name.
pub(crate) fn lock_index(lock: &FaberLock) -> BTreeMap<String, LockedPackage> {
    let mut map = BTreeMap::new();
    for package in &lock.packages {
        map.insert(package.name.clone(), package.clone());
    }
    map
}

/// Validate that declared exact dependencies are present in the lock with matching versions
/// and that locked paths exist on disk.
pub(crate) fn validate_dependencies_against_lock(
    package_root: &Path,
    dependencies: &BTreeMap<String, String>,
    lock: Option<&FaberLock>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    if dependencies.is_empty() {
        return diagnostics;
    }
    let Some(lock) = lock else {
        diagnostics.push(
            Diagnostic::error(format!(
                "faber.toml declares dependencies but {} is missing; install packages with the package manager first",
                package_root.join(LOCK_FILE).display()
            ))
            .with_file(package_root.join(LOCK_FILE).display().to_string())
            .with_arg("issue", "missing_faber_lock"),
        );
        return diagnostics;
    };
    let index = lock_index(lock);
    for (name, version) in dependencies {
        let Some(locked) = index.get(name) else {
            diagnostics.push(
                Diagnostic::error(format!(
                    "dependency `{name} = \"{version}\"` is declared in faber.toml but missing from {LOCK_FILE}"
                ))
                .with_file(package_root.join(LOCK_FILE).display().to_string())
                .with_arg("issue", "dependency_missing_from_lock")
                .with_arg("package", name.clone())
                .with_arg("version", version.clone()),
            );
            continue;
        };
        if &locked.version != version {
            diagnostics.push(
                Diagnostic::error(format!(
                    "dependency `{name}` version mismatch: faber.toml has `{version}`, {LOCK_FILE} has `{}`",
                    locked.version
                ))
                .with_file(package_root.join(LOCK_FILE).display().to_string())
                .with_arg("issue", "dependency_version_mismatch")
                .with_arg("package", name.clone()),
            );
        }
        validate_locked_paths(locked, &mut diagnostics);
    }
    diagnostics
}

fn validate_locked_paths(locked: &LockedPackage, diagnostics: &mut Vec<Diagnostic>) {
    let interface_root = locked.interface_root_path();
    if !interface_root.is_dir() {
        diagnostics.push(
            Diagnostic::error(format!(
                "locked interface_root for `{}` is missing or not a directory: {}",
                locked.name,
                interface_root.display()
            ))
            .with_arg("issue", "locked_interface_root_missing")
            .with_arg("package", locked.name.clone()),
        );
    }
    let artifact = locked.artifact_path();
    if !artifact.is_file() {
        diagnostics.push(
            Diagnostic::error(format!(
                "locked artifact for `{}` is missing: {}",
                locked.name,
                artifact.display()
            ))
            .with_arg("issue", "locked_artifact_missing")
            .with_arg("package", locked.name.clone()),
        );
    }
    let target_manifest = locked.target_manifest_path();
    if !target_manifest.is_file() {
        diagnostics.push(
            Diagnostic::error(format!(
                "locked target_manifest for `{}` is missing: {}",
                locked.name,
                target_manifest.display()
            ))
            .with_arg("issue", "locked_target_manifest_missing")
            .with_arg("package", locked.name.clone()),
        );
    }
}
