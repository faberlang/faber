//! Callable values and captured-closure lowering (M-004).
//!
//! Closure expressions lower to synthetic MIR functions referenced by
//! `MirConstant::Function`. Indirect calls use `MirCallee::Value`. This module
//! owns the nested-function builder path for `clausura` and collection
//! callbacks, then emits ordinary MIR call statements for function-id and
//! value-backed callees.
//!
//! ERROR STRATEGY
//! ==============
//! Failable closures and callbacks fail closed until MIR models alternate-exit
//! call contracts for synthetic functions. Nested lowering errors are merged
//! into the parent builder and abort closure emission when any diagnostic was
//! recorded.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;
use radix_hir::visit::{HirVisitor, walk_expr};

const FAILABLE_CALLBACK_MESSAGE: &str = "failable collection callbacks before failable-callable MIR lowering";

impl FunctionBuilder<'_> {
    pub(super) fn lower_clausura_expr(
        &mut self,
        params: &[radix_hir::HirParam],
        ret_ty: Option<&TypeId>,
        err_ty: Option<&TypeId>,
        body: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let return_ty = ret_ty
            .map(|ty| MirType::semantic(*ty))
            .or_else(|| self.infer_closure_return_ty(body, expr))?;
        let captures = self.collect_closure_captures(params, body);
        let function_id = self.lower_closure_from_parts(
            params,
            ret_ty.copied(),
            err_ty.copied(),
            body,
            return_ty,
            expr.ty.map(MirType::semantic),
            captures.as_slice(),
            expr.span,
        )?;
        let value_ty = self.expr_ty(expr)?;
        if captures.is_empty() {
            // A bare function constant has no semantic callable type in MIR;
            // validators can only recover the nested function's return type.
            // Materialize the constant through a typed temp so higher-order
            // calls retain the closure signature at their argument boundary.
            return Some(self.assign_temp(
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Function(function_id))),
                value_ty,
                expr.span,
            ));
        }
        let environment_id = MirClosureEnvironmentId(function_id.0);
        let fields = captures
            .iter()
            .map(|capture| MirNamedOperand {
                name: capture.name,
                value: MirOperand::Place(capture.source_place.clone()),
            })
            .collect();
        let environment = self.construct_temp(
            MirAggregateKind::ClosureEnvironment(environment_id),
            MirAggregateFields::Named(fields),
            MirType::semantic(self.types.primitive(Primitive::Ignotum)),
            expr.span,
        );
        let closure = MirClosureCallee { function: function_id, environment, environment_id };
        let operand = self.assign_temp(
            MirValueKind::Closure(MirClosureValue {
                function: closure.function,
                environment: closure.environment.clone(),
                environment_id: closure.environment_id,
            }),
            value_ty,
            expr.span,
        );
        if let MirOperand::Temp(temp) = operand {
            self.closure_temp_callees.insert(temp, closure);
        }
        Some(operand)
    }

    /// A path naming a top-level function, used as a value.
    ///
    /// MIR function ids are assigned as items lower, so the target's id is not
    /// known here. The value is a synthetic forwarding function with the
    /// target's parameter modes that calls the definition; the call through the
    /// value then behaves exactly like a direct call (mutable parameters stay
    /// references, failable targets propagate their error).
    pub(super) fn lower_function_value(&mut self, def_id: DefId, expr: &HirExpression) -> Option<MirOperand> {
        if !self.context.function_params.contains_key(&def_id) {
            return None;
        }
        let value_ty = expr.ty?;
        // WHY: a named function bound into an optional function type
        // (`let Op ∪ none f ← shout`) is typed at the store's option type; the
        // function value is the present payload.
        let signature = match self.normalized_type(value_ty) {
            Type::Func(signature) => signature.clone(),
            Type::Option(inner) => match self.normalized_type(*inner) {
                Type::Func(signature) => signature.clone(),
                _ => return None,
            },
            _ => return None,
        };
        let span = expr.span;
        if signature.is_async || signature.is_generator || !signature.type_params.is_empty() {
            self.errors.push(MirError::unsupported(
                span,
                "async, generator, or generic function used as a value",
            ));
            return None;
        }
        let return_ty = MirType::semantic(signature.ret);
        let error_ty = signature.err.map(MirType::semantic);
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; synthetic function count is bounded by u32::MAX"
        )]
        let id = MirFunctionId(self.parent_function_id.0 + 1 + self.synthetic_functions.len() as u32);
        let mut nested = FunctionBuilder::for_function(
            self.types,
            error_ty,
            Self::clone_context(&self.context),
            id,
            self.device_role,
            None,
        );
        let mut args = Vec::with_capacity(signature.params.len());
        for param in &signature.params {
            let local = nested.next_local_id();
            let storage_ty = match self.types.get(param.ty) {
                Type::Ref(_, inner) => *inner,
                _ => param.ty,
            };
            let ty = MirType::semantic(storage_ty);
            let (mode, mutable) = match param.mode {
                radix_types::SemanticParamMode::Owned => (MirParamMode::Owned, false),
                radix_types::SemanticParamMode::Ref => (MirParamMode::Ref, false),
                radix_types::SemanticParamMode::MutRef => (MirParamMode::MutRef, true),
                radix_types::SemanticParamMode::Move => (MirParamMode::Move, false),
            };
            nested
                .params
                .push(MirParam { local, name: None, ty, mode, span });
            nested
                .locals
                .push(MirLocalDecl { id: local, name: None, ty, mutable, span });
            args.push(MirOperand::Place(MirPlace::local(local)));
        }
        let entry = nested.fresh_block(span);
        nested.switch_to(entry);
        let callee = MirCallee::definition(def_id);
        let result = match error_ty {
            Some(error_ty) => nested.lower_failable_call(callee, args, return_ty, error_ty, None, span)?,
            None => nested.emit_call(callee, args, return_ty, span),
        };
        let returned = if nested.is_vacuum(return_ty) {
            None
        } else {
            Some(result)
        };
        nested.terminate_current(MirTerminatorKind::Return(returned), span);
        let blocks = nested.finish_blocks();
        let nested_error_count = nested.errors.len();
        self.errors.extend(nested.errors);
        if nested_error_count > 0 {
            return None;
        }
        self.synthetic_functions.push(MirFunction {
            id,
            source: None,
            name: None,
            params: nested.params,
            locals: nested.locals,
            temps: nested.temps,
            blocks,
            return_ty,
            error_ty,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        });
        Some(self.assign_temp(
            MirValueKind::Operand(MirOperand::Constant(MirConstant::Function(id))),
            MirType::semantic(value_ty),
            span,
        ))
    }

    pub(super) fn lower_closure_function(
        &mut self,
        closure: &HirExpression,
        expected_return: MirType,
    ) -> Option<MirCallee> {
        self.lower_collection_closure_function(closure, expected_return, 1)
    }

    pub(super) fn lower_reducer_function(
        &mut self,
        closure: &HirExpression,
        expected_return: MirType,
    ) -> Option<MirCallee> {
        self.lower_collection_closure_function(closure, expected_return, 2)
    }

    fn lower_collection_closure_function(
        &mut self,
        closure: &HirExpression,
        expected_return: MirType,
        expected_params: usize,
    ) -> Option<MirCallee> {
        let HirExpressionKind::Clausura(params, ret_ty, err_ty, body) = &closure.kind else {
            self.errors.push(MirError::unsupported(
                closure.span,
                "collection callback must be a closure expression",
            ));
            return None;
        };
        if err_ty.is_some() {
            self.errors
                .push(MirError::unsupported(closure.span, FAILABLE_CALLBACK_MESSAGE));
            return None;
        }
        if params.len() != expected_params {
            self.errors.push(MirError::unsupported(
                closure.span,
                "collection callback arity does not match intrinsic",
            ));
            return None;
        }
        let return_ty = ret_ty.map_or(expected_return, MirType::semantic);
        let captures = self.collect_closure_captures(params, body);
        let function_id = self.lower_closure_from_parts(
            params,
            ret_ty.as_ref().copied(),
            None,
            body,
            return_ty,
            closure.ty.map(MirType::semantic),
            captures.as_slice(),
            closure.span,
        )?;
        if captures.is_empty() {
            return Some(MirCallee::Function(function_id));
        }
        let environment_id = MirClosureEnvironmentId(function_id.0);
        let fields = captures
            .iter()
            .map(|capture| MirNamedOperand {
                name: capture.name,
                value: MirOperand::Place(capture.source_place.clone()),
            })
            .collect();
        let environment = self.construct_temp(
            MirAggregateKind::ClosureEnvironment(environment_id),
            MirAggregateFields::Named(fields),
            MirType::semantic(self.types.primitive(Primitive::Ignotum)),
            closure.span,
        );
        Some(MirCallee::Closure(MirClosureCallee {
            function: function_id,
            environment,
            environment_id,
        }))
    }

    fn lower_closure_from_parts(
        &mut self,
        params: &[radix_hir::HirParam],
        ret_ty: Option<TypeId>,
        err_ty: Option<TypeId>,
        body: &HirExpression,
        return_ty: MirType,
        value_ty: Option<MirType>,
        captures: &[MirClosureCapture],
        span: Span,
    ) -> Option<MirFunctionId> {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; synthetic function count is bounded by u32::MAX"
        )]
        let id = MirFunctionId(self.parent_function_id.0 + 1 + self.synthetic_functions.len() as u32);
        let environment_id = MirClosureEnvironmentId(id.0);
        let mut nested = FunctionBuilder::for_function(
            self.types,
            err_ty.map(MirType::semantic),
            Self::clone_context(&self.context),
            id,
            self.device_role,
            None,
        );
        if !captures.is_empty() {
            let value_ty = value_ty.unwrap_or_else(|| MirType::semantic(self.types.primitive(Primitive::Ignotum)));
            let environment_local = nested.add_hidden_closure_environment_local(value_ty, span);
            for capture in captures {
                let slot = MirPlace {
                    base: MirPlaceBase::Local(environment_local),
                    projections: vec![MirProjection::ClosureCapture {
                        environment: environment_id,
                        source: capture.source,
                    }],
                };
                let binding_place = match capture.mode {
                    MirCaptureMode::Value => slot,
                    MirCaptureMode::Shared => {
                        let mut content = slot.clone();
                        content.projections.push(MirProjection::SharedCell);
                        nested.shared_cells.insert(capture.source, slot);
                        content
                    }
                };
                nested
                    .captured_bindings
                    .insert(capture.source, binding_place);
                nested
                    .captured_binding_metadata
                    .insert(capture.source, (capture.name, capture.ty));
            }
            self.closure_environments.push(MirClosureEnvironment {
                id: environment_id,
                function: id,
                value_ty,
                environment_local,
                captures: captures.to_vec(),
            });
        }
        for param in params {
            nested.add_param(param.def_id, param.name, param.ty, param.mode, param.span);
        }
        let blocks = if let HirExpressionKind::Block(block) = &body.kind {
            nested.lower_body(block)
        } else {
            nested.shared_bindings = radix_hir::capture::CaptureFacts::for_expr(body).shared;
            let entry = nested.fresh_block(body.span);
            nested.switch_to(entry);
            nested.emit_copy_param_entries();
            if let Some(value) = nested.lower_expr_value(body) {
                nested.terminate_current(MirTerminatorKind::Return(Some(value)), body.span);
            }
            nested.finish_blocks()
        };
        let nested_synthetics = nested.take_synthetic_functions();
        let nested_environments = nested.take_closure_environments();
        let nested_metadata = nested.take_device_function_metadata();
        let nested_error_count = nested.errors.len();
        self.errors.extend(nested.errors);
        if nested_error_count > 0 {
            return None;
        }
        if blocks.is_empty() {
            self.errors
                .push(MirError::unsupported(span, "closure produced no MIR blocks"));
            return None;
        }

        self.synthetic_functions.push(MirFunction {
            id,
            source: None,
            name: None,
            params: nested.params,
            locals: nested.locals,
            temps: nested.temps,
            blocks,
            return_ty: ret_ty.map_or(return_ty, MirType::semantic),
            error_ty: err_ty.map(MirType::semantic),
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        });
        self.synthetic_functions.extend(nested_synthetics);
        self.closure_environments.extend(nested_environments);
        self.device_function_metadata.extend(nested_metadata);
        Some(id)
    }

    pub(super) fn call_value_operand(
        &mut self,
        callee: MirOperand,
        args: Vec<MirOperand>,
        return_ty: MirType,
        span: Span,
    ) -> MirOperand {
        if let Some(closure) = self.closure_callee_for_operand(&callee) {
            return self.emit_call(MirCallee::Closure(closure), args, return_ty, span);
        }
        self.emit_call(MirCallee::Value(callee), args, return_ty, span)
    }

    pub(super) fn call_callback_callee(
        &mut self,
        callee: MirCallee,
        args: Vec<MirOperand>,
        return_ty: MirType,
        span: Span,
    ) -> MirOperand {
        self.emit_call(callee, args, return_ty, span)
    }

    // consumes callee and args into MIR statements
    fn emit_call(&mut self, callee: MirCallee, args: Vec<MirOperand>, return_ty: MirType, span: Span) -> MirOperand {
        if self.is_vacuum(return_ty) {
            self.append_stmt(MirStatement { kind: MirStatementKind::Call { destination: None, callee, args }, span });
            return MirOperand::Constant(MirConstant::Unit);
        }

        let destination = self.push_temp(return_ty, span);
        self.append_stmt(MirStatement {
            kind: MirStatementKind::Call { destination: Some(MirPlace::temp(destination)), callee, args },
            span,
        });
        MirOperand::Temp(destination)
    }

    /// Return type for an arrowless closure head.
    ///
    /// The head-derived type is the block's tail expression, or the body's own
    /// type when the body is not a block. When neither is present the closure
    /// expression's annotated callable type supplies the return (a binding such
    /// as `let (int) → void f ← int n ∴ do { … }`). A statement-only `do`/`fac`
    /// body with no annotation performs statements and returns nothing.
    fn infer_closure_return_ty(&self, body: &HirExpression, expr: &HirExpression) -> Option<MirType> {
        let head_ty = match &body.kind {
            HirExpressionKind::Block(block) => block.expr.as_ref().and_then(|tail| tail.ty),
            _ => body.ty,
        };
        if let Some(head_ty) = head_ty {
            return Some(MirType::semantic(head_ty));
        }
        if let Some(return_ty) = self.closure_annotation_return_ty(expr) {
            return Some(return_ty);
        }
        match &body.kind {
            HirExpressionKind::Block(_) => Some(MirType::semantic(self.types.primitive(Primitive::Vacuum))),
            _ => None,
        }
    }

    /// The return type a callable-typed closure expression carries from its binding
    /// annotation, when it is a function type.
    fn closure_annotation_return_ty(&self, expr: &HirExpression) -> Option<MirType> {
        let ty = expr.ty?;
        match self.normalized_type(ty) {
            Type::Func(signature) => Some(MirType::semantic(signature.ret)),
            _ => None,
        }
    }

    pub(super) fn clone_context<'a>(context: &'a FunctionBuilderContext<'a>) -> FunctionBuilderContext<'a> {
        FunctionBuilderContext {
            interner: context.interner,
            empty_textus: context.empty_textus,
            diagnostic_format_templates: context.diagnostic_format_templates,
            diagnostic_separator: context.diagnostic_separator,
            diagnostic_nihil: context.diagnostic_nihil,
            diagnostic_lockstep_length_mismatch: context.diagnostic_lockstep_length_mismatch,
            diagnostic_ordo_no_member: context.diagnostic_ordo_no_member,
            diagnostic_potentia_negative_exponent: context.diagnostic_potentia_negative_exponent,
            diagnostic_range_step_not_positive: context.diagnostic_range_step_not_positive,
            diagnostic_map_changed_size: context.diagnostic_map_changed_size,
            diagnostic_ponde_invalid_index: context.diagnostic_ponde_invalid_index,
            diagnostic_accipe_invalid_index: context.diagnostic_accipe_invalid_index,
            diagnostic_at_out_of_range: context.diagnostic_at_out_of_range,
            diagnostic_slice_out_of_range: context.diagnostic_slice_out_of_range,
            diagnostic_take_out_of_range: context.diagnostic_take_out_of_range,
            diagnostic_ranged_push_past_maximum: context.diagnostic_ranged_push_past_maximum,
            diagnostic_ranged_remove_below_minimum: context.diagnostic_ranged_remove_below_minimum,
            fault_delivery: context.fault_delivery,
            function_errors: Arc::clone(&context.function_errors),
            function_params: Arc::clone(&context.function_params),
            imported_function_params: Arc::clone(&context.imported_function_params),
            tensor_edge_policies: Arc::clone(&context.tensor_edge_policies),
            pending_tensor_limes: Arc::clone(&context.pending_tensor_limes),
            shape_calls: Arc::clone(&context.shape_calls),
            shape_functions: Arc::clone(&context.shape_functions),
            call_size_bindings: Arc::clone(&context.call_size_bindings),
            shape_stores: Arc::clone(&context.shape_stores),
            proven_indexes: Arc::clone(&context.proven_indexes),
            function_size_bindings: Arc::clone(&context.function_size_bindings),
            function_async: Arc::clone(&context.function_async),
            function_generator: Arc::clone(&context.function_generator),
            structs: Arc::clone(&context.structs),
            imported_struct_defaults: Arc::clone(&context.imported_struct_defaults),
            struct_field_types: Arc::clone(&context.struct_field_types),
            variant_parents: Arc::clone(&context.variant_parents),
            enum_variants: Arc::clone(&context.enum_variants),
            enum_variant_by_name: Arc::clone(&context.enum_variant_by_name),
            enum_type_params: Arc::clone(&context.enum_type_params),
            variant_fields: Arc::clone(&context.variant_fields),
            variant_field_tys: Arc::clone(&context.variant_field_tys),
            conversion_rows: Arc::clone(&context.conversion_rows),
            sermo_def: context.sermo_def,
            meus_def: context.meus_def,
            tuus_def: context.tuus_def,
            scrinium_def: context.scrinium_def,
            top_level_consts: Arc::clone(&context.top_level_consts),
            static_fields: Arc::clone(&context.static_fields),
            imported_static_fields: Arc::clone(&context.imported_static_fields),
            imported_constants: Arc::clone(&context.imported_constants),
            provider_imports: Arc::clone(&context.provider_imports),
            imported_type_members: Arc::clone(&context.imported_type_members),
            method_targets: Arc::clone(&context.method_targets),
            creo_hooks: Arc::clone(&context.creo_hooks),
            interface_implementors: Arc::clone(&context.interface_implementors),
            imported_method_providers: Arc::clone(&context.imported_method_providers),
            gpu_builtins: Arc::clone(&context.gpu_builtins),
            cli_parsed_record_fields_by_type: Arc::clone(&context.cli_parsed_record_fields_by_type),
            cli_record_fields_by_type: Arc::clone(&context.cli_record_fields_by_type),
            cli_adapter_active: context.cli_adapter_active,
            kernel_lowering: context.kernel_lowering,
        }
    }

    pub(super) fn collect_expression_captures(
        &self,
        excluded: FxHashSet<DefId>,
        body: &HirExpression,
    ) -> Vec<MirClosureCapture> {
        let mut collector = ClosureCaptureCollector {
            parent: self,
            params: excluded,
            seen: FxHashSet::default(),
            captures: Vec::new(),
        };
        collector.visit_expr(body);
        collector.captures
    }

    fn collect_closure_captures(&self, params: &[radix_hir::HirParam], body: &HirExpression) -> Vec<MirClosureCapture> {
        let params = params.iter().map(|param| param.def_id).collect();
        let mut collector =
            ClosureCaptureCollector { parent: self, params, seen: FxHashSet::default(), captures: Vec::new() };
        collector.visit_expr(body);
        collector.captures
    }

    /// The outer bindings a `filum` term block reads: every referenced
    /// definition bound in this builder that is not one of `excluded` (the
    /// term's own coordinate and element binders).
    pub(super) fn collect_block_captures(
        &self,
        excluded: FxHashSet<DefId>,
        body: &radix_hir::HirBlock,
    ) -> Vec<MirClosureCapture> {
        let mut collector = ClosureCaptureCollector {
            parent: self,
            params: excluded,
            seen: FxHashSet::default(),
            captures: Vec::new(),
        };
        collector.visit_block(body);
        collector.captures
    }

    fn add_hidden_closure_environment_local(&mut self, ty: MirType, span: Span) -> MirLocalId {
        let id = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id, name: None, ty, mutable: false, span });
        id
    }

    pub(super) fn closure_callee_for_operand(&self, operand: &MirOperand) -> Option<MirClosureCallee> {
        match operand {
            MirOperand::Place(place) if place.projections.is_empty() => match place.base {
                MirPlaceBase::Local(local) => self.closure_local_callees.get(&local).cloned(),
                MirPlaceBase::Temp(temp) => self.closure_temp_callees.get(&temp).cloned(),
            },
            MirOperand::Temp(temp) => self.closure_temp_callees.get(temp).cloned(),
            _ => None,
        }
    }

    pub(super) fn take_device_function_metadata(&mut self) -> MirDeviceFunctionMetadataMap {
        std::mem::take(&mut self.device_function_metadata)
    }
}

struct ClosureCaptureCollector<'a, 'builder> {
    parent: &'a FunctionBuilder<'builder>,
    params: FxHashSet<DefId>,
    seen: FxHashSet<DefId>,
    captures: Vec<MirClosureCapture>,
}

impl HirVisitor for ClosureCaptureCollector<'_, '_> {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Path(def_id) = &expr.kind {
            self.capture_path(*def_id);
        }
        walk_expr(self, expr);
    }
}

impl ClosureCaptureCollector<'_, '_> {
    fn capture_path(&mut self, def_id: DefId) {
        if self.params.contains(&def_id) || !self.seen.insert(def_id) {
            return;
        }
        // A shared binding is captured as its cell, so the closure and the
        // enclosing scope alias one storage location.
        if let (Some(cell), Some((name, ty))) = (
            self.parent.shared_cells.get(&def_id),
            self.parent.captured_binding_metadata.get(&def_id),
        ) {
            self.captures.push(MirClosureCapture {
                source: def_id,
                name: *name,
                ty: *ty,
                source_place: cell.clone(),
                mode: MirCaptureMode::Shared,
            });
            return;
        }
        if let (Some(source_place), Some((name, ty))) = (
            self.parent.captured_bindings.get(&def_id),
            self.parent.captured_binding_metadata.get(&def_id),
        ) {
            self.captures.push(MirClosureCapture {
                source: def_id,
                name: *name,
                ty: *ty,
                source_place: source_place.clone(),
                mode: MirCaptureMode::Value,
            });
            return;
        }
        let Some(binding) = self.parent.bindings.get(&def_id).copied() else {
            return;
        };
        let Some(local) = self
            .parent
            .locals
            .iter()
            .find(|local| local.id == binding.local)
        else {
            return;
        };
        let Some(name) = local.name else {
            return;
        };
        self.captures.push(MirClosureCapture {
            source: def_id,
            name,
            ty: binding.ty,
            source_place: MirPlace::local(binding.local),
            mode: MirCaptureMode::Value,
        });
    }
}
