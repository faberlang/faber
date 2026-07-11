use radix::codegen::rust::{
    build_local_import_function_params, build_local_import_namespaces, local_import_module_key,
    ImportedFunctionParams, ImportedNamespaceInfo, RustFieldNamePolicy, SiblingModuleExports,
    TestSelection as RustTestSelection,
};
use radix::codegen::Target;
use radix::diagnostics::Diagnostic;
use radix::driver::{
    analyze_source_with_cli_program_and_import_contract, AnalyzedUnit, Config, Session,
};
use radix::hir::visit::{walk_expr, HirVisitor};
use radix::hir::{HirExpressionKind, HirItemKind};
use radix::lexer::Interner;
use radix::syntax::{ImportDecl, ImportKind, StmtKind};
use radix::{CompileResult, GoOutput, Output, RustOutput};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::codegen::{assemble_crate, ModuleNode};
use super::file_interface::extract_file_interface;
use super::frontmatter::{manifest_path_for_spec, merge_entry_test_selection};
use super::import_graph::{
    build_mount_plan, library_import_binding, resolve_import, ImportResolution,
};
use super::{
    analysis_source_for_file, discover_build_layout, discover_package, library_cached_analysis,
    library_cached_file_interface, library_generates_rust_module, library_imported_function_params,
    library_interface_export_names, library_interface_has_module, library_module_segments,
    library_resolver_for_package, load_package, load_package_with_reader_pack,
    load_provider_manifests, load_reader_pack_for_input, program_export_names, read_manifest,
    selected_providers_for_routes, LibraryImportBinding, LibraryInterfaceCache, PackageFile,
    RustRuntimePlan,
};

pub(crate) struct AnalyzedPackage {
    pub(crate) spec: super::PackageSpec,
    pub(crate) units: Vec<AnalyzedPackageUnit>,
    pub(crate) entry_frontmatter: Option<radix::driver::FileFrontmatter>,
    pub(crate) diagnostics: Vec<Diagnostic>,
    /// Provider → Cargo crate name for native-binding library path deps (G4).
    pub(crate) linked_library_crates: std::collections::BTreeMap<String, String>,
}

impl AnalyzedPackage {
    #[allow(dead_code)] // Stage 2 package MIR linking consumes the entry unit directly.
    pub(crate) fn entry_unit(&self) -> Option<&AnalyzedPackageUnit> {
        self.units.iter().find(|unit| unit.is_entry)
    }
}

pub(crate) struct AnalyzedPackageUnit {
    pub(crate) path: PathBuf,
    pub(crate) module_segments: Vec<String>,
    pub(crate) is_entry: bool,
    pub(crate) analysis: AnalyzedUnit,
    #[allow(dead_code)] // Stage 3 consumes extracted interfaces during import lookup/typecheck.
    pub(crate) file_interface: radix::file_interface::FileInterface,
    pub(crate) export_names: Vec<String>,
    #[allow(dead_code)] // Stage 2 uses namespace exports to link package MIR calls.
    pub(crate) namespace_exports: BTreeMap<String, Vec<String>>,
    pub(crate) expanded_library_imports: Vec<LibraryImportBinding>,
}

struct GeneratedPackageRust {
    entry_code: Option<String>,
    module_tree: ModuleNode,
    diagnostics: Vec<Diagnostic>,
}

fn rust_runtime_plan_for_package(package: &AnalyzedPackage) -> RustRuntimePlan {
    let manifest = manifest_path_for_spec(&package.spec).and_then(|path| read_manifest(&path).ok());
    let host = manifest
        .as_ref()
        .and_then(|manifest| manifest.target.get("rust").and_then(|target| target.host));
    let explicit_providers = manifest
        .as_ref()
        .map(|manifest| manifest.dispatch.providers.clone())
        .unwrap_or_default();
    let library_path_deps = package
        .linked_library_crates
        .values()
        .map(|crate_name| {
            let path = package
                .spec
                .package_root
                .join("target")
                .join("faber")
                .join("deps")
                .join(crate_name);
            (crate_name.clone(), path)
        })
        .collect();
    // Runtime deps come from HIR/type facts and the G4 artifact plan — never
    // from scanning emitted Rust text (`faber::` / `tokio::` contains).
    let needs_tokio = package.units.iter().any(|unit| {
        unit.analysis.hir.entry_is_async || unit.analysis.hir.items.iter().any(hir_item_is_async)
    });
    // Package Rust emission always plans `rust:runtime:faber` (see artifact_plan).
    let needs_faber = true;
    let mut plan = RustRuntimePlan {
        needs_faber,
        needs_tokio,
        host,
        non_runtime_routes: BTreeSet::new(),
        selected_providers: BTreeSet::new(),
        provider_manifests: Vec::new(),
        provider_error: None,
        library_path_deps,
    };
    for unit in &package.units {
        let mut collector = AdRouteCollector {
            interner: &unit.analysis.interner,
            routes: &mut plan.non_runtime_routes,
        };
        collector.visit_program(&unit.analysis.hir);
    }
    plan.selected_providers =
        selected_providers_for_routes(&plan.non_runtime_routes, &explicit_providers);
    if matches!(plan.host, Some(super::ManifestRustHost::Native)) {
        match load_provider_manifests(&plan.selected_providers, &plan.non_runtime_routes) {
            Ok(manifests) => plan.provider_manifests = manifests,
            Err(error) => plan.provider_error = Some(error.message),
        }
    }
    plan
}

fn hir_item_is_async(item: &radix::hir::HirItem) -> bool {
    matches!(&item.kind, HirItemKind::Function(function) if function.is_async)
}

struct AdRouteCollector<'a> {
    interner: &'a Interner,
    routes: &'a mut BTreeSet<String>,
}

impl HirVisitor for AdRouteCollector<'_> {
    fn visit_expr(&mut self, expr: &radix::hir::HirExpression) {
        if let HirExpressionKind::Ad { route, .. } = expr.kind {
            let route = self.interner.resolve(route).to_owned();
            if !route.starts_with("runtime:") {
                self.routes.insert(route);
            }
        }
        walk_expr(self, expr);
    }
}

/// Compile a package source graph into one backend output.
///
/// Package compilation currently targets Rust only because it must assemble
/// multiple modules and generated CLI surfaces into a single crate-shaped
/// backend result. Unsupported targets are reported as diagnostics instead of
/// falling back to single-file compilation.
pub fn compile_package(config: &Config, input: &Path) -> CompileResult {
    compile_package_internal(config, input, None)
}

/// Compile a package while forwarding a Rust test-selection policy to codegen.
///
/// This is used by the package test command path so module and entry code are
/// generated under the same test filtering contract.
pub fn compile_package_with_test_selection(
    config: &Config,
    input: &Path,
    test_selection: Option<&RustTestSelection>,
) -> CompileResult {
    compile_package_internal(config, input, test_selection)
}

#[allow(clippy::result_large_err)]
pub(crate) fn package_rust_runtime_plan(
    config: &Config,
    input: &Path,
) -> Result<RustRuntimePlan, Vec<Diagnostic>> {
    let config = effective_package_config(config, input)?;
    let spec = discover_package(input).map_err(|diag| vec![*diag])?;
    let package_root = package_root_for_input(input);
    let library_resolver = library_resolver_for_package(&config, &package_root)?;
    let package = analyze_package_spec(&config, spec, &library_resolver)?;
    Ok(rust_runtime_plan_for_package(&package))
}

#[allow(clippy::result_large_err)]
pub(crate) fn analyze_package(
    config: &Config,
    input: &Path,
) -> Result<AnalyzedPackage, Vec<Diagnostic>> {
    let config = effective_package_config(config, input)?;
    let spec = discover_package(input).map_err(|diag| vec![*diag])?;
    let package_root = package_root_for_input(input);
    let library_resolver = library_resolver_for_package(&config, &package_root)?;
    analyze_package_spec(&config, spec, &library_resolver)
}

fn package_root_for_input(input: &Path) -> PathBuf {
    match discover_build_layout(input) {
        Ok(layout) => layout.package_root,
        Err(_) => {
            if input.is_file() {
                input
                    .parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_path_buf()
            } else {
                input.to_path_buf()
            }
        }
    }
}

fn compile_package_internal(
    config: &Config,
    input: &Path,
    test_selection: Option<&RustTestSelection>,
) -> CompileResult {
    // G4: analyze once before target rejection so Go/TS planners and diagnostics
    // share the same package graph (no reloading source per target).
    let config = match effective_package_config(config, input) {
        Ok(config) => config,
        Err(diagnostics) => {
            return CompileResult {
                output: None,
                diagnostics,
            };
        }
    };

    let spec = match discover_package(input) {
        Ok(spec) => spec,
        Err(diag) => {
            return CompileResult {
                output: None,
                diagnostics: vec![*diag],
            }
        }
    };

    let package_root = package_root_for_input(input);
    let library_resolver = match library_resolver_for_package(&config, &package_root) {
        Ok(resolver) => resolver,
        Err(diagnostics) => {
            return CompileResult {
                output: None,
                diagnostics,
            };
        }
    };
    let field_name_policy = match package_field_name_policy(&spec) {
        Ok(policy) => policy,
        Err(diag) => {
            return CompileResult {
                output: None,
                diagnostics: vec![*diag],
            }
        }
    };
    let mut package = match analyze_package_spec(&config, spec, &library_resolver) {
        Ok(package) => package,
        Err(diagnostics) => {
            return CompileResult {
                output: None,
                diagnostics,
            }
        }
    };

    if config.target == Target::Go {
        let plan = super::artifact_plan::plan_package(&package, Target::Go);
        if !plan.supported {
            return CompileResult {
                output: None,
                diagnostics: vec![Diagnostic::error(plan.rejection.unwrap_or_else(|| {
                    "package compilation does not support this target".to_owned()
                }))
                .with_file(input.display().to_string())
                .with_arg("issue", "package_target_unsupported")
                .with_arg("target", plan.target)],
            };
        }
        return generate_package_go_result(&package, input);
    }

    if config.target != Target::Rust {
        let plan = super::artifact_plan::plan_package(&package, config.target);
        if !plan.supported {
            return CompileResult {
                output: None,
                diagnostics: vec![Diagnostic::error(plan.rejection.unwrap_or_else(|| {
                    "package compilation does not support this target".to_owned()
                }))
                .with_file(input.display().to_string())
                .with_arg("issue", "package_target_unsupported")
                .with_arg("target", plan.target)],
            };
        }
        // Planner seams exist; full product emit for TS is later deliveries.
        return CompileResult {
            output: None,
            diagnostics: vec![Diagnostic::error(format!(
                "package compilation has a {} artifact plan but full product assembly is not implemented yet",
                plan.target
            ))
            .with_file(input.display().to_string())
            .with_arg("issue", "package_target_assembly_pending")
            .with_arg("target", plan.target)],
        };
    }
    let effective_test_selection =
        merge_entry_test_selection(test_selection, package.entry_frontmatter.as_ref());

    let generated = generate_package_rust(
        &mut package,
        &library_resolver,
        effective_test_selection.as_ref(),
        field_name_policy,
    );
    let diagnostics = generated.diagnostics;

    if diagnostics.iter().any(|diag| diag.is_error()) {
        return CompileResult {
            output: None,
            diagnostics,
        };
    }

    let Some(entry_code) = generated.entry_code else {
        return CompileResult {
            output: None,
            diagnostics: vec![Diagnostic::error(
                "package compilation did not produce an entry module",
            )
            .with_file(package.spec.entry.display().to_string())],
        };
    };

    let crate_code = assemble_crate(&entry_code, &generated.module_tree.render(0));
    CompileResult {
        output: Some(Output::Rust(RustOutput { code: crate_code })),
        diagnostics,
    }
}

/// G6 GO3/GO4 — emit package Go for entry (+ sibling modules as same-package files).
///
/// Local Faber imports become same-package namespace vars (`binding.Field`) that
/// point at package-level functions from sibling units. Norma/stdlib imports
/// remain elided by Go codegen.
fn generate_package_go_result(package: &AnalyzedPackage, input: &Path) -> CompileResult {
    let mut diagnostics = package.diagnostics.clone();
    let Some(entry) = package.entry_unit() else {
        return CompileResult {
            output: None,
            diagnostics: {
                diagnostics.push(
                    Diagnostic::error("package has no entry unit for Go assembly".to_owned())
                        .with_file(input.display().to_string())
                        .with_arg("issue", "package_go_entry_missing"),
                );
                diagnostics
            },
        };
    };

    // Generate non-entry modules first (signatures feed namespace vars).
    let mut module_files: Vec<(String, String)> = Vec::new();
    let mut unit_funcs: std::collections::BTreeMap<PathBuf, Vec<super::go_build::GoFuncSig>> =
        std::collections::BTreeMap::new();

    for unit in &package.units {
        if unit.is_entry {
            continue;
        }
        match radix::codegen::generate_from_analyzed(Target::Go, &unit.analysis) {
            Ok(Output::Go(output)) => {
                let body = super::go_build::strip_go_preamble(&output.code);
                let funcs = super::go_build::parse_go_func_sigs(&body);
                unit_funcs.insert(unit.path.clone(), funcs);
                // Restore std imports from structured GoNeeds (modules lose preamble).
                let needs = radix::codegen::collect_go_needs(
                    &unit.analysis.hir,
                    &unit.analysis.types,
                    &unit.analysis.interner,
                );
                let file_code = super::go_build::wrap_module_file(&body, &needs.imports);
                let file = super::go_build::module_go_file_name(&unit.module_segments, &unit.path);
                module_files.push((file, file_code));
            }
            Ok(_) => {
                diagnostics.push(
                    Diagnostic::error("Go module codegen returned a non-Go output".to_owned())
                        .with_file(unit.path.display().to_string())
                        .with_arg("issue", "package_go_codegen_failed"),
                );
                return CompileResult {
                    output: None,
                    diagnostics,
                };
            }
            Err(err) => {
                let mut diag = Diagnostic::error(err.message).with_file(unit.path.display().to_string());
                for arg in err.args {
                    diag = diag.with_arg(arg.name, arg.value);
                }
                diagnostics.push(diag);
                return CompileResult {
                    output: None,
                    diagnostics,
                };
            }
        }
    }

    let entry_code = match entry.analysis.cli_program.as_ref() {
        Some(cli) => match radix::codegen::generate_go_cli(
            &entry.analysis.hir,
            &entry.analysis.types,
            &entry.analysis.interner,
            cli,
        ) {
            Ok(output) => output.code,
            Err(err) => {
                let mut diag = Diagnostic::error(err.message).with_file(entry.path.display().to_string());
                for arg in err.args {
                    diag = diag.with_arg(arg.name, arg.value);
                }
                diagnostics.push(diag);
                return CompileResult {
                    output: None,
                    diagnostics,
                };
            }
        },
        None => match radix::codegen::generate_from_analyzed(Target::Go, &entry.analysis) {
            Ok(Output::Go(output)) => output.code,
            Ok(_) => {
                diagnostics.push(
                    Diagnostic::error("Go package codegen returned a non-Go output".to_owned())
                        .with_file(entry.path.display().to_string())
                        .with_arg("issue", "package_go_codegen_failed"),
                );
                return CompileResult {
                    output: None,
                    diagnostics,
                };
            }
            Err(err) => {
                let mut diag = Diagnostic::error(err.message).with_file(entry.path.display().to_string());
                for arg in err.args {
                    diag = diag.with_arg(arg.name, arg.value);
                }
                diagnostics.push(diag);
                return CompileResult {
                    output: None,
                    diagnostics,
                };
            }
        },
    };

    // P1 (53ff0a7): flatten to package main fails open if two units emit the same
    // package-level func name — catch at Faber compile, not go build.
    if let Some(diag) = go_package_func_name_collision_diagnostic(
        entry.path.as_path(),
        &entry_code,
        &module_files,
        &unit_funcs,
    ) {
        diagnostics.push(diag);
        return CompileResult {
            output: None,
            diagnostics,
        };
    }

    // Namespace vars for local imports + narrow Norma host shims (entry + siblings).
    // WHY (79df18a): inject each binding name at most once — multi-unit packages
    // that all `importa … privata consolum` must not redeclare `var consolum`.
    let mut namespace_block = String::new();
    let mut injected_bindings: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut needs_os_for_shim = false;
    for unit in &package.units {
        for item in &unit.analysis.hir.items {
            let HirItemKind::Import(import) = &item.kind else {
                continue;
            };
            let import_path = unit.analysis.interner.resolve(import.path);
            // G6 residual: narrow norma:consolum Go host (echo dic/scribe/mone).
            if import_path == "norma:consolum" || import_path == "norma/consolum" {
                for it in &import.items {
                    let binding = it
                        .alias
                        .map(|a| unit.analysis.interner.resolve(a))
                        .unwrap_or_else(|| unit.analysis.interner.resolve(it.name))
                        .to_owned();
                    if !injected_bindings.insert(binding.clone()) {
                        continue;
                    }
                    namespace_block.push_str(&super::go_build::render_norma_consolum_shim(&binding));
                    namespace_block.push('\n');
                    needs_os_for_shim = true;
                }
                continue;
            }
            if import_path.starts_with("norma:") || import_path.starts_with("norma/") {
                // Other Norma modules still fail closed at go build (no silent erase).
                continue;
            }
            let Some(target_path) = resolve_local_import_path(&package.spec, &unit.path, import_path)
            else {
                continue;
            };
            let canon = normalize_path_buf(&target_path);
            let Some(funcs) = unit_funcs
                .iter()
                .find(|(p, _)| normalize_path_buf(p) == canon)
                .map(|(_, f)| f.as_slice())
            else {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "Go multi-module assembly could not find unit for import `{import_path}`"
                    ))
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "package_go_import_unit_missing")
                    .with_arg("target", "go"),
                );
                return CompileResult {
                    output: None,
                    diagnostics,
                };
            };
            for it in &import.items {
                let binding = it
                    .alias
                    .map(|a| unit.analysis.interner.resolve(a))
                    .unwrap_or_else(|| unit.analysis.interner.resolve(it.name))
                    .to_owned();
                if !injected_bindings.insert(binding.clone()) {
                    continue;
                }
                namespace_block.push_str(&super::go_build::render_namespace_var(&binding, funcs));
                namespace_block.push('\n');
            }
        }
    }

    let mut entry_code = super::go_build::inject_after_imports(&entry_code, &namespace_block);
    if needs_os_for_shim {
        entry_code = ensure_go_import(&entry_code, "os");
        entry_code = ensure_go_import(&entry_code, "fmt");
    }

    // Side-channel: multi-file emit for build/run (Output is entry main.go only).
    GO_PACKAGE_MODULES.with(|slot| {
        *slot.borrow_mut() = module_files;
    });

    CompileResult {
        output: Some(Output::Go(GoOutput { code: entry_code })),
        diagnostics,
    }
}

std::thread_local! {
    static GO_PACKAGE_MODULES: std::cell::RefCell<Vec<(String, String)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Take multi-module Go files produced by the last `compile_package` Go assembly.
pub(crate) fn take_go_package_modules() -> Vec<(String, String)> {
    GO_PACKAGE_MODULES.with(|slot| std::mem::take(&mut *slot.borrow_mut()))
}

fn resolve_local_import_path(
    spec: &super::PackageSpec,
    from_file: &Path,
    import_path: &str,
) -> Option<PathBuf> {
    let dummy = crate::library::LibraryResolver::default();
    match resolve_import(spec, &dummy, from_file, import_path) {
        ImportResolution::Local(path) => Some(normalize_path_buf(&path)),
        _ => {
            let base = from_file.parent()?;
            let candidate = base.join(import_path);
            let with_fab = if candidate.extension().is_some() {
                candidate
            } else {
                candidate.with_extension("fab")
            };
            if with_fab.exists() {
                Some(normalize_path_buf(&with_fab))
            } else {
                None
            }
        }
    }
}

fn normalize_path_buf(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Detect package-level Go function name collisions across flattened modules.
///
/// WHY: multi-module assembly emits every unit into the same `package main`.
/// Two `functio identity` in different `.fab` files become two `func identity`
/// and only fail at `go build` without this gate (correctness 53ff0a7).
fn go_package_func_name_collision_diagnostic(
    entry_path: &Path,
    entry_code: &str,
    _module_files: &[(String, String)],
    unit_funcs: &std::collections::BTreeMap<PathBuf, Vec<super::go_build::GoFuncSig>>,
) -> Option<Diagnostic> {
    // name → first owner description
    let mut owners: std::collections::BTreeMap<String, String> = std::collections::BTreeMap::new();

    for f in super::go_build::parse_go_func_sigs(entry_code) {
        owners.insert(f.name, format!("entry {}", entry_path.display()));
    }
    for (path, funcs) in unit_funcs {
        let owner = path.display().to_string();
        for f in funcs {
            if let Some(prior) = owners.get(&f.name) {
                return Some(
                    Diagnostic::error(format!(
                        "Go package assembly: function `{}` is declared in both {prior} and {owner}; \
                         flattened `package main` cannot host colliding names",
                        f.name
                    ))
                    .with_file(path.display().to_string())
                    .with_arg("issue", "package_go_func_name_collision")
                    .with_arg("function", f.name.clone())
                    .with_arg("prior", prior.clone())
                    .with_arg("target", "go"),
                );
            }
            owners.insert(f.name.clone(), owner.clone());
        }
    }
    None
}

/// Ensure a single-line or parenthesized Go import block includes `pkg`.
fn ensure_go_import(code: &str, pkg: &str) -> String {
    if code.contains(&format!("\"{pkg}\"")) {
        return code.to_owned();
    }
    // import (\n ... )
    if let Some(idx) = code.find("import (") {
        let insert_at = idx + "import (".len();
        let mut out = String::with_capacity(code.len() + pkg.len() + 8);
        out.push_str(&code[..insert_at]);
        out.push_str(&format!("\n\t\"{pkg}\""));
        out.push_str(&code[insert_at..]);
        return out;
    }
    // import "fmt"\n → import (\n  "fmt"\n  "os"\n)
    if let Some(idx) = code.find("import \"") {
        let line_end = code[idx..].find('\n').map(|n| idx + n).unwrap_or(code.len());
        let existing = code[idx..line_end].trim();
        // existing like `import "fmt"`
        let existing_pkg = existing
            .strip_prefix("import ")
            .unwrap_or(existing)
            .trim();
        let mut out = String::new();
        out.push_str(&code[..idx]);
        out.push_str("import (\n\t");
        out.push_str(existing_pkg);
        out.push_str("\n\t\"");
        out.push_str(pkg);
        out.push_str("\"\n)\n");
        out.push_str(&code[line_end..].trim_start_matches('\n'));
        if !out.ends_with('\n') && code.ends_with('\n') {
            out.push('\n');
        }
        return out;
    }
    // No import block: insert after package main
    if let Some(idx) = code.find("package main") {
        let after = idx + "package main".len();
        let mut out = String::new();
        out.push_str(&code[..after]);
        out.push_str(&format!("\n\nimport \"{pkg}\"\n"));
        out.push_str(&code[after..].trim_start_matches('\n'));
        return out;
    }
    code.to_owned()
}

fn generate_package_rust(
    package: &mut AnalyzedPackage,
    library_resolver: &crate::library::LibraryResolver,
    test_selection: Option<&RustTestSelection>,
    field_name_policy: RustFieldNamePolicy,
) -> GeneratedPackageRust {
    let mut entry_code = None;
    let mut module_tree = ModuleNode::default();
    let mut diagnostics = std::mem::take(&mut package.diagnostics);
    let mut library_cache = LibraryInterfaceCache::default();
    let native_host_bootstrap = manifest_path_for_spec(&package.spec)
        .and_then(|path| read_manifest(&path).ok())
        .and_then(|manifest| manifest.target.get("rust").and_then(|target| target.host))
        .is_some_and(|host| matches!(host, super::ManifestRustHost::Native));

    for index in 0..package.units.len() {
        let (before, rest) = package.units.split_at_mut(index);
        let Some((unit, after)) = rest.split_first_mut() else {
            continue;
        };
        let siblings = local_import_siblings_for_unit(
            unit,
            before.iter().chain(after.iter()),
            &package.spec,
            library_resolver,
        );
        let path = unit.path.display().to_string();
        // Only the package entry owns `main` and the generated host_register
        // module; library units stay free of the bootstrap seam.
        let unit_host_bootstrap = native_host_bootstrap && unit.is_entry;
        let rust = match generate_package_unit_rust(
            unit,
            &siblings,
            library_resolver,
            &mut library_cache,
            test_selection,
            field_name_policy,
            unit_host_bootstrap,
        ) {
            Ok(output) => output,
            Err(err) => {
                diagnostics.push(
                    Diagnostic::codegen_error(&err.message)
                        .with_file(path)
                        .with_args(err.args),
                );
                continue;
            }
        };

        if rust.contains("unresolved_def") {
            diagnostics.push(
                Diagnostic::error("project compilation produced unresolved Rust backend names")
                    .with_file(unit.path.display().to_string()),
            );
            continue;
        }

        if unit.is_entry {
            entry_code = Some(rust);
        } else {
            module_tree.insert(&unit.module_segments, rust);
        }
    }

    if let Err(diag) = insert_generated_library_modules(
        &package.units,
        library_resolver,
        &mut library_cache,
        test_selection,
        field_name_policy,
        &mut module_tree,
        &package.linked_library_crates,
    ) {
        diagnostics.push(diag);
    }

    GeneratedPackageRust {
        entry_code,
        module_tree,
        diagnostics,
    }
}

fn effective_package_config(config: &Config, input: &Path) -> Result<Config, Vec<Diagnostic>> {
    if config.reader_pack.is_some() {
        return Ok(config.clone());
    }
    match load_reader_pack_for_input(input, None) {
        Ok(Some(pack)) => Ok(config.clone().with_reader_pack(pack)),
        Ok(None) => Ok(config.clone()),
        Err(diag) => Err(vec![*diag]),
    }
}

fn generate_package_unit_rust(
    unit: &mut AnalyzedPackageUnit,
    siblings: &[SiblingModuleExports<'_>],
    library_resolver: &crate::library::LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    test_selection: Option<&RustTestSelection>,
    field_name_policy: RustFieldNamePolicy,
    native_host_bootstrap: bool,
) -> Result<String, radix::codegen::CodegenError> {
    let mut imported_function_params = build_local_import_function_params(
        &unit.analysis.hir,
        &unit.analysis.interner,
        &mut unit.analysis.types,
        siblings,
    );
    extend_library_function_params(
        &unit.expanded_library_imports,
        &mut unit.analysis.types,
        library_resolver,
        library_cache,
        &mut imported_function_params,
    )?;
    let mut imported_namespace_info = build_local_import_namespaces(
        &unit.analysis.hir,
        &unit.analysis.interner,
        &mut unit.analysis.types,
        &unit.analysis.resolver,
        siblings,
    );
    extend_library_namespace_type_paths(
        &unit.expanded_library_imports,
        &unit.analysis.hir,
        &unit.analysis.interner,
        &unit.analysis.resolver,
        &mut imported_namespace_info,
    );

    generate_rust_code_for_analysis(
        &unit.analysis,
        unit.is_entry,
        test_selection,
        field_name_policy,
        native_host_bootstrap,
        Some(imported_function_params),
        Some(imported_namespace_info),
    )
}

fn extend_library_namespace_type_paths(
    imports: &[LibraryImportBinding],
    hir: &radix::hir::HirProgram,
    interner: &Interner,
    resolver: &radix::semantic::Resolver,
    info: &mut ImportedNamespaceInfo<'_>,
) {
    for import in imports {
        let Some(binding) = import_binding_symbol(hir, interner, &import.binding) else {
            continue;
        };
        for export in resolver.imported_file_type_exports(binding) {
            let mut path = String::from("crate");
            for segment in library_module_segments(import) {
                path.push_str("::");
                path.push_str(&segment);
            }
            path.push_str("::");
            path.push_str(interner.resolve(export.member));
            info.type_paths.insert(export.def_id, path);
        }
    }
}

fn import_binding_symbol(
    hir: &radix::hir::HirProgram,
    interner: &Interner,
    binding_name: &str,
) -> Option<radix::lexer::Symbol> {
    hir.items.iter().find_map(|item| {
        let HirItemKind::Import(import) = &item.kind else {
            return None;
        };
        import.items.iter().find_map(|import_item| {
            let binding = import_item.alias.unwrap_or(import_item.name);
            (interner.resolve(binding) == binding_name).then_some(binding)
        })
    })
}

fn extend_library_function_params<'entry>(
    imports: &[LibraryImportBinding],
    entry_types: &mut radix::semantic::TypeTable,
    library_resolver: &crate::library::LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    params: &mut ImportedFunctionParams<'entry>,
) -> Result<(), radix::codegen::CodegenError> {
    for import in imports {
        let library_params =
            library_imported_function_params(import, entry_types, library_resolver, library_cache)
                .map_err(|diag| radix::codegen::CodegenError {
                    message: diag.message,
                    args: diag.args,
                })?;
        params.extend(library_params);
    }
    Ok(())
}

#[allow(clippy::result_large_err)]
fn insert_generated_library_modules(
    units: &[AnalyzedPackageUnit],
    library_resolver: &crate::library::LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    test_selection: Option<&RustTestSelection>,
    field_name_policy: RustFieldNamePolicy,
    module_tree: &mut ModuleNode,
    linked_library_crates: &std::collections::BTreeMap<String, String>,
) -> Result<(), Diagnostic> {
    let mut seen = BTreeSet::new();
    for import in units
        .iter()
        .flat_map(|unit| unit.expanded_library_imports.iter())
    {
        // Native-binding package deps are separate Cargo crates (G4), not inlined modules.
        if linked_library_crates.contains_key(&import.module.package) {
            continue;
        }
        let key = library_module_segments(import);
        if !seen.insert(key.clone()) {
            continue;
        }
        if !library_generates_rust_module(import, library_cache)? {
            continue;
        }
        let analysis = library_cached_analysis(import, library_resolver, library_cache)?;
        let rust = generate_rust_code_for_analysis(
            analysis,
            false,
            test_selection,
            field_name_policy,
            false,
            None,
            None,
        )
        .map_err(|err| {
            Diagnostic::codegen_error(&err.message)
                .with_file(import.module.interface_path.display().to_string())
                .with_args(err.args)
        })?;
        module_tree.insert(&key, rust);
    }
    Ok(())
}

fn analyze_package_spec(
    config: &Config,
    spec: super::PackageSpec,
    library_resolver: &crate::library::LibraryResolver,
) -> Result<AnalyzedPackage, Vec<Diagnostic>> {
    let files = match config.reader_pack.as_ref() {
        Some(pack) => load_package_with_reader_pack(&spec, library_resolver, Some(pack))?,
        None => load_package(&spec, library_resolver)?,
    };
    let entry_frontmatter = files
        .iter()
        .find(|file| file.path == spec.entry)
        .and_then(|file| file.frontmatter.clone());
    let session = Session::new(config.clone());
    let mount_plan = build_mount_plan(&spec, &files)?;
    let mut diagnostics = Vec::new();
    let mut library_cache = LibraryInterfaceCache::default();
    let mut units = Vec::new();
    let mut analyzed_interfaces_by_path = BTreeMap::new();
    // Linked crates are known from the package root + lock before unit analysis.
    let linked_library_crates = linked_crates_for_package_root(&spec.package_root);

    for file in package_analysis_order(&spec, &files, library_resolver) {
        let file_cli = mount_plan.module_cli.get(&file.path).cloned();
        let namespace_exports = match namespace_exports_for_file(
            &spec,
            file,
            &files,
            library_resolver,
            &mut library_cache,
        ) {
            Ok(exports) => exports,
            Err(diag) => {
                diagnostics.push(diag);
                continue;
            }
        };
        let file_interfaces = match file_interfaces_for_file(
            &spec,
            file,
            library_resolver,
            &mut library_cache,
            &analyzed_interfaces_by_path,
        ) {
            Ok(interfaces) => interfaces,
            Err(diag) => {
                diagnostics.push(diag);
                continue;
            }
        };
        let analysis_source =
            match analysis_source_for_file(file, library_resolver, &mut library_cache) {
                Ok(source) => source,
                Err(diag) => {
                    diagnostics.push(diag);
                    continue;
                }
            };
        let mut analysis = match analyze_source_with_cli_program_and_import_contract(
            &session,
            &file.path.display().to_string(),
            &analysis_source,
            file_cli,
            namespace_exports.clone(),
            file_interfaces,
        ) {
            Ok(analysis) => analysis,
            Err(file_diagnostics) => {
                diagnostics.extend(file_diagnostics);
                continue;
            }
        };
        let provenance_imports = file.library_imports.clone();
        if let Err(diag) = super::library::attach_library_provenance_with_links(
            &mut analysis,
            &provenance_imports,
            library_resolver,
            &mut library_cache,
            Some(&linked_library_crates),
        ) {
            diagnostics.push(diag);
            continue;
        }

        let is_entry = file.path == spec.entry;
        if !is_entry {
            analysis.hir.entry = None;
        }
        if is_entry {
            if let Some(root_cli) = mount_plan.root_cli.clone() {
                analysis.cli_program = Some(root_cli);
            }
        }

        let export_names = program_export_names(&file.program, &file.interner);
        let file_interface = match extract_file_interface(
            &analysis,
            &export_names,
            &file.path.display().to_string(),
        ) {
            Ok(interface) => interface,
            Err(diag) => {
                diagnostics.push(diag);
                continue;
            }
        };
        analyzed_interfaces_by_path.insert(file.path.clone(), file_interface.clone());

        diagnostics.extend(std::mem::take(&mut analysis.diagnostics));
        units.push(AnalyzedPackageUnit {
            path: file.path.clone(),
            module_segments: file.module_segments.clone(),
            is_entry,
            analysis,
            file_interface,
            export_names,
            namespace_exports,
            expanded_library_imports: file.expanded_library_imports.clone(),
        });
    }

    if diagnostics.iter().any(|diag| diag.is_error()) {
        return Err(diagnostics);
    }

    Ok(AnalyzedPackage {
        spec,
        units,
        entry_frontmatter,
        diagnostics,
        linked_library_crates,
    })
}

fn linked_crates_for_package_root(
    package_root: &Path,
) -> std::collections::BTreeMap<String, String> {
    // Build a lightweight shell package for the lock/manifest scan only.
    let shell = AnalyzedPackage {
        spec: super::PackageSpec {
            package_root: package_root.to_path_buf(),
            source_root: package_root.to_path_buf(),
            entry: package_root.to_path_buf(),
        },
        units: Vec::new(),
        entry_frontmatter: None,
        diagnostics: Vec::new(),
        linked_library_crates: std::collections::BTreeMap::new(),
    };
    super::artifact_plan::linked_library_crate_map(&shell)
}

/// Generate module-mode Rust for a library package unit (G4 library crates).
pub(crate) fn generate_library_unit_rust(
    unit: &AnalyzedPackageUnit,
) -> Result<String, radix::codegen::CodegenError> {
    generate_rust_code_for_analysis(
        &unit.analysis,
        false,
        None,
        RustFieldNamePolicy::Preserve,
        false,
        None,
        None,
    )
}

fn package_analysis_order<'a>(
    spec: &super::PackageSpec,
    files: &'a [PackageFile],
    library_resolver: &crate::library::LibraryResolver,
) -> Vec<&'a PackageFile> {
    let files_by_path = files
        .iter()
        .map(|file| (file.path.clone(), file))
        .collect::<BTreeMap<_, _>>();
    let mut seen = BTreeSet::new();
    let mut ordered = Vec::new();
    for file in files {
        visit_package_analysis_file(
            spec,
            file,
            &files_by_path,
            library_resolver,
            &mut seen,
            &mut ordered,
        );
    }
    ordered
}

fn visit_package_analysis_file<'a>(
    spec: &super::PackageSpec,
    file: &'a PackageFile,
    files_by_path: &BTreeMap<PathBuf, &'a PackageFile>,
    library_resolver: &crate::library::LibraryResolver,
    seen: &mut BTreeSet<PathBuf>,
    ordered: &mut Vec<&'a PackageFile>,
) {
    if !seen.insert(file.path.clone()) {
        return;
    }
    for stmt in &file.program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        let import_path = file.interner.resolve(decl.path);
        let ImportResolution::Local(target) =
            resolve_import(spec, library_resolver, &file.path, import_path)
        else {
            continue;
        };
        if let Some(target_file) = files_by_path.get(&target).copied() {
            visit_package_analysis_file(
                spec,
                target_file,
                files_by_path,
                library_resolver,
                seen,
                ordered,
            );
        }
    }
    ordered.push(file);
}

fn local_import_siblings_for_unit<'a>(
    unit: &AnalyzedPackageUnit,
    candidates: impl Iterator<Item = &'a AnalyzedPackageUnit>,
    spec: &super::PackageSpec,
    library_resolver: &crate::library::LibraryResolver,
) -> Vec<SiblingModuleExports<'a>> {
    let candidates_by_path = candidates
        .map(|candidate| (candidate.path.clone(), candidate))
        .collect::<BTreeMap<_, _>>();
    let mut siblings = Vec::new();
    for item in &unit.analysis.hir.items {
        let radix::hir::HirItemKind::Import(import) = &item.kind else {
            continue;
        };
        let import_path = unit.analysis.interner.resolve(import.path);
        let ImportResolution::Local(target) =
            resolve_import(spec, library_resolver, &unit.path, import_path)
        else {
            continue;
        };
        let Some(sibling) = candidates_by_path.get(&target).copied() else {
            continue;
        };
        siblings.push(SiblingModuleExports {
            module_key: local_import_module_key(import_path),
            module_path: sibling.module_segments.clone(),
            hir: &sibling.analysis.hir,
            interner: &sibling.analysis.interner,
            types: &sibling.analysis.types,
            exports: sibling.export_names.clone(),
        });
    }
    siblings
}

fn generate_rust_code_for_analysis(
    analysis: &radix::driver::AnalyzedUnit,
    is_entry: bool,
    test_selection: Option<&RustTestSelection>,
    field_name_policy: RustFieldNamePolicy,
    native_host_bootstrap: bool,
    imported_function_params: Option<radix::codegen::rust::ImportedFunctionParams<'_>>,
    imported_namespace_info: Option<radix::codegen::rust::ImportedNamespaceInfo<'_>>,
) -> Result<String, radix::codegen::CodegenError> {
    let cli_program = analysis.cli_program.as_ref();
    let module_mode = !is_entry;
    if is_entry {
        if let Some(cli_program) = cli_program {
            let mut codegen =
                radix::codegen::rust::RustCodegen::new_with_library_registry_and_test_selection(
                    &analysis.hir,
                    &analysis.interner,
                    &analysis.libraries,
                    test_selection.cloned(),
                );
            if let Some(params) = imported_function_params {
                codegen.set_imported_function_params(params);
            }
            if let Some(info) = imported_namespace_info {
                codegen.set_imported_namespace_info(info);
            }
            codegen.set_gpu_builtins(&analysis.gpu_builtins);
            codegen.set_field_name_policy(field_name_policy);
            codegen.set_native_host_bootstrap(native_host_bootstrap);
            return codegen
                .generate_cli(&analysis.hir, &analysis.types, cli_program)
                .map(|output| output.code);
        }
    }

    radix::codegen::rust::generate_with_library_registry_test_selection_and_imports(
        radix::codegen::rust::ModuleGenerationRequest {
            hir: &analysis.hir,
            types: &analysis.types,
            interner: &analysis.interner,
            libraries: &analysis.libraries,
            test_selection: test_selection.cloned(),
            module_mode,
            cli_program: if module_mode { cli_program } else { None },
            imported_function_params,
            imported_namespace_info,
            gpu_builtins: &analysis.gpu_builtins,
            field_name_policy,
            native_host_bootstrap,
        },
    )
    .map(|output| output.code)
}

fn package_field_name_policy(
    spec: &super::PackageSpec,
) -> Result<RustFieldNamePolicy, Box<Diagnostic>> {
    let Some(path) = manifest_path_for_spec(spec) else {
        return Ok(RustFieldNamePolicy::Preserve);
    };
    let manifest = read_manifest(&path)?;
    Ok(manifest.build.rust_field_names.into())
}

#[allow(clippy::result_large_err)]
fn file_interfaces_for_file(
    spec: &super::PackageSpec,
    file: &PackageFile,
    library_resolver: &crate::library::LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    analyzed_interfaces_by_path: &BTreeMap<PathBuf, radix::file_interface::FileInterface>,
) -> Result<BTreeMap<String, radix::file_interface::FileInterface>, Diagnostic> {
    let mut interfaces = BTreeMap::new();
    for stmt in &file.program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        let Some(binding) = import_binding(&file.interner, decl) else {
            continue;
        };
        let import_path = file.interner.resolve(decl.path);
        match resolve_import(spec, library_resolver, &file.path, import_path) {
            ImportResolution::Local(target) => {
                let Some(interface) = analyzed_interfaces_by_path.get(&target).cloned() else {
                    return Err(Diagnostic::error(format!(
                        "local import `{import_path}` interface was not analyzed before importer"
                    ))
                    .with_file(file.path.display().to_string())
                    .with_span(decl.span));
                };
                interfaces.insert(binding, interface);
            }
            ImportResolution::Library(module) => {
                let Some(import) = library_import_binding(&file.interner, decl, module) else {
                    continue;
                };
                if library_interface_has_module(&import, library_cache)? {
                    continue;
                }
                interfaces.insert(
                    binding,
                    library_cached_file_interface(&import, library_resolver, library_cache)?,
                );
            }
            ImportResolution::Unsupported | ImportResolution::Error(_) => {}
        }
    }
    for import in &file.expanded_library_imports {
        if library_interface_has_module(import, library_cache)? {
            continue;
        }
        interfaces.insert(
            import.binding.clone(),
            library_cached_file_interface(import, library_resolver, library_cache)?,
        );
    }
    Ok(interfaces)
}

#[allow(clippy::result_large_err)]
fn namespace_exports_for_file(
    spec: &super::PackageSpec,
    file: &PackageFile,
    files: &[PackageFile],
    library_resolver: &crate::library::LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<BTreeMap<String, Vec<String>>, Diagnostic> {
    let mut exports = BTreeMap::new();
    for stmt in &file.program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        let Some(binding) = import_binding(&file.interner, decl) else {
            continue;
        };
        let import_path = file.interner.resolve(decl.path);
        match resolve_import(spec, library_resolver, &file.path, import_path) {
            ImportResolution::Local(target) => {
                let Some(target_file) = files.iter().find(|candidate| candidate.path == target)
                else {
                    continue;
                };
                exports.insert(
                    binding,
                    sorted_export_names(program_export_names(
                        &target_file.program,
                        &target_file.interner,
                    )),
                );
            }
            ImportResolution::Library(module) => {
                let Some(import) = library_import_binding(&file.interner, decl, module) else {
                    continue;
                };
                exports.insert(
                    binding,
                    sorted_export_names(library_interface_export_names(
                        &import,
                        library_resolver,
                        library_cache,
                    )?),
                );
            }
            ImportResolution::Unsupported | ImportResolution::Error(_) => {}
        }
    }
    for import in &file.expanded_library_imports {
        exports.insert(
            import.binding.clone(),
            sorted_export_names(library_interface_export_names(
                import,
                library_resolver,
                library_cache,
            )?),
        );
    }
    Ok(exports)
}

fn import_binding(interner: &Interner, decl: &ImportDecl) -> Option<String> {
    match &decl.kind {
        ImportKind::Named { name, alias, .. } => Some(
            interner
                .resolve(alias.as_ref().unwrap_or(name).name)
                .to_owned(),
        ),
        ImportKind::Wildcard { alias } => Some(interner.resolve(alias.name).to_owned()),
    }
}

fn sorted_export_names(names: Vec<String>) -> Vec<String> {
    names
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// Check every loaded package module and return diagnostics without codegen.
///
/// The checker mirrors package compilation discovery and CLI mount analysis so
/// `faber check` reports the same import, manifest, and mounted-command policy
/// errors that a package build would encounter.
pub fn check_package(config: &Config, input: &Path) -> Vec<Diagnostic> {
    match analyze_package(config, input) {
        Ok(package) => package.diagnostics,
        Err(diagnostics) => diagnostics,
    }
}
