//! Package-local namespace-call linking and rewrite passes.

use super::*;

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
    let mut targets = HashMap::new();
    let mut namespaces = HashMap::new();
    let mut source_rewrites = HashMap::new();
    let mut next_synthetic = PACKAGE_MIR_SYNTHETIC_DEF_BASE;
    let mut diagnostics = Vec::new();
    // Linked library modules (deduplicated by source path), consumed by the
    // lowering pass.
    let mut linked_libraries: Vec<LibraryLinkTarget> = Vec::new();

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
                    namespaces.insert((unit.path.clone(), import_item.def_id), exports.clone());
                    for function in exported_top_level_functions(sibling, &exports) {
                        let synthetic = *source_rewrites
                            .entry((sibling.path.clone(), function.def_id))
                            .or_insert_with(|| {
                                let def_id = DefId(next_synthetic);
                                next_synthetic += 1;
                                def_id
                            });
                        targets.insert(
                            (unit.path.clone(), import_item.def_id, function.name),
                            synthetic,
                        );
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
                        namespaces.insert((unit.path.clone(), import_item.def_id), exports.clone());
                        for function in exported_top_level_functions(sibling, &exports) {
                            let synthetic = *source_rewrites
                                .entry((sibling.path.clone(), function.def_id))
                                .or_insert_with(|| {
                                    let def_id = DefId(next_synthetic);
                                    next_synthetic += 1;
                                    def_id
                                });
                            targets.insert(
                                (unit.path.clone(), import_item.def_id, function.name),
                                synthetic,
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
                    let placeholder = LibraryImportBinding {
                        binding: String::new(),
                        visibility: radix::syntax::Visibility::Privata,
                        import_span: radix::lexer::Span::default(),
                        module: module.clone(),
                    };
                    let Ok(interface) = library_cached_file_interface(
                        &placeholder,
                        library_resolver,
                        library_cache,
                    ) else {
                        push_library_import_unsupported(
                            &mut diagnostics,
                            &unit.path,
                            import_path,
                            consumer,
                        );
                        continue;
                    };
                    let Ok(analysis) =
                        library_cached_analysis(&placeholder, library_resolver, library_cache)
                    else {
                        push_library_import_unsupported(
                            &mut diagnostics,
                            &unit.path,
                            import_path,
                            consumer,
                        );
                        continue;
                    };
                    for import_item in &import.items {
                        let binding = LibraryImportBinding {
                            binding: unit
                                .analysis
                                .interner
                                .resolve(import_item.alias.unwrap_or(import_item.name))
                                .to_owned(),
                            visibility: import.visibility,
                            import_span: radix::lexer::Span::default(),
                            module: module.clone(),
                        };
                        let exports = interface.exports.keys().cloned().collect::<BTreeSet<_>>();
                        namespaces.insert((unit.path.clone(), import_item.def_id), exports.clone());
                        let library_path = module.interface_path.clone();
                        // Every library function gets a synthetic source so no
                        // raw def-id from the library's separate def-id space
                        // leaks into the merged program; only exported names
                        // become call targets for the consumer.
                        for item in &analysis.hir.items {
                            let HirItemKind::Function(function) = &item.kind else {
                                continue;
                            };
                            let name = analysis.interner.resolve(function.name).to_owned();
                            let synthetic = *source_rewrites
                                .entry((library_path.clone(), item.def_id))
                                .or_insert_with(|| {
                                    let def_id = DefId(next_synthetic);
                                    next_synthetic += 1;
                                    def_id
                                });
                            if exports.contains(&name) {
                                targets.insert(
                                    (unit.path.clone(), import_item.def_id, name),
                                    synthetic,
                                );
                            }
                        }
                        // `@ radix backward` companions: compiler-generated
                        // functions with no HIR item; the companion `DefId` is
                        // the source key the generated MIR carries.
                        for (_, backward) in analysis.radix_lanes.iter_backward() {
                            let name = analysis
                                .interner
                                .resolve(backward.companion_name)
                                .to_owned();
                            let synthetic = *source_rewrites
                                .entry((library_path.clone(), backward.companion_def_id))
                                .or_insert_with(|| {
                                    let def_id = DefId(next_synthetic);
                                    next_synthetic += 1;
                                    def_id
                                });
                            if exports.contains(&name) {
                                targets.insert(
                                    (unit.path.clone(), import_item.def_id, name),
                                    synthetic,
                                );
                            }
                        }
                        if linked_libraries
                            .iter()
                            .all(|target| target.path != module.interface_path)
                        {
                            linked_libraries.push(LibraryLinkTarget {
                                path: module.interface_path.clone(),
                                import: binding,
                            });
                        }
                    }
                }
                ImportResolution::Error(diag) => {
                    // External-target builds keep the previous silent-skip;
                    // only the interpreted run path fails closed here.
                    if consumer == PackageMirConsumer::Interpreted {
                        diagnostics.push(diag);
                    }
                }
                ImportResolution::Unsupported => continue,
            }
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    Ok(PackageMirLinks {
        calls: targets,
        namespaces,
        sources: source_rewrites,
        next_synthetic,
        libraries: linked_libraries,
    })
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

pub(super) fn rewrite_unit_namespace_calls(
    unit: &mut AnalyzedPackageUnit,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    if let Some(entry) = &mut unit.analysis.hir.entry {
        rewrite_block(
            &unit.path,
            entry,
            &unit.analysis.interner,
            targets,
            namespaces,
            &mut diagnostics,
        );
    }
    for item in &mut unit.analysis.hir.items {
        if let HirItemKind::Function(function) = &mut item.kind {
            if let Some(body) = &mut function.body {
                rewrite_block(
                    &unit.path,
                    body,
                    &unit.analysis.interner,
                    targets,
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
                crate::package_diagnostic_error(message).with_file(unit.path.display().to_string())
            })
            .collect())
    }
}

pub(super) fn rewrite_block(
    unit_path: &Path,
    block: &mut HirBlock,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    for stmt in &mut block.statements {
        rewrite_stmt(unit_path, stmt, interner, targets, namespaces, diagnostics);
    }
    if let Some(expr) = &mut block.expr {
        rewrite_expr(unit_path, expr, interner, targets, namespaces, diagnostics);
    }
}

pub(super) fn rewrite_stmt(
    unit_path: &Path,
    stmt: &mut HirStatement,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match &mut stmt.kind {
        HirStatementKind::Local(local) => {
            if let Some(init) = &mut local.init {
                rewrite_expr(unit_path, init, interner, targets, namespaces, diagnostics);
            }
        }
        HirStatementKind::Expr(expr) => {
            rewrite_expr(unit_path, expr, interner, targets, namespaces, diagnostics);
        }
        HirStatementKind::Redde(Some(expr)) => {
            rewrite_expr(unit_path, expr, interner, targets, namespaces, diagnostics)
        }
        HirStatementKind::IncDec(inc_dec) => rewrite_expr(
            unit_path,
            &mut inc_dec.target,
            interner,
            targets,
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
                    namespaces,
                    diagnostics,
                );
                rewrite_block(
                    unit_path,
                    &mut clause.body,
                    interner,
                    targets,
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
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match &mut expr.kind {
        HirExpressionKind::Binary(_, lhs, rhs) | HirExpressionKind::Assign(lhs, rhs) => {
            rewrite_expr(unit_path, lhs, interner, targets, namespaces, diagnostics);
            rewrite_expr(unit_path, rhs, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::Unary(_, inner)
        | HirExpressionKind::Cede(inner)
        | HirExpressionKind::Reddet(inner)
        | HirExpressionKind::Tacebit(inner)
        | HirExpressionKind::Yield(inner)
        | HirExpressionKind::Panic(inner)
        | HirExpressionKind::Throw(inner)
        | HirExpressionKind::Praefixum(inner) => {
            rewrite_expr(unit_path, inner, interner, targets, namespaces, diagnostics)
        }
        HirExpressionKind::Call(callee, type_args, args) => {
            rewrite_call_args(unit_path, args, interner, targets, namespaces, diagnostics);
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
                namespaces,
                diagnostics,
            );
            rewrite_call_args(unit_path, args, interner, targets, namespaces, diagnostics);
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
        HirExpressionKind::Field(object, _) => rewrite_expr(
            unit_path,
            object,
            interner,
            targets,
            namespaces,
            diagnostics,
        ),
        HirExpressionKind::Index(object, index) => {
            rewrite_expr(
                unit_path,
                object,
                interner,
                targets,
                namespaces,
                diagnostics,
            );
            rewrite_expr(unit_path, index, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::OptionalChain(object, chain) => {
            rewrite_expr(
                unit_path,
                object,
                interner,
                targets,
                namespaces,
                diagnostics,
            );
            rewrite_optional_chain(unit_path, chain, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::NonNull(object, chain) => {
            rewrite_expr(
                unit_path,
                object,
                interner,
                targets,
                namespaces,
                diagnostics,
            );
            rewrite_non_null_chain(unit_path, chain, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::Block(block) | HirExpressionKind::Loop(block) => {
            rewrite_block(unit_path, block, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::Si {
            cond,
            then_block,
            then_catch,
            else_block,
        } => {
            rewrite_expr(unit_path, cond, interner, targets, namespaces, diagnostics);
            rewrite_block(
                unit_path,
                then_block,
                interner,
                targets,
                namespaces,
                diagnostics,
            );
            if let Some(cape) = then_catch {
                rewrite_cape(unit_path, cape, interner, targets, namespaces, diagnostics);
            }
            if let Some(block) = else_block {
                rewrite_block(unit_path, block, interner, targets, namespaces, diagnostics);
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
                    namespaces,
                    diagnostics,
                );
            }
            for arm in arms {
                rewrite_casu_arm(unit_path, arm, interner, targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Dum(cond, block) => {
            rewrite_expr(unit_path, cond, interner, targets, namespaces, diagnostics);
            rewrite_block(unit_path, block, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::Itera(_, _, _, iterable, block) => {
            rewrite_expr(
                unit_path,
                iterable,
                interner,
                targets,
                namespaces,
                diagnostics,
            );
            rewrite_block(unit_path, block, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::Intervallum {
            start, end, step, ..
        } => {
            rewrite_expr(unit_path, start, interner, targets, namespaces, diagnostics);
            rewrite_expr(unit_path, end, interner, targets, namespaces, diagnostics);
            if let Some(step) = step {
                rewrite_expr(unit_path, step, interner, targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Array(elements) => {
            for element in elements {
                match element {
                    radix::hir::HirArrayElement::Expr(expr)
                    | radix::hir::HirArrayElement::Spread(expr) => {
                        rewrite_expr(unit_path, expr, interner, targets, namespaces, diagnostics);
                    }
                }
            }
        }
        HirExpressionKind::Struct(_, fields) => {
            for (_, value) in fields {
                rewrite_expr(unit_path, value, interner, targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Tuple(items, _)
        | HirExpressionKind::Scribe(_, items)
        | HirExpressionKind::Scriptum(_, items) => {
            for item in items {
                rewrite_expr(unit_path, item, interner, targets, namespaces, diagnostics);
            }
        }
        HirExpressionKind::Adfirma(cond, message) => {
            rewrite_expr(unit_path, cond, interner, targets, namespaces, diagnostics);
            if let Some(message) = message {
                rewrite_expr(
                    unit_path,
                    message,
                    interner,
                    targets,
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Handled { body, catch } => {
            rewrite_block(unit_path, body, interner, targets, namespaces, diagnostics);
            rewrite_cape(unit_path, catch, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::Clausura(_, _, _, body) => {
            rewrite_expr(unit_path, body, interner, targets, namespaces, diagnostics)
        }
        HirExpressionKind::Verte {
            source, entries, ..
        } => {
            rewrite_expr(
                unit_path,
                source,
                interner,
                targets,
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
                namespaces,
                diagnostics,
            );
            if let Some(recovery) = recovery {
                rewrite_expr(
                    unit_path,
                    recovery,
                    interner,
                    targets,
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
                    namespaces,
                    diagnostics,
                );
            }
        }
        HirExpressionKind::Ref(_, inner) | HirExpressionKind::Deref(inner) => {
            rewrite_expr(unit_path, inner, interner, targets, namespaces, diagnostics);
        }
        HirExpressionKind::TypeCheck { expr: inner, .. } => {
            rewrite_expr(unit_path, inner, interner, targets, namespaces, diagnostics);
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

pub(super) fn rewrite_call_args(
    unit_path: &Path,
    args: &mut [HirCallArg],
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    for arg in args {
        rewrite_expr(
            unit_path,
            &mut arg.expr,
            interner,
            targets,
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
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    rewrite_block(
        unit_path,
        &mut cape.body,
        interner,
        targets,
        namespaces,
        diagnostics,
    );
}

pub(super) fn rewrite_casu_arm(
    unit_path: &Path,
    arm: &mut HirCasuArm,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    if let Some(guard) = &mut arm.guard {
        rewrite_expr(unit_path, guard, interner, targets, namespaces, diagnostics);
    }
    rewrite_expr(
        unit_path,
        &mut arm.body,
        interner,
        targets,
        namespaces,
        diagnostics,
    );
}

pub(super) fn rewrite_object_field(
    unit_path: &Path,
    field: &mut HirObjectField,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match &mut field.key {
        radix::hir::HirObjectKey::Computed(key) | radix::hir::HirObjectKey::Spread(key) => {
            rewrite_expr(unit_path, key, interner, targets, namespaces, diagnostics);
        }
        radix::hir::HirObjectKey::Ident(_) | radix::hir::HirObjectKey::String(_) => {}
    }
    if let Some(value) = &mut field.value {
        rewrite_expr(unit_path, value, interner, targets, namespaces, diagnostics);
    }
}

pub(super) fn rewrite_optional_chain(
    unit_path: &Path,
    chain: &mut HirOptionalChainKind,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match chain {
        HirOptionalChainKind::Member(_) => {}
        HirOptionalChainKind::Index(index) => {
            rewrite_expr(unit_path, index, interner, targets, namespaces, diagnostics)
        }
        HirOptionalChainKind::Call(args) => {
            rewrite_call_args(unit_path, args, interner, targets, namespaces, diagnostics)
        }
    }
}

pub(super) fn rewrite_non_null_chain(
    unit_path: &Path,
    chain: &mut radix::hir::HirNonNullKind,
    interner: &Interner,
    targets: &NamespaceCallTargets,
    namespaces: &NamespaceExports,
    diagnostics: &mut Vec<String>,
) {
    match chain {
        radix::hir::HirNonNullKind::Member(_) => {}
        radix::hir::HirNonNullKind::Index(index) => {
            rewrite_expr(unit_path, index, interner, targets, namespaces, diagnostics)
        }
        radix::hir::HirNonNullKind::Call(args) => {
            rewrite_call_args(unit_path, args, interner, targets, namespaces, diagnostics)
        }
    }
}
