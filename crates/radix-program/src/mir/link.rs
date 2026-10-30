//! Package-local namespace-call linking and rewrite passes.

use super::{
    AnalyzedProgram, AnalyzedProgramNode, BTreeMap, BTreeSet, DataMemberLink, DefId, Diagnostic,
    HashMap, HashSet, HirBlock, HirCallArg, HirCape, HirCasuArm, HirConst, HirExpression,
    HirExpressionKind, HirItemKind, HirObjectField, HirOptionalChainKind, HirStatement,
    HirStatementKind, ImportResolution, Interner, LibraryImportBinding, LibraryInterfaceCache,
    LibraryLinkTarget, LibraryResolver, MethodCallTargets, NamespaceCallTargets,
    NamespaceDataMemberTargets, NamespaceExports, PACKAGE_MIR_SYNTHETIC_DEF_BASE,
    PackageMirConsumer, PackageMirLinks, Path, PathBuf, Primitive, Resolver, SourceRewrites,
    Symbol, Type, TypeId, TypeTable, library_cached_analysis, library_cached_file_interface,
    resolve_import,
};
use radix_module::hir::visit::{
    HirVisitor, HirVisitorMut, walk_expr, walk_expr_mut, walk_program_mut,
};

/// Post-rewrite pass: recover the function type for synthetic-path call
/// callees (pkg001-mir-fix).
///
/// `rewrite_analysis_namespace_calls` replaces a typed namespace call
/// (`forma.quantitas(f)`) with `Call(Path(synthetic), …)` whose callee
/// carries no type. The radix MIR lowerer keys failable-ness by the callee
/// def's registered error type (`function_errors`); a synthetic def allocated
/// after analysis is absent there, so a FAILABLE library call would otherwise
/// lower as a plain `Call` — no try-call edge to the `fac/cape` handler — and
/// the handler's blocks become orphaned ("block bbN is not reachable from
/// entry", PKG001). Recovering the callee's function type (with its error
/// half) lets the lowerer fall back to the callee type and emit the try-call.
struct SyntheticCalleeTypePass<'a> {
    method_types: &'a HashMap<radix_module::hir::DefId, radix_types::TypeId>,
}

impl HirVisitorMut for SyntheticCalleeTypePass<'_> {
    fn visit_expr_mut(&mut self, expr: &mut radix_module::hir::HirExpression) {
        if let HirExpressionKind::Call(callee, ..) = &mut expr.kind
            && let HirExpressionKind::Path(def_id) = callee.kind
            && let Some(function_ty) = self.method_types.get(&def_id).copied()
        {
            callee.ty = Some(function_ty);
        }
        walk_expr_mut(self, expr);
    }
}

/// Link a bare call of a selectively imported function to the synthetic
/// function a `module.f(x)` call reaches.
///
/// A selective member (`import from "lib:mod" const f`) is a caller-owned value
/// local with no namespace receiver, so the namespace-call rewrite never keys
/// on `f(x)`. The linker registers every export under each import item def
/// (`links.targets`), so the member's own item def plus its exported name
/// already names the synthetic target; only the callee needs the swap. The
/// callee keeps the interface-supplied function type the typechecker gave the
/// local, so a failable call keeps its try-call edge.
struct SelectiveMemberCallRewriter<'a> {
    synthetic: &'a HashMap<DefId, DefId>,
}

impl HirVisitorMut for SelectiveMemberCallRewriter<'_> {
    fn visit_expr_mut(&mut self, expr: &mut radix_module::hir::HirExpression) {
        if let HirExpressionKind::Call(callee, ..) = &mut expr.kind
            && let HirExpressionKind::Path(def_id) = &mut callee.kind
            && let Some(target) = self.synthetic.get(def_id).copied()
        {
            *def_id = target;
        }
        walk_expr_mut(self, expr);
    }
}

/// The synthetic function each selectively imported member of `analysis`
/// links to, keyed by the member's import item def. Members the linker
/// registered no function target for (consts, types) stay absent.
fn selective_member_targets(
    unit_path: &Path,
    analysis: &radix_module::driver::AnalyzedModule,
    targets: &NamespaceCallTargets,
) -> HashMap<DefId, DefId> {
    let mut members = HashMap::new();
    for item in &analysis.hir.items {
        let HirItemKind::Import(import) = &item.kind else {
            continue;
        };
        if !import.selective {
            continue;
        }
        for member in &import.items {
            let name = analysis.interner.resolve(member.name).to_owned();
            if let Some(target) = targets.get(&(unit_path.to_path_buf(), member.def_id, name)) {
                members.insert(member.def_id, *target);
            }
        }
    }
    members
}

/// Link a bare read of a selectively imported const member to the synthetic
/// const a `module.K` read reaches.
///
/// A selective member (`import from "lib:mod" const K`) is a caller-owned
/// value local with no namespace receiver, so the namespace data-member
/// rewrite never keys on the bare read `K`. The linker registered the exported
/// const under the member's own import item def in `data_member_targets`; the
/// read's `Path` def only needs the swap to the synthetic const so the
/// transplant seam materializes its value.
struct SelectiveMemberConstRewriter<'a> {
    synthetic: &'a HashMap<DefId, DefId>,
}

impl HirVisitorMut for SelectiveMemberConstRewriter<'_> {
    fn visit_expr_mut(&mut self, expr: &mut radix_module::hir::HirExpression) {
        if let HirExpressionKind::Path(def_id) = &mut expr.kind
            && let Some(target) = self.synthetic.get(def_id).copied()
        {
            *def_id = target;
        }
        walk_expr_mut(self, expr);
    }
}

/// The synthetic const each selectively imported member of `analysis` links
/// to, keyed by the member's import item def. Function members are absent
/// (they link through `NamespaceCallTargets`), so function-value references
/// are untouched.
fn selective_member_const_targets(
    unit_path: &Path,
    analysis: &radix_module::driver::AnalyzedModule,
    data_member_targets: &NamespaceDataMemberTargets,
) -> HashMap<DefId, DefId> {
    let mut members = HashMap::new();
    for item in &analysis.hir.items {
        let HirItemKind::Import(import) = &item.kind else {
            continue;
        };
        if !import.selective {
            continue;
        }
        for member in &import.items {
            let name = analysis.interner.resolve(member.name).to_owned();
            if let Some(target) =
                data_member_targets.get(&(unit_path.to_path_buf(), member.def_id, name))
            {
                members.insert(member.def_id, *target);
            }
        }
    }
    members
}

/// Post-rewrite pass: recover a cape handler binding whose error type was
/// typed before the shadowed-alias namespace rewrite resolved its body.
///
/// `rewrite_analysis_namespace_calls` resolves a shadowed namespace call
/// (`forma.quantitas(forma)` where the local param `forma` shadows the
/// import binding) only AFTER type checking. The cape's binding was therefore
/// fixed from the unresolved error sink — `ignotum` (`check_cape`'s fallback)
/// — and the merged program's strict MIR validation rejects passing that
/// value to a concrete enum/struct param. The recovery walks each `Handled`
/// expression and re-derives the binding from the body's failable synthetic
/// calls: when every such call agrees on one error type, the handler receives
/// that type — exactly what the analysis would have produced had the receiver
/// resolved. Bindings that are already concrete, or bodies whose failable
/// calls disagree, are left untouched (fail-closed, no invented types).
struct SyntheticCapeBindingPass<'a> {
    method_types: &'a HashMap<radix_module::hir::DefId, radix_types::TypeId>,
    types: &'a TypeTable,
}

impl HirVisitorMut for SyntheticCapeBindingPass<'_> {
    fn visit_expr_mut(&mut self, expr: &mut radix_module::hir::HirExpression) {
        if let HirExpressionKind::Handled { body, catch } = &mut expr.kind {
            let unresolved = matches!(
                catch.binding_ty.map(|ty| self.types.get(ty)),
                Some(Type::Primitive(Primitive::Ignotum)) | None
            );
            if unresolved {
                let mut collector = CapeFailableCollector {
                    method_types: self.method_types,
                    types: self.types,
                    agreed: None,
                    has_failable: false,
                    ambiguous: false,
                };
                collector.visit_block(body);
                if collector.has_failable
                    && !collector.ambiguous
                    && let Some(error_ty) = collector.agreed
                {
                    catch.binding_ty = Some(error_ty);
                }
            }
        }
        walk_expr_mut(self, expr);
    }
}

/// Read-only collector over one handled body: the distinct error types of
/// the failable synthetic namespace calls it contains.
struct CapeFailableCollector<'a> {
    method_types: &'a HashMap<radix_module::hir::DefId, radix_types::TypeId>,
    types: &'a TypeTable,
    agreed: Option<TypeId>,
    has_failable: bool,
    ambiguous: bool,
}

impl HirVisitor for CapeFailableCollector<'_> {
    fn visit_expr(&mut self, expr: &radix_module::hir::HirExpression) {
        if let HirExpressionKind::Call(callee, ..) = &expr.kind
            && let Some(error_ty) = self.synthetic_callee_error_ty(callee)
        {
            self.has_failable = true;
            match self.agreed {
                None => self.agreed = Some(error_ty),
                Some(existing) if existing != error_ty => self.ambiguous = true,
                _ => {}
            }
        }
        walk_expr(self, expr);
    }
}

impl CapeFailableCollector<'_> {
    /// The error type carried by a synthetic namespace callee, when failable.
    fn synthetic_callee_error_ty(
        &self,
        callee: &radix_module::hir::HirExpression,
    ) -> Option<TypeId> {
        let HirExpressionKind::Path(def_id) = callee.kind else {
            return None;
        };
        let function_ty = self.method_types.get(&def_id).copied()?;
        let Type::Func(sig) = self.types.get(function_ty) else {
            return None;
        };
        sig.err
    }
}

/// Collect the function type for every synthetic namespace-call target the
/// caller can reach: the caller's file-interface resolver remaps the linked
/// library's export signatures into the caller's type table, and the
/// `targets` table maps (import item, verb) to the synthetic def the rewrite
/// installed. Generic exports are included: the rewrite forwards call-site
/// type arguments onto the rewritten call, and the recovered type's error
/// half is what keeps the try-call edge for failable generics.
fn synthetic_namespace_function_types(
    unit_path: &Path,
    analysis: &mut radix_module::driver::AnalyzedModule,
    targets: &NamespaceCallTargets,
) -> HashMap<radix_module::hir::DefId, radix_types::TypeId> {
    let mut method_types = HashMap::new();
    for import in analysis
        .hir
        .items
        .iter()
        .filter_map(|item| match &item.kind {
            HirItemKind::Import(import) => Some(import),
            _ => None,
        })
    {
        for import_item in &import.items {
            let binding = import_item.alias.unwrap_or(import_item.name);
            let exports = analysis.resolver.namespace_file_function_exports(binding);
            if exports.is_empty() {
                continue;
            }
            for (verb, sig) in exports {
                let verb_name = analysis.interner.resolve(verb);
                let Some(synthetic) = targets
                    .get(&(
                        unit_path.to_path_buf(),
                        import_item.def_id,
                        verb_name.to_owned(),
                    ))
                    .copied()
                else {
                    continue;
                };
                let function_ty = analysis.types.function(sig.clone());
                method_types.insert(synthetic, function_ty);
            }
        }
    }
    method_types
}

/// Collect the function type for every synthetic method-call target the
/// caller can reach (CG2-R08): the caller's resolver carries the linked
/// library's struct methods remapped into the caller's type table (canonical
/// name + consumer-locale surface name + remapped signature), and the
/// `methods` map keys them by `(receiver nominal struct def, method name)`.
///
/// The recovered callee type — with its error half — lets the radix MIR
/// lowerer emit the try-call for a failable method; a synthetic callee
/// without a type would otherwise lower as an ordinary call and orphan the
/// cape handler's blocks ("block bbN is not reachable from entry", PKG001).
/// Targets whose signature cannot be recovered stay absent and the
/// [`LibraryMethodCallRewriter`] fails closed at the nominal-method site
/// instead of installing a silent `ty: None` callee.
fn synthetic_method_function_types(
    analysis: &mut radix_module::driver::AnalyzedModule,
    methods: &HashMap<(DefId, String), DefId>,
) -> HashMap<(DefId, String), radix_types::TypeId> {
    let mut function_types = HashMap::new();
    // Group the caller's imported struct methods by receiver nominal def.
    let mut methods_by_receiver: HashMap<DefId, Vec<_>> = HashMap::new();
    for (def_id, _fields, struct_methods, _type_params) in analysis.resolver.imported_file_structs()
    {
        methods_by_receiver.insert(def_id, struct_methods);
    }
    // Each iteration interns a function type into the caller's table, so the
    // visit order fixes the emitted type ids: never walk the std `HashMap`
    // (per-process random order) directly.
    for (receiver_def, method_name) in ordered_method_keys(methods) {
        let Some(struct_methods) = methods_by_receiver.get(receiver_def) else {
            continue;
        };
        let Some(method) = struct_methods.iter().find(|m| {
            analysis.interner.resolve(m.name) == method_name
                || analysis.interner.resolve(m.surface_name) == method_name
        }) else {
            continue;
        };
        let function_ty = analysis.types.function(method.sig.clone());
        function_types.insert((*receiver_def, method_name.clone()), function_ty);
    }
    function_types
}

/// The synthetic method-call keys in a stable `(receiver def, method name)`
/// order, independent of the map's hash seed.
pub(super) fn ordered_method_keys(
    methods: &HashMap<(DefId, String), DefId>,
) -> Vec<&(DefId, String)> {
    let mut keys: Vec<&(DefId, String)> = methods.keys().collect();
    keys.sort_by(|a, b| (a.0.0, &a.1).cmp(&(b.0.0, &b.1)));
    keys
}

struct PackageMirLinkAccumulator {
    targets: NamespaceCallTargets,
    data_member_targets: NamespaceDataMemberTargets,
    data_members: Vec<DataMemberLink>,
    namespaces: NamespaceExports,
    source_rewrites: SourceRewrites,
    imported_function_params: HashMap<DefId, ImportedFunctionParams>,
    next_synthetic: u32,
    diagnostics: Vec<Diagnostic>,
    libraries: Vec<LibraryLinkTarget>,
    method_targets: MethodCallTargets,
    /// Imported-device routes joined to their synthetic template source
    /// `DefId` (KPC-WIRE): one entry per device-bearing library function,
    /// carried to package lowering for the device-aware instantiation index.
    imported_device_routes: Vec<(DefId, radix_mir_lower::lower::ImportedDeviceRoute)>,
}

impl Default for PackageMirLinkAccumulator {
    fn default() -> Self {
        Self {
            targets: HashMap::new(),
            data_member_targets: HashMap::new(),
            data_members: Vec::new(),
            namespaces: HashMap::new(),
            source_rewrites: HashMap::new(),
            imported_function_params: HashMap::new(),
            next_synthetic: PACKAGE_MIR_SYNTHETIC_DEF_BASE,
            diagnostics: Vec::new(),
            libraries: Vec::new(),
            method_targets: HashMap::new(),
            imported_device_routes: Vec::new(),
        }
    }
}

struct LibraryImportSite {
    caller_path: PathBuf,
    import_path: String,
    visibility: radix_module::syntax::Visibility,
    items: Vec<LibraryImportSiteItem>,
}

struct LibraryImportSiteItem {
    def_id: DefId,
    binding: String,
}

pub(super) fn local_namespace_call_targets(
    package: &AnalyzedProgram,
    consumer: PackageMirConsumer,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    loaded_links: Option<&BTreeMap<PathBuf, BTreeMap<String, PathBuf>>>,
) -> Result<PackageMirLinks, Vec<Diagnostic>> {
    let mut links = PackageMirLinkAccumulator::default();
    link_package_unit_imports(
        &mut links,
        package,
        consumer,
        library_resolver,
        library_cache,
        loaded_links,
    );

    if consumer == PackageMirConsumer::Interpreted {
        link_nested_library_imports(&mut links, library_resolver, library_cache);
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
        air_calls: HashMap::new(),
        data_member_targets: links.data_member_targets,
        data_members: links.data_members,
        namespaces: links.namespaces,
        sources: links.source_rewrites,
        imported_function_params: links.imported_function_params,
        next_synthetic: links.next_synthetic,
        libraries: links.libraries,
        method_targets: links.method_targets,
        imported_device_routes: links.imported_device_routes,
    })
}

fn link_package_unit_imports(
    links: &mut PackageMirLinkAccumulator,
    package: &AnalyzedProgram,
    consumer: PackageMirConsumer,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    loaded_links: Option<&BTreeMap<PathBuf, BTreeMap<String, PathBuf>>>,
) {
    let units_by_path = package
        .nodes
        .values()
        .map(|unit| (unit.path.clone(), unit))
        .collect::<BTreeMap<_, _>>();
    // Sibling-route product identity: the package root directory name, the
    // same derivation the package-identity computation applies.
    let sibling_product = package
        .spec
        .package_root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();

    for unit in package.nodes.values() {
        for item in &unit.analysis.hir.items {
            let HirItemKind::Import(import) = &item.kind else {
                continue;
            };
            if let Some(loaded_links) = loaded_links {
                // Loaded-package path: local imports resolve from the explicit
                // link table (import binding → target module path) instead of
                // the filesystem. Library imports are not linked here — the
                // store owns library closure (Stage 5 of the package delivery).
                link_loaded_import_items(links, unit, import, loaded_links, &units_by_path);
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
                    link_sibling_import_items(links, unit, import, sibling, &sibling_product);
                }
                ImportResolution::Library(module) => {
                    // A `provider:module` specifier that resolves to a file
                    // already in this package is a self-import, not an
                    // external library. Keep the sibling-unit link so package
                    // tests execute the in-tree bodies instead of leaving
                    // members as package providers.
                    if let Some(sibling) = units_by_path
                        .get(&crate::paths::normalize_path(&module.interface_path))
                        .copied()
                    {
                        link_sibling_import_items(links, unit, import, sibling, &sibling_product);
                        continue;
                    }
                    // Only the interpreted run path links library imports.
                    // External-target builds keep the previous behavior:
                    // library calls stay provider-backed and emit as-is.
                    if consumer != PackageMirConsumer::Interpreted {
                        continue;
                    }
                    link_library_import_module(
                        links,
                        unit,
                        import,
                        import_path,
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
                ImportResolution::Unsupported => {}
            }
        }
    }
}

fn link_loaded_import_items(
    links: &mut PackageMirLinkAccumulator,
    unit: &AnalyzedProgramNode,
    import: &radix_module::hir::HirImport,
    loaded_links: &BTreeMap<PathBuf, BTreeMap<String, PathBuf>>,
    units_by_path: &BTreeMap<PathBuf, &AnalyzedProgramNode>,
) {
    let Some(unit_links) = loaded_links.get(&unit.path) else {
        return;
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
            .into_keys()
            .collect::<BTreeSet<_>>();
        links
            .namespaces
            .insert((unit.path.clone(), import_item.def_id), exports.clone());
        // D5.1: allocate every function source; module-private
        // helpers are reached from exported functions of the
        // same module. Only exports become call targets.
        for function in top_level_functions(sibling) {
            let synthetic = *links
                .source_rewrites
                .entry((sibling.path.clone(), function.def_id))
                .or_insert_with(|| {
                    let def_id = DefId(links.next_synthetic);
                    links.next_synthetic += 1;
                    def_id
                });
            if exports.contains(&function.name) {
                links.targets.insert(
                    (unit.path.clone(), import_item.def_id, function.name),
                    synthetic,
                );
            }
        }
        for member in top_level_consts(sibling) {
            let exported = exports.contains(&member.name);
            link_const_member(
                links,
                &unit.path,
                import_item.def_id,
                &sibling.path,
                &member,
                exported,
            );
        }
    }
}

fn link_library_import_module(
    links: &mut PackageMirLinkAccumulator,
    unit: &AnalyzedProgramNode,
    import: &radix_module::hir::HirImport,
    import_path: &str,
    module: &crate::library::ResolvedLibraryModule,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) {
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
    link_library_import_site(links, &site, module, library_resolver, library_cache);
}

fn link_sibling_import_items(
    links: &mut PackageMirLinkAccumulator,
    unit: &AnalyzedProgramNode,
    import: &radix_module::hir::HirImport,
    sibling: &AnalyzedProgramNode,
    product: &str,
) {
    for import_item in &import.items {
        // Derive the caller's exports under the item's effective binding; each
        // target is keyed by `(unit, import item def, member)`. A selective
        // import binds no namespace, so its surface is keyed by the import
        // path (`import_binding` in analyze.rs), not by the member's name.
        let binding = if import.selective {
            unit.analysis.interner.resolve(import.path)
        } else {
            unit.analysis
                .interner
                .resolve(import_item.alias.unwrap_or(import_item.name))
        };
        let exports = unit
            .namespace_exports
            .get(binding)
            .cloned()
            .unwrap_or_default()
            .into_keys()
            .collect::<BTreeSet<_>>();
        link_sibling_import_item(
            links,
            &unit.path,
            import_item.def_id,
            sibling,
            &exports,
            product,
        );
    }
}

fn link_sibling_import_item(
    links: &mut PackageMirLinkAccumulator,
    caller_path: &Path,
    import_def_id: DefId,
    sibling: &AnalyzedProgramNode,
    exports: &BTreeSet<String>,
    product: &str,
) {
    links
        .namespaces
        .insert((caller_path.to_path_buf(), import_def_id), exports.clone());
    // SGR-U0: a sibling device export is an imported-device route for every
    // consumer unit of the package. The body is package-owned source, so the
    // route is composable by construction; without the route the consumer's
    // generic instantiation registers no device entry (the spike-3 gap) and
    // the target seam sees a zero-kernel package.
    // D5.1: allocate every function source, not only exports — module-private
    // helpers are reached from exported functions of the same module.
    for function in top_level_functions(sibling) {
        let synthetic = *links
            .source_rewrites
            .entry((sibling.path.clone(), function.def_id))
            .or_insert_with(|| {
                let def_id = DefId(links.next_synthetic);
                links.next_synthetic += 1;
                def_id
            });
        links
            .imported_function_params
            .entry(synthetic)
            .or_insert_with(|| ImportedFunctionParams {
                source_path: sibling.path.clone(),
                params: function.params.clone(),
            });
        if let Some(contract) =
            sibling
                .file_interface
                .exports
                .get(&function.name)
                .and_then(|export| match &export.kind {
                    radix_module::file_interface::FileExportKind::Function(callable) => {
                        callable.device.clone()
                    }
                    _ => None,
                })
        {
            links.imported_device_routes.push((
                synthetic,
                radix_mir_lower::lower::ImportedDeviceRoute {
                    identity: radix_mir::MirExternalFunctionIdentity::new(
                        product.to_owned(),
                        sibling.module_segments.clone(),
                        function.name.clone(),
                    ),
                    contract,
                },
            ));
        }
        if exports.contains(&function.name) {
            links.targets.insert(
                (caller_path.to_path_buf(), import_def_id, function.name),
                synthetic,
            );
        }
    }
    for member in top_level_consts(sibling) {
        let exported = exports.contains(&member.name);
        link_const_member(
            links,
            caller_path,
            import_def_id,
            &sibling.path,
            &member,
            exported,
        );
    }
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
        import_span: radix_module::lexer::Span::default(),
        module: module.clone(),
    };
    let Ok(interface) = library_cached_file_interface(&import, library_resolver, library_cache)
    else {
        push_library_import_unsupported(
            &mut links.diagnostics,
            &site.caller_path,
            &site.import_path,
            PackageMirConsumer::Interpreted,
        );
        return;
    };
    // One cached analysis feeds both the function and const tables; a second
    // query would be a pure cache hit with the same fail-closed branch.
    let Ok(analysis) = library_cached_analysis(&import, library_resolver, library_cache) else {
        push_library_import_unsupported(
            &mut links.diagnostics,
            &site.caller_path,
            &site.import_path,
            PackageMirConsumer::Interpreted,
        );
        return;
    };
    let functions = collect_library_functions(links, site, analysis);
    let consts = collect_library_consts(analysis);
    let exports = interface.exports.keys().cloned().collect::<BTreeSet<_>>();
    let library_path = module.interface_path.clone();

    for item in &site.items {
        links
            .namespaces
            .insert((site.caller_path.clone(), item.def_id), exports.clone());
        // Allocate every function source, not only public call targets. Private
        // functions can be reached from an exported function in the same module.
        // Route→synthetic join (KPC-WIRE done_when (c)): every device-bearing
        // function of the linked library becomes an imported-device route
        // keyed by the synthetic template source the package rewrite will
        // give that function's generic template. Joining here — where both
        // the interface (role + body channel) and the allocated synthetic
        // live — is caller-independent, so one route serves every consumer.
        for function in &functions {
            let synthetic = *links
                .source_rewrites
                .entry((library_path.clone(), function.def_id))
                .or_insert_with(|| {
                    let def_id = DefId(links.next_synthetic);
                    links.next_synthetic += 1;
                    def_id
                });
            // Only a function that actually carries parameter metadata gets an
            // entry. Compiler-generated companions have no HIR item and so no
            // params; an empty entry is not "zero-parameter function", it is
            // "nothing to transplant", and treating it as authoritative would
            // drop every provided argument at the call site.
            if !function.params.is_empty() {
                links
                    .imported_function_params
                    .entry(synthetic)
                    .or_insert_with(|| ImportedFunctionParams {
                        source_path: library_path.clone(),
                        params: function.params.clone(),
                    });
            }
            if let Some(contract) =
                interface
                    .exports
                    .get(&function.name)
                    .and_then(|export| match &export.kind {
                        radix_module::file_interface::FileExportKind::Function(callable) => {
                            callable.device.clone()
                        }
                        _ => None,
                    })
            {
                links.imported_device_routes.push((
                    synthetic,
                    radix_mir_lower::lower::ImportedDeviceRoute {
                        identity: radix_mir::MirExternalFunctionIdentity::new(
                            module.package.clone(),
                            module.module_path.clone(),
                            function.name.clone(),
                        ),
                        contract,
                    },
                ));
            }
            if exports.contains(&function.name) {
                links.targets.insert(
                    (site.caller_path.clone(), item.def_id, function.name.clone()),
                    synthetic,
                );
            }
        }
        // D5.1: module-private consts are linked too — an exported const
        // initializer or function body may reference them — but only exports
        // become member-reference targets.
        for member in &consts {
            let exported = exports.contains(&member.name);
            link_const_member(
                links,
                &site.caller_path,
                item.def_id,
                &library_path,
                member,
                exported,
            );
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

fn collect_library_functions(
    links: &mut PackageMirLinkAccumulator,
    site: &LibraryImportSite,
    analysis: &radix_module::driver::AnalyzedModule,
) -> Vec<TopLevelFunction> {
    let mut functions = analysis
        .hir
        .items
        .iter()
        .filter_map(|item| {
            let HirItemKind::Function(function) = &item.kind else {
                return None;
            };
            Some(TopLevelFunction {
                name: analysis.interner.resolve(function.name).to_owned(),
                def_id: item.def_id,
                params: function
                    .params
                    .iter()
                    .map(|param| ImportedParamMeta {
                        optional: param.optional,
                        default: param.default.clone(),
                    })
                    .collect(),
            })
        })
        .collect::<Vec<_>>();
    // Compiler-generated companions have no HIR item, but their source
    // DefId is the identity carried by their generated MIR function.
    // A declared companion with no resolvable symbol fails closed
    // here (stable `package_mir_companion_unresolved` identity)
    // instead of silently vanishing from the link and surfacing later
    // as an unstable runner-internal error.
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
        functions.push(TopLevelFunction {
            name,
            def_id: backward.companion_def_id,
            params: Vec::new(),
        });
    }
    functions
}

fn collect_library_consts(analysis: &radix_module::driver::AnalyzedModule) -> Vec<TopLevelConst> {
    analysis
        .hir
        .items
        .iter()
        .filter_map(|item| {
            let HirItemKind::Constant(konst) = &item.kind else {
                return None;
            };
            Some(TopLevelConst {
                name: analysis.interner.resolve(konst.name).to_owned(),
                def_id: item.def_id,
                konst: konst.clone(),
            })
        })
        .collect::<Vec<_>>()
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
/// runner-internal `missing MIR function` error.
fn companion_resolves(
    analysis: &radix_module::driver::AnalyzedModule,
    primal_def_id: DefId,
    backward: &radix_module::semantic::RadixBackward,
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
    function.params.iter().any(|param| {
        radix_module::semantic::tensor_type_directed::is_tensor_type(param.ty, &analysis.types)
    })
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
        let Ok(sites) = nested_library_import_sites(&library, library_resolver, library_cache)
        else {
            push_library_import_unsupported(
                &mut links.diagnostics,
                &library.module.interface_path,
                &library.module.package,
                PackageMirConsumer::Interpreted,
            );
            continue;
        };
        for site in sites {
            match library_resolver.resolve(&site.import_path) {
                Ok(Some(module)) => {
                    link_library_import_site(
                        links,
                        &site,
                        &module,
                        library_resolver,
                        library_cache,
                    );
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

/// Link genus-method call targets for methods on linked source nominals
/// (FMIR e2e-hardening CTO-1).
///
/// Every linked source module's public struct methods get a package-MIR
/// synthetic source (same discipline as top-level functions, so cross-unit
/// def-id collisions stay impossible and S1 U3 reachability prunes unused
/// methods), and each method call is registered per caller: key = the
/// caller's entry-side nominal `DefId` for the source struct (the receiver
/// type every caller analysis carries after the canonical nominal import) +
/// method name → the synthetic. The rewrite pass then lowers
/// `receiver.method(args)` to `Path(synthetic)(receiver, args)`, which the
/// merged program resolves to the source method function.
fn link_library_method_targets(
    links: &mut PackageMirLinkAccumulator,
    package: &AnalyzedProgram,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) {
    // Phase 1: extract source-module struct → method tables. Local sibling
    // units are already in `package`; library units come from the cache. The
    // cache borrows are released after each extraction so the caller pass
    // below can re-borrow each library analysis independently.
    struct SourceStruct {
        source_path: PathBuf,
        identity: Option<radix_module::file_interface::InterfaceLibraryIdentity>,
        /// (struct export name, method name, method def id)
        methods: Vec<(String, String, DefId)>,
    }
    let collect_methods =
        |interface: &radix_module::file_interface::FileInterface,
         analysis: &radix_module::driver::AnalyzedModule| {
            let mut methods = Vec::new();
            for export in interface.exports.values() {
                let radix_module::file_interface::FileExportKind::Struct(struct_export) =
                    &export.kind
                else {
                    continue;
                };
                // Find the source struct item (methods carry the def ids).
                let Some(strukt) = analysis.hir.items.iter().find_map(|item| {
                    let HirItemKind::Struct(strukt) = &item.kind else {
                        return None;
                    };
                    (analysis.interner.resolve(strukt.name) == struct_export.name).then_some(strukt)
                }) else {
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
            methods
        };
    let mut sources: Vec<SourceStruct> = Vec::new();
    for unit in package.nodes.values() {
        sources.push(SourceStruct {
            source_path: unit.path.clone(),
            identity: unit.file_interface.identity.clone(),
            methods: collect_methods(&unit.file_interface, &unit.analysis),
        });
    }
    for library in &links.libraries {
        let Ok(interface) =
            library_cached_file_interface(&library.import, library_resolver, library_cache)
        else {
            continue;
        };
        let Ok(analysis) =
            library_cached_analysis(&library.import, library_resolver, library_cache)
        else {
            continue;
        };
        sources.push(SourceStruct {
            source_path: library.path.clone(),
            identity: interface.identity.clone(),
            methods: collect_methods(&interface, analysis),
        });
    }

    // Phase 2: per caller (package units, then linked libraries — a library
    // can call methods on another linked source's nominals), register the
    // method targets that resolve through the caller's canonical nominal
    // table.
    let register_caller = |links: &mut PackageMirLinkAccumulator,
                           caller_path: &Path,
                           resolver: &radix_module::semantic::Resolver,
                           interner: &Interner| {
        for source in &sources {
            if source.source_path == caller_path {
                continue;
            }
            let Some(identity) = source.identity.as_ref() else {
                continue;
            };
            for (struct_name, method_name, method_def_id) in &source.methods {
                let Some(entry_nominal_def) = resolver.imported_nominal_def_id_by_name(
                    radix_module::file_interface::InterfaceNominalKind::Struct,
                    Some(identity),
                    struct_name,
                    interner,
                ) else {
                    continue;
                };
                let synthetic = *links
                    .source_rewrites
                    .entry((source.source_path.clone(), *method_def_id))
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

    for unit in package.nodes.values() {
        register_caller(
            links,
            &unit.path,
            &unit.analysis.resolver,
            &unit.analysis.interner,
        );
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

pub(super) struct TopLevelFunction {
    name: String,
    def_id: DefId,
    params: Vec<ImportedParamMeta>,
}

pub(super) fn top_level_functions(unit: &AnalyzedProgramNode) -> Vec<TopLevelFunction> {
    unit.analysis
        .hir
        .items
        .iter()
        .filter_map(|item| {
            let HirItemKind::Function(function) = &item.kind else {
                return None;
            };
            let name = unit.analysis.interner.resolve(function.name).to_owned();
            Some(TopLevelFunction {
                name,
                def_id: item.def_id,
                params: function
                    .params
                    .iter()
                    .map(|param| ImportedParamMeta {
                        optional: param.optional,
                        default: param.default.clone(),
                    })
                    .collect(),
            })
        })
        .collect()
}

#[derive(Clone)]
pub(super) struct ImportedParamMeta {
    pub optional: bool,
    pub default: Option<HirExpression>,
}

#[derive(Clone)]
pub(super) struct ImportedFunctionParams {
    pub source_path: PathBuf,
    pub params: Vec<ImportedParamMeta>,
}

pub(super) struct TopLevelConst {
    name: String,
    def_id: DefId,
    konst: HirConst,
}

/// Every top-level const data member of a sibling unit, exported or
/// module-private (D5.1: exported initializers and bodies may reference
/// private consts). Like [`top_level_functions`], this walks the analysis HIR
/// so the synthetic def id and the const's value travel together.
pub(super) fn top_level_consts(unit: &AnalyzedProgramNode) -> Vec<TopLevelConst> {
    unit.analysis
        .hir
        .items
        .iter()
        .filter_map(|item| {
            let HirItemKind::Constant(konst) = &item.kind else {
                return None;
            };
            let name = unit.analysis.interner.resolve(konst.name).to_owned();
            Some(TopLevelConst {
                name,
                def_id: item.def_id,
                konst: konst.clone(),
            })
        })
        .collect()
}

/// Link one const data member: allocate a synthetic def id in the
/// package-MIR range (same discipline as function sources), register the
/// member reference target when the member is exported, and carry the const
/// declaration for the entry lowering's transplant.
fn link_const_member(
    links: &mut PackageMirLinkAccumulator,
    caller_path: &Path,
    import_def_id: DefId,
    source_path: &Path,
    member: &TopLevelConst,
    exported: bool,
) {
    let synthetic = *links
        .source_rewrites
        .entry((source_path.to_path_buf(), member.def_id))
        .or_insert_with(|| {
            let def_id = DefId(links.next_synthetic);
            links.next_synthetic += 1;
            def_id
        });
    if exported {
        links.data_member_targets.insert(
            (
                caller_path.to_path_buf(),
                import_def_id,
                member.name.clone(),
            ),
            synthetic,
        );
    }
    links.data_members.push(DataMemberLink {
        synthetic,
        source_path: source_path.to_path_buf(),
        konst: member.konst.clone(),
    });
}

pub(super) fn rewrite_unit_namespace_calls(
    unit: &mut AnalyzedProgramNode,
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
    analysis: &mut radix_module::driver::AnalyzedModule,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    method_targets: &MethodCallTargets,
) -> Result<(), Vec<Diagnostic>> {
    // Library call targets are keyed by the library's native export names.
    // A localized consumer HIR uses the pack surface (`textus_visus`);
    // alias those keys so linking matches what analysis already accepted.
    let targets = expand_locale_namespace_targets(unit_path, analysis, targets);
    let data_member_targets =
        expand_locale_namespace_targets(unit_path, analysis, data_member_targets);
    let namespaces = expand_locale_namespace_exports(unit_path, analysis, namespaces);
    let targets = &targets;
    let data_member_targets = &data_member_targets;
    let namespaces = &namespaces;

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
    if let Some(methods) = method_targets.get(unit_path)
        && !methods.is_empty()
    {
        // CG2-R08: recover the callee function type for every synthetic
        // method-call target so a failable linked method keeps its
        // try-call edge (a ty:None callee would lower as a plain call and
        // orphan the cape handler's blocks, PKG001).
        let method_function_types = synthetic_method_function_types(analysis, methods);
        let mut rewriter = LibraryMethodCallRewriter {
            types: &analysis.types,
            interner: &analysis.interner,
            methods,
            method_function_types: &method_function_types,
            diagnostics: Vec::new(),
        };
        radix_module::hir::visit::walk_program_mut(&mut rewriter, &mut analysis.hir);
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
        match &mut item.kind {
            HirItemKind::Function(function) => {
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
            HirItemKind::Struct(strukt) => {
                // Method bodies are not top-level functions. Namespace calls
                // inside them (`layout.vertex_format_from_component_width`)
                // must still rewrite to linked definitions.
                for method in &mut strukt.methods {
                    if let Some(body) = &mut method.func.body {
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
            _ => {}
        }
    }

    let selective_members = selective_member_targets(unit_path, analysis, targets);
    if !selective_members.is_empty() {
        walk_program_mut(
            &mut SelectiveMemberCallRewriter {
                synthetic: &selective_members,
            },
            &mut analysis.hir,
        );
    }

    let selective_consts = selective_member_const_targets(unit_path, analysis, data_member_targets);
    if !selective_consts.is_empty() {
        walk_program_mut(
            &mut SelectiveMemberConstRewriter {
                synthetic: &selective_consts,
            },
            &mut analysis.hir,
        );
    }

    // pkg001-mir-fix: recover the callee function type on every synthetic
    // namespace-call target the rewrites above installed, so the radix MIR
    // lowerer can emit the try-call for failable library calls (a callee
    // without a type cannot be distinguished from a non-failable call and the
    // cape handler's blocks orphan).
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    let method_types = synthetic_namespace_function_types(unit_path, analysis, targets);
    if !method_types.is_empty() {
        walk_program_mut(
            &mut SyntheticCalleeTypePass {
                method_types: &method_types,
            },
            &mut analysis.hir,
        );
        // The callee type recovery makes failable synthetic calls lower as
        // try-calls; a cape whose binding was typed before the rewrite can
        // still carry the unresolved `ignotum` sink — recover it from the
        // body's failable synthetic callees.
        walk_program_mut(
            &mut SyntheticCapeBindingPass {
                method_types: &method_types,
                types: &analysis.types,
            },
            &mut analysis.hir,
        );
    }

    Ok(())
}

pub(super) fn rewrite_block(
    unit_path: &Path,
    block: &mut HirBlock,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<Diagnostic>,
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
    diagnostics: &mut Vec<Diagnostic>,
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
        HirStatementKind::Expr(expr)
        | HirStatementKind::Redde(Some(expr))
        | HirStatementKind::ReddeFilum(Some(expr)) => {
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
        | HirStatementKind::ReddeFilum(None)
        | HirStatementKind::Rumpe
        | HirStatementKind::Perge
        | HirStatementKind::Tacet => {}
    }
}

/// The synthetic direct call that replaces a linked namespace call.
///
/// Explicit call-site type arguments ride the rewritten direct call so
/// same-module generic lowering runs. Inferred call sites carry empty
/// `type_args` (semantic lowering writes HIR type args only for spelled
/// `<...>` arguments), so one forward covers both forms.
fn synthetic_namespace_call_kind(
    receiver: &HirExpression,
    target_def: DefId,
    type_args: &mut Vec<radix_types::TypeId>,
    args: &mut Vec<HirCallArg>,
) -> HirExpressionKind {
    let callee = HirExpression {
        id: receiver.id,
        kind: HirExpressionKind::Path(target_def),
        ty: None,
        span: receiver.span,
    };
    HirExpressionKind::Call(
        Box::new(callee),
        std::mem::take(type_args),
        std::mem::take(args),
        None,
    )
}

pub(super) fn rewrite_expr(
    unit_path: &Path,
    expr: &mut HirExpression,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &mut expr.kind {
        HirExpressionKind::Binary(_, lhs, rhs) | HirExpressionKind::Assign(lhs, rhs) => {
            rewrite_expr(
                unit_path,
                lhs,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            rewrite_expr(
                unit_path,
                rhs,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        HirExpressionKind::ConversioAssign {
            target,
            source,
            recovery,
            ..
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
        | HirExpressionKind::Praefixum(inner)
        | HirExpressionKind::Format { value: inner, .. }
        | HirExpressionKind::Marked { expr: inner, .. } => rewrite_expr(
            unit_path,
            inner,
            interner,
            targets,
            data_member_targets,
            namespaces,
            diagnostics,
        ),
        HirExpressionKind::Call(callee, type_args, args, _) => {
            rewrite_call_args(
                unit_path,
                args,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            if let HirExpressionKind::Field(receiver, method) = &callee.kind {
                if let Some(target_def) =
                    namespace_call_target(unit_path, receiver, *method, interner, targets)
                {
                    expr.kind =
                        synthetic_namespace_call_kind(receiver, target_def, type_args, args);
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
        HirExpressionKind::Gradient {
            call,
            selected_arguments,
        } => {
            rewrite_expr(
                unit_path,
                call,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            for selected_argument in selected_arguments {
                rewrite_expr(
                    unit_path,
                    selected_argument,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::MethodCall(receiver, method, type_args, args, _) => {
            rewrite_expr(
                unit_path,
                receiver,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            rewrite_call_args(
                unit_path,
                args,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            if let Some(target_def) =
                namespace_call_target(unit_path, receiver, *method, interner, targets)
            {
                expr.kind = synthetic_namespace_call_kind(receiver, target_def, type_args, args);
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
            // materializes the const through the top-level-const seam. The
            // node keeps its typecheck type: lowering reads `expr.ty` for
            // the value (a scalar tensor bracket index types its wrapper list
            // by it), and a cleared type silently dropped the function body.
            if let Some(target_def) = namespace_data_member_target(
                unit_path,
                object,
                *field,
                interner,
                data_member_targets,
            ) {
                expr.kind = HirExpressionKind::Path(target_def);
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
            rewrite_expr(
                unit_path,
                index,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
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
            rewrite_optional_chain(
                unit_path,
                chain,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
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
            rewrite_non_null_chain(
                unit_path,
                chain,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        HirExpressionKind::Block(block) | HirExpressionKind::Loop(block) => {
            rewrite_block(
                unit_path,
                block,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        HirExpressionKind::Si {
            cond,
            then_block,
            then_catch,
            else_block,
        } => {
            rewrite_expr(
                unit_path,
                cond,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
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
                rewrite_cape(
                    unit_path,
                    cape,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
            if let Some(block) = else_block {
                rewrite_block(
                    unit_path,
                    block,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
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
                rewrite_casu_arm(
                    unit_path,
                    arm,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Dum(cond, block) => {
            rewrite_expr(
                unit_path,
                cond,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            rewrite_block(
                unit_path,
                block,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        HirExpressionKind::Itera {
            iter: iterable,
            body: block,
            ..
        } => {
            rewrite_expr(
                unit_path,
                iterable,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            rewrite_block(
                unit_path,
                block,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        HirExpressionKind::IteraLockstep { sources, body, .. } => {
            for source in sources {
                rewrite_expr(
                    unit_path,
                    source,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
            rewrite_block(
                unit_path,
                body,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        // NR-3 (nary-range-walk): the Cartesian range product rewrites every
        // range extent and the body exactly like the lockstep series — one
        // rewrite walk, no equal-length policy attached (the product is a
        // declared-domain walk, never a zip).
        HirExpressionKind::IteraRange { ranges, body, .. } => {
            for range in ranges {
                rewrite_expr(
                    unit_path,
                    range,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
            rewrite_block(
                unit_path,
                body,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        HirExpressionKind::Reduction {
            source,
            group,
            body,
            identity,
            ..
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
            if let Some(group) = group {
                rewrite_expr(
                    unit_path,
                    group,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
            if let Some(body) = body {
                rewrite_block(
                    unit_path,
                    body,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
            if let Some(identity) = identity {
                rewrite_expr(
                    unit_path,
                    identity,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Scan {
            source,
            group,
            body,
            ..
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
            if let Some(group) = group {
                rewrite_expr(
                    unit_path,
                    group,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
            rewrite_block(
                unit_path,
                body,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        HirExpressionKind::Sort { source, body, .. } => {
            rewrite_expr(
                unit_path,
                source,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            if let Some(body) = body {
                rewrite_block(
                    unit_path,
                    body,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Intervallum {
            start, end, step, ..
        } => {
            rewrite_expr(
                unit_path,
                start,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            rewrite_expr(
                unit_path,
                end,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            if let Some(step) = step {
                rewrite_expr(
                    unit_path,
                    step,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Array(elements) => {
            for element in elements {
                match element {
                    radix_module::hir::HirArrayElement::Expr(expr)
                    | radix_module::hir::HirArrayElement::Spread(expr) => {
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
                    radix_module::hir::HirArrayElement::Infer(_) => {}
                }
            }
        }
        HirExpressionKind::Struct(_, fields) => {
            for (_, value) in fields {
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
        HirExpressionKind::Tuple(items, _)
        | HirExpressionKind::Scribe(_, items)
        | HirExpressionKind::Scriptum(_, items) => {
            for item in items {
                rewrite_expr(
                    unit_path,
                    item,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Adfirma(cond, message) => {
            rewrite_expr(
                unit_path,
                cond,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
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
            rewrite_block(
                unit_path,
                body,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            rewrite_cape(
                unit_path,
                catch,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        // UT: the capture boundary's body is an ordinary block for namespace
        // rewriting.
        HirExpressionKind::Trap(body) => rewrite_block(
            unit_path,
            body,
            interner,
            targets,
            data_member_targets,
            namespaces,
            diagnostics,
        ),
        HirExpressionKind::CallDefault { body, default, .. } => {
            rewrite_block(
                unit_path,
                body,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
            rewrite_expr(
                unit_path,
                default,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        HirExpressionKind::Clausura(_, _, _, body) => rewrite_expr(
            unit_path,
            body,
            interner,
            targets,
            data_member_targets,
            namespaces,
            diagnostics,
        ),
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
        HirExpressionKind::Ref(_, inner)
        | HirExpressionKind::Deref(inner)
        | HirExpressionKind::Widen { operand: inner, .. }
        | HirExpressionKind::TypeCheck { expr: inner, .. } => {
            rewrite_expr(
                unit_path,
                inner,
                interner,
                targets,
                data_member_targets,
                namespaces,
                diagnostics,
            );
        }
        HirExpressionKind::CopyInto { target, value } => {
            for part in [target, value] {
                rewrite_expr(
                    unit_path,
                    part,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::SinkInto { sink, item } => {
            for part in [sink, item] {
                rewrite_expr(
                    unit_path,
                    part,
                    interner,
                    targets,
                    data_member_targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Path(_)
        | HirExpressionKind::Literal(_)
        | HirExpressionKind::Insere(_)
        | HirExpressionKind::Vacua
        | HirExpressionKind::ReadLine(_)
        | HirExpressionKind::Error => {}
    }
}

fn expand_locale_namespace_targets<V: Clone>(
    unit_path: &Path,
    analysis: &mut radix_module::driver::AnalyzedModule,
    table: &HashMap<(PathBuf, DefId, String), V>,
) -> HashMap<(PathBuf, DefId, String), V> {
    let mut expanded = table.clone();
    // Interning into the unit interner below is order-sensitive: walk the
    // std `HashMap` in key order, never in its per-process seeded order.
    let mut entries: Vec<_> = table.iter().filter(|(key, _)| key.0 == unit_path).collect();
    entries.sort_by(|a, b| (a.0.1.0, &a.0.2).cmp(&(b.0.1.0, &b.0.2)));
    for ((path, def, name), value) in entries {
        let Some(symbol) = analysis.resolver.get_symbol(*def) else {
            continue;
        };
        let canonical = analysis.interner.intern(name);
        let surface = analysis
            .resolver
            .imported_export_surface_name(symbol.name, canonical);
        let surface_name = analysis.interner.resolve(surface).to_owned();
        if &surface_name != name {
            expanded.insert((path.clone(), *def, surface_name), value.clone());
        }
    }
    expanded
}

fn expand_locale_namespace_exports(
    unit_path: &Path,
    analysis: &mut radix_module::driver::AnalyzedModule,
    namespaces: &NamespaceExports,
) -> NamespaceExports {
    let mut expanded = namespaces.clone();
    // Interning into the unit interner below is order-sensitive: walk the
    // std `HashMap` in key order, never in its per-process seeded order.
    let mut entries: Vec<_> = namespaces
        .iter()
        .filter(|(key, _)| key.0 == unit_path)
        .collect();
    entries.sort_by_key(|(key, _)| key.1.0);
    for ((path, def), names) in entries {
        let Some(symbol) = analysis.resolver.get_symbol(*def) else {
            continue;
        };
        let extra = names
            .iter()
            .filter_map(|name| {
                let canonical = analysis.interner.intern(name);
                let surface = analysis
                    .resolver
                    .imported_export_surface_name(symbol.name, canonical);
                let surface_name = analysis.interner.resolve(surface).to_owned();
                (surface_name != *name).then_some(surface_name)
            })
            .collect::<BTreeSet<_>>();
        if !extra.is_empty() {
            expanded
                .entry((path.clone(), *def))
                .or_default()
                .extend(extra);
        }
    }
    expanded
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
    method: radix_module::lexer::Symbol,
    interner: &Interner,
    namespaces: &NamespaceExports,
) -> Option<Diagnostic> {
    let (namespace_def, mut fields) = namespace_receiver_path(receiver, interner)?;
    let exports = namespaces.get(&(unit_path.to_path_buf(), namespace_def))?;
    let method_name = interner.resolve(method).to_owned();
    let diagnostic = |message: String, reason: &'static str, member: &str| {
        crate::package_diagnostic_error(message)
            .with_file(unit_path.display().to_string())
            .with_arg("reason", reason)
            .with_arg("member", member)
    };
    if fields.is_empty() {
        if exports.contains(&method_name) {
            return Some(diagnostic(
                format!(
                    "package MIR does not yet support non-function namespace member `{method_name}`"
                ),
                "non_function_namespace_member",
                &method_name,
            ));
        }
        return Some(diagnostic(
            format!("namespace does not export `{method_name}`"),
            "namespace_member_not_exported",
            &method_name,
        ));
    }
    fields.push(method_name);
    let qualified = fields.join(".");
    Some(diagnostic(
        format!("package MIR does not yet support nested namespace call `{qualified}`"),
        "nested_namespace_call",
        &qualified,
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
fn rewrite_namespace_variant_members(analysis: &mut radix_module::driver::AnalyzedModule) {
    struct VariantMemberRewriter<'a> {
        resolver: &'a Resolver,
    }

    impl HirVisitorMut for VariantMemberRewriter<'_> {
        fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
            if let HirExpressionKind::Field(object, field) = &expr.kind
                && let HirExpressionKind::Path(namespace_def) = &object.kind
                && let Some(namespace) = self
                    .resolver
                    .get_symbol(*namespace_def)
                    .map(|symbol| symbol.name)
                && let Some((variant_def, _ty)) =
                    self.resolver.namespace_file_variant(namespace, *field)
            {
                // The typechecker already assigned the variant's
                // value type (`Type::Enum(consumer enum def)`) to
                // the field expression; `lower_path` needs that
                // type to construct the consumer variant, so the
                // rewritten path keeps the expression's type.
                expr.kind = HirExpressionKind::Path(variant_def);
            }
            walk_expr_mut(self, expr);
        }
    }

    let mut rewriter = VariantMemberRewriter {
        resolver: &analysis.resolver,
    };
    rewriter.visit_program_mut(&mut analysis.hir);
}

pub(super) struct ShadowedAliasRewriter<'a> {
    pub(super) unit_path: &'a Path,
    pub(super) resolver: &'a Resolver,
    pub(super) interner: &'a Interner,
    pub(super) targets: &'a NamespaceCallTargets,
    pub(super) imports: &'a HashMap<String, DefId>,
    /// `DefId` → binding name for every definition introduced by HIR lowering
    /// (params, locals, itera/cape/pattern bindings, closure params, entry
    /// args). These synthetic defs (>= `1_000_000`) are never registered in the
    /// resolver's symbol table, so this HIR-derived map is the only way to
    /// recover a synthetic receiver's name.
    pub(super) local_names: &'a HashMap<DefId, Symbol>,
    pub(super) types: &'a TypeTable,
    /// `(owner def, method symbol)` pairs for the unit's own genus/interface
    /// methods: a synthetic receiver whose method resolves on its own type is
    /// a genuine method call and must never be rewritten into a namespace
    /// call, even when the receiver's name shadows an import binding.
    pub(super) genus_methods: &'a HashSet<(DefId, Symbol)>,
    pub(super) diagnostics: &'a mut Vec<Diagnostic>,
}
impl ShadowedAliasRewriter<'_> {
    pub(super) fn import_target(
        &self,
        receiver: &HirExpression,
        method: Symbol,
        arg_count: usize,
    ) -> Option<DefId> {
        let HirExpressionKind::Path(def) = &receiver.kind else {
            return None;
        };
        // The main rewrite already handles directly-keyed receivers.
        if namespace_call_target(
            self.unit_path,
            receiver,
            method,
            self.interner,
            self.targets,
        )
        .is_some()
        {
            return None;
        }
        // A HIR-generated receiver def (>= 1_000_000) is a synthetic binding
        // introduced by HIR lowering. The `forma`-shadowing shape is exactly a
        // LOCAL whose name shadows an import binding: the semantic namespace
        // reference then carries the local's synthetic def instead of the
        // import item's def. Synthetic defs are absent from the resolver
        // symbol table, so the binding name is recovered from the HIR
        // def→name map and resolved to the receiver's OWN import binding —
        // never a name sweep across the unit's imports (which rewrites any
        // ordinary method call on a local whose method happens to match an
        // imported module's export, e.g. `h1b.gelu()` → `nn.gelu(h1b)`).
        if def.is_hir_synthetic() {
            let binding_name = self.local_names.get(def).copied()?;
            let local_name = self.interner.resolve(binding_name).to_owned();
            let import_def = self.imports.get(&local_name).copied()?;
            // Guard: a genuine method call on the receiver's own type is not
            // a namespace reference and must not be rewritten, even when the
            // receiver's name shadows an import binding and the module exports
            // the same method name.
            if self.method_resolves_on_receiver_type(receiver, method, arg_count) {
                return None;
            }
            let method_name = self.interner.resolve(method).to_owned();
            return self
                .targets
                .get(&(self.unit_path.to_path_buf(), import_def, method_name))
                .copied();
        }
        // Only Module symbols (import namespaces) — or locals that shadow
        // one (the name-based resolution below then matches the import
        // binding, mirroring the analysis's namespace-member intent).
        let symbol = self.resolver.get_symbol(*def)?;
        if !matches!(
            symbol.kind,
            radix_module::semantic::SymbolKind::Module | radix_module::semantic::SymbolKind::Local
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
        namespace_call_target(
            self.unit_path,
            &import_receiver,
            method,
            self.interner,
            self.targets,
        )
    }

    /// Whether `method` resolves as a genuine method on `receiver`'s type:
    /// the promoted intrinsic registry, or a genus/interface method on the
    /// receiver's (normalized) struct type.
    fn method_resolves_on_receiver_type(
        &self,
        receiver: &HirExpression,
        method: Symbol,
        arg_count: usize,
    ) -> bool {
        let Some(ty) = receiver.ty else {
            return false;
        };
        let method_name = self.interner.resolve(method).to_owned();
        let normalized = match self.types.get(ty) {
            Type::Alias(_, inner) => self.types.get(*inner),
            other => other,
        };
        if radix_module::intrinsics::lookup_method_for_type(normalized, &method_name, arg_count)
            .is_some()
        {
            return true;
        }
        let struct_def = match normalized {
            Type::Struct(def_id) => Some(*def_id),
            Type::Applied(base, _) => match self.types.get(*base) {
                Type::Struct(def_id) => Some(*def_id),
                _ => None,
            },
            _ => None,
        };
        struct_def.is_some_and(|def| self.genus_methods.contains(&(def, method)))
    }
}
impl HirVisitorMut for ShadowedAliasRewriter<'_> {
    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        match &mut expr.kind {
            HirExpressionKind::Call(callee, _, args, _) => {
                if let HirExpressionKind::Field(receiver, method) = &callee.kind
                    && let Some(synthetic) = self.import_target(receiver, *method, args.len())
                {
                    **callee = HirExpression {
                        id: receiver.id,
                        kind: HirExpressionKind::Path(synthetic),
                        ty: None,
                        span: receiver.span,
                    };
                }
            }
            HirExpressionKind::MethodCall(receiver, method, type_args, args, _) => {
                if let Some(synthetic) = self.import_target(receiver, *method, args.len()) {
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
                            None,
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
    analysis: &mut radix_module::driver::AnalyzedModule,
    targets: &NamespaceCallTargets,
    diagnostics: &mut Vec<Diagnostic>,
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
    // Synthetic HIR bindings (params, locals, itera/cape/pattern bindings,
    // closure params) are allocated >= 1_000_000 by HIR lowering and are never
    // registered in the resolver's symbol table, so their names must be
    // recovered from the HIR itself: every definition introduction in the
    // program (items, params, locals, patterns, handlers, loop bindings).
    let mut local_names: HashMap<DefId, Symbol> = HashMap::new();
    {
        let mut collector = LocalNameCollector {
            names: &mut local_names,
        };
        collector.visit_program(&analysis.hir);
    }
    // Genus/interface methods on the unit's own types: a synthetic receiver
    // whose method resolves on its own type is a genuine method call and must
    // not be rewritten into a namespace call.
    let mut genus_methods: HashSet<(DefId, Symbol)> = HashSet::new();
    for item in &analysis.hir.items {
        match &item.kind {
            HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    genus_methods.insert((item.def_id, method.func.name));
                }
            }
            HirItemKind::Interface(interface) => {
                for method in &interface.methods {
                    genus_methods.insert((item.def_id, method.name));
                }
            }
            _ => {}
        }
    }
    let mut rewriter = ShadowedAliasRewriter {
        unit_path,
        resolver: &analysis.resolver,
        interner: &analysis.interner,
        targets,
        imports: &imports,
        local_names: &local_names,
        types: &analysis.types,
        genus_methods: &genus_methods,
        diagnostics,
    };
    rewriter.visit_program_mut(&mut analysis.hir);
}

/// Collect every `DefId → Symbol` definition introduction in a program.
///
/// The HIR visitor's `visit_def` fires for items, parameters, locals,
/// patterns, imports, methods, fields, handlers, and loop bindings — exactly
/// the bindings a synthetic method-call receiver can carry.
struct LocalNameCollector<'a> {
    names: &'a mut HashMap<DefId, Symbol>,
}
impl radix_module::hir::visit::HirVisitor for LocalNameCollector<'_> {
    fn visit_def(&mut self, def_id: DefId, name: Symbol) {
        self.names.insert(def_id, name);
    }
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
    types: &'a radix_module::semantic::TypeTable,
    interner: &'a Interner,
    methods: &'a HashMap<(DefId, String), DefId>,
    /// CG2-R08: recovered function type (with its error half) per
    /// `(receiver nominal struct def, method name)`, so a failable linked
    /// method's synthetic callee keeps its try-call edge. A method absent
    /// from this map fails closed (diagnostic) instead of installing a
    /// silent `ty: None` callee.
    method_function_types: &'a HashMap<(DefId, String), radix_types::TypeId>,
    diagnostics: Vec<String>,
}

impl LibraryMethodCallRewriter<'_> {
    fn receiver_struct_def(&self, receiver: &HirExpression) -> Option<DefId> {
        self.receiver_struct_def_from_ty(receiver.ty?)
    }

    fn receiver_struct_def_from_ty(&self, ty: radix_types::TypeId) -> Option<DefId> {
        // Typecheck peels access markers and nullability before method lookup.
        // The package-MIR rewrite must too: `mut t.get()` / `t ∪ nihil` on a
        // linked library nominal otherwise stays MethodCall and fail-closes as
        // "method call before runtime/provider MIR lowering".
        let payload = self.types.peel_access_marker(ty).1;
        let payload = self.types.nullable_inner(payload).unwrap_or(payload);
        match self.types.get(payload) {
            Type::Struct(def_id) => Some(*def_id),
            // Namespace-qualified imported nominal types can retain an alias
            // or applied wrapper in converted HIR; peel those wrappers before
            // looking up the source method target.
            Type::Alias(_, inner) | Type::Ref(_, inner) => self.receiver_struct_def_from_ty(*inner),
            Type::Applied(base, _) => self.receiver_struct_def_from_ty(*base),
            _ => None,
        }
    }

    /// Build the synthetic call pieces for either HIR method-call shape:
    /// the canonical `MethodCall(receiver, method, …)` form or the converted
    /// `Call(Field(receiver, method), …)` form. The latter is also used for
    /// method calls whose receiver is itself a projection.
    fn synthetic_call_parts(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        type_args: &mut Vec<radix_types::TypeId>,
        args: &mut Vec<HirCallArg>,
    ) -> Option<(HirExpression, Vec<radix_types::TypeId>, Vec<HirCallArg>)> {
        let struct_def = self.receiver_struct_def(receiver)?;
        let method_name = self.interner.resolve(method).to_owned();
        let synthetic = self
            .methods
            .get(&(struct_def, method_name.clone()))
            .copied()?;
        // CG2-R08: recover the callee's function type so the MIR lowerer
        // keeps the try-call edge for failable methods; a method whose
        // signature cannot be recovered fails closed instead of silently
        // emitting a ty:None callee.
        let Some(function_ty) = self
            .method_function_types
            .get(&(struct_def, method_name.clone()))
            .copied()
        else {
            self.diagnostics.push(format!(
                "cannot recover function type for linked library method call `{struct_def:?}.{method_name}`; refusing an untyped synthetic callee"
            ));
            return None;
        };
        let mut call_args = std::mem::take(args);
        let mut new_args = Vec::with_capacity(call_args.len() + 1);
        new_args.push(HirCallArg {
            name: None,
            spread: false,
            marker: None,
            expr: receiver.clone(),
            span: receiver.span,
        });
        new_args.append(&mut call_args);
        let callee = HirExpression {
            id: receiver.id,
            kind: HirExpressionKind::Path(synthetic),
            ty: Some(function_ty),
            span: receiver.span,
        };
        Some((callee, std::mem::take(type_args), new_args))
    }
}

/// Replace a rewritten method call in place with its synthetic direct-call
/// form, keeping the original expression's id, type and span.
fn replace_with_synthetic_call(
    expr: &mut HirExpression,
    callee: HirExpression,
    type_args: Vec<radix_types::TypeId>,
    args: Vec<HirCallArg>,
) {
    *expr = HirExpression {
        id: expr.id,
        kind: HirExpressionKind::Call(Box::new(callee), type_args, args, None),
        ty: expr.ty,
        span: expr.span,
    };
}

impl HirVisitorMut for LibraryMethodCallRewriter<'_> {
    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        match &mut expr.kind {
            HirExpressionKind::MethodCall(receiver, method, type_args, args, _) => {
                if let Some((callee, type_args, args)) =
                    self.synthetic_call_parts(receiver, *method, type_args, args)
                {
                    replace_with_synthetic_call(expr, callee, type_args, args);
                }
            }
            HirExpressionKind::Call(callee, type_args, args, _) => {
                let (receiver, method) =
                    if let HirExpressionKind::Field(receiver, method) = &callee.kind {
                        (receiver, *method)
                    } else {
                        walk_expr_mut(self, expr);
                        return;
                    };
                if let Some((new_callee, type_args, args)) =
                    self.synthetic_call_parts(receiver, method, type_args, args)
                {
                    replace_with_synthetic_call(expr, new_callee, type_args, args);
                }
            }
            _ => {}
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
    diagnostics: &mut Vec<Diagnostic>,
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
    diagnostics: &mut Vec<Diagnostic>,
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
    diagnostics: &mut Vec<Diagnostic>,
) {
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
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &mut field.key {
        radix_module::hir::HirObjectKey::Computed(key)
        | radix_module::hir::HirObjectKey::Spread(key) => {
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
        radix_module::hir::HirObjectKey::Ident(_) | radix_module::hir::HirObjectKey::String(_) => {}
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
    diagnostics: &mut Vec<Diagnostic>,
) {
    match chain {
        HirOptionalChainKind::Member(_) => {}
        HirOptionalChainKind::Index(index) => rewrite_expr(
            unit_path,
            index,
            interner,
            targets,
            data_member_targets,
            namespaces,
            diagnostics,
        ),
        HirOptionalChainKind::Call(args) => rewrite_call_args(
            unit_path,
            args,
            interner,
            targets,
            data_member_targets,
            namespaces,
            diagnostics,
        ),
    }
}

pub(super) fn rewrite_non_null_chain(
    unit_path: &Path,
    chain: &mut radix_module::hir::HirNonNullKind,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    data_member_targets: &NamespaceDataMemberTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match chain {
        radix_module::hir::HirNonNullKind::Value | radix_module::hir::HirNonNullKind::Member(_) => {
        }
        radix_module::hir::HirNonNullKind::Index(index) => rewrite_expr(
            unit_path,
            index,
            interner,
            targets,
            data_member_targets,
            namespaces,
            diagnostics,
        ),
        radix_module::hir::HirNonNullKind::Call(args) => rewrite_call_args(
            unit_path,
            args,
            interner,
            targets,
            data_member_targets,
            namespaces,
            diagnostics,
        ),
    }
}

#[cfg(test)]
#[path = "link_test.rs"]
mod link_tests;

#[cfg(test)]
#[path = "nary_range_link_test.rs"]
mod nary_range_link_tests;
