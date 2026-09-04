//! Package-level concrete AIR instance discovery.
//!
//! Single-module semantic analysis can see direct calls in its own HIR. A
//! package caller sees an imported namespace call until the package linker
//! rewrites it, so the provider's AIR template needs a second, package-owned
//! discovery step before per-unit MIR lowering. This module only discovers
//! sibling source-package calls; external FMIR/library bodies do not have the
//! analyzed HIR required by the AIR contract and remain fail-closed at their
//! existing boundary.

use super::*;
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
}

/// Discover sibling namespace calls that target generic AIR functions,
/// materialize their concrete provider-owned instances, and assign each
/// instance a package synthetic source identity.
pub(super) fn plan_package_instances(
    package: &mut AnalyzedProgram,
    links: &mut PackageMirLinks,
) -> Result<(), Vec<Diagnostic>> {
    let calls = discover_calls(package, links);
    if calls.is_empty() {
        return Ok(());
    }

    let mut grouped = BTreeMap::<PathBuf, Vec<NamespaceGenericCall>>::new();
    let mut errors = Vec::new();
    for call in calls {
        if !request_for_call(&call) {
            errors.push(missing_instance_diagnostic(
                &call.target_path,
                "generic_air_instance_missing_concrete_binding",
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
        let Some((_, target)) = package.node_by_path_mut(&target_path) else {
            continue;
        };
        let mut requests = Vec::with_capacity(calls.len());
        let mut keys = Vec::with_capacity(calls.len());
        for call in &calls {
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
            continue;
        }
        if requests.is_empty() {
            continue;
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
                    &target_path,
                    "generic_air_instance_materialization_failed",
                ));
            }
        }
        if !errors.is_empty() {
            continue;
        }

        // The ordinary semantic pipeline has already run for this module.
        // Newly requested clones must pass the same concrete-only gates before
        // the package lowers any unit to AIR/MIR.
        if let Err(instance_errors) = radix_module::semantic::passes::air_unroll::unroll(
            &mut target.analysis.hir,
            &mut target.analysis.presentation,
            &target.analysis.radix_lanes,
            &mut target.analysis.interner,
            &target.analysis.types,
        ) {
            errors.extend(instance_errors.into_iter().map(|error| {
                Diagnostic::from_semantic_error(&target_path.display().to_string(), "", &error)
            }));
        }
        if let Err(instance_errors) = radix_module::semantic::passes::air_purity::check(
            &mut target.analysis.hir,
            &mut target.analysis.radix_lanes,
            &mut target.analysis.interner,
            &target.analysis.types,
        ) {
            errors.extend(instance_errors.into_iter().map(|error| {
                Diagnostic::from_semantic_error(&target_path.display().to_string(), "", &error)
            }));
        }

        for (key, instance) in report.instance_defs {
            let synthetic = DefId(links.next_synthetic);
            links.next_synthetic = links.next_synthetic.saturating_add(1);
            links
                .sources
                .insert((target_path.clone(), instance), synthetic);
            let _ = key;
        }
        for (call, key) in calls.iter().zip(keys) {
            let Some(instance) = instance_by_key.get(&key).copied() else {
                continue;
            };
            let Some(synthetic) = links.sources.get(&(target_path.clone(), instance)).copied()
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

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
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

fn discover_calls(package: &AnalyzedProgram, links: &PackageMirLinks) -> Vec<NamespaceGenericCall> {
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
            let Some(target_node) = package.node_by_path(&target_path).map(|(_, node)| node) else {
                continue;
            };
            let Some(target_params) = generic_air_params(
                &target_node.analysis.hir,
                &target_node.analysis.radix_lanes,
                target_def,
            ) else {
                continue;
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
            });
        }
    }
    calls
}

#[derive(Clone)]
struct RawNamespaceCall {
    call_span: radix_module::lexer::Span,
    callee_span: radix_module::lexer::Span,
    old_target: DefId,
    explicit_args: Vec<TypeId>,
}

struct NamespaceCallCollector<'a> {
    unit_path: &'a Path,
    interner: &'a radix_module::lexer::Interner,
    links: &'a PackageMirLinks,
    calls: Vec<RawNamespaceCall>,
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
                    rehome_witness(binding.witness, &source_types, target_types, &mut imported)?;
                (binding_sort(param.kind) == witness_sort(witness)).then_some(
                    ResolvedGenericBinding {
                        name: param.name,
                        source: binding.source,
                        witness,
                    },
                )
            })
            .collect();
    }

    if call.explicit_args.len() != params.len() {
        return None;
    }
    let mut args = call.explicit_args.iter().copied();
    params
        .iter()
        .map(|param| {
            let ty = args.next()?;
            let witness = match param.kind {
                GenericParamKind::Magnitudo => {
                    let Type::Param(symbol) = source_types.get(ty) else {
                        return None;
                    };
                    let value =
                        radix_types::parse_natural_capacity(call.caller_interner.resolve(*symbol))?;
                    GenericWitness::Magnitudo(intern_index_canonical(
                        target_types,
                        IndexExpr::Literal(value),
                    ))
                }
                GenericParamKind::Typus => {
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
) -> Option<GenericWitness> {
    Some(match witness {
        GenericWitness::Typus(ty) => GenericWitness::Typus(
            crate::mir::cli_plan::import_semantic_type(source, target, ty, imported),
        ),
        GenericWitness::Magnitudo(index) => {
            GenericWitness::Magnitudo(import_index_canonical(source, target, index))
        }
    })
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
        .map(|raw| IndexId(raw as u32))
        .find(|id| target.get_index(*id) == &expr)
        .unwrap_or_else(|| target.intern_index(expr))
}

fn binding_sort(kind: GenericParamKind) -> GenericParamKind {
    kind
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
