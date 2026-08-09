//! Lower package units (and libraries) into the merged package MIR program.

use super::*;
use radix::file_interface::FileInterface;

pub(super) fn lower_package_units<'a>(
    package: &'a mut AnalyzedPackage,
    entry_index: usize,
    links: &PackageMirLinks,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    cli_plan: &CliPackagePlan,
    no_fuse: bool,
) -> Result<LoweredMirUnit<'a>, Vec<Diagnostic>> {
    struct PendingUnit<'a> {
        lowered: LoweredMirUnit<'a>,
        dispatch_function: Option<MirFunctionId>,
    }

    // S1 U3 reachability seed: the package's rewritten unit HIR references
    // linked library functions through package-MIR synthetic def ids
    // (`Path(synthetic)` namespace-call targets and const data-member
    // references). That set seeds the library-function reachability closure
    // below, so linked library functions are lowered only when reachable from
    // the entry's call graph (default = pruning, delivery §4.3).
    let mut reachable_seed = collect_package_synthetic_references(package);

    let (before, rest) = package.units.split_at_mut(entry_index);
    let Some((entry, after)) = rest.split_first_mut() else {
        unreachable!("entry index selected from package units");
    };
    // Linked const data members (sibling units + libraries) are transplanted
    // into the entry analysis before the entry lowers: the entry's rewritten
    // `Path(synthetic_def)` member references then materialize through the
    // entry's existing top-level-const seam.
    inject_package_data_members(
        entry,
        &*before,
        &*after,
        links,
        library_resolver,
        library_cache,
    )?;
    let entry_path = entry.path.clone();
    let mut pending = Vec::new();
    // Mutable copy of the link sources: the library-lowering pass extends it
    // with synthetic ids for auto-generated sub-companion sources.
    let mut source_rewrites = links.sources.clone();
    let mut next_synthetic = links.next_synthetic;

    for unit in before.iter_mut().chain(after.iter_mut()) {
        let unit_path = unit.path.clone();
        let source_interner = unit.analysis.interner.clone();
        let source_exports = exported_nominal_names(&unit.file_interface);
        // S1 U2 nominal identity: build the canonical nominal remap while the
        // source analysis is freely borrowable (lowering mutates it below).
        let nominal_map = match build_nominal_remap(
            &unit.analysis.types,
            &unit.analysis.interner,
            &unit.analysis.resolver,
            unit.file_interface.identity.as_ref(),
            &source_exports,
            &entry.analysis,
        ) {
            Ok(map) => map,
            Err(message) => {
                return Err(vec![mir_issue_diag(
                    &unit_path,
                    "package_mir_nominal_identity_unknown",
                    message,
                )]);
            }
        };
        // S1 U2 VALUE members: variant definitions ride the same remap.
        let variant_remap = match build_variant_remap(
            &unit.analysis.interner,
            &unit.analysis.resolver,
            unit.file_interface.identity.as_ref(),
            &source_exports,
            &entry.analysis,
        ) {
            Ok(remap) => remap,
            Err(message) => {
                return Err(vec![mir_issue_diag(
                    &unit_path,
                    "package_mir_variant_identity_unknown",
                    message,
                )]);
            }
        };
        let mut lowered = lower_unit(unit, &cli_plan.entry_records, no_fuse)?;
        remap_program_text_symbols(
            &mut lowered.program,
            &lowered.interner,
            &mut entry.analysis.interner,
        );
        let nominal = NominalImportContext { nominal_map: &nominal_map };
        let source_to_entry_types = import_lowered_semantic_types(
            lowered.validated.validation().types,
            &mut entry.analysis.types,
            Some(&nominal),
        );
        rewrite_lowered_type_ids(&mut lowered, &source_to_entry_types);
        rewrite_lowered_variant_defs(&mut lowered, &variant_remap);
        if let Some(rewrite) = cli_plan
            .dispatch
            .as_ref()
            .filter(|dispatch| dispatch.unit_path == unit_path)
            .and_then(|dispatch| dispatch.record_type_rewrite.as_ref())
        {
            let dispatch_rewrites =
                imported_dispatch_type_rewrites(rewrite, &source_to_entry_types);
            rewrite_lowered_type_ids(&mut lowered, &dispatch_rewrites);
        }
        rewrite_program_sources(&mut lowered.program, &unit_path, &source_rewrites);
        ensure_unique_definition_sources(&lowered.program, &unit_path)?;
        let dispatch_function = selected_cli_dispatch_function(
            cli_plan,
            &unit_path,
            &lowered.program,
            &source_interner,
            &entry.analysis.interner.clone(),
        );
        pending.push(PendingUnit {
            lowered,
            dispatch_function,
        });
    }

    // Lower linked library modules before the entry lowers, so their symbols
    // and types can be remapped into the entry's tables without aliasing the
    // merged program. The lowered library programs are owned (functions +
    // closure environments); the validation token is rebuilt on the merged
    // program below.
    let mut library_parts: Vec<(MirProgram, Vec<MirClosureEnvironment>)> = Vec::new();
    for library in &links.libraries {
        let mut diagnostics = Vec::new();
        with_library_cached_analysis_mut(
            &library.import,
            library_resolver,
            library_cache,
            |analysis, cache| {
                if let Err(errors) = rewrite_analysis_namespace_calls(
                    &library.path,
                    analysis,
                    &links.calls,
                    &links.data_member_targets,
                    &links.namespaces,
                ) {
                    diagnostics.extend(errors);
                    return Ok(());
                }
                // S1 U2 nominal identity: build the canonical nominal remap
                // while the library analysis is freely borrowable (the entry
                // registered the library's file interface under the same
                // module identity during analysis).
                let cached_interface =
                    library_cached_file_interface(&library.import, library_resolver, cache).ok();
                let source_identity = cached_interface.as_ref().and_then(|interface| interface.identity.clone());
                let source_exports = cached_interface
                    .as_ref()
                    .map(exported_nominal_names)
                    .unwrap_or_default();
                let nominal_map = match build_nominal_remap(
                    &analysis.types,
                    &analysis.interner,
                    &analysis.resolver,
                    source_identity.as_ref(),
                    &source_exports,
                    &entry.analysis,
                ) {
                    Ok(map) => map,
                    Err(message) => {
                        diagnostics.push(mir_issue_diag(
                            &library.path,
                            "package_mir_nominal_identity_unknown",
                            message,
                        ));
                        return Ok(());
                    }
                };
                // S1 U2 VALUE members: variant definitions ride the same remap.
                let variant_remap = match build_variant_remap(
                    &analysis.interner,
                    &analysis.resolver,
                    source_identity.as_ref(),
                    &source_exports,
                    &entry.analysis,
                ) {
                    Ok(remap) => remap,
                    Err(message) => {
                        diagnostics.push(mir_issue_diag(
                            &library.path,
                            "package_mir_variant_identity_unknown",
                            message,
                        ));
                        return Ok(());
                    }
                };
                let bundle = match radix::driver::prepare_air_backward_bundle(analysis) {
                    Ok(bundle) => bundle,
                    Err(err) => {
                        diagnostics.push(mir_lowering_diag(&library.path, err.message));
                        return Ok(());
                    }
                };
                let mut lowered = match lower_analyzed_unit_with_context(analysis) {
                    Ok(lowered) => lowered,
                    Err(errors) => {
                        diagnostics.extend(
                            errors
                                .into_iter()
                                .map(|error| mir_lowering_diag(&library.path, error.message)),
                        );
                        return Ok(());
                    }
                };
                if let Some(bundle) = bundle {
                    if let Err(err) =
                        radix::driver::apply_air_backward_bundle(&mut lowered, bundle, no_fuse)
                    {
                        diagnostics.push(mir_lowering_diag(&library.path, err.message));
                        return Ok(());
                    }
                }
                remap_program_text_symbols(
                    &mut lowered.program,
                    &lowered.interner,
                    &mut entry.analysis.interner,
                );
                let nominal = NominalImportContext { nominal_map: &nominal_map };
                let source_to_entry_types = import_lowered_semantic_types(
                    lowered.validated.validation().types,
                    &mut entry.analysis.types,
                    Some(&nominal),
                );
                rewrite_lowered_type_ids(&mut lowered, &source_to_entry_types);
                rewrite_lowered_variant_defs(&mut lowered, &variant_remap);
                rewrite_program_sources(&mut lowered.program, &library.path, &source_rewrites);
                extend_unmapped_library_sources(
                    &lowered.program,
                    &library.path,
                    &mut source_rewrites,
                    &mut next_synthetic,
                );
                rewrite_program_sources(&mut lowered.program, &library.path, &source_rewrites);
                if let Err(errors) =
                    ensure_unique_definition_sources(&lowered.program, &library.path)
                {
                    diagnostics.extend(errors);
                    return Ok(());
                }
                library_parts.push((lowered.program, lowered.closure_environments));
                Ok(())
            },
        )
        .map_err(|diag| vec![diag])?;
        if !diagnostics.is_empty() {
            return Err(diagnostics);
        }
    }

    // S1 U3 reachability (default = pruning): linked library functions that
    // are unreachable from the package's synthetic call graph are dropped
    // before they merge into the program (delivery §4.3).
    prune_unreachable_library_functions(&mut library_parts, &mut reachable_seed);

    let mut merged = lower_unit(entry, &cli_plan.entry_records, no_fuse)?;
    ensure_unique_definition_sources(&merged.program, &entry_path)?;
    let mut dispatch_function = selected_cli_dispatch_function(
        cli_plan,
        &entry_path,
        &merged.program,
        &merged.interner,
        &merged.interner,
    );

    for mut unit in pending {
        if let Some(local_id) = unit.dispatch_function {
            let offset = merged.program.functions.len() as u32;
            dispatch_function = Some(MirFunctionId(local_id.0 + offset));
        }
        append_shifted_program(&mut merged, &mut unit.lowered);
        ensure_unique_definition_sources(&merged.program, &entry_path)?;
    }
    for (mut program, mut closure_environments) in library_parts {
        append_shifted_parts(&mut merged, &mut program, &mut closure_environments);
        ensure_unique_definition_sources(&merged.program, &entry_path)?;
    }
    rebuild_merged_validated(&mut merged, &entry_path)?;

    if cli_plan.dispatch.is_some() {
        let Some(function) = dispatch_function else {
            return Err(vec![mir_diag(
                &entry_path,
                "package MIR could not find selected CLI command function",
            )]);
        };
        install_cli_dispatch_entry(&mut merged, function, &entry_path)?;
    }

    Ok(merged)
}

/// Transplant linked const data members into the entry analysis before the
/// entry unit lowers (S1 data-member ABI).
///
/// Each linked const is re-interned into the entry interner and its type is
/// imported into the entry type table, then injected as a synthetic
/// `HirItemKind::Constant` item under the member's synthetic def id. The
/// entry lowering context then records it in `top_level_consts`, so rewritten
/// `Path(synthetic_def)` member references materialize through the existing
/// top-level-const seam.
fn inject_package_data_members(
    entry: &mut AnalyzedPackageUnit,
    before: &[AnalyzedPackageUnit],
    after: &[AnalyzedPackageUnit],
    links: &PackageMirLinks,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let entry_path = entry.path.clone();
    for member in &links.data_members {
        if member.source_path == entry_path {
            // Same-unit consts already lower through the entry's own
            // top-level-const seam; the linker never registers those.
            continue;
        }
        // The source analysis is borrowed from the sibling slices (disjoint
        // from `entry`) or the library cache; only the interner needs cloning
        // (the entry borrow must not conflict with the package-unit borrow).
        let source = before
            .iter()
            .chain(after.iter())
            .find(|unit| unit.path == member.source_path)
            .map(|unit| {
                (
                    unit.analysis.interner.clone(),
                    &unit.analysis.types,
                    &unit.analysis.resolver,
                    unit.file_interface.clone(),
                )
            })
            .or_else(|| {
                let library = links
                    .libraries
                    .iter()
                    .find(|library| library.path == member.source_path)?;
                let interface = library_cached_file_interface(
                    &library.import,
                    library_resolver,
                    library_cache,
                )
                .ok()?;
                let analysis = library_cached_analysis(&library.import, library_resolver, library_cache)
                    .ok()?;
                Some((analysis.interner.clone(), &analysis.types, &analysis.resolver, interface))
            });
        let Some((source_interner, source_types, source_resolver, source_interface)) = source else {
            diagnostics.push(mir_issue_diag(
                &entry_path,
                "package_mir_data_member_unsupported",
                format!(
                    "package MIR could not find const data member source unit `{}`",
                    member.source_path.display()
                ),
            ));
            continue;
        };
        let item = match transplant_data_member(
            member,
            &source_interner,
            source_types,
            source_resolver,
            &source_interface,
            entry,
        ) {
            Ok(item) => item,
            Err(message) => {
                diagnostics.push(mir_issue_diag(
                    &entry_path,
                    "package_mir_data_member_unsupported",
                    message,
                ));
                continue;
            }
        };
        entry.analysis.hir.items.push(item);
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

/// Build the synthetic `HirItemKind::Constant` item for one linked const,
/// re-interning the const name + initializer symbols into the entry interner
/// and importing the const (and initializer) types into the entry type table.
fn transplant_data_member(
    member: &DataMemberLink,
    source_interner: &Interner,
    source_types: &TypeTable,
    source_resolver: &Resolver,
    source_interface: &FileInterface,
    entry: &mut AnalyzedPackageUnit,
) -> Result<HirItem, String> {
    let mut imported = HashMap::new();
    let source_exports = exported_nominal_names(source_interface);
    // S1 U2 nominal identity: build the canonical nominal remap so const
    // types/initializers referencing library nominals unify with the entry's
    // semantic types (the linked source module's identity is the key).
    let nominal_map = build_nominal_remap(
        source_types,
        source_interner,
        source_resolver,
        source_interface.identity.as_ref(),
        &source_exports,
        &entry.analysis,
    )?;
    let nominal = NominalImportContext { nominal_map: &nominal_map };
    let konst = HirConst {
        name: remap_symbol(member.konst.name, source_interner, &mut entry.analysis.interner),
        ty: member
            .konst
            .ty
            .map(|ty| {
                import_semantic_type_with_nominal(
                    source_types,
                    &mut entry.analysis.types,
                    ty,
                    &mut imported,
                    Some(&nominal),
                )
            }),
        value: transplant_expr(
            &member.konst.value,
            source_interner,
            source_types,
            &mut entry.analysis.interner,
            &mut entry.analysis.types,
            &mut imported,
            Some(&nominal),
        )?,
        mutable: member.konst.mutable,
        is_await: member.konst.is_await,
    };
    Ok(HirItem {
        // The synthetic def id doubles as a unique item/expression id; no
        // entry HIR id range collides with the package-MIR synthetic range.
        id: HirId(member.synthetic.0),
        def_id: member.synthetic,
        kind: HirItemKind::Constant(konst),
        span: member.konst.value.span,
    })
}

/// Re-intern one text symbol into the target interner. Symbols outside the
/// source string table are compiler-generated identity tokens and stay as-is.
fn remap_symbol(symbol: Symbol, source: &Interner, target: &mut Interner) -> Symbol {
    remap_optional_symbol(Some(symbol), source, target).unwrap_or(symbol)
}

/// Transplant a const initializer expression into the entry analysis:
/// re-intern text symbols and import every semantic type. Expression shapes
/// that reference cross-analysis definitions (paths, calls, fields, struct
/// literals, …) fail closed — those are the nominal/value-member surfaces U2
/// carries, not the U1 const-value ABI.
fn transplant_expr(
    expr: &HirExpression,
    source_interner: &Interner,
    source_types: &TypeTable,
    entry_interner: &mut Interner,
    entry_types: &mut TypeTable,
    imported: &mut HashMap<TypeId, TypeId>,
    nominal: Option<&NominalImportContext<'_>>,
) -> Result<HirExpression, String> {
    let ty = expr
        .ty
        .map(|ty| import_semantic_type_with_nominal(source_types, entry_types, ty, imported, nominal));
    let kind = match &expr.kind {
        HirExpressionKind::Literal(literal) => {
            HirExpressionKind::Literal(transplant_literal(literal, source_interner, entry_interner))
        }
        HirExpressionKind::Vacua => HirExpressionKind::Vacua,
        HirExpressionKind::Binary(op, lhs, rhs) => HirExpressionKind::Binary(
            *op,
            Box::new(transplant_expr(lhs, source_interner, source_types, entry_interner, entry_types, imported, nominal)?),
            Box::new(transplant_expr(rhs, source_interner, source_types, entry_interner, entry_types, imported, nominal)?),
        ),
        HirExpressionKind::Unary(op, inner) => HirExpressionKind::Unary(
            *op,
            Box::new(transplant_expr(inner, source_interner, source_types, entry_interner, entry_types, imported, nominal)?),
        ),
        HirExpressionKind::Array(elements) => {
            let mut transplanted = Vec::with_capacity(elements.len());
            for element in elements {
                transplanted.push(match element {
                    radix::hir::HirArrayElement::Expr(element) => radix::hir::HirArrayElement::Expr(
                        transplant_expr(element, source_interner, source_types, entry_interner, entry_types, imported, nominal)?,
                    ),
                    radix::hir::HirArrayElement::Spread(element) => radix::hir::HirArrayElement::Spread(
                        transplant_expr(element, source_interner, source_types, entry_interner, entry_types, imported, nominal)?,
                    ),
                });
            }
            HirExpressionKind::Array(transplanted)
        }
        HirExpressionKind::Tuple(items, type_args) => {
            let mut transplanted = Vec::with_capacity(items.len());
            for item in items {
                transplanted.push(transplant_expr(
                    item,
                    source_interner,
                    source_types,
                    entry_interner,
                    entry_types,
                    imported,
                    nominal,
                )?);
            }
            let type_args = type_args
                .as_ref()
                .map(|args| {
                    args.iter()
                        .map(|ty| import_semantic_type_with_nominal(source_types, entry_types, *ty, imported, nominal))
                        .collect()
                });
            HirExpressionKind::Tuple(transplanted, type_args)
        }
        HirExpressionKind::Scriptum(template, items) => {
            let mut transplanted = Vec::with_capacity(items.len());
            for item in items {
                transplanted.push(transplant_expr(
                    item,
                    source_interner,
                    source_types,
                    entry_interner,
                    entry_types,
                    imported,
                    nominal,
                )?);
            }
            HirExpressionKind::Scriptum(
                remap_symbol(*template, source_interner, entry_interner),
                transplanted,
            )
        }
        HirExpressionKind::Scribe(kind, items) => {
            let mut transplanted = Vec::with_capacity(items.len());
            for item in items {
                transplanted.push(transplant_expr(
                    item,
                    source_interner,
                    source_types,
                    entry_interner,
                    entry_types,
                    imported,
                    nominal,
                )?);
            }
            HirExpressionKind::Scribe(*kind, transplanted)
        }
        _ => {
            return Err(
                "package MIR cannot transplant const data member initializer with unsupported expression shape"
                    .to_string(),
            );
        }
    };
    Ok(HirExpression {
        id: expr.id,
        kind,
        ty,
        span: expr.span,
    })
}

fn transplant_literal(
    literal: &HirLiteral,
    source_interner: &Interner,
    entry_interner: &mut Interner,
) -> HirLiteral {
    match literal {
        HirLiteral::String(symbol) => {
            HirLiteral::String(remap_symbol(*symbol, source_interner, entry_interner))
        }
        HirLiteral::Ascii(symbol) => {
            HirLiteral::Ascii(remap_symbol(*symbol, source_interner, entry_interner))
        }
        HirLiteral::Octeti(symbol) => {
            HirLiteral::Octeti(remap_symbol(*symbol, source_interner, entry_interner))
        }
        HirLiteral::Regex(pattern, flags) => HirLiteral::Regex(
            remap_symbol(*pattern, source_interner, entry_interner),
            flags
                .as_ref()
                .map(|flags| remap_symbol(*flags, source_interner, entry_interner)),
        ),
        HirLiteral::Int(value) => HirLiteral::Int(*value),
        HirLiteral::Float(value) => HirLiteral::Float(*value),
        HirLiteral::JsonValor(value) => HirLiteral::JsonValor(value.clone()),
        HirLiteral::Bool(value) => HirLiteral::Bool(*value),
        HirLiteral::Nil => HirLiteral::Nil,
    }
}

pub(super) fn selected_cli_dispatch_function(
    cli_plan: &CliPackagePlan,
    unit_path: &Path,
    program: &MirProgram,
    dispatch_interner: &Interner,
    program_interner: &Interner,
) -> Option<MirFunctionId> {
    let dispatch = cli_plan.dispatch.as_ref()?;
    if dispatch.unit_path != unit_path {
        return None;
    }
    // Compare by resolved text: the mounted-command symbol is interned in the
    // command unit's analysis, while lowered program names are re-interned
    // into the entry interner during package symbol remapping.
    let name_text = dispatch_interner.resolve(dispatch.function);
    program
        .functions
        .iter()
        .find(|candidate| {
            candidate
                .name
                .is_some_and(|name| program_interner.resolve(name) == name_text)
        })
        .map(|candidate| candidate.id)
}

pub(super) fn import_lowered_semantic_types(
    source: &TypeTable,
    target: &mut TypeTable,
    nominal: Option<&NominalImportContext<'_>>,
) -> Vec<(TypeId, TypeId)> {
    let mut imported = HashMap::new();
    let mut rewrites = Vec::new();
    for index in 0..source.type_count() {
        let source_ty = TypeId(index as u32);
        let target_ty = import_semantic_type_with_nominal(source, target, source_ty, &mut imported, nominal);
        push_type_rewrite(&mut rewrites, source_ty, target_ty);
    }
    rewrites
}

pub(super) fn imported_dispatch_type_rewrites(
    rewrite: &CliRecordTypeRewrite,
    imported: &[(TypeId, TypeId)],
) -> Vec<(TypeId, TypeId)> {
    rewrite
        .types
        .iter()
        .filter_map(|(from, to)| {
            let imported_from = imported
                .iter()
                .find_map(|(source, target)| (*source == *from).then_some(*target))
                .unwrap_or(*from);
            (imported_from != *to).then_some((imported_from, *to))
        })
        .collect()
}

pub(super) fn rewrite_lowered_type_ids(
    lowered: &mut LoweredMirUnit<'_>,
    rewrites: &[(TypeId, TypeId)],
) {
    for function in &mut lowered.program.functions {
        rewrite_type_id(&mut function.return_ty, rewrites);
        if let Some(error_ty) = &mut function.error_ty {
            rewrite_type_id(error_ty, rewrites);
        }
        for param in &mut function.params {
            rewrite_type_id(&mut param.ty, rewrites);
        }
        for local in &mut function.locals {
            rewrite_type_id(&mut local.ty, rewrites);
        }
        for temp in &mut function.temps {
            rewrite_type_id(&mut temp.ty, rewrites);
        }
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                rewrite_statement_type_id(statement, rewrites);
            }
        }
    }
    for environment in &mut lowered.closure_environments {
        rewrite_type_id(&mut environment.value_ty, rewrites);
        for capture in &mut environment.captures {
            rewrite_type_id(&mut capture.ty, rewrites);
        }
    }
}

pub(super) fn rewrite_type_id(ty: &mut radix::mir::MirType, rewrites: &[(TypeId, TypeId)]) {
    if let Some((_, to)) = rewrites.iter().find(|(from, _)| ty.semantic_id() == *from) {
        *ty = radix::mir::MirType::semantic(*to);
    }
}

pub(super) fn rewrite_statement_type_id(
    statement: &mut MirStatement,
    rewrites: &[(TypeId, TypeId)],
) {
    match &mut statement.kind {
        MirStatementKind::Assign { value, .. } => rewrite_value_type_id(value, rewrites),
        MirStatementKind::Call { .. } => {}
        MirStatementKind::RuntimeCall { call, .. } => {
            rewrite_type_id(&mut call.return_ty, rewrites);
        }
        MirStatementKind::Construct { aggregate, .. } => {
            rewrite_type_id(&mut aggregate.ty, rewrites);
        }
    }
}

pub(super) fn rewrite_value_type_id(value: &mut MirValue, rewrites: &[(TypeId, TypeId)]) {
    rewrite_type_id(&mut value.ty, rewrites);
}

/// Rewrite a lowered source program's enum-variant definitions to the
/// consumer analysis's variant identities (codex-gap S1 U2 VALUE members).
///
/// A library unit constructs and projects its variants under its own variant
/// `DefId`s; the merged program's variant metadata and the stepper's variant
/// tags are keyed by the *consumer* variant defs, so every
/// `EnumVariant`/`VariantField` reference in the lowered source program must
/// ride the nominal remap. Variant defs without a consumer mapping keep their
/// source-local value (entry-local types referenced from a sibling).
pub(super) fn rewrite_lowered_variant_defs(
    lowered: &mut LoweredMirUnit<'_>,
    variant_remap: &HashMap<DefId, DefId>,
) {
    if variant_remap.is_empty() {
        return;
    }
    for function in &mut lowered.program.functions {
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                rewrite_statement_variant_defs(statement, variant_remap);
            }
            rewrite_terminator_variant_defs(&mut block.terminator, variant_remap);
        }
    }
}

fn rewrite_statement_variant_defs(
    statement: &mut MirStatement,
    variant_remap: &HashMap<DefId, DefId>,
) {
    match &mut statement.kind {
        MirStatementKind::Assign { value, .. } => {
            rewrite_value_variant_defs(value, variant_remap);
        }
        MirStatementKind::Call { callee: _, args, .. } => {
            for arg in args {
                rewrite_operand_variant_defs(arg, variant_remap);
            }
        }
        MirStatementKind::RuntimeCall { call, .. } => {
            for arg in &mut call.args {
                rewrite_operand_variant_defs(arg, variant_remap);
            }
        }
        MirStatementKind::Construct { aggregate, .. } => {
            if let MirAggregateKind::EnumVariant(def_id) = &mut aggregate.kind {
                if let Some(rewritten) = variant_remap.get(def_id) {
                    aggregate.kind = MirAggregateKind::EnumVariant(*rewritten);
                }
            }
            rewrite_aggregate_variant_defs(aggregate, variant_remap);
        }
    }
}

fn rewrite_terminator_variant_defs(
    terminator: &mut MirTerminator,
    variant_remap: &HashMap<DefId, DefId>,
) {
    match &mut terminator.kind {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            rewrite_operand_variant_defs(operand, variant_remap);
        }
        MirTerminatorKind::TryCall {
            callee: _, args, ..
        } => {
            for arg in args {
                rewrite_operand_variant_defs(arg, variant_remap);
            }
        }
        MirTerminatorKind::Branch { condition, .. } => {
            rewrite_operand_variant_defs(condition, variant_remap);
        }
        MirTerminatorKind::Switch { value, .. } => {
            rewrite_operand_variant_defs(value, variant_remap);
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

fn rewrite_value_variant_defs(value: &mut MirValue, variant_remap: &HashMap<DefId, DefId>) {
    match &mut value.kind {
        MirValueKind::Operand(operand) => rewrite_operand_variant_defs(operand, variant_remap),
        MirValueKind::Closure(closure) => {
            rewrite_operand_variant_defs(&mut closure.environment, variant_remap);
        }
        MirValueKind::Unary { operand, .. } => {
            rewrite_operand_variant_defs(operand, variant_remap);
        }
        MirValueKind::Binary { lhs, rhs, .. } => {
            rewrite_operand_variant_defs(lhs, variant_remap);
            rewrite_operand_variant_defs(rhs, variant_remap);
        }
        MirValueKind::Option(op) => rewrite_option_variant_defs(op, variant_remap),
    }
}

fn rewrite_operand_variant_defs(operand: &mut MirOperand, variant_remap: &HashMap<DefId, DefId>) {
    if let MirOperand::Place(place) = operand {
        rewrite_place_variant_defs(place, variant_remap);
    }
}

fn rewrite_place_variant_defs(place: &mut MirPlace, variant_remap: &HashMap<DefId, DefId>) {
    for projection in &mut place.projections {
        match projection {
            MirProjection::VariantField { variant, .. } => {
                if let Some(rewritten) = variant_remap.get(variant) {
                    *variant = *rewritten;
                }
            }
            MirProjection::Index(operand) => {
                rewrite_operand_variant_defs(operand, variant_remap);
            }
            MirProjection::Field(_)
            | MirProjection::ClosureCapture { .. }
            | MirProjection::VectorLane(_)
            | MirProjection::MatrixCell { .. } => {}
        }
    }
}

fn rewrite_aggregate_variant_defs(
    aggregate: &mut MirAggregate,
    variant_remap: &HashMap<DefId, DefId>,
) {
    match &mut aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        rewrite_operand_variant_defs(operand, variant_remap);
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                rewrite_operand_variant_defs(&mut item.value, variant_remap);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                rewrite_operand_variant_defs(&mut item.key, variant_remap);
                rewrite_operand_variant_defs(&mut item.value, variant_remap);
            }
        }
    }
}

fn rewrite_option_variant_defs(op: &mut MirOptionOp, variant_remap: &HashMap<DefId, DefId>) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand)
        | MirOptionOp::Unwrap { value: operand, .. } => {
            rewrite_operand_variant_defs(operand, variant_remap);
        }
        MirOptionOp::Coalesce { value, fallback } => {
            rewrite_operand_variant_defs(value, variant_remap);
            rewrite_operand_variant_defs(fallback, variant_remap);
        }
        MirOptionOp::Chain { base, link } => {
            rewrite_operand_variant_defs(base, variant_remap);
            rewrite_option_chain_variant_defs(link, variant_remap);
        }
        MirOptionOp::None => {}
    }
}

fn rewrite_option_chain_variant_defs(
    link: &mut MirOptionChainLink,
    variant_remap: &HashMap<DefId, DefId>,
) {
    match link {
        MirOptionChainLink::VariantField { variant, .. } => {
            if let Some(rewritten) = variant_remap.get(variant) {
                *variant = *rewritten;
            }
        }
        MirOptionChainLink::Index(operand) => {
            rewrite_operand_variant_defs(operand, variant_remap);
        }
        MirOptionChainLink::Call { callee: _, args } => {
            for arg in args {
                rewrite_operand_variant_defs(arg, variant_remap);
            }
        }
        MirOptionChainLink::Field(_) => {}
    }
}

pub(super) fn install_cli_dispatch_entry(
    lowered: &mut LoweredMirUnit<'_>,
    command: MirFunctionId,
    entry_path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let Some(entry_index) = lowered.program.functions.iter().position(|function| {
        is_explicit_entry_function(function, lowered.validated.validation().types)
    }) else {
        return Err(vec![mir_diag(
            entry_path,
            "package MIR could not find root CLI entry function",
        )]);
    };
    let span = lowered.program.functions[entry_index].span;
    lowered.program.functions[entry_index].locals.clear();
    lowered.program.functions[entry_index].temps.clear();
    lowered.program.functions[entry_index].blocks = vec![MirBlock {
        id: MirBlockId(0),
        statements: vec![MirStatement {
            kind: MirStatementKind::Call {
                destination: None,
                callee: MirCallee::Function(command),
                args: Vec::new(),
            },
            span,
        }],
        terminator: MirTerminator {
            kind: MirTerminatorKind::Return(None),
            span,
        },
        span,
    }];
    Ok(())
}

pub(super) fn is_explicit_entry_function(
    function: &MirFunction,
    types: &radix::semantic::TypeTable,
) -> bool {
    function.source.is_none()
        && function.name.is_none()
        && function.params.is_empty()
        && matches!(
            types.get(function.return_ty.semantic_id()),
            Type::Primitive(Primitive::Vacuum)
        )
}

pub(super) fn lower_unit<'a>(
    unit: &'a mut AnalyzedPackageUnit,
    cli_entry_records: &CliEntryRecords,
    no_fuse: bool,
) -> Result<LoweredMirUnit<'a>, Vec<Diagnostic>> {
    let bundle = radix::driver::prepare_air_backward_bundle(&mut unit.analysis)
        .map_err(|err| vec![mir_lowering_diag(&unit.path, err.message)])?;

    let result = if unit.analysis.cli_program.is_some() {
        let fields = cli_entry_records
            .get(&unit.path)
            .cloned()
            .unwrap_or_default();
        lower_analyzed_unit_allowing_cli_runtime_records_with_context(&mut unit.analysis, fields)
    } else {
        lower_analyzed_unit_with_context(&mut unit.analysis)
    };
    let mut lowered = result.map_err(|errors| {
        errors
            .into_iter()
            .map(|error| mir_lowering_diag(&unit.path, error.message))
            .collect::<Vec<Diagnostic>>()
    })?;

    if let Some(bundle) = bundle {
        radix::driver::apply_air_backward_bundle(&mut lowered, bundle, no_fuse)
            .map_err(|err| vec![mir_lowering_diag(&unit.path, err.message)])?;
    }

    Ok(lowered)
}

pub(super) fn append_shifted_program(
    merged: &mut LoweredMirUnit<'_>,
    lowered: &mut LoweredMirUnit<'_>,
) {
    append_shifted_parts(
        merged,
        &mut lowered.program,
        &mut lowered.closure_environments,
    );
}

/// Append owned program parts (functions + closure environments) into the
/// merged program, shifting function and closure ids past the merged prefix.
pub(super) fn append_shifted_parts(
    merged: &mut LoweredMirUnit<'_>,
    program: &mut MirProgram,
    closure_environments: &mut Vec<MirClosureEnvironment>,
) {
    let offset = merged.program.functions.len() as u32;
    shift_program_ids(program, closure_environments, offset);
    // Closure environments are collected on the Vec and folded into a fresh
    // ValidatedMir token after all merges complete (rebuild_merged_validated).
    merged.closure_environments.append(closure_environments);
    merged.program.functions.append(&mut program.functions);
}

/// Rebuild the validated-MIR token after package merging shifts IDs and
/// appends functions/closure_environments. The prior token proved the
/// pre-merge program; the merged program needs its own proof.
pub(super) fn rebuild_merged_validated(
    merged: &mut LoweredMirUnit<'_>,
    diagnostic_path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let mut context = merged.validated.validation().clone();
    context.closure_environments.clear();
    for environment in &merged.closure_environments {
        context
            .closure_environments
            .insert(environment.id, environment.clone());
    }
    merged.validated =
        radix::mir::ValidatedMir::new(merged.program.clone(), context).map_err(|errors| {
            errors
                .into_iter()
                .map(|error| mir_lowering_diag(diagnostic_path, error.message))
                .collect::<Vec<_>>()
        })?;
    Ok(())
}

pub(super) fn ensure_unique_definition_sources(
    program: &MirProgram,
    path: &Path,
) -> Result<(), Vec<Diagnostic>> {
    let mut seen = HashSet::new();
    for function in &program.functions {
        let Some(source) = function.source else {
            continue;
        };
        if !seen.insert(source) {
            return Err(vec![mir_diag(
                path,
                format!(
                    "package MIR link found duplicate function source def#{}",
                    source.0
                ),
            )]);
        }
    }
    Ok(())
}

pub(super) fn shift_program_ids(
    program: &mut MirProgram,
    closure_environments: &mut [MirClosureEnvironment],
    offset: u32,
) {
    if offset == 0 {
        return;
    }
    for function in &mut program.functions {
        shift_function_id(&mut function.id, offset);
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                shift_statement_ids(statement, offset);
            }
            shift_terminator_ids(&mut block.terminator, offset);
        }
    }
    for environment in closure_environments {
        shift_environment_id(&mut environment.id, offset);
        shift_function_id(&mut environment.function, offset);
        for capture in &mut environment.captures {
            shift_place_ids(&mut capture.source_place, offset);
        }
    }
}

pub(super) fn shift_statement_ids(statement: &mut MirStatement, offset: u32) {
    match &mut statement.kind {
        MirStatementKind::Assign { place, value } => {
            shift_place_ids(place, offset);
            shift_value_ids(value, offset);
        }
        MirStatementKind::Call {
            destination,
            callee,
            args,
        } => {
            if let Some(destination) = destination {
                shift_place_ids(destination, offset);
            }
            shift_callee_ids(callee, offset);
            for arg in args {
                shift_operand_ids(arg, offset);
            }
        }
        MirStatementKind::RuntimeCall { destination, call } => {
            if let Some(destination) = destination {
                shift_place_ids(destination, offset);
            }
            shift_runtime_call_ids(call, offset);
        }
        MirStatementKind::Construct {
            destination,
            aggregate,
        } => {
            shift_place_ids(destination, offset);
            shift_aggregate_ids(aggregate, offset);
        }
    }
}

pub(super) fn shift_terminator_ids(terminator: &mut MirTerminator, offset: u32) {
    match &mut terminator.kind {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            shift_operand_ids(operand, offset);
        }
        MirTerminatorKind::TryCall {
            destination,
            callee,
            args,
            error_place,
            ..
        } => {
            if let Some(destination) = destination {
                shift_place_ids(destination, offset);
            }
            shift_callee_ids(callee, offset);
            for arg in args {
                shift_operand_ids(arg, offset);
            }
            shift_place_ids(error_place, offset);
        }
        MirTerminatorKind::Branch { condition, .. } => shift_operand_ids(condition, offset),
        MirTerminatorKind::Switch { value, cases, .. } => {
            shift_operand_ids(value, offset);
            for case in cases {
                shift_switch_case_ids(case, offset);
            }
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

pub(super) fn shift_value_ids(value: &mut MirValue, offset: u32) {
    match &mut value.kind {
        MirValueKind::Operand(operand) => shift_operand_ids(operand, offset),
        MirValueKind::Closure(closure) => shift_closure_value_ids(closure, offset),
        MirValueKind::Unary { operand, .. } => shift_operand_ids(operand, offset),
        MirValueKind::Binary { lhs, rhs, .. } => {
            shift_operand_ids(lhs, offset);
            shift_operand_ids(rhs, offset);
        }
        MirValueKind::Option(op) => shift_option_ids(op, offset),
    }
}

pub(super) fn shift_operand_ids(operand: &mut MirOperand, offset: u32) {
    match operand {
        MirOperand::Place(place) => shift_place_ids(place, offset),
        MirOperand::Constant(constant) => shift_constant_ids(constant, offset),
        MirOperand::Temp(_) | MirOperand::Value(_) => {}
    }
}

pub(super) fn shift_place_ids(place: &mut MirPlace, offset: u32) {
    for projection in &mut place.projections {
        match projection {
            MirProjection::ClosureCapture { environment, .. } => {
                shift_environment_id(environment, offset)
            }
            MirProjection::Index(operand) => shift_operand_ids(operand, offset),
            MirProjection::Field(_)
            | MirProjection::VariantField { .. }
            | MirProjection::VectorLane(_)
            | MirProjection::MatrixCell { .. } => {}
        }
    }
}

pub(super) fn shift_constant_ids(constant: &mut MirConstant, offset: u32) {
    if let MirConstant::Function(id) = constant {
        shift_function_id(id, offset);
    }
}

pub(super) fn shift_callee_ids(callee: &mut MirCallee, offset: u32) {
    match callee {
        MirCallee::Function(id) => shift_function_id(id, offset),
        MirCallee::Closure(closure) => shift_closure_callee_ids(closure, offset),
        MirCallee::Value(operand) => shift_operand_ids(operand, offset),
        MirCallee::Definition { .. } => {}
    }
}

pub(super) fn shift_runtime_call_ids(call: &mut MirRuntimeCall, offset: u32) {
    for arg in &mut call.args {
        shift_operand_ids(arg, offset);
    }
}

pub(super) fn shift_aggregate_ids(aggregate: &mut MirAggregate, offset: u32) {
    match &mut aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        shift_operand_ids(operand, offset);
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                shift_operand_ids(&mut item.value, offset);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                shift_operand_ids(&mut item.key, offset);
                shift_operand_ids(&mut item.value, offset);
            }
        }
    }
}

pub(super) fn shift_switch_case_ids(case: &mut MirSwitchCase, offset: u32) {
    shift_constant_ids(&mut case.value, offset);
}

pub(super) fn shift_option_ids(op: &mut MirOptionOp, offset: u32) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand)
        | MirOptionOp::Unwrap { value: operand, .. } => shift_operand_ids(operand, offset),
        MirOptionOp::Coalesce { value, fallback } => {
            shift_operand_ids(value, offset);
            shift_operand_ids(fallback, offset);
        }
        MirOptionOp::Chain { base, link } => {
            shift_operand_ids(base, offset);
            shift_option_chain_link_ids(link, offset);
        }
        MirOptionOp::None => {}
    }
}

pub(super) fn shift_option_chain_link_ids(link: &mut MirOptionChainLink, offset: u32) {
    match link {
        MirOptionChainLink::Field(_) | MirOptionChainLink::VariantField { .. } => {}
        MirOptionChainLink::Index(operand) => shift_operand_ids(operand, offset),
        MirOptionChainLink::Call { callee, args } => {
            shift_callee_ids(callee, offset);
            for arg in args {
                shift_operand_ids(arg, offset);
            }
        }
    }
}

pub(super) fn shift_closure_value_ids(closure: &mut MirClosureValue, offset: u32) {
    shift_function_id(&mut closure.function, offset);
    shift_environment_id(&mut closure.environment_id, offset);
    shift_operand_ids(&mut closure.environment, offset);
}

pub(super) fn shift_closure_callee_ids(closure: &mut MirClosureCallee, offset: u32) {
    shift_function_id(&mut closure.function, offset);
    shift_environment_id(&mut closure.environment_id, offset);
    shift_operand_ids(&mut closure.environment, offset);
}

pub(super) fn shift_function_id(id: &mut MirFunctionId, offset: u32) {
    id.0 += offset;
}

pub(super) fn shift_environment_id(id: &mut MirClosureEnvironmentId, offset: u32) {
    id.0 += offset;
}

// ── S1 U3 reachability: linked library functions are lowered only when
//    reachable from the package's synthetic call graph (default = pruning) ──

/// Collect every package-MIR synthetic def id referenced by the package's
/// rewritten unit HIR. The linker installed these `Path(synthetic)` targets
/// for used namespace calls (library exports) and const data members, so the
/// reference set is exactly the entry's call-graph seeds into the linked
/// libraries.
fn collect_package_synthetic_references(package: &AnalyzedPackage) -> HashSet<DefId> {
    let mut references = HashSet::new();
    for unit in &package.units {
        let mut collector = SyntheticReferenceCollector::default();
        radix::hir::visit::walk_program(&mut collector, &unit.analysis.hir);
        references.extend(collector.references);
    }
    references
}

#[derive(Default)]
struct SyntheticReferenceCollector {
    references: HashSet<DefId>,
}

impl radix::hir::visit::HirVisitor for SyntheticReferenceCollector {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Path(def_id) = &expr.kind {
            if def_id.0 >= PACKAGE_MIR_SYNTHETIC_DEF_BASE {
                self.references.insert(*def_id);
            }
        }
        radix::hir::visit::walk_expr(self, expr);
    }
}

/// Drop linked-library functions unreachable from the package's synthetic
/// call graph (S1 U3 reachability, default = pruning).
///
/// The reachable set is a global fixpoint over the linked library programs:
/// the package units' synthetic references seed the closure, each library's
/// BFS follows intra-program function edges (direct callees, closure bodies,
/// function constants) and `MirCallee::Definition { source }` edges (calls
/// into sibling libraries), and newly discovered definition sources extend
/// the seed until no library gains a function. Closure environments
/// referenced by kept functions are kept; unreferenced ones are dropped
/// alongside their (pruned) functions.
fn prune_unreachable_library_functions(
    library_parts: &mut Vec<(MirProgram, Vec<MirClosureEnvironment>)>,
    reachable_seed: &mut HashSet<DefId>,
) {
    if library_parts.is_empty() {
        return;
    }
    loop {
        let mut new_seed = HashSet::new();
        let mut keep_sets = Vec::with_capacity(library_parts.len());
        for (program, _) in library_parts.iter() {
            let (reachable, definitions) = reachable_library_function_set(program, reachable_seed);
            keep_sets.push(reachable);
            new_seed.extend(definitions);
        }
        let grew = new_seed
            .iter()
            .any(|definition| !reachable_seed.contains(definition));
        reachable_seed.extend(new_seed);
        if grew {
            continue;
        }
        // Final keep sets (computed against the settled seed): filter each
        // library program to its reachable functions and keep only the
        // closure environments those functions reference.
        for ((program, environments), keep) in library_parts.iter_mut().zip(keep_sets.iter()) {
            let mut referenced_environments = HashSet::new();
            for function_id in keep.iter().copied() {
                referenced_environments
                    .extend(scan_mir_function_references(program, function_id).environments);
            }
            program.functions.retain(|function| keep.contains(&function.id));
            environments.retain(|environment| referenced_environments.contains(&environment.id));
        }
        break;
    }
}

/// Compute the reachable function set of one lowered library program from the
/// current synthetic-def seed, plus the `MirCallee::Definition` sources those
/// functions reference (cross-library edges for the fixpoint).
fn reachable_library_function_set(
    program: &MirProgram,
    reachable_seed: &HashSet<DefId>,
) -> (HashSet<MirFunctionId>, Vec<DefId>) {
    let mut work: Vec<MirFunctionId> = Vec::new();
    for function in &program.functions {
        if let Some(source) = function.source {
            if reachable_seed.contains(&source) {
                work.push(function.id);
            }
        }
    }
    let mut reachable = HashSet::new();
    let mut definitions = Vec::new();
    let mut seen_definitions = HashSet::new();
    while let Some(function_id) = work.pop() {
        if !reachable.insert(function_id) {
            continue;
        }
        let scan = scan_mir_function_references(program, function_id);
        work.extend(scan.functions);
        for definition in scan.definitions {
            if seen_definitions.insert(definition) {
                definitions.push(definition);
            }
        }
    }
    (reachable, definitions)
}

/// Function, definition, and closure-environment references made by one MIR
/// function body (S1 U3 reachability edges).
#[derive(Default)]
struct MirReferenceScan {
    functions: Vec<MirFunctionId>,
    definitions: Vec<DefId>,
    environments: Vec<MirClosureEnvironmentId>,
}

fn scan_mir_function_references(
    program: &MirProgram,
    function_id: MirFunctionId,
) -> MirReferenceScan {
    let mut scan = MirReferenceScan::default();
    let Some(function) = program
        .functions
        .iter()
        .find(|function| function.id == function_id)
    else {
        return scan;
    };
    for block in &function.blocks {
        for statement in &block.statements {
            scan_statement_references(statement, &mut scan);
        }
        scan_terminator_references(&block.terminator, &mut scan);
    }
    scan
}

fn scan_statement_references(statement: &MirStatement, scan: &mut MirReferenceScan) {
    match &statement.kind {
        MirStatementKind::Assign { value, .. } => scan_value_references(value, scan),
        MirStatementKind::Call { callee, args, .. } => {
            scan_callee_references(callee, scan);
            for arg in args {
                scan_operand_references(arg, scan);
            }
        }
        MirStatementKind::RuntimeCall { call, .. } => {
            for arg in &call.args {
                scan_operand_references(arg, scan);
            }
        }
        MirStatementKind::Construct { aggregate, .. } => {
            scan_aggregate_references(aggregate, scan);
        }
    }
}

fn scan_terminator_references(terminator: &MirTerminator, scan: &mut MirReferenceScan) {
    match &terminator.kind {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            scan_operand_references(operand, scan);
        }
        MirTerminatorKind::TryCall { callee, args, .. } => {
            scan_callee_references(callee, scan);
            for arg in args {
                scan_operand_references(arg, scan);
            }
        }
        MirTerminatorKind::Branch { condition, .. } => {
            scan_operand_references(condition, scan);
        }
        MirTerminatorKind::Switch { value, cases, .. } => {
            scan_operand_references(value, scan);
            for case in cases {
                scan_constant_references(&case.value, scan);
            }
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => {}
    }
}

fn scan_value_references(value: &MirValue, scan: &mut MirReferenceScan) {
    match &value.kind {
        MirValueKind::Operand(operand) => scan_operand_references(operand, scan),
        MirValueKind::Closure(closure) => {
            scan.functions.push(closure.function);
            scan.environments.push(closure.environment_id);
            scan_operand_references(&closure.environment, scan);
        }
        MirValueKind::Unary { operand, .. } => scan_operand_references(operand, scan),
        MirValueKind::Binary { lhs, rhs, .. } => {
            scan_operand_references(lhs, scan);
            scan_operand_references(rhs, scan);
        }
        MirValueKind::Option(op) => scan_option_references(op, scan),
    }
}

fn scan_operand_references(operand: &MirOperand, scan: &mut MirReferenceScan) {
    match operand {
        MirOperand::Place(place) => scan_place_references(place, scan),
        MirOperand::Constant(constant) => scan_constant_references(constant, scan),
        MirOperand::Temp(_) | MirOperand::Value(_) => {}
    }
}

fn scan_place_references(place: &MirPlace, scan: &mut MirReferenceScan) {
    for projection in &place.projections {
        match projection {
            MirProjection::ClosureCapture { environment, .. } => {
                scan.environments.push(*environment);
            }
            MirProjection::Index(operand) => scan_operand_references(operand, scan),
            MirProjection::Field(_)
            | MirProjection::VariantField { .. }
            | MirProjection::VectorLane(_)
            | MirProjection::MatrixCell { .. } => {}
        }
    }
}

fn scan_constant_references(constant: &MirConstant, scan: &mut MirReferenceScan) {
    if let MirConstant::Function(function_id) = constant {
        scan.functions.push(*function_id);
    }
}

fn scan_callee_references(callee: &MirCallee, scan: &mut MirReferenceScan) {
    match callee {
        MirCallee::Function(function_id) => scan.functions.push(*function_id),
        MirCallee::Closure(closure) => {
            scan.functions.push(closure.function);
            scan.environments.push(closure.environment_id);
            scan_operand_references(&closure.environment, scan);
        }
        MirCallee::Value(operand) => scan_operand_references(operand, scan),
        MirCallee::Definition { source, .. } => scan.definitions.push(*source),
    }
}

fn scan_aggregate_references(aggregate: &MirAggregate, scan: &mut MirReferenceScan) {
    if let MirAggregateKind::ClosureEnvironment(environment_id) = &aggregate.kind {
        scan.environments.push(*environment_id);
    }
    match &aggregate.fields {
        MirAggregateFields::Ordered(items) => {
            for item in items {
                match item {
                    MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                        scan_operand_references(operand, scan);
                    }
                }
            }
        }
        MirAggregateFields::Named(items) => {
            for item in items {
                scan_operand_references(&item.value, scan);
            }
        }
        MirAggregateFields::Keyed(items) => {
            for item in items {
                scan_operand_references(&item.key, scan);
                scan_operand_references(&item.value, scan);
            }
        }
    }
}

fn scan_option_references(op: &MirOptionOp, scan: &mut MirReferenceScan) {
    match op {
        MirOptionOp::Some(operand)
        | MirOptionOp::IsNil(operand)
        | MirOptionOp::IsNotNil(operand)
        | MirOptionOp::Unwrap { value: operand, .. } => scan_operand_references(operand, scan),
        MirOptionOp::Coalesce { value, fallback } => {
            scan_operand_references(value, scan);
            scan_operand_references(fallback, scan);
        }
        MirOptionOp::Chain { base, link } => {
            scan_operand_references(base, scan);
            scan_option_chain_references(link, scan);
        }
        MirOptionOp::None => {}
    }
}

fn scan_option_chain_references(link: &MirOptionChainLink, scan: &mut MirReferenceScan) {
    match link {
        MirOptionChainLink::Field(_) | MirOptionChainLink::VariantField { .. } => {}
        MirOptionChainLink::Index(operand) => scan_operand_references(operand, scan),
        MirOptionChainLink::Call { callee, args } => {
            scan_callee_references(callee, scan);
            for arg in args {
                scan_operand_references(arg, scan);
            }
        }
    }
}
