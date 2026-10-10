//! Elementwise operator lift over tensors (gpu-reset rule 15, K4).
//!
//! The typechecker admits `+ - * / % ÷`, unary `-`, the numeric comparisons
//! `< > ≤ ≥ ≅ ≇`, `et`/`aut`/`non`, `vel` and the `✓ ✗` select on tensors,
//! against a scalar or a same-shape tensor. They lower to one
//! `MirIntrinsic::TensorLift(op)` whose operands are all evaluated first, so
//! nothing short-circuits (a documented deviation from the scalar meaning).
//! `≡ ≢` over two tensors compare whole values and lower to the same call with
//! `WholeEq`/`WholeNotEq`, which yields one `bivalens`. Tensor `+ -` against a
//! tensor keeps its `addita`/`subtrahe` lowering and `⊙ ⊘` their glyph lowering.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;

impl FunctionBuilder<'_> {
    /// Whether an operand's type is a tensor (aliases and access markers peeled).
    fn is_tensor_operand(&self, expr: &HirExpression) -> bool {
        expr.ty.is_some_and(|ty| {
            let payload = self.types.peel_access_marker(ty).1;
            matches!(self.normalized_type(payload), Type::Tensor(..))
        })
    }

    /// The lift operator a binary operator becomes over these operands, if any.
    pub(super) fn tensor_lift_binary_op(
        &self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
    ) -> Option<MirLiftOp> {
        let (left, right) = (self.is_tensor_operand(lhs), self.is_tensor_operand(rhs));
        if !left && !right {
            return None;
        }
        // Tensor with tensor keeps the existing `addita`/`subtrahe` routes.
        let against_scalar = left != right;
        Some(match op {
            HirBinOp::Add if against_scalar => MirLiftOp::Add,
            HirBinOp::Sub if against_scalar => MirLiftOp::Sub,
            HirBinOp::Mul if against_scalar => MirLiftOp::Mul,
            HirBinOp::Div if against_scalar => MirLiftOp::Div,
            HirBinOp::Mod if against_scalar => MirLiftOp::Mod,
            HirBinOp::TrueDiv => MirLiftOp::TrueDiv,
            // `≡ ≢` compare whole tensors (one verdict); the typechecker rejects
            // a scalar peer, so only a tensor pair reaches here.
            HirBinOp::Eq | HirBinOp::StrictEq if !against_scalar => MirLiftOp::WholeEq,
            HirBinOp::NotEq | HirBinOp::StrictNotEq if !against_scalar => MirLiftOp::WholeNotEq,
            HirBinOp::ApproxEq => MirLiftOp::Eq,
            HirBinOp::ApproxNotEq => MirLiftOp::NotEq,
            HirBinOp::Lt => MirLiftOp::Lt,
            HirBinOp::Gt => MirLiftOp::Gt,
            HirBinOp::LtEq => MirLiftOp::LtEq,
            HirBinOp::GtEq => MirLiftOp::GtEq,
            HirBinOp::And => MirLiftOp::And,
            HirBinOp::Or => MirLiftOp::Or,
            HirBinOp::Coalesce => MirLiftOp::Coalesce,
            _ => return None,
        })
    }

    /// The lift operator of a unary operator over a tensor operand, if any.
    pub(super) fn tensor_lift_unary_op(&self, op: HirUnOp, operand: &HirExpression) -> Option<MirLiftOp> {
        if !self.is_tensor_operand(operand) {
            return None;
        }
        match op {
            HirUnOp::Neg => Some(MirLiftOp::Neg),
            HirUnOp::Not => Some(MirLiftOp::Not),
            HirUnOp::BitNot => None,
        }
    }

    /// Lower `operands` (in operator order) to one `TensorLift` call typed as `expr`.
    pub(super) fn lower_tensor_lift(
        &mut self,
        op: MirLiftOp,
        operands: &[&HirExpression],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let mut args = Vec::with_capacity(operands.len());
        for operand in operands {
            args.push(self.lower_expr_value(operand)?);
        }
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(MirIntrinsic::TensorLift(op), args, ty, expr.span))
    }

    /// Whether an expression-valued `si` is a tensor select: its condition is a
    /// tensor mask, not a branch condition.
    pub(super) fn is_tensor_select(&self, cond: &HirExpression) -> bool {
        self.is_tensor_operand(cond)
    }

    /// Lower the `✓ ✗` select over a mask: the mask and both branch values are
    /// evaluated, then each element takes the branch its mask element selects.
    pub(super) fn lower_tensor_select(
        &mut self,
        cond: &HirExpression,
        then_block: &HirBlock,
        else_block: &HirBlock,
        destination: MirPlace,
        ty: MirType,
        span: Span,
    ) -> Option<()> {
        let (Some(then_value), Some(else_value)) = (then_block.expr.as_deref(), else_block.expr.as_deref()) else {
            self.errors.push(MirError::unsupported(
                span,
                "tensor select branch without a value before MIR lowering",
            ));
            return None;
        };
        let mask = self.lower_expr_value(cond)?;
        let then_operand = self.lower_expr_value(then_value)?;
        let else_operand = self.lower_expr_value(else_value)?;
        let value = self.runtime_call_value(
            MirIntrinsic::TensorLift(MirLiftOp::Select),
            vec![mask, then_operand, else_operand],
            ty,
            span,
        );
        self.assign(destination, value, ty, span);
        Some(())
    }
}
