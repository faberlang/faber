use super::frontmatter::RustTestSelection;
#[cfg(feature = "hir-go")]
use radix::cli::CliProgram;
use radix::codegen::Target;
use radix::diagnostics::Diagnostic;
use radix::driver::Config;
#[cfg(feature = "hir-go")]
use radix::hir::visit::{walk_expr, HirVisitor};
#[cfg(feature = "hir-go")]
use radix::hir::{HirExpressionKind, HirItemKind};
#[cfg(feature = "hir-go")]
use radix::lexer::Interner;
use radix::CompileResult;
#[cfg(feature = "hir-go")]
use radix::GoOutput;
#[cfg(any(feature = "hir-go", feature = "mir-wasm"))]
use radix::Output;
#[cfg(feature = "mir-wasm")]
use radix::WasmTextOutput;
#[cfg(feature = "hir-go")]
use std::collections::HashMap;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

// RP3: multi-unit analysis lives in `radix-package` (`analyze.rs`); the
// production analyzed-package contract is radix-package-owned. This forwarding
// keeps faber-internal consumers compiling until RP7 removes the residue.
pub use radix_package::analyze::{AnalyzedPackage, AnalyzedPackageUnit};
// RP4: the emission assembly (and its result envelope) moved to `radix-package`
// (`emit.rs`); faber keeps the Go/Wasm generation bodies (toolchain-bound) and
// forwards the result types so consumers keep their existing names.
pub use radix_package::emit::{PackageCompileResult, WasmPackageModuleFile};

#[cfg(feature = "hir-go")]
use super::import_graph::{resolve_import, ImportResolution};
use super::{
    library_cache_default, library_cached_analysis, load_provider_manifests,
    selected_providers_for_routes, RustRuntimePlan,
};

// RP4: faber-supplied emission seams. Go and Wasm package generation stay
// faber-side (their toolchain modules are product-owned and stay in faber);
// the shared analysis seams and the target-neutral dispatch live in
// `radix-package`.
pub(crate) fn faber_emission_seams() -> radix_package::emit::EmissionSeams {
    radix_package::emit::EmissionSeams {
        analysis: super::faber_analysis_seams(),
        #[cfg(feature = "hir-go")]
        emit_go: generate_package_go_result,
        #[cfg(feature = "mir-wasm")]
        emit_wasm: generate_package_wasm_result,
    }
}

pub(crate) fn rust_runtime_plan_for_package(
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
    let mut library_cache = library_cache_default();
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
///
/// RP4: the emission assembly lives in `radix-package`; this adapter supplies
/// the faber seams (Go/Wasm generation, analysis) and forwards. RP6: the
/// binary commands consume `radix_package::emit::compile_package` directly;
/// this forward remains for the public API + lib tests until RP7.
#[allow(dead_code)]
pub fn compile_package(config: &Config, input: &Path) -> CompileResult {
    radix_package::emit::compile_package(config, input, &faber_emission_seams())
}

/// Compile a Go-targeted package, returning the entry output and the explicit
/// multi-module sibling file collection in one result (no hidden thread-local
/// side channel).
///
/// The `config` must target `Target::HirGo`; other targets leave
/// [`PackageCompileResult::go_modules`] empty.
// `allow(dead_code)`: RP6 re-wired the binary go route to
// `radix_package::emit::compile_package_result` directly; lib tests keep this
// adapter until RP7.
#[allow(dead_code)]
#[cfg(feature = "hir-go")]
pub(crate) fn compile_package_go(config: &Config, input: &Path) -> PackageCompileResult {
    radix_package::emit::compile_package_result(config, input, &faber_emission_seams())
}

/// Compile a Wasm-targeted package, returning the entry module output and the
/// explicit per-unit module collection in one result (no hidden thread-local
/// side channel).
///
/// The `config` must target `Target::MirWasm`; other targets leave
/// [`PackageCompileResult::wasm_modules`] empty.
// `allow(dead_code)`: emit-path seam with no in-crate caller — the binary
// routes package wasm builds through `wasm::build_package_wasm` directly
// (cmd.rs), so this compile-result entry is only a future emit-path consumer.
#[allow(dead_code)]
#[cfg(feature = "mir-wasm")]
pub(crate) fn compile_package_wasm(config: &Config, input: &Path) -> PackageCompileResult {
    radix_package::emit::compile_package_result(config, input, &faber_emission_seams())
}

/// Compile a package while forwarding a Rust test-selection policy to codegen.
///
/// Historical Rust harness path: still used by package integration tests that
/// assert on generated Rust. Product `faber test` uses the MIR runner instead.
#[allow(dead_code)] // public API + lib tests; binary test command no longer calls this
pub fn compile_package_with_test_selection(
    config: &Config,
    input: &Path,
    test_selection: Option<&RustTestSelection>,
) -> CompileResult {
    radix_package::emit::compile_package_with_test_selection(
        config,
        input,
        test_selection,
        &faber_emission_seams(),
    )
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
    radix_package::emit::compile_package_with_test_options(
        config,
        input,
        test_selection,
        proba_filter,
        &faber_emission_seams(),
    )
}

/// Package runtime plan (RP4): the analysis spine runs in `radix-package`;
/// the product `RustRuntimePlan` construction stays faber-side and is handed
/// to the entry as the explicit plan builder.
// `allow(dead_code)`: RP6 re-wired the binary build route to
// `radix_package::emit::package_rust_runtime_plan` directly; lib tests keep
// this adapter until RP7.
#[allow(dead_code)]
pub(crate) fn package_rust_runtime_plan(
    config: &Config,
    input: &Path,
) -> Result<RustRuntimePlan, Vec<Diagnostic>> {
    radix_package::emit::package_rust_runtime_plan(
        config,
        input,
        &super::faber_analysis_seams(),
        rust_runtime_plan_for_package,
    )
}

/// G6 GO3/GO4 — emit package Go for entry (+ sibling modules as same-package files).
///
/// Local Faber imports become same-package namespace vars (`binding.Field`) that
/// point at package-level functions from sibling units. Norma/stdlib imports
/// remain elided by Go codegen.
///
/// RP4: stays faber-side as the Go emission seam (consumes the product
/// `go_build` module); the dispatcher lives in `radix-package`.
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
    // that all `importa … consolum` must not redeclare `var consolum`.
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
        #[cfg(feature = "mir-wasm")]
        wasm_modules: Vec::new(),
    }
}

/// U6-D — emit a package's Wasm through the reusable package-to-Wasm builder.
///
/// The entry unit's WAT is the compile result output (text-only emission);
/// every unit module (entry + siblings) travels in
/// [`PackageCompileResult::wasm_modules`] with its package-aware WAT
/// (canonical sibling exports applied).
///
/// RP4: stays faber-side as the Wasm emission seam (the package-to-Wasm
/// builder is faber-owned until RP5).
#[cfg(feature = "mir-wasm")]
fn generate_package_wasm_result(
    package: &mut AnalyzedPackage,
    _input: &Path,
) -> PackageCompileResult {
    let options = super::wasm::PackageWasmOptions::new(
        package
            .spec
            .package_root
            .join("target")
            .join("faber")
            .join("wasm"),
    );
    let build = match super::wasm::build_package_wasm_from_graph(package, &options) {
        Ok(build) => build,
        Err(diagnostics) => return compile_failure(diagnostics),
    };
    let entry_wat = build
        .modules
        .iter()
        .find(|module| module.is_entry)
        .map(|module| module.wat.clone())
        .unwrap_or_default();
    let wasm_modules = build
        .modules
        .into_iter()
        .map(|module| WasmPackageModuleFile {
            module_segments: module.module_segments,
            is_entry: module.is_entry,
            wat: module.wat,
        })
        .collect::<Vec<_>>();
    PackageCompileResult {
        compile_result: CompileResult {
            output: Some(Output::WasmText(WasmTextOutput { code: entry_wat })),
            // Carry package-analysis diagnostics (warnings) forward like the
            // Go path; `build_package_wasm_from_graph` only surfaces errors.
            diagnostics: package.diagnostics.clone(),
        },
        wasm_modules,
        #[cfg(feature = "hir-go")]
        go_modules: Vec::new(),
    }
}

/// Failed compile: no output, no Go/Wasm modules (shared with the moved
/// emission dispatcher in `radix-package`).
fn compile_failure(diagnostics: Vec<Diagnostic>) -> PackageCompileResult {
    radix_package::emit::compile_failure(diagnostics)
}

#[cfg(feature = "hir-go")]
fn resolve_local_import_path(
    spec: &super::PackageSpec,
    from_file: &Path,
    import_path: &str,
) -> Option<PathBuf> {
    let dummy = crate::library::default_library_resolver();
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

/// Check every loaded package module and return diagnostics without codegen.
///
/// The checker mirrors package compilation discovery and CLI mount analysis so
/// `faber check` reports the same import, manifest, and mounted-command policy
/// errors that a package build would encounter.
///
/// RP6: the command-entry body moved to `radix-package`
/// (`analyze::check_package`); this forward keeps the faber signature for the
/// lib-test surface until RP7.
#[allow(dead_code)]
pub fn check_package(config: &Config, input: &Path) -> Vec<Diagnostic> {
    radix_package::analyze::check_package(config, input, &super::faber_analysis_seams())
}
