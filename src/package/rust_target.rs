//! Rust package assembly for the `hir-rust` target.
//!
//! The main package compiler owns source discovery, package graph analysis, and
//! target dispatch. This module owns the Rust-specific assembly step that turns
//! analyzed package units into one generated Rust crate.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use faber_hir_rust::{
    build_local_import_function_params, build_local_import_namespaces, local_import_module_key,
    remap_function_param_info, ImportedFunctionParams, ImportedNamespaceInfo, RustFieldNamePolicy,
    SiblingModuleExports,
};
use radix::diagnostics::Diagnostic;
use radix::hir::{DefId, HirItemKind, LibraryItemKind};
use radix::lexer::Interner;
use radix::{CompileResult, Output, RustOutput};

use super::codegen::{assemble_crate, ModuleNode};
use super::compile::{AnalyzedPackage, AnalyzedPackageUnit, PackageCompileResult};
use super::frontmatter::{manifest_path_for_spec, merge_entry_test_selection, RustTestSelection};
use super::import_graph::{resolve_import, ImportResolution};
use super::{
    library_cached_analysis, library_cached_expanded_imports, library_generates_rust_module,
    library_module_segments, read_manifest, with_library_cached_analysis_mut, LibraryImportBinding,
    LibraryInterfaceCache,
};

/// Result of Rust package assembly before final crate rendering.
pub(super) struct GeneratedPackageRust {
    pub(super) entry_code: Option<String>,
    pub(super) module_tree: ModuleNode,
    pub(super) diagnostics: Vec<Diagnostic>,
}

/// Crate root for packages whose `faber.toml` has no `paths.entry` (typical
/// `build.kind = "lib"` layout). Nested modules carry product + proba code;
/// cargo test discovers `#[test]` inside those modules.
const LIBRARY_PACKAGE_HARNESS_ENTRY: &str = "\
// Generated library package harness — no paths.entry file.
// Package units live in nested modules below.
fn main() {}\n";

pub(super) fn compile_package_rust(
    mut package: AnalyzedPackage,
    library_resolver: &crate::library::LibraryResolver,
    test_selection: Option<&RustTestSelection>,
) -> PackageCompileResult {
    let field_name_policy = match package_field_name_policy(&package.spec) {
        Ok(policy) => policy,
        Err(diag) => {
            return compile_failure(vec![*diag]);
        }
    };
    let effective_test_selection =
        merge_entry_test_selection(test_selection, package.entry_frontmatter.as_ref());

    let generated = generate_package_rust(
        &mut package,
        library_resolver,
        effective_test_selection.as_ref(),
        field_name_policy,
        None,
    );
    let diagnostics = generated.diagnostics;

    if diagnostics.iter().any(Diagnostic::is_error) {
        return compile_failure(diagnostics);
    }

    let Some(entry_code) = generated.entry_code else {
        return compile_failure(vec![crate::package_diagnostic_error(
            "package compilation did not produce an entry module",
        )
        .with_file(package.spec.entry.display().to_string())]);
    };

    let crate_code = assemble_crate(&entry_code, &generated.module_tree.render(0));
    PackageCompileResult {
        compile_result: CompileResult {
            output: Some(Output::Rust(RustOutput { code: crate_code })),
            diagnostics,
        },
        #[cfg(feature = "hir-go")]
        go_modules: Vec::new(),
    }
}

/// Structured runtime plan for a single `.fab` file (no package manifest).
///
/// Uses HIR/type facts via [`radix::codegen::collect_rust_needs`] — never scans
/// generated Rust text for `faber::` / `tokio::`.
pub(super) fn single_file_rust_runtime_plan(
    config: &radix::Config,
    input_path: &Path,
) -> Result<super::RustRuntimePlan, Vec<radix::Diagnostic>> {
    use radix::driver::{analyze_source, Session};
    use std::fs;

    let source = fs::read_to_string(input_path).map_err(|err| {
        vec![crate::package_diagnostic_error(format!(
            "failed to read '{}': {err}",
            input_path.display()
        ))]
    })?;
    let session = Session::new(config.clone());
    let name = input_path
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("input.fab");
    let analysis = analyze_source(&session, name, &source)?;
    let needs =
        radix::codegen::collect_rust_needs(&analysis.hir, &analysis.types, BTreeSet::new(), None);
    let needs_tokio =
        analysis.hir.entry_is_async
            || analysis.hir.items.iter().any(
                |item| matches!(&item.kind, radix::hir::HirItemKind::Function(f) if f.is_async),
            );
    Ok(super::RustRuntimePlan {
        needs_faber: needs.needs_faber_runtime,
        needs_tokio,
        host: None,
        non_runtime_routes: BTreeSet::new(),
        selected_providers: BTreeSet::new(),
        provider_manifests: Vec::new(),
        provider_error: None,
        library_path_deps: Vec::new(),
    })
}

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

pub(super) fn generate_package_rust(
    package: &mut AnalyzedPackage,
    library_resolver: &crate::library::LibraryResolver,
    test_selection: Option<&RustTestSelection>,
    field_name_policy: RustFieldNamePolicy,
    loaded_links: Option<&BTreeMap<PathBuf, BTreeMap<String, PathBuf>>>,
) -> GeneratedPackageRust {
    let mut entry_code = None;
    let mut module_tree = ModuleNode::default();
    let mut diagnostics = std::mem::take(&mut package.diagnostics);
    let mut library_cache = LibraryInterfaceCache::default();
    let native_host_bootstrap = manifest_path_for_spec(&package.spec)
        .and_then(|path| read_manifest(&path).ok())
        .and_then(|manifest| manifest.target.get("rust").and_then(|target| target.host))
        .is_some_and(|host| matches!(host, super::ManifestRustHost::Native));

    // FBR-P2-007: build one package-wide normalized path index once (unit
    // paths are normalized by discovery); each unit resolves its local imports
    // against it in one lookup and is excluded by identity. Sibling output
    // order stays deterministic: it follows each unit's import order.
    let units_by_path: HashMap<PathBuf, usize> = package
        .units
        .iter()
        .enumerate()
        .map(|(index, unit)| (super::paths::normalize_path(&unit.path), index))
        .collect();

    for index in 0..package.units.len() {
        let (before, rest) = package.units.split_at_mut(index);
        let Some((unit, after)) = rest.split_first_mut() else {
            continue;
        };
        let siblings = local_import_siblings_for_unit(
            unit,
            index,
            &units_by_path,
            before,
            after,
            &package.spec,
            library_resolver,
            loaded_links.and_then(|links| links.get(&unit.path)),
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
                crate::package_diagnostic_error(
                    "project compilation produced unresolved Rust backend names",
                )
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

    // Library packages often omit `paths.entry`, so discovery sets `entry` to the
    // source directory. No unit path equals that directory → no `is_entry` unit.
    // Synthesize a crate-root harness so module code (including `*.proba` tests)
    // can still assemble into a cargo-testable crate.
    if entry_code.is_none() {
        entry_code = Some(LIBRARY_PACKAGE_HARNESS_ENTRY.to_owned());
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

pub(super) fn render_binding_probe(
    analysis: &radix::driver::AnalyzedUnit,
    def_id: DefId,
    symbol: &str,
    probe_name: &str,
) -> Result<String, radix::codegen::CodegenError> {
    faber_hir_rust::render_binding_probe(analysis, def_id, symbol, probe_name)
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
        &mut LibraryNamespaceExtension {
            hir: &unit.analysis.hir,
            interner: &unit.analysis.interner,
            entry_types: &mut unit.analysis.types,
            resolver: &unit.analysis.resolver,
            library_resolver,
            library_cache,
            info: &mut imported_namespace_info,
        },
    )?;

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

struct LibraryNamespaceExtension<'entry, 'state> {
    hir: &'entry radix::hir::HirProgram,
    interner: &'entry Interner,
    entry_types: &'state mut radix::semantic::TypeTable,
    resolver: &'entry radix::semantic::Resolver,
    library_resolver: &'state crate::library::LibraryResolver,
    library_cache: &'state mut LibraryInterfaceCache,
    info: &'state mut ImportedNamespaceInfo<'entry>,
}

fn extend_library_namespace_type_paths(
    imports: &[LibraryImportBinding],
    context: &mut LibraryNamespaceExtension<'_, '_>,
) -> Result<(), radix::codegen::CodegenError> {
    for import in imports {
        let Some((binding, namespace_def_id)) =
            import_binding_symbol_and_def_id(context.hir, context.interner, &import.binding)
        else {
            continue;
        };
        let module_segments = library_module_segments(import);
        let analysis =
            library_cached_analysis(import, context.library_resolver, context.library_cache)
                .map_err(|diag| radix::codegen::CodegenError {
                    message: diag.message,
                    args: diag.args,
                })?;
        for item in &analysis.hir.items {
            let HirItemKind::Function(func) = &item.kind else {
                continue;
            };
            let name = analysis.interner.resolve(func.name).to_owned();
            context.info.function_params.insert(
                (namespace_def_id, name),
                remap_function_param_info(func, context.entry_types, &analysis.types),
            );
        }
        for export in context.resolver.imported_file_type_exports(binding) {
            let mut path = String::from("crate");
            for segment in &module_segments {
                path.push_str("::");
                path.push_str(segment);
            }
            path.push_str("::");
            path.push_str(context.interner.resolve(export.member));
            context.info.type_paths.insert(export.def_id, path);
        }
    }
    Ok(())
}

fn import_binding_symbol_and_def_id(
    hir: &radix::hir::HirProgram,
    interner: &Interner,
    binding_name: &str,
) -> Option<(radix::lexer::Symbol, radix::hir::DefId)> {
    hir.items.iter().find_map(|item| {
        let HirItemKind::Import(import) = &item.kind else {
            return None;
        };
        import.items.iter().find_map(|import_item| {
            let binding = import_item.alias.unwrap_or(import_item.name);
            (interner.resolve(binding) == binding_name).then_some((binding, import_item.def_id))
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
            library_imported_function_params(import, entry_types, library_resolver, library_cache)?;
        params.extend(library_params);
    }
    Ok(())
}

fn library_imported_function_params<'entry>(
    import: &LibraryImportBinding,
    entry_types: &mut radix::semantic::TypeTable,
    library_resolver: &crate::library::LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<ImportedFunctionParams<'entry>, radix::codegen::CodegenError> {
    let analysis =
        library_cached_analysis(import, library_resolver, library_cache).map_err(|diag| {
            radix::codegen::CodegenError {
                message: diag.message,
                args: diag.args,
            }
        })?;
    let mut params = ImportedFunctionParams::default();
    for item in &analysis.hir.items {
        let HirItemKind::Function(func) = &item.kind else {
            continue;
        };
        let name = analysis.interner.resolve(func.name);
        params.insert(
            super::library::synthetic_library_item_def_id(
                &import.module,
                name,
                &LibraryItemKind::Function,
            ),
            remap_function_param_info(func, entry_types, &analysis.types),
        );
    }
    Ok(params)
}

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
        let imports = library_cached_expanded_imports(import, library_resolver, library_cache)?;
        let rust = with_library_cached_analysis_mut(
            import,
            library_resolver,
            library_cache,
            |analysis, library_cache| {
                let mut imported_namespace_info = build_local_import_namespaces(
                    &analysis.hir,
                    &analysis.interner,
                    &mut analysis.types,
                    &analysis.resolver,
                    &[],
                );
                extend_library_namespace_type_paths(
                    &imports,
                    &mut LibraryNamespaceExtension {
                        hir: &analysis.hir,
                        interner: &analysis.interner,
                        entry_types: &mut analysis.types,
                        resolver: &analysis.resolver,
                        library_resolver,
                        library_cache,
                        info: &mut imported_namespace_info,
                    },
                )?;
                generate_rust_code_for_analysis(
                    analysis,
                    false,
                    test_selection,
                    field_name_policy,
                    false,
                    None,
                    Some(imported_namespace_info),
                )
            },
        )?;
        module_tree.insert(&key, rust);
    }
    Ok(())
}

/// Generate module-mode Rust for a library package unit (G4 library crates).
pub(super) fn generate_library_unit_rust(
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

#[allow(clippy::too_many_arguments)]
fn local_import_siblings_for_unit<'a>(
    unit: &AnalyzedPackageUnit,
    unit_index: usize,
    units_by_path: &HashMap<PathBuf, usize>,
    before: &'a [AnalyzedPackageUnit],
    after: &'a [AnalyzedPackageUnit],
    spec: &super::PackageSpec,
    library_resolver: &crate::library::LibraryResolver,
    links: Option<&BTreeMap<String, PathBuf>>,
) -> Vec<SiblingModuleExports<'a>> {
    let mut siblings = Vec::new();
    if let Some(links) = links {
        // Loaded-package path: local imports resolve from the explicit link
        // table (import binding → target module path), never from the
        // filesystem. The link table was captured at analysis time, so the
        // loaded path observes the same sibling relationships the direct path
        // would have derived from disk.
        let mut seen_targets = BTreeSet::new();
        for item in &unit.analysis.hir.items {
            let radix::hir::HirItemKind::Import(import) = &item.kind else {
                continue;
            };
            let import_path = unit.analysis.interner.resolve(import.path);
            for import_item in &import.items {
                let binding = unit
                    .analysis
                    .interner
                    .resolve(import_item.alias.unwrap_or(import_item.name));
                let Some(target) = links.get(binding) else {
                    continue;
                };
                if !seen_targets.insert(target.clone()) {
                    continue;
                }
                let Some(&candidate_index) = units_by_path.get(target) else {
                    continue;
                };
                if candidate_index == unit_index {
                    continue;
                }
                let sibling = if candidate_index < unit_index {
                    &before[candidate_index]
                } else {
                    &after[candidate_index - unit_index - 1]
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
        }
        return siblings;
    }
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
        // One lookup against the package-wide index; the current unit is
        // never its own sibling (excluded by identity).
        let Some(&candidate_index) = units_by_path.get(&target) else {
            continue;
        };
        if candidate_index == unit_index {
            continue;
        }
        let sibling = if candidate_index < unit_index {
            &before[candidate_index]
        } else {
            &after[candidate_index - unit_index - 1]
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
    imported_function_params: Option<faber_hir_rust::ImportedFunctionParams<'_>>,
    imported_namespace_info: Option<faber_hir_rust::ImportedNamespaceInfo<'_>>,
) -> Result<String, radix::codegen::CodegenError> {
    let cli_program = analysis.cli_program.as_ref();
    let module_mode = !is_entry;
    let codegen_test_selection = rust_codegen_test_selection(test_selection);
    if is_entry {
        if let Some(cli_program) = cli_program {
            let mut codegen =
                faber_hir_rust::RustCodegen::new_with_library_registry_test_selection_and_types(
                    &analysis.hir,
                    &analysis.interner,
                    &analysis.libraries,
                    codegen_test_selection.clone(),
                    Some(&analysis.types),
                );
            if let Some(params) = imported_function_params {
                codegen.set_imported_function_params(params);
            }
            if let Some(info) = imported_namespace_info {
                codegen.set_imported_namespace_info(info);
            }
            let gpu_builtins = faber_hir_rust::rust_gpu_builtins(&analysis.gpu_builtins);
            codegen.set_gpu_builtins(&gpu_builtins);
            codegen.set_field_name_policy(field_name_policy);
            codegen.set_native_host_bootstrap(native_host_bootstrap);
            let cli_ir = faber_hir_rust::to_cli_ir(cli_program);
            return codegen
                .generate_cli(&analysis.hir, &analysis.types, &cli_ir)
                .map(|output| output.code);
        }
    }

    let gpu_builtins = faber_hir_rust::rust_gpu_builtins(&analysis.gpu_builtins);
    let cli_ir = if module_mode {
        None
    } else {
        analysis.cli_program.as_ref().map(faber_hir_rust::to_cli_ir)
    };
    // Leaf ModuleGenerationRequest borrows CLI IR; keep owned ir alive for the call.
    let cli_ir_ref = cli_ir.as_ref();
    faber_hir_rust::generate_with_library_registry_test_selection_and_imports(
        faber_hir_rust::ModuleGenerationRequest {
            hir: &analysis.hir,
            types: &analysis.types,
            interner: &analysis.interner,
            libraries: &analysis.libraries,
            test_selection: codegen_test_selection,
            module_mode,
            cli_program: cli_ir_ref,
            imported_function_params,
            imported_namespace_info,
            gpu_builtins: &gpu_builtins,
            field_name_policy,
            native_host_bootstrap,
        },
    )
    .map(|output| output.code)
}

fn rust_codegen_test_selection(
    selection: Option<&RustTestSelection>,
) -> Option<faber_hir_rust::TestSelection> {
    selection.map(|selection| faber_hir_rust::TestSelection {
        name: selection.name.clone(),
        suite: selection.suite.clone(),
        tag: selection.tag.clone(),
    })
}

fn package_field_name_policy(
    spec: &super::PackageSpec,
) -> Result<RustFieldNamePolicy, Box<Diagnostic>> {
    let Some(path) = manifest_path_for_spec(spec) else {
        return Ok(RustFieldNamePolicy::Preserve);
    };
    let manifest = read_manifest(&path)?;
    Ok(match manifest.build.rust_field_names {
        super::ManifestRustFieldNames::Preserve => RustFieldNamePolicy::Preserve,
        super::ManifestRustFieldNames::SnakeCase => RustFieldNamePolicy::SnakeCase,
    })
}
