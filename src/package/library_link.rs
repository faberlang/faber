//! Emit generated Rust library crates for native-binding Faber packages (G4 P3).
//!
//! Application builds link these crates through Cargo path dependencies instead of
//! inlining bodyless panic stubs or scanning emitted Rust text for deps.
//!
//! TARGET: Unblock SQLite-style native libraries for application packages.
//! WHY: Phase 4 verifies bindings in isolation; this module links them into apps.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use radix::codegen::Target;
use radix::diagnostics::Diagnostic;
use radix::driver::Config;
use radix::hir::HirItemKind;

use super::artifact_plan::native_library_deps;
use super::binding::verify_library_bindings;
use super::codegen::ModuleNode;
use super::compile::{generate_library_unit_rust, AnalyzedPackageUnit};
use super::discovery::sanitize_crate_name;
use super::member_path::resolve_package_member;
use super::runtime_dependency::{
    parse_dependency_requirement, runtime_path_for_target_dependencies,
};
use super::{analyze_package, BuildLayout};

/// One generated library crate ready for a path dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LinkedLibraryCrate {
    pub provider: String,
    pub crate_name: String,
    pub crate_root: PathBuf,
}

/// Emit native-binding library crates under the app's `target/faber/deps/`.
pub(crate) fn emit_linked_library_crates(
    app_root: &Path,
    layout: &BuildLayout,
) -> Result<Vec<LinkedLibraryCrate>, Vec<Diagnostic>> {
    let deps = native_library_deps(app_root)?;
    if deps.is_empty() {
        return Ok(Vec::new());
    }

    let mut linked = Vec::new();
    let mut diagnostics = Vec::new();
    for (provider, locked, lib_manifest) in deps {
        if let Err(verify_diags) = verify_library_bindings(Path::new(&locked.package_root), "rust")
        {
            diagnostics.extend(verify_diags);
            continue;
        }

        let crate_name = sanitize_crate_name(&provider);
        let crate_root = layout.generated_crate_root.join("deps").join(&crate_name);
        match emit_one_library_crate(
            app_root,
            Path::new(&locked.package_root),
            &lib_manifest,
            &crate_name,
            &crate_root,
        ) {
            Ok(()) => linked.push(LinkedLibraryCrate {
                provider,
                crate_name,
                crate_root,
            }),
            Err(mut diags) => diagnostics.append(&mut diags),
        }
    }

    if diagnostics.is_empty() {
        Ok(linked)
    } else {
        Err(diagnostics)
    }
}

fn emit_one_library_crate(
    _app_root: &Path,
    package_root: &Path,
    lib_manifest: &super::FaberManifest,
    crate_name: &str,
    crate_root: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let config = Config::default().with_bodyless_functions();
    let mut package = analyze_package(&config, package_root)?;

    let target = lib_manifest.target.get("rust").ok_or_else(|| {
        vec![
            crate::package_diagnostic_error("library missing [target.rust]")
                .with_file(
                    package_root
                        .join(super::MANIFEST_FILE)
                        .display()
                        .to_string(),
                )
                .with_arg("issue", "library_target_rust_missing"),
        ]
    })?;
    let bindings_rel = target.bindings.as_deref().ok_or_else(|| {
        vec![
            crate::package_diagnostic_error("library missing [target.rust].bindings")
                .with_file(
                    package_root
                        .join(super::MANIFEST_FILE)
                        .display()
                        .to_string(),
                )
                .with_arg("issue", "library_bindings_missing"),
        ]
    })?;
    let binding_path = resolve_package_member(
        package_root,
        bindings_rel,
        &package_root.join(super::MANIFEST_FILE),
    )
    .map_err(|d| vec![d])?;
    let binding_manifest = read_binding_manifest(&binding_path)?;
    #[allow(clippy::result_large_err)]
    let shim_path = binding_manifest
        .shim
        .as_ref()
        .map(|shim| resolve_package_member(package_root, &shim.path, &binding_path))
        .transpose()
        .map_err(|d| vec![d])?;

    let mut module_tree = ModuleNode::default();
    let mut diagnostics = Vec::new();
    for unit in &mut package.units {
        match generate_linked_unit_rust(unit, &binding_manifest.functions) {
            Ok(code) => {
                if unit.module_segments.is_empty() {
                    module_tree.insert(&[], code);
                } else {
                    module_tree.insert(&unit.module_segments, code);
                }
            }
            Err(diag) => diagnostics.push(diag),
        }
    }
    if diagnostics.iter().any(Diagnostic::is_error) {
        return Err(diagnostics);
    }

    let src_dir = crate_root.join("src");
    fs::create_dir_all(&src_dir).map_err(|err| vec![Diagnostic::io_error(&src_dir, err)])?;

    let mut lib_rs = String::from(
        "// Generated by faber build — library package crate. Do not edit by hand.\n\n",
    );
    if let Some(shim) = &shim_path {
        let abs = fs::canonicalize(shim).unwrap_or_else(|_| shim.clone());
        lib_rs.push_str(&format!(
            "#[path = {:?}]\nmod shim;\n\n",
            abs.display().to_string()
        ));
    }
    lib_rs.push_str(&module_tree.render(0));
    let lib_rs = format_rust(&lib_rs);
    let lib_path = src_dir.join("lib.rs");
    fs::write(&lib_path, lib_rs).map_err(|err| vec![Diagnostic::io_error(&lib_path, err)])?;

    let cargo_toml = render_library_cargo_toml(
        crate_name,
        &lib_manifest.package.version,
        package_root,
        target,
    )
    .map_err(|error| vec![error])?;
    let cargo_path = crate_root.join("Cargo.toml");
    fs::write(&cargo_path, cargo_toml)
        .map_err(|err| vec![Diagnostic::io_error(&cargo_path, err)])?;

    Ok(())
}

#[allow(clippy::result_large_err)]
fn generate_linked_unit_rust(
    unit: &mut AnalyzedPackageUnit,
    bindings: &BTreeMap<String, FunctionBinding>,
) -> Result<String, Diagnostic> {
    let module = unit.module_segments.join("/");
    let mut binding_wrappers = String::new();
    let mut wrapper_def_ids = Vec::new();

    for item in &unit.analysis.hir.items {
        let HirItemKind::Function(func) = &item.kind else {
            continue;
        };
        if func.body.is_some() {
            continue;
        }
        let name = unit.analysis.interner.resolve(func.name);
        let Some(binding) = binding_for_function(bindings, &module, name) else {
            continue;
        };
        let probe = radix::codegen::rust::render_binding_probe(
            &unit.analysis,
            item.def_id,
            &binding.symbol,
            name,
        )
        .map_err(|err| {
            Diagnostic::codegen_error(&err.message)
                .with_file(unit.path.display().to_string())
                .with_args(err.args)
                .with_arg("issue", "library_binding_wrapper_failed")
        })?;
        // Public API for application path-dep consumers. Binding probes emit
        // either `fn name` or `async fn name`; promote the generated wrapper
        // without rewriting its body or attributes.
        let pub_probe = promote_binding_function_visibility(&probe);
        binding_wrappers.push_str(&pub_probe);
        if !binding_wrappers.ends_with('\n') {
            binding_wrappers.push('\n');
        }
        wrapper_def_ids.push(item.def_id);
    }

    unit.analysis
        .hir
        .items
        .retain(|item| !wrapper_def_ids.contains(&item.def_id));

    let mut code = String::new();
    if !unit.analysis.hir.items.is_empty() {
        let generated = generate_library_unit_rust(unit).map_err(|err| {
            Diagnostic::codegen_error(&err.message)
                .with_file(unit.path.display().to_string())
                .with_args(err.args)
        })?;
        // Module-mode codegen uses `pub(crate)` for non-entry units. Linked
        // library crates are path-deps for consumer packages, so promote
        // package surface items to `pub` (G4/API2 pure-Faber + mixed libraries).
        code.push_str(&promote_library_surface_visibility(&generated));
    }
    code.push_str(&binding_wrappers);
    if code.trim().is_empty() {
        code.push_str("// empty library unit\n");
    }
    Ok(code)
}

fn binding_for_function<'a>(
    bindings: &'a BTreeMap<String, FunctionBinding>,
    module: &str,
    name: &str,
) -> Option<&'a FunctionBinding> {
    let suffix = format!(":{module}.{name}");
    bindings
        .iter()
        .find(|(key, _)| key.ends_with(&suffix))
        .map(|(_, binding)| binding)
}

fn promote_binding_function_visibility(source: &str) -> String {
    let mut promoted = source
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let prefix_len = line.len() - trimmed.len();
            let Some(rest) = trimmed
                .strip_prefix("async fn ")
                .or_else(|| trimmed.strip_prefix("fn "))
            else {
                return line.to_owned();
            };
            let mut promoted = String::with_capacity(line.len() + 4);
            promoted.push_str(&line[..prefix_len]);
            promoted.push_str("pub ");
            if trimmed.starts_with("async fn ") {
                promoted.push_str("async fn ");
            } else {
                promoted.push_str("fn ");
            }
            promoted.push_str(rest);
            promoted
        })
        .collect::<Vec<_>>()
        .join("\n");
    if source.ends_with('\n') {
        promoted.push('\n');
    }
    promoted
}

#[allow(clippy::result_large_err)]
fn render_library_cargo_toml(
    crate_name: &str,
    version: &str,
    package_root: &Path,
    target: &super::manifest::ManifestTarget,
) -> Result<String, Diagnostic> {
    let version = if version.trim().is_empty() {
        "0.1.0"
    } else {
        version.trim()
    };
    let runtime_path = runtime_path_for_target_dependencies(package_root, &target.dependencies)?;
    let mut deps = format!(
        "faber = {{ package = {}, path = {} }}\n",
        super::cargo::toml_string("faber-runtime"),
        super::cargo::toml_path(&runtime_path),
    );
    for (name, req) in &target.dependencies {
        if name == "faber" {
            continue;
        }
        let rendered = render_dependency_requirement(package_root, req);
        deps.push_str(&format!("{} = {rendered}\n", super::cargo::toml_key(name)));
    }
    // No [workspace] table: this crate is a path dependency under the application
    // generated workspace root (`target/faber/`). Nested empty workspaces make Cargo
    // reject the tree as multiple workspace roots.
    Ok(format!(
        r#"[package]
name = {crate_name}
version = {version}
edition = "2021"

# Generated by faber build from a Faber library package.
# Do not edit by hand.

[lib]
path = "src/lib.rs"

[dependencies]
{deps}"#,
        crate_name = super::cargo::toml_string(crate_name),
        version = super::cargo::toml_string(version),
    ))
}

fn render_dependency_requirement(package_root: &Path, requirement: &str) -> String {
    if requirement.trim_start().starts_with('{') {
        absolutize_inline_dependency_paths(package_root, requirement)
    } else {
        super::cargo::toml_string(requirement)
    }
}

fn absolutize_inline_dependency_paths(package_root: &Path, requirement: &str) -> String {
    let toml::Value::Table(mut table) = parse_dependency_requirement(requirement) else {
        return super::cargo::toml_string(requirement);
    };
    if let Some(toml::Value::String(path)) = table.get_mut("path") {
        *path = package_root.join(&*path).display().to_string();
    }
    toml::Value::Table(table).to_string()
}

/// Promote module-mode `pub(crate)` items to `pub` for path-dep consumers.
fn promote_library_surface_visibility(source: &str) -> String {
    source
        .lines()
        .map(promote_library_surface_visibility_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn promote_library_surface_visibility_line(line: &str) -> String {
    let trimmed = line.trim_start();
    let prefix_len = line.len() - trimmed.len();
    let Some(rest) = trimmed.strip_prefix("pub(crate) ") else {
        return line.to_owned();
    };

    const ITEM_PREFIXES: &[&str] = &[
        "fn ",
        "async fn ",
        "struct ",
        "enum ",
        "type ",
        "const ",
        "static ",
        "trait ",
        "mod ",
    ];

    if ITEM_PREFIXES.iter().any(|prefix| rest.starts_with(prefix)) {
        let mut out = String::with_capacity(line.len());
        out.push_str(&line[..prefix_len]);
        out.push_str("pub ");
        out.push_str(rest);
        out
    } else {
        line.to_owned()
    }
}

fn format_rust(source: &str) -> String {
    radix::tool::format_generated_code(Target::Rust, source).unwrap_or_else(|_| source.to_owned())
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingManifest {
    #[serde(default)]
    functions: BTreeMap<String, FunctionBinding>,
    #[serde(default)]
    shim: Option<ShimBinding>,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FunctionBinding {
    symbol: String,
}

#[derive(Debug, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ShimBinding {
    path: String,
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

#[cfg(test)]
#[path = "library_link_test.rs"]
mod tests;
