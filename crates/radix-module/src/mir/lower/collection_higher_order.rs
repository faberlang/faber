//! Higher-order collection method lowering (`filtrata`, `mappata`, `reducta`).
//!
//! Collection transforms lower into explicit MIR loops over array receivers.
//! Each callback is emitted as a synthetic closure function, then invoked once
//! per element through the shared callable lowering path. Filtered results
//! append only when the predicate returns true; mapped results append the
//! callback return value directly; reduced results update an accumulator.
//!
//! INVARIANTS
//! ==========
//! - Only array receivers are supported in this phase.
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
        if !matches!(self.normalized_type(receiver_ty), Type::Array(_)) {
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
        if let Type::Array(_) = normalized {
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

    fn collection_element_ty(&self, ty: TypeId) -> Option<MirType> {
        match self.normalized_type(ty) {
            Type::Array(item) => Some(MirType::semantic(*item)),
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
    Filter,
    Map,
}
