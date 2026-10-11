use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use radix::module::codegen::Target;
use radix::module::diagnostics::{Diagnostic, DiagnosticPhase};
use radix::module::driver::Config;
use radix::module::hir::HirItemKind;

use super::binding_manifest::{BindingManifest, read_binding_manifest};
use super::{faber_analysis_seams, read_manifest, validate_manifest};
use radix::program::analyze::AnalyzedProgram;
use radix::program::discovery::resolve_source_root;
use radix::program::file_interface::extract_callable_contracts;
use radix::program::manifest::validate_target_tables;
use radix::program::member_path::resolve_package_member;
#[cfg(feature = "hir-rust")]
use radix::program::rust_target::render_binding_probe;

#[cfg(feature = "hir-rust")]
use super::binding_probe::run_rust_binding_probe;

#[derive(Debug, Clone, PartialEq)]
struct BindingContract {
    key: String,
    file: PathBuf,
    span: radix::module::lexer::Span,
    callable: radix::module::file_interface::InterfaceCallable,
    has_body: bool,
    def_id: radix::module::hir::DefId,
}

#[derive(Debug, Eq, PartialEq)]
pub struct BindingVerification {
    pub declarations: usize,
    pub bindings: usize,
    pub types: usize,
    pub shim: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum BindingProbeMode {
    #[cfg(feature = "hir-rust")]
    ShapeOnly,
    Cargo,
}

/// Verify the library bindings declared for `target`.
///
/// # Errors
///
/// Returns the manifest, member-path or binding-verification diagnostics.
pub fn verify_library_bindings(
    package_root: &Path,
    target: &str,
) -> Result<BindingVerification, Vec<Diagnostic>> {
    verify_library_bindings_with_probe_mode(package_root, target, BindingProbeMode::Cargo)
}

/// Read the route-to-spelling maps (functions and types) that the library at
/// `package_root` declares for `target`, without verifying them.
///
/// A library with no `[target.<target>]` table or no `bindings` file yields
/// empty maps. Keys and values are the manifest's opaque strings; the compiler
/// names no library vocabulary.
///
/// # Errors
/// Returns the manifest, member-path or binding-manifest diagnostics.
pub fn load_target_bindings(
    package_root: &Path,
    target: &str,
) -> Result<radix::module::driver::TargetBindings, Vec<Diagnostic>> {
    let manifest_path = package_root.join(super::MANIFEST_FILE);
    let manifest = match read_manifest(&manifest_path) {
        Ok(manifest) => manifest,
        Err(diag) => return Err(vec![*diag]),
    };
    if let Err(diag) = validate_target_tables(&manifest.target, &manifest_path) {
        return Err(vec![*diag]);
    }
    let Some(bindings_path) = manifest
        .target
        .get(target)
        .and_then(|config| config.bindings.as_deref())
    else {
        return Ok(radix::module::driver::TargetBindings::default());
    };
    let binding_path = match resolve_package_member(package_root, bindings_path, &manifest_path) {
        Ok(path) => path,
        Err(diagnostic) => return Err(vec![diagnostic]),
    };
    let binding_manifest = read_binding_manifest(&binding_path)?;
    Ok(radix::module::driver::TargetBindings {
        functions: binding_manifest
            .functions
            .into_iter()
            .map(|(route, binding)| (route, binding.symbol))
            .collect(),
        types: binding_manifest
            .types
            .into_iter()
            .map(|(route, binding)| (route, binding.symbol))
            .collect(),
    })
}

/// Verify the shape of the library bindings declared for `target`.
///
/// # Errors
///
/// Returns the manifest, target-binding or binding-shape diagnostics.
#[cfg(feature = "hir-rust")]
pub fn verify_library_binding_shapes(
    package_root: &Path,
    target: &str,
) -> Result<BindingVerification, Vec<Diagnostic>> {
    verify_library_bindings_with_probe_mode(package_root, target, BindingProbeMode::ShapeOnly)
}

fn verify_library_bindings_with_probe_mode(
    package_root: &Path,
    target: &str,
    probe_mode: BindingProbeMode,
) -> Result<BindingVerification, Vec<Diagnostic>> {
    let manifest_path = package_root.join(super::MANIFEST_FILE);
    let manifest = match read_manifest(&manifest_path) {
        Ok(manifest) => manifest,
        Err(diag) => return Err(vec![*diag]),
    };
    if let Err(diag) = validate_manifest(&manifest, &manifest_path) {
        return Err(vec![*diag]);
    }
    if let Err(diag) = validate_target_tables(&manifest.target, &manifest_path) {
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
    // FBR-P1-001: the supported source-root set is `src` (default) and `.`
    // (package root). Reuse discovery's resolution so bindings follow the same
    // contract; `resolve_package_member` rejects a bare `.` as naming nothing.
    let source_root =
        match resolve_source_root(package_root, &manifest.paths.source, &manifest_path) {
            Ok(path) => path,
            Err(diagnostic) => return Err(vec![*diagnostic]),
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
        .map_or(manifest.package.name.as_str(), |library| {
            library.provider.as_str()
        });
    let (package, declarations) = collect_binding_contracts(provider, package_root)?;
    let declared_types = collect_declared_types(provider, &package);
    validate_bindings(
        &binding_path,
        &binding_manifest,
        &declarations,
        &declared_types,
    )?;
    let shim = validate_shim(package_root, &binding_path, &binding_manifest)?;
    // The Rust ABI probe proves Rust symbols; a WGSL manifest binds opaque
    // spellings, so shape validation above is the whole check for it.
    if !is_wgsl_target(target) {
        prove_rust_bindings(
            target,
            package_root,
            &binding_path,
            &target_config.dependencies,
            shim.as_deref(),
            &binding_manifest,
            &declarations,
            &package,
            probe_mode,
        )?;
    }

    Ok(BindingVerification {
        declarations: declarations.len(),
        bindings: binding_manifest.functions.len(),
        types: binding_manifest.types.len(),
        shim,
    })
}

fn is_wgsl_target(target: &str) -> bool {
    Target::from_spelling(target) == Some(Target::MirWgsl)
}

/// Route keys (`provider:module.Name`) of every exported struct, genus and
/// enum item in the library.
fn collect_declared_types(provider: &str, package: &AnalyzedProgram) -> BTreeSet<String> {
    let mut types = BTreeSet::new();
    for unit in package.nodes.values() {
        let module = unit.module_segments.join("/");
        for item in &unit.analysis.hir.items {
            let name = match &item.kind {
                HirItemKind::Struct(decl) => decl.name,
                HirItemKind::Enum(decl) => decl.name,
                _ => continue,
            };
            let name = unit.analysis.interner.resolve(name);
            if unit.export_names.iter().any(|export| export == name) {
                types.insert(format!("{provider}:{module}.{name}"));
            }
        }
    }
    types
}

fn collect_binding_contracts(
    provider: &str,
    package_root: &Path,
) -> Result<(AnalyzedProgram, Vec<BindingContract>), Vec<Diagnostic>> {
    let package = radix::program::analyze::analyze_package(
        &Config::default().with_bodyless_functions(),
        package_root,
        &faber_analysis_seams(),
    )?;
    let mut declarations = Vec::new();
    let mut keys = BTreeSet::new();
    let mut diagnostics = Vec::new();
    for unit in package.nodes.values() {
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
    declared_types: &BTreeSet<String>,
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

    for (key, binding) in &manifest.types {
        if binding.symbol.trim().is_empty() {
            diagnostics.push(diagnostic(
                path,
                format!("type binding `{key}` has an empty target spelling"),
                "binding_symbol_empty",
            ));
        }
        if !declared_types.contains(key) {
            diagnostics.push(diagnostic(
                path,
                format!("type binding `{key}` does not match an exported Faber type"),
                "binding_unknown_type",
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
#[cfg(feature = "hir-rust")]
fn prove_rust_bindings(
    target: &str,
    package_root: &Path,
    binding_path: &Path,
    dependencies: &BTreeMap<String, String>,
    shim: Option<&Path>,
    manifest: &BindingManifest,
    declarations: &[BindingContract],
    package: &AnalyzedProgram,
    probe_mode: BindingProbeMode,
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
        let Some((_, unit)) = package.node_by_path(&declaration.file) else {
            continue;
        };
        match render_binding_probe(
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
    if probe_mode == BindingProbeMode::ShapeOnly {
        return Ok(());
    }

    run_rust_binding_probe(package_root, binding_path, dependencies, shim, &probes).map_err(
        |diagnostic| {
            vec![
                diagnostic.with_arg(
                    "bindings",
                    manifest
                        .functions
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(","),
                ),
            ]
        },
    )
}

#[allow(clippy::too_many_arguments)]
#[cfg(not(feature = "hir-rust"))]
fn prove_rust_bindings(
    target: &str,
    _package_root: &Path,
    binding_path: &Path,
    _dependencies: &BTreeMap<String, String>,
    _shim: Option<&Path>,
    manifest: &BindingManifest,
    _declarations: &[BindingContract],
    _package: &AnalyzedProgram,
    _probe_mode: BindingProbeMode,
) -> Result<(), Vec<Diagnostic>> {
    if manifest.functions.is_empty() {
        return Ok(());
    }
    Err(vec![diagnostic(
        binding_path,
        format!(
            "binding contract probes for target `{target}` are not available in this faber build; rebuild with feature `hir-rust`"
        ),
        "binding_probe_target_unavailable",
    )])
}

fn diagnostic(path: &Path, message: impl Into<String>, issue: &'static str) -> Diagnostic {
    crate::package_diagnostic_error(message.into())
        .with_file(path.display().to_string())
        .with_arg("issue", issue)
}

#[cfg(test)]
#[path = "binding_test.rs"]
mod tests;
