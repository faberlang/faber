use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use radix::diagnostics::Diagnostic;
use serde::Deserialize;

use super::{read_manifest, validate_manifest};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingManifest {
    #[serde(default)]
    functions: BTreeMap<String, FunctionBinding>,

    #[serde(default)]
    shim: Option<ShimBinding>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FunctionBinding {
    symbol: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ShimBinding {
    path: String,
}

#[derive(Debug, Eq, PartialEq)]
struct SourceDeclaration {
    key: String,
    has_body: bool,
}

#[derive(Debug, Eq, PartialEq)]
pub struct BindingVerification {
    pub declarations: usize,
    pub bindings: usize,
    pub shim: Option<PathBuf>,
}

pub fn verify_library_bindings(
    package_root: &Path,
    target: &str,
) -> Result<BindingVerification, Vec<Diagnostic>> {
    let manifest_path = package_root.join(super::MANIFEST_FILE);
    let manifest = match read_manifest(&manifest_path) {
        Ok(manifest) => manifest,
        Err(diag) => return Err(vec![*diag]),
    };
    if let Err(diag) = validate_manifest(&manifest, &manifest_path) {
        return Err(vec![*diag]);
    }
    if manifest.build.kind != "lib" {
        return Err(vec![diagnostic(
            &manifest_path,
            "verify-library requires build.kind = \"lib\"",
            "binding_verify_requires_library",
        )]);
    }
    if !manifest.build.targets.iter().any(|item| item == target) {
        return Err(vec![diagnostic(
            &manifest_path,
            format!("library does not declare build target `{target}`"),
            "binding_target_not_declared",
        )]);
    }

    let Some(target_config) = manifest.target.get(target) else {
        return Err(vec![diagnostic(
            &manifest_path,
            format!("missing [target.{target}] binding configuration"),
            "binding_target_missing",
        )]);
    };
    let Some(bindings_path) = target_config.bindings.as_deref() else {
        return Err(vec![diagnostic(
            &manifest_path,
            format!("missing [target.{target}].bindings"),
            "binding_manifest_missing",
        )]);
    };

    let binding_path = package_root.join(bindings_path);
    let binding_manifest = read_binding_manifest(&binding_path)?;
    let source_root = package_root.join(&manifest.paths.source);
    let provider = manifest
        .library
        .as_ref()
        .map(|library| library.provider.as_str())
        .unwrap_or(&manifest.package.name);
    let declarations = collect_source_declarations(provider, &source_root)?;
    validate_bindings(&binding_path, &binding_manifest, &declarations)?;
    let shim = validate_shim(package_root, &binding_path, &binding_manifest)?;

    Ok(BindingVerification {
        declarations: declarations.len(),
        bindings: binding_manifest.functions.len(),
        shim,
    })
}

fn read_binding_manifest(path: &Path) -> Result<BindingManifest, Vec<Diagnostic>> {
    let source = fs::read_to_string(path).map_err(|err| vec![Diagnostic::io_error(path, err)])?;
    toml::from_str::<BindingManifest>(&source).map_err(|err| {
        vec![
            Diagnostic::error(format!("invalid binding manifest: {err}"))
                .with_file(path.display().to_string())
                .with_arg("issue", "invalid_binding_manifest"),
        ]
    })
}

fn collect_source_declarations(
    provider: &str,
    source_root: &Path,
) -> Result<Vec<SourceDeclaration>, Vec<Diagnostic>> {
    let mut declarations = Vec::new();
    collect_source_declarations_in(provider, source_root, source_root, &mut declarations)?;
    Ok(declarations)
}

fn collect_source_declarations_in(
    provider: &str,
    source_root: &Path,
    dir: &Path,
    declarations: &mut Vec<SourceDeclaration>,
) -> Result<(), Vec<Diagnostic>> {
    let entries = fs::read_dir(dir).map_err(|err| vec![Diagnostic::io_error(dir, err)])?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            vec![
                Diagnostic::error(format!("failed to read source directory entry: {err}"))
                    .with_file(dir.display().to_string())
                    .with_arg("issue", "binding_source_read_failed"),
            ]
        })?;
        let path = entry.path();
        if path.is_dir() {
            collect_source_declarations_in(provider, source_root, &path, declarations)?;
        } else if path.extension().is_some_and(|ext| ext == "fab") {
            collect_file_declarations(provider, source_root, &path, declarations)?;
        }
    }
    Ok(())
}

fn collect_file_declarations(
    provider: &str,
    source_root: &Path,
    path: &Path,
    declarations: &mut Vec<SourceDeclaration>,
) -> Result<(), Vec<Diagnostic>> {
    let source = fs::read_to_string(path).map_err(|err| vec![Diagnostic::io_error(path, err)])?;
    let module = path
        .strip_prefix(source_root)
        .unwrap_or(path)
        .with_extension("")
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    for line in source.lines() {
        let trimmed = line.trim_start();
        let Some(rest) = trimmed.strip_prefix("functio ") else {
            continue;
        };
        let Some((name, _)) = rest.split_once('(') else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() {
            continue;
        }
        declarations.push(SourceDeclaration {
            key: format!("{provider}:{module}.{name}"),
            has_body: trimmed.contains('{'),
        });
    }
    Ok(())
}

fn validate_bindings(
    path: &Path,
    manifest: &BindingManifest,
    declarations: &[SourceDeclaration],
) -> Result<(), Vec<Diagnostic>> {
    let declaration_keys = declarations
        .iter()
        .map(|decl| decl.key.as_str())
        .collect::<BTreeSet<_>>();
    let binding_keys = manifest
        .functions
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();

    for (key, binding) in &manifest.functions {
        if binding.symbol.trim().is_empty() {
            return Err(vec![diagnostic(
                path,
                format!("binding `{key}` has an empty Rust symbol"),
                "binding_symbol_empty",
            )]);
        }
        if !declaration_keys.contains(key.as_str()) {
            return Err(vec![diagnostic(
                path,
                format!("binding `{key}` does not match a Faber declaration"),
                "binding_unknown_declaration",
            )]);
        }
    }

    for declaration in declarations {
        if !declaration.has_body && !binding_keys.contains(declaration.key.as_str()) {
            return Err(vec![diagnostic(
                path,
                format!(
                    "declaration `{}` has no Faber body and no binding",
                    declaration.key
                ),
                "binding_required_missing",
            )]);
        }
    }

    Ok(())
}

fn validate_shim(
    package_root: &Path,
    binding_path: &Path,
    manifest: &BindingManifest,
) -> Result<Option<PathBuf>, Vec<Diagnostic>> {
    let Some(shim) = &manifest.shim else {
        return Ok(None);
    };
    if shim.path.trim().is_empty() {
        return Err(vec![diagnostic(
            binding_path,
            "binding shim.path must not be empty",
            "binding_shim_path_empty",
        )]);
    }
    let path = package_root.join(&shim.path);
    if !path.is_file() {
        return Err(vec![diagnostic(
            binding_path,
            format!("binding shim source is missing: {}", path.display()),
            "binding_shim_missing",
        )]);
    }
    Ok(Some(path))
}

fn diagnostic(path: &Path, message: impl Into<String>, issue: &'static str) -> Diagnostic {
    Diagnostic::error(message.into())
        .with_file(path.display().to_string())
        .with_arg("issue", issue)
}
