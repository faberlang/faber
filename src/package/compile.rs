use super::frontmatter::RustTestSelection;
#[cfg(feature = "hir-go")]
use radix::cli::CliProgram;
use radix::codegen::Target;
use radix::diagnostics::Diagnostic;
use radix::driver::{
    analyze_source_with_cli_program_and_import_contract, AnalyzedUnit, Config, Session,
};
use radix::hir::visit::{walk_expr, HirVisitor};
use radix::hir::{HirExpressionKind, HirItemKind};
use radix::lexer::Interner;
use radix::syntax::{ImportDecl, ImportKind, StmtKind};
use radix::CompileResult;
#[cfg(feature = "hir-go")]
use radix::GoOutput;
#[cfg(feature = "hir-go")]
use radix::Output;
#[cfg(feature = "hir-go")]
use std::collections::HashMap;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::frontmatter::manifest_path_for_spec;
use super::import_graph::{
    build_mount_plan, library_import_binding, resolve_import, ImportResolution,
};
use super::{
    analysis_source_for_file, discover_build_layout, discover_package, library_cached_analysis,
    library_cached_file_interface, library_interface_export_names, library_interface_has_module,
    library_resolver_for_package, load_locale_pack_for_input, load_package_with_locale_pack,
    load_provider_manifests, program_export_names, read_manifest, selected_providers_for_routes,
    LibraryImportBinding, LibraryInterfaceCache, PackageFile, RustRuntimePlan,
};

pub(crate) struct AnalyzedPackage {
    pub(crate) spec: super::PackageSpec,
    pub(crate) units: Vec<AnalyzedPackageUnit>,
    #[allow(dead_code)]
    // retained for FHIR/frontmatter metadata even when optional target leaves are disabled
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

/// Result of a package compile that explicitly carries the Go multi-module
/// file collection produced by the Go assembly path (G6 GO3/GO4).
///
/// The module files are an output of the Go compile path — never hidden
/// thread-local state (FBR-P2-003) — so failed, repeated, nested, and
/// concurrent compile calls cannot exchange module output.
pub(crate) struct PackageCompileResult {
    pub(crate) compile_result: CompileResult,
    /// Go multi-module files `(file name, file body)` for non-entry units,
    /// populated only for `Target::HirGo` packages that reached codegen.
    #[cfg(feature = "hir-go")]
    pub(crate) go_modules: Vec<(String, String)>,
}

/// Failed compile: no output, no Go modules.
fn compile_failure(diagnostics: Vec<Diagnostic>) -> PackageCompileResult {
    PackageCompileResult {
        compile_result: CompileResult {
            output: None,
            diagnostics,
        },
        #[cfg(feature = "hir-go")]
        go_modules: Vec::new(),
    }
}

fn rust_runtime_plan_for_package(
    package: &AnalyzedPackage,
    library_resolver: &crate::library::LibraryResolver,
) -> Result<RustRuntimePlan, Box<Diagnostic>> {
    let manifest = super::manifest::manifest_for_spec(&package.spec)?;
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
    // Collect ad routes from package units **and** expanded library import
    // bodies (norma is the primary product path — package-unit Ad alone is a
    // false-negative for host-only devices).
    let mut library_cache = LibraryInterfaceCache::default();
    for unit in &package.units {
        let mut collector = AdRouteCollector {
            interner: &unit.analysis.interner,
            routes: &mut plan.non_runtime_routes,
        };
        collector.visit_program(&unit.analysis.hir);
        for import in &unit.expanded_library_imports {
            if super::library::is_builtin_norma_http_module(&import.module) {
                continue;
            }
            let Ok(analysis) =
                library_cached_analysis(import, library_resolver, &mut library_cache)
            else {
                continue;
            };
            let mut collector = AdRouteCollector {
                interner: &analysis.interner,
                routes: &mut plan.non_runtime_routes,
            };
            collector.visit_program(&analysis.hir);
        }
    }
    // Dual-backend host gate:
    // - host=native → select providers for all non-runtime routes (host path)
    // - host unset → auto-select only host-only routes (builtin-covered are free)
    // Explicit `[dispatch].providers` always participate.
    let host_required = super::dispatch::host_required_routes(&plan.non_runtime_routes);
    if matches!(plan.host, Some(super::ManifestRustHost::Native)) {
        plan.selected_providers =
            selected_providers_for_routes(&plan.non_runtime_routes, &explicit_providers);
        // Validate host-provider coverage only for host-required routes so
        // builtin-only dual-backend routes (e.g. tempus:expectet) do not fail
        // closed against incomplete host manifests.
        match load_provider_manifests(&plan.selected_providers, &host_required) {
            Ok(manifests) => plan.provider_manifests = manifests,
            Err(error) => plan.provider_error = Some(error.message),
        }
    } else {
        plan.selected_providers =
            selected_providers_for_routes(&host_required, &explicit_providers);
    }
    Ok(plan)
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
    finalize_package_compile_result(
        compile_package_internal(config, input, None, false, None).compile_result,
        &config.warn_policy,
    )
}

/// Compile a Go-targeted package, returning the entry output and the explicit
/// multi-module sibling file collection in one result (no hidden thread-local
/// side channel).
///
/// The `config` must target `Target::HirGo`; other targets leave
/// [`PackageCompileResult::go_modules`] empty.
#[cfg(feature = "hir-go")]
pub(crate) fn compile_package_go(config: &Config, input: &Path) -> PackageCompileResult {
    let result = compile_package_internal(config, input, None, false, None);
    PackageCompileResult {
        compile_result: finalize_package_compile_result(result.compile_result, &config.warn_policy),
        go_modules: result.go_modules,
    }
}

/// Compile a package while forwarding a Rust test-selection policy to codegen.
///
/// Historical Rust harness path: still used by package integration tests that
/// assert on generated Rust. Product `faber test` uses the MIR stepper instead.
#[allow(dead_code)] // public API + lib tests; binary test command no longer calls this
pub fn compile_package_with_test_selection(
    config: &Config,
    input: &Path,
    test_selection: Option<&RustTestSelection>,
) -> CompileResult {
    compile_package_with_test_options(config, input, test_selection, None)
}

/// Like [`compile_package_with_test_selection`], with optional path filters for
/// which `*.proba` files are loaded (`--include` / `--exclude` on `faber test`).
#[allow(dead_code)] // public API + lib tests; binary test command no longer calls this
pub fn compile_package_with_test_options(
    config: &Config,
    input: &Path,
    test_selection: Option<&RustTestSelection>,
    proba_filter: Option<&super::TestSourceFilter>,
) -> CompileResult {
    finalize_package_compile_result(
        compile_package_internal(config, input, test_selection, true, proba_filter).compile_result,
        &config.warn_policy,
    )
}

fn finalize_package_compile_result(
    mut result: CompileResult,
    warn_policy: &radix::driver::WarnPolicy,
) -> CompileResult {
    radix::apply_warn_policy(&mut result.diagnostics, warn_policy);
    if result.diagnostics.iter().any(Diagnostic::is_error) {
        result.output = None;
    }
    result
}

pub(crate) fn package_rust_runtime_plan(
    config: &Config,
    input: &Path,
) -> Result<RustRuntimePlan, Vec<Diagnostic>> {
    let config = effective_package_config(config, input)?;
    let spec = discover_package(input).map_err(|diag| vec![*diag])?;
    let package_root = package_root_for_input(input);
    let library_resolver = library_resolver_for_package(&config, &package_root)?;
    let package = analyze_package_spec(&config, spec, &library_resolver, false, None)?;
    rust_runtime_plan_for_package(&package, &library_resolver).map_err(|diag| vec![*diag])
}

pub(crate) fn analyze_package(
    config: &Config,
    input: &Path,
) -> Result<AnalyzedPackage, Vec<Diagnostic>> {
    let config = effective_package_config(config, input)?;
    let spec = discover_package(input).map_err(|diag| vec![*diag])?;
    let package_root = package_root_for_input(input);
    let library_resolver = library_resolver_for_package(&config, &package_root)?;
    analyze_package_spec(&config, spec, &library_resolver, false, None)
}

/// Analyze a package for `faber test`: includes `*.proba` and optional path filters.
///
/// Production [`analyze_package`] keeps `include_proba = false` so build/run graphs
/// stay free of test sources.
///
/// Used by the binary `faber test` command and lib tests; the lib crate root does
/// not call it outside `cfg(test)`.
#[allow(dead_code)] // binary `commands/test` + cfg(test); shared package sources
pub(crate) fn analyze_package_for_tests(
    config: &Config,
    input: &Path,
    proba_filter: Option<&super::TestSourceFilter>,
) -> Result<AnalyzedPackage, Vec<Diagnostic>> {
    let config = effective_package_config(config, input)?;
    let spec = discover_package(input).map_err(|diag| vec![*diag])?;
    let package_root = package_root_for_input(input);
    let library_resolver = library_resolver_for_package(&config, &package_root)?;
    analyze_package_spec(&config, spec, &library_resolver, true, proba_filter)
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
    include_proba: bool,
    proba_filter: Option<&super::TestSourceFilter>,
) -> PackageCompileResult {
    // G4: analyze once before target rejection so Go/TS planners and diagnostics
    // share the same package graph (no reloading source per target).
    let config = match effective_package_config(config, input) {
        Ok(config) => config,
        Err(diagnostics) => {
            return compile_failure(diagnostics);
        }
    };

    let spec = match discover_package(input) {
        Ok(spec) => spec,
        Err(diag) => {
            return compile_failure(vec![*diag]);
        }
    };

    let package_root = package_root_for_input(input);
    let library_resolver = match library_resolver_for_package(&config, &package_root) {
        Ok(resolver) => resolver,
        Err(diagnostics) => {
            return compile_failure(diagnostics);
        }
    };
    let package = match analyze_package_spec(
        &config,
        spec,
        &library_resolver,
        include_proba,
        proba_filter,
    ) {
        Ok(package) => package,
        Err(diagnostics) => {
            return compile_failure(diagnostics);
        }
    };

    if config.target == Target::HirGo {
        #[cfg(not(feature = "hir-go"))]
        return compile_failure(vec![crate::package_diagnostic_error(
            "target `go` is not available in this faber build; rebuild with feature `hir-go`",
        )
        .with_file(input.display().to_string())
        .with_arg("issue", "package_target_unavailable")
        .with_arg("target", "go")]);

        #[cfg(feature = "hir-go")]
        {
            let plan = super::artifact_plan::plan_package(&package, Target::HirGo);
            if !plan.supported {
                return compile_failure(vec![crate::package_diagnostic_error(
                    plan.rejection.unwrap_or_else(|| {
                        "package compilation does not support this target".to_owned()
                    }),
                )
                .with_file(input.display().to_string())
                .with_arg("issue", "package_target_unsupported")
                .with_arg("target", plan.target)]);
            }
            return generate_package_go_result(&package, input);
        }
    }

    if config.target != Target::HirRust {
        let plan = super::artifact_plan::plan_package(&package, config.target);
        if !plan.supported {
            return compile_failure(vec![crate::package_diagnostic_error(
                plan.rejection.unwrap_or_else(|| {
                    "package compilation does not support this target".to_owned()
                }),
            )
            .with_file(input.display().to_string())
            .with_arg("issue", "package_target_unsupported")
            .with_arg("target", plan.target)]);
        }
        // Planner seams exist; full product emit for TS is later deliveries.
        return compile_failure(vec![crate::package_diagnostic_error(format!(
            "package compilation has a {} artifact plan but full product assembly is not implemented yet",
            plan.target
        ))
        .with_file(input.display().to_string())
        .with_arg("issue", "package_target_assembly_pending")
        .with_arg("target", plan.target)]);
    }

    #[cfg(not(feature = "hir-rust"))]
    {
        let _ = test_selection;
        return compile_failure(vec![crate::package_diagnostic_error(
            "target `rust` is not available in this faber build; rebuild with feature `hir-rust`",
        )
        .with_file(input.display().to_string())
        .with_arg("issue", "package_target_unavailable")
        .with_arg("target", "rust")]);
    }

    #[cfg(feature = "hir-rust")]
    {
        return super::rust_target::compile_package_rust(
            package,
            &library_resolver,
            test_selection,
        );
    }
}

/// G6 GO3/GO4 — emit package Go for entry (+ sibling modules as same-package files).
///
/// Local Faber imports become same-package namespace vars (`binding.Field`) that
/// point at package-level functions from sibling units. Norma/stdlib imports
/// remain elided by Go codegen.
#[cfg(feature = "hir-go")]
fn generate_package_go_result(package: &AnalyzedPackage, input: &Path) -> PackageCompileResult {
    let mut diagnostics = package.diagnostics.clone();
    let Some(entry) = package.entry_unit() else {
        return compile_failure({
            diagnostics.push(
                crate::package_diagnostic_error(
                    "package has no entry unit for Go assembly".to_owned(),
                )
                .with_file(input.display().to_string())
                .with_arg("issue", "package_go_entry_missing"),
            );
            diagnostics
        });
    };

    // Generate non-entry modules first (signatures feed namespace vars).
    let mut module_files: Vec<(String, String)> = Vec::new();
    let mut unit_funcs: std::collections::BTreeMap<PathBuf, Vec<super::go_build::GoFuncSig>> =
        std::collections::BTreeMap::new();

    // Go is not a localized target; the surface is unused but required by the
    // shared dispatch seam after reader-locale emit threading.
    let go_surface_latin = radix::locale::latin_locale_pack();
    let go_surface = radix::locale::KeywordSurface::new(&go_surface_latin);
    for unit in &package.units {
        if unit.is_entry {
            continue;
        }
        match radix::codegen::generate_from_analyzed(Target::HirGo, &unit.analysis, &go_surface) {
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
                let file_code = super::go_build::wrap_module_file(&body, needs.imports());
                let file = super::go_build::module_go_file_name(&unit.module_segments, &unit.path);
                module_files.push((file, file_code));
            }
            Ok(_) => {
                diagnostics.push(
                    crate::package_diagnostic_error(
                        "Go module codegen returned a non-Go output".to_owned(),
                    )
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "package_go_codegen_failed"),
                );
                return compile_failure(diagnostics);
            }
            Err(err) => {
                let mut diag = crate::package_diagnostic_error(err.message)
                    .with_file(unit.path.display().to_string());
                for arg in err.args {
                    diag = diag.with_arg(arg.name, arg.value);
                }
                diagnostics.push(diag);
                return compile_failure(diagnostics);
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
                let mut diag = crate::package_diagnostic_error(err.message)
                    .with_file(entry.path.display().to_string());
                for arg in err.args {
                    diag = diag.with_arg(arg.name, arg.value);
                }
                diagnostics.push(diag);
                return compile_failure(diagnostics);
            }
        },
        None => {
            match radix::codegen::generate_from_analyzed(
                Target::HirGo,
                &entry.analysis,
                &go_surface,
            ) {
                Ok(Output::Go(output)) => output.code,
                Ok(_) => {
                    diagnostics.push(
                        crate::package_diagnostic_error(
                            "Go package codegen returned a non-Go output".to_owned(),
                        )
                        .with_file(entry.path.display().to_string())
                        .with_arg("issue", "package_go_codegen_failed"),
                    );
                    return compile_failure(diagnostics);
                }
                Err(err) => {
                    let mut diag = crate::package_diagnostic_error(err.message)
                        .with_file(entry.path.display().to_string());
                    for arg in err.args {
                        diag = diag.with_arg(arg.name, arg.value);
                    }
                    diagnostics.push(diag);
                    return compile_failure(diagnostics);
                }
            }
        }
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
        return compile_failure(diagnostics);
    }

    // FBR-P2-006: normalize each unit path once and index the module function
    // signatures; every local import then resolves in one map lookup instead
    // of an O(U) scan with repeated path normalization per import edge.
    let unit_funcs_by_path: HashMap<PathBuf, &[super::go_build::GoFuncSig]> = unit_funcs
        .iter()
        .map(|(path, funcs)| (normalize_path_buf(path), funcs.as_slice()))
        .collect();

    // Namespace vars for local imports + explicit Norma host shims (entry + siblings).
    // WHY (79df18a): inject each binding name at most once — multi-unit packages
    // that all `importa … privata consolum` must not redeclare `var consolum`.
    let mut namespace_block = String::new();
    let mut injected_bindings: std::collections::BTreeSet<String> =
        std::collections::BTreeSet::new();
    let mut needs_os_for_shim = false;
    for unit in &package.units {
        for item in &unit.analysis.hir.items {
            let HirItemKind::Import(import) = &item.kind else {
                continue;
            };
            let import_path = unit.analysis.interner.resolve(import.path);
            // Package Go assembly owns an explicit `norma:consolum` host shim.
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
                    namespace_block
                        .push_str(&super::go_build::render_norma_consolum_shim(&binding));
                    namespace_block.push('\n');
                    needs_os_for_shim = true;
                }
                continue;
            }
            if import_path.starts_with("norma:") || import_path.starts_with("norma/") {
                // Other Norma modules still fail closed at go build (no silent erase).
                continue;
            }
            let Some(target_path) =
                resolve_local_import_path(&package.spec, &unit.path, import_path)
            else {
                continue;
            };
            let Some(funcs) = unit_funcs_by_path.get(&target_path).copied() else {
                diagnostics.push(
                    crate::package_diagnostic_error(format!(
                        "Go multi-module assembly could not find unit for import `{import_path}`"
                    ))
                    .with_file(unit.path.display().to_string())
                    .with_arg("issue", "package_go_import_unit_missing")
                    .with_arg("target", "go"),
                );
                return compile_failure(diagnostics);
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
    if entry
        .analysis
        .cli_program
        .as_ref()
        .is_some_and(go_cli_accepts_dashed_rest_operands)
    {
        entry_code = allow_go_cli_dashed_rest_operands(&entry_code);
    }
    if needs_os_for_shim {
        entry_code = ensure_go_import(&entry_code, "bufio");
        entry_code = ensure_go_import(&entry_code, "os");
        entry_code = ensure_go_import(&entry_code, "fmt");
        entry_code = ensure_go_import(&entry_code, "io");
        entry_code = ensure_go_import(&entry_code, "strings");
    }

    // Multi-module files travel in the returned result (FBR-P2-003): the
    // emitter consumes them from the result, never from a hidden thread-local.
    PackageCompileResult {
        compile_result: CompileResult {
            output: Some(Output::Go(GoOutput { code: entry_code })),
            diagnostics,
        },
        go_modules: module_files,
    }
}

#[cfg(feature = "hir-go")]
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

#[cfg(feature = "hir-go")]
fn normalize_path_buf(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

#[cfg(feature = "hir-go")]
fn go_cli_accepts_dashed_rest_operands(program: &CliProgram) -> bool {
    if !(program.global_options.is_empty() && program.options.is_empty()) {
        return false;
    }
    program
        .global_operands
        .iter()
        .chain(program.operands.iter())
        .any(|operand| operand.rest)
}

#[cfg(feature = "hir-go")]
fn allow_go_cli_dashed_rest_operands(code: &str) -> String {
    code.replace(
        "if strings.HasPrefix(arg, \"-\") {",
        "if strings.HasPrefix(arg, \"-\") && false {",
    )
}

/// Detect package-level Go function name collisions across flattened modules.
///
/// WHY: multi-module assembly emits every unit into the same `package main`.
/// Two `functio identity` in different `.fab` files become two `func identity`
/// and only fail at `go build` without this gate (correctness 53ff0a7).
#[cfg(feature = "hir-go")]
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
                    crate::package_diagnostic_error(format!(
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
#[cfg(feature = "hir-go")]
fn ensure_go_import(code: &str, pkg: &str) -> String {
    if go_imports(code).iter().any(|existing| existing == pkg) {
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
        let line_end = code[idx..]
            .find('\n')
            .map(|n| idx + n)
            .unwrap_or(code.len());
        let existing = code[idx..line_end].trim();
        // existing like `import "fmt"`
        let existing_pkg = existing.strip_prefix("import ").unwrap_or(existing).trim();
        let mut out = String::new();
        out.push_str(&code[..idx]);
        out.push_str("import (\n\t");
        out.push_str(existing_pkg);
        out.push_str("\n\t\"");
        out.push_str(pkg);
        out.push_str("\"\n)\n");
        out.push_str(code[line_end..].trim_start_matches('\n'));
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
        out.push_str(code[after..].trim_start_matches('\n'));
        return out;
    }
    code.to_owned()
}

#[cfg(feature = "hir-go")]
fn go_imports(code: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut in_block = false;

    for line in code.lines() {
        let trimmed = line.trim();
        if in_block {
            if trimmed == ")" {
                in_block = false;
                continue;
            }
            if let Some(path) = go_import_path(trimmed) {
                imports.push(path.to_owned());
            }
            continue;
        }

        if trimmed == "import (" {
            in_block = true;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("import ") {
            if let Some(path) = go_import_path(rest.trim()) {
                imports.push(path.to_owned());
            }
        }
    }

    imports
}

#[cfg(feature = "hir-go")]
fn go_import_path(segment: &str) -> Option<&str> {
    let quoted = if let Some((_, rest)) = segment.split_once(' ') {
        rest.trim()
    } else {
        segment
    };
    quoted.strip_prefix('"')?.strip_suffix('"')
}

#[cfg(test)]
#[path = "compile_test.rs"]
mod tests;

fn effective_package_config(config: &Config, input: &Path) -> Result<Config, Vec<Diagnostic>> {
    if config.locale_pack.is_some() {
        return Ok(config.clone());
    }
    match load_locale_pack_for_input(input, None) {
        Ok(Some(pack)) => Ok(config.clone().with_locale_pack(pack)),
        Ok(None) => Ok(config.clone()),
        Err(diag) => Err(vec![*diag]),
    }
}

fn analyze_package_spec(
    config: &Config,
    spec: super::PackageSpec,
    library_resolver: &crate::library::LibraryResolver,
    include_proba: bool,
    proba_filter: Option<&super::TestSourceFilter>,
) -> Result<AnalyzedPackage, Vec<Diagnostic>> {
    let files = match config.locale_pack.as_ref() {
        Some(pack) => load_package_with_locale_pack(
            &spec,
            library_resolver,
            Some(pack),
            include_proba,
            proba_filter,
        )?,
        None => load_package_with_locale_pack(
            &spec,
            library_resolver,
            None,
            include_proba,
            proba_filter,
        )?,
    };
    let entry_frontmatter = files
        .iter()
        .find(|file| file.path == spec.entry)
        .and_then(|file| file.frontmatter.clone());
    // Install [paths.templates] into the driver config so SEM006 accepts §name/…
    // and validation matches package resolution.
    let mut config = config.clone();
    if !spec.templates.is_empty() {
        let mut templates = std::collections::BTreeMap::new();
        for (name, path) in &spec.templates {
            templates.insert(name.clone(), path.display().to_string());
        }
        config.import_path_templates = Some(templates);
    }
    let session = Session::new(config);
    let mount_plan = build_mount_plan(&spec, &files)?;
    let mut diagnostics = Vec::new();
    let mut library_cache = LibraryInterfaceCache::with_config(&session.config);
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
        let package_name = manifest_path_for_spec(&spec)
            .and_then(|path| read_manifest(&path).ok())
            .map(|manifest| manifest.package.name);
        let export_identity = super::file_interface::ExportIdentityContext {
            provider: "package".to_owned(),
            package: package_name,
            module_path: file.module_segments.clone(),
        };
        let file_interface = match super::file_interface::extract_file_interface_with_identity(
            &analysis,
            &export_names,
            &file.path.display().to_string(),
            Some(&export_identity),
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
            templates: std::collections::BTreeMap::new(),
            manifest_backed: true,
        },
        units: Vec::new(),
        entry_frontmatter: None,
        diagnostics: Vec::new(),
        linked_library_crates: std::collections::BTreeMap::new(),
    };
    super::artifact_plan::linked_library_crate_map(&shell)
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
                    return Err(crate::package_diagnostic_error(format!(
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
