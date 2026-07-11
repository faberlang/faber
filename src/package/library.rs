use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

use crate::library::{LibraryResolver, ResolvedLibraryModule};
use radix::codegen::rust::{remap_function_param_info, ImportedFunctionParams};
use radix::diagnostics::Diagnostic;
use radix::driver::{
    analyze_source_with_cli_program_and_import_contract, peel_raw_source, source_load_diagnostic,
    AnalyzedUnit, Config, Session,
};
use radix::file_interface::FileInterface;
use radix::hir::{
    DefId, HirItemKind, LibraryBinding, LibraryIdentity, LibraryItem, LibraryItemKind,
    LibraryProvider,
};
use radix::lexer::Interner;
use radix::parser;
use radix::semantic::TypeTable;
use radix::syntax::{AnnotationKind, Program, StmtKind, Visibility};

use super::import_graph::{
    inner_library_import_unresolved_diagnostic, library_import_binding,
    library_import_kind_diagnostic, library_resolve_diagnostic,
};
use super::{LibraryImportBinding, PackageFile};

struct LibraryInterfaceItem {
    exported_name: String,
    local_name: String,
    kind: LibraryItemKind,
    is_failable: bool,
    is_async: bool,
}

struct CachedLibraryInterface {
    peeled_body: String,
    program: Program,
    interner: Interner,
    analysis: Option<AnalyzedUnit>,
    file_interface: Option<FileInterface>,
}

type LibraryIdentityKey = (u8, String, Vec<String>);

#[derive(Default)]
pub(crate) struct LibraryInterfaceCache {
    entries: BTreeMap<LibraryIdentityKey, CachedLibraryInterface>,
}

#[allow(clippy::result_large_err)]
pub(crate) fn analysis_source_for_file(
    file: &PackageFile,
    _library_resolver: &LibraryResolver,
    _library_cache: &mut LibraryInterfaceCache,
) -> Result<String, Diagnostic> {
    Ok(file.source.clone())
}

#[allow(clippy::result_large_err)]
pub(crate) fn library_interface_export_names(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<Vec<String>, Diagnostic> {
    let cached = load_cached_library_interface(&import.module, library_cache)?;
    let mut names = program_export_names(&cached.program, &cached.interner);
    for child in public_library_imports(import, library_resolver, library_cache)? {
        names.push(child.binding.clone());
        for child_member in library_interface_export_names(&child, library_resolver, library_cache)?
        {
            names.push(format!("{}.{}", child.binding, child_member));
        }
    }
    Ok(names)
}

fn library_module_label(import: &LibraryImportBinding) -> String {
    import
        .module
        .module_name()
        .map(str::to_owned)
        .unwrap_or_else(|| import.module.package.clone())
}

fn library_identity_key(identity: &LibraryIdentity) -> LibraryIdentityKey {
    match &identity.provider {
        LibraryProvider::Builtin(package) => (0, package.clone(), identity.module_path.clone()),
        LibraryProvider::Package(package) => (1, package.clone(), identity.module_path.clone()),
    }
}

fn synthetic_library_item_def_id(
    module: &ResolvedLibraryModule,
    exported_name: &str,
    kind: &LibraryItemKind,
) -> DefId {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    module.package.hash(&mut hasher);
    module.module_path.hash(&mut hasher);
    exported_name.hash(&mut hasher);
    kind.hash(&mut hasher);
    DefId(0x8000_0000 | ((hasher.finish() as u32) & 0x7fff_ffff))
}

fn library_generated_call_path_with_crate(
    module: &ResolvedLibraryModule,
    exported_name: &str,
    linked_crate: Option<&str>,
) -> String {
    let mut path = match linked_crate {
        Some(crate_name) => rust_ident(crate_name),
        None => String::from("crate"),
    };
    for segment in &module.module_path {
        path.push_str("::");
        path.push_str(&rust_ident(segment));
    }
    path.push_str("::");
    path.push_str(exported_name);
    path
}

fn library_reexport_path_with_crate(
    module: &ResolvedLibraryModule,
    linked_crate: Option<&str>,
) -> String {
    let mut path = match linked_crate {
        Some(crate_name) => rust_ident(crate_name),
        None => String::from("crate"),
    };
    for segment in &module.module_path {
        path.push_str("::");
        path.push_str(&rust_ident(segment));
    }
    path
}

fn rust_ident(name: &str) -> String {
    // Shared with package module path emission so call paths match `pub mod` trees.
    super::modules::sanitize_rust_module_ident(name)
}

struct TransitiveLibraryWalk {
    ordered: Vec<LibraryImportBinding>,
    seen: BTreeSet<LibraryIdentityKey>,
    seen_bindings: BTreeMap<LibraryIdentityKey, String>,
    visiting: BTreeSet<LibraryIdentityKey>,
    stack: Vec<String>,
    cycle_detected: bool,
}

pub(super) fn expand_library_imports(
    direct: &[LibraryImportBinding],
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<LibraryImportBinding> {
    let mut walk = TransitiveLibraryWalk {
        ordered: Vec::new(),
        seen: BTreeSet::new(),
        seen_bindings: BTreeMap::new(),
        visiting: BTreeSet::new(),
        stack: Vec::new(),
        cycle_detected: false,
    };

    for import in direct {
        visit_transitive_library_import(
            import,
            library_resolver,
            library_cache,
            &mut walk,
            diagnostics,
        );
    }

    if walk.cycle_detected || diagnostics.iter().any(Diagnostic::is_error) {
        Vec::new()
    } else {
        walk.ordered
    }
}

fn take_first_error(diagnostics: &mut Vec<Diagnostic>) -> Option<Diagnostic> {
    let index = diagnostics.iter().position(Diagnostic::is_error)?;
    Some(diagnostics.remove(index))
}

fn visit_transitive_library_import(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    walk: &mut TransitiveLibraryWalk,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if walk.cycle_detected {
        return;
    }

    let identity = library_identity(&import.module);
    let key = library_identity_key(&identity);
    let module_label = library_module_label(import);

    if walk.seen.contains(&key) {
        if let Some(existing) = walk.seen_bindings.get(&key) {
            if existing != &import.binding {
                diagnostics.push(
                    Diagnostic::error(format!(
                        "library module `{module_label}` is imported under conflicting aliases `{existing}` and `{}`",
                        import.binding
                    ))
                    .with_file(import.module.interface_path.display().to_string())
                    .with_arg("issue", "library_conflicting_aliases")
                    .with_arg("module", module_label.clone())
                    .with_arg("existing_alias", existing.clone())
                    .with_arg("alias", import.binding.clone())
                    .with_span(import.import_span),
                );
            }
        }
        return;
    }

    if walk.visiting.contains(&key) {
        let mut cycle = walk.stack.clone();
        cycle.push(module_label);
        diagnostics.push(
            Diagnostic::error(format!(
                "library import cycle detected: {}",
                cycle.join(" -> ")
            ))
            .with_file(import.module.interface_path.display().to_string())
            .with_arg("issue", "library_import_cycle")
            .with_arg("cycle", cycle.join(" -> "))
            .with_span(import.import_span),
        );
        walk.cycle_detected = true;
        return;
    }

    walk.visiting.insert(key.clone());
    walk.stack.push(module_label);

    let inner_imports = match load_cached_library_interface(&import.module, library_cache) {
        Ok(cached) => library_imports_from_cached(
            cached,
            &import.module.interface_path,
            library_resolver,
            diagnostics,
        ),
        Err(diag) => {
            diagnostics.push(diag);
            Vec::new()
        }
    };
    for inner in &inner_imports {
        visit_transitive_library_import(inner, library_resolver, library_cache, walk, diagnostics);
        if walk.cycle_detected {
            walk.stack.pop();
            walk.visiting.remove(&key);
            return;
        }
    }

    walk.stack.pop();
    walk.visiting.remove(&key);
    walk.seen.insert(key.clone());
    walk.seen_bindings.insert(key, import.binding.clone());
    walk.ordered.push(import.clone());
}

#[allow(clippy::result_large_err)]
fn read_and_parse_library_interface(
    module: &ResolvedLibraryModule,
) -> Result<CachedLibraryInterface, Diagnostic> {
    let raw_source = fs::read_to_string(&module.interface_path)
        .map_err(|err| Diagnostic::io_error(&module.interface_path, err))?;
    let display_name = module.interface_path.display().to_string();
    let peeled = peel_raw_source(&display_name, &raw_source)
        .map_err(|error| source_load_diagnostic(&display_name, error))?;
    // Library interfaces may declare bodyless functions bound by target manifests (G4).
    let parse = parser::parse_with_options(
        radix::lexer::lex(peeled.body),
        parser::ParseOptions {
            allow_bodyless_functions: true,
        },
    );
    if !parse.success() {
        let mut message = format!(
            "library interface `{}` failed to parse",
            module.interface_path.display()
        );
        if let Some(err) = parse.errors.first() {
            let diagnostic = Diagnostic::from_parse_error(&display_name, peeled.body, err);
            message.push_str(&format!(": {}", diagnostic.message));
        }
        return Err(
            Diagnostic::error(message).with_file(module.interface_path.display().to_string())
        );
    }

    let Some(program) = parse.program else {
        return Err(
            Diagnostic::error("successful library interface parse result missing program")
                .with_file(module.interface_path.display().to_string()),
        );
    };

    Ok(CachedLibraryInterface {
        peeled_body: peeled.body.to_owned(),
        program,
        interner: parse.interner,
        analysis: None,
        file_interface: None,
    })
}

#[allow(clippy::result_large_err)]
fn load_cached_library_interface<'a>(
    module: &ResolvedLibraryModule,
    library_cache: &'a mut LibraryInterfaceCache,
) -> Result<&'a CachedLibraryInterface, Diagnostic> {
    let key = library_identity_key(&library_identity(module));
    if !library_cache.entries.contains_key(&key) {
        let cached = read_and_parse_library_interface(module)?;
        library_cache.entries.insert(key.clone(), cached);
    }
    library_cache.entries.get(&key).ok_or_else(|| {
        Diagnostic::error("library interface cache missing entry after insert")
            .with_file(module.interface_path.display().to_string())
    })
}

#[allow(clippy::result_large_err)]
pub(crate) fn library_cached_file_interface(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<FileInterface, Diagnostic> {
    let key = library_identity_key(&library_identity(&import.module));
    if let Some(interface) = library_cache
        .entries
        .get(&key)
        .and_then(|cached| cached.file_interface.clone())
    {
        return Ok(interface);
    }

    analyze_cached_library_interface(import, library_resolver, library_cache)?;
    library_cache
        .entries
        .get(&key)
        .and_then(|cached| cached.file_interface.clone())
        .ok_or_else(|| {
            Diagnostic::error("library interface cache missing extracted file interface")
                .with_file(import.module.interface_path.display().to_string())
        })
}

#[allow(clippy::result_large_err)]
pub(crate) fn library_cached_analysis<'a>(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &'a mut LibraryInterfaceCache,
) -> Result<&'a AnalyzedUnit, Diagnostic> {
    let key = library_identity_key(&library_identity(&import.module));
    if library_cache
        .entries
        .get(&key)
        .is_none_or(|cached| cached.analysis.is_none())
    {
        analyze_cached_library_interface(import, library_resolver, library_cache)?;
    }
    library_cache
        .entries
        .get(&key)
        .and_then(|cached| cached.analysis.as_ref())
        .ok_or_else(|| {
            Diagnostic::error("library interface cache missing analyzed unit")
                .with_file(import.module.interface_path.display().to_string())
        })
}

pub(crate) fn library_module_segments(import: &LibraryImportBinding) -> Vec<String> {
    import.module.module_path.clone()
}

#[allow(clippy::result_large_err)]
pub(crate) fn library_generates_rust_module(
    import: &LibraryImportBinding,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<bool, Diagnostic> {
    let cached = load_cached_library_interface(&import.module, library_cache)?;
    Ok(cached.program.statements.iter().any(|stmt| {
        !has_private_visibility(&stmt.annotations, &cached.interner)
            && matches!(
                stmt.kind,
                StmtKind::Func(_)
                    | StmtKind::Class(_)
                    | StmtKind::Enum(_)
                    | StmtKind::Union(_)
                    | StmtKind::TypeAlias(_)
                    | StmtKind::Var(_)
            )
    }))
}

#[allow(clippy::result_large_err)]
pub(crate) fn library_imported_function_params<'entry>(
    import: &LibraryImportBinding,
    entry_types: &mut TypeTable,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<ImportedFunctionParams<'entry>, Diagnostic> {
    let analysis = library_cached_analysis(import, library_resolver, library_cache)?;
    let mut params = ImportedFunctionParams::default();
    for item in &analysis.hir.items {
        let HirItemKind::Function(func) = &item.kind else {
            continue;
        };
        let name = analysis.interner.resolve(func.name);
        params.insert(
            synthetic_library_item_def_id(&import.module, name, &LibraryItemKind::Function),
            remap_function_param_info(func, entry_types, &analysis.types),
        );
    }
    Ok(params)
}

#[allow(clippy::result_large_err)]
fn analyze_cached_library_interface(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<(), Diagnostic> {
    let key = library_identity_key(&library_identity(&import.module));
    let mut diagnostics = Vec::new();
    let expanded = expand_library_imports(
        std::slice::from_ref(import),
        library_resolver,
        library_cache,
        &mut diagnostics,
    );
    if let Some(error) = take_first_error(&mut diagnostics) {
        return Err(error);
    }

    for dependency in &expanded {
        let dependency_key = library_identity_key(&library_identity(&dependency.module));
        if dependency_key != key {
            library_cached_file_interface(dependency, library_resolver, library_cache)?;
        }
    }

    let direct_imports = {
        let cached = load_cached_library_interface(&import.module, library_cache)?;
        library_imports_from_cached(
            cached,
            &import.module.interface_path,
            library_resolver,
            &mut diagnostics,
        )
    };
    if let Some(error) = take_first_error(&mut diagnostics) {
        return Err(error);
    }

    let mut namespace_exports = BTreeMap::new();
    for dependency in &direct_imports {
        namespace_exports.insert(
            dependency.binding.clone(),
            library_namespace_member_export_names(dependency, library_resolver, library_cache)?,
        );
    }
    let mut file_interfaces = BTreeMap::new();
    for dependency in &direct_imports {
        file_interfaces.insert(
            dependency.binding.clone(),
            library_cached_file_interface(dependency, library_resolver, library_cache)?,
        );
    }

    let analysis_source = {
        let cached = load_cached_library_interface(&import.module, library_cache)?;
        cached.peeled_body.clone()
    };

    // Native-binding library interfaces declare bodyless functions (G4).
    let session = Session::new(Config::default().with_bodyless_functions());
    let mut analysis = match analyze_source_with_cli_program_and_import_contract(
        &session,
        &import.module.interface_path.display().to_string(),
        &analysis_source,
        None,
        namespace_exports,
        file_interfaces,
    ) {
        Ok(analysis) => analysis,
        Err(diagnostics) => {
            return Err(diagnostics.into_iter().next().unwrap_or_else(|| {
                Diagnostic::error("library interface analysis failed without diagnostics")
                    .with_file(import.module.interface_path.display().to_string())
            }))
        }
    };
    attach_library_provenance(
        &mut analysis,
        &direct_imports,
        library_resolver,
        library_cache,
    )?;
    let export_names = {
        let cached = load_cached_library_interface(&import.module, library_cache)?;
        program_export_names(&cached.program, &cached.interner)
    };
    let export_identity = super::file_interface::ExportIdentityContext {
        provider: "package".to_owned(),
        package: Some(import.module.package.clone()),
        module_path: import.module.module_path.clone(),
    };
    let file_interface = super::file_interface::extract_file_interface_with_identity(
        &analysis,
        &export_names,
        &import.module.interface_path.display().to_string(),
        Some(&export_identity),
    )?;

    let cached = library_cache.entries.get_mut(&key).ok_or_else(|| {
        Diagnostic::error("library interface cache missing entry after analysis")
            .with_file(import.module.interface_path.display().to_string())
    })?;
    cached.analysis = Some(analysis);
    cached.file_interface = Some(file_interface);
    Ok(())
}

fn library_imports_from_cached(
    cached: &CachedLibraryInterface,
    interface_path: &Path,
    library_resolver: &LibraryResolver,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<LibraryImportBinding> {
    let mut imports = Vec::new();
    for stmt in &cached.program.statements {
        let StmtKind::Import(decl) = &stmt.kind else {
            continue;
        };
        let import_path = cached.interner.resolve(decl.path);
        match library_resolver.resolve(import_path) {
            Ok(Some(resolved)) => {
                if let Some(binding) = library_import_binding(&cached.interner, decl, resolved) {
                    imports.push(binding);
                } else {
                    diagnostics.push(library_import_kind_diagnostic(
                        interface_path,
                        decl,
                        import_path,
                    ));
                }
            }
            Ok(None) => {
                diagnostics.push(inner_library_import_unresolved_diagnostic(
                    interface_path,
                    decl,
                    import_path,
                ));
            }
            Err(err) => diagnostics
                .push(library_resolve_diagnostic(interface_path, err).with_span(decl.span)),
        }
    }

    imports
}

#[allow(clippy::result_large_err)]
pub(crate) fn library_interface_has_module(
    _import: &LibraryImportBinding,
    _library_cache: &mut LibraryInterfaceCache,
) -> Result<bool, Diagnostic> {
    Ok(false)
}

#[allow(clippy::result_large_err)]
pub(crate) fn attach_library_provenance(
    analysis: &mut radix::driver::AnalyzedUnit,
    imports: &[LibraryImportBinding],
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<(), Diagnostic> {
    attach_library_provenance_with_links(analysis, imports, library_resolver, library_cache, None)
}

/// Attach library provenance, routing native-binding package deps to external crates (G4).
#[allow(clippy::result_large_err)]
pub(crate) fn attach_library_provenance_with_links(
    analysis: &mut radix::driver::AnalyzedUnit,
    imports: &[LibraryImportBinding],
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    linked_library_crates: Option<&BTreeMap<String, String>>,
) -> Result<(), Diagnostic> {
    if imports.is_empty() {
        return Ok(());
    }

    let hir_items = analysis
        .hir
        .items
        .iter()
        .filter_map(|item| {
            hir_item_name_and_kind(item, &analysis.interner)
                .map(|(name, kind)| (name, kind, item.def_id))
        })
        .collect::<Vec<_>>();
    let hir_import_bindings = analysis
        .hir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Import(import) => Some(import),
            _ => None,
        })
        .flat_map(|import| {
            import.items.iter().map(|item| {
                let binding = item.alias.unwrap_or(item.name);
                (
                    analysis.interner.resolve(binding).to_owned(),
                    binding,
                    item.def_id,
                )
            })
        })
        .collect::<Vec<_>>();
    for import in imports {
        let identity = library_identity(&import.module);
        let linked_crate = linked_library_crates.and_then(|map| map.get(&import.module.package));
        let rust_runtime_module = None;
        let rust_runtime_methods = BTreeMap::new();
        let interface_items = library_interface_items(import, library_resolver, library_cache)?;
        let exported_members =
            library_namespace_member_export_names(import, library_resolver, library_cache)?;
        let has_namespace_binding = library_interface_has_module(import, library_cache)?;

        let binding_def_id = hir_import_bindings
            .iter()
            .find(|(name, _, _)| name.as_str() == import.binding)
            .map(|(_, _, def_id)| *def_id)
            .or_else(|| {
                hir_items
                    .iter()
                    .find(|(name, _, _)| name.as_str() == import.binding)
                    .map(|(_, _, def_id)| *def_id)
            });
        let imported_type_def_ids = hir_import_bindings
            .iter()
            .find(|(name, _, _)| name.as_str() == import.binding)
            .map(|(_, binding, _)| {
                analysis
                    .resolver
                    .imported_file_type_exports(*binding)
                    .into_iter()
                    .map(|export| {
                        (
                            analysis.interner.resolve(export.member).to_owned(),
                            export.def_id,
                        )
                    })
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default();

        if let Some(binding_def_id) = binding_def_id {
            analysis.libraries.bindings.insert(
                binding_def_id,
                LibraryBinding {
                    local_def_id: binding_def_id,
                    identity: identity.clone(),
                    rust_runtime_module: rust_runtime_module.clone(),
                    rust_runtime_methods,
                },
            );
            analysis
                .libraries
                .exports
                .insert(binding_def_id, exported_members.into_iter().collect());
            for child_import in public_library_imports(import, library_resolver, library_cache)? {
                let child_crate = linked_library_crates
                    .and_then(|map| map.get(&child_import.module.package))
                    .map(String::as_str);
                analysis.libraries.reexports.insert(
                    (binding_def_id, child_import.binding),
                    library_reexport_path_with_crate(&child_import.module, child_crate),
                );
            }
        } else if has_namespace_binding {
            return Err(Diagnostic::error(format!(
                "library import `{}` did not produce binding `{}` in analyzed HIR",
                import.module.package, import.binding
            ))
            .with_span(import.import_span));
        } else if !exported_members.is_empty() {
            for exported_member in exported_members {
                if interface_items
                    .iter()
                    .all(|item| item.exported_name != exported_member)
                {
                    return Err(Diagnostic::error(format!(
                        "library import `{}` did not produce binding `{}` in analyzed HIR",
                        import.module.package, import.binding
                    ))
                    .with_span(import.import_span));
                }
            }
        }

        for interface_item in interface_items {
            let rust_runtime_type = library_item_call_path_with_crate(
                &import.module,
                &interface_item,
                linked_crate.map(String::as_str),
            );
            let elide_rust_decl = false;
            let def_id = lookup_library_item_def_id(
                &hir_items,
                &interface_item.local_name,
                &interface_item.kind,
            )
            .or_else(|| {
                imported_type_def_ids
                    .get(&interface_item.exported_name)
                    .copied()
            })
            .unwrap_or_else(|| {
                synthetic_library_item_def_id(
                    &import.module,
                    &interface_item.exported_name,
                    &interface_item.kind,
                )
            });

            analysis.libraries.items.insert(
                def_id,
                LibraryItem {
                    def_id,
                    identity: identity.clone(),
                    exported_name: interface_item.exported_name,
                    kind: interface_item.kind,
                    is_failable: interface_item.is_failable,
                    is_async: interface_item.is_async,
                    rust_runtime_type,
                    elide_rust_decl,
                },
            );
        }
    }

    Ok(())
}

#[allow(clippy::result_large_err)]
fn library_namespace_member_export_names(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<Vec<String>, Diagnostic> {
    library_interface_export_names(import, library_resolver, library_cache)
}

#[allow(clippy::result_large_err)]
fn public_library_imports(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<Vec<LibraryImportBinding>, Diagnostic> {
    let cached = load_cached_library_interface(&import.module, library_cache)?;
    let mut diagnostics = Vec::new();
    let imports = library_imports_from_cached(
        cached,
        &import.module.interface_path,
        library_resolver,
        &mut diagnostics,
    )
    .into_iter()
    .filter(|binding| binding.visibility == Visibility::Publica)
    .collect::<Vec<_>>();
    if let Some(error) = take_first_error(&mut diagnostics) {
        return Err(error);
    }
    Ok(imports)
}

fn library_item_call_path_with_crate(
    module: &ResolvedLibraryModule,
    item: &LibraryInterfaceItem,
    linked_crate: Option<&str>,
) -> Option<String> {
    if item.kind == LibraryItemKind::Function {
        return Some(library_generated_call_path_with_crate(
            module,
            &item.exported_name,
            linked_crate,
        ));
    }
    None
}

fn lookup_library_item_def_id(
    hir_items: &[(String, LibraryItemKind, DefId)],
    local_name: &str,
    kind: &LibraryItemKind,
) -> Option<DefId> {
    hir_items
        .iter()
        .find(|(name, item_kind, _)| name == local_name && item_kind == kind)
        .map(|(_, _, def_id)| *def_id)
}

fn library_identity(module: &ResolvedLibraryModule) -> LibraryIdentity {
    let provider = if module.package == "norma" {
        LibraryProvider::Builtin(module.package.clone())
    } else {
        match module.provider {
            crate::library::LibraryProviderKind::PackageDependency => {
                LibraryProvider::Package(module.package.clone())
            }
        }
    };
    LibraryIdentity {
        provider,
        module_path: module.module_path.clone(),
    }
}

#[allow(clippy::result_large_err)]
fn library_interface_items(
    import: &LibraryImportBinding,
    _library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<Vec<LibraryInterfaceItem>, Diagnostic> {
    let cached = load_cached_library_interface(&import.module, library_cache)?;

    let mut items = Vec::new();
    for stmt in &cached.program.statements {
        if has_private_visibility(&stmt.annotations, &cached.interner) {
            continue;
        }
        if let Some(mut item) = library_interface_item(stmt, &cached.interner) {
            item.is_async = cached
                .file_interface
                .as_ref()
                .and_then(|interface| interface.exports.get(&item.exported_name))
                .is_some_and(|export| {
                    matches!(
                        &export.kind,
                        radix::file_interface::FileExportKind::Function(callable) if callable.is_async
                    )
                });
            items.push(item);
        }
    }
    Ok(items)
}

pub(crate) fn program_export_names(program: &Program, interner: &Interner) -> Vec<String> {
    program
        .statements
        .iter()
        .filter(|stmt| !has_private_visibility(&stmt.annotations, interner))
        .filter_map(|stmt| stmt_export_name(stmt, interner))
        .collect()
}

pub(crate) fn has_private_visibility(
    annotations: &[radix::syntax::Annotation],
    interner: &Interner,
) -> bool {
    annotations.iter().any(|annotation| match &annotation.kind {
        AnnotationKind::Privata => true,
        AnnotationKind::Statement(stmt) => interner.resolve(stmt.name.name) == "privata",
        _ => false,
    })
}

fn stmt_export_name(stmt: &radix::syntax::Stmt, interner: &Interner) -> Option<String> {
    let name = match &stmt.kind {
        StmtKind::Interface(interface) => interner.resolve(interface.name.name),
        StmtKind::Func(func) => interner.resolve(func.name.name),
        StmtKind::TypeAlias(alias) => interner.resolve(alias.name.name),
        StmtKind::Class(class) => interner.resolve(class.name.name),
        StmtKind::Enum(enm) => interner.resolve(enm.name.name),
        StmtKind::Union(union) => interner.resolve(union.name.name),
        StmtKind::Var(var) => {
            let radix::syntax::BindingPattern::Ident(ident) = &var.binding else {
                return None;
            };
            interner.resolve(ident.name)
        }
        _ => return None,
    };
    Some(name.to_owned())
}

fn library_interface_item(
    stmt: &radix::syntax::Stmt,
    interner: &Interner,
) -> Option<LibraryInterfaceItem> {
    let (name, kind, is_failable) = match &stmt.kind {
        StmtKind::Interface(interface) => (
            interner.resolve(interface.name.name),
            LibraryItemKind::Interface,
            false,
        ),
        StmtKind::Func(func) => (
            interner.resolve(func.name.name),
            LibraryItemKind::Function,
            func.err.is_some(),
        ),
        StmtKind::TypeAlias(alias) => (
            interner.resolve(alias.name.name),
            LibraryItemKind::TypeAlias,
            false,
        ),
        StmtKind::Class(class) => (
            interner.resolve(class.name.name),
            LibraryItemKind::Struct,
            false,
        ),
        StmtKind::Enum(enm) => (
            interner.resolve(enm.name.name),
            LibraryItemKind::Enum,
            false,
        ),
        StmtKind::Union(union) => (
            interner.resolve(union.name.name),
            LibraryItemKind::Enum,
            false,
        ),
        StmtKind::Var(var) => {
            let radix::syntax::BindingPattern::Ident(ident) = &var.binding else {
                return None;
            };
            (interner.resolve(ident.name), LibraryItemKind::Const, false)
        }
        _ => return None,
    };
    Some(LibraryInterfaceItem {
        exported_name: name.to_owned(),
        local_name: name.to_owned(),
        kind,
        is_failable,
        is_async: false,
    })
}

fn hir_item_name_and_kind(
    item: &radix::hir::HirItem,
    interner: &Interner,
) -> Option<(String, LibraryItemKind)> {
    match &item.kind {
        HirItemKind::Interface(interface) => Some((
            interner.resolve(interface.name).to_owned(),
            LibraryItemKind::Interface,
        )),
        HirItemKind::Function(func) => Some((
            interner.resolve(func.name).to_owned(),
            LibraryItemKind::Function,
        )),
        HirItemKind::TypeAlias(alias) => Some((
            interner.resolve(alias.name).to_owned(),
            LibraryItemKind::TypeAlias,
        )),
        HirItemKind::Struct(strukt) => Some((
            interner.resolve(strukt.name).to_owned(),
            LibraryItemKind::Struct,
        )),
        HirItemKind::Enum(enm) => {
            Some((interner.resolve(enm.name).to_owned(), LibraryItemKind::Enum))
        }
        HirItemKind::Const(konst) => Some((
            interner.resolve(konst.name).to_owned(),
            LibraryItemKind::Const,
        )),
        HirItemKind::Import(_) => None,
    }
}
