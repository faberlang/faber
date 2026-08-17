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
use crate::hir::visit::{walk_expr, HirVisitor};

const FAILABLE_CALLBACK_MESSAGE: &str = "failable collection callbacks before failable-callable MIR lowering";

impl FunctionBuilder<'_> {
    pub(super) fn lower_clausura_expr(
        &mut self,
        params: &[crate::hir::HirParam],
        ret_ty: Option<&TypeId>,
        err_ty: Option<&TypeId>,
        body: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let return_ty = ret_ty
            .map(|ty| MirType::semantic(*ty))
            .or_else(|| self.infer_closure_return_ty(body))?;
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
        params: &[crate::hir::HirParam],
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
                nested.captured_bindings.insert(
                    capture.source,
                    MirPlace {
                        base: MirPlaceBase::Local(environment_local),
                        projections: vec![MirProjection::ClosureCapture {
                            environment: environment_id,
                            source: capture.source,
                        }],
                    },
                );
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
            let entry = nested.fresh_block(body.span);
            nested.switch_to(entry);
            if let Some(value) = nested.lower_expr_value(body) {
                nested.terminate_current(MirTerminatorKind::Return(Some(value)), body.span);
            }
            nested.finish_blocks()
        };
        let nested_synthetics = nested.take_synthetic_functions();
        let nested_environments = nested.take_closure_environments();
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

    fn infer_closure_return_ty(&self, body: &HirExpression) -> Option<MirType> {
        match &body.kind {
            HirExpressionKind::Block(block) => block
                .expr
                .as_ref()
                .and_then(|expr| expr.ty.map(MirType::semantic)),
            _ => body.ty.map(MirType::semantic),
        }
    }

    pub(super) fn clone_context<'a>(context: &'a FunctionBuilderContext<'a>) -> FunctionBuilderContext<'a> {
        FunctionBuilderContext {
            interner: context.interner,
            empty_textus: context.empty_textus,
            diagnostic_format_templates: context.diagnostic_format_templates,
            diagnostic_separator: context.diagnostic_separator,
            diagnostic_nihil: context.diagnostic_nihil,
            function_errors: Arc::clone(&context.function_errors),
            function_params: Arc::clone(&context.function_params),
            function_async: Arc::clone(&context.function_async),
            function_generator: Arc::clone(&context.function_generator),
            structs: Arc::clone(&context.structs),
            struct_field_types: Arc::clone(&context.struct_field_types),
            variant_parents: Arc::clone(&context.variant_parents),
            enum_variants: Arc::clone(&context.enum_variants),
            enum_type_params: Arc::clone(&context.enum_type_params),
            variant_fields: Arc::clone(&context.variant_fields),
            variant_field_tys: Arc::clone(&context.variant_field_tys),
            sermo_def: context.sermo_def,
            meus_def: context.meus_def,
            tuus_def: context.tuus_def,
            scrinium_def: context.scrinium_def,
            scrinium_data_field: context.scrinium_data_field,
            top_level_consts: Arc::clone(&context.top_level_consts),
            provider_imports: Arc::clone(&context.provider_imports),
            method_targets: Arc::clone(&context.method_targets),
            imported_method_providers: Arc::clone(&context.imported_method_providers),
            gpu_builtins: Arc::clone(&context.gpu_builtins),
            cli_record_fields_by_local: Arc::clone(&context.cli_record_fields_by_local),
            cli_record_fields_by_type: Arc::clone(&context.cli_record_fields_by_type),
            cli_adapter_active: context.cli_adapter_active,
        }
    }

    fn collect_closure_captures(
        &self,
        params: &[crate::hir::HirParam],
        body: &HirExpression,
    ) -> Vec<MirClosureCapture> {
        let params = params.iter().map(|param| param.def_id).collect();
        let mut collector =
            ClosureCaptureCollector { parent: self, params, seen: FxHashSet::default(), captures: Vec::new() };
        collector.visit_expr(body);
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
        if let (Some(source_place), Some((name, ty))) = (
            self.parent.captured_bindings.get(&def_id),
            self.parent.captured_binding_metadata.get(&def_id),
        ) {
            self.captures.push(MirClosureCapture {
                source: def_id,
                name: *name,
                ty: *ty,
                source_place: source_place.clone(),
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
        });
    }
}
