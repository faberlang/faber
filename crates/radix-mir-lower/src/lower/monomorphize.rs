//! Generic callable monomorphization before MIR validation.
//!
//! HIR lowering initially preserves source definition callees because ordinary
//! validation can type-check calls against semantic signatures. Systems targets
//! need a stronger fact: executable calls to generic definitions must become
//! concrete MIR functions before target classification or emission.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;
use radix_mir::generic::{
    ExistingTypeIndex, MirShapeBindings, bind_generic_type_witness_with_shapes, canonical_index,
    function_contains_shape_size, instantiate_generic_type_with_shapes_with_index, substitute_shape_size_constants,
    substitute_tensor_from_flat_shape_size_constants,
};
use radix_mir::ty::index_projection_result_ty;
use radix_mir::{
    MirFunctionIndex, MirGenericBindings, MirMonomorphization, MirMonomorphizationKey, MirPlaceBase, MirProgram,
    instantiate_existing_generic_type, type_contains_param,
};
use radix_semantic::IndexId;

const MAX_MONOMORPHIZATIONS: usize = 64;

#[derive(Debug, Clone)]
struct Rewrite {
    function: MirFunctionId,
    block_index: usize,
    location: CallLocation,
    target: MirFunctionId,
    /// True when `ensure_instance` created a fresh instance for this call.
    fresh_instance: bool,
}

#[derive(Debug, Clone)]
enum CallLocation {
    Statement(usize),
    Terminator,
}

/// Snapshot of one call site that targets a generic definition. The scan
/// phase holds the borrow into `lowerer.functions` while building these; the
/// instantiate phase replays the owned records with `&mut self`.
struct CallSite {
    function: MirFunctionId,
    block_index: usize,
    location: CallLocation,
    destination: Option<MirPlace>,
    callee: MirCallee,
    args: Vec<MirOperand>,
    error_place: Option<MirPlace>,
    span: Span,
    scope: FunctionTypeScope,
    shape_check: Option<MirShapeCheck>,
}

#[derive(Clone)]
struct FunctionTypeScope {
    locals: FxHashMap<MirLocalId, MirType>,
    temps: FxHashMap<MirTempId, MirType>,
    values: FxHashMap<MirValueId, MirType>,
}

struct CallWitness<'a> {
    caller_source: Option<DefId>,
    args: &'a [MirOperand],
    destination: Option<&'a MirPlace>,
    error_place: Option<&'a MirPlace>,
    span: Span,
    scope: &'a FunctionTypeScope,
    shape_check: Option<&'a MirShapeCheck>,
}

fn shape_check_before_call(
    function: &MirFunction,
    block_index: usize,
    location: &CallLocation,
    arguments: &[MirOperand],
) -> Option<MirShapeCheck> {
    let block = function.blocks.get(block_index)?;
    let previous = match location {
        CallLocation::Statement(index) => index.checked_sub(1),
        CallLocation::Terminator => block.statements.len().checked_sub(1),
    }?;
    let MirStatementKind::RuntimeCall { call, .. } = &block.statements.get(previous)?.kind else {
        return None;
    };
    if call.args.get(..arguments.len())? != arguments {
        return None;
    }
    let MirIntrinsic::TensorShapeCheck(check) = &call.intrinsic else {
        return None;
    };
    Some(check.clone())
}

fn collect_runtime_shape_conflicts(
    types: &TypeTable,
    expected: TypeId,
    actual: TypeId,
    bindings: &MirShapeBindings,
    conflicts: &mut Vec<(Symbol, u32)>,
) {
    fn payload(types: &TypeTable, mut ty: TypeId) -> TypeId {
        while let Type::Ref(_, inner) | Type::Alias(_, inner) = types.get(ty) {
            ty = *inner;
        }
        ty
    }
    fn visit(
        types: &TypeTable,
        expected: IndexId,
        actual: IndexId,
        axis: u32,
        bindings: &MirShapeBindings,
        conflicts: &mut Vec<(Symbol, u32)>,
    ) {
        match (types.get_index(expected), types.get_index(actual)) {
            (IndexExpr::Param(parameter), _) => {
                if let Some(bound) = bindings.get(parameter).copied()
                    && !types.index_equals(bound, actual)
                    && super::index_contains_runtime_extent(types, bound, &mut FxHashSet::default())
                    && super::index_contains_runtime_extent(types, actual, &mut FxHashSet::default())
                {
                    conflicts.push((*parameter, axis));
                }
            }
            (IndexExpr::Tuple(expected), IndexExpr::Tuple(actual)) if expected.len() == actual.len() => {
                for (axis, (expected, actual)) in expected.iter().zip(actual).enumerate() {
                    visit(types, *expected, *actual, axis as u32, bindings, conflicts);
                }
            }
            _ => {}
        }
    }
    let (Type::Tensor(_, expected), Type::Tensor(_, actual)) =
        (types.get(payload(types, expected)), types.get(payload(types, actual)))
    else {
        return;
    };
    visit(types, *expected, *actual, 0, bindings, conflicts);
}

fn formal_runtime_witness(types: &TypeTable, witness: IndexId) -> bool {
    matches!(
        types.get_index(witness),
        IndexExpr::Runtime(
            RuntimeExtentId::FunctionParameterAxis { .. } | RuntimeExtentId::FunctionParameterFieldAxis { .. }
        )
    )
}

fn shape_check_authorizes_runtime_conflicts(
    types: &TypeTable,
    check: Option<&MirShapeCheck>,
    params: &[OrderedParam],
    conflicts: &[(Symbol, u32)],
    current_argument: u32,
) -> bool {
    let Some(check) = check else {
        return false;
    };
    !conflicts.is_empty()
        && conflicts.iter().all(|(symbol, current_axis)| {
            let Some((ordinal, param)) = params
                .iter()
                .enumerate()
                .find(|(_, param)| param.name == *symbol && param.kind == ParamKind::Magnitudo)
            else {
                return false;
            };
            let Some(witness) = param.runtime_witness else {
                return false;
            };
            if !formal_runtime_witness(types, witness) {
                return false;
            }
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return false;
            };
            let Some(binding) = check
                .size_bindings
                .iter()
                .find(|binding| binding.parameter == ordinal)
            else {
                return false;
            };
            let IndexExpr::Runtime(RuntimeExtentId::FunctionParameterAxis { parameter, axis }) =
                types.get_index(witness)
            else {
                return false;
            };
            let first = MirShapeExpr::TensorAxis { argument: *parameter, axis: *axis };
            if !check.proves_equal(&binding.witness, &first) {
                return false;
            }
            let current = MirShapeExpr::TensorAxis { argument: current_argument, axis: *current_axis };
            check.proves_equal(&first, &current)
        })
}

pub(super) struct MonomorphizeOutput {
    pub monomorphizations: Vec<MirMonomorphization>,
    pub templates: Vec<MirFunction>,
    pub generic_params: Vec<(DefId, Vec<super::MirGenericParam>)>,
    pub tensor_limes_templates: Vec<super::MirTensorLimesTemplate>,
}

impl MirLowerer<'_> {
    pub(super) fn monomorphize(&mut self) -> MonomorphizeOutput {
        let mut state = MonomorphizationState::new(self);
        state.run();
        let generic_params = state
            .generic_param_order
            .iter()
            .map(|(source, params)| {
                (
                    *source,
                    params
                        .iter()
                        .map(|param| super::MirGenericParam {
                            name: param.name,
                            is_shape: param.kind == ParamKind::Magnitudo,
                            runtime_witness: param.runtime_witness,
                        })
                        .collect(),
                )
            })
            .collect();
        let tensor_limes_templates = state.tensor_limes_templates();
        MonomorphizeOutput {
            monomorphizations: state.monomorphizations,
            templates: state.templates.into_values().collect(),
            generic_params,
            tensor_limes_templates,
        }
    }
}

struct MonomorphizationState<'lowerer, 'unit> {
    lowerer: &'lowerer mut MirLowerer<'unit>,
    templates: FxHashMap<DefId, MirFunction>,
    generic_param_order: FxHashMap<DefId, Vec<OrderedParam>>,
    struct_fields: Arc<FxHashMap<DefId, FxHashMap<Symbol, MirType>>>,
    monomorphizations: Vec<MirMonomorphization>,
    instance_index: FxHashMap<MirMonomorphizationKey, MirFunctionId>,
    /// Function-id resolution over `lowerer.functions`; every append records
    /// its position so fresh instances resolve like the seed set.
    function_index: MirFunctionIndex,
    next_id: MirFunctionId,
    /// Contract bound dispatch (D1.1): contract method → its name, and
    /// `(genus, method) → method` for rewriting a bound call once its
    /// receiver witness is concrete.
    contract_methods: FxHashMap<DefId, Symbol>,
    genus_methods: FxHashMap<(DefId, Symbol), DefId>,
}

/// A-133: outcome of an explicit size witness lookup. `Found` carries the
/// resolved index; `MissingReported` means the lookup already pushed the more
/// specific diagnostic, so the caller must not repeat it; `MissingUnreported`
/// leaves the kind-mismatch diagnostic to the caller, so one failed argument
/// reports exactly once.
enum ExplicitShapeWitness {
    Found(IndexId),
    MissingReported,
    MissingUnreported,
}

impl<'lowerer, 'unit> MonomorphizationState<'lowerer, 'unit> {
    fn new(lowerer: &'lowerer mut MirLowerer<'unit>) -> Self {
        let mut generic_param_order = generic_param_order(lowerer.unit.hir);
        for (source, params) in &mut generic_param_order {
            let Some(bindings) = lowerer
                .unit
                .shape_obligations
                .function_size_bindings
                .get(source)
            else {
                continue;
            };
            for binding in bindings {
                if let Some(param) = usize::try_from(binding.parameter)
                    .ok()
                    .and_then(|ordinal| params.get_mut(ordinal))
                    && param.kind == ParamKind::Magnitudo
                {
                    param.runtime_witness = Some(binding.witness);
                }
            }
        }
        let struct_fields = lowerer.struct_field_types.take().unwrap_or_else(|| {
            // Retained from the earlier lowering pass; `run` aborts on the
            // recorded error before any instance reads the empty table.
            lowerer.errors.push(MirError::validation(
                Span::new(0, 0),
                "struct field types were not retained for monomorphization",
            ));
            Arc::new(FxHashMap::default())
        });
        let templates = lowerer
            .functions
            .iter()
            .filter_map(|function| {
                let source = function.source?;
                generic_param_order
                    .contains_key(&source)
                    .then_some((source, function.clone()))
            })
            .collect();
        let next_id = if let Ok(id) = MirProgram::allocate_function_id(&lowerer.functions) {
            id
        } else {
            // Unreachable minting: `run` aborts on lowering errors before
            // any instance allocation.
            lowerer.errors.push(MirError::validation(
                Span::new(0, 0),
                "package MIR function id space is exhausted",
            ));
            MirFunctionId(u32::MAX)
        };
        let function_index = MirFunctionIndex::for_functions(&lowerer.functions);
        let (mut contract_methods, genus_methods) = contract_dispatch_tables(lowerer.unit.hir);
        // D1.4: ordering-bound calls name the contract itself (it may be
        // imported); a genus witness resolves to its own `compare`.
        if let (Some(contract), Some(compare)) = (
            lowerer.types.ordering_contract,
            lowerer
                .interner
                .lookup(radix_semantic::radix_contract::ORDERING_METHOD),
        ) {
            contract_methods.insert(contract, compare);
        }
        Self {
            lowerer,
            templates,
            generic_param_order,
            struct_fields,
            monomorphizations: Vec::new(),
            instance_index: FxHashMap::default(),
            function_index,
            next_id,
            contract_methods,
            genus_methods,
        }
    }

    fn tensor_limes_templates(&mut self) -> Vec<super::MirTensorLimesTemplate> {
        let pending = self.lowerer.deferred_tensor_limes.clone();
        let mut templates = Vec::with_capacity(pending.len());
        for pending in pending {
            let Some(template) = self
                .templates
                .values()
                .find(|template| template.id == pending.function)
            else {
                self.lowerer.errors.push(MirError::unsupported(
                    pending.span,
                    "deferred tensor limes has no generic template owner",
                ));
                continue;
            };
            let Some(source) = template.source else {
                self.lowerer.errors.push(MirError::unsupported(
                    pending.span,
                    "deferred tensor limes template has no source identity",
                ));
                continue;
            };
            let PendingTensorLimesDispatch::Deferred { policy_type } = pending.dispatch else {
                continue;
            };
            let Some(policy_symbol) = tensor_edge_policy_param_symbol(self.types(), policy_type) else {
                self.lowerer.errors.push(MirError::unsupported(
                    pending.span,
                    "deferred tensor limes policy is not a generic type parameter",
                ));
                continue;
            };
            let Some(parameters) = self.generic_param_order.get(&source) else {
                self.lowerer.errors.push(MirError::unsupported(
                    pending.span,
                    "deferred tensor limes template has no generic parameter metadata",
                ));
                continue;
            };
            let Some(policy_type_argument) = typus_param_ordinal(parameters, policy_symbol) else {
                self.lowerer.errors.push(MirError::unsupported(
                    pending.span,
                    "deferred tensor limes policy does not name a typus parameter",
                ));
                continue;
            };
            let call_return_ty = deferred_limes_placeholder_return_ty(template, pending.destination);
            let Some((_, rank)) = call_return_ty.and_then(|ty| tensor_element_and_rank(self.types(), ty.semantic_id()))
            else {
                self.lowerer.errors.push(MirError::unsupported(
                    pending.span,
                    "deferred tensor limes template result has no static tensor rank",
                ));
                continue;
            };
            templates.push(super::MirTensorLimesTemplate {
                source,
                destination: pending.destination,
                policy_type_argument,
                rank,
            });
        }
        templates
    }

    fn run(&mut self) {
        // Only newly created instances can contain generic call sites that the
        // seed pass has not patched yet, so scanning one function at a time is
        // complete; the old fixpoint re-scanned every function each round and
        // deep-cloned the whole function vec as a borrow-checker workaround.
        let mut worklist = self.seed_worklist();
        while let Some(function_id) = worklist.pop() {
            if !self.lowerer.errors.is_empty() {
                break;
            }
            let call_sites = {
                let Some(function) = self
                    .function_index
                    .function(&self.lowerer.functions, function_id)
                else {
                    continue;
                };
                self.collect_call_sites(function)
            };
            if call_sites.is_empty() {
                continue;
            }
            let mut rewrites = Vec::with_capacity(call_sites.len());
            for site in call_sites {
                if let Some(rewrite) = self.rewrite_for_call(site) {
                    if rewrite.fresh_instance {
                        worklist.push(rewrite.target);
                    }
                    rewrites.push(rewrite);
                }
            }
            self.apply_rewrites(rewrites);
        }

        self.remove_templates();
    }

    /// Non-template functions whose bodies already call a generic definition.
    /// These seed the worklist; every later entry is a freshly created instance.
    ///
    /// A param-bearing function is skipped: a genus method carries its genus's
    /// type parameters (genus methods have no per-genus instance to be written
    /// against), so every witness its body can offer for a nested generic call
    /// is its own unresolved parameter. Instantiating from it would mint a
    /// param-bearing instance — exactly the survivor the final net rejects —
    /// so the call stays a `Definition` callee and resolves to the retained
    /// template by `DefId`, matching how the method itself is executed.
    fn seed_worklist(&self) -> Vec<MirFunctionId> {
        self.lowerer
            .functions
            .iter()
            .filter(|function| {
                !self.is_template_function(function)
                    && !function_contains_param(self.types(), function)
                    && self.function_has_generic_callee(function)
            })
            .map(|function| function.id)
            .collect()
    }

    /// True when any statement or terminator call targets a generic definition
    /// (the same guard `rewrite_for_call` applies). Cheap seed predicate; the
    /// scan phase re-walks matching functions for exact call-site metadata.
    /// A `cursor_stream` runtime call names its generic generator through the
    /// intrinsic's `DefId`, not a `MirCallee`, so it is matched here too.
    fn function_has_generic_callee(&self, function: &MirFunction) -> bool {
        function.blocks.iter().any(|block| {
            block.statements.iter().any(|statement| {
                matches!(
                    &statement.kind,
                    MirStatementKind::Call { callee, .. } if self.is_generic_callee(callee)
                ) || matches!(
                    &statement.kind,
                    MirStatementKind::RuntimeCall { call, .. }
                        if self.is_generic_cursor_stream(&call.intrinsic)
                )
            }) || matches!(
                &block.terminator.kind,
                MirTerminatorKind::TryCall { callee, .. } if self.is_generic_callee(callee)
            )
        })
    }

    /// Walk one function and snapshot every call site that may require a
    /// rewrite. The owned snapshot (operands/places/callee plus the type scope
    /// at that point in the walk) lets the instantiate phase mutate
    /// `lowerer.functions` without holding a borrow into it.
    fn collect_call_sites(&self, function: &MirFunction) -> Vec<CallSite> {
        let mut sites = Vec::new();
        let mut scope = FunctionTypeScope::from_function(function);
        for (block_index, block) in function.blocks.iter().enumerate() {
            for (statement_index, statement) in block.statements.iter().enumerate() {
                match &statement.kind {
                    MirStatementKind::Assign { value, .. } => {
                        scope.values.insert(value.id, value.ty);
                    }
                    MirStatementKind::Call { destination, callee, args } => {
                        if self.is_generic_callee(callee) {
                            let location = CallLocation::Statement(statement_index);
                            sites.push(CallSite {
                                function: function.id,
                                block_index,
                                location: location.clone(),
                                destination: destination.clone(),
                                callee: callee.clone(),
                                args: args.clone(),
                                error_place: None,
                                span: statement.span,
                                scope: scope.clone(),
                                shape_check: shape_check_before_call(function, block_index, &location, args),
                            });
                        }
                    }
                    MirStatementKind::RuntimeCall { call, .. } => {
                        // A generic generator call is not a `MirCallee`; its
                        // template sits in the intrinsic. Instantiating it
                        // here leaves the intrinsic's `DefId` in place: the
                        // concrete instance keeps the template source, so the
                        // runner's `DefId` index resolves it to the instance.
                        // The destination is the cursor carrier, not the
                        // template's element return type, so it is not a type
                        // witness; the arguments bind the instance.
                        if let MirIntrinsic::CursorStream(stream) = &call.intrinsic
                            && self.generic_param_order.contains_key(&stream.callee)
                        {
                            let location = CallLocation::Statement(statement_index);
                            sites.push(CallSite {
                                function: function.id,
                                block_index,
                                location: location.clone(),
                                destination: None,
                                callee: MirCallee::Definition { source: stream.callee, type_args: Vec::new() },
                                args: call.args.clone(),
                                error_place: None,
                                span: statement.span,
                                scope: scope.clone(),
                                shape_check: shape_check_before_call(function, block_index, &location, &call.args),
                            });
                        }
                    }
                    MirStatementKind::Construct { .. } => {}
                }
            }

            if let MirTerminatorKind::TryCall { destination, callee, args, error_place, .. } = &block.terminator.kind
                && self.is_generic_callee(callee)
            {
                let location = CallLocation::Terminator;
                sites.push(CallSite {
                    function: function.id,
                    block_index,
                    location: location.clone(),
                    destination: destination.clone(),
                    callee: callee.clone(),
                    args: args.clone(),
                    error_place: Some(error_place.clone()),
                    span: block.terminator.span,
                    scope: scope.clone(),
                    shape_check: shape_check_before_call(function, block_index, &location, args),
                });
            }
        }
        sites
    }

    fn is_generic_callee(&self, callee: &MirCallee) -> bool {
        matches!(
            callee,
            MirCallee::Definition { source, .. } if self.generic_param_order.contains_key(source)
        )
    }

    fn is_generic_cursor_stream(&self, intrinsic: &MirIntrinsic) -> bool {
        matches!(
            intrinsic,
            MirIntrinsic::CursorStream(stream) if self.generic_param_order.contains_key(&stream.callee)
        )
    }

    fn rewrite_for_call(&mut self, site: CallSite) -> Option<Rewrite> {
        let CallSite {
            function,
            block_index,
            location,
            destination,
            callee,
            args,
            error_place,
            span,
            scope,
            shape_check,
        } = site;
        let MirCallee::Definition { source, type_args: explicit_type_args } = &callee else {
            return None;
        };
        if !self.generic_param_order.contains_key(source) {
            return None;
        }

        let template = self.templates.get(source).cloned().or_else(|| {
            let error = if self.lowerer.unit.radix_lanes.lane_for(*source) == Some(radix_semantic::RadixLane::Air) {
                MirError::missing_generic_air_template(span, source.0)
            } else {
                MirError::validation(span, format!("generic definition #{} has no MIR template", source.0))
            };
            self.lowerer.errors.push(error);
            None
        })?;

        let caller_source = self
            .lowerer
            .functions
            .iter()
            .find(|candidate| candidate.id == function)
            .and_then(|f| f.source);
        let witness = CallWitness {
            caller_source,
            args: &args,
            destination: destination.as_ref(),
            error_place: error_place.as_ref(),
            span,
            scope: &scope,
            shape_check: shape_check.as_ref(),
        };
        let parent_shape_bindings = self.instance_shape_bindings(function);
        let key = self.key_for_call(&template, explicit_type_args, &witness, &parent_shape_bindings)?;
        let (target, fresh_instance) = self.ensure_instance(&template, &key, span)?;
        Some(Rewrite { function, block_index, location, target, fresh_instance })
    }

    /// Reconstruct the concrete shape environment of the instance containing
    /// a nested call. Fresh instances are scanned before the final mutable
    /// shape-substitution pass, so their body types may still carry the
    /// template's `IndexExpr::Param` nodes. Resolving the child key through
    /// the parent's already-canonical key keeps worklist identity concrete.
    fn instance_shape_bindings(&self, function: MirFunctionId) -> MirShapeBindings {
        let Some(monomorphization) = self
            .monomorphizations
            .iter()
            .find(|monomorphization| monomorphization.function == function)
        else {
            return MirShapeBindings::default();
        };
        let Some(params) = self.generic_param_order.get(&monomorphization.key.source) else {
            return MirShapeBindings::default();
        };
        shape_bindings_for_key(params, &monomorphization.key.shape_args)
    }

    fn key_for_call(
        &mut self,
        template: &MirFunction,
        explicit_type_args: &[MirType],
        witness: &CallWitness<'_>,
        parent_shape_bindings: &MirShapeBindings,
    ) -> Option<MirMonomorphizationKey> {
        let CallWitness { caller_source, args, destination, error_place, span, scope, shape_check } = *witness;
        let (source, params, mut bindings, mut shape_bindings) =
            self.prepare_call_bindings(template, explicit_type_args, caller_source, span)?;
        for (index, arg) in args.iter().enumerate() {
            let Some(expected) = template.params.get(index) else {
                continue;
            };
            let actual = self.operand_ty(arg, scope, span)?;
            let mut trial_bindings = bindings.clone();
            let mut trial_shapes = shape_bindings.clone();
            if bind_generic_type_witness_with_shapes(
                self.types(),
                expected.ty,
                actual,
                &mut trial_bindings,
                &mut trial_shapes,
            ) {
                bindings = trial_bindings;
                shape_bindings = trial_shapes;
                continue;
            }

            let mut conflicts = Vec::new();
            collect_runtime_shape_conflicts(
                self.types(),
                expected.ty.semantic_id(),
                actual.semantic_id(),
                &shape_bindings,
                &mut conflicts,
            );
            if shape_check_authorizes_runtime_conflicts(
                self.types(),
                shape_check,
                &params,
                &conflicts,
                u32::try_from(index).unwrap_or(u32::MAX),
            ) {
                let conflicting_params = conflicts
                    .iter()
                    .map(|(parameter, _)| *parameter)
                    .collect::<FxHashSet<_>>();
                let mut retry_bindings = bindings.clone();
                let mut retry_shapes = shape_bindings.clone();
                for parameter in &conflicting_params {
                    retry_shapes.remove(parameter);
                }
                if bind_generic_type_witness_with_shapes(
                    self.types(),
                    expected.ty,
                    actual,
                    &mut retry_bindings,
                    &mut retry_shapes,
                ) {
                    bindings = retry_bindings;
                    for (parameter, witness) in retry_shapes {
                        shape_bindings.entry(parameter).or_insert(witness);
                    }
                    continue;
                }
            }

            {
                let expected_ty = self.format_ty(expected.ty);
                let actual_ty = self.format_ty(actual);
                self.lowerer.errors.push(MirError {
                    message: format!(
                        "invalid MIR: call argument type mismatch (arg {index}: expected {expected_ty}, got {actual_ty})"
                    ),
                    issue: "invalid_mir_call_argument_type_mismatch".to_owned(),
                    span,
                    definition_span: None,
                });
                return None;
            }
        }

        self.bind_call_destination(template.return_ty, destination, scope, span, &mut bindings, &mut shape_bindings)?;

        if let (Some(expected), Some(error_place)) = (template.error_ty, error_place) {
            self.bind_call_error_place(expected, error_place, scope, span, &mut bindings, &mut shape_bindings)?;
        }

        let (type_args, shape_args) =
            self.build_call_type_args(&params, &bindings, &shape_bindings, parent_shape_bindings, span)?;
        Some(MirMonomorphizationKey { source, type_args, shape_args })
    }

    fn prepare_call_bindings(
        &mut self,
        template: &MirFunction,
        explicit_type_args: &[MirType],
        caller_source: Option<DefId>,
        span: Span,
    ) -> Option<(DefId, Vec<OrderedParam>, MirGenericBindings, MirShapeBindings)> {
        let source = template.source?;
        let params = self.generic_param_order.get(&source)?.clone();
        let mut bindings = MirGenericBindings::default();
        let mut shape_bindings = MirShapeBindings::default();
        self.bind_explicit_type_args(
            caller_source,
            source,
            explicit_type_args,
            &mut bindings,
            &mut shape_bindings,
            span,
        )?;
        Some((source, params, bindings, shape_bindings))
    }

    fn bind_call_destination(
        &mut self,
        return_ty: MirType,
        destination: Option<&MirPlace>,
        scope: &FunctionTypeScope,
        span: Span,
        bindings: &mut MirGenericBindings,
        shape_bindings: &mut MirShapeBindings,
    ) -> Option<()> {
        let Some(destination) = destination else {
            return Some(());
        };
        let actual = self.place_ty(destination, scope, span)?;
        // The destination type of a call is the callee's return type with the
        // call's own extents substituted. Extents already bound from the
        // arguments are authoritative: a run-time axis of the destination
        // (the call expression's own result axis) is the same value read back,
        // not a second witness, so it never conflicts with them.
        let mut trial_bindings = bindings.clone();
        let mut trial_shapes = shape_bindings.clone();
        if bind_generic_type_witness_with_shapes(
            self.types(),
            return_ty,
            actual,
            &mut trial_bindings,
            &mut trial_shapes,
        ) {
            *bindings = trial_bindings;
            *shape_bindings = trial_shapes;
        } else {
            let mut conflicts = Vec::new();
            collect_runtime_shape_conflicts(
                self.types(),
                return_ty.semantic_id(),
                actual.semantic_id(),
                shape_bindings,
                &mut conflicts,
            );
            let mut retry_bindings = bindings.clone();
            let mut retry_shapes = shape_bindings.clone();
            for (parameter, _) in &conflicts {
                retry_shapes.remove(parameter);
            }
            if conflicts.is_empty()
                || !bind_generic_type_witness_with_shapes(
                    self.types(),
                    return_ty,
                    actual,
                    &mut retry_bindings,
                    &mut retry_shapes,
                )
            {
                self.lowerer
                    .errors
                    .push(MirError::validation(span, "destination type mismatch"));
                return None;
            }
            *bindings = retry_bindings;
            for (parameter, witness) in retry_shapes {
                shape_bindings.entry(parameter).or_insert(witness);
            }
        }
        Some(())
    }

    fn bind_call_error_place(
        &mut self,
        error_ty: MirType,
        error_place: &MirPlace,
        scope: &FunctionTypeScope,
        span: Span,
        bindings: &mut MirGenericBindings,
        shape_bindings: &mut MirShapeBindings,
    ) -> Option<()> {
        let actual = self.place_ty(error_place, scope, span)?;
        if !bind_generic_type_witness_with_shapes(self.types(), error_ty, actual, bindings, shape_bindings) {
            self.lowerer
                .errors
                .push(MirError::validation(span, "error destination type mismatch"));
            return None;
        }
        Some(())
    }

    fn build_call_type_args(
        &mut self,
        params: &[OrderedParam],
        bindings: &MirGenericBindings,
        shape_bindings: &MirShapeBindings,
        parent_shape_bindings: &MirShapeBindings,
        span: Span,
    ) -> Option<(Vec<MirType>, Vec<IndexId>)> {
        let mut type_args = Vec::with_capacity(params.len());
        let mut shape_args = Vec::with_capacity(params.len());
        for param in params {
            match param.kind {
                ParamKind::Typus => {
                    let Some(bound) = bindings.get(&param.name).copied() else {
                        self.lowerer.errors.push(MirError::validation(
                            span,
                            "generic call signature has unresolved type parameter",
                        ));
                        return None;
                    };
                    type_args.push(bound);
                }
                ParamKind::Magnitudo => {
                    let Some(bound) = shape_bindings.get(&param.name).copied() else {
                        self.lowerer.errors.push(MirError::validation(
                            span,
                            "generic call signature has unresolved type parameter",
                        ));
                        return None;
                    };
                    let contains_runtime =
                        super::index_contains_runtime_extent(self.types(), bound, &mut FxHashSet::default());
                    let concrete = if contains_runtime {
                        // A runtime-bearing bound without a formal carrier
                        // witness stays fail-closed: the origin key would be
                        // unstable across monomorphization sites.
                        param.runtime_witness
                    } else {
                        resolve_parent_shape_binding(self.types(), bound, parent_shape_bindings)
                    };
                    let Some(concrete) = concrete else {
                        self.lowerer.errors.push(MirError::validation(
                            span,
                            if contains_runtime {
                                "runtime generic shape has no stable formal carrier witness"
                            } else {
                                "generic call signature has unresolved type parameter"
                            },
                        ));
                        return None;
                    };
                    shape_args.push(canonical_index(self.types(), concrete));
                }
            }
        }
        Some((type_args, shape_args))
    }

    fn bind_explicit_type_args(
        &mut self,
        caller_source: Option<DefId>,
        source: DefId,
        explicit_type_args: &[MirType],
        bindings: &mut MirGenericBindings,
        shape_bindings: &mut MirShapeBindings,
        span: Span,
    ) -> Option<()> {
        if explicit_type_args.is_empty() {
            return Some(());
        }
        let Some(params) = self.generic_param_order.get(&source).cloned() else {
            return Some(());
        };
        let type_params = params
            .iter()
            .filter(|param| param.kind == ParamKind::Typus)
            .collect::<Vec<_>>();
        let shape_params = params
            .iter()
            .filter(|param| param.kind == ParamKind::Magnitudo)
            .collect::<Vec<_>>();
        // The explicit list binds by marker: a `size` argument (a NATURAL or
        // shape spelling, or a caller-owned symbolic size `Param` that
        // resolves through the calling instance's shape bindings, GSF-1)
        // binds the next magnitudo param, every other argument the next
        // Typus param, and the two cursors advance independently.
        let (type_args, size_args): (Vec<MirType>, Vec<MirType>) = explicit_type_args
            .iter()
            .copied()
            .partition(|concrete| !self.explicit_arg_is_size(*concrete));
        if !size_args.is_empty() {
            if size_args.len() != shape_params.len() {
                self.lowerer.errors.push(MirError::validation(
                    span,
                    format!(
                        "generic call supplied {} size arguments for {} parameters",
                        size_args.len(),
                        shape_params.len()
                    ),
                ));
                return None;
            }
            for (param, concrete) in shape_params.iter().zip(size_args.iter().copied()) {
                let index = match self.explicit_shape_witness(caller_source, concrete, span) {
                    ExplicitShapeWitness::Found(index) => index,
                    ExplicitShapeWitness::MissingReported => return None,
                    ExplicitShapeWitness::MissingUnreported => {
                        self.lowerer.errors.push(MirError::validation(
                            span,
                            "generic call type argument does not match parameter kind",
                        ));
                        return None;
                    }
                };
                shape_bindings.insert(param.name, canonical_index(self.types(), index));
            }
        }
        if type_args.is_empty() {
            return Some(());
        }
        if type_args.len() != type_params.len() {
            self.lowerer.errors.push(MirError::validation(
                span,
                format!(
                    "generic call supplied {} type arguments for {} parameters",
                    type_args.len(),
                    type_params.len()
                ),
            ));
            return None;
        }
        for (param, concrete) in type_params.iter().zip(type_args.iter().copied()) {
            if let Some(existing) = bindings.get(&param.name).copied() {
                if existing != concrete {
                    self.lowerer.errors.push(MirError::validation(
                        span,
                        "explicit generic type argument conflicts with witness",
                    ));
                    return None;
                }
            } else {
                bindings.insert(param.name, concrete);
            }
        }
        Some(())
    }

    /// Whether an explicit call argument is a `size` argument: the marker
    /// reaches MIR in the spelling of the lowered `Type::Param`
    /// (`radix_types::parse_explicit_size`).
    fn explicit_arg_is_size(&self, concrete: MirType) -> bool {
        self.explicit_size_of(concrete).is_some()
    }

    fn explicit_size_of(&self, concrete: MirType) -> Option<radix_types::ExplicitSize<'_>> {
        let Type::Param(sym) = self.types().get(concrete.semantic_id()) else {
            return None;
        };
        radix_types::parse_explicit_size(self.lowerer.interner.resolve(*sym))
    }

    /// `DefId` of the local or parameter spelled `sym` that a run-time
    /// `size n` argument names, searched in the whole program and then in the
    /// calling function's parameters.
    fn value_binding_named(&self, caller_source: Option<DefId>, sym: Symbol) -> Option<DefId> {
        struct LocalFinder {
            name: Symbol,
            found: Option<DefId>,
        }
        impl radix_hir::visit::HirVisitor for LocalFinder {
            fn visit_param(&mut self, param: &radix_hir::HirParam) {
                if param.name == self.name {
                    self.found = Some(param.def_id);
                }
            }
            fn visit_local(&mut self, local: &radix_hir::HirLocal) {
                if local.name == self.name {
                    self.found = Some(local.def_id);
                }
                radix_hir::visit::walk_local(self, local);
            }
        }
        let mut finder = LocalFinder { name: sym, found: None };
        finder.visit_program(self.lowerer.unit.hir);
        finder.found.or_else(|| {
            caller_source.and_then(|source| {
                self.lowerer
                    .unit
                    .hir
                    .items
                    .iter()
                    .find(|item| item.def_id == source)
                    .and_then(|item| match &item.kind {
                        radix_hir::HirItemKind::Function(function) => function
                            .params
                            .iter()
                            .find(|p| p.name == sym)
                            .map(|p| p.def_id),
                        _ => None,
                    })
            })
        })
    }

    /// Index witness of an explicit `size` argument: a NATURAL maps to its
    /// literal index, a bracketed shape to its tuple of literals, a name to
    /// the run-time extent of an in-scope value, or to a caller-owned
    /// symbolic `size` param kept as the rigid param node (GSF-1 forwarding),
    /// whose key derivation resolves it through the calling instance's shape
    /// bindings. Anything else fails closed so wrong-kind rejection is
    /// preserved.
    fn explicit_shape_witness(
        &mut self,
        caller_source: Option<DefId>,
        concrete: MirType,
        span: Span,
    ) -> ExplicitShapeWitness {
        let unresolved = |this: &mut Self| {
            this.lowerer.errors.push(MirError::validation(
                span,
                "generic call signature has unresolved type parameter",
            ));
            ExplicitShapeWitness::MissingReported
        };
        let Some(size) = self.explicit_size_of(concrete) else {
            return ExplicitShapeWitness::MissingUnreported;
        };
        let name = match size {
            radix_types::ExplicitSize::Natural(value) => {
                return match find_literal_index(self.types(), value) {
                    Some(index) => ExplicitShapeWitness::Found(index),
                    None => unresolved(self),
                };
            }
            radix_types::ExplicitSize::Shape(dims) => {
                return match find_literal_shape_index(self.types(), &dims) {
                    Some(index) => ExplicitShapeWitness::Found(index),
                    None => unresolved(self),
                };
            }
            radix_types::ExplicitSize::Name(name) => name.to_owned(),
        };
        let Some(sym) = self.lowerer.interner.lookup(&name) else {
            return unresolved(self);
        };
        if let Some(binding) = self.value_binding_named(caller_source, sym) {
            return self.lowerer.unit.shape_obligations.call_size_bindings.values().flatten()
                .find(|row| matches!(self.types().get_index(row.witness), IndexExpr::Runtime(RuntimeExtentId::ScalarBinding(found)) if *found == binding))
                .map_or(ExplicitShapeWitness::MissingUnreported, |row| ExplicitShapeWitness::Found(row.witness));
        }
        if let Some(index) = find_param_index(self.types(), sym) {
            ExplicitShapeWitness::Found(index)
        } else {
            self.lowerer.errors.push(MirError::validation(
                span,
                "generic call type argument does not match parameter kind",
            ));
            ExplicitShapeWitness::MissingReported
        }
    }

    fn ensure_instance(
        &mut self,
        template: &MirFunction,
        key: &MirMonomorphizationKey,
        span: Span,
    ) -> Option<(MirFunctionId, bool)> {
        if let Some(existing) = self.instance_index.get(key).copied() {
            return Some((existing, false));
        }
        if self.monomorphizations.len() >= MAX_MONOMORPHIZATIONS {
            self.lowerer.errors.push(MirError::validation(
                span,
                format!("generic monomorphization limit exceeded ({MAX_MONOMORPHIZATIONS})"),
            ));
            return None;
        }

        let source = template.source?;
        let params = self.generic_param_order.get(&source)?;
        let mut bindings = MirGenericBindings::default();
        let mut type_args = key.type_args.iter();
        for param in params {
            if param.kind == ParamKind::Typus {
                bindings.insert(param.name, *type_args.next()?);
            }
        }

        let id = self.next_id;
        self.next_id = MirFunctionId(self.next_id.0 + 1);
        let mut instance = template.clone();
        instance.id = id;
        if !self.substitute_function(&mut instance, &bindings, span) {
            return None;
        }
        if !self.specialize_synthetic_callees(&mut instance, &bindings, span) {
            return None;
        }
        if !self.resolve_deferred_tensor_limes(template.id, key, &mut instance, span) {
            return None;
        }

        self.function_index.insert(id, self.lowerer.functions.len());
        self.lowerer.functions.push(instance);
        // Device-role rows are keyed by function id, so a minted instance
        // starts without the template's explicit facts. Copy (not move) the
        // template row with the instance as its parent — package
        // instantiation still reads the original template row.
        if let Some(mut metadata) = self
            .lowerer
            .device_function_metadata
            .get(&template.id)
            .copied()
        {
            metadata.identity.parent = id;
            self.lowerer.device_function_metadata.insert(id, metadata);
        }
        self.monomorphizations
            .push(MirMonomorphization { key: key.clone(), function: id });
        self.instance_index.insert(key.clone(), id);
        Some((id, true))
    }

    /// Resolve deferred `limes<P>()` selectors in this concrete clone. The
    /// semantic discharge pass owns the checked provider rows; MIR only binds
    /// the actual generic type witness to its already validated dispatch.
    fn resolve_deferred_tensor_limes(
        &mut self,
        template_id: MirFunctionId,
        key: &MirMonomorphizationKey,
        function: &mut MirFunction,
        span: Span,
    ) -> bool {
        let pending = self
            .lowerer
            .deferred_tensor_limes
            .iter()
            .copied()
            .filter(|pending| pending.function == template_id)
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return true;
        }
        let Some(params) = self.generic_param_order.get(&key.source).cloned() else {
            self.lowerer.errors.push(MirError::unsupported(
                span,
                "deferred tensor limes has no generic parameter metadata",
            ));
            return false;
        };
        let callback_ids = self
            .lowerer
            .functions
            .iter()
            .filter_map(|candidate| candidate.source.map(|source| (source, candidate.id)))
            .collect::<FxHashMap<_, _>>();

        for pending in pending {
            let PendingTensorLimesDispatch::Deferred { policy_type } = pending.dispatch else {
                continue;
            };
            let Some(symbol) = tensor_edge_policy_param_symbol(self.types(), policy_type) else {
                self.lowerer.errors.push(MirError::unsupported(
                    pending.span,
                    "deferred tensor limes policy is not a generic type parameter",
                ));
                return false;
            };
            let Some(actual_policy) = typus_param_ordinal(&params, symbol)
                .and_then(|ordinal| key.type_args.get(ordinal))
                .map(|ty| ty.semantic_id())
            else {
                self.lowerer.errors.push(MirError::unsupported(
                    pending.span,
                    "deferred tensor limes policy has no concrete specialization argument",
                ));
                return false;
            };

            let Some(dispatch) = self.resolve_deferred_limes_dispatch(function, &pending, actual_policy, &callback_ids)
            else {
                return false;
            };
            let Some(statement) = function
                .blocks
                .iter_mut()
                .flat_map(|block| &mut block.statements)
                .find(|statement| is_deferred_limes_placeholder(statement, pending.destination))
            else {
                self.lowerer.errors.push(MirError::unsupported(
                    pending.span,
                    "deferred tensor limes placeholder changed during specialization",
                ));
                return false;
            };
            if let MirStatementKind::RuntimeCall { call, .. } = &mut statement.kind {
                call.intrinsic = MirIntrinsic::TensorLimes(dispatch);
            }
        }
        true
    }

    fn resolve_deferred_limes_dispatch(
        &mut self,
        function: &MirFunction,
        pending: &PendingTensorLimes,
        actual_policy: TypeId,
        callback_ids: &FxHashMap<DefId, MirFunctionId>,
    ) -> Option<MirEdgePolicy> {
        let Some(call_return_ty) = deferred_limes_placeholder_return_ty(function, pending.destination) else {
            self.lowerer.errors.push(MirError::unsupported(
                pending.span,
                "deferred tensor limes placeholder is missing from its concrete instance",
            ));
            return None;
        };
        let Some((element_type, rank)) = tensor_element_and_rank(self.types(), call_return_ty.semantic_id()) else {
            self.lowerer.errors.push(MirError::unsupported(
                pending.span,
                "deferred tensor limes result is not a ranked tensor",
            ));
            return None;
        };
        let providers = self
            .lowerer
            .unit
            .tensor_edge_policies
            .provider_impls
            .iter()
            .copied()
            .filter(|provider| {
                same_type_identity(self.types(), provider.policy_type, actual_policy)
                    && same_type_identity(self.types(), provider.element_type, element_type)
                    && provider.rank == rank
            })
            .collect::<Vec<_>>();
        let [provider] = providers.as_slice() else {
            self.lowerer.errors.push(MirError::unsupported(
                pending.span,
                "generic tensor limes specialization has no unique checked policy provider",
            ));
            return None;
        };
        let dispatch = match provider.policy {
            radix_hir::ResolvedTensorEdgePolicy::Builtin(policy) => MirEdgePolicy::Builtin(match policy {
                radix_types::BuiltinTensorEdgePolicy::Clamp => MirBuiltinEdgePolicy::Clamp,
                radix_types::BuiltinTensorEdgePolicy::Reflect => MirBuiltinEdgePolicy::Reflect,
                radix_types::BuiltinTensorEdgePolicy::Wrap => MirBuiltinEdgePolicy::Wrap,
            }),
            radix_hir::ResolvedTensorEdgePolicy::Custom { policy_def, remap, transform } => {
                let Some(remap) = callback_ids.get(&remap).copied() else {
                    self.lowerer.errors.push(MirError::unsupported(
                        pending.span,
                        "generic tensor limes remap callback has no lowered function",
                    ));
                    return None;
                };
                let transform = match transform {
                    Some(def_id) => {
                        if let Some(function) = callback_ids.get(&def_id).copied() {
                            Some(function)
                        } else {
                            self.lowerer.errors.push(MirError::unsupported(
                                pending.span,
                                "generic tensor limes transform callback has no lowered function",
                            ));
                            return None;
                        }
                    }
                    None => None,
                };
                MirEdgePolicy::Custom { policy_def, remap, transform }
            }
            radix_hir::ResolvedTensorEdgePolicy::Deferred { .. } => {
                self.lowerer.errors.push(MirError::unsupported(
                    pending.span,
                    "generic tensor limes provider remained deferred after specialization",
                ));
                return None;
            }
        };
        Some(dispatch)
    }

    /// Specialize call sites whose callee is a synthetic, param-bearing
    /// helper (`source == None`). `substitute_function` leaves
    /// `MirCallee::Function(_)` untouched (`substitute_callee` only rewrites
    /// `Definition` callees), so a generic instance would otherwise call a
    /// helper whose `Convert` still names `T`. Each offending helper is
    /// cloned once per instance, substituted, recursively specialized, and
    /// registered under a fresh id; the original param-bearing helper stays.
    fn specialize_synthetic_callees(
        &mut self,
        function: &mut MirFunction,
        bindings: &MirGenericBindings,
        span: Span,
    ) -> bool {
        let mut specialized: FxHashMap<MirFunctionId, MirFunctionId> = FxHashMap::default();
        for block in &mut function.blocks {
            for statement in &mut block.statements {
                if let MirStatementKind::Call { callee, .. } = &mut statement.kind
                    && !self.specialize_synthetic_callee(callee, bindings, span, &mut specialized)
                {
                    return false;
                }
            }
            if let MirTerminatorKind::TryCall { callee, .. } = &mut block.terminator.kind
                && !self.specialize_synthetic_callee(callee, bindings, span, &mut specialized)
            {
                return false;
            }
        }
        true
    }

    fn specialize_synthetic_callee(
        &mut self,
        callee: &mut MirCallee,
        bindings: &MirGenericBindings,
        span: Span,
        specialized: &mut FxHashMap<MirFunctionId, MirFunctionId>,
    ) -> bool {
        let MirCallee::Function(helper) = callee else {
            return true;
        };
        let helper = *helper;
        if let Some(&id) = specialized.get(&helper) {
            *callee = MirCallee::Function(id);
            return true;
        }
        let Some(candidate) = self
            .function_index
            .function(&self.lowerer.functions, helper)
        else {
            return true;
        };
        if candidate.source.is_some() || !function_contains_param(self.types(), candidate) {
            return true;
        }
        let mut clone = candidate.clone();
        let id = self.next_id;
        self.next_id = MirFunctionId(self.next_id.0 + 1);
        clone.id = id;
        if !self.substitute_function(&mut clone, bindings, span) {
            return false;
        }
        if !self.specialize_synthetic_callees(&mut clone, bindings, span) {
            return false;
        }
        self.function_index.insert(id, self.lowerer.functions.len());
        self.lowerer.functions.push(clone);
        specialized.insert(helper, id);
        *callee = MirCallee::Function(id);
        true
    }

    fn substitute_function(&mut self, function: &mut MirFunction, bindings: &MirGenericBindings, span: Span) -> bool {
        for param in &mut function.params {
            if !self.substitute_ty_in_place(&mut param.ty, bindings, span) {
                return false;
            }
        }
        for local in &mut function.locals {
            if !self.substitute_ty_in_place(&mut local.ty, bindings, span) {
                return false;
            }
        }
        for temp in &mut function.temps {
            if !self.substitute_ty_in_place(&mut temp.ty, bindings, span) {
                return false;
            }
        }
        if !self.substitute_ty_in_place(&mut function.return_ty, bindings, span) {
            return false;
        }
        if let Some(error_ty) = &mut function.error_ty
            && !self.substitute_ty_in_place(error_ty, bindings, span)
        {
            return false;
        }

        for block in &mut function.blocks {
            for statement in &mut block.statements {
                if !self.substitute_statement(statement, bindings, span) {
                    return false;
                }
            }
            if let MirTerminatorKind::TryCall { callee, .. } = &mut block.terminator.kind
                && !self.substitute_callee(callee, bindings, span)
            {
                return false;
            }
        }
        true
    }

    fn substitute_statement(
        &mut self,
        statement: &mut MirStatement,
        bindings: &MirGenericBindings,
        span: Span,
    ) -> bool {
        match &mut statement.kind {
            MirStatementKind::Assign { value, .. } => self.substitute_value(value, bindings, span),
            MirStatementKind::Call { callee, destination, args }
                if self.is_builtin_ordering_call(callee, bindings, span) =>
            {
                // D1.4: a built-in or tuple witness of the ordering bound
                // compares through the compiler-supplied three-way compare.
                // Take the fields the arm already destructured instead of
                // matching the variant a second time — the shape is proven
                // by this pattern, not by a runtime check.
                let destination = destination.take();
                let args = std::mem::take(args);
                let return_ty = MirType::semantic(self.types().primitive(Primitive::Numerus));
                statement.kind = MirStatementKind::RuntimeCall {
                    destination,
                    call: MirRuntimeCall {
                        intrinsic: MirIntrinsic::Collection(MirCollectionOp::Compare),
                        args,
                        return_ty,
                    },
                };
                true
            }
            MirStatementKind::Call { callee, .. } => self.substitute_callee(callee, bindings, span),
            MirStatementKind::RuntimeCall { call, .. } => self.substitute_runtime_call(call, bindings, span),
            MirStatementKind::Construct { aggregate, .. } => self.substitute_aggregate(aggregate, bindings, span),
        }
    }

    fn substitute_callee(&mut self, callee: &mut MirCallee, bindings: &MirGenericBindings, span: Span) -> bool {
        let MirCallee::Definition { source, type_args } = callee else {
            return true;
        };
        for type_arg in type_args.iter_mut() {
            // GSF-1: a marked size argument (`size D`, `size n`) stays a
            // symbolic `Param` in callee type args — it is not a typus
            // binding and is resolved through the calling instance's shape
            // bindings at key derivation. Substituting it here would reject
            // the caller-owned magnitudo symbol as an unresolved type
            // parameter.
            if self.explicit_arg_is_size(*type_arg) {
                continue;
            }
            *type_arg = match self.substitute_ty(*type_arg, bindings, span) {
                Some(ty) => ty,
                None => return false,
            };
        }
        if let Some(target) = self.bound_call_target(*source, type_args, span) {
            *callee = MirCallee::definition(target);
        }
        true
    }

    /// Whether `callee` is an ordering-bound call (D1.4) whose substituted
    /// witness is a built-in or tuple rather than a genus.
    fn is_builtin_ordering_call(&mut self, callee: &MirCallee, bindings: &MirGenericBindings, span: Span) -> bool {
        let MirCallee::Definition { source, type_args } = callee else {
            return false;
        };
        if self.types().ordering_contract != Some(*source) {
            return false;
        }
        let Some(witness) = type_args.first() else {
            return false;
        };
        let Some(witness) = self.substitute_ty(*witness, bindings, span) else {
            return false;
        };
        let witness = witness.semantic_id();
        !type_contains_param(self.types(), witness)
            && struct_def_of(self.types(), witness).is_none()
            && radix_types::ordering::builtin_ordering_kind(self.types(), witness).is_some()
    }

    /// Resolve a contract-method callee (a D1.1 bound call) whose receiver
    /// witness is now concrete to the witness genus's own method. Typecheck
    /// admitted the witness only when its `implet` clause names the contract,
    /// so a missing method is a conformance hole reported here, not guessed.
    fn bound_call_target(&mut self, source: DefId, type_args: &[MirType], span: Span) -> Option<DefId> {
        let method = *self.contract_methods.get(&source)?;
        let witness = type_args.first()?.semantic_id();
        if type_contains_param(self.types(), witness) {
            return None;
        }
        let genus = struct_def_of(self.types(), witness);
        let target = genus.and_then(|genus| self.genus_methods.get(&(genus, method)).copied());
        if target.is_none() {
            let witness = self.format_ty(MirType::semantic(witness));
            self.lowerer.errors.push(MirError::validation(
                span,
                format!("bound call witness {witness} declares no method for the contract"),
            ));
        }
        target
    }

    fn substitute_value(&mut self, value: &mut MirValue, bindings: &MirGenericBindings, span: Span) -> bool {
        match self.substitute_ty(value.ty, bindings, span) {
            Some(ty) => {
                value.ty = ty;
                true
            }
            None => false,
        }
    }

    fn substitute_runtime_call(
        &mut self,
        call: &mut MirRuntimeCall,
        bindings: &MirGenericBindings,
        span: Span,
    ) -> bool {
        call.return_ty = match self.substitute_ty(call.return_ty, bindings, span) {
            Some(ty) => ty,
            None => return false,
        };
        if let MirIntrinsic::Convert(conversion) = &mut call.intrinsic {
            conversion.source_ty = match self.substitute_ty(conversion.source_ty, bindings, span) {
                Some(ty) => ty,
                None => return false,
            };
            conversion.target_ty = match self.substitute_ty(conversion.target_ty, bindings, span) {
                Some(ty) => ty,
                None => return false,
            };
        }
        true
    }

    fn substitute_aggregate(
        &mut self,
        aggregate: &mut MirAggregate,
        bindings: &MirGenericBindings,
        span: Span,
    ) -> bool {
        aggregate.ty = match self.substitute_ty(aggregate.ty, bindings, span) {
            Some(ty) => ty,
            None => return false,
        };
        true
    }

    fn substitute_ty(&mut self, ty: MirType, bindings: &MirGenericBindings, span: Span) -> Option<MirType> {
        if !type_contains_param(self.types(), ty.semantic_id()) {
            return Some(ty);
        }
        match instantiate_existing_generic_type(self.types(), ty, bindings, span) {
            Ok(substituted) => Some(substituted),
            // The concrete instantiation is well formed but not interned yet.
            // Leave the param-bearing type in place: the finalize pass interns
            // the substituted types and re-substitutes this site.
            Err(error) if error.is_missing_metadata() => Some(ty),
            Err(error) => {
                self.lowerer
                    .errors
                    .push(MirError::validation(error.span, error.message));
                None
            }
        }
    }

    /// In-place form of `substitute_ty` for declaration types; a failure is
    /// reported by `substitute_ty` and aborts the instance at the call site.
    fn substitute_ty_in_place(&mut self, ty: &mut MirType, bindings: &MirGenericBindings, span: Span) -> bool {
        match self.substitute_ty(*ty, bindings, span) {
            Some(substituted) => {
                *ty = substituted;
                true
            }
            None => false,
        }
    }

    fn apply_rewrites(&mut self, rewrites: Vec<Rewrite>) {
        for rewrite in rewrites {
            let Some(function) = self
                .function_index
                .function_mut(&mut self.lowerer.functions, rewrite.function)
            else {
                continue;
            };
            let Some(block) = function.blocks.get_mut(rewrite.block_index) else {
                continue;
            };
            match rewrite.location {
                CallLocation::Statement(statement_index) => {
                    if let Some(MirStatement { kind: MirStatementKind::Call { callee, .. }, .. }) =
                        block.statements.get_mut(statement_index)
                    {
                        *callee = MirCallee::Function(rewrite.target);
                    }
                }
                CallLocation::Terminator => {
                    if let MirTerminatorKind::TryCall { callee, .. } = &mut block.terminator.kind {
                        *callee = MirCallee::Function(rewrite.target);
                    }
                }
            }
        }
    }

    fn remove_templates(&mut self) {
        let template_ids = self
            .templates
            .values()
            .map(|function| function.id)
            .collect::<FxHashSet<_>>();
        // A call site whose caller is param-polymorphic keeps the template's
        // source `DefId` (see `seed_worklist`), and the runner resolves a
        // `Definition` callee through that index. Dropping a still-referenced
        // template would leave the call with no function to run, so those are
        // retained. A concrete instance of the same source is appended after
        // its template, so the `DefId` index still prefers the concrete one.
        let retained_sources = self.referenced_template_sources(&template_ids);
        self.lowerer.functions.retain(|function| {
            !template_ids.contains(&function.id)
                || function
                    .source
                    .is_some_and(|source| retained_sources.contains(&source))
        });
    }

    /// Source defs of templates that a kept function still names. Non-template
    /// functions seed the set and a retained template contributes its own
    /// callees, so a helper-to-helper chain survives as a chain.
    fn referenced_template_sources(&self, template_ids: &FxHashSet<MirFunctionId>) -> FxHashSet<DefId> {
        let mut retained_sources = FxHashSet::default();
        let mut pending: Vec<&MirFunction> = self
            .lowerer
            .functions
            .iter()
            .filter(|function| !template_ids.contains(&function.id))
            .collect();
        let mut scanned = 0;
        while let Some(function) = pending.get(scanned) {
            let sources = definition_callee_sources(function);
            scanned += 1;
            for source in sources {
                if !self.templates.contains_key(&source) || !retained_sources.insert(source) {
                    continue;
                }
                if let Some(function) = self
                    .lowerer
                    .functions
                    .iter()
                    .find(|candidate| candidate.source == Some(source))
                {
                    pending.push(function);
                }
            }
        }
        retained_sources
    }

    fn operand_ty(&mut self, operand: &MirOperand, scope: &FunctionTypeScope, span: Span) -> Option<MirType> {
        match operand {
            MirOperand::Place(place) => self.place_ty(place, scope, span),
            MirOperand::Temp(id) => scope.temps.get(id).copied().or_else(|| {
                self.lowerer
                    .errors
                    .push(MirError::validation(span, format!("undefined temp t{}", id.0)));
                None
            }),
            MirOperand::Value(id) => scope.values.get(id).copied().or_else(|| {
                self.lowerer
                    .errors
                    .push(MirError::validation(span, format!("undefined value v{}", id.0)));
                None
            }),
            MirOperand::Constant(constant) => self.constant_ty(constant, span),
        }
    }

    fn place_ty(&mut self, place: &MirPlace, scope: &FunctionTypeScope, span: Span) -> Option<MirType> {
        let mut ty = match place.base {
            MirPlaceBase::Local(id) => scope.locals.get(&id).copied().or_else(|| {
                self.lowerer
                    .errors
                    .push(MirError::validation(span, format!("undefined local l{}", id.0)));
                None
            }),
            MirPlaceBase::Temp(id) => scope.temps.get(&id).copied().or_else(|| {
                self.lowerer
                    .errors
                    .push(MirError::validation(span, format!("undefined temp t{}", id.0)));
                None
            }),
        }?;
        for projection in &place.projections {
            ty = self.projection_ty(ty, projection, span)?;
        }
        Some(ty)
    }

    fn projection_ty(&mut self, base_ty: MirType, projection: &MirProjection, span: Span) -> Option<MirType> {
        match projection {
            MirProjection::Field(field) => self.field_projection_ty(base_ty, *field, span),
            MirProjection::Index(_) | MirProjection::ProvenIndex(_) => {
                index_projection_result_ty(base_ty, self.types())
                    .map_err(|err| {
                        self.lowerer
                            .errors
                            .push(MirError::validation(span, err.to_string()));
                    })
                    .ok()
            }
            MirProjection::VariantField { .. } => {
                self.lowerer.errors.push(MirError::validation(
                    span,
                    "generic monomorphization through variant-field projections is unsupported",
                ));
                None
            }
            MirProjection::ClosureCapture { .. } => {
                self.lowerer.errors.push(MirError::validation(
                    span,
                    "generic monomorphization through closure-capture projections is unsupported",
                ));
                None
            }
            MirProjection::SharedCell => Some(base_ty),
            MirProjection::VectorLane(_) => {
                self.lowerer.errors.push(MirError::validation(
                    span,
                    "generic monomorphization through vector-lane projections is unsupported",
                ));
                None
            }
            MirProjection::MatrixCell { .. } => {
                self.lowerer.errors.push(MirError::validation(
                    span,
                    "generic monomorphization through matrix-cell projections is unsupported",
                ));
                None
            }
        }
    }

    fn field_projection_ty(&mut self, base_ty: MirType, field: Symbol, span: Span) -> Option<MirType> {
        match self.types().get(base_ty.semantic_id()) {
            Type::Struct(def_id) => self
                .struct_fields
                .get(def_id)
                .and_then(|fields| fields.get(&field))
                .copied()
                .or_else(|| {
                    self.lowerer
                        .errors
                        .push(MirError::validation(span, format!("unknown struct field symbol #{}", field.0)));
                    None
                }),
            Type::Alias(_, target) => self.field_projection_ty(MirType::semantic(*target), field, span),
            _ => {
                self.lowerer.errors.push(MirError::validation(
                    span,
                    "generic monomorphization field projection base is not a struct",
                ));
                None
            }
        }
    }

    fn constant_ty(&mut self, constant: &MirConstant, span: Span) -> Option<MirType> {
        let primitive = match constant {
            MirConstant::Int(_) => Primitive::Numerus,
            MirConstant::UInt(_) => {
                return Some(MirType::semantic(
                    self.lowerer
                        .unit
                        .types
                        .find_sized_numeric(Primitive::Numerus, radix_semantic::NumericWidth::U64)
                        .unwrap_or_else(|| self.lowerer.types.primitive(Primitive::Numerus)),
                ));
            }
            // An exact bignum constant is the unbounded integer.
            MirConstant::Magnus(_) => {
                return Some(MirType::semantic(
                    self.lowerer
                        .unit
                        .types
                        .find_sized_numeric(Primitive::Numerus, radix_semantic::NumericWidth::Inf)
                        .unwrap_or_else(|| self.lowerer.types.primitive(Primitive::Numerus)),
                ));
            }
            MirConstant::Bool(_) => Primitive::Bivalens,
            MirConstant::Unit => Primitive::Vacuum,
            MirConstant::String(_) => Primitive::Textus,
            MirConstant::Ascii(_) => Primitive::Ascii,
            MirConstant::Nil => Primitive::Nihil,
            MirConstant::Octeti(_) => Primitive::Octeti,
            MirConstant::Regex { .. } => Primitive::Regex,
            MirConstant::Function(id) => {
                return self
                    .function_index
                    .function(&self.lowerer.functions, *id)
                    .map(|function| function.return_ty)
                    .or_else(|| {
                        self.lowerer
                            .errors
                            .push(MirError::validation(span, format!("unknown function f{}", id.0)));
                        None
                    });
            }
            // A shape-size scalar is a float-sized compile-time value.
            MirConstant::Float(_) | MirConstant::ShapeSize { .. } => Primitive::Fractus,
        };
        Some(MirType::semantic(self.lowerer.types.primitive(primitive)))
    }

    fn is_template_function(&self, function: &MirFunction) -> bool {
        self.templates
            .values()
            .any(|template| template.id == function.id)
    }

    fn types(&self) -> &TypeTable {
        self.lowerer.types
    }

    fn format_ty(&self, ty: MirType) -> String {
        radix_semantic::format_faber_type(ty.semantic_id(), self.types(), self.lowerer.interner, &|def_id| {
            self.lowerer.unit.resolver.get_symbol(def_id).map_or_else(
                || format!("def{}", def_id.0),
                |symbol| self.lowerer.interner.resolve(symbol.name).to_owned(),
            )
        })
    }
}

impl FunctionTypeScope {
    fn from_function(function: &MirFunction) -> Self {
        Self {
            locals: function
                .locals
                .iter()
                .map(|local| (local.id, local.ty))
                .collect(),
            temps: function
                .temps
                .iter()
                .map(|temp| (temp.id, temp.ty))
                .collect(),
            values: FxHashMap::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParamKind {
    Typus,
    Magnitudo,
}

/// One generic parameter in source declaration order, with its kind.
#[derive(Debug, Clone, Copy)]
struct OrderedParam {
    name: Symbol,
    kind: ParamKind,
    runtime_witness: Option<IndexId>,
}

/// Zero-based position of `name` among the `typus` parameters. A key's
/// `type_args` index only typus parameters, so `magnitudo` entries do not
/// advance the ordinal.
fn typus_param_ordinal(params: &[OrderedParam], name: Symbol) -> Option<usize> {
    let mut ordinal = 0_usize;
    for param in params {
        if param.kind != ParamKind::Typus {
            continue;
        }
        if param.name == name {
            return Some(ordinal);
        }
        ordinal += 1;
    }
    None
}

/// Bind each `magnitudo` parameter to the key's shape argument in the same
/// declaration order. A key with fewer arguments leaves the trailing parameters unbound;
/// the finalize survivor net fails closed on those.
fn shape_bindings_for_key(params: &[OrderedParam], shape_args: &[IndexId]) -> MirShapeBindings {
    params
        .iter()
        .filter(|param| param.kind == ParamKind::Magnitudo)
        .zip(shape_args.iter().copied())
        .map(|(param, shape)| (param.name, shape))
        .collect()
}

/// Bind each `typus` parameter to the key's type argument in the same
/// declaration order, mirroring [`ensure_instance`]. A key with fewer
/// arguments leaves the trailing parameters unbound; the finalize pass
/// survivor net fails closed on those.
fn type_bindings_for_key(params: &[OrderedParam], type_args: &[MirType]) -> MirGenericBindings {
    params
        .iter()
        .filter(|param| param.kind == ParamKind::Typus)
        .zip(type_args.iter().copied())
        .map(|(param, ty)| (param.name, ty))
        .collect()
}

/// Contract-method names and genus method targets for bound-call dispatch.
fn contract_dispatch_tables(
    hir: &radix_hir::HirModule,
) -> (FxHashMap<DefId, Symbol>, FxHashMap<(DefId, Symbol), DefId>) {
    let mut contract_methods = FxHashMap::default();
    let mut genus_methods = FxHashMap::default();
    for item in &hir.items {
        match &item.kind {
            radix_hir::HirItemKind::Interface(interface) => {
                for method in &interface.methods {
                    contract_methods.insert(method.def_id, method.name);
                }
            }
            radix_hir::HirItemKind::Struct(strukt) => {
                for method in &strukt.methods {
                    genus_methods.insert((item.def_id, method.func.name), method.def_id);
                }
            }
            _ => {}
        }
    }
    (contract_methods, genus_methods)
}

/// The genus a concrete witness type names, through access markers.
fn struct_def_of(types: &TypeTable, ty: TypeId) -> Option<DefId> {
    match types.get(ty) {
        Type::Struct(def) => Some(*def),
        Type::Ref(_, inner) => struct_def_of(types, *inner),
        Type::Applied(base, _) => struct_def_of(types, *base),
        _ => None,
    }
}

fn generic_param_order(hir: &radix_hir::HirModule) -> FxHashMap<DefId, Vec<OrderedParam>> {
    let mut order = FxHashMap::default();
    for item in &hir.items {
        match &item.kind {
            HirItemKind::Function(function) => {
                let params = generic_param_names(&function.type_params);
                if !params.is_empty() {
                    order.insert(item.def_id, params);
                }
            }
            HirItemKind::Struct(strukt) => {
                let class_params = generic_param_names(&strukt.type_params);
                for method in &strukt.methods {
                    let method_params = generic_param_names(&method.func.type_params);
                    // A method generic shadows the class parameter with the same name.
                    let mut params = class_params
                        .iter()
                        .filter(|class_param| {
                            !method_params
                                .iter()
                                .any(|method_param| method_param.name == class_param.name)
                        })
                        .copied()
                        .collect::<Vec<_>>();
                    params.extend(method_params);
                    if !params.is_empty() {
                        order.insert(method.def_id, params);
                    }
                }
            }
            HirItemKind::Enum(_)
            | HirItemKind::Interface(_)
            | HirItemKind::TypeAlias(_)
            | HirItemKind::Schema(_)
            | HirItemKind::Constant(_)
            | HirItemKind::Import(_) => {}
        }
    }
    order
}

/// Resolve one nested call's inferred shape through its concrete parent
/// instance. Runtime origins remain symbolic here; the caller canonicalizes
/// them against the callee's stable formal witness, never the runtime value.
/// Tuple and arithmetic terms are accepted only when their children resolve;
/// a shape that remains a parameter, inference hole, or unspecified is
/// genuinely unbound and fails closed under D6.
pub(super) fn resolve_parent_shape_binding(
    types: &TypeTable,
    shape: IndexId,
    parent_shape_bindings: &MirShapeBindings,
) -> Option<IndexId> {
    match types.get_index(shape) {
        IndexExpr::Literal(_) | IndexExpr::Runtime(_) => Some(shape),
        IndexExpr::Param(name) => parent_shape_bindings.get(name).copied().and_then(|bound| {
            (bound != shape)
                .then(|| resolve_parent_shape_binding(types, bound, parent_shape_bindings))
                .flatten()
        }),
        IndexExpr::Affine { base, offset } => {
            let resolved_base = resolve_parent_shape_binding(types, *base, parent_shape_bindings)?;
            if resolved_base == *base {
                return Some(shape);
            }
            if let Some(value) = radix_types::evaluate_index_dimension(resolved_base, types).ok()? {
                let value = i128::from(value).checked_add(*offset)?;
                if value < 0 {
                    return None;
                }
                return find_literal_index(types, u64::try_from(value).ok()?);
            }
            (0..types.index_count()).find_map(|index| {
                let candidate = u32::try_from(index).ok().map(IndexId)?;
                matches!(
                    types.get_index(candidate),
                    IndexExpr::Affine { base, offset: candidate_offset }
                        if *base == resolved_base && *candidate_offset == *offset
                )
                .then_some(candidate)
            })
        }
        IndexExpr::Divide { numerator, divisor } => {
            if *divisor == 0 {
                return None;
            }
            let resolved_numerator = resolve_parent_shape_binding(types, *numerator, parent_shape_bindings)?;
            if resolved_numerator == *numerator {
                return Some(shape);
            }
            if let Some(value) = radix_types::evaluate_index_dimension(resolved_numerator, types).ok()? {
                if value % *divisor != 0 {
                    return None;
                }
                return find_literal_index(types, value / *divisor);
            }
            (0..types.index_count()).find_map(|index| {
                let candidate = u32::try_from(index).ok().map(IndexId)?;
                matches!(
                    types.get_index(candidate),
                    IndexExpr::Divide { numerator, divisor: candidate_divisor }
                        if *numerator == resolved_numerator && *candidate_divisor == *divisor
                )
                .then_some(candidate)
            })
        }
        IndexExpr::Tuple(dims) => {
            let resolved = dims
                .iter()
                .map(|dim| resolve_parent_shape_binding(types, *dim, parent_shape_bindings))
                .collect::<Option<Vec<_>>>()?;
            (resolved == *dims).then_some(shape).or_else(|| {
                (0..types.index_count()).find_map(|index| {
                    let candidate = u32::try_from(index).ok().map(IndexId)?;
                    matches!(types.get_index(candidate), IndexExpr::Tuple(candidate_dims) if candidate_dims == &resolved)
                        .then_some(candidate)
                })
            })
        }
        IndexExpr::Infer(_) | IndexExpr::Unspecified => None,
    }
}

/// Interned `IndexExpr::Literal` node for a concrete extent, if present.
fn find_literal_index(types: &TypeTable, value: u64) -> Option<IndexId> {
    (0..types.index_count()).find_map(|index| {
        #[expect(clippy::cast_possible_truncation, reason = "IndexId is a u32 arena index by design")]
        let id = IndexId(index as u32);
        matches!(types.get_index(id), IndexExpr::Literal(found) if *found == value).then_some(id)
    })
}

/// Interned `IndexExpr::Tuple` node whose dimensions are exactly these
/// literal extents (an explicit `size [2, 3]` witness), if present. The
/// typechecker interns the witness when it binds the call, so a well-formed
/// call always finds it here.
pub(super) fn find_literal_shape_index(types: &TypeTable, dims: &[u64]) -> Option<IndexId> {
    (0..types.index_count()).find_map(|index| {
        #[expect(clippy::cast_possible_truncation, reason = "IndexId is a u32 arena index by design")]
        let id = IndexId(index as u32);
        let IndexExpr::Tuple(found) = types.get_index(id) else {
            return None;
        };
        (found.len() == dims.len()
            && found
                .iter()
                .zip(dims)
                .all(|(dim, value)| matches!(types.get_index(*dim), IndexExpr::Literal(v) if v == value)))
        .then_some(id)
    })
}

/// Interned `IndexExpr::Param` node for a caller-owned symbolic size param.
/// The forwarding call site's witness types reference this node, so the
/// insert-only shape binder sees one consistent identity (GSF-1).
fn find_param_index(types: &TypeTable, sym: radix_lexer::Symbol) -> Option<IndexId> {
    (0..types.index_count()).find_map(|index| {
        #[expect(clippy::cast_possible_truncation, reason = "IndexId is a u32 arena index by design")]
        let id = IndexId(index as u32);
        matches!(types.get_index(id), IndexExpr::Param(found) if *found == sym).then_some(id)
    })
}

fn generic_param_names(params: &[radix_hir::HirTypeParam]) -> Vec<OrderedParam> {
    params
        .iter()
        .map(|param| OrderedParam {
            name: param.name,
            kind: if param.is_typus() {
                ParamKind::Typus
            } else {
                ParamKind::Magnitudo
            },
            runtime_witness: None,
        })
        .collect()
}

/// Concretize shape params in monomorphized instances (4A, final pass).
///
/// Shape substitution interns the concrete shapes/types each instance carries
/// into `types` — the lowering-owned extension arena cloned from the analyzed
/// unit's table at the lowering entry point, so the analyzed unit's arena is
/// never written. Each instance's shape params are bound from its key's
/// `shape_args` (D1) and substituted to literal dims; any function still
/// carrying a param afterwards is rejected (D6 net). Its typus params are bound
/// from the key's `type_args` the same way, so a body type the substitution
/// pass deferred because its concrete instantiation was not yet interned is
/// completed here.
///
/// Phase-4 producer fact: every `MirConstant::ShapeSize` operand/constant in
/// an instance is also rewritten to the concrete `Float` it denotes from that
/// instance's shape bindings, and a final survivor net rejects any shape-size
/// scalar that substitution could not concretize (missing/unresolved/
/// infer/unspecified/out-of-range binding) before any backend runs.
pub fn finalize_monomorphized_shapes(
    hir: &radix_hir::HirModule,
    types: &mut TypeTable,
    functions: &mut [MirFunction],
    monomorphizations: &[MirMonomorphization],
) -> Vec<MirError> {
    let mut errors = Vec::new();
    let param_order = generic_param_order(hir);
    let index = ExistingTypeIndex::build(types);
    let function_index = MirFunctionIndex::for_functions(functions);
    for monomorphization in monomorphizations {
        let Some(params) = param_order.get(&monomorphization.key.source) else {
            continue;
        };
        let Some(function) = function_index.function_mut(functions, monomorphization.function) else {
            continue;
        };
        // The instance's typus bindings, exactly as `ensure_instance` built
        // them, so a body site the substitution pass left deferred (because its
        // concrete type was not interned yet) re-substitutes against the same
        // bindings and `finalize_type_shapes` interns the result.
        let type_bindings = type_bindings_for_key(params, &monomorphization.key.type_args);
        let shape_bindings = shape_bindings_for_key(params, &monomorphization.key.shape_args);
        if !finalize_function_shapes(types, function, &type_bindings, &shape_bindings, &index, &mut errors) {
            continue;
        }
        if function_contains_param(types, function) {
            errors.push(MirError::validation(
                function.span,
                format!("function f{} contains unresolved generic type parameter", function.id.0),
            ));
        }
    }

    // Post-monomorphization survivor net (P4): templates are already removed
    // above, so every function here is executable. A surviving shape-size
    // scalar means its instance's binding was missing, unresolved,
    // infer/unspecified, or out-of-range — all fail closed through this one
    // stable MIR validation diagnostic, before any backend can guess at it.
    for function in functions.iter_mut() {
        if function_contains_shape_size(function) {
            errors.push(MirError::validation(function.span, UNRESOLVED_SHAPE_SIZE_SURVIVOR));
        }
    }
    // Every function is now executable with literal widths: expand the
    // deferred vector ops into lanes (instances and non-generic functions).
    for function in functions.iter_mut() {
        super::expand_deferred_vector_ops(types, function, &mut errors);
    }
    errors
}

/// Stable fail-closed diagnostic for a shape-size scalar that survives
/// monomorphization (missing/unresolved/infer/unspecified/out-of-range).
const UNRESOLVED_SHAPE_SIZE_SURVIVOR: &str = "unresolved shape-size constant survives monomorphization";

fn finalize_function_shapes(
    types: &mut TypeTable,
    function: &mut MirFunction,
    type_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    index: &ExistingTypeIndex,
    errors: &mut Vec<MirError>,
) -> bool {
    let span = function.span;
    for param in &mut function.params {
        param.ty = match finalize_type_shapes(types, param.ty, type_bindings, shape_bindings, span, index, errors) {
            Some(ty) => ty,
            None => return false,
        };
    }
    for local in &mut function.locals {
        local.ty = match finalize_type_shapes(types, local.ty, type_bindings, shape_bindings, span, index, errors) {
            Some(ty) => ty,
            None => return false,
        };
    }
    for temp in &mut function.temps {
        temp.ty = match finalize_type_shapes(types, temp.ty, type_bindings, shape_bindings, span, index, errors) {
            Some(ty) => ty,
            None => return false,
        };
    }
    function.return_ty =
        match finalize_type_shapes(types, function.return_ty, type_bindings, shape_bindings, span, index, errors) {
            Some(ty) => ty,
            None => return false,
        };
    function.error_ty = match function.error_ty {
        Some(error_ty) => {
            match finalize_type_shapes(types, error_ty, type_bindings, shape_bindings, span, index, errors) {
                Some(error_ty) => Some(error_ty),
                None => return false,
            }
        }
        None => None,
    };
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            if !finalize_statement_shapes(types, statement, type_bindings, shape_bindings, span, index, errors) {
                return false;
            }
        }
    }
    // Phase-4 producer fact: rewrite every shape-size operand/constant in the
    // body from this instance's shape bindings to the concrete `Float` it
    // denotes. Package-imported TensorFromFlat producers also repair their
    // source-local shape index against the finalized result type before the
    // general walk. Terms that cannot fold stay untouched and are rejected by
    // the post-finalize survivor net (one stable diagnostic).
    let _ = substitute_tensor_from_flat_shape_size_constants(types, function, shape_bindings);
    let _ = substitute_shape_size_constants(types, function, shape_bindings);
    fold_static_shape_checks(types, function);
    true
}

fn finalize_statement_shapes(
    types: &mut TypeTable,
    statement: &mut MirStatement,
    type_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    span: Span,
    index: &ExistingTypeIndex,
    errors: &mut Vec<MirError>,
) -> bool {
    match &mut statement.kind {
        MirStatementKind::Assign { value, .. } => {
            value.ty = match finalize_type_shapes(types, value.ty, type_bindings, shape_bindings, span, index, errors) {
                Some(ty) => ty,
                None => return false,
            };
        }
        MirStatementKind::RuntimeCall { call, .. } => {
            call.return_ty =
                match finalize_type_shapes(types, call.return_ty, type_bindings, shape_bindings, span, index, errors) {
                    Some(ty) => ty,
                    None => return false,
                };
            if let MirIntrinsic::Convert(conversion) = &mut call.intrinsic {
                conversion.source_ty = match finalize_type_shapes(
                    types,
                    conversion.source_ty,
                    type_bindings,
                    shape_bindings,
                    span,
                    index,
                    errors,
                ) {
                    Some(ty) => ty,
                    None => return false,
                };
                conversion.target_ty = match finalize_type_shapes(
                    types,
                    conversion.target_ty,
                    type_bindings,
                    shape_bindings,
                    span,
                    index,
                    errors,
                ) {
                    Some(ty) => ty,
                    None => return false,
                };
            }
        }
        MirStatementKind::Construct { aggregate, .. } => {
            aggregate.ty =
                match finalize_type_shapes(types, aggregate.ty, type_bindings, shape_bindings, span, index, errors) {
                    Some(ty) => ty,
                    None => return false,
                };
        }
        MirStatementKind::Call { .. } => {}
    }
    true
}

fn finalize_type_shapes(
    types: &mut TypeTable,
    ty: MirType,
    type_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    span: Span,
    index: &ExistingTypeIndex,
    errors: &mut Vec<MirError>,
) -> Option<MirType> {
    match instantiate_generic_type_with_shapes_with_index(types, ty, type_bindings, shape_bindings, span, index) {
        Ok(ty) => Some(ty),
        Err(error) => {
            errors.push(MirError::validation(error.span, error.message));
            None
        }
    }
}

/// Drop the shape relations that a finalized instance proves statically.
///
/// A template's entry and return checks relate formal carrier axes because
/// its sizes are symbolic. Once an instance binds those sizes to literals, an
/// axis read from an operand whose type has a literal extent is a compile-time
/// fact, already proven at the typed call site that created the instance (Q3:
/// fixed sizes are checked at the typed call site, never at function entry).
/// Equal static relations are removed; a check left with no relation and no
/// size binding is removed. Relations that read a runtime extent, and static
/// relations that disagree, stay for the runtime check.
pub fn fold_static_shape_checks(types: &TypeTable, function: &mut MirFunction) {
    let scope = FunctionTypeScope::from_function(function);
    for block in &mut function.blocks {
        block.statements.retain_mut(|statement| {
            let MirStatementKind::RuntimeCall { destination: None, call } = &mut statement.kind else {
                return true;
            };
            let MirIntrinsic::TensorShapeCheck(check) = &mut call.intrinsic else {
                return true;
            };
            let args = &call.args;
            check.relations.retain(|relation| {
                let left = static_shape_value(types, &scope, args, &relation.left);
                let right = static_shape_value(types, &scope, args, &relation.right);
                !matches!((left, right), (Some(left), Some(right)) if left == right)
            });
            !check.relations.is_empty() || !check.size_bindings.is_empty()
        });
    }
}

/// The value of a shape expression when the check's operand types fix it.
fn static_shape_value(
    types: &TypeTable,
    scope: &FunctionTypeScope,
    args: &[MirOperand],
    expr: &MirShapeExpr,
) -> Option<u64> {
    match expr {
        MirShapeExpr::Literal(value) => Some(*value),
        MirShapeExpr::Product(factors) => factors.iter().try_fold(1u64, |product, factor| {
            product.checked_mul(static_shape_value(types, scope, args, factor)?)
        }),
        MirShapeExpr::ScalarArgument { argument } => match args.get(usize::try_from(*argument).ok()?)? {
            MirOperand::Constant(MirConstant::Int(value)) => u64::try_from(*value).ok(),
            MirOperand::Constant(MirConstant::UInt(value)) => Some(*value),
            // A substituted shape-size constant is an integral extent carried
            // as a float scalar.
            MirOperand::Constant(MirConstant::Float(value))
                if value.is_finite() && *value >= 0.0 && value.fract() == 0.0 && *value <= u64::MAX as f64 =>
            {
                Some(*value as u64)
            }
            _ => None,
        },
        MirShapeExpr::TensorAxis { argument, axis } => {
            let MirOperand::Place(place) = args.get(usize::try_from(*argument).ok()?)? else {
                return None;
            };
            if !place.projections.is_empty() {
                return None;
            }
            let ty = match place.base {
                MirPlaceBase::Local(local) => scope.locals.get(&local)?,
                MirPlaceBase::Temp(temp) => scope.temps.get(&temp)?,
            };
            static_axis_extent(types, ty.semantic_id(), *axis)
        }
        MirShapeExpr::TensorFieldAxis { .. } => None,
        MirShapeExpr::Affine { base, offset } => {
            let base = i128::from(static_shape_value(types, scope, args, base)?);
            u64::try_from(base.checked_add(*offset)?).ok()
        }
        MirShapeExpr::Divide { numerator, divisor } => {
            let numerator = static_shape_value(types, scope, args, numerator)?;
            (*divisor != 0 && numerator % divisor == 0).then(|| numerator / divisor)
        }
    }
}

/// The literal extent of one axis of a tensor-like type, when it has one.
fn static_axis_extent(types: &TypeTable, mut ty: TypeId, axis: u32) -> Option<u64> {
    while let Type::Ref(_, inner) | Type::Alias(_, inner) = types.get(ty) {
        ty = *inner;
    }
    let (Type::Tensor(_, shape) | Type::Matrix(_, shape) | Type::Vector(_, shape) | Type::Sparsa(_, shape)) =
        types.get(ty)
    else {
        return None;
    };
    let dim = match types.get_index(*shape) {
        IndexExpr::Tuple(dims) => *dims.get(usize::try_from(axis).ok()?)?,
        _ if axis == 0 => *shape,
        _ => return None,
    };
    match types.get_index(dim) {
        IndexExpr::Literal(value) => Some(*value),
        _ => None,
    }
}

/// Source defs of the `MirCallee::Definition` call sites in one function. A
/// rewritten generic call names a `MirCallee::Function` id instead, so every
/// entry is a call still resolved by `DefId` when the program runs.
fn definition_callee_sources(function: &MirFunction) -> Vec<DefId> {
    let mut sources = Vec::new();
    for block in &function.blocks {
        for statement in &block.statements {
            if let MirStatementKind::Call { callee: MirCallee::Definition { source, .. }, .. } = &statement.kind {
                sources.push(*source);
            }
        }
        if let MirTerminatorKind::TryCall { callee: MirCallee::Definition { source, .. }, .. } = &block.terminator.kind
        {
            sources.push(*source);
        }
    }
    sources
}

/// Whether any param-bearing type survives in a function (D6 net).
pub(super) fn function_contains_param(types: &TypeTable, function: &MirFunction) -> bool {
    function
        .params
        .iter()
        .any(|param| type_contains_param(types, param.ty.semantic_id()))
        || function
            .locals
            .iter()
            .any(|local| type_contains_param(types, local.ty.semantic_id()))
        || function
            .temps
            .iter()
            .any(|temp| type_contains_param(types, temp.ty.semantic_id()))
        || type_contains_param(types, function.return_ty.semantic_id())
        || function
            .error_ty
            .is_some_and(|error_ty| type_contains_param(types, error_ty.semantic_id()))
}

/// Whether `statement` is the private `limes` placeholder call whose result
/// lands in `destination`. The placeholder's staging tag is
/// `MirBuiltinEdgePolicy::Clamp`; specialization replaces it before
/// validation.
fn is_deferred_limes_placeholder(statement: &MirStatement, destination: MirTempId) -> bool {
    matches!(
        &statement.kind,
        MirStatementKind::RuntimeCall { destination: Some(place), call }
            if place.base == MirPlaceBase::Temp(destination)
                && place.projections.is_empty()
                && matches!(
                    &call.intrinsic,
                    MirIntrinsic::TensorLimes(MirEdgePolicy::Builtin(MirBuiltinEdgePolicy::Clamp))
                )
    )
}

/// Return type recorded on a pending `limes` placeholder call.
fn deferred_limes_placeholder_return_ty(function: &MirFunction, destination: MirTempId) -> Option<MirType> {
    function
        .blocks
        .iter()
        .flat_map(|block| &block.statements)
        .find_map(|statement| {
            if !is_deferred_limes_placeholder(statement, destination) {
                return None;
            }
            let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                return None;
            };
            Some(call.return_ty)
        })
}

fn tensor_edge_policy_param_symbol(types: &TypeTable, mut ty: TypeId) -> Option<Symbol> {
    let mut seen = FxHashSet::default();
    while let Type::Alias(_, inner) = types.get(ty) {
        if !seen.insert(ty) {
            return None;
        }
        ty = *inner;
    }
    match types.get(ty) {
        Type::Param(symbol) => Some(*symbol),
        _ => None,
    }
}

fn tensor_element_and_rank(types: &TypeTable, mut ty: TypeId) -> Option<(TypeId, usize)> {
    let mut seen = FxHashSet::default();
    while let Type::Alias(_, inner) = types.get(ty) {
        if !seen.insert(ty) {
            return None;
        }
        ty = *inner;
    }
    let Type::Tensor(element, shape) = types.get(ty) else {
        return None;
    };
    let rank = match types.get_index(*shape) {
        IndexExpr::Tuple(dimensions) => dimensions.len(),
        IndexExpr::Literal(_)
        | IndexExpr::Param(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Affine { .. }
        | IndexExpr::Divide { .. } => 1,
        IndexExpr::Infer(_) | IndexExpr::Unspecified => return None,
    };
    Some((*element, rank))
}

fn same_type_identity(types: &TypeTable, left: TypeId, right: TypeId) -> bool {
    fn resolved(types: &TypeTable, mut ty: TypeId) -> Option<TypeId> {
        let mut seen = FxHashSet::default();
        while let Type::Alias(_, inner) = types.get(ty) {
            if !seen.insert(ty) {
                return None;
            }
            ty = *inner;
        }
        Some(ty)
    }
    resolved(types, left) == resolved(types, right)
}
