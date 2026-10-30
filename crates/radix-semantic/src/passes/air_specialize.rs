//! Concrete instance planning for generic AIR functions.
//!
//! Typechecking deliberately keeps generic bodies symbolic. AIR does not: its
//! tensor shapes, loop bounds, and intrinsic payloads are concrete facts. This
//! pass is the boundary between those two contracts. It clones each reachable
//! direct generic AIR call whose complete binding vector is concrete, rewrites
//! its HIR types and indices, and retargets the call to the clone before
//! `air_unroll`, `air_purity`, or HIR→AIR lowering runs.
//!
//! The pass is intentionally demand-driven. Generic templates remain in HIR as
//! source/FHIR templates and are never added to the executable AIR route. A
//! missing or symbolic witness is left to the ordinary generic/MIR route; the
//! typechecker has already diagnosed genuinely unresolved calls.

use crate::{
    GenericWitness, IndexExpr, IndexId, RadixLane, RadixLaneMetadata, ResolvedGenericBinding,
    ResolvedGenericCall, Type, TypeId, TypeTable,
};
use handroll::{FxHashMap, FxHashSet};
use radix_hir::visit::{HirVisitor, HirVisitorMut, walk_expr, walk_expr_mut};
use radix_hir::{
    DefId, HirExpression, HirExpressionKind, HirFunction, HirId, HirItem, HirItemKind, HirLocal,
    HirModule, HirParam, HirStatement, HirTypeParam,
};
use radix_lexer::{Interner, Span, Symbol};

/// Stable key for one concrete AIR instance.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AirInstanceKey {
    pub template: DefId,
    pub witnesses: Vec<GenericWitness>,
}

/// Facts emitted by the planner for tests and downstream diagnostics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AirSpecializationReport {
    pub instances: Vec<AirInstanceKey>,
    /// Concrete HIR identity paired with each key, in the same order as
    /// `instances`. Package linking uses this to connect a source-owned clone
    /// to its package synthetic source identity.
    pub instance_defs: Vec<(AirInstanceKey, DefId)>,
}

/// A concrete generic binding requested by a package-level caller.
///
/// Same-module calls are discovered from HIR by [`specialize`]. Package
/// linking discovers namespace calls before namespace rewriting and supplies
/// those roots through this request shape so the callee's own module remains
/// the owner of substitution and AIR preparation.
#[derive(Debug, Clone)]
pub struct AirSpecializationRequest {
    pub template: DefId,
    pub bindings: Vec<ResolvedGenericBinding>,
}

#[derive(Clone)]
struct GenericAirTemplate {
    function: HirFunction,
    span: Span,
    /// The template item's annotations: an instance carries the same
    /// visibility (`@ public`) as its template, so a concrete kernel instance
    /// is a launchable entry exactly when the authored kernel is.
    annotations: Vec<radix_hir::HirAnnotation>,
}

#[derive(Clone)]
struct GenericCallSite {
    callee: DefId,
    span: Span,
    callee_span: Span,
    explicit_args: Vec<TypeId>,
    /// A nested call collected from a concrete clone carries the original
    /// binder row after the enclosing instance's substitutions. This is
    /// needed when the nested call had no authored generic prefix.
    inherited: Option<Vec<ResolvedGenericBinding>>,
    /// `None` means the call belongs to the original module; `Some` points at
    /// the concrete clone whose body must be retargeted without changing the
    /// symbolic source template with the same source spans.
    owner: Option<DefId>,
}

/// Plan and materialize concrete HIR instances for reachable generic AIR
/// functions. The caller runs this only on MIR-backed analyses; FHIR/source
/// targets retain the template HIR unchanged.
pub fn specialize(
    hir: &mut HirModule,
    lanes: &mut RadixLaneMetadata,
    types: &mut TypeTable,
    interner: &mut Interner,
) -> AirSpecializationReport {
    specialize_with_requests(hir, lanes, types, interner, &[])
}

/// Materialize concrete instances requested by a package-level planner.
///
/// The request bindings must use the callee module's type/index table. The
/// function also walks direct generic calls nested in each newly materialized
/// instance, so nested AIR generics use the same deduplication and identity
/// rules as local calls.
pub fn specialize_requested(
    hir: &mut HirModule,
    lanes: &mut RadixLaneMetadata,
    types: &mut TypeTable,
    interner: &mut Interner,
    requests: &[AirSpecializationRequest],
) -> AirSpecializationReport {
    specialize_with_requests(hir, lanes, types, interner, requests)
}

fn specialize_with_requests(
    hir: &mut HirModule,
    lanes: &mut RadixLaneMetadata,
    types: &mut TypeTable,
    interner: &mut Interner,
    requests: &[AirSpecializationRequest],
) -> AirSpecializationReport {
    let templates = collect_templates(hir, lanes);
    if templates.is_empty() {
        return AirSpecializationReport::default();
    }

    // A declared `@ radix backward` companion of a generic template is called
    // by its own identity; a concrete caller's call to it plans through the
    // template's instance and retargets to that instance's companion.
    let companion_primals = templates
        .keys()
        .filter_map(|template| {
            lanes
                .backward_for(*template)
                .map(|backward| (backward.companion_def_id, *template))
        })
        .collect::<FxHashMap<_, _>>();

    let mut ids = IdAllocator::from_module(hir);
    let mut work = collect_calls(hir)
        .into_iter()
        .map(SpecializationWork::Call)
        .collect::<Vec<_>>();
    work.extend(requests.iter().cloned().map(SpecializationWork::Request));
    let mut planned = FxHashMap::<AirInstanceKey, DefId>::default();
    let mut report = AirSpecializationReport::default();
    let mut cursor = 0;

    while cursor < work.len() {
        let item = work[cursor].clone();
        cursor += 1;
        let (template, template_id, bindings, site) = match item {
            SpecializationWork::Call(site) => {
                let template_id = if templates.contains_key(&site.callee) {
                    site.callee
                } else if let Some(primal) = companion_primals.get(&site.callee) {
                    *primal
                } else {
                    continue;
                };
                let Some(template) = templates.get(&template_id) else {
                    continue;
                };
                let Some(bindings) = binding_vector(&site, template, types, interner) else {
                    continue;
                };
                if !bindings_are_concrete(&template.function.type_params, &bindings, types) {
                    continue;
                }
                (template, template_id, bindings, Some(site))
            }
            SpecializationWork::Request(request) => {
                let Some(template) = templates.get(&request.template) else {
                    continue;
                };
                if !bindings_are_concrete(&template.function.type_params, &request.bindings, types)
                {
                    continue;
                }
                (template, request.template, request.bindings, None)
            }
        };
        let key = AirInstanceKey {
            template: template_id,
            witnesses: bindings.iter().map(|binding| binding.witness).collect(),
        };
        let instance = if let Some(instance) = planned.get(&key).copied() {
            instance
        } else {
            let instance = ids.next_def();
            let (type_map, index_map) = binding_maps(&bindings);
            let mut function = template.function.clone();
            function.type_params.clear();
            substitute_function(
                &mut function,
                &type_map,
                &index_map,
                types,
                interner,
                &mut ids,
            );
            let item = HirItem {
                annotations: template.annotations.clone(),
                id: ids.next_hir(),
                def_id: instance,
                span: template.span,
                kind: HirItemKind::Function(Box::new(function.clone())),
            };
            hir.items.push(item);
            lanes.insert(instance, RadixLane::Air);
            if let Some(backward) = lanes.backward_for(template_id).cloned() {
                lanes.insert_backward(
                    instance,
                    crate::RadixBackward {
                        companion_name: backward.companion_name,
                        companion_def_id: radix_hir::DefId::INVALID,
                        selected_params: backward.selected_params.clone(),
                    },
                );
            }
            let nested =
                collect_function_calls(&function, instance, &type_map, &index_map, types, interner);
            work.extend(nested.into_iter().map(SpecializationWork::Call));
            planned.insert(key.clone(), instance);
            report.instances.push(key.clone());
            report.instance_defs.push((key, instance));
            instance
        };

        if let Some(site) = site {
            // The span pair handles both the call expression span and the
            // callee span used by the typechecker when it records resolved
            // witnesses.
            let target = if site.callee == template_id {
                instance
            } else {
                // The call names the template's companion: it runs the
                // instance's companion.
                lanes
                    .backward_for(instance)
                    .map_or(instance, |backward| backward.companion_def_id)
            };
            retarget_call(
                hir,
                site.callee,
                target,
                site.span,
                site.callee_span,
                site.owner,
            );
        }
    }

    report
}

#[derive(Clone)]
enum SpecializationWork {
    Call(GenericCallSite),
    Request(AirSpecializationRequest),
}

fn collect_templates(
    hir: &HirModule,
    lanes: &RadixLaneMetadata,
) -> FxHashMap<DefId, GenericAirTemplate> {
    hir.items
        .iter()
        .filter_map(|item| {
            if lanes.lane_for(item.def_id) != Some(RadixLane::Air) {
                return None;
            }
            let HirItemKind::Function(function) = &item.kind else {
                return None;
            };
            (!function.type_params.is_empty()).then(|| {
                (
                    item.def_id,
                    GenericAirTemplate {
                        function: (**function).clone(),
                        span: item.span,
                        annotations: item.annotations.clone(),
                    },
                )
            })
        })
        .collect()
}

fn collect_calls(hir: &HirModule) -> Vec<GenericCallSite> {
    let mut collector = CallCollector::default();
    collector.visit_program(hir);
    collector.calls
}

fn collect_function_calls(
    function: &HirFunction,
    owner: DefId,
    type_map: &FxHashMap<Symbol, TypeId>,
    index_map: &FxHashMap<Symbol, IndexId>,
    types: &mut TypeTable,
    interner: &mut Interner,
) -> Vec<GenericCallSite> {
    let mut collector = CallCollector::default();
    if let Some(body) = &function.body {
        collector.visit_block(body);
    }
    for site in &mut collector.calls {
        site.owner = Some(owner);
        site.inherited = inherited_bindings(site, type_map, index_map, types, interner);
    }
    collector.calls
}

#[derive(Default)]
struct CallCollector {
    calls: Vec<GenericCallSite>,
}

impl HirVisitor for CallCollector {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Call(callee, explicit_args, _, _) = &expr.kind
            && let HirExpressionKind::Path(def_id) = callee.kind
        {
            self.calls.push(GenericCallSite {
                callee: def_id,
                span: expr.span,
                callee_span: callee.span,
                explicit_args: explicit_args.clone(),
                inherited: None,
                owner: None,
            });
        }
        walk_expr(self, expr);
    }
}

fn binding_vector(
    site: &GenericCallSite,
    template: &GenericAirTemplate,
    types: &TypeTable,
    interner: &Interner,
) -> Option<Vec<ResolvedGenericBinding>> {
    let params = &template.function.type_params;
    if let Some(bindings) = &site.inherited
        && bindings.len() == params.len()
        && bindings
            .iter()
            .zip(params)
            .all(|(binding, param)| binding.name == param.name)
        && bindings_have_declared_sorts(bindings, params)
        && bindings_are_concrete(params, bindings, types)
    {
        return Some(bindings.clone());
    }
    let mut candidates = Vec::new();
    // Resolved vectors are authoritative for inferred and defaulted slots.
    // Matching by both direct identity and either recorded span supports old
    // FHIR artifacts whose identity field was absent before this pass landed.
    for row in &types.resolved_generic_calls {
        if row.callee.is_some_and(|callee| callee != site.callee) || !same_call_span(row, site) {
            continue;
        }
        if row.bindings.len() == params.len()
            && row
                .bindings
                .iter()
                .zip(params)
                .all(|(binding, param)| binding.name == param.name)
        {
            candidates.push(row.clone());
        }
    }
    candidates.sort_by_key(|row| {
        row.bindings
            .iter()
            .map(|binding| binding.source as u8)
            .collect::<Vec<_>>()
    });
    if let Some(row) = candidates.into_iter().find(|row| {
        bindings_have_declared_sorts(&row.bindings, params)
            && bindings_are_concrete(params, &row.bindings, types)
    }) {
        return Some(row.bindings);
    }

    explicit_binding_vector(params, &site.explicit_args, types, interner)
}

fn inherited_bindings(
    site: &GenericCallSite,
    type_map: &FxHashMap<Symbol, TypeId>,
    index_map: &FxHashMap<Symbol, IndexId>,
    types: &mut TypeTable,
    interner: &mut Interner,
) -> Option<Vec<ResolvedGenericBinding>> {
    let bindings = types
        .resolved_generic_calls
        .iter()
        .find(|row| {
            row.callee == Some(site.callee)
                && (row.call_span == site.span || row.call_span == site.callee_span)
        })
        .map(|row| row.bindings.clone())?;
    Some(
        bindings
            .iter()
            .map(|binding| ResolvedGenericBinding {
                name: binding.name,
                source: binding.source,
                witness: match binding.witness {
                    GenericWitness::Typus(ty) => GenericWitness::Typus(substitute_type(
                        ty, type_map, index_map, types, interner,
                    )),
                    GenericWitness::Magnitudo(index) => {
                        GenericWitness::Magnitudo(substitute_index(index, index_map, types))
                    }
                },
            })
            .collect(),
    )
}

fn same_call_span(row: &ResolvedGenericCall, site: &GenericCallSite) -> bool {
    row.call_span == site.span || row.call_span == site.callee_span
}

fn explicit_binding_vector(
    params: &[HirTypeParam],
    explicit_args: &[TypeId],
    types: &TypeTable,
    interner: &Interner,
) -> Option<Vec<ResolvedGenericBinding>> {
    if explicit_args.is_empty() {
        return None;
    }
    let size_only = params
        .iter()
        .all(|param| param.kind == radix_syntax::GenericParamKind::Magnitudo);
    if size_only {
        if explicit_args.len() != params.len() {
            return None;
        }
        return params
            .iter()
            .zip(explicit_args)
            .map(|(param, ty)| {
                Some(ResolvedGenericBinding {
                    name: param.name,
                    source: radix_types::GenericBindingSource::Explicit,
                    witness: GenericWitness::Magnitudo(explicit_size_index(*ty, types, interner)?),
                })
            })
            .collect();
    }

    let typus_count = params
        .iter()
        .filter(|param| param.kind == radix_syntax::GenericParamKind::Typus)
        .count();
    if explicit_args.len() != typus_count
        || params
            .iter()
            .any(|param| param.kind == radix_syntax::GenericParamKind::Magnitudo)
    {
        return None;
    }
    Some(
        params
            .iter()
            .zip(explicit_args.iter().copied())
            .map(|(param, ty)| ResolvedGenericBinding {
                name: param.name,
                source: radix_types::GenericBindingSource::Explicit,
                witness: GenericWitness::Typus(ty),
            })
            .collect(),
    )
}

fn explicit_size_index(ty: TypeId, types: &TypeTable, interner: &Interner) -> Option<IndexId> {
    let Type::Param(symbol) = types.get(ty) else {
        return None;
    };
    // Old-artifact fallback: only a natural size has a literal witness here;
    // a shape or a name stays fail-closed.
    let radix_types::ExplicitSize::Natural(value) =
        radix_types::parse_explicit_size(interner.resolve(*symbol))?
    else {
        return None;
    };
    find_or_intern_literal(value, types)
}

// Explicit fallback planning is used only for old artifacts without a
// resolved-call row. Their type table is immutable here, so locate the
// already-interned literal rather than minting a new index.
fn find_or_intern_literal(value: u64, types: &TypeTable) -> Option<IndexId> {
    (0..types.index_count())
        .filter_map(|ordinal| u32::try_from(ordinal).ok().map(IndexId))
        .find(|id| matches!(types.get_index(*id), IndexExpr::Literal(found) if *found == value))
}

fn bindings_have_declared_sorts(
    bindings: &[ResolvedGenericBinding],
    params: &[HirTypeParam],
) -> bool {
    bindings.iter().zip(params).all(|(binding, param)| {
        matches!(
            (param.kind, binding.witness),
            (
                radix_syntax::GenericParamKind::Typus,
                GenericWitness::Typus(_)
            ) | (
                radix_syntax::GenericParamKind::Magnitudo,
                GenericWitness::Magnitudo(_)
            )
        )
    })
}

fn bindings_are_concrete(
    params: &[HirTypeParam],
    bindings: &[ResolvedGenericBinding],
    types: &TypeTable,
) -> bool {
    bindings_have_declared_sorts(bindings, params)
        && bindings.iter().all(|binding| match binding.witness {
            GenericWitness::Typus(ty) => {
                !contains_unresolved_type(ty, types, &mut FxHashSet::default())
            }
            GenericWitness::Magnitudo(index) => {
                concrete_index(index, types, &mut FxHashSet::default())
            }
        })
}

fn concrete_index(index: IndexId, types: &TypeTable, seen: &mut FxHashSet<IndexId>) -> bool {
    if !seen.insert(index) {
        return false;
    }
    match types.get_index(index) {
        IndexExpr::Literal(_) => true,
        IndexExpr::Tuple(dims) => dims.iter().all(|dim| concrete_index(*dim, types, seen)),
        IndexExpr::Affine { .. } | IndexExpr::Divide { .. } => {
            radix_types::evaluate_index_dimension(index, types).is_ok_and(|value| value.is_some())
        }
        IndexExpr::Param(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Infer(_)
        | IndexExpr::Unspecified => false,
    }
}

fn contains_unresolved_type(
    type_id: TypeId,
    types: &TypeTable,
    seen: &mut FxHashSet<TypeId>,
) -> bool {
    if !seen.insert(type_id) {
        return false;
    }
    match types.get(type_id) {
        Type::Param(_) | Type::Infer(_) | Type::InferUnion(_) | Type::MarkerHole { .. } => true,
        Type::Primitive(_)
        | Type::Bits(_)
        | Type::ModularWord(_)
        | Type::SaturatingWord(_)
        | Type::RangedInt { .. }
        | Type::PolicyFloat(_, _)
        | Type::TensorEdgePolicy(_)
        | Type::SizedInstans(_)
        | Type::Struct(_)
        | Type::Enum(_)
        | Type::Interface(_)
        | Type::SizedNumeric(_, _)
        | Type::Error => false,
        Type::Array(inner)
        | Type::Queue(inner)
        | Type::Stack(inner)
        | Type::Set(inner)
        | Type::Promissum(inner)
        | Type::Cursor(inner)
        | Type::Atomic(inner)
        | Type::Intervallum(inner, _)
        | Type::Option(inner) => contains_unresolved_type(*inner, types, seen),
        Type::RangedArray { element, .. } => contains_unresolved_type(*element, types, seen),
        Type::BoundedArray { element, capacity }
        | Type::BoundedQueue { element, capacity }
        | Type::BoundedStack { element, capacity } => {
            contains_unresolved_type(*element, types, seen)
                || !concrete_index(*capacity, types, &mut FxHashSet::default())
        }
        Type::BoundedTextus { capacity }
        | Type::BoundedAscii { capacity }
        | Type::BoundedOcteti { capacity } => {
            !concrete_index(*capacity, types, &mut FxHashSet::default())
        }
        Type::Map(key, value)
        | Type::Ledger(key, value)
        | Type::PromissumFailable(key, value)
        | Type::AsyncCursor(key, value) => {
            contains_unresolved_type(*key, types, seen)
                || contains_unresolved_type(*value, types, seen)
        }
        Type::Record(fields) => fields
            .values()
            .any(|ty| contains_unresolved_type(*ty, types, seen)),
        Type::Tensor(element, shape)
        | Type::Vector(element, shape)
        | Type::Matrix(element, shape)
        | Type::Sparsa(element, shape) => {
            contains_unresolved_type(*element, types, seen)
                || !concrete_index(*shape, types, &mut FxHashSet::default())
        }
        Type::Ref(_, inner) | Type::Alias(_, inner) => {
            contains_unresolved_type(*inner, types, seen)
        }
        Type::Func(sig) => {
            sig.params
                .iter()
                .any(|param| contains_unresolved_type(param.ty, types, seen))
                || contains_unresolved_type(sig.ret, types, seen)
                || sig
                    .err
                    .is_some_and(|ty| contains_unresolved_type(ty, types, seen))
        }
        Type::Applied(base, args) => {
            contains_unresolved_type(*base, types, seen)
                || args.iter().any(|arg| match arg {
                    radix_types::AppliedArg::Type(ty) => contains_unresolved_type(*ty, types, seen),
                    radix_types::AppliedArg::Size(index) => {
                        !concrete_index(*index, types, &mut FxHashSet::default())
                    }
                })
        }
        Type::Union(members) | Type::Intersection(members) | Type::Tuple(members) => members
            .iter()
            .any(|ty| contains_unresolved_type(*ty, types, seen)),
    }
}

fn binding_maps(
    bindings: &[ResolvedGenericBinding],
) -> (FxHashMap<Symbol, TypeId>, FxHashMap<Symbol, IndexId>) {
    let mut type_map = FxHashMap::default();
    let mut index_map = FxHashMap::default();
    for binding in bindings {
        match binding.witness {
            GenericWitness::Typus(ty) => {
                type_map.insert(binding.name, ty);
            }
            GenericWitness::Magnitudo(index) => {
                index_map.insert(binding.name, index);
            }
        }
    }
    (type_map, index_map)
}

fn substitute_function(
    function: &mut HirFunction,
    type_map: &FxHashMap<Symbol, TypeId>,
    index_map: &FxHashMap<Symbol, IndexId>,
    types: &mut TypeTable,
    interner: &mut Interner,
    ids: &mut IdAllocator,
) {
    function.params.iter_mut().for_each(|param| {
        param.ty = substitute_type(param.ty, type_map, index_map, types, interner);
    });
    if let Some(param) = &mut function.cli_args {
        param.ty = substitute_type(param.ty, type_map, index_map, types, interner);
    }
    function.ret_ty = function
        .ret_ty
        .map(|ty| substitute_type(ty, type_map, index_map, types, interner));
    function.err_ty = function
        .err_ty
        .map(|ty| substitute_type(ty, type_map, index_map, types, interner));
    let mut rewriter = InstanceRewriter {
        type_map,
        index_map,
        types,
        interner,
        next_hir: &mut ids.next_hir_id,
    };
    rewriter.visit_function_mut(function);
}

struct InstanceRewriter<'a> {
    type_map: &'a FxHashMap<Symbol, TypeId>,
    index_map: &'a FxHashMap<Symbol, IndexId>,
    types: &'a mut TypeTable,
    interner: &'a mut Interner,
    next_hir: &'a mut u32,
}

impl HirVisitorMut for InstanceRewriter<'_> {
    fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
        expr.id = HirId(next_id(self.next_hir));
        expr.ty = expr.ty.map(|ty| {
            substitute_type(ty, self.type_map, self.index_map, self.types, self.interner)
        });
        match &mut expr.kind {
            HirExpressionKind::Call(_, type_args, _, _)
            | HirExpressionKind::MethodCall(_, _, type_args, _, _) => {
                for type_arg in type_args {
                    *type_arg = substitute_type(
                        *type_arg,
                        self.type_map,
                        self.index_map,
                        self.types,
                        self.interner,
                    );
                }
            }
            _ => {}
        }
        walk_expr_mut(self, expr);
    }

    fn visit_stmt_mut(&mut self, statement: &mut HirStatement) {
        statement.id = HirId(next_id(self.next_hir));
        radix_hir::visit::walk_stmt_mut(self, statement);
    }

    fn visit_param_mut(&mut self, param: &mut HirParam) {
        param.ty = substitute_type(
            param.ty,
            self.type_map,
            self.index_map,
            self.types,
            self.interner,
        );
        if let Some(default) = &mut param.default {
            self.visit_expr_mut(default);
        }
    }

    fn visit_local_mut(&mut self, local: &mut HirLocal) {
        local.ty = local.ty.map(|ty| {
            substitute_type(ty, self.type_map, self.index_map, self.types, self.interner)
        });
        if let Some(init) = &mut local.init {
            self.visit_expr_mut(init);
        }
    }
}

fn substitute_type(
    type_id: TypeId,
    type_map: &FxHashMap<Symbol, TypeId>,
    index_map: &FxHashMap<Symbol, IndexId>,
    types: &mut TypeTable,
    interner: &mut Interner,
) -> TypeId {
    let current = types.get(type_id).clone();
    let replacement = match current {
        Type::Param(symbol) => {
            if let Some(ty) = type_map.get(&symbol) {
                return *ty;
            }
            if let Some(index) = index_map.get(&symbol)
                && let IndexExpr::Literal(value) = types.get_index(*index)
            {
                let numeric = interner.intern(&value.to_string());
                return types.intern(Type::Param(numeric));
            }
            // A marked size argument that forwards a size param of the
            // template (`size D`) becomes the marked natural the instance
            // binds that param to.
            let forwarded = match radix_types::parse_explicit_size(interner.resolve(symbol)) {
                Some(radix_types::ExplicitSize::Name(operand)) => {
                    index_map.iter().find_map(|(name, index)| {
                        if interner.resolve(*name) != operand {
                            return None;
                        }
                        match types.get_index(*index) {
                            IndexExpr::Literal(value) => Some(*value),
                            _ => None,
                        }
                    })
                }
                _ => None,
            };
            if let Some(value) = forwarded {
                let marked = interner.intern(&radix_types::spell_explicit_size(&value.to_string()));
                return types.intern(Type::Param(marked));
            }
            Type::Param(symbol)
        }
        Type::Array(inner) => {
            Type::Array(substitute_type(inner, type_map, index_map, types, interner))
        }
        Type::Queue(inner) => {
            Type::Queue(substitute_type(inner, type_map, index_map, types, interner))
        }
        Type::Stack(inner) => {
            Type::Stack(substitute_type(inner, type_map, index_map, types, interner))
        }
        Type::Set(inner) => Type::Set(substitute_type(inner, type_map, index_map, types, interner)),
        Type::Promissum(inner) => {
            Type::Promissum(substitute_type(inner, type_map, index_map, types, interner))
        }
        Type::Cursor(inner) => {
            Type::Cursor(substitute_type(inner, type_map, index_map, types, interner))
        }
        Type::Atomic(inner) => {
            Type::Atomic(substitute_type(inner, type_map, index_map, types, interner))
        }
        Type::Intervallum(inner, step) => Type::Intervallum(
            substitute_type(inner, type_map, index_map, types, interner),
            substitute_index(step, index_map, types),
        ),
        Type::Option(inner) => {
            Type::Option(substitute_type(inner, type_map, index_map, types, interner))
        }
        Type::RangedArray { element, min, max } => Type::RangedArray {
            element: substitute_type(element, type_map, index_map, types, interner),
            min,
            max,
        },
        Type::BoundedArray { element, capacity } => Type::BoundedArray {
            element: substitute_type(element, type_map, index_map, types, interner),
            capacity: substitute_index(capacity, index_map, types),
        },
        Type::BoundedQueue { element, capacity } => Type::BoundedQueue {
            element: substitute_type(element, type_map, index_map, types, interner),
            capacity: substitute_index(capacity, index_map, types),
        },
        Type::BoundedStack { element, capacity } => Type::BoundedStack {
            element: substitute_type(element, type_map, index_map, types, interner),
            capacity: substitute_index(capacity, index_map, types),
        },
        Type::BoundedTextus { capacity } => Type::BoundedTextus {
            capacity: substitute_index(capacity, index_map, types),
        },
        Type::BoundedAscii { capacity } => Type::BoundedAscii {
            capacity: substitute_index(capacity, index_map, types),
        },
        Type::BoundedOcteti { capacity } => Type::BoundedOcteti {
            capacity: substitute_index(capacity, index_map, types),
        },
        Type::Map(key, value) => Type::Map(
            substitute_type(key, type_map, index_map, types, interner),
            substitute_type(value, type_map, index_map, types, interner),
        ),
        Type::Ledger(key, value) => Type::Ledger(
            substitute_type(key, type_map, index_map, types, interner),
            substitute_type(value, type_map, index_map, types, interner),
        ),
        Type::PromissumFailable(success, alternate) => Type::PromissumFailable(
            substitute_type(success, type_map, index_map, types, interner),
            substitute_type(alternate, type_map, index_map, types, interner),
        ),
        Type::AsyncCursor(item, alternate) => Type::AsyncCursor(
            substitute_type(item, type_map, index_map, types, interner),
            substitute_type(alternate, type_map, index_map, types, interner),
        ),
        Type::Tensor(element, shape) => Type::Tensor(
            substitute_type(element, type_map, index_map, types, interner),
            substitute_index(shape, index_map, types),
        ),
        Type::Vector(element, shape) => Type::Vector(
            substitute_type(element, type_map, index_map, types, interner),
            substitute_index(shape, index_map, types),
        ),
        Type::Matrix(element, shape) => Type::Matrix(
            substitute_type(element, type_map, index_map, types, interner),
            substitute_index(shape, index_map, types),
        ),
        Type::Sparsa(element, shape) => Type::Sparsa(
            substitute_type(element, type_map, index_map, types, interner),
            substitute_index(shape, index_map, types),
        ),
        Type::Record(fields) => Type::Record(
            fields
                .into_iter()
                .map(|(name, ty)| {
                    (
                        name,
                        substitute_type(ty, type_map, index_map, types, interner),
                    )
                })
                .collect(),
        ),
        Type::Ref(mode, inner) => Type::Ref(
            mode,
            substitute_type(inner, type_map, index_map, types, interner),
        ),
        Type::Alias(def_id, inner) => Type::Alias(
            def_id,
            substitute_type(inner, type_map, index_map, types, interner),
        ),
        Type::Func(mut sig) => {
            sig.params = sig
                .params
                .into_iter()
                .map(|mut param| {
                    param.ty = substitute_type(param.ty, type_map, index_map, types, interner);
                    param
                })
                .collect();
            sig.ret = substitute_type(sig.ret, type_map, index_map, types, interner);
            sig.err = sig
                .err
                .map(|err| substitute_type(err, type_map, index_map, types, interner));
            Type::Func(sig)
        }
        Type::Applied(base, args) => Type::Applied(
            substitute_type(base, type_map, index_map, types, interner),
            args.into_iter()
                .map(|arg| match arg {
                    radix_types::AppliedArg::Type(ty) => radix_types::AppliedArg::Type(
                        substitute_type(ty, type_map, index_map, types, interner),
                    ),
                    radix_types::AppliedArg::Size(index) => {
                        radix_types::AppliedArg::Size(substitute_index(index, index_map, types))
                    }
                })
                .collect(),
        ),
        Type::Union(members) => Type::Union(
            members
                .into_iter()
                .map(|ty| substitute_type(ty, type_map, index_map, types, interner))
                .collect(),
        ),
        Type::Intersection(members) => Type::Intersection(
            members
                .into_iter()
                .map(|ty| substitute_type(ty, type_map, index_map, types, interner))
                .collect(),
        ),
        Type::Tuple(members) => Type::Tuple(
            members
                .into_iter()
                .map(|ty| substitute_type(ty, type_map, index_map, types, interner))
                .collect(),
        ),
        other => other,
    };
    types.intern(replacement)
}

fn substitute_index(
    index: IndexId,
    index_map: &FxHashMap<Symbol, IndexId>,
    types: &mut TypeTable,
) -> IndexId {
    match types.get_index(index).clone() {
        IndexExpr::Param(symbol) => index_map.get(&symbol).copied().unwrap_or(index),
        IndexExpr::Tuple(dims) => {
            let dims = dims
                .into_iter()
                .map(|dim| substitute_index(dim, index_map, types))
                .collect();
            types.intern_index(IndexExpr::Tuple(dims))
        }
        IndexExpr::Affine { base, offset } => {
            let base = substitute_index(base, index_map, types);
            match radix_types::compose_index_affine_constant(types, base, offset) {
                Ok(term) => match radix_types::evaluate_index_dimension(term, types) {
                    Ok(Some(value)) => types.intern_index(IndexExpr::Literal(value)),
                    Ok(None) | Err(_) => term,
                },
                Err(_) => types.intern_index(IndexExpr::Affine { base, offset }),
            }
        }
        IndexExpr::Divide { numerator, divisor } => {
            let numerator = substitute_index(numerator, index_map, types);
            match radix_types::divide_index_exact(types, numerator, divisor) {
                Ok(term) => term,
                Err(_) => types.intern_index(IndexExpr::Divide { numerator, divisor }),
            }
        }
        other => types.intern_index(other),
    }
}

fn retarget_call(
    hir: &mut HirModule,
    old: DefId,
    new: DefId,
    span: Span,
    callee_span: Span,
    owner: Option<DefId>,
) {
    struct Retarget {
        old: DefId,
        new: DefId,
        span: Span,
        callee_span: Span,
    }
    impl HirVisitorMut for Retarget {
        fn visit_expr_mut(&mut self, expr: &mut HirExpression) {
            if let HirExpressionKind::Call(callee, _, _, _) = &mut expr.kind
                && let HirExpressionKind::Path(def_id) = &mut callee.kind
                && *def_id == self.old
                && (expr.span == self.span || callee.span == self.callee_span)
            {
                *def_id = self.new;
            }
            walk_expr_mut(self, expr);
        }
    }
    let mut retarget = Retarget {
        old,
        new,
        span,
        callee_span,
    };
    if let Some(owner) = owner {
        if let Some(item) = hir.items.iter_mut().find(|item| item.def_id == owner)
            && let HirItemKind::Function(function) = &mut item.kind
        {
            retarget.visit_function_mut(function);
        }
    } else {
        retarget.visit_program_mut(hir);
    }
}

struct IdAllocator {
    next_def_id: u32,
    next_hir_id: u32,
}

impl IdAllocator {
    fn from_module(hir: &HirModule) -> Self {
        let mut scan = IdScan::default();
        scan.visit_program(hir);
        for item in &hir.items {
            scan.max_hir = scan.max_hir.max(item.id.0);
            scan.max_def = scan.max_def.max(item.def_id.0);
        }
        Self {
            next_def_id: scan.max_def.saturating_add(1),
            next_hir_id: scan.max_hir.saturating_add(1),
        }
    }

    fn next_def(&mut self) -> DefId {
        let id = DefId(self.next_def_id);
        self.next_def_id = self.next_def_id.saturating_add(1);
        id
    }

    fn next_hir(&mut self) -> HirId {
        HirId(next_id(&mut self.next_hir_id))
    }
}

fn next_id(next: &mut u32) -> u32 {
    let id = *next;
    *next = (*next).saturating_add(1);
    id
}

#[derive(Default)]
struct IdScan {
    max_def: u32,
    max_hir: u32,
}

impl HirVisitor for IdScan {
    fn visit_def(&mut self, def_id: DefId, _name: Symbol) {
        self.max_def = self.max_def.max(def_id.0);
    }

    fn visit_expr(&mut self, expr: &HirExpression) {
        self.max_hir = self.max_hir.max(expr.id.0);
        walk_expr(self, expr);
    }

    fn visit_stmt(&mut self, stmt: &HirStatement) {
        self.max_hir = self.max_hir.max(stmt.id.0);
        radix_hir::visit::walk_stmt(self, stmt);
    }
}

#[cfg(test)]
#[path = "air_specialize_test.rs"]
mod tests;
