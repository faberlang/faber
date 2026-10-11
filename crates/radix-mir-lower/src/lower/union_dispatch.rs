//! Per-member dispatch through a `∪` of nominal members (D9.2).
//!
//! Semantic admits a method call, field read, or field write on a union
//! receiver only in the overlap: every member declares the name with exactly
//! the same signature / type. MIR has no union-level method or field, so the
//! access lowers to a runtime type-test chain over the members. Each arm
//! narrows the receiver into a member-typed temp and runs that member's own
//! method or field projection; the last member takes the fall-through arm so
//! the CFG stays total without a runner-unsupported `Unreachable`.
//!
//! Writes narrow, store the field on the member temp, and write the member
//! back into the union place, so the update holds under both value and
//! shared-reference runner representations.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;
use radix_hir::HirCallArg;

/// One union member the dispatch chain can narrow to.
#[derive(Clone, Copy)]
struct DispatchMember {
    ty: TypeId,
    def_id: DefId,
}

impl FunctionBuilder<'_> {
    /// Nominal members of a union receiver type, or `None` when the receiver
    /// is not a `∪` whose every member is a `genus`.
    fn nominal_union_dispatch_members(&self, ty: TypeId) -> Option<Vec<DispatchMember>> {
        let payload = self.method_receiver_payload_id(ty);
        let Type::Union(members) = self.normalized_type(payload) else {
            return None;
        };
        if members.len() < 2 {
            return None;
        }
        members
            .iter()
            .map(|member| {
                let def_id = self.method_receiver_struct_def(*member)?;
                Some(DispatchMember { ty: *member, def_id })
            })
            .collect()
    }

    /// Implementing `genus` members for a method call through a contract-typed
    /// receiver (`Bumpable b`), or `None` when the receiver is not a contract
    /// with at least one local implementor. The chain is the same type-test
    /// dispatch a union uses, over the closed set of `implet` declarers.
    fn interface_dispatch_members(&self, ty: TypeId) -> Option<Vec<DispatchMember>> {
        let mut payload = self.normalized_type(self.method_receiver_payload_id(ty));
        if let Type::Applied(base, _) = payload {
            payload = self.normalized_type(*base);
        }
        let Type::Interface(contract) = payload else {
            return None;
        };
        let implementors = self.context.interface_implementors.get(contract)?;
        Some(
            implementors
                .iter()
                .map(|(def_id, ty)| DispatchMember { ty: *ty, def_id: *def_id })
                .collect(),
        )
    }

    /// Run `arm` once per member inside a type-test chain over `receiver`.
    ///
    /// `arm` receives the member-narrowed temp place and returns the arm's
    /// value; when `result_ty` is non-vacuum the value is stored into a shared
    /// result temp returned as the expression operand.
    fn lower_union_dispatch(
        &mut self,
        receiver_place: &MirPlace,
        members: &[DispatchMember],
        result_ty: MirType,
        span: Span,
        mut arm: impl FnMut(&mut Self, MirPlace, DispatchMember) -> Option<MirOperand>,
    ) -> Option<MirOperand> {
        let destination = (!self.is_vacuum(result_ty)).then(|| MirPlace::temp(self.push_temp(result_ty, span)));
        let join_id = self.fresh_block(span);
        let bool_ty = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let last = members.len() - 1;
        let mut reaches_join = false;
        for (index, member) in members.iter().copied().enumerate() {
            let else_id = if index < last {
                let condition = self.runtime_call_value(
                    MirIntrinsic::TypeCheck(MirTypeCheckOp { positive: true, ty: MirType::semantic(member.ty) }),
                    vec![MirOperand::Place(receiver_place.clone())],
                    bool_ty,
                    span,
                );
                let then_id = self.fresh_block(span);
                let else_id = self.fresh_block(span);
                self.terminate_current(
                    MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_id },
                    span,
                );
                self.switch_to(then_id);
                Some(else_id)
            } else {
                None
            };
            let member_ty = MirType::semantic(member.ty);
            let narrowed = MirPlace::temp(self.push_temp(member_ty, span));
            self.assign(narrowed.clone(), MirOperand::Place(receiver_place.clone()), member_ty, span);
            let value = arm(self, narrowed, member)?;
            if let Some(destination) = destination.clone() {
                self.assign(destination, value, result_ty, span);
            }
            reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);
            if let Some(else_id) = else_id {
                self.switch_to(else_id);
            }
        }
        if reaches_join {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, span);
        }
        Some(destination.map_or(MirOperand::Constant(MirConstant::Unit), MirOperand::Place))
    }

    /// Materialize a union receiver expression as a place the chain can test.
    fn union_receiver_place(&mut self, receiver: &HirExpression, receiver_ty: TypeId) -> Option<MirPlace> {
        let value = self.lower_expr_value(receiver)?;
        Some(self.materialize_operand_place(value, MirType::semantic(receiver_ty), receiver.span))
    }

    /// `receiver.method(args)` on a nominal union: call each member's method.
    pub(super) fn lower_union_method_call(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        args: &[HirCallArg],
        conversion: Option<DefId>,
        expr: &HirExpression,
    ) -> Option<Option<MirOperand>> {
        let receiver_ty = receiver.ty?;
        let members = self
            .nominal_union_dispatch_members(receiver_ty)
            .or_else(|| self.interface_dispatch_members(receiver_ty))?;
        let mut targets = Vec::with_capacity(members.len());
        for member in &members {
            targets.push(
                self.context
                    .method_targets
                    .get(&(member.def_id, method))
                    .copied()?,
            );
        }
        let receiver_place = self.union_receiver_place(receiver, receiver_ty);
        let Some(receiver_place) = receiver_place else {
            return Some(None);
        };
        let Some(lowered_args) = self.lower_hir_call_args(args) else {
            return Some(None);
        };
        let Some(result_ty) = self.expr_ty(expr) else {
            return Some(None);
        };
        let mut next_target = targets.into_iter();
        Some(
            self.lower_union_dispatch(&receiver_place, &members, result_ty, expr.span, |builder, narrowed, _| {
                let target = next_target.next()?;
                let mut call_args = Vec::with_capacity(lowered_args.len() + 1);
                call_args.push(MirOperand::Place(narrowed));
                call_args.extend(lowered_args.iter().cloned());
                builder.lower_definition_call(target.def_id, Vec::new(), call_args, conversion, 1, expr)
            }),
        )
    }

    /// `object.field` read on a nominal union: project each member's field.
    pub(super) fn lower_union_field_read(
        &mut self,
        object: &HirExpression,
        name: Symbol,
        expr: &HirExpression,
    ) -> Option<Option<MirOperand>> {
        let object_ty = object.ty?;
        let members = self.nominal_union_dispatch_members(object_ty)?;
        let Some(receiver_place) = self.union_receiver_place(object, object_ty) else {
            return Some(None);
        };
        let Some(result_ty) = self.expr_ty(expr) else {
            return Some(None);
        };
        Some(
            self.lower_union_dispatch(&receiver_place, &members, result_ty, expr.span, |_, mut narrowed, _| {
                narrowed.projections.push(MirProjection::Field(name));
                Some(MirOperand::Place(narrowed))
            }),
        )
    }

    /// `object.field ← value` on a nominal union: store the field on the live
    /// member and write the member back into the union place.
    pub(super) fn lower_union_field_assign(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        span: Span,
    ) -> Option<Option<MirOperand>> {
        let HirExpressionKind::Field(object, name) = &lhs.kind else {
            return None;
        };
        let object_ty = object.ty?;
        let members = self.nominal_union_dispatch_members(object_ty)?;
        let Some((receiver_place, _)) = self.lower_assignment_place_with_fallback(object, MirType::semantic(object_ty))
        else {
            return Some(None);
        };
        let Some(field_ty) = self.expr_ty(lhs) else {
            return Some(None);
        };
        let Some(value) = self.lower_expr_value(rhs) else {
            return Some(None);
        };
        let union_ty = MirType::semantic(object_ty);
        let vacuum = MirType::semantic(self.types.primitive(Primitive::Vacuum));
        let name = *name;
        let writeback = receiver_place.clone();
        Some(
            self.lower_union_dispatch(&receiver_place, &members, vacuum, span, |builder, narrowed, _| {
                let mut field = narrowed.clone();
                field.projections.push(MirProjection::Field(name));
                builder.assign(field, value.clone(), field_ty, span);
                builder.assign(writeback.clone(), MirOperand::Place(narrowed), union_ty, span);
                Some(MirOperand::Constant(MirConstant::Unit))
            }),
        )
    }
}
