//! Package-local namespace-call linking and rewrite passes.

use super::*;
use radix::hir::visit::{HirVisitorMut, walk_expr_mut};

struct PackageMirLinkAccumulator {
    targets: NamespaceCallTargets,
    data_member_targets: NamespaceDataMemberTargets,
    data_members: Vec<DataMemberLink>,
    namespaces: NamespaceExports,
    source_rewrites: SourceRewrites,
    next_synthetic: u32,
    diagnostics: Vec<Diagnostic>,
    libraries: Vec<LibraryLinkTarget>,
    method_targets: MethodCallTargets,
}

impl Default for PackageMirLinkAccumulator {
    fn default() -> Self {
        Self {
            targets: HashMap::new(),
            data_member_targets: HashMap::new(),
            data_members: Vec::new(),
            namespaces: HashMap::new(),
            source_rewrites: HashMap::new(),
            next_synthetic: PACKAGE_MIR_SYNTHETIC_DEF_BASE,
            diagnostics: Vec::new(),
            libraries: Vec::new(),
            method_targets: HashMap::new(),
        }
    }
}

struct LibraryImportSite {
    caller_path: PathBuf,
    import_path: String,
    visibility: radix::syntax::Visibility,
    items: Vec<LibraryImportSiteItem>,
}

struct LibraryImportSiteItem {
    def_id: DefId,
    binding: String,
}

pub(super) fn local_namespace_call_targets(
    package: &AnalyzedPackage,
    consumer: PackageMirConsumer,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    loaded_links: Option<&BTreeMap<PathBuf, BTreeMap<String, PathBuf>>>,
) -> Result<PackageMirLinks, Vec<Diagnostic>> {
    let units_by_path = package
        .units
        .iter()
        .map(|unit| (unit.path.clone(), unit))
        .collect::<BTreeMap<_, _>>();
    let mut links = PackageMirLinkAccumulator::default();

    for unit in &package.units {
        for item in &unit.analysis.hir.items {
            let HirItemKind::Import(import) = &item.kind else {
                continue;
            };
            if let Some(loaded_links) = loaded_links {
                // Loaded-package path: local imports resolve from the explicit
                // link table (import binding → target module path) instead of
                // the filesystem. Library imports are not linked here — the
                // store owns library closure (Stage 5 of the package delivery).
                let Some(unit_links) = loaded_links.get(&unit.path) else {
                    continue;
                };
                for import_item in &import.items {
                    let binding = unit
                        .analysis
                        .interner
                        .resolve(import_item.alias.unwrap_or(import_item.name));
                    let Some(target_path) = unit_links.get(binding) else {
                        continue;
                    };
                    let Some(sibling) = units_by_path.get(target_path).copied() else {
                        continue;
                    };
                    let exports = unit
                        .namespace_exports
                        .get(binding)
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .collect::<BTreeSet<_>>();
                    links
                        .namespaces
                        .insert((unit.path.clone(), import_item.def_id), exports.clone());
                    for function in exported_top_level_functions(sibling, &exports) {
                        let synthetic = *links
                            .source_rewrites
                            .entry((sibling.path.clone(), function.def_id))
                            .or_insert_with(|| {
                                let def_id = DefId(links.next_synthetic);
                                links.next_synthetic += 1;
                                def_id
                            });
                        links.targets.insert(
                            (unit.path.clone(), import_item.def_id, function.name),
                            synthetic,
                        );
                    }
                    for member in exported_top_level_consts(sibling, &exports) {
                        link_const_member(&mut links, &unit.path, import_item.def_id, &sibling.path, &member);
                    }
                }
                continue;
            }
            let import_path = unit.analysis.interner.resolve(import.path);
            let resolution =
                resolve_import(&package.spec, library_resolver, &unit.path, import_path);
            match resolution {
                ImportResolution::Local(target_path) => {
                    let Some(sibling) = units_by_path.get(&target_path).copied() else {
                        continue;
                    };
                    for import_item in &import.items {
                        let binding = unit
                            .analysis
                            .interner
                            .resolve(import_item.alias.unwrap_or(import_item.name));
                        let exports = unit
                            .namespace_exports
                            .get(binding)
                            .cloned()
                            .unwrap_or_default()
                            .into_iter()
                            .collect::<BTreeSet<_>>();
                        links
                            .namespaces
                            .insert((unit.path.clone(), import_item.def_id), exports.clone());
                        for function in exported_top_level_functions(sibling, &exports) {
                            let synthetic = *links
                                .source_rewrites
                                .entry((sibling.path.clone(), function.def_id))
                                .or_insert_with(|| {
                                    let def_id = DefId(links.next_synthetic);
                                    links.next_synthetic += 1;
                                    def_id
                                });
                            links.targets.insert(
                                (unit.path.clone(), import_item.def_id, function.name),
                                synthetic,
                            );
                        }
                        for member in exported_top_level_consts(sibling, &exports) {
                            link_const_member(
                                &mut links,
                                &unit.path,
                                import_item.def_id,
                                &sibling.path,
                                &member,
                            );
                        }
                    }
                }
                ImportResolution::Library(module) => {
                    // Kernel-manifest `norma:*` modules stay on the
                    // stepper-kernel bridge (`bridge_norma_providers_to_kernel`);
                    // they are not linked into the program.
                    if is_bridged_norma_import_path(import_path) {
                        continue;
                    }
                    // Only the interpreted run path links library imports.
                    // External-target builds keep the previous behavior:
                    // library calls stay provider-backed and emit as-is.
                    if consumer != PackageMirConsumer::Interpreted {
                        continue;
                    }
                    let site = LibraryImportSite {
                        caller_path: unit.path.clone(),
                        import_path: import_path.to_owned(),
                        visibility: import.visibility,
                        items: import
                            .items
                            .iter()
                            .map(|item| LibraryImportSiteItem {
                                def_id: item.def_id,
                                binding: unit
                                    .analysis
                                    .interner
                                    .resolve(item.alias.unwrap_or(item.name))
                                    .to_owned(),
                            })
                            .collect(),
                    };
                    link_library_import_site(
                        &mut links,
                        &site,
                        &module,
                        library_resolver,
                        library_cache,
                    );
                }
                ImportResolution::Error(diag) => {
                    // External-target builds keep the previous silent-skip;
                    // only the interpreted run path fails closed here.
                    if consumer == PackageMirConsumer::Interpreted {
                        links.diagnostics.push(diag);
                    }
                }
                ImportResolution::Unsupported => continue,
            }
        }
    }

    if consumer == PackageMirConsumer::Interpreted {
        link_nested_library_imports(&mut links, library_resolver, library_cache);
    }

    if consumer == PackageMirConsumer::Interpreted {
        // FMIR e2e-hardening (CTO-1): link genus-method call targets for
        // methods on linked library nominals (receiver.method(args) →
        // Path(synthetic)(receiver, args)). Runs after the library closure is
        // final so every library that can receive method calls is registered.
        link_library_method_targets(&mut links, package, library_resolver, library_cache);
    }

    if !links.diagnostics.is_empty() {
        return Err(links.diagnostics);
    }

    Ok(PackageMirLinks {
        calls: links.targets,
        data_member_targets: links.data_member_targets,
        data_members: links.data_members,
        namespaces: links.namespaces,
        sources: links.source_rewrites,
        next_synthetic: links.next_synthetic,
        libraries: links.libraries,
        method_targets: links.method_targets,
    })
}

fn link_library_import_site(
    links: &mut PackageMirLinkAccumulator,
    site: &LibraryImportSite,
    module: &crate::library::ResolvedLibraryModule,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) {
    let Some(first_item) = site.items.first() else {
        return;
    };
    let import = LibraryImportBinding {
        binding: first_item.binding.clone(),
        visibility: site.visibility,
        import_span: radix::lexer::Span::default(),
        module: module.clone(),
    };
    let interface = match library_cached_file_interface(&import, library_resolver, library_cache) {
        Ok(interface) => interface,
        Err(_) => {
            push_library_import_unsupported(
                &mut links.diagnostics,
                &site.caller_path,
                &site.import_path,
                PackageMirConsumer::Interpreted,
            );
            return;
        }
    };
    let functions = match library_cached_analysis(&import, library_resolver, library_cache) {
        Ok(analysis) => {
            let mut functions = analysis
                .hir
                .items
                .iter()
                .filter_map(|item| {
                    let HirItemKind::Function(function) = &item.kind else {
                        return None;
                    };
                    Some(ExportedFunction {
                        name: analysis.interner.resolve(function.name).to_owned(),
                        def_id: item.def_id,
                    })
                })
                .collect::<Vec<_>>();
            // Compiler-generated companions have no HIR item, but their source
            // DefId is the identity carried by their generated MIR function.
            // A declared companion with no resolvable symbol fails closed
            // here (stable `package_mir_companion_unresolved` identity)
            // instead of silently vanishing from the link and surfacing later
            // as an unstable stepper-internal error.
            for (primal_def_id, backward) in analysis.radix_lanes.iter_backward() {
                let name = analysis
                    .interner
                    .resolve(backward.companion_name)
                    .to_owned();
                if !companion_resolves(analysis, primal_def_id, backward) {
                    push_companion_unresolved(
                        &mut links.diagnostics,
                        &site.caller_path,
                        &site.import_path,
                        &name,
                        PackageMirConsumer::Interpreted,
                    );
                    continue;
                }
                functions.push(ExportedFunction {
                    name,
                    def_id: backward.companion_def_id,
                });
            }
            functions
        }
        Err(_) => {
            push_library_import_unsupported(
                &mut links.diagnostics,
                &site.caller_path,
                &site.import_path,
                PackageMirConsumer::Interpreted,
            );
            return;
        }
    };
    let consts = match library_cached_analysis(&import, library_resolver, library_cache) {
        Ok(analysis) => analysis
            .hir
            .items
            .iter()
            .filter_map(|item| {
                let HirItemKind::Constant(konst) = &item.kind else {
                    return None;
                };
                Some(ExportedConst {
                    name: analysis.interner.resolve(konst.name).to_owned(),
                    def_id: item.def_id,
                    konst: konst.clone(),
                })
            })
            .collect::<Vec<_>>(),
        Err(_) => {
            push_library_import_unsupported(
                &mut links.diagnostics,
                &site.caller_path,
                &site.import_path,
                PackageMirConsumer::Interpreted,
            );
            return;
        }
    };
    let exports = interface.exports.keys().cloned().collect::<BTreeSet<_>>();
    let library_path = module.interface_path.clone();

    for item in &site.items {
        links
            .namespaces
            .insert((site.caller_path.clone(), item.def_id), exports.clone());
        // Allocate every function source, not only public call targets. Private
        // functions can be reached from an exported function in the same module.
        for function in &functions {
            let synthetic = *links
                .source_rewrites
                .entry((library_path.clone(), function.def_id))
                .or_insert_with(|| {
                    let def_id = DefId(links.next_synthetic);
                    links.next_synthetic += 1;
                    def_id
                });
            if exports.contains(&function.name) {
                links.targets.insert(
                    (site.caller_path.clone(), item.def_id, function.name.clone()),
                    synthetic,
                );
            }
        }
        for member in &consts {
            if exports.contains(&member.name) {
                link_const_member(links, &site.caller_path, item.def_id, &library_path, member);
            }
        }
    }

    if links
        .libraries
        .iter()
        .all(|target| target.path != module.interface_path)
    {
        links.libraries.push(LibraryLinkTarget {
            path: module.interface_path.clone(),
            import,
        });
    }
}

/// Whether a declared `@ radix backward` companion can be linked by the
/// package-MIR path.
///
/// The companion must carry a typed resolver symbol — the same gate the
/// file-interface extractor applies, since the interface only declares
/// companions whose symbol resolves (`file_interface.rs` companion loop) —
/// and its primal must be a valid reverse-AD companion candidate (present in
/// HIR with a return type and at least one tensor parameter). Without the
/// latter, radix's snapshot pass skips the primal and no companion function
/// is generated, so the link target would dangle into an unstable
/// stepper-internal `missing MIR function` error.
fn companion_resolves(
    analysis: &radix::driver::AnalyzedUnit,
    primal_def_id: DefId,
    backward: &radix::semantic::RadixBackward,
) -> bool {
    let Some(symbol) = analysis.resolver.get_symbol(backward.companion_def_id) else {
        return false;
    };
    if symbol.ty.is_none() {
        return false;
    }
    let Some(function) = analysis
        .hir
        .items
        .iter()
        .find_map(|item| (item.def_id == primal_def_id).then_some(&item.kind))
    else {
        return false;
    };
    let HirItemKind::Function(function) = function else {
        return false;
    };
    if function.ret_ty.is_none() {
        return false;
    }
    function
        .params
        .iter()
        .any(|param| radix::air::reverse_ad::is_tensor_type(param.ty, &analysis.types))
}

fn link_nested_library_imports(
    links: &mut PackageMirLinkAccumulator,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) {
    // Breadth-first closure over resolved module identity. `link_library_import_site`
    // always installs the caller-specific namespace edge, while `libraries`
    // deduplicates lowering and makes import cycles finite.
    let mut library_index = 0;
    while library_index < links.libraries.len() {
        let library = links.libraries[library_index].import.clone();
        library_index += 1;
        let sites = match nested_library_import_sites(&library, library_resolver, library_cache) {
            Ok(sites) => sites,
            Err(_) => {
                push_library_import_unsupported(
                    &mut links.diagnostics,
                    &library.module.interface_path,
                    &library.module.package,
                    PackageMirConsumer::Interpreted,
                );
                continue;
            }
        };
        for site in sites {
            if is_bridged_norma_import_path(&site.import_path) {
                continue;
            }
            match library_resolver.resolve(&site.import_path) {
                Ok(Some(module)) => {
                    link_library_import_site(links, &site, &module, library_resolver, library_cache)
                }
                Ok(None) | Err(_) => push_library_import_unsupported(
                    &mut links.diagnostics,
                    &site.caller_path,
                    &site.import_path,
                    PackageMirConsumer::Interpreted,
                ),
            }
        }
    }
}

fn nested_library_import_sites(
    import: &LibraryImportBinding,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<Vec<LibraryImportSite>, Diagnostic> {
    let analysis = library_cached_analysis(import, library_resolver, library_cache)?;
    Ok(analysis
        .hir
        .items
        .iter()
        .filter_map(|item| {
            let HirItemKind::Import(nested) = &item.kind else {
                return None;
            };
            Some(LibraryImportSite {
                caller_path: import.module.interface_path.clone(),
                import_path: analysis.interner.resolve(nested.path).to_owned(),
                visibility: nested.visibility,
                items: nested
                    .items
                    .iter()
                    .map(|item| LibraryImportSiteItem {
                        def_id: item.def_id,
                        binding: analysis
                            .interner
                            .resolve(item.alias.unwrap_or(item.name))
                            .to_owned(),
                    })
                    .collect(),
            })
        })
        .collect())
}

/// Link genus-method call targets for methods on linked library nominals
/// (FMIR e2e-hardening CTO-1).
///
/// Every linked library's public struct methods get a package-MIR synthetic
/// source (same discipline as top-level functions, so cross-library def-id
/// collisions stay impossible and S1 U3 reachability prunes unused methods),
/// and each method call is registered per caller: key = the caller's
/// entry-side nominal `DefId` for the library's struct (the receiver type
/// every caller analysis carries after the canonical nominal import) +
/// method name → the synthetic. The rewrite pass then lowers
/// `receiver.method(args)` to `Path(synthetic)(receiver, args)`, which the
/// merged program resolves to the library's lowered method function.
fn link_library_method_targets(
    links: &mut PackageMirLinkAccumulator,
    package: &AnalyzedPackage,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) {
    // Phase 1: extract the source library struct → method tables. The
    // library cache borrows are released after each extraction (the caller
    // pass below re-borrows the cache per caller, one at a time).
    struct SourceStruct {
        library_path: PathBuf,
        identity: Option<radix::file_interface::InterfaceLibraryIdentity>,
        /// (struct export name, method name, method def id)
        methods: Vec<(String, String, DefId)>,
    }
    let mut sources: Vec<SourceStruct> = Vec::new();
    for library in &links.libraries {
        let Ok(interface) = library_cached_file_interface(&library.import, library_resolver, library_cache)
        else {
            continue;
        };
        let Ok(analysis) = library_cached_analysis(&library.import, library_resolver, library_cache)
        else {
            continue;
        };
        let mut methods = Vec::new();
        for export in interface.exports.values() {
            let radix::file_interface::FileExportKind::Struct(struct_export) = &export.kind else {
                continue;
            };
            // Find the library's struct item (methods carry the def ids).
            let Some(struct_item) = analysis.hir.items.iter().find(|item| {
                let HirItemKind::Struct(strukt) = &item.kind else {
                    return false;
                };
                analysis.interner.resolve(strukt.name) == struct_export.name
            }) else {
                continue;
            };
            let HirItemKind::Struct(strukt) = &struct_item.kind else {
                continue;
            };
            for method in &strukt.methods {
                methods.push((
                    struct_export.name.clone(),
                    analysis.interner.resolve(method.func.name).to_owned(),
                    method.def_id,
                ));
            }
        }
        sources.push(SourceStruct {
            library_path: library.path.clone(),
            identity: interface.identity.clone(),
            methods,
        });
    }

    // Phase 2: per caller (package units, then linked libraries — a library
    // can call methods on ANOTHER linked library's nominals), register the
    // method targets that resolve through the caller's canonical nominal
    // table.
    let register_caller = |links: &mut PackageMirLinkAccumulator,
                               caller_path: &Path,
                               resolver: &radix::semantic::Resolver,
                               interner: &Interner| {
        for source in &sources {
            if source.library_path == caller_path {
                continue;
            }
            let Some(identity) = source.identity.as_ref() else {
                continue;
            };
            for (struct_name, method_name, method_def_id) in &source.methods {
                let Some(entry_nominal_def) = resolver.imported_nominal_def_id_by_name(
                    radix::file_interface::InterfaceNominalKind::Struct,
                    Some(identity),
                    struct_name,
                    interner,
                ) else {
                    continue;
                };
                let synthetic = *links
                    .source_rewrites
                    .entry((source.library_path.clone(), *method_def_id))
                    .or_insert_with(|| {
                        let def_id = DefId(links.next_synthetic);
                        links.next_synthetic += 1;
                        def_id
                    });
                links
                    .method_targets
                    .entry(caller_path.to_path_buf())
                    .or_default()
                    .insert((entry_nominal_def, method_name.clone()), synthetic);
            }
        }
    };

    for unit in &package.units {
        register_caller(links, &unit.path, &unit.analysis.resolver, &unit.analysis.interner);
    }
    // The linked-library caller loop needs `&mut links` for synthetic
    // allocation while iterating the closure, so the target list is cloned
    // first (LibraryImportBinding is Clone).
    let library_targets: Vec<(PathBuf, LibraryImportBinding)> = links
        .libraries
        .iter()
        .map(|target| (target.path.clone(), target.import.clone()))
        .collect();
    for (path, import) in &library_targets {
        if let Ok(analysis) = library_cached_analysis(import, library_resolver, library_cache) {
            register_caller(links, path, &analysis.resolver, &analysis.interner);
        }
    }
}

/// Fail-closed diagnostic for a library import the MIR package path cannot
/// link. Preserves the `package_mir_library_imports_unsupported` identity;
/// external-target consumers keep the previous silent-skip behavior.
pub(super) fn push_library_import_unsupported(
    diagnostics: &mut Vec<Diagnostic>,
    unit_path: &Path,
    import_path: &str,
    consumer: PackageMirConsumer,
) {
    if consumer != PackageMirConsumer::Interpreted {
        return;
    }
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "package MIR does not yet support library imports such as `{import_path}`; use compiled package execution for this surface"
        ))
        .with_file(unit_path.display().to_string())
        .with_arg("issue", "package_mir_library_imports_unsupported")
        .with_arg("import", import_path),
    );
}

/// Fail-closed diagnostic for a declared `@ radix backward` companion that
/// has no resolvable symbol in the library analysis. Stable identity
/// `package_mir_companion_unresolved`; external-target consumers keep the
/// previous silent-skip behavior.
pub(super) fn push_companion_unresolved(
    diagnostics: &mut Vec<Diagnostic>,
    unit_path: &Path,
    import_path: &str,
    companion_name: &str,
    consumer: PackageMirConsumer,
) {
    if consumer != PackageMirConsumer::Interpreted {
        return;
    }
    diagnostics.push(
        crate::package_diagnostic_error(format!(
            "companion `{companion_name}` declared by library `{import_path}` has no resolvable symbol in the analysis; package MIR cannot link it"
        ))
        .with_file(unit_path.display().to_string())
        .with_arg("issue", "package_mir_companion_unresolved")
        .with_arg("import", import_path)
        .with_arg("companion", companion_name),
    );
}

pub(super) struct ExportedFunction {
    name: String,
    def_id: DefId,
}

pub(super) fn exported_top_level_functions(
    unit: &AnalyzedPackageUnit,
    exports: &BTreeSet<String>,
) -> Vec<ExportedFunction> {
    unit.analysis
        .hir
        .items
        .iter()
        .filter_map(|item| {
            let HirItemKind::Function(function) = &item.kind else {
                return None;
            };
            let name = unit.analysis.interner.resolve(function.name).to_owned();
            exports.contains(&name).then_some(ExportedFunction {
                name,
                def_id: item.def_id,
            })
        })
        .collect()
}

pub(super) struct ExportedConst {
    name: String,
    def_id: DefId,
    konst: HirConst,
}

/// Top-level const data members a sibling/library unit exports. Like
/// [`exported_top_level_functions`], this walks the analysis HIR so the
/// synthetic def id and the const's value travel together.
pub(super) fn exported_top_level_consts(
    unit: &AnalyzedPackageUnit,
    exports: &BTreeSet<String>,
) -> Vec<ExportedConst> {
    unit.analysis
        .hir
        .items
        .iter()
        .filter_map(|item| {
            let HirItemKind::Constant(konst) = &item.kind else {
                return None;
            };
            let name = unit.analysis.interner.resolve(konst.name).to_owned();
            exports.contains(&name).then(|| ExportedConst {
                name,
                def_id: item.def_id,
                konst: konst.clone(),
            })
        })
        .collect()
}

/// Link one exported const data member: allocate a synthetic def id in the
/// package-MIR range (same discipline as function sources), register the
/// member reference target, and carry the const declaration for the entry
/// lowering's transplant.
fn link_const_member(
    links: &mut PackageMirLinkAccumulator,
    caller_path: &Path,
    import_def_id: DefId,
    source_path: &Path,
    member: &ExportedConst,
) {
    let synthetic = *links
        .source_rewrites
        .entry((source_path.to_path_buf(), member.def_id))
        .or_insert_with(|| {
            let def_id = DefId(links.next_synthetic);
            links.next_synthetic += 1;
            def_id
        });
    links
        .data_member_targets
        .insert((caller_path.to_path_buf(), import_def_id, member.name.clone()), synthetic);
    links.data_members.push(DataMemberLink {
        synthetic,
        source_path: source_path.to_path_buf(),
        konst: member.konst.clone(),
    });
}

pub(super) fn rewrite_unit_namespace_calls(
    unit: &mut AnalyzedPackageUnit,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    method_targets: &MethodCallTargets,
) -> Result<(), Vec<Diagnostic>> {
    rewrite_analysis_namespace_calls(
        &unit.path,
        &mut unit.analysis,
        targets,
        data_member_targets,
        namespaces,
        method_targets,
    )
}

pub(super) fn rewrite_analysis_namespace_calls(
    unit_path: &Path,
    analysis: &mut radix::driver::AnalyzedUnit,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    method_targets: &MethodCallTargets,
) -> Result<(), Vec<Diagnostic>> {
    // S1 U2 VALUE members (operator ruling O1): namespace enum-variant value
    // references (`module.VARIANT`) rewrite to the consumer variant `Path`
    // before lowering, riding the nominal remap. The consumer analysis
    // registered the imported enum variants (with their consumer defs) when
    // it installed the file interface.
    rewrite_namespace_variant_members(analysis);
    // FMIR e2e-hardening (CTO-1): method calls on linked library nominals
    // rewrite to synthetic-path calls. The library's method is a separate
    // definition in its own analysis, so the caller's `MethodCall` cannot
    // resolve through the caller's own genus table — the rewrite makes the
    // link explicit before lowering.
    if let Some(methods) = method_targets.get(unit_path) {
        if !methods.is_empty() {
            let mut rewriter = LibraryMethodCallRewriter {
                types: &analysis.types,
                interner: &analysis.interner,
                methods,
                diagnostics: Vec::new(),
            };
            radix::hir::visit::walk_program_mut(&mut rewriter, &mut analysis.hir);
            if !rewriter.diagnostics.is_empty() {
                return Err(rewriter
                    .diagnostics
                    .into_iter()
                    .map(|message| {
                        crate::package_diagnostic_error(message)
                            .with_file(unit_path.display().to_string())
                    })
                    .collect());
            }
        }
    }
    let mut diagnostics = Vec::new();
    // FMIR e2e-hardening (CTO-1): fix up namespace calls whose receiver is
    // keyed by a non-import `DefId` (an import alias shadowed by a local of
    // the same name can make the semantic namespace reference carry a
    // generated def instead of the import item's def).
    rewrite_shadowed_alias_namespace_calls(unit_path, analysis, targets, &mut diagnostics);
    if let Some(entry) = &mut analysis.hir.entry {
        rewrite_block(
            unit_path,
            entry,
            &analysis.interner,
            targets,
            data_member_targets,
            namespaces,
            &mut diagnostics,
        );
    }
    for item in &mut analysis.hir.items {
        if let HirItemKind::Function(function) = &mut item.kind {
            if let Some(body) = &mut function.body {
                rewrite_block(
                    unit_path,
                    body,
                    &analysis.interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    &mut diagnostics,
                );
            }
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics
            .into_iter()
            .map(|message| {
                crate::package_diagnostic_error(message).with_file(unit_path.display().to_string())
            })
            .collect())
    }
}

pub(super) fn rewrite_block(
    unit_path: &Path,
    block: &mut HirBlock,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    for stmt in &mut block.statements {
        rewrite_stmt(
            unit_path,
            stmt,
            interner,
            targets,
            data_member_targets,
            namespaces,
            diagnostics,
        );
    }
    if let Some(expr) = &mut block.expr {
        rewrite_expr(
            unit_path,
            expr,
            interner,
            targets,
            data_member_targets,
            namespaces,
            diagnostics,
        );
    }
}

pub(super) fn rewrite_stmt(
    unit_path: &Path,
    stmt: &mut HirStatement,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match &mut stmt.kind {
        HirStatementKind::Local(local) => {
            if let Some(init) = &mut local.init {
                rewrite_expr(
                    unit_path,
                    init,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirStatementKind::Expr(expr) => {
            rewrite_expr(
                unit_path,
                expr,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        HirStatementKind::Redde(Some(expr)) => {
            rewrite_expr(
                unit_path,
                expr,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            )
        }
        HirStatementKind::IncDec(inc_dec) => rewrite_expr(
            unit_path,
            &mut inc_dec.target,
            interner,
            targets,
            data_member_targets,
            namespaces,
            diagnostics,
        ),
        HirStatementKind::Custodi(custodi) => {
            for clause in &mut custodi.clauses {
                rewrite_expr(
                    unit_path,
                    &mut clause.cond,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
                rewrite_block(
                    unit_path,
                    &mut clause.body,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirStatementKind::Redde(None)
        | HirStatementKind::Rumpe
        | HirStatementKind::Perge
        | HirStatementKind::Tacet => {}
    }
}

pub(super) fn rewrite_expr(
    unit_path: &Path,
    expr: &mut HirExpression,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match &mut expr.kind {
        HirExpressionKind::Binary(_, lhs, rhs) | HirExpressionKind::Assign(lhs, rhs) => {
            rewrite_expr(unit_path, lhs, interner, targets, data_member_targets, namespaces, diagnostics);
            rewrite_expr(unit_path, rhs, interner, targets, data_member_targets, namespaces, diagnostics);
        }
        HirExpressionKind::ConversioAssign {
            target,
            source,
            recovery,
        } => {
            rewrite_expr(
                unit_path,
                target,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            rewrite_expr(
                unit_path,
                source,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            if let Some(recovery) = recovery {
                rewrite_expr(
                    unit_path,
                    recovery,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Unary(_, inner)
        | HirExpressionKind::Cede(inner)
        | HirExpressionKind::Reddet(inner)
        | HirExpressionKind::Tacebit(inner)
        | HirExpressionKind::Yield(inner)
        | HirExpressionKind::Panic(inner)
        | HirExpressionKind::Throw(inner)
        | HirExpressionKind::Praefixum(inner) => {
            rewrite_expr(unit_path, inner, interner, targets, data_member_targets, namespaces, diagnostics)
        }
        HirExpressionKind::Call(callee, type_args, args) => {
            rewrite_call_args(unit_path, args, interner, targets, data_member_targets, namespaces, diagnostics);
            if let HirExpressionKind::Field(receiver, method) = &callee.kind {
                if let Some(target_def) =
                    namespace_call_target(unit_path, receiver, *method, interner, targets)
                {
                    if !type_args.is_empty() {
                        diagnostics.push(
                            "package MIR does not support type arguments on namespace calls"
                                .to_owned(),
                        );
                        return;
                    }
                    let call_args = std::mem::take(args);
                    let callee = HirExpression {
                        id: receiver.id,
                        kind: HirExpressionKind::Path(target_def),
                        ty: None,
                        span: receiver.span,
                    };
                    expr.kind = HirExpressionKind::Call(Box::new(callee), Vec::new(), call_args);
                    return;
                }
                if let Some(message) =
                    namespace_call_diagnostic(unit_path, receiver, *method, interner, namespaces)
                {
                    diagnostics.push(message);
                    return;
                }
            }
            rewrite_expr(
                unit_path,
                callee,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        HirExpressionKind::MethodCall(receiver, method, type_args, args) => {
            rewrite_expr(
                unit_path,
                receiver,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            rewrite_call_args(unit_path, args, interner, targets, data_member_targets, namespaces, diagnostics);
            if let Some(target_def) =
                namespace_call_target(unit_path, receiver, *method, interner, targets)
            {
                if !type_args.is_empty() {
                    diagnostics.push(
                        "package MIR does not support type arguments on namespace calls".to_owned(),
                    );
                    return;
                }
                let call_args = std::mem::take(args);
                let callee = HirExpression {
                    id: receiver.id,
                    kind: HirExpressionKind::Path(target_def),
                    ty: None,
                    span: receiver.span,
                };
                expr.kind = HirExpressionKind::Call(Box::new(callee), Vec::new(), call_args);
            } else if let Some(message) =
                namespace_call_diagnostic(unit_path, receiver, *method, interner, namespaces)
            {
                diagnostics.push(message);
            }
        }
        HirExpressionKind::Field(object, field) => {
            // Const data members: `utilModule.VALUE` is a value reference
            // (`Field(Path(namespace), member)`), not a call. When the linker
            // registered a data-member target for the member, rewrite the
            // reference to the member's synthetic def so the entry lowering
            // materializes the const through the top-level-const seam.
            if let Some(target_def) =
                namespace_data_member_target(unit_path, object, *field, interner, data_member_targets)
            {
                expr.kind = HirExpressionKind::Path(target_def);
                expr.ty = None;
                return;
            }
            rewrite_expr(
                unit_path,
                object,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        HirExpressionKind::Index(object, index) => {
            rewrite_expr(
                unit_path,
                object,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            rewrite_expr(unit_path, index, interner, targets, data_member_targets, namespaces, diagnostics);
        }
        HirExpressionKind::OptionalChain(object, chain) => {
            rewrite_expr(
                unit_path,
                object,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            rewrite_optional_chain(unit_path, chain, interner, targets, data_member_targets, namespaces, diagnostics);
        }
        HirExpressionKind::NonNull(object, chain) => {
            rewrite_expr(
                unit_path,
                object,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            rewrite_non_null_chain(unit_path, chain, interner, targets, data_member_targets, namespaces, diagnostics);
        }
        HirExpressionKind::Block(block) | HirExpressionKind::Loop(block) => {
            rewrite_block(unit_path, block, interner, targets, data_member_targets, namespaces, diagnostics);
        }
        HirExpressionKind::Si {
            cond,
            then_block,
            then_catch,
            else_block,
        } => {
            rewrite_expr(unit_path, cond, interner, targets, data_member_targets, namespaces, diagnostics);
            rewrite_block(
                unit_path,
                then_block,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            if let Some(cape) = then_catch {
                rewrite_cape(unit_path, cape, interner, targets, data_member_targets, namespaces, diagnostics);
            }
            if let Some(block) = else_block {
                rewrite_block(unit_path, block, interner, targets, data_member_targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Discerne {
            scrutinees, arms, ..
        } => {
            for scrutinee in scrutinees {
                rewrite_expr(
                    unit_path,
                    scrutinee,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
            for arm in arms {
                rewrite_casu_arm(unit_path, arm, interner, targets, data_member_targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Dum(cond, block) => {
            rewrite_expr(unit_path, cond, interner, targets, data_member_targets, namespaces, diagnostics);
            rewrite_block(unit_path, block, interner, targets, data_member_targets, namespaces, diagnostics);
        }
        HirExpressionKind::Itera(_, _, _, iterable, block) => {
            rewrite_expr(
                unit_path,
                iterable,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            rewrite_block(unit_path, block, interner, targets, data_member_targets, namespaces, diagnostics);
        }
        HirExpressionKind::Intervallum {
            start, end, step, ..
        } => {
            rewrite_expr(unit_path, start, interner, targets, data_member_targets, namespaces, diagnostics);
            rewrite_expr(unit_path, end, interner, targets, data_member_targets, namespaces, diagnostics);
            if let Some(step) = step {
                rewrite_expr(unit_path, step, interner, targets, data_member_targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Array(elements) => {
            for element in elements {
                match element {
                    radix::hir::HirArrayElement::Expr(expr)
                    | radix::hir::HirArrayElement::Spread(expr) => {
                        rewrite_expr(
                            unit_path,
                            expr,
                            interner,
                            targets,
                            data_member_targets,
                            namespaces,
                            diagnostics,
                        );
                    }
                }
            }
        }
        HirExpressionKind::Struct(_, fields) => {
            for (_, value) in fields {
                rewrite_expr(unit_path, value, interner, targets, data_member_targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Tuple(items, _)
        | HirExpressionKind::Scribe(_, items)
        | HirExpressionKind::Scriptum(_, items) => {
            for item in items {
                rewrite_expr(unit_path, item, interner, targets, data_member_targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Adfirma(cond, message) => {
            rewrite_expr(unit_path, cond, interner, targets, data_member_targets, namespaces, diagnostics);
            if let Some(message) = message {
                rewrite_expr(
                    unit_path,
                    message,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Handled { body, catch } => {
            rewrite_block(unit_path, body, interner, targets, data_member_targets, namespaces, diagnostics);
            rewrite_cape(unit_path, catch, interner, targets, data_member_targets, namespaces, diagnostics);
        }
        HirExpressionKind::Clausura(_, _, _, body) => {
            rewrite_expr(unit_path, body, interner, targets, data_member_targets, namespaces, diagnostics)
        }
        HirExpressionKind::Verte {
            source, entries, ..
        } => {
            rewrite_expr(
                unit_path,
                source,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            if let Some(entries) = entries {
                for entry in entries {
                    rewrite_object_field(
                        unit_path,
                        entry,
                        interner,
                        targets,
                        data_member_targets,
                        namespaces,
                        diagnostics,
                    );
                }
            }
        }
        HirExpressionKind::Conversio {
            source, recovery, ..
        } => {
            rewrite_expr(
                unit_path,
                source,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            if let Some(recovery) = recovery {
                rewrite_expr(
                    unit_path,
                    recovery,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Ad { opener, .. } => {
            if let Some(opener) = opener {
                rewrite_expr(
                    unit_path,
                    opener,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Ref(_, inner) | HirExpressionKind::Deref(inner) => {
            rewrite_expr(unit_path, inner, interner, targets, data_member_targets, namespaces, diagnostics);
        }
        HirExpressionKind::TypeCheck { expr: inner, .. } => {
            rewrite_expr(unit_path, inner, interner, targets, data_member_targets, namespaces, diagnostics);
        }
        HirExpressionKind::Path(_)
        | HirExpressionKind::Literal(_)
        | HirExpressionKind::Vacua
        | HirExpressionKind::ReadLine(_)
        | HirExpressionKind::Error => {}
    }
}

pub(super) fn namespace_call_target(
    unit_path: &Path,
    receiver: &HirExpression,
    method: Symbol,
    interner: &Interner,
    targets: &NamespaceCallTargets,
) -> Option<DefId> {
    let HirExpressionKind::Path(namespace_def) = &receiver.kind else {
        return None;
    };
    let method_name = interner.resolve(method).to_owned();
    targets
        .get(&(unit_path.to_path_buf(), *namespace_def, method_name))
        .copied()
}

/// Resolve a linked const data-member reference (`Field(Path(namespace),
/// member)` value position) to its synthetic def id.
pub(super) fn namespace_data_member_target(
    unit_path: &Path,
    receiver: &HirExpression,
    member: Symbol,
    interner: &Interner,
    targets: &NamespaceDataMemberTargets,
) -> Option<DefId> {
    let HirExpressionKind::Path(namespace_def) = &receiver.kind else {
        return None;
    };
    let member_name = interner.resolve(member).to_owned();
    targets
        .get(&(unit_path.to_path_buf(), *namespace_def, member_name))
        .copied()
}

pub(super) fn namespace_call_diagnostic(
    unit_path: &Path,
    receiver: &HirExpression,
    method: radix::lexer::Symbol,
    interner: &Interner,
    namespaces: &NamespaceExports,
) -> Option<String> {
    let (namespace_def, mut fields) = namespace_receiver_path(receiver, interner)?;
    let exports = namespaces.get(&(unit_path.to_path_buf(), namespace_def))?;
    let method_name = interner.resolve(method).to_owned();
    if fields.is_empty() {
        if exports.contains(&method_name) {
            return Some(format!(
                "package MIR does not yet support non-function namespace member `{method_name}`"
            ));
        }
        return Some(format!("namespace does not export `{method_name}`"));
    }
    fields.push(method_name);
    let qualified = fields.join(".");
    Some(format!(
        "package MIR does not yet support nested namespace call `{qualified}`"
    ))
}

pub(super) fn namespace_receiver_path(
    expr: &HirExpression,
    interner: &Interner,
) -> Option<(DefId, Vec<String>)> {
    match &expr.kind {
        HirExpressionKind::Path(def_id) => Some((*def_id, Vec::new())),
        HirExpressionKind::Field(object, field) => {
            let (def_id, mut fields) = namespace_receiver_path(object, interner)?;
            fields.push(interner.resolve(*field).to_owned());
            Some((def_id, fields))
        }
        _ => None,
    }
}

/// Rewrite namespace enum-variant value references (`module.VARIANT`) into
/// `Path(consumer variant def)` before lowering (codex-gap S1 U2, operator
/// ruling O1 VALUE members).
///
/// The entry's HIR carries `Field(Path(namespace), variant)` for variant
/// value references (the resolve/lower path only special-cases enum variants
/// reached through a local enum identifier). The consumer analysis registered
/// the imported enum's variants — with their consumer `DefId`s — when it
/// installed the module's file interface, so the rewrite resolves the variant
/// through the analysis resolver and lowers through the entry's variant
/// aggregate seam.
fn rewrite_namespace_variant_members(analysis: &mut radix::driver::AnalyzedUnit) {
    struct VariantMemberRewriter<'a> {
        resolver: &'a Resolver,
    }

    impl HirVisitorMut for VariantMemberRewriter<'_> {
        fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
            if let HirExpressionKind::Field(object, field) = &expr.kind {
                if let HirExpressionKind::Path(namespace_def) = &object.kind {
                    if let Some(namespace) = self
                        .resolver
                        .get_symbol(*namespace_def)
                        .map(|symbol| symbol.name)
                    {
                        if let Some((variant_def, _ty)) =
                            self.resolver.namespace_file_variant(namespace, *field)
                        {
                            // The typechecker already assigned the variant's
                            // value type (`Type::Enum(consumer enum def)`) to
                            // the field expression; `lower_path` needs that
                            // type to construct the consumer variant, so the
                            // rewritten path keeps the expression's type.
                            expr.kind = HirExpressionKind::Path(variant_def);
                        }
                    }
                }
            }
            walk_expr_mut(self, expr);
        }
    }

    let mut rewriter = VariantMemberRewriter { resolver: &analysis.resolver };
    rewriter.visit_program_mut(&mut analysis.hir);
}

/// Rewrite namespace calls whose receiver `Path` carries a NON-import
/// `DefId` (FMIR e2e-hardening CTO-1).
///
/// When an import alias is shadowed by a local of the same name (gradus uses
/// `importa ex "gradus:shape" … forma` and function params named `forma`),
/// the semantic analysis can key the namespace reference by a generated def
/// instead of the import item's def; the main namespace-call rewrite then
/// misses the target and the call fails with "method call before
/// runtime/provider MIR lowering" at library lowering. This pass resolves the
/// receiver's symbol name back to the import binding and rewrites the call to
/// the linker's synthetic target.
fn rewrite_shadowed_alias_namespace_calls(
    unit_path: &Path,
    analysis: &mut radix::driver::AnalyzedUnit,
    targets: &NamespaceCallTargets,
    diagnostics: &mut Vec<String>,
) {
    // Import binding name → import item def id.
    let mut imports: HashMap<String, DefId> = HashMap::new();
    for item in &analysis.hir.items {
        let HirItemKind::Import(import) = &item.kind else {
            continue;
        };
        for import_item in &import.items {
            let binding = analysis
                .interner
                .resolve(import_item.alias.unwrap_or(import_item.name))
                .to_owned();
            imports.insert(binding, import_item.def_id);
        }
    }
    if imports.is_empty() {
        return;
    }
    struct ShadowedAliasRewriter<'a> {
        unit_path: &'a Path,
        resolver: &'a Resolver,
        interner: &'a Interner,
        targets: &'a NamespaceCallTargets,
        imports: &'a HashMap<String, DefId>,
        diagnostics: &'a mut Vec<String>,
    }
    impl ShadowedAliasRewriter<'_> {
        fn import_target(&self, receiver: &HirExpression, method: Symbol) -> Option<DefId> {
            let HirExpressionKind::Path(def) = &receiver.kind else {
                return None;
            };
            // The main rewrite already handles directly-keyed receivers.
            if namespace_call_target(self.unit_path, receiver, method, self.interner, self.targets)
                .is_some()
            {
                return None;
            }
            // A HIR-generated namespace def (import aliases whose semantic
            // namespace reference carries a generated def instead of the
            // import item's def — the `forma`-shadowing shape) is absent from
            // the symbol table, so resolve the METHOD NAME uniquely across the
            // unit's import bindings instead.
            if def.0 >= 1_000_000 {
                let method_name = self.interner.resolve(method).to_owned();
                let mut found = None;
                for import_def in self.imports.values() {
                    if let Some(target) =
                        self.targets.get(&(self.unit_path.to_path_buf(), *import_def, method_name.clone()))
                    {
                        if found.is_some() {
                            // Ambiguous: more than one import exports the
                            // method — leave the call to fail closed.
                            return None;
                        }
                        found = Some(*target);
                    }
                }
                return found;
            }
            // Only Module symbols (import namespaces) — or locals that shadow
            // one (the name-based resolution below then matches the import
            // binding, mirroring the analysis's namespace-member intent).
            let Some(symbol) = self.resolver.get_symbol(*def) else {
                return None;
            };
            if !matches!(
                symbol.kind,
                radix::semantic::SymbolKind::Module | radix::semantic::SymbolKind::Local
            ) {
                return None;
            }
            let name = self.interner.resolve(symbol.name).to_owned();
            let import_def = self.imports.get(&name).copied()?;
            let import_receiver = HirExpression {
                id: receiver.id,
                kind: HirExpressionKind::Path(import_def),
                ty: None,
                span: receiver.span,
            };
            namespace_call_target(self.unit_path, &import_receiver, method, self.interner, self.targets)
        }
    }
    impl HirVisitorMut for ShadowedAliasRewriter<'_> {
        fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
            match &mut expr.kind {
                HirExpressionKind::Call(callee, _, _) => {
                    if let HirExpressionKind::Field(receiver, method) = &callee.kind {
                        if let Some(synthetic) = self.import_target(receiver, *method) {
                            **callee = HirExpression {
                                id: receiver.id,
                                kind: HirExpressionKind::Path(synthetic),
                                ty: None,
                                span: receiver.span,
                            };
                        }
                    }
                }
                HirExpressionKind::MethodCall(receiver, method, type_args, args) => {
                    if let Some(synthetic) = self.import_target(receiver, *method) {
                        let call_args = std::mem::take(args);
                        let callee = HirExpression {
                            id: receiver.id,
                            kind: HirExpressionKind::Path(synthetic),
                            ty: None,
                            span: receiver.span,
                        };
                        *expr = HirExpression {
                            id: expr.id,
                            kind: HirExpressionKind::Call(
                                Box::new(callee),
                                std::mem::take(type_args),
                                call_args,
                            ),
                            ty: expr.ty,
                            span: expr.span,
                        };
                    }
                }
                _ => {}
            }
            walk_expr_mut(self, expr);
        }
    }
    let mut rewriter = ShadowedAliasRewriter {
        unit_path,
        resolver: &analysis.resolver,
        interner: &analysis.interner,
        targets,
        imports: &imports,
        diagnostics,
    };
    rewriter.visit_program_mut(&mut analysis.hir);
}

/// Rewrite method calls on linked library nominals into synthetic-path calls
/// (FMIR e2e-hardening CTO-1).
///
/// `receiver.method(args)` where `receiver`'s type is a linked library
/// nominal becomes `Path(synthetic)(receiver, args)`: the receiver becomes
/// the first positional argument (the library's method function takes the
/// receiver as its first parameter), and the synthetic def id was allocated
/// by [`link_library_method_targets`] for the library's method function, so
/// the merged program resolves the call through the shared source key.
struct LibraryMethodCallRewriter<'a> {
    types: &'a radix::semantic::TypeTable,
    interner: &'a Interner,
    methods: &'a HashMap<(DefId, String), DefId>,
    diagnostics: Vec<String>,
}

impl LibraryMethodCallRewriter<'_> {
    fn receiver_struct_def(&self, receiver: &HirExpression) -> Option<DefId> {
        let ty = receiver.ty?;
        match self.types.get(ty) {
            Type::Struct(def_id) => Some(*def_id),
            Type::Applied(base, _) => match self.types.get(*base) {
                Type::Struct(def_id) => Some(*def_id),
                _ => None,
            },
            _ => None,
        }
    }
}

impl HirVisitorMut for LibraryMethodCallRewriter<'_> {
    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        if let HirExpressionKind::MethodCall(receiver, method, type_args, args) = &mut expr.kind {
            if let Some(struct_def) = self.receiver_struct_def(receiver) {
                let method_name = self.interner.resolve(*method).to_owned();
                if let Some(synthetic) = self.methods.get(&(struct_def, method_name)).copied() {
                    let mut call_args = std::mem::take(args);
                    let mut new_args = Vec::with_capacity(call_args.len() + 1);
                    new_args.push(HirCallArg {
                        name: None,
                        spread: false,
                        expr: receiver.as_ref().clone(),
                        span: receiver.span,
                    });
                    new_args.append(&mut call_args);
                    let callee = HirExpression {
                        id: receiver.id,
                        kind: HirExpressionKind::Path(synthetic),
                        ty: None,
                        span: receiver.span,
                    };
                    *expr = HirExpression {
                        id: expr.id,
                        kind: HirExpressionKind::Call(
                            Box::new(callee),
                            std::mem::take(type_args),
                            new_args,
                        ),
                        ty: expr.ty,
                        span: expr.span,
                    };
                }
            }
        }
        walk_expr_mut(self, expr);
    }
}

pub(super) fn rewrite_call_args(
    unit_path: &Path,
    args: &mut [HirCallArg],
    interner: &Interner,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    for arg in args {
        rewrite_expr(
            unit_path,
            &mut arg.expr,
            interner,
            targets,
            data_member_targets,
            namespaces,
            diagnostics,
        );
    }
}

pub(super) fn rewrite_cape(
    unit_path: &Path,
    cape: &mut HirCape,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    rewrite_block(
        unit_path,
        &mut cape.body,
        interner,
        targets,
        data_member_targets,
        namespaces,
        diagnostics,
    );
}

pub(super) fn rewrite_casu_arm(
    unit_path: &Path,
    arm: &mut HirCasuArm,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    if let Some(guard) = &mut arm.guard {
        rewrite_expr(
            unit_path,
            guard,
            interner,
            targets,
            data_member_targets,
            namespaces,
            diagnostics,
        );
    }
    rewrite_expr(
        unit_path,
        &mut arm.body,
        interner,
        targets,
        data_member_targets,
        namespaces,
        diagnostics,
    );
}

pub(super) fn rewrite_object_field(
    unit_path: &Path,
    field: &mut HirObjectField,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match &mut field.key {
        radix::hir::HirObjectKey::Computed(key) | radix::hir::HirObjectKey::Spread(key) => {
            rewrite_expr(
                unit_path,
                key,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        radix::hir::HirObjectKey::Ident(_) | radix::hir::HirObjectKey::String(_) => {}
    }
    if let Some(value) = &mut field.value {
        rewrite_expr(
            unit_path,
            value,
            interner,
            targets,
            data_member_targets,
            namespaces,
            diagnostics,
        );
    }
}

pub(super) fn rewrite_optional_chain(
    unit_path: &Path,
    chain: &mut HirOptionalChainKind,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match chain {
        HirOptionalChainKind::Member(_) => {}
        HirOptionalChainKind::Index(index) => {
            rewrite_expr(
                unit_path,
                index,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            )
        }
        HirOptionalChainKind::Call(args) => {
            rewrite_call_args(
                unit_path,
                args,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            )
        }
    }
}

pub(super) fn rewrite_non_null_chain(
    unit_path: &Path,
    chain: &mut radix::hir::HirNonNullKind,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match chain {
        radix::hir::HirNonNullKind::Member(_) => {}
        radix::hir::HirNonNullKind::Index(index) => {
            rewrite_expr(
                unit_path,
                index,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            )
        }
        radix::hir::HirNonNullKind::Call(args) => {
            rewrite_call_args(
                unit_path,
                args,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            )
        }
    }
}
