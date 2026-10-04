//! Scalar operators lifted over a register vector (gpu-reset rule 15, K4).
//!
//! The typechecker admits `+ - * / %` between a `vector` and a scalar (either
//! side) and unary `-` on a vector. They lower to one deferred
//! [`MirVectorOp`], which `radix_mir::vector_expand` unrolls into per-lane
//! scalar statements once the width is a literal, so every backend sees plain
//! scalar arithmetic and a vector construct, never a vector operator. The
//! result is a new vector; neither operand is touched. Vector with vector
//! `+ -` keeps its `addita`/`subtrahe` route and `⊙ ⊘` their glyph route.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;

/// What an operand is to the vector lift.
#[derive(PartialEq, Eq)]
enum LiftOperand {
    Vector,
    Scalar,
    Other,
}

impl FunctionBuilder<'_> {
    fn vector_lift_operand(&self, expr: &HirExpression) -> LiftOperand {
        let Some(ty) = expr.ty else {
            return LiftOperand::Other;
        };
        let payload = self.types.peel_access_marker(ty).1;
        match self.normalized_type(payload) {
            Type::Vector(..) => LiftOperand::Vector,
            Type::Primitive(Primitive::Numerus | Primitive::Fractus) | Type::SizedNumeric(..) => {
                LiftOperand::Scalar
            }
            _ => LiftOperand::Other,
        }
    }

    /// Lower `vector op scalar` / `scalar op vector` to one deferred
    /// [`MirVectorOp::ElementwiseScalar`], or `None` when the pair is not a
    /// vector-by-scalar lift (the caller keeps resolving).
    pub(super) fn lower_vector_scalar_binary(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<Option<MirOperand>> {
        let mir_op = match op {
            HirBinOp::Add => MirBinOp::Add,
            HirBinOp::Sub => MirBinOp::Sub,
            HirBinOp::Mul => MirBinOp::Mul,
            HirBinOp::Div => MirBinOp::Div,
            HirBinOp::Mod => MirBinOp::Mod,
            _ => return None,
        };
        let (vector, scalar, scalar_first) =
            match (self.vector_lift_operand(lhs), self.vector_lift_operand(rhs)) {
                (LiftOperand::Vector, LiftOperand::Scalar) => (lhs, rhs, false),
                (LiftOperand::Scalar, LiftOperand::Vector) => (rhs, lhs, true),
                _ => return None,
            };
        // The source order of evaluation is kept: the left operand first.
        let (vector_value, scalar_value) = if scalar_first {
            let scalar_value = self.lower_expr_value(scalar)?;
            (self.lower_expr_value(vector)?, scalar_value)
        } else {
            let vector_value = self.lower_expr_value(vector)?;
            (vector_value, self.lower_expr_value(scalar)?)
        };
        let checked = self.vector_lanes_are_checked(expr);
        Some(Some(self.vector_lift_call(
            MirVectorOp::ElementwiseScalar {
                op: mir_op,
                scalar_first,
                checked,
            },
            vec![(vector_value, vector), (scalar_value, scalar)],
            expr,
        )?))
    }

    /// Lower unary `-` on a vector to one deferred [`MirVectorOp::Negate`].
    pub(super) fn lower_vector_negate(
        &mut self,
        operand: &HirExpression,
        expr: &HirExpression,
    ) -> Option<Option<MirOperand>> {
        if self.vector_lift_operand(operand) != LiftOperand::Vector {
            return None;
        }
        let value = self.lower_expr_value(operand)?;
        let checked = self.vector_lanes_are_checked(expr);
        Some(Some(self.vector_lift_call(
            MirVectorOp::Negate { checked },
            vec![(value, operand)],
            expr,
        )?))
    }

    /// Whether the lanes of a lifted vector expression are integer slots whose
    /// store traps: each lane is exact and stored under the element type at the
    /// operation (ruling 19(g)). Float lanes follow IEEE, and a device function
    /// keeps its own checked device arithmetic, so neither takes the node.
    fn vector_lanes_are_checked(&self, expr: &HirExpression) -> bool {
        if self.is_device_lowering() {
            return false;
        }
        let Some(ty) = expr.ty else {
            return false;
        };
        let payload = self.types.peel_access_marker(ty).1;
        let Type::Vector(element, _) = self.normalized_type(payload) else {
            return false;
        };
        self.is_integer_store_slot(MirType::semantic(*element))
    }

    fn vector_lift_call(
        &mut self,
        op: MirVectorOp,
        operands: Vec<(MirOperand, &HirExpression)>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let result_ty = self.expr_ty(expr)?;
        let mut args = Vec::with_capacity(operands.len());
        for (operand, source) in operands {
            let ty = source.ty.map_or(result_ty, MirType::semantic);
            args.push(self.vector_operand_place(operand, ty, source.span));
        }
        Some(self.runtime_call_value(MirIntrinsic::VectorOp(op), args, result_ty, expr.span))
    }
}
