//! Higher-order collection method lowering (`omnia`, `quilibet`, `filtrata`,
//! `mappata`, `reducta`, `cumulata`).
//!
//! Collection transforms lower into explicit MIR loops over array receivers.
//! Each callback is emitted as a synthetic closure function, then invoked once
//! per element through the shared callable lowering path. Predicate folds
//! settle on the first decisive result; filtered results append only when the
//! predicate returns true; mapped results append the callback return value
//! directly; reduced results update an accumulator; cumulated results append
//! the accumulator after every fold step (serial prefix scan).
//!
//! INVARIANTS
//! ==========
//! - Only lista receivers are supported in this phase (exact-bound
//!   `BoundedArray` literals included — the loop reads length at runtime).
//! - Filter/map callbacks are single-parameter closures with non-failable
//!   signatures; reduce callbacks are two-parameter closures.
//! - Loop blocks are created before any terminator seals the current block.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;

impl FunctionBuilder<'_> {
    pub(super) fn lower_filtrata(
        &mut self,
        receiver: &HirExpression,
        closure: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let predicate = self.lower_closure_function(closure, bivalens)?;
        self.lower_collection_transform(receiver, predicate, TransformKind::Filter, expr)
    }

    /// `omnia` — left-to-right universal predicate fold over a `lista`.
    ///
    /// The initial accumulator is true, so an empty source satisfies the
    /// vacuous-truth law. A false predicate result settles the fold early.
    pub(super) fn lower_omnia(
        &mut self,
        receiver: &HirExpression,
        closure: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let predicate = self.lower_closure_function(closure, bivalens)?;
        self.lower_collection_predicate(receiver, predicate, TransformKind::All, expr)
    }

    /// `quilibet` — left-to-right existential predicate fold over a `lista`.
    ///
    /// The initial accumulator is false, so an empty source fails the fold.
    /// A true predicate result settles the fold early.
    pub(super) fn lower_quilibet(
        &mut self,
        receiver: &HirExpression,
        closure: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let predicate = self.lower_closure_function(closure, bivalens)?;
        self.lower_collection_predicate(receiver, predicate, TransformKind::Any, expr)
    }

    pub(super) fn lower_mappata(
        &mut self,
        receiver: &HirExpression,
        closure: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(receiver_ty) = receiver.ty else {
            self.errors
                .push(MirError::missing_type(receiver.span, "mappata receiver"));
            return None;
        };
        if self.collection_element_ty(receiver_ty).is_none() {
            self.errors.push(MirError::unsupported(
                receiver.span,
                "mappata receiver is not a supported collection type",
            ));
            return None;
        }
        let Some(result_ty) = expr.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "mappata result"));
            return None;
        };
        let Some(result_element_ty) = self.collection_element_ty(result_ty) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "mappata result is not a supported collection type",
            ));
            return None;
        };
        let mapper = self.lower_closure_function(closure, result_element_ty)?;
        self.lower_collection_transform(receiver, mapper, TransformKind::Map, expr)
    }

    pub(super) fn lower_reducta(
        &mut self,
        receiver: &HirExpression,
        reducer: &HirExpression,
        initial: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(receiver_ty) = receiver.ty else {
            self.errors
                .push(MirError::missing_type(receiver.span, "collection reduce receiver"));
            return None;
        };
        if !matches!(self.normalized_type(receiver_ty), Type::Array(_) | Type::BoundedArray { .. }) {
            self.errors.push(MirError::unsupported(
                receiver.span,
                "collection reduce before array MIR lowering",
            ));
            return None;
        }
        let result_ty = self.expr_ty(expr)?;
        let reducer = self.lower_reducer_function(reducer, result_ty)?;
        self.lower_collection_reduce(receiver, receiver_ty, reducer, initial, result_ty, expr)
    }

    /// `cumulata` — serial prefix scan over an array receiver.
    ///
    /// Same seam as `reducta` (two-parameter reducer closure, seeded
    /// accumulator), but one prefix result is appended to the result array
    /// after every fold step, mirroring the Rust codegen contract pinned by
    /// `lista_cumulata_emits_serial_prefix_scan`.
    pub(super) fn lower_cumulata(
        &mut self,
        receiver: &HirExpression,
        reducer: &HirExpression,
        initial: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(receiver_ty) = receiver.ty else {
            self.errors
                .push(MirError::missing_type(receiver.span, "collection cumulata receiver"));
            return None;
        };
        if !matches!(self.normalized_type(receiver_ty), Type::Array(_) | Type::BoundedArray { .. }) {
            self.errors.push(MirError::unsupported(
                receiver.span,
                "collection cumulata before array MIR lowering",
            ));
            return None;
        }
        let result_ty = self.expr_ty(expr)?;
        let Some(accumulator_ty) = self.collection_element_ty(result_ty.semantic_id()) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "collection cumulata result before array MIR lowering",
            ));
            return None;
        };
        let reducer = self.lower_reducer_function(reducer, accumulator_ty)?;
        self.lower_collection_cumulata(receiver, receiver_ty, reducer, initial, result_ty, accumulator_ty, expr)
    }

    fn lower_collection_predicate(
        &mut self,
        receiver: &HirExpression,
        callback: MirCallee,
        kind: TransformKind,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(receiver_ty) = receiver.ty else {
            self.errors
                .push(MirError::missing_type(receiver.span, "collection predicate receiver"));
            return None;
        };
        if !matches!(self.normalized_type(receiver_ty), Type::Array(_) | Type::BoundedArray { .. }) {
            self.errors.push(MirError::unsupported(
                receiver.span,
                "collection predicate before array MIR lowering",
            ));
            return None;
        }
        let result_ty = self.expr_ty(expr)?;
        let collection_ty = MirType::semantic(receiver_ty);
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));

        let collection = self.lower_expr_value(receiver)?;
        let collection = self.materialize_operand_place(collection, collection_ty, receiver.span);

        let result = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: result, name: None, ty: result_ty, mutable: true, span: expr.span });
        self.assign(
            MirPlace::local(result),
            MirOperand::Constant(MirConstant::Bool(matches!(kind, TransformKind::All))),
            result_ty,
            expr.span,
        );

        let index = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: index, name: None, ty: numerus, mutable: true, span: expr.span });
        self.assign(
            MirPlace::local(index),
            MirOperand::Constant(MirConstant::Int(0)),
            numerus,
            expr.span,
        );

        let length = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![MirOperand::Place(collection.clone())],
            numerus,
            expr.span,
        );
        if !self.current_is_open() {
            return None;
        }

        let cond_id = self.fresh_block(expr.span);
        let body_id = self.fresh_block(expr.span);
        let decisive_id = self.fresh_block(expr.span);
        let increment_id = self.fresh_block(expr.span);
        let after_id = self.fresh_block(expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(cond_id);
        let continues = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(index)),
            length,
            bivalens,
            expr.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: continues, then_block: body_id, else_block: after_id },
            expr.span,
        );

        self.switch_to(body_id);
        let mut item_place = collection;
        item_place
            .projections
            .push(MirProjection::Index(MirOperand::Place(MirPlace::local(index))));
        let predicate = self.call_callback_callee(callback, vec![MirOperand::Place(item_place)], bivalens, expr.span);
        if !self.current_is_open() {
            return None;
        }

        let (then_block, else_block) = match kind {
            TransformKind::All => (increment_id, decisive_id),
            TransformKind::Any => (decisive_id, increment_id),
            TransformKind::Filter | TransformKind::Map => {
                return None;
            }
        };
        self.terminate_current(
            MirTerminatorKind::Branch { condition: predicate, then_block, else_block },
            expr.span,
        );

        self.switch_to(decisive_id);
        self.assign(
            MirPlace::local(result),
            MirOperand::Constant(MirConstant::Bool(matches!(kind, TransformKind::Any))),
            result_ty,
            expr.span,
        );
        self.terminate_current(MirTerminatorKind::Goto(after_id), expr.span);

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            expr.span,
        );
        self.assign(MirPlace::local(index), next, numerus, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(after_id);
        Some(MirOperand::Place(MirPlace::local(result)))
    }

    fn lower_collection_transform(
        &mut self,
        receiver: &HirExpression,
        callback: MirCallee,
        kind: TransformKind,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(receiver_ty) = receiver.ty else {
            self.errors
                .push(MirError::missing_type(receiver.span, "collection transform receiver"));
            return None;
        };
        let normalized = self.normalized_type(receiver_ty);
        if matches!(normalized, Type::Array(_) | Type::BoundedArray { .. }) {
        } else {
            self.errors.push(MirError::unsupported(
                receiver.span,
                "collection transform before array MIR lowering",
            ));
            return None;
        }
        let collection_ty = MirType::semantic(receiver_ty);
        let result_ty = self.expr_ty(expr)?;
        let result_element_ty = if let Type::Array(item) = self.normalized_type(result_ty.semantic_id()) {
            MirType::semantic(*item)
        } else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "collection transform result before array MIR lowering",
            ));
            return None;
        };
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let vacuum = MirType::semantic(self.types.primitive(Primitive::Vacuum));

        let collection = self.lower_expr_value(receiver)?;
        let collection = self.materialize_operand_place(collection, collection_ty, receiver.span);
        let result = self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(Vec::new()),
            result_ty,
            expr.span,
        );
        let result = self.materialize_operand_place(result, result_ty, expr.span);

        let index = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: index, name: None, ty: numerus, mutable: true, span: expr.span });
        self.assign(
            MirPlace::local(index),
            MirOperand::Constant(MirConstant::Int(0)),
            numerus,
            expr.span,
        );

        let length = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![MirOperand::Place(collection.clone())],
            numerus,
            expr.span,
        );
        if !self.current_is_open() {
            return None;
        }

        let cond_id = self.fresh_block(expr.span);
        let body_id = self.fresh_block(expr.span);
        let append_id = self.fresh_block(expr.span);
        let increment_id = self.fresh_block(expr.span);
        let after_id = self.fresh_block(expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(cond_id);
        let continues = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(index)),
            length,
            bivalens,
            expr.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: continues, then_block: body_id, else_block: after_id },
            expr.span,
        );

        self.switch_to(body_id);
        let mut item_place = collection.clone();
        item_place
            .projections
            .push(MirProjection::Index(MirOperand::Place(MirPlace::local(index))));
        let item = MirOperand::Place(item_place);
        let callback_return = match kind {
            TransformKind::Filter => bivalens,
            TransformKind::Map => result_element_ty,
            TransformKind::All | TransformKind::Any => {
                unreachable!("predicate folds use lower_collection_predicate")
            }
        };
        let mapped = self.call_callback_callee(callback, vec![item.clone()], callback_return, expr.span);
        if !self.current_is_open() {
            return None;
        }

        match kind {
            TransformKind::Filter => {
                self.terminate_current(
                    MirTerminatorKind::Branch { condition: mapped, then_block: append_id, else_block: increment_id },
                    expr.span,
                );
                self.switch_to(append_id);
                self.runtime_call_value(
                    MirIntrinsic::Collection(MirCollectionOp::Append),
                    vec![MirOperand::Place(result.clone()), item],
                    vacuum,
                    expr.span,
                );
            }
            TransformKind::Map => {
                self.terminate_current(MirTerminatorKind::Goto(append_id), expr.span);
                self.switch_to(append_id);
                self.runtime_call_value(
                    MirIntrinsic::Collection(MirCollectionOp::Append),
                    vec![MirOperand::Place(result.clone()), mapped],
                    vacuum,
                    expr.span,
                );
            }
            TransformKind::All | TransformKind::Any => {
                unreachable!("predicate folds use lower_collection_predicate")
            }
        }
        if !self.current_is_open() {
            return None;
        }
        self.terminate_open_current(MirTerminatorKind::Goto(increment_id), expr.span);

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            expr.span,
        );
        self.assign(MirPlace::local(index), next, numerus, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(after_id);
        Some(MirOperand::Place(result))
    }

    /// Lower the dedicated `primus_quem` first-match head (AFM-3 base lane).
    ///
    /// Total traversal with dataflow first-live selection over the same
    /// array seam the collection transforms use (`Collection(Length)` plus
    /// index projection): the predicate block is lowered INSIDE the loop, so
    /// it is evaluated for every candidate lane — including lanes after the
    /// first live match — and a predicate error rides the ordinary error
    /// channel. The result local starts at `Nil` (`nihil`): no-match and
    /// empty sources return `nihil`, never an error. The first live lane
    /// stores the element; later live lanes do not overwrite it.
    pub(super) fn lower_first_match_expr(
        &mut self,
        source: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        binding: &(DefId, Symbol),
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if !coordinates.is_empty() {
            self.errors.push(MirError::unsupported(
                expr.span,
                "primus_quem apud coordinates before multi-axis first-match MIR lowering",
            ));
            return None;
        }
        let Some(source_ty) = source.ty else {
            self.errors
                .push(MirError::missing_type(source.span, "primus_quem source"));
            return None;
        };
        let elem_ty = if let Type::Array(inner) = self.normalized_type(source_ty) { MirType::semantic(*inner) } else {
            self.errors.push(MirError::unsupported(
                source.span,
                "primus_quem source before array first-match MIR lowering",
            ));
            return None;
        };
        let result_ty = self.expr_ty(expr)?;
        let collection_ty = MirType::semantic(source_ty);
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));

        let collection = self.lower_expr_value(source)?;
        let collection = self.materialize_operand_place(collection, collection_ty, source.span);

        // `T ∪ nihil` result slot: `nihil` until the first live lane stores.
        let result = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: result, name: None, ty: result_ty, mutable: true, span: expr.span });
        self.assign(
            MirPlace::local(result),
            MirOperand::Constant(MirConstant::Nil),
            result_ty,
            expr.span,
        );

        // First-live mask state: once set, later live lanes do not store.
        let matched = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: matched, name: None, ty: bivalens, mutable: true, span: expr.span });
        self.assign(
            MirPlace::local(matched),
            MirOperand::Constant(MirConstant::Bool(false)),
            bivalens,
            expr.span,
        );

        let index = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: index, name: None, ty: numerus, mutable: true, span: expr.span });
        self.assign(
            MirPlace::local(index),
            MirOperand::Constant(MirConstant::Int(0)),
            numerus,
            expr.span,
        );

        let length = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![MirOperand::Place(collection.clone())],
            numerus,
            expr.span,
        );
        if !self.current_is_open() {
            return None;
        }

        // The candidate binder is a real local so the predicate body's
        // places resolve against it; the enclosing binding (if any) is
        // restored after the loop.
        let binder = self.next_local_id();
        self.locals.push(MirLocalDecl {
            id: binder,
            name: Some(binding.1),
            ty: elem_ty,
            mutable: false,
            span: expr.span,
        });
        let saved_binding = self
            .bindings
            .insert(binding.0, LocalBinding { local: binder, ty: elem_ty });

        let cond_id = self.fresh_block(expr.span);
        let body_id = self.fresh_block(expr.span);
        let take_id = self.fresh_block(expr.span);
        let increment_id = self.fresh_block(expr.span);
        let after_id = self.fresh_block(expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(cond_id);
        let continues = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(index)),
            length,
            bivalens,
            expr.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: continues, then_block: body_id, else_block: after_id },
            expr.span,
        );

        self.switch_to(body_id);
        let mut item_place = collection.clone();
        item_place
            .projections
            .push(MirProjection::Index(MirOperand::Place(MirPlace::local(index))));
        self.assign(
            MirPlace::local(binder),
            MirOperand::Place(item_place.clone()),
            elem_ty,
            expr.span,
        );

        // Total evaluation: the predicate runs for every lane, post-match
        // lanes included — no early exit, no skip after the first match.
        let predicate = self.push_temp(bivalens, expr.span);
        self.lower_block_to_destination(body, MirPlace::temp(predicate), bivalens, expr.span)?;
        if !self.current_is_open() {
            return None;
        }
        let not_matched = self.binary_temp(
            MirBinOp::Eq,
            MirOperand::Place(MirPlace::local(matched)),
            MirOperand::Constant(MirConstant::Bool(false)),
            bivalens,
            expr.span,
        );
        let first_live = self.binary_temp(MirBinOp::And, MirOperand::Temp(predicate), not_matched, bivalens, expr.span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: first_live, then_block: take_id, else_block: increment_id },
            expr.span,
        );

        self.switch_to(take_id);
        self.assign(MirPlace::local(result), MirOperand::Place(item_place), result_ty, expr.span);
        self.assign(
            MirPlace::local(matched),
            MirOperand::Constant(MirConstant::Bool(true)),
            bivalens,
            expr.span,
        );
        if !self.current_is_open() {
            return None;
        }
        self.terminate_current(MirTerminatorKind::Goto(increment_id), expr.span);

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            expr.span,
        );
        self.assign(MirPlace::local(index), next, numerus, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(after_id);
        match saved_binding {
            Some(previous) => {
                self.bindings.insert(binding.0, previous);
            }
            None => {
                self.bindings.remove(&binding.0);
            }
        }
        Some(MirOperand::Place(MirPlace::local(result)))
    }

    fn lower_collection_reduce(
        &mut self,
        receiver: &HirExpression,
        receiver_ty: TypeId,
        reducer: MirCallee,
        initial: &HirExpression,
        result_ty: MirType,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let collection_ty = MirType::semantic(receiver_ty);
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));

        let collection = self.lower_expr_value(receiver)?;
        let collection = self.materialize_operand_place(collection, collection_ty, receiver.span);
        let accumulator = self.lower_expr_value(initial)?;
        let accumulator = self.materialize_operand_place(accumulator, result_ty, initial.span);

        let index = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: index, name: None, ty: numerus, mutable: true, span: expr.span });
        self.assign(
            MirPlace::local(index),
            MirOperand::Constant(MirConstant::Int(0)),
            numerus,
            expr.span,
        );

        let length = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![MirOperand::Place(collection.clone())],
            numerus,
            expr.span,
        );
        if !self.current_is_open() {
            return None;
        }

        let cond_id = self.fresh_block(expr.span);
        let body_id = self.fresh_block(expr.span);
        let increment_id = self.fresh_block(expr.span);
        let after_id = self.fresh_block(expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(cond_id);
        let continues = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(index)),
            length,
            bivalens,
            expr.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: continues, then_block: body_id, else_block: after_id },
            expr.span,
        );

        self.switch_to(body_id);
        let mut item_place = collection.clone();
        item_place
            .projections
            .push(MirProjection::Index(MirOperand::Place(MirPlace::local(index))));
        let next_accumulator = self.call_callback_callee(
            reducer,
            vec![MirOperand::Place(accumulator.clone()), MirOperand::Place(item_place)],
            result_ty,
            expr.span,
        );
        self.assign(accumulator.clone(), next_accumulator, result_ty, expr.span);
        if !self.current_is_open() {
            return None;
        }
        self.terminate_current(MirTerminatorKind::Goto(increment_id), expr.span);

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            expr.span,
        );
        self.assign(MirPlace::local(index), next, numerus, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(after_id);
        Some(MirOperand::Place(accumulator))
    }

    fn lower_collection_cumulata(
        &mut self,
        receiver: &HirExpression,
        receiver_ty: TypeId,
        reducer: MirCallee,
        initial: &HirExpression,
        result_ty: MirType,
        accumulator_ty: MirType,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let collection_ty = MirType::semantic(receiver_ty);
        let numerus = MirType::semantic(self.types.primitive(Primitive::Numerus));
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let vacuum = MirType::semantic(self.types.primitive(Primitive::Vacuum));

        let collection = self.lower_expr_value(receiver)?;
        let collection = self.materialize_operand_place(collection, collection_ty, receiver.span);
        let accumulator = self.lower_expr_value(initial)?;
        let accumulator = self.materialize_operand_place(accumulator, accumulator_ty, initial.span);
        let result = self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(Vec::new()),
            result_ty,
            expr.span,
        );
        let result = self.materialize_operand_place(result, result_ty, expr.span);

        let index = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: index, name: None, ty: numerus, mutable: true, span: expr.span });
        self.assign(
            MirPlace::local(index),
            MirOperand::Constant(MirConstant::Int(0)),
            numerus,
            expr.span,
        );

        let length = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Length),
            vec![MirOperand::Place(collection.clone())],
            numerus,
            expr.span,
        );
        if !self.current_is_open() {
            return None;
        }

        let cond_id = self.fresh_block(expr.span);
        let body_id = self.fresh_block(expr.span);
        let append_id = self.fresh_block(expr.span);
        let increment_id = self.fresh_block(expr.span);
        let after_id = self.fresh_block(expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(cond_id);
        let continues = self.binary_temp(
            MirBinOp::Lt,
            MirOperand::Place(MirPlace::local(index)),
            length,
            bivalens,
            expr.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: continues, then_block: body_id, else_block: after_id },
            expr.span,
        );

        self.switch_to(body_id);
        let mut item_place = collection.clone();
        item_place
            .projections
            .push(MirProjection::Index(MirOperand::Place(MirPlace::local(index))));
        let next_accumulator = self.call_callback_callee(
            reducer,
            vec![MirOperand::Place(accumulator.clone()), MirOperand::Place(item_place)],
            accumulator_ty,
            expr.span,
        );
        self.assign(accumulator.clone(), next_accumulator, accumulator_ty, expr.span);
        if !self.current_is_open() {
            return None;
        }
        self.terminate_current(MirTerminatorKind::Goto(append_id), expr.span);

        self.switch_to(append_id);
        self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Append),
            vec![MirOperand::Place(result.clone()), MirOperand::Place(accumulator)],
            vacuum,
            expr.span,
        );
        if !self.current_is_open() {
            return None;
        }
        self.terminate_current(MirTerminatorKind::Goto(increment_id), expr.span);

        self.switch_to(increment_id);
        let next = self.binary_temp(
            MirBinOp::Add,
            MirOperand::Place(MirPlace::local(index)),
            MirOperand::Constant(MirConstant::Int(1)),
            numerus,
            expr.span,
        );
        self.assign(MirPlace::local(index), next, numerus, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(cond_id), expr.span);

        self.switch_to(after_id);
        Some(MirOperand::Place(result))
    }

    fn collection_element_ty(&self, ty: TypeId) -> Option<MirType> {
        match self.normalized_type(ty) {
            Type::Array(item) | Type::BoundedArray { element: item, .. } => Some(MirType::semantic(*item)),
            _ => None,
        }
    }

    pub(super) fn take_synthetic_functions(&mut self) -> Vec<MirFunction> {
        std::mem::take(&mut self.synthetic_functions)
    }

    pub(super) fn take_closure_environments(&mut self) -> Vec<MirClosureEnvironment> {
        std::mem::take(&mut self.closure_environments)
    }
}

#[derive(Debug, Clone, Copy)]
enum TransformKind {
    All,
    Any,
    Filter,
    Map,
}
