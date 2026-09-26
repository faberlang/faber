//! MIR lowering for routed `↦` conversions.
//!
//! Typecheck records a [`HirConversioRoute`] when a conversion is served by a
//! built-in `ordo` row (D9.4) rather than a runtime conversion intrinsic. Each
//! row lowers to an equality chain over the members, in declaration order,
//! that writes one result temp:
//!
//! - `Ordo ↦ numerus` / `Ordo ↦ textus`: test the scrutinee against each unit
//!   variant and write its discriminant or name. The chain is exhaustive, so
//!   the final fall-through block is unreachable.
//! - `numerus ↦ Ordo`: test the value against each discriminant and write the
//!   first matching variant. No match yields the `⊥` default, or fails the
//!   conversion through the active alternate exit (trap, `cape` handler, or
//!   the function's `⇥` channel) with a `textus` payload, or panics when no
//!   alternate exit is active — the same postures as a failable runtime
//!   conversion.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;
use crate::hir::{HirConversioRoute, HirOrdoRouteMember, HirOrdoRow};

impl FunctionBuilder<'_> {
    pub(super) fn lower_conversio_route(
        &mut self,
        source: &HirExpression,
        route: &HirConversioRoute,
        recovery: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        match route {
            HirConversioRoute::Ordo { row, members } => {
                self.lower_ordo_conversio(source, *row, members, recovery, expr)
            }
            HirConversioRoute::Function { .. } => {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "registered conversio function route before MIR lowering",
                ));
                None
            }
        }
    }

    fn lower_ordo_conversio(
        &mut self,
        source: &HirExpression,
        row: HirOrdoRow,
        members: &[HirOrdoRouteMember],
        recovery: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let span = expr.span;
        let source_ty = self.expr_ty(source)?;
        let result_ty = self.expr_ty(expr)?;
        let value = self.lower_expr_value(source)?;
        let result = MirPlace::temp(self.push_temp(result_ty, span));
        let join_id = self.fresh_block(span);
        let bool_ty = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let mut reaches_join = false;

        for member in members {
            let (probe, written) = match row {
                HirOrdoRow::ToNumerus => (
                    self.unit_variant_operand(member.variant, source_ty, span),
                    MirOperand::Constant(MirConstant::Int(member.value?)),
                ),
                HirOrdoRow::ToTextus => (
                    self.unit_variant_operand(member.variant, source_ty, span),
                    MirOperand::Constant(MirConstant::String(member.name)),
                ),
                HirOrdoRow::FromNumerus => {
                    let Some(discriminant) = member.value else {
                        continue;
                    };
                    (
                        MirOperand::Constant(MirConstant::Int(discriminant)),
                        self.unit_variant_operand(member.variant, result_ty, span),
                    )
                }
            };
            let condition = self.binary_temp(MirBinOp::Eq, value.clone(), probe, bool_ty, span);
            let then_id = self.fresh_block(span);
            let else_id = self.fresh_block(span);
            self.terminate_current(
                MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_id },
                span,
            );
            self.switch_to(then_id);
            self.assign(result.clone(), written, result_ty, span);
            reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);
            self.switch_to(else_id);
        }

        match (row, recovery) {
            (HirOrdoRow::FromNumerus, Some(recovery)) => {
                let fallback = self.lower_expr_value(recovery)?;
                self.assign(result.clone(), fallback, result_ty, span);
                reaches_join |= self.terminate_open_current(MirTerminatorKind::Goto(join_id), span);
            }
            (HirOrdoRow::FromNumerus, None) => self.fail_ordo_conversio(span),
            // `Ordo ↦ numerus` / `↦ textus` cover every member.
            _ => {
                self.terminate_current(MirTerminatorKind::Unreachable, span);
            }
        }

        if reaches_join {
            self.switch_to(join_id);
        } else {
            self.seal_unreachable(join_id, span);
        }
        Some(MirOperand::Place(result))
    }

    fn unit_variant_operand(&mut self, variant: DefId, ty: MirType, span: Span) -> MirOperand {
        self.construct_temp(
            MirAggregateKind::EnumVariant(variant),
            MirAggregateFields::Ordered(Vec::new()),
            ty,
            span,
        )
    }

    /// Route a failed `numerus ↦ Ordo` through the innermost alternate exit,
    /// mirroring `iace` with a `textus` payload; panic when none is active.
    fn fail_ordo_conversio(&mut self, span: Span) {
        let message = MirOperand::Constant(MirConstant::String(self.context.diagnostic_ordo_no_member));
        let textus = MirType::semantic(self.types.primitive(Primitive::Textus));
        if let Some(trap) = self.take_innermost_trap() {
            self.assign(trap.destination, message, textus, span);
            self.terminate_current(MirTerminatorKind::Goto(trap.join_block), span);
            return;
        }
        if let Some(handler) = self.handlers.last().cloned() {
            self.assign(handler.error_place, message, textus, span);
            self.terminate_current(MirTerminatorKind::Goto(handler.error_block), span);
            return;
        }
        if self.error_ty.is_some() {
            self.terminate_current(MirTerminatorKind::ReturnError(message), span);
            return;
        }
        let numquam = MirType::semantic(self.types.primitive(Primitive::Numquam));
        self.append_stmt(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: MirRuntimeCall { intrinsic: MirIntrinsic::Panic, args: vec![message], return_ty: numquam },
            },
            span,
        });
        self.terminate_current(MirTerminatorKind::Unreachable, span);
    }
}
