//! Ordered reference scans. Device lowering keeps its own collective plan;
//! this path uses exact accumulator temporaries and stores only emitted prefixes.
use super::expr::HirExpressionLoweringVisitor;
use super::{
    BreakableContext, FilumCapture, FunctionBuilder, HirBlock, HirExpression, HirExpressionKind, HirStatement,
    HirStatementKind, LocalBinding, MirAggregateFields, MirAggregateItem, MirAggregateKind, MirBinOp, MirCollectionOp,
    MirConstant, MirError, MirIntrinsic, MirLocalDecl, MirOperand, MirPlace, MirTerminatorKind, MirType, Primitive,
    ReductionBucket, ReductionFoldContext, Span, Type, TypeId, TypeTable,
};
use radix_hir::visit::HirVisitor;
use radix_hir::{HirReductionOp, HirScanTotal, HirSortDirection};
use radix_types::i64s_from_literal_index;

/// Reference grouped state is sparse in observed keys, so a finite integer
/// domain does not require allocating its full cardinality on the host.
pub(super) fn ensure_scan_types(hir: &radix_hir::HirModule, types: &mut TypeTable) {
    struct Collect(Vec<(Vec<TypeId>, TypeId)>);
    impl HirVisitor for Collect {
        fn visit_expr(&mut self, expr: &HirExpression) {
            if let HirExpressionKind::Scan { group: Some(group), .. } = &expr.kind
                && let Some(result) = expr.ty
            {
                let keys = radix_hir::reduction_group_keys(group)
                    .iter()
                    .filter_map(|key| key.ty)
                    .collect();
                self.0.push((keys, result));
            }
            radix_hir::visit::walk_expr(self, expr);
        }
    }
    let mut collector = Collect(Vec::new());
    radix_hir::visit::walk_program(&mut collector, hir);
    for (keys, result) in collector.0 {
        let Type::Tensor(element, _) = types.get(result) else {
            continue;
        };
        let element = *element;
        let key = if keys.len() == 1 {
            keys[0]
        } else {
            types.intern(Type::Tuple(keys))
        };
        types.intern(Type::Map(key, element));
    }
}

impl FunctionBuilder<'_> {
    pub(super) fn lower_scan_expr(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        if !self.is_host_kernel_lowering() {
            return self.lower_device_scan_expr(expr);
        }
        let HirExpressionKind::Scan {
            op, total, source, coordinates, group, group_domain, order, binding, body, ..
        } = &expr.kind
        else {
            return None;
        };
        if matches!(op, HirReductionOp::ArgMax | HirReductionOp::ArgMin | HirReductionOp::First) {
            self.errors
                .push(MirError::unsupported(expr.span, "scan_operator_unsupported"));
            return None;
        }
        if *total == HirScanTotal::Exclusive && matches!(op, HirReductionOp::Max | HirReductionOp::Min) {
            self.errors
                .push(MirError::unsupported(expr.span, "scan_exclusive_extremum_unsupported"));
            return None;
        }
        if group.is_some() && group_domain.is_none() {
            self.errors
                .push(MirError::unsupported(expr.span, "scan_group_domain_unbounded"));
            return None;
        }
        if !scan_term_is_total(body) {
            self.errors
                .push(MirError::unsupported(expr.span, "scan_term_return_not_total"));
            return None;
        }
        let span = expr.span;
        let source_ty = self.types.peel_access_marker(source.ty?).1;
        let (Type::Tensor(element, shape) | Type::Matrix(element, shape)) = self.normalized_type(source_ty) else {
            self.errors
                .push(MirError::unsupported(span, "scan_source_not_tensor"));
            return None;
        };
        let (element, shape) = (*element, *shape);
        let result_ty = MirType::semantic(expr.ty?);
        let Type::Tensor(result_element, _) = self.normalized_type(expr.ty?) else {
            return None;
        };
        let accumulator_ty = MirType::semantic(*result_element);
        let term_ty = scan_term_type(body)?;
        let integer = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let boolean = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let vacuum = MirType::semantic(self.types.primitive(Primitive::Vacuum));
        let dimensions = i64s_from_literal_index(shape, self.types);
        let value = self.lower_expr_value(source)?;
        let source = self.materialize_operand_place(value, MirType::semantic(source_ty), span);
        let bounds = self.tensor_axis_bounds(&source, dimensions.as_deref(), coordinates.len(), span)?;
        let dims = self.scan_index_list(bounds.clone(), span)?;
        let zero = self.scan_zero(accumulator_ty);
        let initial = match op {
            HirReductionOp::Product => match zero {
                MirConstant::Float(_) => MirConstant::Float(1.0),
                _ => MirConstant::Int(1),
            },
            HirReductionOp::All => MirConstant::Bool(true),
            _ => zero.clone(),
        };
        let seed = self.construct_temp(
            MirAggregateKind::Tensor,
            MirAggregateFields::Ordered(Vec::new()),
            result_ty,
            span,
        );
        let output = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorCreate),
            vec![seed, MirOperand::Constant(zero.clone()), dims],
            result_ty,
            span,
        );
        let mut length = MirOperand::Constant(MirConstant::Int(1));
        for bound in &bounds {
            length = self.binary_temp(MirBinOp::Mul, length, bound.clone(), integer, span);
        }
        let saved_bindings = self.bindings.clone();
        let coordinate_ty = coordinates
            .first()
            .and_then(|(id, _)| super::control::reduction_coordinate_type(Some(body), group.as_deref(), *id))
            .map_or(integer, MirType::semantic);
        let mut locals = Vec::new();
        for (id, name) in coordinates {
            let local = self.next_local_id();
            self.locals
                .push(MirLocalDecl { id: local, name: Some(*name), ty: coordinate_ty, mutable: true, span });
            self.bindings
                .insert(*id, LocalBinding { local, ty: coordinate_ty });
            locals.push(local);
        }
        let element_local = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: element_local,
            name: Some(binding.1),
            ty: MirType::semantic(element),
            mutable: true,
            span,
        });
        self.bindings
            .insert(binding.0, LocalBinding { local: element_local, ty: MirType::semantic(element) });
        let term_local = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: term_local, name: None, ty: term_ty, mutable: true, span });
        let bucket = ReductionBucket {
            accumulator: self.push_temp(accumulator_ty, span),
            score: self.push_temp(term_ty, span),
            seen: self.push_temp(boolean, span),
            coordinates: Vec::new(),
        };
        self.assign(
            MirPlace::temp(bucket.accumulator),
            MirOperand::Constant(initial.clone()),
            accumulator_ty,
            span,
        );
        let term_zero = self.scan_zero(term_ty);
        self.assign(
            MirPlace::temp(bucket.score),
            MirOperand::Constant(term_zero.clone()),
            term_ty,
            span,
        );
        self.assign(
            MirPlace::temp(bucket.seen),
            MirOperand::Constant(MirConstant::Bool(false)),
            boolean,
            span,
        );
        let state = if let Some(group) = group {
            let key_types: Vec<_> = radix_hir::reduction_group_keys(group)
                .iter()
                .map(|key| key.ty)
                .collect::<Option<_>>()?;
            let key_ty = if key_types.len() == 1 {
                key_types[0]
            } else {
                self.types.find_equal(&Type::Tuple(key_types))?
            };
            let map_ty = self
                .types
                .find_equal(&Type::Map(key_ty, accumulator_ty.semantic_id()))?;
            Some((
                self.construct_temp(
                    MirAggregateKind::Map,
                    MirAggregateFields::Keyed(Vec::new()),
                    MirType::semantic(map_ty),
                    span,
                ),
                key_ty,
            ))
        } else {
            None
        };
        let index = self.push_temp(integer, span);
        let descending = *order == HirSortDirection::Descending;
        let start = if descending {
            self.binary_temp(
                MirBinOp::Sub,
                length.clone(),
                MirOperand::Constant(MirConstant::Int(1)),
                integer,
                span,
            )
        } else {
            MirOperand::Constant(MirConstant::Int(0))
        };
        self.assign(MirPlace::temp(index), start, integer, span);
        let condition = self.fresh_block(span);
        let step = self.fresh_block(span);
        let yielded = self.fresh_block(span);
        let after = self.fresh_block(span);
        self.terminate_current(MirTerminatorKind::Goto(condition), span);
        self.switch_to(condition);
        let in_range = if descending {
            self.binary_temp(
                MirBinOp::GtEq,
                MirOperand::Temp(index),
                MirOperand::Constant(MirConstant::Int(0)),
                boolean,
                span,
            )
        } else {
            self.binary_temp(MirBinOp::Lt, MirOperand::Temp(index), length, boolean, span)
        };
        self.terminate_current(
            MirTerminatorKind::Branch { condition: in_range, then_block: step, else_block: after },
            span,
        );
        self.switch_to(step);
        let mut remainder = MirOperand::Temp(index);
        for axis in (0..locals.len()).rev() {
            let coordinate =
                self.binary_temp(MirBinOp::Mod, remainder.clone(), bounds[axis].clone(), coordinate_ty, span);
            self.assign(MirPlace::local(locals[axis]), coordinate, coordinate_ty, span);
            remainder = self.binary_temp(MirBinOp::Div, remainder, bounds[axis].clone(), integer, span);
        }
        let indexes = self.scan_index_list(
            locals
                .iter()
                .map(|local| MirOperand::Place(MirPlace::local(*local)))
                .collect(),
            span,
        )?;
        let loaded = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorGet),
            vec![MirOperand::Place(source), indexes.clone()],
            MirType::semantic(element),
            span,
        );
        self.assign(MirPlace::local(element_local), loaded, MirType::semantic(element), span);
        let old_capture = self
            .filum_capture
            .replace(FilumCapture { term_result: term_local, ty: term_ty });
        // Isolate the term's return from an enclosing reduction's return scope.
        let old_fold = self.reduction_fold.take();
        self.breakables
            .push(BreakableContext { perge_target: Some(yielded), rumpe_target: after, rumpe_used: false });
        self.visit_block(body);
        self.breakables.pop();
        self.filum_capture = old_capture;
        self.reduction_fold = old_fold;
        self.terminate_open_current(MirTerminatorKind::Goto(yielded), span);
        self.switch_to(yielded);
        let key = if let (Some(group), Some((state, key_ty))) = (group, &state) {
            let keys = radix_hir::reduction_group_keys(group)
                .iter()
                .map(|key| self.lower_expr_value(key))
                .collect::<Option<Vec<_>>>()?;
            let key = if keys.len() == 1 {
                keys[0].clone()
            } else {
                self.construct_temp(
                    MirAggregateKind::Tuple,
                    MirAggregateFields::Ordered(keys.into_iter().map(MirAggregateItem::Operand).collect()),
                    MirType::semantic(*key_ty),
                    span,
                )
            };
            let already_present = self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Contains),
                vec![state.clone(), key.clone()],
                boolean,
                span,
            );
            self.assign(MirPlace::temp(bucket.seen), already_present, boolean, span);
            self.assign(
                MirPlace::temp(bucket.accumulator),
                MirOperand::Constant(initial.clone()),
                accumulator_ty,
                span,
            );
            self.assign(MirPlace::temp(bucket.score), MirOperand::Constant(term_zero), term_ty, span);
            let existing = self.fresh_block(span);
            let ready = self.fresh_block(span);
            self.terminate_current(
                MirTerminatorKind::Branch {
                    condition: MirOperand::Temp(bucket.seen),
                    then_block: existing,
                    else_block: ready,
                },
                span,
            );
            self.switch_to(existing);
            let previous = self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Index),
                vec![state.clone(), key.clone()],
                accumulator_ty,
                span,
            );
            self.assign(MirPlace::temp(bucket.accumulator), previous.clone(), accumulator_ty, span);
            if matches!(op, HirReductionOp::Max | HirReductionOp::Min) {
                self.assign(MirPlace::temp(bucket.score), previous, term_ty, span);
            }
            self.terminate_current(MirTerminatorKind::Goto(ready), span);
            self.switch_to(ready);
            Some(key)
        } else {
            None
        };
        if *total == HirScanTotal::Exclusive {
            self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::TensorSet),
                vec![output.clone(), indexes.clone(), MirOperand::Temp(bucket.accumulator)],
                vacuum,
                span,
            );
        }
        let fold = ReductionFoldContext {
            op: *op,
            term_ty,
            accumulator_ty,
            buckets: Vec::new(),
            coordinates: locals,
            coordinate_ty,
            group: None,
            group_dims: Vec::new(),
            error_tensor: None,
            element: None,
        };
        self.combine_reduction_bucket(&fold, &bucket, MirOperand::Place(MirPlace::local(term_local)), span);
        if *total == HirScanTotal::Inclusive {
            self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::TensorSet),
                vec![output.clone(), indexes, MirOperand::Temp(bucket.accumulator)],
                vacuum,
                span,
            );
        }
        if let (Some((state, _)), Some(key)) = (state, key) {
            self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Put),
                vec![state, key, MirOperand::Temp(bucket.accumulator)],
                vacuum,
                span,
            );
        }
        let next = self.binary_temp(
            if descending { MirBinOp::Sub } else { MirBinOp::Add },
            MirOperand::Temp(index),
            MirOperand::Constant(MirConstant::Int(1)),
            integer,
            span,
        );
        self.assign(MirPlace::temp(index), next, integer, span);
        self.terminate_open_current(MirTerminatorKind::Goto(condition), span);
        self.switch_to(after);
        self.bindings = saved_bindings;
        Some(output)
    }

    fn scan_index_list(&mut self, values: Vec<MirOperand>, span: Span) -> Option<MirOperand> {
        let integer = self.types.primitive(Primitive::Numerus);
        let ty = MirType::semantic(self.require_array(integer, span, "scan tensor coordinates")?);
        Some(self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(values.into_iter().map(MirAggregateItem::Operand).collect()),
            ty,
            span,
        ))
    }

    fn scan_zero(&self, ty: MirType) -> MirConstant {
        match self.normalized_type(ty.semantic_id()) {
            Type::Primitive(Primitive::Bivalens) => MirConstant::Bool(false),
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => MirConstant::Float(0.0),
            _ => MirConstant::Int(0),
        }
    }
}

fn scan_term_type(body: &HirBlock) -> Option<MirType> {
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

// A scan emits one value for every visited source cell. A loop-control escape
// or a term with a fall-through path cannot supply that value.
fn scan_term_is_total(body: &HirBlock) -> bool {
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
