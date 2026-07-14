use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use radix::diagnostics::{Diagnostic, DiagnosticPhase};
use radix::driver::Config;
use serde::Deserialize;

use super::binding_probe::run_rust_binding_probe;
use super::file_interface::extract_callable_contracts;
use super::{analyze_package, read_manifest, resolve_package_member, validate_manifest};

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

#[derive(Debug, Clone, PartialEq)]
struct BindingContract {
    key: String,
    file: PathBuf,
    span: radix::lexer::Span,
    callable: radix::file_interface::InterfaceCallable,
    has_body: bool,
    unit_index: usize,
    def_id: radix::hir::DefId,
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

    let binding_path = match resolve_package_member(package_root, bindings_path, &manifest_path) {
        Ok(path) => path,
        Err(diagnostic) => return Err(vec![diagnostic]),
    };
    let binding_manifest = read_binding_manifest(&binding_path)?;
    let source_root =
        match resolve_package_member(package_root, &manifest.paths.source, &manifest_path) {
            Ok(path) => path,
            Err(diagnostic) => return Err(vec![diagnostic]),
        };
    if !source_root.is_dir() {
        return Err(vec![diagnostic(
            &manifest_path,
            format!(
                "package source directory is missing: {}",
                source_root.display()
            ),
            "binding_source_root_missing",
        )]);
    }
    let provider = manifest
        .library
        .as_ref()
        .map(|library| library.provider.as_str())
        .unwrap_or(&manifest.package.name);
    let (package, declarations) = collect_binding_contracts(provider, package_root)?;
    validate_bindings(&binding_path, &binding_manifest, &declarations)?;
    let shim = validate_shim(package_root, &binding_path, &binding_manifest)?;
    prove_rust_bindings(
        target,
        package_root,
        &binding_path,
        &target_config.dependencies,
        shim.as_deref(),
        &binding_manifest,
        &declarations,
        &package,
    )?;

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
            crate::package_diagnostic_error(format!("invalid binding manifest: {err}"))
                .with_file(path.display().to_string())
                .with_arg("issue", "invalid_binding_manifest"),
        ]
    })
}

fn collect_binding_contracts(
    provider: &str,
    package_root: &Path,
) -> Result<(super::AnalyzedPackage, Vec<BindingContract>), Vec<Diagnostic>> {
    let package = analyze_package(&Config::default().with_bodyless_functions(), package_root)?;
    let mut declarations = Vec::new();
    let mut keys = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for (unit_index, unit) in package.units.iter().enumerate() {
        let module = unit.module_segments.join("/");
        let contracts = match extract_callable_contracts(
            &unit.analysis,
            &unit.export_names,
            &unit.path.display().to_string(),
        ) {
            Ok(contracts) => contracts,
            Err(diagnostic) => {
                diagnostics.push(diagnostic);
                continue;
            }
        };
        for contract in contracts {
            let key = format!("{provider}:{module}.{}", contract.name);
            if !keys.insert(key.clone()) {
                diagnostics.push(
                    crate::package_diagnostic_error(format!(
                        "duplicate Faber binding contract `{key}`"
                    ))
                    .with_phase(DiagnosticPhase::Analysis)
                    .with_file(unit.path.display().to_string())
                    .with_span(contract.span)
                    .with_arg("issue", "binding_duplicate_contract")
                    .with_arg("binding", key),
                );
                continue;
            }
            declarations.push(BindingContract {
                key,
                file: unit.path.clone(),
                span: contract.span,
                callable: contract.callable,
                has_body: contract.has_body,
                unit_index,
                def_id: contract.def_id,
            });
        }
    }
    if diagnostics.is_empty() {
        Ok((package, declarations))
    } else {
        Err(diagnostics)
    }
}

fn validate_bindings(
    path: &Path,
    manifest: &BindingManifest,
    declarations: &[BindingContract],
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

    let mut diagnostics = Vec::new();
    for (key, binding) in &manifest.functions {
        if binding.symbol.trim().is_empty() {
            diagnostics.push(diagnostic(
                path,
                format!("binding `{key}` has an empty Rust symbol"),
                "binding_symbol_empty",
            ));
        }
        if !declaration_keys.contains(key.as_str()) {
            diagnostics.push(diagnostic(
                path,
                format!("binding `{key}` does not match a Faber declaration"),
                "binding_unknown_declaration",
            ));
        }
    }

    for declaration in declarations {
        if !declaration.has_body && !binding_keys.contains(declaration.key.as_str()) {
            diagnostics.push(
                crate::package_diagnostic_error(format!(
                    "declaration `{}` has no Faber body and no binding",
                    declaration.key
                ))
                .with_phase(DiagnosticPhase::Analysis)
                .with_file(declaration.file.display().to_string())
                .with_span(declaration.span)
                .with_arg("issue", "binding_required_missing")
                .with_arg("binding", declaration.key.clone()),
            );
        }
    }

    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
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
    let path = resolve_package_member(package_root, &shim.path, binding_path)
        .map_err(|diagnostic| vec![diagnostic])?;
    if !path.is_file() {
        return Err(vec![diagnostic(
            binding_path,
            format!("binding shim source is missing: {}", path.display()),
            "binding_shim_missing",
        )]);
    }
    Ok(Some(path))
}

#[allow(clippy::too_many_arguments)]
fn prove_rust_bindings(
    target: &str,
    package_root: &Path,
    binding_path: &Path,
    dependencies: &BTreeMap<String, String>,
    shim: Option<&Path>,
    manifest: &BindingManifest,
    declarations: &[BindingContract],
    package: &super::AnalyzedPackage,
) -> Result<(), Vec<Diagnostic>> {
    if manifest.functions.is_empty() {
        return Ok(());
    }
    if target != "rust" {
        return Err(vec![diagnostic(
            binding_path,
            format!("binding contract probes are not supported for target `{target}`"),
            "binding_probe_target_unsupported",
        )]);
    }

    let by_key = declarations
        .iter()
        .map(|declaration| (declaration.key.as_str(), declaration))
        .collect::<BTreeMap<_, _>>();
    let mut probes = Vec::new();
    let mut diagnostics = Vec::new();
    for (index, (key, binding)) in manifest.functions.iter().enumerate() {
        let Some(declaration) = by_key.get(key.as_str()).copied() else {
            continue;
        };
        let unit = &package.units[declaration.unit_index];
        match radix::codegen::rust::render_binding_probe(
            &unit.analysis,
            declaration.def_id,
            &binding.symbol,
            &format!("__faber_binding_probe_{index}"),
        ) {
            Ok(probe) => probes.push(format!(
                "// binding: {key}\n// contract: {:?}\n{probe}",
                declaration.callable
            )),
            Err(error) => diagnostics.push(
                crate::package_diagnostic_error(format!(
                    "binding `{key}` cannot be represented as a Rust ABI probe: {}",
                    error.message
                ))
                .with_phase(DiagnosticPhase::Codegen)
                .with_file(declaration.file.display().to_string())
                .with_span(declaration.span)
                .with_args(error.args)
                .with_arg("issue", "binding_probe_render_failed")
                .with_arg("binding", key.clone()),
            ),
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    run_rust_binding_probe(package_root, binding_path, dependencies, shim, &probes).map_err(
        |diagnostic| {
            vec![diagnostic.with_arg(
                "bindings",
                manifest
                    .functions
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(","),
            )]
        },
    )
}

fn diagnostic(path: &Path, message: impl Into<String>, issue: &'static str) -> Diagnostic {
    crate::package_diagnostic_error(message.into())
        .with_file(path.display().to_string())
        .with_arg("issue", issue)
}

#[cfg(test)]
#[path = "binding_test.rs"]
mod tests;
