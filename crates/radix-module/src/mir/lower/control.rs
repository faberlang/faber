//! Control-flow and alternate-exit lowering for MIR functions.
//!
//! This module turns expression-level control constructs into explicit MIR
//! blocks and terminators. It owns the contracts for `si`, `dum`, `rumpe`,
//! `perge`, `iace`, and `cape`: each construct either wires concrete CFG edges
//! or reports an unsupported lowering diagnostic. Value-producing control flow
//! writes into a destination place so branch arms join through storage rather
//! than through an implicit expression stack.
//!
//! ERROR HANDLING
//! ==============
//! Local `cape` handlers are modeled as a stack of handler contexts. `iace` and
//! failable calls assign the error value into the active handler place and jump
//! to its handler block. Without a local handler, `iace` can only lower for
//! functions with a declared alternate-exit type; otherwise it fails closed.
//!
//! CFG INVARIANTS
//! ==============
//! - Branch and loop helpers create all successor blocks before terminating the
//!   current block.
//! - Reachable arms jump to an explicit join block.
//! - Unreachable joins are sealed with `Unreachable` so MIR validation sees a
//!   terminator on every block.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;
use crate::hir::visit::HirVisitor;
use radix_types::i64s_from_literal_index;

struct RangeIteraLowering<'a> {
    binding: DefId,
    name: Symbol,
    start: &'a HirExpression,
    end: &'a HirExpression,
    step: Option<&'a HirExpression>,
    kind: HirRangeKind,
    body: &'a HirBlock,
    span: Span,
}

#[derive(Clone)]
enum CollectionIteraKind {
    Array(TypeId),
    MapKeys(TypeId),
    MapValues(TypeId),
    Cursor(TypeId),
    AsyncCursor(TypeId),
    Tensor(TypeId),
    /// Unified shaped lane: tensors and matrices share rank-N loop lowering.
    /// The matrix flag selects static cell projections for the register carrier;
    /// tensors use the runtime indexed-access ABI for rank-N coordinates.
    Shaped {
        element: TypeId,
        dimensions: Vec<i64>,
        matrix: bool,
    },
    // Route B (AEU-4): runtime-len `lista<T, N>` splits on the carrier `len`
    // into N+1 guard-free unrolled arms; the literal capacity is carried so
    // the split stays base-lane MIR (never AIR).
    BoundedArray(TypeId, i64),
    Unsupported,
}

struct ArrayIteraLowering<'a> {
    mode: HirIteraMode,
    coordinates: &'a [(DefId, Symbol)],
    binding: DefId,
    name: Symbol,
    iter: &'a HirExpression,
    body: &'a HirBlock,
    span: Span,
}

impl FunctionBuilder<'_> {
    /// Lower `iace` either to the innermost active `cape` handler or to a
    /// function-level alternate return.
    pub(super) fn lower_iace(&mut self, value: &HirExpression, span: Span) -> Option<MirOperand> {
        if let Some(handler) = self.handlers.last().cloned() {
            let error_ty = self.expr_ty(value)?;
            let value = self.lower_transfer_expr(value)?;
            let place = handler.error_place;
            self.assign(place, value, error_ty, span);
            self.terminate_current(MirTerminatorKind::Goto(handler.error_block), span);
            return None;
        }

        if self.error_ty.is_none() {
            self.errors
                .push(MirError::unsupported(span, "iace without a declared alternate-exit type"));
            return None;
        }

        let value = self.lower_transfer_expr(value)?;
        self.terminate_current(MirTerminatorKind::ReturnError(value), span);
        None
    }

    /// Lower a handled expression when the handler is statement-like.
    ///
    /// Expression-valued handlers are intentionally rejected until MIR has a
    /// value-join representation for handler results.
    pub(super) fn lower_handled_expr(
        &mut self,
        body: &HirBlock,
        catch: &HirCape,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        if !self.is_vacuum(ty) {
            self.errors.push(MirError::unsupported(
                expr.span,
                "expression-valued cape handler before value-join MIR lowering",
            ));
            return None;
        }

        self.lower_handled_block(body, catch, expr.span);
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    fn lower_handled_block(&mut self, body: &HirBlock, catch: &HirCape, span: Span) {
        let Some(error_ty) = catch.binding_ty.map(MirType::semantic) else {
            self.errors
                .push(MirError::missing_type(catch.span, "cape handler binding"));
            return;
        };

        let handler_id = self.fresh_block(catch.body.span);
        let after_id = self.fresh_block(span);
        let handler_local = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: handler_local,
            name: Some(catch.binding_name),
            ty: error_ty,
            mutable: false,
            span: catch.span,
        });
        let handler_binding = LocalBinding { local: handler_local, ty: error_ty };
        self.bindings.insert(catch.binding_def_id, handler_binding);

        self.handlers.push(HandlerContext {
            error_place: MirPlace::local(handler_local),
            error_ty,
            error_block: handler_id,
        });
        self.visit_block(body);
        self.handlers.pop();
        let body_reaches = self.terminate_open_current(MirTerminatorKind::Goto(after_id), span);

        self.switch_to(handler_id);
        self.visit_block(&catch.body);
        let handler_reaches = self.terminate_open_current(MirTerminatorKind::Goto(after_id), catch.span);

        if body_reaches || handler_reaches {
            self.switch_to(after_id);
        } else {
            self.seal_unreachable(after_id, span);
        }
    }

    /// Lower any expression into an already-chosen destination place.
    ///
    /// This is the join-point contract for expression-valued blocks and `si`:
    /// complex control forms assign their eventual value into `destination`,
    /// while ordinary expressions lower to an operand and are assigned once.
    pub(super) fn lower_expr_to_destination(
        &mut self,
        expr: &HirExpression,
        destination: MirPlace,
        ty: MirType,
    ) -> Option<()> {
        match &expr.kind {
            HirExpressionKind::Block(block) => self.lower_block_to_destination(block, destination, ty, expr.span),
            HirExpressionKind::Si { cond, then_block, then_catch, else_block } => {
                if then_catch.is_some() {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "expression-valued si with cape before handler value MIR lowering",
                    ));
                    return None;
                }
                self.lower_si_to_destination(cond, then_block, else_block.as_ref(), destination, ty, expr.span)
            }
            _ => {
                let value = self.lower_expr_value(expr)?;
                self.record_closure_destination(&destination, &value);
                if self.needs_vec3_to_vec4_widen(ty, expr) {
                    self.emit_vec3_to_vec4_construct(destination, value, ty, expr.span);
                } else {
                    self.assign(destination, value, ty, expr.span);
                }
                Some(())
            }
        }
    }

    /// Detect when a `@vertex` body assigns a vec3 value into a vec4
    /// destination — the lowering must emit a Construct(Vector) to widen
    /// the value with a `1.0` w-component instead of a plain Assign.
    fn needs_vec3_to_vec4_widen(&self, dest_ty: MirType, expr: &HirExpression) -> bool {
        if !matches!(
            self.shader_stage,
            Some(MirKernelShaderStage::Vertex | MirKernelShaderStage::Fragment)
        ) {
            return false;
        }
        let Some(f32_id) = self
            .types
            .find_sized_numeric(Primitive::Fractus, NumericWidth::F32)
        else {
            return false;
        };
        let is_vec4 = matches!(
            self.types.get(dest_ty.semantic_id()),
            Type::Vector(elem, width) if *elem == f32_id && matches!(self.types.get_index(*width), IndexExpr::Literal(4))
        );
        if !is_vec4 {
            return false;
        }
        let Some(value_ty) = expr.ty else {
            return false;
        };
        matches!(
            self.types.get(value_ty),
            Type::Vector(elem, width) if *elem == f32_id && matches!(self.types.get_index(*width), IndexExpr::Literal(3))
        )
    }

    /// Emit a `Construct(Vector)` statement that widens a vec3 operand to
    /// vec4 by projecting x, y, z as scalar lanes and appending a `1.0`
    /// w-component.  Each field is a scalar lane so MIR scalar-per-lane
    /// vector validation passes.
    fn emit_vec3_to_vec4_construct(&mut self, destination: MirPlace, value: MirOperand, ty: MirType, span: Span) {
        let f32_id = self
            .types
            .find_sized_numeric(Primitive::Fractus, NumericWidth::F32)
            .unwrap_or_else(|| self.types.primitive(Primitive::Fractus));
        let elem_ty = MirType::semantic(f32_id);

        // Materialize the vec3 source as a place so we can project lanes.
        let src_place = match value {
            MirOperand::Place(place) => place,
            MirOperand::Temp(temp) => MirPlace::temp(temp),
            other => {
                // Push a temp and assign the value to it first.
                // We know the source type is vec3<f32> from needs_vec3_to_vec4_widen.
                let temp = self.push_temp(elem_ty, span);
                self.assign(MirPlace::temp(temp), other, elem_ty, span);
                MirPlace::temp(temp)
            }
        };
        let one = self.assign_temp(
            MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(1.0))),
            elem_ty,
            span,
        );
        let aggregate = MirAggregate {
            kind: MirAggregateKind::Vector,
            ty,
            fields: MirAggregateFields::Ordered(vec![
                // Scalar-per-lane: four scalar elements, not mixed [vec3, scalar].
                Self::vec3_lane_operand(&src_place, 0),
                Self::vec3_lane_operand(&src_place, 1),
                Self::vec3_lane_operand(&src_place, 2),
                MirAggregateItem::Operand(one),
            ]),
        };
        self.append_stmt(MirStatement { kind: MirStatementKind::Construct { destination, aggregate }, span });
    }

    fn vec3_lane_operand(src: &MirPlace, lane: u8) -> MirAggregateItem {
        let mut projections = src.projections.clone();
        projections.push(MirProjection::VectorLane(lane));
        MirAggregateItem::Operand(MirOperand::Place(MirPlace { base: src.base.clone(), projections }))
    }

    /// Lower a HIR block used as an expression.
    ///
    /// Non-`vacuum` blocks allocate a temp and route the tail expression into
    /// it. Blocks with no tail expression are only valid when the expected MIR
    /// type is `vacuum`.
    pub(super) fn lower_block_expr(&mut self, block: &HirBlock, expr: &HirExpression) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        if self.is_vacuum(ty) {
            self.visit_block(block);
            return Some(MirOperand::Constant(MirConstant::Unit));
        }

        let temp = self.push_temp(ty, expr.span);
        self.lower_block_to_destination(block, MirPlace::temp(temp), ty, expr.span)?;
        Some(MirOperand::Temp(temp))
    }

    /// Lower `si` in expression position.
    ///
    /// `vacuum` branches are statement-like. Value-producing branches require
    /// a `secus` block and share a temp destination so the two arms join through
    /// ordinary MIR storage.
    pub(super) fn lower_si_expr(
        &mut self,
        cond: &HirExpression,
        then_block: &HirBlock,
        then_catch: Option<&HirCape>,
        else_block: Option<&HirBlock>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        if self.is_vacuum(ty) {
            self.lower_si_stmt(cond, then_block, then_catch, else_block, expr.span);
            return Some(MirOperand::Constant(MirConstant::Unit));
        }

        if then_catch.is_some() {
            self.errors.push(MirError::unsupported(
                expr.span,
                "expression-valued si with cape before handler value MIR lowering",
            ));
            return None;
        }

        let temp = self.push_temp(ty, expr.span);
        self.lower_si_to_destination(cond, then_block, else_block, MirPlace::temp(temp), ty, expr.span)?;
        Some(MirOperand::Temp(temp))
    }

    /// Lower `dum` in expression position.
    ///
    /// Loops currently have a statement-like MIR contract: they may produce
    /// `vacuum`, but a non-`vacuum` loop result is rejected until the language
    /// has a defined value-yielding loop model.
    pub(super) fn lower_dum_expr(
        &mut self,
        cond: &HirExpression,
        block: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        if !self.is_vacuum(ty) {
            self.errors
                .push(MirError::unsupported(expr.span, "dum expression with non-vacuum result"));
            return None;
        }

        self.lower_dum(cond, block, expr.span);
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    /// Lower supported `itera` subsets into target-neutral MIR.
    ///
    /// Numeric ranges and array-backed collection loops can use ordinary locals,
    /// branches, gotos, runtime length calls, and index projections. Map, set,
    /// text, and cursor iteration still need iterator/runtime ABI policy.
    pub(super) fn lower_itera_expr(
        &mut self,
        mode: HirIteraMode,
        coordinates: &[(DefId, Symbol)],
        binding: DefId,
        name: Symbol,
        iter: &HirExpression,
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        if !self.is_vacuum(ty) {
            self.errors
                .push(MirError::unsupported(expr.span, "itera expression with non-vacuum result"));
            return None;
        }
        match mode {
            HirIteraMode::Ab => {
                let HirExpressionKind::Intervallum { start, end, step, kind } = &iter.kind else {
                    self.errors
                        .push(MirError::unsupported(iter.span, "itera ab source before range MIR lowering"));
                    return None;
                };

                self.lower_range_itera(RangeIteraLowering {
                    binding,
                    name,
                    start,
                    end,
                    step: step.as_deref(),
                    kind: *kind,
                    body,
                    span: expr.span,
                })?;
            }
            HirIteraMode::Ex | HirIteraMode::De => {
                self.lower_array_itera(ArrayIteraLowering {
                    mode,
                    coordinates,
                    binding,
                    name,
                    iter,
                    body,
                    span: expr.span,
                })?;
            }
        }
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    /// Lower the sequential `summa ex` spelling to a MIR fold over the shared
    /// apud tensor-iteration arm (SFR-4).
    ///
    /// The accumulator starts at the `+` identity; a valued `redde` in the
    /// term body adds its value into the accumulator (through the summa fold
    /// context, never as a function return) and continues the iteration loop.
    /// The distributed `filum` spelling lowers here to the declared kernel-
    /// plan entry (SFR-5): a `FilumSum` runtime call carrying the filum
    /// metadata, the captured term-body MIR, and the coordinate facts —
    /// the plan pass resolves it to `DistributedSum`, never inline MIR.
    pub(super) fn lower_summa_expr(
        &mut self,
        source: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        filum: Option<&(DefId, Symbol)>,
        binding: &(DefId, Symbol),
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if let Some(filum) = filum {
            return self.lower_filum_summa_expr(source, coordinates, filum, binding, body, expr);
        }
        let Some(term_ty) = expr.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "summa term type"));
            return None;
        };
        let term_mir_ty = MirType::semantic(term_ty);
        let zero = match self.types.get(term_ty) {
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => MirConstant::Float(0.0),
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => MirConstant::Int(0),
            _ => {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "summa term before numeric scalar MIR lowering",
                ));
                return None;
            }
        };
        let accumulator = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: accumulator, name: None, ty: term_mir_ty, mutable: true, span: expr.span });
        self.assign(MirPlace::local(accumulator), MirOperand::Constant(zero), term_mir_ty, expr.span);
        let previous_fold = self
            .summa_fold
            .replace(SummaFoldContext { accumulator, ty: term_mir_ty });
        let lowered = self.lower_array_itera(ArrayIteraLowering {
            mode: HirIteraMode::Ex,
            coordinates,
            binding: binding.0,
            name: binding.1,
            iter: source,
            body,
            span: expr.span,
        });
        self.summa_fold = previous_fold;
        lowered?;
        Some(MirOperand::Place(MirPlace::local(accumulator)))
    }

    /// Frozen filum lane width `W` (SFR-5 settled default: W=32 cyclic; tails
    /// fail closed at admission — `K % W ≠ 0` is rejected by typecheck).
    const FILUM_LANE_WIDTH: u32 = 32;

    /// Lower the distributed `summa … filum` spelling to the declared
    /// kernel-plan entry (SFR-5): one `FilumSum` runtime call carrying the
    /// filum metadata, the captured term-body MIR, and the coordinate
    /// facts. The plan pass (`kernel_plan_for_function`) resolves the call
    /// to `CollectionKernelPlan::DistributedSum`; backends without a native
    /// arm lower it through the shared-memory tree floor the plan pins.
    ///
    /// The term body is lowered once into scratch blocks with the binder and
    /// coordinate locals bound; the emitted statements are captured into the
    /// declaration and the scratch blocks are dropped, so nothing of the
    /// term body runs inline in the base lane.
    fn lower_filum_summa_expr(
        &mut self,
        source: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        filum: &(DefId, Symbol),
        binding: &(DefId, Symbol),
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(term_ty) = expr.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "summa term type"));
            return None;
        };
        let term_mir_ty = MirType::semantic(term_ty);
        match self.types.get(term_ty) {
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {}
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => {}
            _ => {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "filum summa before numeric scalar MIR lowering",
                ));
                return None;
            }
        }
        let Some(source_ty) = source.ty else {
            self.errors
                .push(MirError::missing_type(source.span, "filum summa source type"));
            return None;
        };
        let element_ty = match self.normalized_type(source_ty) {
            Type::Tensor(element, _) => *element,
            _ => {
                self.errors.push(MirError::unsupported(
                    source.span,
                    "filum summa source before tensor MIR lowering",
                ));
                return None;
            }
        };
        // The source operand lowers into the real flow (ahead of the
        // capture) so the declaration's argument is a live kernel value.
        let source_operand = self.lower_expr_value(source)?;
        let source_operand =
            self.materialize_operand_place(source_operand, MirType::semantic(source_ty), source.span);

        // Per-element binders: coordinates (numerus, one per axis) and the
        // term binder (the element type). They live as real locals so the
        // captured term-body statements resolve against them.
        let numerus_ty = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let mut saved_bindings: Vec<(DefId, Option<LocalBinding>)> = Vec::new();
        let mut coordinate_facts = Vec::with_capacity(coordinates.len());
        for (def_id, name) in coordinates {
            let local = self.next_local_id();
            self.locals.push(MirLocalDecl {
                id: local,
                name: Some(*name),
                ty: numerus_ty,
                mutable: false,
                span: expr.span,
            });
            saved_bindings.push((*def_id, self.bindings.insert(*def_id, LocalBinding { local, ty: numerus_ty })));
            coordinate_facts.push(MirFilumCoordinate { local, name: Some(*name) });
        }
        let binder_local = self.next_local_id();
        let binder_ty = MirType::semantic(element_ty);
        self.locals.push(MirLocalDecl {
            id: binder_local,
            name: Some(binding.1),
            ty: binder_ty,
            mutable: false,
            span: expr.span,
        });
        saved_bindings.push((
            binding.0,
            self.bindings.insert(binding.0, LocalBinding { local: binder_local, ty: binder_ty }),
        ));
        let term_result = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: term_result,
            name: None,
            ty: term_mir_ty,
            mutable: true,
            span: expr.span,
        });

        // Capture the term body into scratch blocks: the redde arm stores
        // the term value into `term_result` and jumps to the capture exit.
        let blocks_mark = self.blocks.len();
        let saved_current = self.current;
        let capture_entry = self.fresh_block(expr.span);
        let capture_exit = self.fresh_block(expr.span);
        self.current = Some(capture_entry);
        self.breakables.push(BreakableContext {
            perge_target: Some(capture_exit),
            rumpe_target: capture_exit,
            rumpe_used: false,
        });
        let previous_capture =
            self.filum_capture.replace(SummaFoldContext { accumulator: term_result, ty: term_mir_ty });
        self.visit_block(body);
        self.filum_capture = previous_capture;
        self.breakables.pop();
        if !self.blocks[blocks_mark..].iter().any(|block| block.terminator.is_some()) {
            self.errors.push(MirError::unsupported(
                expr.span,
                "filum summa term body without a valued redde",
            ));
        }
        let term_body: Vec<MirStatement> = self.blocks[blocks_mark..]
            .iter()
            .flat_map(|block| block.statements.iter().cloned())
            .collect();
        self.blocks.truncate(blocks_mark);
        self.current = saved_current;
        for (def_id, binding) in saved_bindings {
            match binding {
                Some(previous) => {
                    self.bindings.insert(def_id, previous);
                }
                None => {
                    self.bindings.remove(&def_id);
                }
            }
        }

        let declaration = MirFilumSumOp {
            filum_binder: Some(filum.1),
            term_binder: binder_local,
            term_binder_name: Some(binding.1),
            coordinates: coordinate_facts,
            lane_width: Self::FILUM_LANE_WIDTH,
            term_body,
            term_result,
        };
        let call = self.runtime_call_value(
            MirIntrinsic::FilumSum(declaration),
            vec![MirOperand::Place(source_operand)],
            term_mir_ty,
            expr.span,
        );
        let result = self.materialize_operand_place(call, term_mir_ty, expr.span);
        Some(MirOperand::Place(result))
    }

    /// Lower the literal `elige` subset into a MIR switch.
    ///
    /// HIR represents `elige` as `Discerne` with one scrutinee and literal
    /// patterns. Destructuring, enum variants, guards, multi-subject matching,
    /// and value-producing matches remain explicit later-phase work.
    pub(super) fn lower_discerne_expr(
        &mut self,
        scrutinees: &[HirExpression],
        arms: &[HirCasuArm],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        if !self.is_vacuum(ty) {
            self.errors.push(MirError::unsupported(
                expr.span,
                "value-producing discerne before switch MIR lowering",
            ));
            return None;
        }
        let [scrutinee] = scrutinees else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "multi-subject discerne before switch MIR lowering",
            ));
            return None;
        };

        let value = self.lower_expr_value(scrutinee)?;
        let join_id = self.fresh_block(expr.span);
        let mut cases = Vec::new();
        let mut bodies = Vec::new();
        let mut default_id = join_id;
        let mut default_body = None;

        for arm in arms {
            if arm.guard.is_some() {
                self.errors
                    .push(MirError::unsupported(arm.span, "guarded discerne before switch MIR lowering"));
                return None;
            }
            let [pattern] = arm.patterns.as_slice() else {
                self.errors.push(MirError::unsupported(
                    arm.span,
                    "multi-pattern discerne arm before switch MIR lowering",
                ));
                return None;
            };
            match pattern {
                HirPattern::Literal(literal) => {
                    let constant = self.literal_switch_constant(literal, arm.span)?;
                    let target = self.fresh_block(arm.span);
                    cases.push(MirSwitchCase { value: constant, target });
                    bodies.push((target, &arm.body, arm.span));
                }
                HirPattern::Wildcard if default_body.is_none() => {
                    default_id = self.fresh_block(arm.span);
                    default_body = Some((&arm.body, arm.span));
                }
                HirPattern::Wildcard => {
                    self.errors.push(MirError::unsupported(
                        arm.span,
                        "multiple discerne defaults before switch MIR lowering",
                    ));
                    return None;
                }
                _ => {
                    return self.lower_unit_variant_discerne(value, scrutinee, arms, expr);
                }
            }
        }

        // A discerne whose arms are all wildcards produces a switch with no
        // literal cases; MIR validation rejects empty switches. Fold it into a
        // direct jump to the default (the only arm body), skipping the switch.
        if cases.is_empty() {
            let (body, span) = default_body?;
            self.terminate_current(MirTerminatorKind::Goto(default_id), expr.span);
            self.switch_to(default_id);
            self.lower_expr_value(body);
            let reaches_join = self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);
            if reaches_join {
                self.switch_to(join_id);
            } else {
                self.seal_unreachable(join_id, expr.span);
            }
            return Some(MirOperand::Constant(MirConstant::Unit));
        }

        self.terminate_current(MirTerminatorKind::Switch { value, cases, default: default_id }, expr.span);

        let mut reaches_join = default_body.is_none();

        for (target, body, span) in bodies {
            self.switch_to(target);
            self.lower_expr_value(body);
            reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);
        }

        if let Some((body, span)) = default_body {
            self.switch_to(default_id);
            self.lower_expr_value(body);
            reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);
        }

        if reaches_join {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, expr.span);
        }
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    fn lower_unit_variant_discerne(
        &mut self,
        value: MirOperand,
        scrutinee: &HirExpression,
        arms: &[HirCasuArm],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let scrutinee_ty = self.expr_ty(scrutinee)?;
        let bool_ty = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let scrutinee_place = self.materialize_operand_place(value.clone(), scrutinee_ty, scrutinee.span);
        let join_id = self.fresh_block(expr.span);
        let mut default_body = None;
        let mut reaches_join = false;

        for arm in arms {
            if arm.guard.is_some() {
                self.errors
                    .push(MirError::unsupported(arm.span, "guarded discerne before switch MIR lowering"));
                return None;
            }
            let [pattern] = arm.patterns.as_slice() else {
                self.errors.push(MirError::unsupported(
                    arm.span,
                    "multi-pattern discerne arm before switch MIR lowering",
                ));
                return None;
            };
            match pattern {
                HirPattern::Variant(variant, fields) => {
                    let then_id = self.fresh_block(arm.span);
                    let else_id = self.fresh_block(arm.span);
                    let fields = if fields.is_empty() {
                        MirAggregateFields::Ordered(Vec::new())
                    } else {
                        self.variant_pattern_probe_fields(*variant, &scrutinee_place, arm.span)?
                    };
                    let variant =
                        self.construct_temp(MirAggregateKind::EnumVariant(*variant), fields, scrutinee_ty, arm.span);
                    let condition = self.assign_temp(
                        MirValueKind::Binary { op: MirBinOp::Eq, lhs: value.clone(), rhs: variant },
                        bool_ty,
                        arm.span,
                    );
                    self.terminate_current(
                        MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_id },
                        arm.span,
                    );

                    self.switch_to(then_id);
                    self.bind_variant_payload(pattern, &scrutinee_place, scrutinee_ty, arm.span)?;
                    self.lower_expr_value(&arm.body);
                    reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), arm.span);

                    self.switch_to(else_id);
                }
                HirPattern::Alias(_, _, inner) if matches!(inner.as_ref(), HirPattern::Variant(_, _)) => {
                    let then_id = self.fresh_block(arm.span);
                    let else_id = self.fresh_block(arm.span);
                    let HirPattern::Alias(_, _, inner) = pattern else {
                        unreachable!("alias pattern shape was matched");
                    };
                    let HirPattern::Variant(variant, fields) = inner.as_ref() else {
                        unreachable!("inner variant pattern shape was matched");
                    };
                    let fields = if fields.is_empty() {
                        MirAggregateFields::Ordered(Vec::new())
                    } else {
                        self.variant_pattern_probe_fields(*variant, &scrutinee_place, arm.span)?
                    };
                    let variant =
                        self.construct_temp(MirAggregateKind::EnumVariant(*variant), fields, scrutinee_ty, arm.span);
                    let condition = self.assign_temp(
                        MirValueKind::Binary { op: MirBinOp::Eq, lhs: value.clone(), rhs: variant },
                        bool_ty,
                        arm.span,
                    );
                    self.terminate_current(
                        MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_id },
                        arm.span,
                    );

                    self.switch_to(then_id);
                    self.bind_variant_payload(pattern, &scrutinee_place, scrutinee_ty, arm.span)?;
                    self.lower_expr_value(&arm.body);
                    reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), arm.span);

                    self.switch_to(else_id);
                }
                HirPattern::Wildcard if default_body.is_none() => {
                    default_body = Some((&arm.body, arm.span));
                    break;
                }
                HirPattern::Wildcard => {
                    self.errors.push(MirError::unsupported(
                        arm.span,
                        "multiple discerne defaults before switch MIR lowering",
                    ));
                    return None;
                }
                _ => {
                    self.errors.push(MirError::unsupported(
                        arm.span,
                        "non-literal discerne pattern before switch MIR lowering",
                    ));
                    return None;
                }
            }
        }

        if let Some((body, span)) = default_body {
            self.lower_expr_value(body);
            reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);
        } else if let Some(current) = self.current {
            self.seal_unreachable(current, expr.span);
        }

        if reaches_join {
            self.switch_to(join_id);
            Some(MirOperand::Constant(MirConstant::Unit))
        } else {
            self.seal_unreachable(join_id, expr.span);
            None
        }
    }

    fn variant_pattern_probe_fields(
        &mut self,
        variant: DefId,
        scrutinee: &MirPlace,
        span: Span,
    ) -> Option<MirAggregateFields> {
        let Some(field_names) = self.context.variant_fields.get(&variant).cloned() else {
            self.errors
                .push(MirError::unsupported(span, "variant payload pattern is missing field metadata"));
            return None;
        };

        Some(MirAggregateFields::Named(
            field_names
                .into_iter()
                .map(|field| {
                    let mut place = scrutinee.clone();
                    place
                        .projections
                        .push(MirProjection::VariantField { variant, field });
                    MirNamedOperand { name: field, value: MirOperand::Place(place) }
                })
                .collect(),
        ))
    }

    fn bind_variant_payload(
        &mut self,
        pattern: &HirPattern,
        scrutinee: &MirPlace,
        scrutinee_ty: MirType,
        span: Span,
    ) -> Option<()> {
        match pattern {
            HirPattern::Variant(variant, fields) => {
                self.bind_variant_fields(*variant, fields, scrutinee, scrutinee_ty, span)
            }
            HirPattern::Alias(alias_def, alias_name, inner) => {
                let alias = self.next_local_id();
                self.locals.push(MirLocalDecl {
                    id: alias,
                    name: Some(*alias_name),
                    ty: scrutinee_ty,
                    mutable: false,
                    span,
                });
                self.bindings
                    .insert(*alias_def, LocalBinding { local: alias, ty: scrutinee_ty });
                self.assign(MirPlace::local(alias), MirOperand::Place(scrutinee.clone()), scrutinee_ty, span);
                self.bind_variant_payload(inner, scrutinee, scrutinee_ty, span)
            }
            _ => {
                self.errors
                    .push(MirError::unsupported(span, "non-variant discerne payload binding"));
                None
            }
        }
    }

    fn bind_variant_fields(
        &mut self,
        variant: DefId,
        fields: &[HirPattern],
        scrutinee: &MirPlace,
        scrutinee_ty: MirType,
        span: Span,
    ) -> Option<()> {
        let Some(field_names) = self.context.variant_fields.get(&variant).cloned() else {
            self.errors
                .push(MirError::unsupported(span, "variant payload pattern is missing field metadata"));
            return None;
        };
        let Some(field_tys) = self.context.variant_field_tys.get(&variant).cloned() else {
            self.errors
                .push(MirError::missing_type(span, "variant payload pattern fields"));
            return None;
        };
        if fields.len() != field_names.len() {
            self.errors
                .push(MirError::unsupported(span, "variant payload pattern arity mismatch"));
            return None;
        }

        for (field, subpattern) in field_names.into_iter().zip(fields) {
            let field_ty = field_tys
                .get(&field)
                .copied()
                .and_then(|ty| self.instantiate_variant_field_ty(variant, ty, scrutinee_ty, span));
            match subpattern {
                HirPattern::Binding(def_id, name) => {
                    self.bind_variant_field_payload(*def_id, *name, variant, field, scrutinee, field_ty, span)?;
                }
                HirPattern::Alias(def_id, name, _) => {
                    self.bind_variant_field_payload(*def_id, *name, variant, field, scrutinee, field_ty, span)?;
                }
                HirPattern::Wildcard => {}
                _ => {
                    self.errors.push(MirError::unsupported(
                        span,
                        "nested variant payload patterns before MIR lowering",
                    ));
                    return None;
                }
            }
        }
        Some(())
    }

    fn instantiate_variant_field_ty(
        &mut self,
        variant: DefId,
        field_ty: MirType,
        scrutinee_ty: MirType,
        span: Span,
    ) -> Option<MirType> {
        if !type_contains_param(self.types, field_ty.semantic_id()) {
            return Some(field_ty);
        }
        let parent = *self.context.variant_parents.get(&variant)?;
        let params = self.context.enum_type_params.get(&parent)?;
        let Type::Applied(base, args) = self.types.get(scrutinee_ty.semantic_id()) else {
            return Some(field_ty);
        };
        if !matches!(self.types.get(*base), Type::Enum(def_id) if *def_id == parent) || params.len() != args.len() {
            return Some(field_ty);
        }
        let bindings = params
            .iter()
            .copied()
            .zip(args.iter().copied().map(MirType::semantic))
            .collect::<MirGenericBindings>();
        instantiate_existing_generic_type(self.types, field_ty, &bindings, span)
            .map_err(|error| {
                self.errors
                    .push(MirError::validation(error.span, error.message));
            })
            .ok()
    }

    fn bind_variant_field_payload(
        &mut self,
        def_id: DefId,
        name: Symbol,
        variant: DefId,
        field: Symbol,
        scrutinee: &MirPlace,
        ty: Option<MirType>,
        span: Span,
    ) -> Option<()> {
        let Some(ty) = ty else {
            self.errors
                .push(MirError::missing_type(span, "variant payload pattern field"));
            return None;
        };
        let local = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: local, name: Some(name), ty, mutable: false, span });
        self.bindings.insert(def_id, LocalBinding { local, ty });
        let mut place = scrutinee.clone();
        place
            .projections
            .push(MirProjection::VariantField { variant, field });
        self.assign(MirPlace::local(local), MirOperand::Place(place), ty, span);
        Some(())
    }

    fn literal_switch_constant(&mut self, literal: &HirLiteral, span: Span) -> Option<MirConstant> {
        match literal {
            HirLiteral::Int(value) => match i64::try_from(*value) {
                Ok(value) => Some(MirConstant::Int(value)),
                // Switch patterns on unsigned scrutinees may legitimately reach
                // the upper half of u64; carry them exactly as `UInt`.
                Err(_) => Some(MirConstant::UInt(*value)),
            },
            HirLiteral::Float(value) => Some(MirConstant::Float(*value)),
            HirLiteral::String(symbol) => Some(MirConstant::String(*symbol)),
            HirLiteral::Ascii(symbol) => Some(MirConstant::Ascii(*symbol)),
            HirLiteral::Bool(value) => Some(MirConstant::Bool(*value)),
            HirLiteral::Nil => Some(MirConstant::Nil),
            HirLiteral::Octeti(_) | HirLiteral::JsonValor(_) | HirLiteral::Regex(_, _) => {
                self.errors
                    .push(MirError::unsupported(span, "literal pattern before switch MIR lowering"));
                None
            }
        }
    }

    /// Lower a block into a destination place, preserving early terminators.
    ///
    /// Statement prefixes are emitted first. If they leave the current block
    /// sealed, no tail assignment is generated; otherwise the tail expression
    /// supplies the destination value.
    pub(super) fn lower_block_to_destination(
        &mut self,
        block: &HirBlock,
        destination: MirPlace,
        ty: MirType,
        span: Span,
    ) -> Option<()> {
        if block.breakable.is_some() {
            self.lower_breakable_block(block, block.span);
            if !self.current_is_open() {
                return Some(());
            }
            if self.is_vacuum(ty) {
                self.assign(destination, MirOperand::Constant(MirConstant::Unit), ty, span);
                return Some(());
            }
            self.errors.push(MirError::unsupported(
                block.span,
                "breakable block with value destination requires redde or a tail expression after epilogue",
            ));
            return None;
        }

        for stmt in &block.statements {
            if !self.current_is_open() {
                return Some(());
            }
            self.visit_stmt(stmt);
        }

        if !self.current_is_open() {
            return Some(());
        }

        let Some(expr) = &block.expr else {
            if self.is_vacuum(ty) {
                self.assign(destination, MirOperand::Constant(MirConstant::Unit), ty, span);
                return Some(());
            }
            self.errors.push(MirError::unsupported(
                block.span,
                "expression-valued block without a tail expression",
            ));
            return None;
        };

        self.lower_expr_to_destination(expr, destination, ty)
    }

    /// Lower `si` when its result is used only for control effects.
    ///
    /// The helper creates an explicit branch and an explicit join even when no
    /// `secus` block exists; the missing else arm simply reaches the join.
    pub(super) fn lower_si_stmt(
        &mut self,
        cond: &HirExpression,
        then_block: &HirBlock,
        then_catch: Option<&HirCape>,
        else_block: Option<&HirBlock>,
        span: Span,
    ) {
        if let Some(catch) = then_catch {
            self.lower_handled_si_stmt(cond, then_block, catch, else_block, span);
            return;
        }

        let Some(condition) = self.lower_expr_value(cond) else {
            return;
        };

        let then_id = self.fresh_block(then_block.span);
        let (else_id, join_id) = if let Some(block) = else_block {
            let else_id = self.fresh_block(block.span);
            let join_id = self.fresh_block(span);
            (else_id, join_id)
        } else {
            let join_id = self.fresh_block(span);
            (join_id, join_id)
        };

        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_id },
            span,
        );

        self.switch_to(then_id);
        self.visit_block(then_block);
        let then_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);

        let else_reaches = if let Some(block) = else_block {
            self.switch_to(else_id);
            self.visit_block(block);
            self.terminate_open_current(MirTerminatorKind::Goto(join_id), span)
        } else {
            true
        };

        if then_reaches || else_reaches {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, span);
        }
    }

    fn lower_handled_si_stmt(
        &mut self,
        cond: &HirExpression,
        then_block: &HirBlock,
        catch: &HirCape,
        else_block: Option<&HirBlock>,
        span: Span,
    ) {
        let Some(error_ty) = catch.binding_ty.map(MirType::semantic) else {
            self.errors
                .push(MirError::missing_type(catch.span, "cape handler binding"));
            return;
        };

        let then_id = self.fresh_block(then_block.span);
        let (else_id, join_id) = if let Some(block) = else_block {
            let else_id = self.fresh_block(block.span);
            let join_id = self.fresh_block(span);
            (else_id, join_id)
        } else {
            let join_id = self.fresh_block(span);
            (join_id, join_id)
        };
        let handler_id = self.fresh_block(catch.body.span);

        let handler_local = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: handler_local,
            name: Some(catch.binding_name),
            ty: error_ty,
            mutable: false,
            span: catch.span,
        });
        self.bindings
            .insert(catch.binding_def_id, LocalBinding { local: handler_local, ty: error_ty });

        self.handlers.push(HandlerContext {
            error_place: MirPlace::local(handler_local),
            error_ty,
            error_block: handler_id,
        });
        let Some(condition) = self.lower_expr_value(cond) else {
            self.handlers.pop();
            self.seal_unreachable(handler_id, catch.span);
            self.switch_to(join_id);
            return;
        };

        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_id },
            span,
        );

        self.switch_to(then_id);
        self.visit_block(then_block);
        self.handlers.pop();
        let then_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);

        self.switch_to(handler_id);
        self.visit_block(&catch.body);
        let handler_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), catch.span);

        let else_reaches = if let Some(block) = else_block {
            self.switch_to(else_id);
            self.visit_block(block);
            self.terminate_open_current(MirTerminatorKind::Goto(join_id), span)
        } else {
            true
        };

        if then_reaches || handler_reaches || else_reaches {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, span);
        }
    }

    /// Lower expression-valued `si` into an existing destination.
    ///
    /// A missing `secus` branch is rejected because MIR has no implicit default
    /// value for an expression result.
    pub(super) fn lower_si_to_destination(
        &mut self,
        cond: &HirExpression,
        then_block: &HirBlock,
        else_block: Option<&HirBlock>,
        destination: MirPlace,
        ty: MirType,
        span: Span,
    ) -> Option<()> {
        let Some(else_block) = else_block else {
            self.errors
                .push(MirError::unsupported(span, "expression-valued si without secus destination"));
            return None;
        };

        let condition = self.lower_expr_value(cond)?;
        let then_id = self.fresh_block(then_block.span);
        let else_id = self.fresh_block(else_block.span);
        let join_id = self.fresh_block(span);

        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_id },
            span,
        );

        self.switch_to(then_id);
        self.lower_block_to_destination(then_block, destination.clone(), ty, then_block.span)?;
        let then_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);

        self.switch_to(else_id);
        self.lower_block_to_destination(else_block, destination, ty, else_block.span)?;
        let else_reaches = self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);

        if then_reaches || else_reaches {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, span);
        }

        Some(())
    }

    /// Lower an unconditional `Loop` block (`fac { } dum cond` body) into a
    /// self-jumping body block plus an after block.
    ///
    /// `rumpe` jumps to the after block through the loop context pushed while
    /// lowering the body; the body block unconditionally jumps back to itself.
    pub(super) fn lower_loop_expr(&mut self, block: &HirBlock, expr: &HirExpression) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        if !self.is_vacuum(ty) {
            self.errors
                .push(MirError::unsupported(expr.span, "loop expression with non-vacuum result"));
            return None;
        }

        let body_id = self.fresh_block(block.span);
        let after_id = self.fresh_block(expr.span);

        self.terminate_current(MirTerminatorKind::Goto(body_id), expr.span);
        self.breakables.push(BreakableContext {
            perge_target: Some(body_id),
            rumpe_target: after_id,
            rumpe_used: false,
        });
        self.switch_to(body_id);
        self.visit_block(block);
        self.breakables.pop();
        self.terminate_open_current(MirTerminatorKind::Goto(body_id), expr.span);
        self.switch_to(after_id);
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    /// Lower `dum` into condition, body, and after blocks.
    ///
    /// `perge` jumps back to the condition block and `rumpe` jumps to the after
    /// block through the loop context pushed while lowering the body.
    pub(super) fn lower_dum(&mut self, cond: &HirExpression, body: &HirBlock, span: Span) {
        let cond_id = self.fresh_block(cond.span);
        let body_id = self.fresh_block(body.span);
        let after_id = self.fresh_block(span);

        self.terminate_current(MirTerminatorKind::Goto(cond_id), span);

        self.switch_to(cond_id);
        let Some(condition) = self.lower_expr_value(cond) else {
            self.seal_unreachable(cond_id, cond.span);
            self.switch_to(after_id);
            return;
        };
        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: body_id, else_block: after_id },
            cond.span,
        );

        self.breakables.push(BreakableContext {
            perge_target: Some(cond_id),
            rumpe_target: after_id,
            rumpe_used: false,
        });
        self.switch_to(body_id);
        self.visit_block(body);
        self.breakables.pop();
        self.terminate_open_current(MirTerminatorKind::Goto(cond_id), span);

        self.switch_to(after_id);
    }

    fn lower_range_itera(&mut self, range: RangeIteraLowering<'_>) -> Option<()> {
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));

        let index = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: index,
            name: Some(range.name),
            ty: numerus,
            mutable: true,
            span: range.span,
        });
        self.bindings
            .insert(range.binding, LocalBinding { local: index, ty: numerus });

        let end_local = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: end_local, name: None, ty: numerus, mutable: false, span: range.span });
        let step_local = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: step_local, name: None, ty: numerus, mutable: false, span: range.span });

        self.lower_expr_to_destination(range.start, MirPlace::local(index), numerus)?;
        self.lower_expr_to_destination(range.end, MirPlace::local(end_local), numerus)?;
        if !self.current_is_open() {
            return None;
        }

        let cond_id = self.fresh_block(range.span);
        match range.step {
            Some(step) => {
                self.lower_expr_to_destination(step, MirPlace::local(step_local), numerus)?;
                self.terminate_open_current(MirTerminatorKind::Goto(cond_id), range.span);
            }
            None => self.lower_default_range_step(index, end_local, step_local, cond_id, bivalens, range.span),
        }

        let positive_check_id = self.fresh_block(range.span);
        let negative_or_zero_id = self.fresh_block(range.span);
        let negative_check_id = self.fresh_block(range.span);
        let body_id = self.fresh_block(range.body.span);
        let increment_id = self.fresh_block(range.span);
        let after_id = self.fresh_block(range.span);

        self.switch_to(cond_id);
        let step_is_positive = self.binary_temp(
            MirBinOp::Gt,
            MirOperand::Place(MirPlace::local(step_local)),
            MirOperand::Constant(MirConstant::Int(0)),
            bivalens,
            range.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch {
                condition: step_is_positive,
                then_block: positive_check_id,
                else_block: negative_or_zero_id,
            },
            range.span,
        );

        self.switch_to(positive_check_id);
        let positive_op = match range.kind {
            HirRangeKind::Exclusive => MirBinOp::Lt,
            HirRangeKind::Inclusive => MirBinOp::LtEq,
        };
        let positive_continues = self.binary_temp(
            positive_op,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Place(MirPlace::local(end_local)),
            bivalens,
            range.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: positive_continues, then_block: body_id, else_block: after_id },
            range.span,
        );

        self.switch_to(negative_or_zero_id);
        let step_is_negative = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(step_local)),
            MirOperand::Constant(MirConstant::Int(0)),
            bivalens,
            range.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch {
                condition: step_is_negative,
                then_block: negative_check_id,
                else_block: after_id,
            },
            range.span,
        );

        self.switch_to(negative_check_id);
        let negative_op = match range.kind {
            HirRangeKind::Exclusive => MirBinOp::Gt,
            HirRangeKind::Inclusive => MirBinOp::GtEq,
        };
        let negative_continues = self.binary_temp(
            negative_op,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Place(MirPlace::local(end_local)),
            bivalens,
            range.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: negative_continues, then_block: body_id, else_block: after_id },
            range.span,
        );

        self.breakables.push(BreakableContext {
            perge_target: Some(increment_id),
            rumpe_target: after_id,
            rumpe_used: false,
        });
        self.switch_to(body_id);
        self.visit_block(range.body);
        self.breakables.pop();
        self.terminate_open_current(MirTerminatorKind::Goto(increment_id), range.span);

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Place(MirPlace::local(step_local)),
            numerus,
            range.span,
        );
        self.assign(MirPlace::local(index), next, numerus, range.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), range.span);

        self.switch_to(after_id);
        Some(())
    }

    fn lower_array_itera(&mut self, iter: ArrayIteraLowering<'_>) -> Option<()> {
        let Some(iter_ty) = iter.iter.ty else {
            self.errors
                .push(MirError::missing_type(iter.iter.span, "itera collection source"));
            return None;
        };
        // Classify inside a scope so the `&Type` borrow of the type table ends
        // before the builder mutates itself below.
        let (collection_kind, tensor_lanes) = {
            let normalized = self.normalized_type(iter_ty);
            let has_coordinates = !iter.coordinates.is_empty();
            let kind = match normalized {
                Type::Tensor(elem, shape) => {
                    let dimensions = i64s_from_literal_index(*shape, self.types);
                    if matches!(iter.mode, HirIteraMode::Ex)
                        && has_coordinates
                        && dimensions
                            .as_ref()
                            .is_some_and(|dims| dims.len() == iter.coordinates.len())
                    {
                        CollectionIteraKind::Shaped {
                            element: *elem,
                            dimensions: dimensions.expect("literal tensor dimensions checked above"),
                            matrix: false,
                        }
                    } else {
                        // The coordinate-free rank-1 route remains unchanged;
                        // rank-N shaped tensors use the unified shaped arm.
                        let rank1_literal = dimensions.as_ref().is_some_and(|dims| dims.len() == 1);
                        if matches!(iter.mode, HirIteraMode::Ex) && !has_coordinates && rank1_literal {
                            CollectionIteraKind::Tensor(*elem)
                        } else {
                            CollectionIteraKind::Unsupported
                        }
                    }
                }
                Type::Matrix(elem, shape) => {
                    let dimensions = i64s_from_literal_index(*shape, self.types);
                    let literal_matrix = dimensions
                        .as_ref()
                        .is_some_and(|dims| dims.len() == 2 && dims.iter().all(|dim| u8::try_from(*dim).is_ok()));
                    if matches!(iter.mode, HirIteraMode::Ex)
                        && has_coordinates
                        && iter.coordinates.len() == 2
                        && literal_matrix
                    {
                        CollectionIteraKind::Shaped {
                            element: *elem,
                            dimensions: dimensions.expect("literal matrix dimensions checked above"),
                            matrix: true,
                        }
                    } else {
                        CollectionIteraKind::Unsupported
                    }
                }
                // Route B (AEU-4): runtime-len `lista<T, N>` lowers in the base
                // lane to a static split on the carrier `len` with N+1
                // guard-free arms (total N(N+1)/2 lanes). The N <= 10
                // code-size cap keeps the split bounded; non-literal capacities
                // (shape-generics) and N > 10 fall through to Unsupported.
                Type::BoundedArray { element, capacity } => {
                    let literal_n = if matches!(iter.mode, HirIteraMode::Ex) {
                        i64s_from_literal_index(*capacity, self.types)
                            .and_then(|dims| dims.first().copied())
                            .filter(|n| (0..=10).contains(n))
                    } else {
                        None
                    };
                    match literal_n {
                        Some(n) => CollectionIteraKind::BoundedArray(*element, n),
                        None => CollectionIteraKind::Unsupported,
                    }
                }
                Type::Array(item) => CollectionIteraKind::Array(*item),
                Type::Map(key, _) if matches!(iter.mode, HirIteraMode::De) => CollectionIteraKind::MapKeys(*key),
                Type::Map(_, value) if matches!(iter.mode, HirIteraMode::Ex) => CollectionIteraKind::MapValues(*value),
                Type::Cursor(value) if matches!(iter.mode, HirIteraMode::Ex) => CollectionIteraKind::Cursor(*value),
                Type::AsyncCursor(value, _) if matches!(iter.mode, HirIteraMode::Ex) => {
                    CollectionIteraKind::AsyncCursor(*value)
                }
                _ => CollectionIteraKind::Unsupported,
            };
            // Static bound: the coordinate-free tensor loop runs the literal N
            // lanes — no runtime length intrinsic on that tensor carrier.
            let lanes = if matches!(&kind, CollectionIteraKind::Tensor(_)) {
                let Type::Tensor(_, shape) = normalized else {
                    unreachable!("tensor itera kind was classified from a tensor type")
                };
                i64s_from_literal_index(*shape, self.types)
                    .and_then(|dims| dims.first().copied())
                    .unwrap_or(0)
            } else {
                0
            };
            (kind, lanes)
        };
        if matches!(&collection_kind, CollectionIteraKind::Unsupported) {
            // Only map-value iteration carries the tabula decline; tensor and
            // bounded-lista declines keep the generic iterator diagnostic.
            let is_map = matches!(self.normalized_type(iter_ty), Type::Map(_, _));
            self.errors.push(MirError::unsupported(
                iter.iter.span,
                if matches!(iter.mode, HirIteraMode::Ex) && is_map {
                    "itera ex over tabula before map value iteration MIR lowering"
                } else {
                    "itera collection before iterator MIR lowering"
                },
            ));
            return None;
        }

        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let collection = self.lower_expr_value(iter.iter)?;
        let (collection, collection_ty) = match &collection_kind {
            CollectionIteraKind::Cursor(element) | CollectionIteraKind::AsyncCursor(element) => {
                let array = self.types.find_array(*element)?;
                let array_ty = MirType::semantic(array);
                let snapshot = self.runtime_call_value(
                    MirIntrinsic::Sermo(crate::mir::MirSermoOp::DrainCursor),
                    vec![collection],
                    array_ty,
                    iter.iter.span,
                );
                (snapshot, array_ty)
            }
            _ => (collection, MirType::semantic(iter_ty)),
        };
        let collection = self.materialize_operand_place(collection, collection_ty, iter.iter.span);
        if let CollectionIteraKind::Shaped { element, dimensions, matrix } = &collection_kind {
            return self.lower_shaped_itera(&iter, collection, *element, dimensions, *matrix);
        }

        let (iteration_place, binding_ty, array_de) = match &collection_kind {
            CollectionIteraKind::Array(item) => {
                let item_ty = MirType::semantic(*item);
                let binding_ty = if matches!(iter.mode, HirIteraMode::De) {
                    numerus
                } else {
                    item_ty
                };
                (collection, binding_ty, matches!(iter.mode, HirIteraMode::De))
            }
            CollectionIteraKind::MapKeys(key) => {
                let (snapshot_place, binding_ty) =
                    self.lower_map_itera_snapshot(collection, *key, MirCollectionOp::Keys, iter.iter.span)?;
                (snapshot_place, binding_ty, false)
            }
            CollectionIteraKind::MapValues(value) => {
                let (snapshot_place, binding_ty) =
                    self.lower_map_itera_snapshot(collection, *value, MirCollectionOp::Values, iter.iter.span)?;
                (snapshot_place, binding_ty, false)
            }
            CollectionIteraKind::Cursor(value) | CollectionIteraKind::AsyncCursor(value) => {
                (collection, MirType::semantic(*value), false)
            }
            CollectionIteraKind::Tensor(elem) => {
                // Element gather rides the generic Index projection over the
                // tensor place (every lane is defined by construction).
                (collection, MirType::semantic(*elem), false)
            }
            CollectionIteraKind::BoundedArray(elem, _) => (collection, MirType::semantic(*elem), false),
            CollectionIteraKind::Shaped { .. } => {
                unreachable!("shaped collection itera returns through nested lowering above")
            }
            CollectionIteraKind::Unsupported => {
                unreachable!("unsupported collection itera kinds fail closed above")
            }
        };

        if let CollectionIteraKind::BoundedArray(_, capacity) = &collection_kind {
            return self.lower_bounded_array_itera_split(&iter, iteration_place, binding_ty, *capacity);
        }

        let index = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: index, name: None, ty: numerus, mutable: true, span: iter.span });
        self.assign(
            MirPlace::local(index),
            MirOperand::Constant(MirConstant::Int(0)),
            numerus,
            iter.span,
        );

        let length = if matches!(&collection_kind, CollectionIteraKind::Tensor(_)) {
            MirOperand::Constant(MirConstant::Int(tensor_lanes))
        } else {
            self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Length),
                vec![MirOperand::Place(iteration_place.clone())],
                numerus,
                iter.span,
            )
        };
        if !self.current_is_open() {
            return None;
        }

        let binding = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: binding,
            name: Some(iter.name),
            ty: binding_ty,
            mutable: true,
            span: iter.span,
        });
        self.bindings
            .insert(iter.binding, LocalBinding { local: binding, ty: binding_ty });

        let cond_id = self.fresh_block(iter.span);
        let body_id = self.fresh_block(iter.body.span);
        let increment_id = self.fresh_block(iter.span);
        let after_id = self.fresh_block(iter.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), iter.span);

        self.switch_to(cond_id);
        let condition = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(index)),
            length,
            bivalens,
            iter.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: body_id, else_block: after_id },
            iter.span,
        );

        self.switch_to(body_id);
        let value = if array_de {
            MirOperand::Place(MirPlace::local(index))
        } else {
            let mut indexed = iteration_place;
            indexed
                .projections
                .push(MirProjection::Index(MirOperand::Place(MirPlace::local(index))));
            MirOperand::Place(indexed)
        };
        self.assign(MirPlace::local(binding), value, binding_ty, iter.span);
        self.breakables.push(BreakableContext {
            perge_target: Some(increment_id),
            rumpe_target: after_id,
            rumpe_used: false,
        });
        self.visit_block(iter.body);
        self.breakables.pop();
        self.terminate_open_current(MirTerminatorKind::Goto(increment_id), iter.span);

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            iter.span,
        );
        self.assign(MirPlace::local(index), next, numerus, iter.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), iter.span);

        self.switch_to(after_id);
        Some(())
    }

    /// Lower one shaped `apud` loop as rank-N nested CFG loops.
    ///
    /// Tensor element reads use the runtime `TensorGet` ABI with the current
    /// coordinate locals as its index list. Matrix values are register-class
    /// aggregates, so their dynamic coordinate pair is switched to the
    /// corresponding static `MatrixCell` projection before the shared body.
    fn lower_shaped_itera(
        &mut self,
        iter: &ArrayIteraLowering<'_>,
        collection: MirPlace,
        element: TypeId,
        dimensions: &[i64],
        matrix: bool,
    ) -> Option<()> {
        if dimensions.is_empty() || dimensions.len() != iter.coordinates.len() {
            self.errors.push(MirError::unsupported(
                iter.iter.span,
                "itera shaped collection dimensions before nested loop MIR lowering",
            ));
            return None;
        }

        let element_ty = MirType::semantic(element);
        let binding = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: binding,
            name: Some(iter.name),
            ty: element_ty,
            mutable: true,
            span: iter.span,
        });
        self.bindings
            .insert(iter.binding, LocalBinding { local: binding, ty: element_ty });

        let after_id = self.fresh_block(iter.span);
        self.lower_shaped_level(iter, collection, element, binding, dimensions, matrix, 0, after_id)?;
        self.switch_to(after_id);
        Some(())
    }

    fn lower_shaped_level(
        &mut self,
        iter: &ArrayIteraLowering<'_>,
        collection: MirPlace,
        element: TypeId,
        binding: MirLocalId,
        dimensions: &[i64],
        matrix: bool,
        level: usize,
        continuation: MirBlockId,
    ) -> Option<()> {
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let (coordinate_def, coordinate_name) = iter.coordinates[level];
        let coordinate = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: coordinate,
            name: Some(coordinate_name),
            ty: numerus,
            mutable: true,
            span: iter.span,
        });
        self.bindings
            .insert(coordinate_def, LocalBinding { local: coordinate, ty: numerus });
        self.assign(
            MirPlace::local(coordinate),
            MirOperand::Constant(MirConstant::Int(0)),
            numerus,
            iter.span,
        );

        let condition_id = self.fresh_block(iter.span);
        let body_id = self.fresh_block(iter.body.span);
        let increment_id = self.fresh_block(iter.span);
        let loop_after_id = self.fresh_block(iter.span);
        self.terminate_current(MirTerminatorKind::Goto(condition_id), iter.span);

        self.switch_to(condition_id);
        let condition = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(coordinate)),
            MirOperand::Constant(MirConstant::Int(dimensions[level])),
            MirType::semantic(self.types.primitive(Primitive::Bivalens)),
            iter.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: body_id, else_block: loop_after_id },
            iter.span,
        );

        self.switch_to(body_id);
        if level + 1 < dimensions.len() {
            self.lower_shaped_level(iter, collection, element, binding, dimensions, matrix, level + 1, increment_id)?;
        } else if matrix {
            self.lower_shaped_matrix_body(
                iter,
                collection,
                element,
                binding,
                coordinate,
                iter.coordinates[level - 1].0,
                dimensions,
                increment_id,
                loop_after_id,
            )?;
        } else {
            self.lower_shaped_tensor_body(iter, collection, element, binding, increment_id, loop_after_id)?;
        }

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(coordinate)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            iter.span,
        );
        self.assign(MirPlace::local(coordinate), next, numerus, iter.span);
        self.terminate_current(MirTerminatorKind::Goto(condition_id), iter.span);

        self.switch_to(loop_after_id);
        self.terminate_open_current(MirTerminatorKind::Goto(continuation), iter.span);
        Some(())
    }

    fn lower_shaped_tensor_body(
        &mut self,
        iter: &ArrayIteraLowering<'_>,
        collection: MirPlace,
        element: TypeId,
        binding: MirLocalId,
        increment_id: MirBlockId,
        loop_after_id: MirBlockId,
    ) -> Option<()> {
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let Some(index_array) = self.types.find_array(numerus.semantic_id()) else {
            self.errors
                .push(MirError::missing_type(iter.iter.span, "shaped itera coordinate list"));
            return None;
        };
        let coordinate_operands = iter
            .coordinates
            .iter()
            .map(|(def_id, _)| {
                let local = self
                    .bindings
                    .get(def_id)
                    .expect("shaped coordinate binding exists")
                    .local;
                MirAggregateItem::Operand(MirOperand::Place(MirPlace::local(local)))
            })
            .collect();
        let index_list = self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(coordinate_operands),
            MirType::semantic(index_array),
            iter.span,
        );
        let Some(option_ty) = self.types.find_equal(&Type::Option(element)) else {
            self.errors
                .push(MirError::missing_type(iter.iter.span, "shaped itera element option"));
            return None;
        };
        let option = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorGet),
            vec![MirOperand::Place(collection), index_list],
            MirType::semantic(option_ty),
            iter.iter.span,
        );
        if !self.current_is_open() {
            return None;
        }
        let value = self.assign_temp(
            MirValueKind::Option(MirOptionOp::Unwrap { value: option, mode: MirOptionUnwrapMode::Assume }),
            MirType::semantic(element),
            iter.span,
        );
        self.assign(MirPlace::local(binding), value, MirType::semantic(element), iter.span);
        self.breakables.push(BreakableContext {
            perge_target: Some(increment_id),
            rumpe_target: loop_after_id,
            rumpe_used: false,
        });
        self.visit_block(iter.body);
        self.breakables.pop();
        self.terminate_open_current(MirTerminatorKind::Goto(increment_id), iter.span);
        Some(())
    }

    fn lower_shaped_matrix_body(
        &mut self,
        iter: &ArrayIteraLowering<'_>,
        collection: MirPlace,
        element: TypeId,
        binding: MirLocalId,
        column: MirLocalId,
        row_def: DefId,
        dimensions: &[i64],
        increment_id: MirBlockId,
        loop_after_id: MirBlockId,
    ) -> Option<()> {
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let row = self
            .bindings
            .get(&row_def)
            .expect("matrix row coordinate binding exists")
            .local;
        let rows = u8::try_from(dimensions[0]).ok()?;
        let columns = u8::try_from(dimensions[1]).ok()?;
        let row_stride = self.binary_temp(
            MirBinOp::Mul,
            MirOperand::Place(MirPlace::local(row)),
            MirOperand::Constant(MirConstant::Int(i64::from(columns))),
            numerus,
            iter.span,
        );
        let linear = self.binary_temp(
            MirBinOp::Add,
            row_stride,
            MirOperand::Place(MirPlace::local(column)),
            numerus,
            iter.span,
        );
        let body_id = self.fresh_block(iter.body.span);
        let cases = (0..u16::from(rows) * u16::from(columns))
            .map(|offset| MirSwitchCase {
                value: MirConstant::Int(i64::from(offset)),
                target: self.fresh_block(iter.span),
            })
            .collect::<Vec<_>>();
        let cell_blocks = cases.iter().map(|case| case.target).collect::<Vec<_>>();
        self.terminate_current(
            MirTerminatorKind::Switch { value: linear, cases, default: increment_id },
            iter.span,
        );
        for (offset, cell_id) in cell_blocks.into_iter().enumerate() {
            let matrix_row = u8::try_from(offset / usize::from(columns)).ok()?;
            let matrix_column = u8::try_from(offset % usize::from(columns)).ok()?;
            self.switch_to(cell_id);
            let mut cell = collection.clone();
            cell.projections
                .push(MirProjection::MatrixCell { row: matrix_row, column: matrix_column });
            self.assign(
                MirPlace::local(binding),
                MirOperand::Place(cell),
                MirType::semantic(element),
                iter.span,
            );
            self.terminate_current(MirTerminatorKind::Goto(body_id), iter.span);
        }
        self.switch_to(body_id);
        self.breakables.push(BreakableContext {
            perge_target: Some(increment_id),
            rumpe_target: loop_after_id,
            rumpe_used: false,
        });
        self.visit_block(iter.body);
        self.breakables.pop();
        self.terminate_open_current(MirTerminatorKind::Goto(increment_id), iter.span);
        Some(())
    }

    /// Route B (AEU-4): lower `itera ex` over a runtime-len `lista<T, N>` to
    /// a base-lane static split — a switch on the carrier `len` with N+1
    /// literal arms, arm k being a guard-free unroll of k lanes over 0..k.
    ///
    /// Each arm touches only `0..k ⊆ 0..len`, so no dead slot (i ≥ len) is
    /// ever read. The branch is base-lane MIR; the AIR lane never admits the
    /// loop (`air_unroll` declines runtime-len values `not_concrete`), so
    /// reverse AD never sees a non-literal scrutinee.
    fn lower_bounded_array_itera_split(
        &mut self,
        iter: &ArrayIteraLowering<'_>,
        collection: MirPlace,
        binding_ty: MirType,
        capacity: i64,
    ) -> Option<()> {
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        // Runtime `len` read from the ListaN carrier — the split scrutinee.
        let length = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![MirOperand::Place(collection.clone())],
            numerus,
            iter.iter.span,
        );
        if !self.current_is_open() {
            return None;
        }

        let binding = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: binding,
            name: Some(iter.name),
            ty: binding_ty,
            mutable: true,
            span: iter.span,
        });
        self.bindings
            .insert(iter.binding, LocalBinding { local: binding, ty: binding_ty });

        let after_id = self.fresh_block(iter.span);
        let mut cases = Vec::new();
        for k in 0..=capacity {
            cases.push(MirSwitchCase { value: MirConstant::Int(k), target: self.fresh_block(iter.span) });
        }
        let arm_blocks = cases.iter().map(|case| case.target).collect::<Vec<_>>();
        self.terminate_current(
            MirTerminatorKind::Switch { value: length, cases, default: after_id },
            iter.iter.span,
        );

        let mut reaches_after = false;
        for (k, arm_id) in arm_blocks.into_iter().enumerate() {
            self.switch_to(arm_id);
            for lane in 0..k as i64 {
                // Arm k reads only lanes 0..k ⊆ 0..len — never a dead slot.
                let mut indexed = collection.clone();
                indexed
                    .projections
                    .push(MirProjection::Index(MirOperand::Constant(MirConstant::Int(lane))));
                self.assign(MirPlace::local(binding), MirOperand::Place(indexed), binding_ty, iter.span);
                let lane_end = self.fresh_block(iter.span);
                self.breakables.push(BreakableContext {
                    perge_target: Some(lane_end),
                    rumpe_target: after_id,
                    rumpe_used: false,
                });
                self.visit_block(iter.body);
                let ctx = self.breakables.pop().expect("breakable context balances");
                reaches_after |= ctx.rumpe_used;
                self.terminate_open_current(MirTerminatorKind::Goto(lane_end), iter.span);
                self.switch_to(lane_end);
            }
            reaches_after |= self.terminate_open_current(MirTerminatorKind::Goto(after_id), iter.span);
        }
        if reaches_after {
            self.switch_to(after_id);
        } else {
            self.seal_unreachable(after_id, iter.span);
        }
        Some(())
    }

    fn lower_map_itera_snapshot(
        &mut self,
        collection: MirPlace,
        element: TypeId,
        op: MirCollectionOp,
        span: Span,
    ) -> Option<(MirPlace, MirType)> {
        debug_assert!(matches!(op, MirCollectionOp::Keys | MirCollectionOp::Values));
        let element_ty = MirType::semantic(element);
        let array_id = self.types.find_array(element)?;
        let array_ty = MirType::semantic(array_id);
        let snapshot = self.runtime_call_value(
            MirIntrinsic::Collection(op),
            vec![MirOperand::Place(collection)],
            array_ty,
            span,
        );
        if !self.current_is_open() {
            return None;
        }
        let snapshot_place = self.materialize_operand_place(snapshot, array_ty, span);
        Some((snapshot_place, element_ty))
    }

    pub(super) fn materialize_operand_place(&mut self, operand: MirOperand, ty: MirType, span: Span) -> MirPlace {
        match operand {
            MirOperand::Place(place) => place,
            MirOperand::Temp(temp) => MirPlace::temp(temp),
            other => {
                let temp = self.push_temp(ty, span);
                self.assign(MirPlace::temp(temp), other, ty, span);
                MirPlace::temp(temp)
            }
        }
    }

    fn lower_default_range_step(
        &mut self,
        index: MirLocalId,
        end: MirLocalId,
        step: MirLocalId,
        cond_id: MirBlockId,
        bivalens: MirType,
        span: Span,
    ) {
        let positive_step_id = self.fresh_block(span);
        let negative_step_id = self.fresh_block(span);
        let ascends = self.binary_temp(
            MirBinOp::LtEq,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Place(MirPlace::local(end)),
            bivalens,
            span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch {
                condition: ascends,
                then_block: positive_step_id,
                else_block: negative_step_id,
            },
            span,
        );

        self.switch_to(positive_step_id);
        self.assign(
            MirPlace::local(step),
            MirOperand::Constant(MirConstant::Int(1)),
            MirType::semantic(self.types.primitive(Primitive::Numerus)),
            span,
        );
        self.terminate_current(MirTerminatorKind::Goto(cond_id), span);

        self.switch_to(negative_step_id);
        self.assign(
            MirPlace::local(step),
            MirOperand::Constant(MirConstant::Int(-1)),
            MirType::semantic(self.types.primitive(Primitive::Numerus)),
            span,
        );
        self.terminate_current(MirTerminatorKind::Goto(cond_id), span);
    }

    pub(super) fn binary_temp(
        &mut self,
        op: MirBinOp,
        lhs: MirOperand,
        rhs: MirOperand,
        ty: MirType,
        span: Span,
    ) -> MirOperand {
        self.assign_temp(MirValueKind::Binary { op, lhs, rhs }, ty, span)
    }

    /// Lower `rumpe` to the innermost breakable scope exit edge.
    pub(super) fn lower_rumpe(&mut self, span: Span) {
        let Some(context) = self.breakables.last_mut() else {
            self.errors
                .push(MirError::unsupported(span, "rumpe without an enclosing breakable block"));
            return;
        };
        context.rumpe_used = true;
        let target = context.rumpe_target;
        self.terminate_current(MirTerminatorKind::Goto(target), span);
    }

    /// Lower `perge` to the active loop's head edge.
    pub(super) fn lower_perge(&mut self, span: Span) {
        let Some(context) = self.breakables.last().copied() else {
            self.errors
                .push(MirError::unsupported(span, "perge without an active loop"));
            return;
        };
        let Some(perge_target) = context.perge_target else {
            self.errors
                .push(MirError::unsupported(span, "perge outside a loop"));
            return;
        };
        self.terminate_current(MirTerminatorKind::Goto(perge_target), span);
    }

    /// Lower a HIR block marked [`HirBlock::breakable`].
    pub(super) fn lower_breakable_block(&mut self, block: &HirBlock, span: Span) {
        if block.breakable.is_none() {
            self.visit_block_statements(block);
            return;
        }

        let after_id = self.fresh_block(span);
        let exit_id = after_id;

        // Lower the body into the current (predecessor) block. Body fall-through
        // and `rumpe` both goto `exit_id`. This must run before the epilogue is
        // built so the predecessor stays the active cursor and body statements
        // are not emitted into a sealed block.
        let mut context = BreakableContext { perge_target: None, rumpe_target: exit_id, rumpe_used: false };
        self.breakables.push(context);
        self.visit_block_statements(block);
        // WHY: read back the context so `rumpe_used` reflects any `rumpe` that
        // fired while lowering the body. The body may have pushed and popped
        // nested breakable scopes, so a copy taken upfront would go stale.
        context = *self.breakables.last().unwrap();
        self.breakables.pop();
        let body_reaches = self.terminate_open_current(MirTerminatorKind::Goto(exit_id), span);

        // The after-block is reachable if the body's straight-line fall-through
        // reached it, or if any `rumpe` targeted this scope's exit edge. Keep it
        // live only then; otherwise leave current sealed so the enclosing
        // builder does not lower subsequent code (or an implicit function
        // return) into a block with no predecessors. A truly dead after-block
        // is sealed by the finish pass.
        if body_reaches || context.rumpe_used {
            self.switch_to(after_id);
        }
    }

    fn visit_block_statements(&mut self, block: &HirBlock) {
        for stmt in &block.statements {
            if !self.current_is_open() {
                break;
            }
            self.visit_stmt(stmt);
        }
        if let Some(expr) = &block.expr {
            if self.current_is_open() {
                self.visit_expr(expr);
            }
        }
    }
}
