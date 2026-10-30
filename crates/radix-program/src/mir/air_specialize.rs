//! Package-level concrete AIR instance discovery.
//!
//! Single-module semantic analysis can see direct calls in its own HIR. A
//! package caller sees an imported namespace call until the package linker
//! rewrites it, so the provider's AIR template needs a second, package-owned
//! discovery step before per-unit MIR lowering. This module discovers both
//! sibling source-package calls and calls into linked-library generic AIR
//! targets: library bodies are analyzed HIR in the library interface cache,
//! so their concrete instances materialize in the cached library analysis at
//! link time and the calls retarget to package synthetic instance identities.

use super::{
    AnalyzedProgram, BTreeMap, DefId, Diagnostic, DiagnosticPhase, HashMap, LibraryImportBinding,
    LibraryInterfaceCache, LibraryResolver, PackageMirLinks, Path, PathBuf, TypeId,
    library_cached_analysis, with_library_cached_analysis_mut,
};
use radix_module::DiagnosticConvert;
use radix_module::hir::visit::{HirVisitor, HirVisitorMut, walk_expr, walk_expr_mut, walk_program};
use radix_module::hir::{HirExpression, HirExpressionKind, HirItemKind, HirTypeParam};
use radix_module::semantic::{IndexExpr, IndexId, RadixLane, ResolvedGenericBinding, Type};
use radix_module::syntax::GenericParamKind;
use radix_types::{GenericWitness, TypeTable};

#[derive(Clone)]
struct NamespaceGenericCall {
    caller_path: PathBuf,
    call_span: radix_module::lexer::Span,
    callee_span: radix_module::lexer::Span,
    old_target: DefId,
    target_path: PathBuf,
    target_def: DefId,
    target_params: Vec<HirTypeParam>,
    resolved: Option<radix_types::ResolvedGenericCall>,
    explicit_args: Vec<TypeId>,
    caller_types: radix_types::TypeTableSnapshot,
    caller_interner: radix_module::lexer::Interner,
    /// Owning linked-library import when the target is a linked-library body
    /// (`None` for in-package sibling targets).
    library: Option<LibraryImportBinding>,
    /// The generic AIR primal template when this call targets a declared
    /// `@ radix backward` companion (`None` for direct template calls).
    companion_primal: Option<DefId>,
}

/// Discover package namespace calls that target generic AIR functions (sibling
/// units and linked libraries), materialize their concrete provider-owned
/// instances, and assign each instance a package synthetic source identity.
pub(super) fn plan_package_instances(
    package: &mut AnalyzedProgram,
    links: &mut PackageMirLinks,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Result<(), Vec<Diagnostic>> {
    let calls = discover_calls(package, links, library_resolver, library_cache);
    if calls.is_empty() {
        return Ok(());
    }

    let mut grouped = BTreeMap::<PathBuf, Vec<NamespaceGenericCall>>::new();
    let mut errors = Vec::new();
    for call in calls {
        if !request_for_call(&call) {
            errors.push(missing_instance_diagnostic(
                &call.target_path,
                if call.library.is_some() {
                    "generic_air_library_instance_missing_concrete_binding"
                } else {
                    "generic_air_instance_missing_concrete_binding"
                },
            ));
            continue;
        }
        grouped
            .entry(call.target_path.clone())
            .or_default()
            .push(call);
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    for (target_path, calls) in grouped {
        if calls.first().is_some_and(|call| call.library.is_some()) {
            plan_linked_library_instances(
                &target_path,
                &calls,
                links,
                library_resolver,
                library_cache,
                &mut errors,
            );
            continue;
        }
        plan_sibling_target_instances(&target_path, &calls, package, links, &mut errors);
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Materialize concrete provider-owned instances for one in-package sibling
/// target and register every instance under a package synthetic source identity.
fn plan_sibling_target_instances(
    target_path: &Path,
    calls: &[NamespaceGenericCall],
    package: &mut AnalyzedProgram,
    links: &mut PackageMirLinks,
    errors: &mut Vec<Diagnostic>,
) {
    let Some((_, target)) = package.node_by_path_mut(target_path) else {
        return;
    };
    let mut requests = Vec::with_capacity(calls.len());
    let mut keys = Vec::with_capacity(calls.len());
    for call in calls {
        let Some(bindings) = request_for_target(call, &mut target.analysis.types) else {
            errors.push(missing_instance_diagnostic(
                &call.target_path,
                "generic_air_instance_type_rehome_failed",
            ));
            continue;
        };
        keys.push(
            radix_module::semantic::passes::air_specialize::AirInstanceKey {
                template: call.target_def,
                witnesses: bindings.iter().map(|binding| binding.witness).collect(),
            },
        );
        requests.push(
            radix_module::semantic::passes::air_specialize::AirSpecializationRequest {
                template: call.target_def,
                bindings,
            },
        );
    }
    if !errors.is_empty() {
        return;
    }
    if requests.is_empty() {
        return;
    }

    let report = radix_module::semantic::passes::air_specialize::specialize_requested(
        &mut target.analysis.hir,
        &mut target.analysis.radix_lanes,
        &mut target.analysis.types,
        &mut target.analysis.interner,
        &requests,
    );
    let instance_by_key = report
        .instance_defs
        .iter()
        .cloned()
        .collect::<HashMap<_, _>>();
    for key in &keys {
        if !instance_by_key.contains_key(key) {
            errors.push(missing_instance_diagnostic(
                target_path,
                "generic_air_instance_materialization_failed",
            ));
        }
    }
    if !errors.is_empty() {
        return;
    }

    run_concrete_gates(target_path, &mut target.analysis, errors);

    for (_, instance) in report.instance_defs {
        let synthetic = DefId(links.next_synthetic);
        links.next_synthetic = links.next_synthetic.saturating_add(1);
        links
            .sources
            .insert((target_path.to_path_buf(), instance), synthetic);
    }
    for (call, key) in calls.iter().zip(keys) {
        let Some(instance) = instance_by_key.get(&key).copied() else {
            continue;
        };
        let Some(synthetic) = links
            .sources
            .get(&(target_path.to_path_buf(), instance))
            .copied()
        else {
            continue;
        };
        links.air_calls.insert(
            (
                call.caller_path.clone(),
                call.call_span.start,
                call.call_span.end,
                call.old_target,
            ),
            synthetic,
        );
    }
}

/// Run the concrete-only unroll and AIR-purity gates over an analysis whose
/// ordinary semantic pipeline has already completed.
fn run_concrete_gates(
    target_path: &Path,
    analysis: &mut radix_module::driver::AnalyzedModule,
    errors: &mut Vec<Diagnostic>,
) {
    if let Err(instance_errors) = radix_module::semantic::passes::air_unroll::unroll(
        &mut analysis.hir,
        &mut analysis.presentation,
        &analysis.radix_lanes,
        &mut analysis.interner,
        &analysis.types,
    ) {
        errors.extend(instance_errors.into_iter().map(|error| {
            Diagnostic::from_semantic_error(&target_path.display().to_string(), "", &error)
        }));
    }
    if let Err(instance_errors) = radix_module::semantic::passes::air_purity::check(
        &mut analysis.hir,
        &mut analysis.radix_lanes,
        &mut analysis.interner,
        &analysis.types,
    ) {
        errors.extend(instance_errors.into_iter().map(|error| {
            Diagnostic::from_semantic_error(&target_path.display().to_string(), "", &error)
        }));
    }
}

/// Apply the call-site-specific synthetic identities after the normal
/// namespace rewrite has converted `module.fn(...)` to `Path(old_synthetic)`.
pub(super) fn rewrite_package_instance_calls(
    package: &mut AnalyzedProgram,
    targets: &HashMap<(PathBuf, u32, u32, DefId), DefId>,
) {
    if targets.is_empty() {
        return;
    }
    for unit in package.nodes.values_mut() {
        let mut rewriter = InstanceCallRewriter {
            unit_path: &unit.path,
            targets,
        };
        rewriter.visit_program_mut(&mut unit.analysis.hir);
    }
}

/// One materialized concrete instance in a linked library's cached analysis.
struct LibraryInstancePlan {
    /// Instance key → concrete HIR instance def in the library analysis.
    instance_by_key: HashMap<radix_module::semantic::passes::air_specialize::AirInstanceKey, DefId>,
    /// Instance def → its generated backward companion def (present when the
    /// template carried a `@ radix backward` row).
    companions: HashMap<DefId, DefId>,
    /// The call rows the plan was requested for, paired with their resolved
    /// instance keys (owned clones; the closure must not borrow the caller's
    /// call vector).
    planned_calls: Vec<(
        radix_module::semantic::passes::air_specialize::AirInstanceKey,
        NamespaceGenericCall,
    )>,
}

/// Materialize concrete instances for package calls into one linked library's
/// generic AIR templates, inside that library's cached analysis.
///
/// The library body is analyzed HIR in the interface cache, so the same
/// provider-owned specialization contract as the sibling path applies: the
/// callee's module stays the owner of substitution, named unroll, and AIR
/// purity, and each materialized instance (plus its backward companion, when
/// the template declares one) receives a package synthetic source identity
/// the caller's calls are retargeted to.
fn plan_linked_library_instances(
    target_path: &Path,
    calls: &[NamespaceGenericCall],
    links: &mut PackageMirLinks,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
    errors: &mut Vec<Diagnostic>,
) {
    let Some(import) = calls.first().and_then(|call| call.library.clone()) else {
        return;
    };

    let mut semantic_errors = Vec::new();
    let result = with_library_cached_analysis_mut(
        &import,
        library_resolver,
        library_cache,
        |analysis, _| {
            Ok(plan_instances_in_library_analysis(
                target_path,
                calls,
                analysis,
                &mut semantic_errors,
            ))
        },
    );

    let plan = match result {
        Ok(plan) => plan,
        Err(diagnostic) => {
            errors.push(diagnostic);
            return;
        }
    };
    if !semantic_errors.is_empty() {
        errors.extend(semantic_errors);
        return;
    }

    let mut materialization_missing = false;
    for (key, _) in &plan.planned_calls {
        if !plan.instance_by_key.contains_key(key) {
            errors.push(missing_instance_diagnostic(
                target_path,
                "generic_air_library_instance_materialization_failed",
            ));
            materialization_missing = true;
        }
    }
    if materialization_missing {
        return;
    }

    // Register every materialized instance (including nested-call instances
    // the specialization pass discovered inside the library) and each
    // instance's backward companion under package synthetic source identities.
    // Synthetic ids are handed out in walk order, so walk the std `HashMap`s
    // in `DefId` order, never in their per-process seeded order.
    let mut instances: Vec<DefId> = plan.instance_by_key.values().copied().collect();
    instances.sort_by_key(|instance| instance.0);
    let mut synthetic_of = HashMap::new();
    for instance in &instances {
        let synthetic = DefId(links.next_synthetic);
        links.next_synthetic = links.next_synthetic.saturating_add(1);
        links
            .sources
            .insert((target_path.to_path_buf(), *instance), synthetic);
        synthetic_of.insert(*instance, synthetic);
    }
    let mut companion_synthetic_of = HashMap::new();
    let mut companions: Vec<(DefId, DefId)> =
        plan.companions.iter().map(|(i, c)| (*i, *c)).collect();
    companions.sort_by_key(|(instance, _)| instance.0);
    for (instance, companion) in &companions {
        let synthetic = DefId(links.next_synthetic);
        links.next_synthetic = links.next_synthetic.saturating_add(1);
        links
            .sources
            .insert((target_path.to_path_buf(), *companion), synthetic);
        companion_synthetic_of.insert(*instance, synthetic);
    }

    for (key, call) in plan.planned_calls {
        let Some(instance) = plan.instance_by_key.get(&key).copied() else {
            continue;
        };
        let target = if call.companion_primal.is_some() {
            if !plan.companions.contains_key(&instance) {
                errors.push(missing_instance_diagnostic(
                    target_path,
                    "generic_air_library_instance_companion_missing",
                ));
                continue;
            }
            companion_synthetic_of[&instance]
        } else {
            synthetic_of[&instance]
        };
        links.air_calls.insert(
            (
                call.caller_path.clone(),
                call.call_span.start,
                call.call_span.end,
                call.old_target,
            ),
            target,
        );
    }
}

/// Materialize the requested concrete instances inside a linked library's
/// cached analysis and collect any concrete-only gate errors.
fn plan_instances_in_library_analysis(
    target_path: &Path,
    calls: &[NamespaceGenericCall],
    analysis: &mut radix_module::driver::AnalyzedModule,
    semantic_errors: &mut Vec<Diagnostic>,
) -> LibraryInstancePlan {
    let mut requests = Vec::with_capacity(calls.len());
    let mut planned_calls = Vec::with_capacity(calls.len());
    for call in calls {
        let Some(bindings) = request_for_target(call, &mut analysis.types) else {
            semantic_errors.push(missing_instance_diagnostic(
                target_path,
                "generic_air_library_instance_type_rehome_failed",
            ));
            continue;
        };
        let template = call.companion_primal.unwrap_or(call.target_def);
        planned_calls.push((
            radix_module::semantic::passes::air_specialize::AirInstanceKey {
                template,
                witnesses: bindings.iter().map(|binding| binding.witness).collect(),
            },
            call.clone(),
        ));
        requests.push(
            radix_module::semantic::passes::air_specialize::AirSpecializationRequest {
                template,
                bindings,
            },
        );
    }
    if !semantic_errors.is_empty() || requests.is_empty() {
        return LibraryInstancePlan {
            instance_by_key: HashMap::new(),
            companions: HashMap::new(),
            planned_calls,
        };
    }

    let report = radix_module::semantic::passes::air_specialize::specialize_requested(
        &mut analysis.hir,
        &mut analysis.radix_lanes,
        &mut analysis.types,
        &mut analysis.interner,
        &requests,
    );

    // The library's ordinary semantic pipeline has already run; the newly
    // materialized clones must pass the same concrete-only gates before the
    // package lowers the library unit to AIR/MIR.
    run_concrete_gates(target_path, analysis, semantic_errors);

    let mut companions = HashMap::new();
    for (_, instance) in &report.instance_defs {
        if let Some(backward) = analysis.radix_lanes.backward_for(*instance) {
            companions.insert(*instance, backward.companion_def_id);
        }
    }
    LibraryInstancePlan {
        instance_by_key: report.instance_defs.iter().cloned().collect(),
        companions,
        planned_calls,
    }
}

fn discover_calls(
    package: &AnalyzedProgram,
    links: &PackageMirLinks,
    library_resolver: &LibraryResolver,
    library_cache: &mut LibraryInterfaceCache,
) -> Vec<NamespaceGenericCall> {
    let mut calls = Vec::new();
    for unit in package.nodes.values() {
        let mut collector = NamespaceCallCollector {
            unit_path: &unit.path,
            interner: &unit.analysis.interner,
            links,
            calls: Vec::new(),
        };
        walk_program(&mut collector, &unit.analysis.hir);
        for raw in collector.calls {
            let Some((target_path, target_def)) =
                links.sources.iter().find_map(|((path, def), synthetic)| {
                    (*synthetic == raw.old_target).then(|| (path.clone(), *def))
                })
            else {
                continue;
            };
            let mut library = None;
            let mut companion_primal = None;
            let target_params = if let Some(target_node) =
                package.node_by_path(&target_path).map(|(_, node)| node)
            {
                let Some(target_params) = generic_air_params(
                    &target_node.analysis.hir,
                    &target_node.analysis.radix_lanes,
                    target_def,
                ) else {
                    continue;
                };
                target_params
            } else {
                // A linked-library body: classified against its cached
                // analysis (already resident from the link pass).
                let Some(target) = links
                    .libraries
                    .iter()
                    .find(|library| library.path == target_path)
                    .map(|library| library.import.clone())
                else {
                    continue;
                };
                let Ok(analysis) =
                    library_cached_analysis(&target, library_resolver, library_cache)
                else {
                    continue;
                };
                let params = if let Some(params) =
                    generic_air_params(&analysis.hir, &analysis.radix_lanes, target_def)
                {
                    params
                } else {
                    // Not a template call: a declared `@ radix
                    // backward` companion of a generic AIR template
                    // plans through its primal's instance.
                    let Some(primal) = declared_companion_primal(&analysis.radix_lanes, target_def)
                    else {
                        continue;
                    };
                    let Some(params) =
                        generic_air_params(&analysis.hir, &analysis.radix_lanes, primal)
                    else {
                        continue;
                    };
                    companion_primal = Some(primal);
                    params
                };
                library = Some(target);
                params
            };
            let resolved = unit
                .analysis
                .types
                .resolved_generic_calls
                .iter()
                .find(|row| row.call_span == raw.call_span || row.call_span == raw.callee_span)
                .cloned();
            calls.push(NamespaceGenericCall {
                caller_path: unit.path.clone(),
                call_span: raw.call_span,
                callee_span: raw.callee_span,
                old_target: raw.old_target,
                target_path,
                target_def,
                target_params,
                resolved,
                explicit_args: raw.explicit_args,
                caller_types: unit.analysis.types.snapshot(),
                caller_interner: unit.analysis.interner.clone(),
                library,
                companion_primal,
            });
        }
    }
    calls
}

/// The generic AIR primal owning a declared backward companion def, when the
/// companion's own `DefId` names a `@ radix backward` row.
fn declared_companion_primal(
    lanes: &radix_module::semantic::RadixLaneMetadata,
    companion_def: DefId,
) -> Option<DefId> {
    lanes.iter_backward().find_map(|(primal, backward)| {
        (backward.companion_def_id == companion_def).then_some(primal)
    })
}

#[derive(Clone)]
pub(super) struct RawNamespaceCall {
    pub(super) call_span: radix_module::lexer::Span,
    pub(super) callee_span: radix_module::lexer::Span,
    pub(super) old_target: DefId,
    pub(super) explicit_args: Vec<TypeId>,
}

pub(super) struct NamespaceCallCollector<'a> {
    pub(super) unit_path: &'a Path,
    pub(super) interner: &'a radix_module::lexer::Interner,
    pub(super) links: &'a PackageMirLinks,
    pub(super) calls: Vec<RawNamespaceCall>,
}

impl HirVisitor for NamespaceCallCollector<'_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Call(callee, explicit_args, _, _) = &expr.kind
            && let HirExpressionKind::Field(receiver, method) = &callee.kind
            && let HirExpressionKind::Path(import_def) = receiver.kind
        {
            let method = self.interner.resolve(*method);
            if let Some(old_target) = self
                .links
                .calls
                .get(&(self.unit_path.to_path_buf(), import_def, method.to_owned()))
                .copied()
            {
                self.calls.push(RawNamespaceCall {
                    call_span: expr.span,
                    callee_span: callee.span,
                    old_target,
                    explicit_args: explicit_args.clone(),
                });
            }
        }
        if let HirExpressionKind::MethodCall(receiver, method, explicit_args, _, _) = &expr.kind
            && let HirExpressionKind::Path(import_def) = receiver.kind
        {
            let method = self.interner.resolve(*method);
            if let Some(old_target) = self
                .links
                .calls
                .get(&(self.unit_path.to_path_buf(), import_def, method.to_owned()))
                .copied()
            {
                self.calls.push(RawNamespaceCall {
                    call_span: expr.span,
                    callee_span: receiver.span,
                    old_target,
                    explicit_args: explicit_args.clone(),
                });
            }
        }
        walk_expr(self, expr);
    }
}

fn generic_air_params(
    hir: &radix_module::hir::HirModule,
    lanes: &radix_module::semantic::RadixLaneMetadata,
    def_id: DefId,
) -> Option<Vec<HirTypeParam>> {
    if lanes.lane_for(def_id) != Some(RadixLane::Air) {
        return None;
    }
    hir.items.iter().find_map(|item| {
        if item.def_id != def_id {
            return None;
        }
        let HirItemKind::Function(function) = &item.kind else {
            return None;
        };
        (!function.type_params.is_empty()).then(|| function.type_params.clone())
    })
}

fn request_for_call(call: &NamespaceGenericCall) -> bool {
    call.resolved.is_some() || !call.explicit_args.is_empty()
}

fn request_for_target(
    call: &NamespaceGenericCall,
    target_types: &mut TypeTable,
) -> Option<Vec<ResolvedGenericBinding>> {
    let source_types = TypeTable::from_snapshot(call.caller_types.clone()).ok()?;
    let params = &call.target_params;
    let mut imported = HashMap::new();
    if let Some(row) = &call.resolved {
        if row.bindings.len() != params.len() {
            return None;
        }
        return row
            .bindings
            .iter()
            .zip(params)
            .map(|(binding, param)| {
                let witness =
                    rehome_witness(binding.witness, &source_types, target_types, &mut imported);
                (param.kind == witness_sort(witness)).then_some(ResolvedGenericBinding {
                    name: param.name,
                    source: binding.source,
                    witness,
                })
            })
            .collect();
    }

    if call.explicit_args.len() != params.len() {
        return None;
    }
    // The explicit list binds by marker: a `size` argument fills the next
    // size parameter, an unmarked argument the next type parameter.
    let (size_args, type_args): (Vec<TypeId>, Vec<TypeId>) =
        call.explicit_args.iter().copied().partition(|ty| {
            matches!(source_types.get(*ty), Type::Param(symbol)
                if radix_types::parse_explicit_size(call.caller_interner.resolve(*symbol)).is_some())
        });
    let mut size_args = size_args.into_iter();
    let mut type_args = type_args.into_iter();
    params
        .iter()
        .map(|param| {
            let witness = match param.kind {
                GenericParamKind::Magnitudo => {
                    let ty = size_args.next()?;
                    let Type::Param(symbol) = source_types.get(ty) else {
                        return None;
                    };
                    // Old-artifact fallback: only a natural size has a
                    // literal witness; a shape or a name stays fail-closed.
                    let radix_types::ExplicitSize::Natural(value) =
                        radix_types::parse_explicit_size(call.caller_interner.resolve(*symbol))?
                    else {
                        return None;
                    };
                    GenericWitness::Magnitudo(intern_index_canonical(
                        target_types,
                        IndexExpr::Literal(value),
                    ))
                }
                GenericParamKind::Typus => {
                    let ty = type_args.next()?;
                    GenericWitness::Typus(crate::mir::cli_plan::import_semantic_type(
                        &source_types,
                        target_types,
                        ty,
                        &mut imported,
                    ))
                }
            };
            Some(ResolvedGenericBinding {
                name: param.name,
                source: radix_types::GenericBindingSource::Explicit,
                witness,
            })
        })
        .collect()
}

fn rehome_witness(
    witness: GenericWitness,
    source: &TypeTable,
    target: &mut TypeTable,
    imported: &mut HashMap<TypeId, TypeId>,
) -> GenericWitness {
    match witness {
        GenericWitness::Typus(ty) => GenericWitness::Typus(
            crate::mir::cli_plan::import_semantic_type(source, target, ty, imported),
        ),
        GenericWitness::Magnitudo(index) => {
            GenericWitness::Magnitudo(import_index_canonical(source, target, index))
        }
    }
}

/// Package calls can carry equivalent concrete indices allocated by separate
/// caller analyses. Keep the instance key content-stable when those indices
/// are rehomed into the provider table.
fn import_index_canonical(source: &TypeTable, target: &mut TypeTable, index: IndexId) -> IndexId {
    let imported = crate::mir::cli_plan::import_index_expr(source, target, index);
    canonicalize_index(target, imported)
}

fn canonicalize_index(target: &mut TypeTable, index: IndexId) -> IndexId {
    let expr = target.get_index(index).clone();
    match expr {
        IndexExpr::Tuple(items) => {
            let items = items
                .into_iter()
                .map(|item| canonicalize_index(target, item))
                .collect();
            intern_index_canonical(target, IndexExpr::Tuple(items))
        }
        expr => intern_index_canonical(target, expr),
    }
}

fn intern_index_canonical(target: &mut TypeTable, expr: IndexExpr) -> IndexId {
    (0..target.index_count())
        .filter_map(|raw| u32::try_from(raw).ok().map(IndexId))
        .find(|id| target.get_index(*id) == &expr)
        .unwrap_or_else(|| target.intern_index(expr))
}

fn witness_sort(witness: GenericWitness) -> GenericParamKind {
    match witness {
        GenericWitness::Typus(_) => GenericParamKind::Typus,
        GenericWitness::Magnitudo(_) => GenericParamKind::Magnitudo,
    }
}

fn missing_instance_diagnostic(path: &Path, issue: &'static str) -> Diagnostic {
    Diagnostic::error("generic AIR call has no concrete analyzed template instance")
        .with_phase(DiagnosticPhase::Analysis)
        .with_code("GAS001")
        .with_args(vec![radix_module::diagnostics::DiagnosticArg::new(
            "issue", issue,
        )])
        .with_file(path.display().to_string())
}

struct InstanceCallRewriter<'a> {
    unit_path: &'a Path,
    targets: &'a HashMap<(PathBuf, u32, u32, DefId), DefId>,
}

impl HirVisitorMut for InstanceCallRewriter<'_> {
    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        if let HirExpressionKind::Call(callee, _, _, _) = &mut expr.kind
            && let HirExpressionKind::Path(old_target) = &mut callee.kind
            && let Some(new_target) = self
                .targets
                .get(&(
                    self.unit_path.to_path_buf(),
                    expr.span.start,
                    expr.span.end,
                    *old_target,
                ))
                .copied()
        {
            *old_target = new_target;
        }
        walk_expr_mut(self, expr);
    }
}
