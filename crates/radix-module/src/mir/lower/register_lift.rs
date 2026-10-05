//! Operators lifted over a register `vector` or `matrix` (gpu-reset rule 15.1, K4).
//!
//! The typechecker admits `+ - * / % ÷` between a register and a scalar (either
//! side), unary `-`, and `÷`, `⤒`, `⤓` between two registers of one shape. The
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

/// What an operand is to the register lift.
#[derive(PartialEq, Eq)]
enum LiftOperand {
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
struct MatrixLanes<'a, 'b> {
    builder: &'a mut FunctionBuilder<'b>,
    span: Span,
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
    fn lift_operand(&self, expr: &HirExpression) -> LiftOperand {
        let Some(ty) = expr.ty else {
            return LiftOperand::Other;
        };
        let payload = self.types.peel_access_marker(ty).1;
        match self.normalized_type(payload) {
            Type::Vector(..) => LiftOperand::Vector,
            Type::Matrix(..) => LiftOperand::Matrix,
            Type::Primitive(Primitive::Numerus | Primitive::Fractus) | Type::SizedNumeric(..) => LiftOperand::Scalar,
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
            _ => return None,
        };
        match (self.lift_operand(lhs), self.lift_operand(rhs)) {
            (LiftOperand::Vector, LiftOperand::Scalar) => Some(self.lower_vector_lift(lane, (lhs, rhs), false, expr)),
            (LiftOperand::Scalar, LiftOperand::Vector) => Some(self.lower_vector_lift(lane, (rhs, lhs), true, expr)),
            (LiftOperand::Vector, LiftOperand::Vector) if lane == MirLaneOp::TrueDiv => {
                Some(self.lower_vector_pair(lane, lhs, rhs, expr))
            }
            (LiftOperand::Matrix, LiftOperand::Scalar) | (LiftOperand::Scalar, LiftOperand::Matrix) => {
                Some(self.lower_matrix_lift(lane, lhs, rhs, expr))
            }
            (LiftOperand::Matrix, LiftOperand::Matrix) if lane == MirLaneOp::TrueDiv => {
                Some(self.lower_matrix_lift(lane, lhs, rhs, expr))
            }
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
            LiftOperand::Matrix => Some(self.lower_matrix_negate(operand, expr)),
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
        let ty = expr.ty?;
        let payload = self.types.peel_access_marker(ty).1;
        let Type::Matrix(element, shape) = self.normalized_type(payload).clone() else {
            return None;
        };
        let Some((rows, columns)) = crate::semantic::literal_matrix_shape(shape, self.types) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "matrix lift with non-literal shape before MIR lowering",
            ));
            return None;
        };
        if rows > 4 || columns > 4 {
            self.errors
                .push(MirError::unsupported(expr.span, "matrix lift exceeds 4x4 CPU subset"));
            return None;
        }
        Some((element, rows, columns))
    }

    /// Lower one operand of a matrix lift: a matrix as a place its cells can
    /// be read from, a scalar as its value.
    fn matrix_lift_source(&mut self, operand: &HirExpression) -> Option<LaneSource> {
        let value = self.lower_expr_value(operand)?;
        if self.lift_operand(operand) == LiftOperand::Matrix {
            let ty = MirType::semantic(operand.ty?);
            return Some(LaneSource::Cells(self.value_base_place(value, ty, operand.span)?));
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

    /// Unary `-` over a matrix: each cell negated and stored under the
    /// element type, as a new matrix.
    fn lower_matrix_negate(&mut self, operand: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
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
                let value =
                    self.assign_temp(MirValueKind::Unary { op: MirUnOp::Neg, operand: cell }, element, expr.span);
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
