//! Shared runtime dependency discovery for generated package artifacts.

use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn runtime_path_from_crate_roots<'a>(
    crate_roots: impl IntoIterator<Item = &'a Path>,
) -> Option<PathBuf> {
    crate_roots
        .into_iter()
        .find_map(runtime_path_from_cargo_manifest)
}

pub(super) fn runtime_path_from_cargo_manifest(crate_root: &Path) -> Option<PathBuf> {
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

pub(super) fn dependency_path(
    base: &Path,
    table: &toml::map::Map<String, toml::Value>,
) -> Option<PathBuf> {
    table.get("path").and_then(toml::Value::as_str).map(|path| {
        let path = base.join(path);
        fs::canonicalize(&path).unwrap_or(path)
    })
}

fn is_runtime_dependency(name: &str, table: &toml::map::Map<String, toml::Value>) -> bool {
    name == "faber" || table.get("package").and_then(toml::Value::as_str) == Some("faber-runtime")
}
