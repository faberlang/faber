//! Generic callable monomorphization before MIR validation.
//!
//! HIR lowering initially preserves source definition callees because ordinary
//! validation can type-check calls against semantic signatures. Systems targets
//! need a stronger fact: executable calls to generic definitions must become
//! concrete MIR functions before target classification or emission.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;
use crate::mir::generic::{
    ExistingTypeIndex, MirShapeBindings, bind_generic_type_witness_with_shapes, canonical_index,
    function_contains_shape_size, instantiate_generic_type_with_shapes_with_index, substitute_shape_size_constants,
    substitute_tensor_from_flat_shape_size_constants,
};
use crate::mir::ty::index_projection_result_ty;
use crate::mir::{
    MirGenericBindings, MirMonomorphization, MirMonomorphizationKey, MirPlaceBase, instantiate_existing_generic_type,
    type_contains_param,
};
use crate::semantic::IndexId;

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
}

#[derive(Clone)]
struct FunctionTypeScope {
    locals: FxHashMap<MirLocalId, MirType>,
    temps: FxHashMap<MirTempId, MirType>,
    values: FxHashMap<MirValueId, MirType>,
}

struct CallWitness<'a> {
    args: &'a [MirOperand],
    destination: Option<&'a MirPlace>,
    error_place: Option<&'a MirPlace>,
    span: Span,
    scope: &'a FunctionTypeScope,
}

pub(super) struct MonomorphizeOutput {
    pub monomorphizations: Vec<MirMonomorphization>,
    pub templates: Vec<MirFunction>,
    pub generic_params: Vec<(DefId, Vec<super::MirGenericParam>)>,
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
                        })
                        .collect(),
                )
            })
            .collect();
        MonomorphizeOutput {
            monomorphizations: state.monomorphizations,
            templates: state.templates.into_values().collect(),
            generic_params,
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
    next_id: MirFunctionId,
}

impl<'lowerer, 'unit> MonomorphizationState<'lowerer, 'unit> {
    fn new(lowerer: &'lowerer mut MirLowerer<'unit>) -> Self {
        let generic_param_order = generic_param_order(&lowerer.unit.hir);
        let struct_fields = lowerer
            .struct_field_types
            .take()
            .expect("struct field types retained during lowering");
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
        let next_id = lowerer
            .functions
            .iter()
            .map(|function| function.id.0)
            .max()
            .map_or(MirFunctionId(0), |id| MirFunctionId(id + 1));
        Self {
            lowerer,
            templates,
            generic_param_order,
            struct_fields,
            monomorphizations: Vec::new(),
            instance_index: FxHashMap::default(),
            next_id,
        }
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
                let Some(function) = function_by_id(&self.lowerer.functions, function_id) else {
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
    fn seed_worklist(&self) -> Vec<MirFunctionId> {
        self.lowerer
            .functions
            .iter()
            .filter(|function| !self.is_template_function(function) && self.function_has_generic_callee(function))
            .map(|function| function.id)
            .collect()
    }

    /// True when any statement or terminator call targets a generic definition
    /// (the same guard `rewrite_for_call` applies). Cheap seed predicate; the
    /// scan phase re-walks matching functions for exact call-site metadata.
    fn function_has_generic_callee(&self, function: &MirFunction) -> bool {
        function.blocks.iter().any(|block| {
            block.statements.iter().any(|statement| {
                matches!(
                    &statement.kind,
                    MirStatementKind::Call { callee, .. } if self.is_generic_callee(callee)
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
                            sites.push(CallSite {
                                function: function.id,
                                block_index,
                                location: CallLocation::Statement(statement_index),
                                destination: destination.clone(),
                                callee: callee.clone(),
                                args: args.clone(),
                                error_place: None,
                                span: statement.span,
                                scope: scope.clone(),
                            });
                        }
                    }
                    MirStatementKind::RuntimeCall { .. } | MirStatementKind::Construct { .. } => {}
                }
            }

            if let MirTerminatorKind::TryCall { destination, callee, args, error_place, .. } = &block.terminator.kind
                && self.is_generic_callee(callee)
            {
                sites.push(CallSite {
                    function: function.id,
                    block_index,
                    location: CallLocation::Terminator,
                    destination: destination.clone(),
                    callee: callee.clone(),
                    args: args.clone(),
                    error_place: Some(error_place.clone()),
                    span: block.terminator.span,
                    scope: scope.clone(),
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

    fn rewrite_for_call(&mut self, site: CallSite) -> Option<Rewrite> {
        let CallSite { function, block_index, location, destination, callee, args, error_place, span, scope } = site;
        let MirCallee::Definition { source, type_args: explicit_type_args } = &callee else {
            return None;
        };
        if !self.generic_param_order.contains_key(source) {
            return None;
        }

        let template = self.templates.get(source).cloned().or_else(|| {
            let error = if self.lowerer.unit.radix_lanes.lane_for(*source) == Some(crate::semantic::RadixLane::Air) {
                MirError::missing_generic_air_template(span, source.0)
            } else {
                MirError::validation(span, format!("generic definition #{} has no MIR template", source.0))
            };
            self.lowerer.errors.push(error);
            None
        })?;

        let witness = CallWitness {
            args: &args,
            destination: destination.as_ref(),
            error_place: error_place.as_ref(),
            span,
            scope: &scope,
        };
        let parent_shape_bindings = self.instance_shape_bindings(function);
        let key = self.key_for_call(&template, explicit_type_args, witness, &parent_shape_bindings)?;
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
        params
            .iter()
            .filter(|param| param.kind == ParamKind::Magnitudo)
            .zip(monomorphization.key.shape_args.iter().copied())
            .map(|(param, shape)| (param.name, shape))
            .collect()
    }

    fn key_for_call(
        &mut self,
        template: &MirFunction,
        explicit_type_args: &[MirType],
        witness: CallWitness<'_>,
        parent_shape_bindings: &MirShapeBindings,
    ) -> Option<MirMonomorphizationKey> {
        let CallWitness { args, destination, error_place, span, scope } = witness;
        let mut bindings = MirGenericBindings::default();
        let mut shape_bindings = MirShapeBindings::default();
        let source = template.source?;
        self.bind_explicit_type_args(source, explicit_type_args, &mut bindings, &mut shape_bindings, span)?;
        for (index, arg) in args.iter().enumerate() {
            let Some(expected) = template.params.get(index) else {
                continue;
            };
            let actual = self.operand_ty(arg, scope, span)?;
            if !bind_generic_type_witness_with_shapes(
                self.types(),
                expected.ty,
                actual,
                &mut bindings,
                &mut shape_bindings,
            ) {
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

        if let Some(destination) = destination {
            let actual = self.place_ty(destination, scope, span)?;
            if !bind_generic_type_witness_with_shapes(
                self.types(),
                template.return_ty,
                actual,
                &mut bindings,
                &mut shape_bindings,
            ) {
                self.lowerer
                    .errors
                    .push(MirError::validation(span, "destination type mismatch"));
                return None;
            }
        }

        if let (Some(expected), Some(error_place)) = (template.error_ty, error_place) {
            let actual = self.place_ty(error_place, scope, span)?;
            if !bind_generic_type_witness_with_shapes(
                self.types(),
                expected,
                actual,
                &mut bindings,
                &mut shape_bindings,
            ) {
                self.lowerer
                    .errors
                    .push(MirError::validation(span, "error destination type mismatch"));
                return None;
            }
        }

        let params = self.generic_param_order.get(&source)?;
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
                    let Some(concrete) = resolve_parent_shape_binding(self.types(), bound, parent_shape_bindings)
                    else {
                        self.lowerer.errors.push(MirError::validation(
                            span,
                            "generic call signature has unresolved type parameter",
                        ));
                        return None;
                    };
                    shape_args.push(canonical_index(self.types(), concrete));
                }
            }
        }
        Some(MirMonomorphizationKey { source, type_args, shape_args })
    }

    fn bind_explicit_type_args(
        &mut self,
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
        // Size-only generics: NATURAL typeArguments bind magnitudo params,
        // and a caller-owned symbolic `Param` resolves through the calling
        // instance's shape bindings (GSF-1 forwarding). Mixed/Typus callees
        // keep D1 (type args zip Typus; shapes from witnesses).
        if type_params.is_empty() && !shape_params.is_empty() {
            if explicit_type_args.len() != shape_params.len() {
                self.lowerer.errors.push(MirError::validation(
                    span,
                    format!(
                        "generic call supplied {} type arguments for {} parameters",
                        explicit_type_args.len(),
                        shape_params.len()
                    ),
                ));
                return None;
            }
            for (param, concrete) in shape_params.iter().zip(explicit_type_args.iter().copied()) {
                let Some(index) = self.explicit_shape_witness(concrete, span) else {
                    self.lowerer.errors.push(MirError::validation(
                        span,
                        "generic call type argument does not match parameter kind",
                    ));
                    return None;
                };
                shape_bindings.insert(param.name, canonical_index(self.types(), index));
            }
            return Some(());
        }
        if explicit_type_args.len() != type_params.len() {
            self.lowerer.errors.push(MirError::validation(
                span,
                format!(
                    "generic call supplied {} type arguments for {} parameters",
                    explicit_type_args.len(),
                    type_params.len()
                ),
            ));
            return None;
        }
        for (param, concrete) in type_params.iter().zip(explicit_type_args.iter().copied()) {
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

    fn magnitudo_literal_of(&self, ty: MirType) -> Option<u64> {
        let Type::Param(sym) = self.types().get(ty.semantic_id()) else {
            return None;
        };
        let sym = *sym;
        radix_types::parse_natural_capacity(self.lowerer.interner.resolve(sym))
    }

    /// Index witness of an explicit magnitudo argument: a NATURAL spelling
    /// maps to its literal index, a caller-owned symbolic `Param` stays the
    /// rigid param node (GSF-1 forwarding) — the key derivation resolves it
    /// through the calling instance's shape bindings. Anything else fails
    /// closed so wrong-kind rejection is preserved.
    fn explicit_shape_witness(&mut self, concrete: MirType, span: Span) -> Option<IndexId> {
        let Type::Param(sym) = self.types().get(concrete.semantic_id()) else {
            return None;
        };
        let sym = *sym;
        if let Some(value) = self.magnitudo_literal_of(concrete) {
            let index = find_literal_index(self.types(), value);
            if index.is_none() {
                self.lowerer.errors.push(MirError::validation(
                    span,
                    "generic call signature has unresolved type parameter",
                ));
            }
            return index;
        }
        let index = find_param_index(self.types(), sym);
        if index.is_none() {
            self.lowerer.errors.push(MirError::validation(
                span,
                "generic call type argument does not match parameter kind",
            ));
        }
        index
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

    fn substitute_function(&mut self, function: &mut MirFunction, bindings: &MirGenericBindings, span: Span) -> bool {
        // Shape params of the generic being instantiated (GSF-1): forwarded
        // size witnesses in nested callee type args stay symbolic through
        // typus substitution; the key derivation resolves them through the
        // calling instance's shape bindings.
        let shape_params: FxHashSet<Symbol> = function
            .source
            .and_then(|source| self.generic_param_order.get(&source))
            .map(|params| {
                params
                    .iter()
                    .filter(|param| param.kind == ParamKind::Magnitudo)
                    .map(|param| param.name)
                    .collect()
            })
            .unwrap_or_default();
        for param in &mut function.params {
            param.ty = match self.substitute_ty(param.ty, bindings, span) {
                Some(ty) => ty,
                None => return false,
            };
        }
        for local in &mut function.locals {
            local.ty = match self.substitute_ty(local.ty, bindings, span) {
                Some(ty) => ty,
                None => return false,
            };
        }
        for temp in &mut function.temps {
            temp.ty = match self.substitute_ty(temp.ty, bindings, span) {
                Some(ty) => ty,
                None => return false,
            };
        }
        function.return_ty = match self.substitute_ty(function.return_ty, bindings, span) {
            Some(ty) => ty,
            None => return false,
        };
        function.error_ty = match function.error_ty {
            Some(error_ty) => match self.substitute_ty(error_ty, bindings, span) {
                Some(error_ty) => Some(error_ty),
                None => return false,
            },
            None => None,
        };

        for block in &mut function.blocks {
            for statement in &mut block.statements {
                if !self.substitute_statement(statement, bindings, &shape_params, span) {
                    return false;
                }
            }
            if let MirTerminatorKind::TryCall { callee, .. } = &mut block.terminator.kind
                && !self.substitute_callee(callee, bindings, &shape_params, span)
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
        shape_params: &FxHashSet<Symbol>,
        span: Span,
    ) -> bool {
        match &mut statement.kind {
            MirStatementKind::Assign { value, .. } => self.substitute_value(value, bindings, span),
            MirStatementKind::Call { callee, .. } => self.substitute_callee(callee, bindings, shape_params, span),
            MirStatementKind::RuntimeCall { call, .. } => self.substitute_runtime_call(call, bindings, span),
            MirStatementKind::Construct { aggregate, .. } => self.substitute_aggregate(aggregate, bindings, span),
        }
    }
    fn substitute_callee(
        &mut self,
        callee: &mut MirCallee,
        bindings: &MirGenericBindings,
        shape_params: &FxHashSet<Symbol>,
        span: Span,
    ) -> bool {
        let MirCallee::Definition { type_args, .. } = callee else {
            return true;
        };
        for type_arg in type_args {
            // GSF-1: a forwarded size witness stays a symbolic `Param` in
            // callee type args — it is not a typus binding and is resolved
            // through the calling instance's shape bindings at key
            // derivation. Substituting it here would reject the caller-owned
            // magnitudo symbol as an unresolved type parameter.
            if let Type::Param(sym) = self.types().get(type_arg.semantic_id())
                && shape_params.contains(sym)
                && !bindings.contains_key(sym)
            {
                continue;
            }
            *type_arg = match self.substitute_ty(*type_arg, bindings, span) {
                Some(ty) => ty,
                None => return false,
            };
        }
        true
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
        instantiate_existing_generic_type(self.types(), ty, bindings, span)
            .map_err(|error| {
                self.lowerer
                    .errors
                    .push(MirError::validation(error.span, error.message));
            })
            .ok()
    }

    fn apply_rewrites(&mut self, rewrites: Vec<Rewrite>) {
        for rewrite in rewrites {
            let Some(function) = function_by_id_mut(&mut self.lowerer.functions, rewrite.function) else {
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
        self.lowerer
            .functions
            .retain(|function| !template_ids.contains(&function.id));
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
            MirProjection::Index(_) => index_projection_result_ty(base_ty, self.types())
                .map_err(|err| {
                    self.lowerer
                        .errors
                        .push(MirError::validation(span, err.to_string()));
                })
                .ok(),
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
                        .find_sized_numeric(Primitive::Numerus, crate::semantic::NumericWidth::U64)
                        .unwrap_or_else(|| self.lowerer.types.primitive(Primitive::Numerus)),
                ));
            }
            MirConstant::Float(_) => Primitive::Fractus,
            MirConstant::Bool(_) => Primitive::Bivalens,
            MirConstant::Unit => Primitive::Vacuum,
            MirConstant::String(_) => Primitive::Textus,
            MirConstant::Ascii(_) => Primitive::Ascii,
            MirConstant::Nil => Primitive::Nihil,
            MirConstant::Octeti(_) => Primitive::Octeti,
            MirConstant::Regex { .. } => Primitive::Regex,
            MirConstant::Function(id) => {
                return function_by_id(&self.lowerer.functions, *id)
                    .map(|function| function.return_ty)
                    .or_else(|| {
                        self.lowerer
                            .errors
                            .push(MirError::validation(span, format!("unknown function f{}", id.0)));
                        None
                    });
            }
            // A shape-size scalar is a float-sized compile-time value.
            MirConstant::ShapeSize { .. } => Primitive::Fractus,
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
        crate::semantic::format_faber_type(ty.semantic_id(), self.types(), self.lowerer.interner, &|def_id| {
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
}

fn generic_param_order(hir: &crate::hir::HirModule) -> FxHashMap<DefId, Vec<OrderedParam>> {
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
                for method in &strukt.methods {
                    let params = generic_param_names(&method.func.type_params);
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
/// instance. Tuple terms are accepted only when every child resolves; a shape
/// that remains a parameter, inference hole, or unspecified is genuinely
/// unbound and fails closed under D6.
pub(super) fn resolve_parent_shape_binding(
    types: &TypeTable,
    shape: IndexId,
    parent_shape_bindings: &MirShapeBindings,
) -> Option<IndexId> {
    match types.get_index(shape) {
        IndexExpr::Literal(_) => Some(shape),
        IndexExpr::Param(name) => parent_shape_bindings.get(name).copied().and_then(|bound| {
            (bound != shape)
                .then(|| resolve_parent_shape_binding(types, bound, parent_shape_bindings))
                .flatten()
        }),
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

/// All generic parameters in declaration order — `magnitudo` shape params sit
/// parallel to `typus` params so the witness binds both from the call site.
fn find_literal_index(types: &TypeTable, value: u64) -> Option<IndexId> {
    (0..types.index_count()).find_map(|index| {
        #[expect(clippy::cast_possible_truncation, reason = "IndexId is a u32 arena index by design")]
        let id = IndexId(index as u32);
        matches!(types.get_index(id), IndexExpr::Literal(found) if *found == value).then_some(id)
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

fn generic_param_names(params: &[crate::hir::HirTypeParam]) -> Vec<OrderedParam> {
    params
        .iter()
        .map(|param| OrderedParam {
            name: param.name,
            kind: if param.is_typus() {
                ParamKind::Typus
            } else {
                ParamKind::Magnitudo
            },
        })
        .collect()
}

/// O(1) when `id.0` matches the dense vector index (common lowering order).
/// Linear fallback keeps sparse/reordered IDs correct after template removal.
pub(super) fn function_by_id(functions: &[MirFunction], id: MirFunctionId) -> Option<&MirFunction> {
    if let Some(function) = functions.get(id.0 as usize)
        && function.id == id
    {
        return Some(function);
    }
    functions.iter().find(|function| function.id == id)
}

/// Mutable variant of [`function_by_id`] for monomorphization rewrites.
pub(super) fn function_by_id_mut(functions: &mut [MirFunction], id: MirFunctionId) -> Option<&mut MirFunction> {
    if functions
        .get(id.0 as usize)
        .is_some_and(|function| function.id == id)
    {
        return functions.get_mut(id.0 as usize);
    }
    functions.iter_mut().find(|function| function.id == id)
}

/// Concretize shape params in monomorphized instances (4A, final pass).
///
/// Shape substitution interns the concrete shapes/types each instance carries
/// into `types` — the lowering-owned extension arena cloned from the analyzed
/// unit's table at the lowering entry point, so the analyzed unit's arena is
/// never written. Each instance's shape params are bound from its key's
/// `shape_args` (D1) and substituted to literal dims; any function still
/// carrying a param afterwards is rejected (D6 net).
///
/// Phase-4 producer fact: every `MirConstant::ShapeSize` operand/constant in
/// an instance is also rewritten to the concrete `Float` it denotes from that
/// instance's shape bindings, and a final survivor net rejects any shape-size
/// scalar that substitution could not concretize (missing/unresolved/
/// infer/unspecified/out-of-range binding) before any backend runs.
pub(super) fn finalize_monomorphized_shapes(
    hir: &crate::hir::HirModule,
    types: &mut TypeTable,
    functions: &mut [MirFunction],
    monomorphizations: &[MirMonomorphization],
) -> Vec<MirError> {
    let mut errors = Vec::new();
    let param_order = generic_param_order(hir);
    let type_bindings = MirGenericBindings::default();
    let index = ExistingTypeIndex::build(types);
    for monomorphization in monomorphizations {
        let Some(params) = param_order.get(&monomorphization.key.source) else {
            continue;
        };
        let Some(function) = function_by_id_mut(functions, monomorphization.function) else {
            continue;
        };
        let mut shape_bindings = MirShapeBindings::default();
        let mut shape_args = monomorphization.key.shape_args.iter();
        for param in params {
            if param.kind == ParamKind::Magnitudo {
                let Some(concrete) = shape_args.next().copied() else {
                    break;
                };
                shape_bindings.insert(param.name, concrete);
            }
        }
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
    for function in functions {
        if function_contains_shape_size(function) {
            errors.push(MirError::validation(function.span, UNRESOLVED_SHAPE_SIZE_SURVIVOR));
        }
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

#[cfg(test)]
#[path = "monomorphize_test.rs"]
mod tests;
