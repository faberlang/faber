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
use radix::hir::HirItemKind;
use radix::lexer::Interner;
use radix::syntax::{ImportDecl, ImportKind, StmtKind};
use radix::{CompileResult, Output, RustOutput};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::codegen::{assemble_crate, ModuleNode};
use super::file_interface::extract_file_interface;
use super::frontmatter::{manifest_path_for_spec, merge_entry_test_selection};
use super::import_graph::{
    build_mount_plan, library_import_binding, resolve_import, ImportResolution,
};
use super::{
    analysis_source_for_file, attach_library_provenance, discover_package, library_cached_analysis,
    library_cached_file_interface, library_generates_rust_module, library_imported_function_params,
    library_interface_export_names, library_interface_has_module, library_module_segments,
    library_resolver_from_config, load_package, load_package_with_reader_pack,
    load_reader_pack_for_input, program_export_names, read_manifest, LibraryImportBinding,
    LibraryInterfaceCache, PackageFile,
};

pub(crate) struct AnalyzedPackage {
    pub(crate) spec: super::PackageSpec,
    pub(crate) units: Vec<AnalyzedPackageUnit>,
    pub(crate) entry_frontmatter: Option<radix::driver::FileFrontmatter>,
    pub(crate) diagnostics: Vec<Diagnostic>,
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
pub(crate) fn analyze_package(
    config: &Config,
    input: &Path,
) -> Result<AnalyzedPackage, Vec<Diagnostic>> {
    let config = effective_package_config(config, input)?;
    let spec = discover_package(input).map_err(|diag| vec![*diag])?;
    let library_resolver = library_resolver_from_config(&config);
    analyze_package_spec(&config, spec, &library_resolver)
}

fn compile_package_internal(
    config: &Config,
    input: &Path,
    test_selection: Option<&RustTestSelection>,
) -> CompileResult {
    if config.target != Target::Rust {
        return CompileResult {
            output: None,
            diagnostics: vec![Diagnostic::error(
                "package compilation currently supports Rust target only",
            )
            .with_file(input.display().to_string())],
        };
    }

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

    let library_resolver = library_resolver_from_config(&config);
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
        let rust = match generate_package_unit_rust(
            unit,
            &siblings,
            library_resolver,
            &mut library_cache,
            test_selection,
            field_name_policy,
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
) -> Result<(), Diagnostic> {
    let mut seen = BTreeSet::new();
    for import in units
        .iter()
        .flat_map(|unit| unit.expanded_library_imports.iter())
    {
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
        if let Err(diag) = attach_library_provenance(
            &mut analysis,
            &provenance_imports,
            library_resolver,
            &mut library_cache,
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
    })
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
