//! Runtime dependency selection for generated Rust package artifacts.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use radix::diagnostics::Diagnostic;

#[allow(clippy::result_large_err)]
pub(crate) fn runtime_path_for_target_dependencies(
    package_root: &Path,
    dependencies: &BTreeMap<String, String>,
) -> Result<PathBuf, Diagnostic> {
    if let Some(path) = runtime_path_from_target_dependencies(package_root, dependencies) {
        return Ok(path);
    }
    crate::core_support::materialize::materialize()
        .and_then(|support| support.faber_runtime())
        .map_err(|error| {
            crate::package_diagnostic_error(format!(
                "verified core support is unavailable: {error}"
            ))
            .with_arg("issue", "core_support_materialization_failed")
        })
}

pub(crate) fn runtime_path_from_crate_roots<'a>(
    crate_roots: impl IntoIterator<Item = &'a Path>,
) -> Option<PathBuf> {
    crate_roots
        .into_iter()
        .find_map(runtime_path_from_cargo_manifest)
}

fn runtime_path_from_target_dependencies(
    package_root: &Path,
    dependencies: &BTreeMap<String, String>,
) -> Option<PathBuf> {
    for (name, requirement) in dependencies {
        let value = parse_dependency_requirement(requirement);
        let Some(table) = value.as_table() else {
            continue;
        };
        if is_runtime_dependency(name, table) {
            if let Some(path) = dependency_path(package_root, table) {
                return Some(path);
            }
        }
        if let Some(path) = dependency_path(package_root, table) {
            let path = fs::canonicalize(&path).unwrap_or(path);
            if let Some(runtime_path) = runtime_path_from_cargo_manifest(&path) {
                return Some(runtime_path);
            }
        }
    }
    None
}

fn runtime_path_from_cargo_manifest(crate_root: &Path) -> Option<PathBuf> {
    let manifest_path = crate_root.join("Cargo.toml");
    let source = fs::read_to_string(&manifest_path).ok()?;
    let manifest = toml::from_str::<toml::Value>(&source).ok()?;
    let dependencies = manifest.get("dependencies")?.as_table()?;
    for (name, value) in dependencies {
        let Some(table) = value.as_table() else {
            continue;
        };
        if is_runtime_dependency(name, table) {
            return dependency_path(crate_root, table);
        }
    }
    None
}

pub(crate) fn parse_dependency_requirement(requirement: &str) -> toml::Value {
    let trimmed = requirement.trim();
    if trimmed.starts_with('{') {
        trimmed
            .parse::<toml::Value>()
            .unwrap_or_else(|_| toml::Value::String(requirement.to_owned()))
    } else {
        toml::Value::String(requirement.to_owned())
    }
}

pub(crate) fn normalize_dependency_value(package_root: &Path, value: toml::Value) -> toml::Value {
    match value {
        toml::Value::Table(mut table) => {
            if let Some(toml::Value::String(path)) = table.get_mut("path") {
                *path = package_root.join(&*path).display().to_string();
            }
            toml::Value::Table(table)
        }
        other => other,
    }
}

fn dependency_path(base: &Path, table: &toml::map::Map<String, toml::Value>) -> Option<PathBuf> {
    table.get("path").and_then(toml::Value::as_str).map(|path| {
        let path = base.join(path);
        fs::canonicalize(&path).unwrap_or(path)
    })
}

fn is_runtime_dependency(name: &str, table: &toml::map::Map<String, toml::Value>) -> bool {
    name == "faber" || table.get("package").and_then(toml::Value::as_str) == Some("faber-runtime")
}

#[cfg(test)]
#[path = "runtime_dependency_test.rs"]
mod tests;
