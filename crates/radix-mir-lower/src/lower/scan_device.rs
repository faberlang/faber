//! Typed device-scan declaration lowering. The reference interpreter keeps
//! its explicit loop implementation in `scan.rs`; this module builds the
//! distinct collective node consumed by device planners and emitters.

use super::expr::HirExpressionLoweringVisitor;
use super::{
    DefId, FunctionBuilder, FxHashSet, HirBlock, HirExpression, HirExpressionKind, HirParamMode, HirStatement,
    HirStatementKind, HirVisitor, MirCaptureMode, MirClosureCapture, MirConstant, MirDeviceFunctionMetadata,
    MirDeviceRole, MirError, MirFunction, MirFunctionId, MirFunctionOrigin, MirFunctionVisibility, MirIntrinsic,
    MirKernelShaderStage, MirOperand, MirReduceOp, MirScanGroupAxis, MirScanGroupOp, MirScanOp, MirScanTotal,
    MirSyntheticFunctionIdentity, MirTerminatorKind, MirType, Primitive, Span, Symbol, Type, TypeId,
};
use radix_hir::{HirReductionOp, HirScanKeyDomain, HirScanTotal};

impl FunctionBuilder<'_> {
    /// Lower a typed scan into a device collective declaration. The caller
    /// selects this route only for a device kernel; ordinary reference scans
    /// retain their language-level loop implementation.
    pub(super) fn lower_device_scan_expr(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        let HirExpressionKind::Scan {
            op, total, source, coordinates, group, group_domain, order, binding, body, ..
        } = &expr.kind
        else {
            return None;
        };
        let Some(scan_op) = scan_mir_reduce_op(*op) else {
            self.errors
                .push(MirError::unsupported(expr.span, "scan_operator_unsupported"));
            return None;
        };
        if *total == HirScanTotal::Exclusive && matches!(op, HirReductionOp::Max | HirReductionOp::Min) {
            self.errors
                .push(MirError::unsupported(expr.span, "scan_exclusive_extremum_unsupported"));
            return None;
        }
        if !scan_device_term_is_total(body) {
            self.errors
                .push(MirError::unsupported(expr.span, "scan_term_return_not_total"));
            return None;
        }
        let span = expr.span;
        let source_ty = self.types.peel_access_marker(source.ty?).1;
        let element_ty = match self.normalized_type(source_ty) {
            Type::Tensor(element, _) => *element,
            Type::Matrix(_, _) => {
                self.errors
                    .push(MirError::unsupported(span, "scan_matrix_source_device_unsupported"));
                return None;
            }
            _ => {
                self.errors
                    .push(MirError::unsupported(span, "scan_source_not_tensor"));
                return None;
            }
        };
        let rank = coordinates.len();
        if rank == 0 {
            self.errors
                .push(MirError::unsupported(span, "scan_coordinate_rank_mismatch"));
            return None;
        }
        let rank = u8::try_from(rank).ok()?;
        let result_ty = MirType::semantic(expr.ty?);
        let Type::Tensor(result_element, _) = self.normalized_type(expr.ty?) else {
            self.errors
                .push(MirError::unsupported(span, "scan_result_not_tensor"));
            return None;
        };
        let result_element_ty = MirType::semantic(*result_element);
        let term_ty = scan_device_term_type(body)?;

        let source_value = self.lower_expr_value(source)?;
        let source_place = self.materialize_operand_place(source_value, MirType::semantic(source_ty), span);
        let mut args = vec![MirOperand::Place(source_place)];
        let (term_function, term_captures) =
            self.lower_scan_term_function(body, coordinates, binding, element_ty, term_ty, group.as_deref(), span)?;
        args.extend(
            term_captures
                .iter()
                .map(|capture| MirOperand::Place(capture.source_place.clone())),
        );
        args.push(MirOperand::Constant(MirConstant::Function(term_function)));

        let scan_group = if let (Some(group), Some(group_domain)) = (group.as_deref(), group_domain) {
            if !super::control::grouped_key_tensor_reads_are_proven(self.types, source, coordinates, group) {
                self.errors
                    .push(MirError::unsupported(group.span, "scan_group_key_index_unproven"));
                return None;
            }
            let keys = radix_hir::reduction_group_keys(group);
            if keys.len() != group_domain.key_domains.len() {
                self.errors
                    .push(MirError::unsupported(span, "scan_group_domain_rank_mismatch"));
                return None;
            }
            let mut axes = Vec::with_capacity(keys.len());
            for (key, domain) in keys.iter().zip(&group_domain.key_domains) {
                let key_ty = MirType::semantic(key.ty?);
                let (min, extent) = scan_device_key_domain(*domain)
                    .ok_or_else(|| {
                        self.errors
                            .push(MirError::unsupported(key.span, "scan_group_domain_too_large_for_device"));
                    })
                    .ok()?;
                if extent == 0 {
                    self.errors
                        .push(MirError::unsupported(key.span, "scan_group_domain_empty_for_device"));
                    return None;
                }
                let excluded = coordinates.iter().map(|(def_id, _)| *def_id).collect();
                let captures = self.collect_expression_captures(excluded, key);
                if captures
                    .iter()
                    .any(|capture| capture.mode == MirCaptureMode::Shared)
                {
                    self.errors
                        .push(MirError::unsupported(key.span, "scan_group_key_captures_shared_state"));
                    return None;
                }
                let function = self.lower_scan_group_key_function(key, group, coordinates, &captures, key_ty, span)?;
                args.extend(
                    captures
                        .iter()
                        .map(|capture| MirOperand::Place(capture.source_place.clone())),
                );
                args.push(MirOperand::Constant(MirConstant::Function(function)));
                axes.push(MirScanGroupAxis { min, extent, key_ty, captures: u32::try_from(captures.len()).ok()? });
            }
            let group_count = axes
                .iter()
                .try_fold(1_u64, |count, axis| count.checked_mul(axis.extent));
            let summary_count = group_count
                .and_then(|count| count.checked_mul(u64::from(radix_mir::kernel_plan::GRID_REDUCE_PARTIALS_CAP)));
            if group_count.is_none_or(|count| count > u64::from(u32::MAX))
                || summary_count.is_none_or(|count| count > u64::from(u32::MAX))
            {
                self.errors
                    .push(MirError::unsupported(span, "scan_group_domain_too_large_for_device"));
                return None;
            }
            Some(MirScanGroupOp { axes })
        } else if group.is_some() {
            self.errors
                .push(MirError::unsupported(span, "scan_group_domain_unbounded"));
            return None;
        } else {
            None
        };

        let declaration = MirScanOp {
            op: scan_op,
            total: match total {
                HirScanTotal::Inclusive => MirScanTotal::Inclusive,
                HirScanTotal::Exclusive => MirScanTotal::Exclusive,
            },
            reverse: *order == radix_hir::HirSortDirection::Descending,
            rank,
            term_ty,
            result_element_ty,
            term_captures: u32::try_from(term_captures.len()).ok()?,
            group: scan_group,
        };
        let call = self.runtime_call_value(MirIntrinsic::Scan(declaration), args, result_ty, span);
        let result = self.materialize_operand_place(call, result_ty, span);
        Some(MirOperand::Place(result))
    }

    fn lower_scan_term_function(
        &mut self,
        body: &HirBlock,
        coordinates: &[(DefId, Symbol)],
        binding: &(DefId, Symbol),
        element_ty: TypeId,
        term_ty: MirType,
        group: Option<&HirExpression>,
        span: Span,
    ) -> Option<(MirFunctionId, Vec<MirClosureCapture>)> {
        let mut excluded: FxHashSet<DefId> = coordinates.iter().map(|(def_id, _)| *def_id).collect();
        excluded.insert(binding.0);
        let captures = self.collect_block_captures(excluded, body);
        if captures
            .iter()
            .any(|capture| capture.mode == MirCaptureMode::Shared)
        {
            self.errors
                .push(MirError::unsupported(span, "scan_term_captures_shared_state"));
            return None;
        }
        let id = scan_synthetic_function_id(self)?;
        let mut nested = FunctionBuilder::for_function(
            self.types,
            None,
            Self::clone_context(&self.context),
            id,
            self.device_role,
            None,
        );
        for capture in &captures {
            nested.add_param(
                capture.source,
                capture.name,
                capture.ty.semantic_id(),
                HirParamMode::Owned,
                span,
            );
        }
        for (def_id, name) in coordinates {
            let ty = super::control::reduction_coordinate_type(Some(body), group, *def_id)
                .unwrap_or_else(|| self.types.primitive(Primitive::Numerus));
            nested.add_param(*def_id, *name, ty, HirParamMode::Owned, span);
        }
        nested.add_param(binding.0, binding.1, element_ty, HirParamMode::Owned, span);
        let blocks = nested.lower_body(body);
        let nested_synthetics = nested.take_synthetic_functions();
        let nested_environments = nested.take_closure_environments();
        let nested_metadata = nested.take_device_function_metadata();
        let nested_error_count = nested.errors.len();
        self.errors.extend(nested.errors);
        if nested_error_count != 0 {
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
            return_ty: term_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        });
        self.synthetic_functions.extend(nested_synthetics);
        self.closure_environments.extend(nested_environments);
        self.device_function_metadata.extend(nested_metadata);
        self.device_function_metadata.insert(
            id,
            MirDeviceFunctionMetadata {
                identity: MirSyntheticFunctionIdentity {
                    parent: self.parent_function_id,
                    ordinal: id.0 - self.parent_function_id.0,
                },
                role: MirDeviceRole::Kernel,
                shader_stage: MirKernelShaderStage::Compute,
                visibility: MirFunctionVisibility::Private,
                origin: MirFunctionOrigin::SyntheticKernelClosure,
                lane_domain_local: None,
                refused_backends: Default::default(),
                tier: radix_mir::device::KernelTier::Default,
            },
        );
        Some((id, captures))
    }

    fn lower_scan_group_key_function(
        &mut self,
        key: &HirExpression,
        group: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        captures: &[MirClosureCapture],
        key_ty: MirType,
        span: Span,
    ) -> Option<MirFunctionId> {
        let id = scan_synthetic_function_id(self)?;
        let mut nested = FunctionBuilder::for_function(
            self.types,
            None,
            Self::clone_context(&self.context),
            id,
            self.device_role,
            None,
        );
        for capture in captures {
            nested.add_param(
                capture.source,
                capture.name,
                capture.ty.semantic_id(),
                HirParamMode::Owned,
                span,
            );
        }
        for (def_id, name) in coordinates {
            let ty = super::control::reduction_coordinate_type(None, Some(group), *def_id)
                .unwrap_or_else(|| self.types.primitive(Primitive::Numerus));
            nested.add_param(*def_id, *name, ty, HirParamMode::Owned, span);
        }
        let entry = nested.fresh_block(key.span);
        nested.switch_to(entry);
        nested.emit_copy_param_entries();
        let Some(value) = nested.lower_expr_value(key) else {
            self.errors.extend(nested.errors);
            return None;
        };
        nested.terminate_current(MirTerminatorKind::Return(Some(value)), key.span);
        let blocks = nested.finish_blocks();
        let nested_synthetics = nested.take_synthetic_functions();
        let nested_environments = nested.take_closure_environments();
        let nested_metadata = nested.take_device_function_metadata();
        let nested_error_count = nested.errors.len();
        self.errors.extend(nested.errors);
        if nested_error_count != 0 {
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
            return_ty: key_ty,
            error_ty: None,
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        });
        self.synthetic_functions.extend(nested_synthetics);
        self.closure_environments.extend(nested_environments);
        self.device_function_metadata.extend(nested_metadata);
        self.device_function_metadata.insert(
            id,
            MirDeviceFunctionMetadata {
                identity: MirSyntheticFunctionIdentity {
                    parent: self.parent_function_id,
                    ordinal: id.0 - self.parent_function_id.0,
                },
                role: MirDeviceRole::Kernel,
                shader_stage: MirKernelShaderStage::Compute,
                visibility: MirFunctionVisibility::Private,
                origin: MirFunctionOrigin::SyntheticKernelClosure,
                lane_domain_local: None,
                refused_backends: Default::default(),
                tier: radix_mir::device::KernelTier::Default,
            },
        );
        Some(id)
    }
}

/// The device scan recipe for a value-prefix operator; `None` for the
/// coordinate extrema and the first reduction, which no scan carries.
fn scan_mir_reduce_op(op: HirReductionOp) -> Option<MirReduceOp> {
    Some(match op {
        HirReductionOp::Sum => MirReduceOp::Sum,
        HirReductionOp::Product => MirReduceOp::Product,
        HirReductionOp::Max => MirReduceOp::Max,
        HirReductionOp::Min => MirReduceOp::Min,
        HirReductionOp::All => MirReduceOp::All,
        HirReductionOp::Any => MirReduceOp::Any,
        HirReductionOp::Count => MirReduceOp::Count,
        HirReductionOp::ArgMax | HirReductionOp::ArgMin | HirReductionOp::First => return None,
    })
}

fn scan_device_key_domain(domain: HirScanKeyDomain) -> Option<(i128, u64)> {
    let (min, max) = match domain {
        HirScanKeyDomain::IntegerRange { min, max } => (min, max),
        HirScanKeyDomain::IntegerWidth(width) => width.integer_range()?,
    };
    if max < min {
        return Some((min, 0));
    }
    let extent = max.checked_sub(min)?.checked_add(1)?;
    Some((min, u64::try_from(extent).ok()?))
}

fn scan_synthetic_function_id(builder: &FunctionBuilder<'_>) -> Option<MirFunctionId> {
    let offset = u32::try_from(builder.synthetic_functions.len())
        .ok()?
        .checked_add(1)?;
    Some(MirFunctionId(builder.parent_function_id.0.checked_add(offset)?))
}

fn scan_device_term_type(body: &HirBlock) -> Option<MirType> {
    struct Term(Option<TypeId>);
    impl HirVisitor for Term {
        fn visit_stmt(&mut self, statement: &HirStatement) {
            if let HirStatementKind::Redde(Some(value)) = &statement.kind {
                self.0 = value.ty;
            }
            radix_hir::visit::walk_stmt(self, statement);
        }
    }
    let mut term = Term(None);
    term.visit_block(body);
    term.0.map(MirType::semantic)
}

fn scan_device_term_is_total(body: &HirBlock) -> bool {
    struct Escapes(bool);
    impl HirVisitor for Escapes {
        fn visit_stmt(&mut self, statement: &HirStatement) {
            if matches!(statement.kind, HirStatementKind::Perge | HirStatementKind::Rumpe) {
                self.0 = true;
            }
            radix_hir::visit::walk_stmt(self, statement);
        }
        fn visit_expr(&mut self, expr: &HirExpression) {
            if matches!(
                expr.kind,
                HirExpressionKind::Scan { .. } | HirExpressionKind::Reduction { .. } | HirExpressionKind::Clausura(..)
            ) {
                return;
            }
            radix_hir::visit::walk_expr(self, expr);
        }
    }
    fn returns(body: &HirBlock) -> bool {
        body.statements
            .iter()
            .any(|statement| match &statement.kind {
                HirStatementKind::Redde(Some(_)) => true,
                HirStatementKind::Expr(expr) => match &expr.kind {
                    HirExpressionKind::Si { then_block, else_block: Some(other), .. } => {
                        returns(then_block) && returns(other)
                    }
                    HirExpressionKind::Block(inner) => returns(inner),
                    _ => false,
                },
                _ => false,
            })
    }
    let mut escapes = Escapes(false);
    escapes.visit_block(body);
    !escapes.0 && returns(body)
}
