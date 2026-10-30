//! Operators lifted over a register `vector` or `matrix` (gpu-reset rule 15.1, K4).
//!
//! The typechecker admits `+ - * / % ÷` between a register and a scalar (either
//! side), unary `-`, `÷`, `⤒`, `⤓`, the comparisons `< > ≤ ≥ ≅ ≇` and `et`/`aut`
//! between two registers of one shape (the last two over bool registers, with
//! `non` and the `✓ ✗` select over a bool mask). The
//! scalar operator is applied per lane by one rule, [`LaneOp`], so a register
//! gets no second arithmetic model:
//!
//! - a `vector` lowers to one deferred [`MirVectorOp`], which
//!   `radix_mir::vector_expand` unrolls into per-lane scalar statements once
//!   the width is a literal, so every backend sees plain scalar arithmetic and
//!   a vector construct, never a vector operator;
//! - a `matrix` has a literal shape at lowering (the CPU register subset), so
//!   its cells are unrolled here with the same [`LaneOp`] and a matrix
//!   construct.
//!
//! The result is a new register; neither operand is touched. Register with
//! register `+ -` keeps its `addita`/`subtrahe` route and `⊙ ⊘` their glyph
//! route; both now store each lane under the element type through the same
//! checked-lane flag.

use super::runtime::matrix_cell_operand;
#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;
use radix_mir::lane_op::{LaneSink, lane_conversion, stored_lane};

/// Whether an operator takes two registers of one kind and shape (`÷` and the
/// comparison and logic lanes); `+ -` keep their `addita` route and `⊙ ⊘` their
/// glyph route.
fn pairs_registers(lane: MirLaneOp) -> bool {
    matches!(
        lane,
        MirLaneOp::TrueDiv
            | MirLaneOp::Lt
            | MirLaneOp::Gt
            | MirLaneOp::LtEq
            | MirLaneOp::GtEq
            | MirLaneOp::Eq
            | MirLaneOp::NotEq
            | MirLaneOp::And
            | MirLaneOp::Or
    )
}

/// What an operand is to the register lift.
#[derive(PartialEq, Eq)]
pub(super) enum LiftOperand {
    Vector,
    Matrix,
    Scalar,
    Other,
}

/// One side of a matrix lane operator: a matrix read cell by cell, or a scalar
/// read as it is.
pub(super) enum LaneSource {
    Cells(MirPlace),
    Scalar(MirOperand),
}

/// A matrix lane's statements, written through the lowerer.
pub(super) struct MatrixLanes<'a, 'b> {
    pub(super) builder: &'a mut FunctionBuilder<'b>,
    pub(super) span: Span,
}

impl LaneSink for MatrixLanes<'_, '_> {
    fn lane_binary(&mut self, op: MirBinOp, lhs: MirOperand, rhs: MirOperand, ty: MirType) -> MirOperand {
        self.builder
            .assign_temp(MirValueKind::Binary { op, lhs, rhs }, ty, self.span)
    }

    fn lane_read(&mut self, operand: MirOperand, ty: MirType) -> MirOperand {
        self.builder
            .assign_temp(MirValueKind::Operand(operand), ty, self.span)
    }

    fn lane_convert(
        &mut self,
        value: MirOperand,
        source: MirType,
        target: MirType,
        store: Option<radix_hir::HirStoreSite>,
    ) -> MirOperand {
        self.builder.runtime_call_value(
            MirIntrinsic::Convert(lane_conversion(source, target, store)),
            vec![value],
            target,
            self.span,
        )
    }

    fn lane_call(&mut self, intrinsic: MirIntrinsic, args: Vec<MirOperand>, ty: MirType) -> MirOperand {
        self.builder
            .runtime_call_value(intrinsic, args, ty, self.span)
    }
}

impl FunctionBuilder<'_> {
    pub(super) fn lift_operand(&self, expr: &HirExpression) -> LiftOperand {
        let Some(ty) = expr.ty else {
            return LiftOperand::Other;
        };
        let payload = self.types.peel_access_marker(ty).1;
        match self.normalized_type(payload) {
            Type::Vector(..) => LiftOperand::Vector,
            Type::Matrix(..) => LiftOperand::Matrix,
            Type::Primitive(Primitive::Numerus | Primitive::Fractus | Primitive::Bivalens) | Type::SizedNumeric(..) => {
                LiftOperand::Scalar
            }
            _ => LiftOperand::Other,
        }
    }

    /// The type of one lane of an operand: the element of a register, the type
    /// of a scalar.
    fn lift_lane_ty(&self, expr: &HirExpression) -> Option<MirType> {
        let ty = expr.ty?;
        let payload = self.types.peel_access_marker(ty).1;
        Some(match self.normalized_type(payload) {
            Type::Vector(element, _) | Type::Matrix(element, _) => MirType::semantic(*element),
            _ => MirType::semantic(payload),
        })
    }

    /// Lower a binary operator over a register to a deferred vector op (a
    /// `vector`) or unrolled cells (a `matrix`), or `None` when the pair is
    /// not a register lift (the caller keeps resolving). `Some(None)` means a
    /// diagnostic was recorded.
    pub(super) fn lower_register_lift_binary(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<Option<MirOperand>> {
        let lane = match op {
            HirBinOp::Add => MirLaneOp::Add,
            HirBinOp::Sub => MirLaneOp::Sub,
            HirBinOp::Mul => MirLaneOp::Mul,
            HirBinOp::Div => MirLaneOp::Div,
            HirBinOp::Mod => MirLaneOp::Mod,
            HirBinOp::TrueDiv => MirLaneOp::TrueDiv,
            HirBinOp::Lt => MirLaneOp::Lt,
            HirBinOp::Gt => MirLaneOp::Gt,
            HirBinOp::LtEq => MirLaneOp::LtEq,
            HirBinOp::GtEq => MirLaneOp::GtEq,
            HirBinOp::ApproxEq => MirLaneOp::Eq,
            HirBinOp::ApproxNotEq => MirLaneOp::NotEq,
            HirBinOp::And => MirLaneOp::And,
            HirBinOp::Or => MirLaneOp::Or,
            _ => return None,
        };
        let pairs = pairs_registers(lane);
        match (self.lift_operand(lhs), self.lift_operand(rhs)) {
            (LiftOperand::Vector, LiftOperand::Scalar) => Some(self.lower_vector_lift(lane, (lhs, rhs), false, expr)),
            (LiftOperand::Scalar, LiftOperand::Vector) => Some(self.lower_vector_lift(lane, (rhs, lhs), true, expr)),
            (LiftOperand::Vector, LiftOperand::Vector) if pairs => Some(self.lower_vector_pair(lane, lhs, rhs, expr)),
            (LiftOperand::Matrix, LiftOperand::Scalar) | (LiftOperand::Scalar, LiftOperand::Matrix) => {
                Some(self.lower_matrix_lift(lane, lhs, rhs, expr))
            }
            (LiftOperand::Matrix, LiftOperand::Matrix) if pairs => Some(self.lower_matrix_lift(lane, lhs, rhs, expr)),
            _ => None,
        }
    }

    /// Lower the `⤒`/`⤓` method twin over a register receiver: a scalar or a
    /// same-shape register peer. `None` when the receiver is not a register.
    pub(super) fn lower_register_extremum(
        &mut self,
        receiver: &HirExpression,
        method_name: &str,
        arg: &HirExpression,
        expr: &HirExpression,
    ) -> Option<Option<MirOperand>> {
        let lane = match method_name {
            "maximus" => MirLaneOp::Max,
            "minimus" => MirLaneOp::Min,
            _ => return None,
        };
        let receiver_kind = self.lift_operand(receiver);
        let arg_kind = self.lift_operand(arg);
        match (receiver_kind, arg_kind) {
            (LiftOperand::Vector, LiftOperand::Scalar) => {
                Some(self.lower_vector_lift(lane, (receiver, arg), false, expr))
            }
            (LiftOperand::Vector, LiftOperand::Vector) => Some(self.lower_vector_pair(lane, receiver, arg, expr)),
            (LiftOperand::Matrix, LiftOperand::Scalar | LiftOperand::Matrix) => {
                Some(self.lower_matrix_lift(lane, receiver, arg, expr))
            }
            _ => None,
        }
    }

    /// Lower unary `-` over a register to one deferred [`MirVectorOp::Negate`]
    /// (a `vector`) or negated cells (a `matrix`).
    pub(super) fn lower_register_negate(
        &mut self,
        operand: &HirExpression,
        expr: &HirExpression,
    ) -> Option<Option<MirOperand>> {
        match self.lift_operand(operand) {
            LiftOperand::Vector => {
                let Some(value) = self.lower_expr_value(operand) else {
                    return Some(None);
                };
                let checked = self.register_lanes_are_checked(expr);
                Some(self.vector_lift_call(MirVectorOp::Negate { checked }, vec![(value, operand)], expr))
            }
            LiftOperand::Matrix => Some(self.lower_matrix_unary(MirUnOp::Neg, operand, expr)),
            _ => None,
        }
    }

    /// Lower `non` over a bool register to one deferred [`MirVectorOp::Not`]
    /// (a `vector`) or negated cells (a `matrix`).
    pub(super) fn lower_register_not(
        &mut self,
        operand: &HirExpression,
        expr: &HirExpression,
    ) -> Option<Option<MirOperand>> {
        match self.lift_operand(operand) {
            LiftOperand::Vector => {
                let Some(value) = self.lower_expr_value(operand) else {
                    return Some(None);
                };
                Some(self.vector_lift_call(MirVectorOp::Not, vec![(value, operand)], expr))
            }
            LiftOperand::Matrix => Some(self.lower_matrix_unary(MirUnOp::Not, operand, expr)),
            _ => None,
        }
    }

    /// Whether the lanes of a lifted register expression are integer slots
    /// whose store traps: each lane is exact and stored under the element type
    /// at the operation (ruling 19(g)). Float lanes follow IEEE, and a device
    /// function keeps its own checked device arithmetic, so neither takes the
    /// node.
    pub(super) fn register_lanes_are_checked(&self, expr: &HirExpression) -> bool {
        if self.is_device_lowering() {
            return false;
        }
        let Some(ty) = expr.ty else {
            return false;
        };
        let payload = self.types.peel_access_marker(ty).1;
        let (Type::Vector(element, _) | Type::Matrix(element, _)) = self.normalized_type(payload) else {
            return false;
        };
        self.is_integer_store_slot(MirType::semantic(*element))
    }

    /// `vector op scalar` / `scalar op vector` as one deferred
    /// [`MirVectorOp::ElementwiseScalar`]; `operands` is `(vector, scalar)`
    /// whichever side the scalar was written.
    fn lower_vector_lift(
        &mut self,
        op: MirLaneOp,
        (vector, scalar): (&HirExpression, &HirExpression),
        scalar_first: bool,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        // The source order of evaluation is kept: the left operand first.
        let (vector_value, scalar_value) = if scalar_first {
            let scalar_value = self.lower_expr_value(scalar)?;
            (self.lower_expr_value(vector)?, scalar_value)
        } else {
            let vector_value = self.lower_expr_value(vector)?;
            (vector_value, self.lower_expr_value(scalar)?)
        };
        let checked = self.register_lanes_are_checked(expr);
        self.vector_lift_call(
            MirVectorOp::ElementwiseScalar { op, scalar_first, checked },
            vec![(vector_value, vector), (scalar_value, scalar)],
            expr,
        )
    }

    /// `vector op vector` lane by lane as one deferred
    /// [`MirVectorOp::Elementwise`].
    fn lower_vector_pair(
        &mut self,
        op: MirLaneOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let lhs_value = self.lower_expr_value(lhs)?;
        let rhs_value = self.lower_expr_value(rhs)?;
        let checked = self.register_lanes_are_checked(expr);
        self.vector_lift_call(
            MirVectorOp::Elementwise { op, checked },
            vec![(lhs_value, lhs), (rhs_value, rhs)],
            expr,
        )
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

    /// The element and literal shape of a matrix result, within the 4x4 CPU
    /// register subset; records a diagnostic and returns `None` otherwise.
    fn matrix_lift_shape(&mut self, expr: &HirExpression) -> Option<(TypeId, u8, u8)> {
        self.matrix_shape_of(expr.ty?, expr.span)
    }

    /// [`Self::matrix_lift_shape`] for a matrix type.
    fn matrix_shape_of(&mut self, ty: TypeId, span: Span) -> Option<(TypeId, u8, u8)> {
        let payload = self.types.peel_access_marker(ty).1;
        let Type::Matrix(element, shape) = self.normalized_type(payload).clone() else {
            return None;
        };
        let Some((rows, columns)) = radix_semantic::literal_matrix_shape(shape, self.types) else {
            self.errors.push(MirError::unsupported(
                span,
                "matrix lift with non-literal shape before MIR lowering",
            ));
            return None;
        };
        if rows > 4 || columns > 4 {
            self.errors
                .push(MirError::unsupported(span, "matrix lift exceeds 4x4 CPU subset"));
            return None;
        }
        Some((element, rows, columns))
    }

    /// Whether an expression-valued `si` is a select over a bool vector or
    /// matrix mask.
    pub(super) fn is_register_select(&self, cond: &HirExpression) -> bool {
        matches!(self.lift_operand(cond), LiftOperand::Vector | LiftOperand::Matrix)
    }

    /// Lower the `✓ ✗` select over a bool register mask: the mask and both
    /// branch values are evaluated, then each lane takes the branch its mask
    /// lane selects. A `vector` is one deferred [`MirVectorOp::Select`]; a
    /// `matrix` has a literal shape, so each cell branches here.
    pub(super) fn lower_register_select(
        &mut self,
        cond: &HirExpression,
        (then_block, else_block): (&HirBlock, &HirBlock),
        destination: MirPlace,
        ty: MirType,
        span: Span,
    ) -> Option<()> {
        let (Some(then_value), Some(else_value)) = (then_block.expr.as_deref(), else_block.expr.as_deref()) else {
            self.errors.push(MirError::unsupported(
                span,
                "register select branch without a value before MIR lowering",
            ));
            return None;
        };
        let value = if self.lift_operand(cond) == LiftOperand::Vector {
            let mask = self.lower_expr_value(cond)?;
            let then_operand = self.lower_expr_value(then_value)?;
            let else_operand = self.lower_expr_value(else_value)?;
            let mut args = Vec::with_capacity(3);
            for (operand, source) in [(mask, cond), (then_operand, then_value), (else_operand, else_value)] {
                let operand_ty = source.ty.map_or(ty, MirType::semantic);
                args.push(self.vector_operand_place(operand, operand_ty, source.span));
            }
            self.runtime_call_value(MirIntrinsic::VectorOp(MirVectorOp::Select), args, ty, span)
        } else {
            self.lower_matrix_select(cond, (then_value, else_value), ty, span)?
        };
        self.assign(destination, value, ty, span);
        Some(())
    }

    /// `mask ✓ then ✗ else` over a bool matrix: for each cell a branch picks
    /// the then cell (or the shared scalar) or the else cell, into a new
    /// matrix.
    fn lower_matrix_select(
        &mut self,
        cond: &HirExpression,
        (then_value, else_value): (&HirExpression, &HirExpression),
        ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let (element, rows, columns) = self.matrix_shape_of(ty.semantic_id(), span)?;
        let element = MirType::semantic(element);
        let mask_ty = self.lift_lane_ty(cond)?;
        let LaneSource::Cells(mask) = self.matrix_lift_source(cond)? else {
            return None;
        };
        let then_source = self.matrix_lift_source(then_value)?;
        let else_source = self.matrix_lift_source(else_value)?;
        let cell = |source: &LaneSource, row: u8, column: u8| match source {
            LaneSource::Cells(place) => matrix_cell_operand(place, row, column),
            LaneSource::Scalar(value) => value.clone(),
        };
        let mut fields = Vec::with_capacity(usize::from(rows) * usize::from(columns));
        for row in 0..rows {
            for column in 0..columns {
                let result = self.push_temp(element, span);
                let result_place = MirPlace::temp(result);
                let condition =
                    self.assign_temp(MirValueKind::Operand(matrix_cell_operand(&mask, row, column)), mask_ty, span);
                let then_id = self.fresh_block(span);
                let else_id = self.fresh_block(span);
                let join_id = self.fresh_block(span);
                self.terminate_current(
                    MirTerminatorKind::Branch { condition, then_block: then_id, else_block: else_id },
                    span,
                );
                for (arm_id, source) in [(then_id, &then_source), (else_id, &else_source)] {
                    self.switch_to(arm_id);
                    self.assign(result_place.clone(), cell(source, row, column), element, span);
                    self.terminate_current(MirTerminatorKind::Goto(join_id), span);
                }
                self.switch_to(join_id);
                fields.push(MirAggregateItem::Operand(MirOperand::Temp(result)));
            }
        }
        Some(self.construct_temp(MirAggregateKind::Matrix, MirAggregateFields::Ordered(fields), ty, span))
    }

    /// Lower one operand of a matrix lift: a matrix as a place its cells can
    /// be read from, a scalar as its value.
    fn matrix_lift_source(&mut self, operand: &HirExpression) -> Option<LaneSource> {
        let value = self.lower_expr_value(operand)?;
        if self.lift_operand(operand) == LiftOperand::Matrix {
            let ty = MirType::semantic(operand.ty?);
            return Some(LaneSource::Cells(self.value_base_place(value, ty, operand.span)));
        }
        Some(LaneSource::Scalar(value))
    }

    /// `matrix op scalar`, `scalar op matrix`, or `matrix op matrix` cell by
    /// cell, as a new matrix.
    fn lower_matrix_lift(
        &mut self,
        op: MirLaneOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let (element, rows, columns) = self.matrix_lift_shape(expr)?;
        // The source order of evaluation is kept: the left operand first.
        let lhs_source = self.matrix_lift_source(lhs)?;
        let rhs_source = self.matrix_lift_source(rhs)?;
        let result_ty = self.expr_ty(expr)?;
        let lane_op = LaneOp {
            op,
            checked: self.register_lanes_are_checked(expr),
            lane_tys: (self.lift_lane_ty(lhs)?, self.lift_lane_ty(rhs)?),
            element: MirType::semantic(element),
            types: self.types,
        };
        Some(self.matrix_lane_cells(&lane_op, (rows, columns), (&lhs_source, &rhs_source), result_ty, expr.span))
    }

    /// A unary operator over a matrix (`-`, or `non` over a bool matrix): each
    /// cell transformed and stored under the element type, as a new matrix.
    fn lower_matrix_unary(&mut self, op: MirUnOp, operand: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        let (element, rows, columns) = self.matrix_lift_shape(expr)?;
        let source = self.matrix_lift_source(operand)?;
        let LaneSource::Cells(place) = source else {
            return None;
        };
        let element = MirType::semantic(element);
        let checked = self.register_lanes_are_checked(expr);
        let result_ty = self.expr_ty(expr)?;
        let mut fields = Vec::with_capacity(usize::from(rows) * usize::from(columns));
        for row in 0..rows {
            for column in 0..columns {
                let cell = matrix_cell_operand(&place, row, column);
                let value = self.assign_temp(MirValueKind::Unary { op, operand: cell }, element, expr.span);
                let mut lanes = MatrixLanes { builder: self, span: expr.span };
                fields.push(MirAggregateItem::Operand(stored_lane(&mut lanes, value, element, checked)));
            }
        }
        Some(self.construct_temp(
            MirAggregateKind::Matrix,
            MirAggregateFields::Ordered(fields),
            result_ty,
            expr.span,
        ))
    }

    /// One matrix aggregate whose cell `(r, c)` is `lane_op` over the two
    /// sources' cell `(r, c)` (a scalar source reads the same value every
    /// time). Row-major, as the register layout is.
    pub(super) fn matrix_lane_cells(
        &mut self,
        lane_op: &LaneOp<'_>,
        (rows, columns): (u8, u8),
        (lhs, rhs): (&LaneSource, &LaneSource),
        matrix_ty: MirType,
        span: Span,
    ) -> MirOperand {
        let cell = |source: &LaneSource, row: u8, column: u8| match source {
            LaneSource::Cells(place) => matrix_cell_operand(place, row, column),
            LaneSource::Scalar(value) => value.clone(),
        };
        let mut fields = Vec::with_capacity(usize::from(rows) * usize::from(columns));
        for row in 0..rows {
            for column in 0..columns {
                let mut lanes = MatrixLanes { builder: self, span };
                let value = lane_op.apply(&mut lanes, cell(lhs, row, column), cell(rhs, row, column));
                fields.push(MirAggregateItem::Operand(value));
            }
        }
        self.construct_temp(MirAggregateKind::Matrix, MirAggregateFields::Ordered(fields), matrix_ty, span)
    }
}
