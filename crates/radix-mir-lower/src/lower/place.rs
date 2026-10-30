//! Place and projection lowering for addressable MIR operands.
//!
//! This module decides when a HIR expression can be represented as a `MirPlace`
//! and when a value must first be materialized into a temporary. Field and index
//! access preserve addressability whenever the base resolves to a local, temp,
//! or nested projection; constants and value operands are assigned into temps so
//! later projections still have a stable MIR base.
//!
//! NULLABILITY
//! ===========
//! Optional chaining and non-null access are lowered as MIR option operations.
//! This file does not infer nullability: it consumes the expression type
//! assigned by semantic typechecking and reports missing type information
//! through the shared lowering error path.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;

impl FunctionBuilder<'_> {
    /// Lower field access into a projected place when the receiver can be made
    /// addressable.
    pub(super) fn lower_field(
        &mut self,
        object: &HirExpression,
        name: radix_lexer::Symbol,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if self.is_handler_error_nuntius_projection(object, name) {
            return self.lower_expr_value(object);
        }
        if let Some(result) = self.lower_union_field_read(object, name, expr) {
            return result;
        }
        let mut place = self.lower_projectable_place(object)?;
        place.projections.push(MirProjection::Field(name));
        Some(MirOperand::Place(place))
    }

    /// String-backed handler errors expose `.nuntius` as an identity projection.
    fn is_handler_error_nuntius_projection(&self, object: &HirExpression, field: radix_lexer::Symbol) -> bool {
        let Some(interner) = self.context.interner else {
            return false;
        };
        if interner.resolve(field) != radix_mir::HANDLER_MESSAGE_FIELD {
            return false;
        }
        let Some(ty) = object.ty else {
            return false;
        };
        matches!(
            self.normalized_type(ty),
            Type::Primitive(Primitive::Textus | Primitive::Ignotum)
        )
    }

    /// Lower index access into a projected place, keeping the index as a MIR
    /// operand because it may itself require computation.
    pub(super) fn lower_index(
        &mut self,
        object: &HirExpression,
        index: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let object_is_coordinate = object
            .ty
            .is_some_and(|ty| radix_semantic::tensor_bracket::is_coordinate_bracket_receiver(self.types, ty));
        // WHY: kernel slice lowering is deliberately not in need, so a
        // range bracket on the device path fails loudly instead of lowering a
        // range value as a projection.
        if self.is_device_lowering() && object_is_coordinate && tensor_bracket_index_has_range(index) {
            self.errors.push(MirError::unsupported(
                index.span,
                "kernel bracket range slice is not lowered on the device path",
            ));
            return None;
        }
        // WHY: tensor bracket read (`grid[idx]`) is sugar for `accipe`. Desugar
        // to the `TensorGet` collection op so MIR reuses the existing tensor
        // runtime path instead of a second `MirProjection::Index` protocol,
        // which validate rejects for tensors (DEFER-063).
        if !self.is_device_lowering() && object_is_coordinate {
            // WHY: a range index (`t[1 ‥ 4]`) is the trapping slice view of
            // need, not a coordinate read.
            if matches!(index.kind, HirExpressionKind::Intervallum { .. }) {
                return self.lower_tensor_bracket_slice(object, index, expr);
            }
            // WHY: a slot list with a range slot (`g[[1, 0 ‥ 3]]`) retains and
            // drops axes per slot; an all-integer slot list stays a coordinate
            // read. A matrix is a register aggregate, not a tensor, so its
            // slice is built from cell reads rather than `TensorSliceNd`.
            if tensor_bracket_index_has_range(index) {
                let object_is_matrix = object.ty.is_some_and(|ty| {
                    matches!(self.normalized_type(self.types.peel_access_marker(ty).1), Type::Matrix(..))
                });
                if object_is_matrix {
                    return self.lower_matrix_bracket_slot_slice(object, index, expr);
                }
                return self.lower_tensor_bracket_slot_slice(object, index, expr);
            }
            return self.lower_tensor_bracket_get(object, index, expr);
        }
        // WHY: `xs[lo‥hi]` over a list is a window (ruling B10 of need
        // 6a13d711): a copied `lista<T>` read through the `Index` collection
        // op, whose declared return type tells the runner it is a window and
        // not an octeti carrier read.
        if self.is_list_window(object, index) {
            let receiver = self.lower_expr_value(object)?;
            let range = self.lower_expr_value(index)?;
            let ty = self.expr_ty(expr)?;
            return Some(self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::Index),
                vec![receiver, range],
                ty,
                expr.span,
            ));
        }
        // WHY: a register vector is not a lane base for a range: `v[1 ‥ 3]`
        // stays in the vector family at the width the checker computes, so it
        // builds a fresh vector from the kept lanes instead of reading a lane
        //.
        if object.ty.is_some_and(|ty| self.is_vector_lane_base(ty))
            && matches!(index.kind, HirExpressionKind::Intervallum { .. })
        {
            return self.lower_vector_bracket_slice(object, index, expr);
        }
        let mut place = self.lower_projectable_place(object)?;
        let projection = self.lower_index_projection(&place, object.ty, index, expr.id)?;
        place.projections.push(projection);
        Some(MirOperand::Place(place))
    }

    /// Lower a vector bracket range (`v[1 ‥ 3]`, `v[0 ‥ 4 per 2]`) as a fresh
    /// vector of its kept lanes.
    ///
    /// The checker only accepts a range whose start, end and step are static
    /// literals (a register vector carries a literal width), so the kept lanes
    /// are enumerated here and each one read through the `VectorLane`
    /// projection. A non-literal bound cannot reach this path; if it does, it
    /// fails loudly instead of lowering a range value as a lane index.
    fn lower_vector_bracket_slice(
        &mut self,
        object: &HirExpression,
        index: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let HirExpressionKind::Intervallum { start, end, step, kind } = &index.kind else {
            self.errors
                .push(MirError::unsupported(index.span, "vector bracket range needs literal bounds"));
            return None;
        };
        let lanes = self.literal_range_coordinates(
            start,
            end,
            step.as_deref(),
            matches!(kind, HirRangeKind::Inclusive),
            index.span,
            "vector bracket range needs literal bounds",
            "vector bracket range lane out of range",
        )?;
        let place = self.lower_projectable_place(object)?;
        let items = lanes
            .into_iter()
            .map(|lane| {
                // WHY: the lane read is the same `VectorLane` projection a
                // swizzle uses (`vector_lane_operand` is private to the runtime
                // module; the projection is identical).
                let mut lane_place = place.clone();
                lane_place.projections.push(MirProjection::VectorLane(lane));
                MirAggregateItem::Operand(MirOperand::Place(lane_place))
            })
            .collect();
        let ty = self.expr_ty(expr)?;
        Some(self.construct_temp(MirAggregateKind::Vector, MirAggregateFields::Ordered(items), ty, expr.span))
    }

    /// Whether `object[index]` is a list window: a list receiver and a range
    /// index.
    fn is_list_window(&self, object: &HirExpression, index: &HirExpression) -> bool {
        let (Some(object_ty), Some(index_ty)) = (object.ty, index.ty) else {
            return false;
        };
        let object_ty = self.types.peel_access_marker(object_ty).1;
        matches!(
            self.normalized_type(object_ty),
            Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. }
        ) && matches!(self.normalized_type(index_ty), Type::Intervallum(..))
    }

    /// The projection of `base[index]`.
    ///
    /// A vector is a register value, never a storage buffer. A constant lane
    /// is the `VectorLane` projection swizzle uses; a run-time index stays an
    /// `Index` projection on the vector base, which MIR defines as the checked
    /// lane select (the checked-index law: `0 <= i < N`, else the same failure
    /// as an out-of-range `lista` index).
    pub(super) fn lower_index_projection(
        &mut self,
        base: &MirPlace,
        base_ty: Option<TypeId>,
        index: &HirExpression,
        access: radix_hir::HirId,
    ) -> Option<MirProjection> {
        if base_ty.is_some_and(|ty| self.is_vector_lane_base(ty))
            && let Some(lane) = radix_semantic::passes::const_int_value(index).and_then(|lane| u8::try_from(lane).ok())
        {
            return Some(MirProjection::VectorLane(lane));
        }
        // The checker's proof for this access (every component against its own
        // container or axis) rides on the projection; nothing here derives one.
        let proven = self.context.proven_indexes.contains(&access);
        let index_projection = |operand| {
            if proven {
                MirProjection::ProvenIndex(operand)
            } else {
                MirProjection::Index(operand)
            }
        };
        if let Some(linear) = self.lower_device_tensor_linear_index(base, base_ty, index) {
            return Some(index_projection(linear?));
        }
        Some(index_projection(self.lower_expr_value(index)?))
    }

    /// Under device lowering, an N-element (N >= 1) bracket index list into a rank-N
    /// tensor is ONE row-major linear integer index into the device view's
    /// flat storage: `((i0 * e1 + i1) * e2 + i2) ...`, where `e_k` is the
    /// extent of axis `k`. The outer `Option` is `None` when this lowering
    /// does not apply (host lowering, non-tensor base, non-literal index list).
    ///
    /// A literal axis extent folds to a constant; any other axis reads its
    /// extent at run time through `TensorExtent { tensor: base, axis }`.
    fn lower_device_tensor_linear_index(
        &mut self,
        base: &MirPlace,
        base_ty: Option<TypeId>,
        index: &HirExpression,
    ) -> Option<Option<MirOperand>> {
        if !self.is_device_lowering() {
            return None;
        }
        let HirExpressionKind::Array(items) = &index.kind else {
            return None;
        };
        let elements: Vec<&HirExpression> = items
            .iter()
            .map(|item| match item {
                HirArrayElement::Expr(expr) => Some(expr),
                HirArrayElement::Spread(_) | HirArrayElement::Infer(_) => None,
            })
            .collect::<Option<_>>()?;
        let mut ty = self.types.peel_access_marker(base_ty?).1;
        if let Type::Ref(_, inner) = self.normalized_type(ty) {
            ty = *inner;
        }
        let Type::Tensor(_, shape) = self.normalized_type(ty) else {
            return None;
        };
        let shape = *shape;
        // An empty index list has no linear form; the ordinary lowering takes it.
        if elements.is_empty() {
            return None;
        }
        let extents: Vec<Option<i64>> = match self.types.get_index(shape) {
            radix_types::IndexExpr::Tuple(dims) => dims
                .iter()
                .map(|dim| {
                    radix_types::evaluate_index_dimension(*dim, self.types)
                        .ok()
                        .flatten()
                        .and_then(|n| i64::try_from(n).ok())
                })
                .collect(),
            _ => match radix_semantic::figura::i64s_from_literal_index(shape, self.types) {
                Some(literal) => literal.into_iter().map(Some).collect(),
                // WHY: a non-tuple symbolic shape states no rank; the index list does.
                None => vec![None; elements.len()],
            },
        };
        if extents.len() != elements.len() {
            return None;
        }
        Some(self.lower_row_major_linear_index(base, &elements, &extents, index.span))
    }

    /// Emit `((i0 * e1 + i1) * e2 + i2) ...` over the lowered index elements.
    /// A rank-1 list `v[[i]]` is the linear index `i` itself, the same device
    /// form as the scalar spelling `v[i]`.
    pub(super) fn lower_row_major_linear_index(
        &mut self,
        base: &MirPlace,
        elements: &[&HirExpression],
        extents: &[Option<i64>],
        span: Span,
    ) -> Option<MirOperand> {
        let ty = match elements[0].ty {
            Some(ty) => MirType::semantic(ty),
            None => MirType::semantic(self.types.primitive(Primitive::Numerus)),
        };
        let mut linear = self.lower_expr_value(elements[0])?;
        for (axis, (element, extent)) in elements.iter().zip(extents).enumerate().skip(1) {
            let index = self.lower_expr_value(element)?;
            let extent = match extent {
                Some(n) => MirOperand::Constant(MirConstant::Int(*n)),
                None => self.assign_temp(
                    MirValueKind::TensorExtent {
                        tensor: MirOperand::Place(base.clone()),
                        axis: u32::try_from(axis).ok()?,
                    },
                    ty,
                    span,
                ),
            };
            let scaled =
                self.assign_temp(MirValueKind::Binary { op: MirBinOp::Mul, lhs: linear, rhs: extent }, ty, span);
            linear = self.assign_temp(MirValueKind::Binary { op: MirBinOp::Add, lhs: scaled, rhs: index }, ty, span);
        }
        Some(linear)
    }

    /// Whether `ty`, under access markers, aliases, and one optional layer
    /// (the base of `v![k]`), is a `vector<T, size N>`.
    fn is_vector_lane_base(&self, ty: TypeId) -> bool {
        let mut ty = self.types.peel_access_marker(ty).1;
        if let Some(inner) = self.types.nullable_inner(ty) {
            ty = inner;
        }
        matches!(self.normalized_type(ty), Type::Vector(..))
    }

    /// Lower a tensor bracket read (`tensor[`idx`]`) as a `TensorGet` collection
    /// op (sugar for `accipe`). The result type is `Option<T>` (`T ∪ nihil`).
    fn lower_tensor_bracket_get(
        &mut self,
        object: &HirExpression,
        index: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let receiver = self.lower_expr_value(object)?;
        let index_list = self.lower_tensor_index_list_operand(index)?;
        let ty = self.expr_ty(expr)?;
        let call_ty = self
            .checked_read_result_type(MirCollectionOp::TensorGet, object.ty, ty)
            .unwrap_or(ty);
        let value = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorGet),
            vec![receiver, index_list],
            call_ty,
            expr.span,
        );
        if call_ty != ty && self.types.nullable_inner(ty.semantic_id()) == Some(call_ty.semantic_id()) {
            return Some(self.assign_temp(MirValueKind::Option(MirOptionOp::Some(value)), ty, expr.span));
        }
        Some(value)
    }

    /// Lower a rank-1 tensor bracket range (`t[1 ‥ 4]`, `t[0 ‥ 6 step 2]`,
    /// `t[1 … 3]`) as the `TensorSliceNd` collection op: one axis entry per
    /// list, `keep = true` (the range keeps its axis), inclusive flag from the
    /// range kind. Need; the per-slot rank-N form is a later unit.
    fn lower_tensor_bracket_slice(
        &mut self,
        object: &HirExpression,
        index: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let HirExpressionKind::Intervallum { start, end, step, kind } = &index.kind else {
            return self.lower_tensor_bracket_get(object, index, expr);
        };
        let receiver = self.lower_expr_value(object)?;
        let start_op = self.lower_expr_value(start)?;
        let end_op = self.lower_expr_value(end)?;
        let step_op = match step.as_deref() {
            Some(step) => self.lower_expr_value(step)?,
            None => MirOperand::Constant(MirConstant::Int(1)),
        };
        // WHY: a range used only as a bracket index is not typed by the check
        // pass, so the axis entries take the counter type an ordinary `itera`
        // range defaults to. The list types are interned by
        // `ensure_tensor_bracket_index_array_types`.
        let counter = self.types.primitive(Primitive::Numerus);
        let int_list_ty =
            MirType::semantic(self.require_array(counter, index.span, "tensor bracket range index list")?);
        let flags = self.types.primitive(Primitive::Bivalens);
        let bool_list_ty =
            MirType::semantic(self.require_array(flags, index.span, "tensor bracket range flag list")?);
        let starts = self.one_element_list(start_op, int_list_ty, index.span);
        let ends = self.one_element_list(end_op, int_list_ty, index.span);
        let steps = self.one_element_list(step_op, int_list_ty, index.span);
        let keeps = self.one_element_list(MirOperand::Constant(MirConstant::Bool(true)), bool_list_ty, index.span);
        let inclusives = self.one_element_list(
            MirOperand::Constant(MirConstant::Bool(matches!(kind, HirRangeKind::Inclusive))),
            bool_list_ty,
            index.span,
        );
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorSliceNd),
            vec![receiver, starts, ends, steps, keeps, inclusives],
            ty,
            expr.span,
        ))
    }

    /// Lower a rank-N tensor bracket slot list (`g[[1, 0 ‥ 3]]`) as the
    /// `TensorSliceNd` collection op: one axis entry per slot, in receiver
    /// axis order. A range slot keeps its axis; an integer slot selects one
    /// coordinate and drops its axis.
    fn lower_tensor_bracket_slot_slice(
        &mut self,
        object: &HirExpression,
        index: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let HirExpressionKind::Array(elements) = &index.kind else {
            return self.lower_tensor_bracket_slice(object, index, expr);
        };
        let receiver = self.lower_expr_value(object)?;
        // WHY: a range used only as a bracket index is not typed by the check
        // pass, so the axis entries take the counter type an ordinary `itera`
        // range defaults to. The list types are interned by
        // `ensure_tensor_bracket_index_array_types`.
        let counter = self.types.primitive(Primitive::Numerus);
        let int_list_ty =
            MirType::semantic(self.require_array(counter, index.span, "tensor bracket slot index list")?);
        let flags = self.types.primitive(Primitive::Bivalens);
        let bool_list_ty = MirType::semantic(self.require_array(flags, index.span, "tensor bracket slot flag list")?);
        let mut starts = Vec::with_capacity(elements.len());
        let mut ends = Vec::with_capacity(elements.len());
        let mut steps = Vec::with_capacity(elements.len());
        let mut keeps = Vec::with_capacity(elements.len());
        let mut inclusives = Vec::with_capacity(elements.len());
        for element in elements {
            let HirArrayElement::Expr(slot) = element else {
                self.errors.push(MirError::unsupported(
                    index.span,
                    "tensor bracket slot list with a non-slot element",
                ));
                return None;
            };
            if let HirExpressionKind::Intervallum { start, end, step, kind } = &slot.kind {
                starts.push(self.lower_expr_value(start)?);
                ends.push(self.lower_expr_value(end)?);
                steps.push(match step.as_deref() {
                    Some(step) => self.lower_expr_value(step)?,
                    None => MirOperand::Constant(MirConstant::Int(1)),
                });
                keeps.push(MirOperand::Constant(MirConstant::Bool(true)));
                inclusives.push(MirOperand::Constant(MirConstant::Bool(matches!(kind, HirRangeKind::Inclusive))));
            } else {
                // WHY: a dropped axis is a one-cell window: the runner folds the
                // inclusive flag into `end_eff = end + 1`, so the slot value is
                // both bounds and no add is emitted here.
                let value = self.lower_expr_value(slot)?;
                starts.push(value.clone());
                ends.push(value);
                steps.push(MirOperand::Constant(MirConstant::Int(1)));
                keeps.push(MirOperand::Constant(MirConstant::Bool(false)));
                inclusives.push(MirOperand::Constant(MirConstant::Bool(true)));
            }
        }
        let starts = self.element_list(starts, int_list_ty, index.span);
        let ends = self.element_list(ends, int_list_ty, index.span);
        let steps = self.element_list(steps, int_list_ty, index.span);
        let keeps = self.element_list(keeps, bool_list_ty, index.span);
        let inclusives = self.element_list(inclusives, bool_list_ty, index.span);
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorSliceNd),
            vec![receiver, starts, ends, steps, keeps, inclusives],
            ty,
            expr.span,
        ))
    }

    /// Lower a matrix slot list with a range slot (`m[[1, 0 ‥ 3]]`) as a fresh
    /// register aggregate of cell reads.
    ///
    /// A matrix is rank 2 and its cells are addressed by the `MatrixCell`
    /// projection, so the kept row and column coordinates are enumerated here
    /// from the literal slots. One range slot keeps a row or a column as a
    /// `vector`; two range slots keep a block as a `matrix`, in row-major
    /// order. The checker only accepts literal bounds on this path; a
    /// non-literal bound fails loudly instead of guessing a shape.
    fn lower_matrix_bracket_slot_slice(
        &mut self,
        object: &HirExpression,
        index: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let HirExpressionKind::Array(elements) = &index.kind else {
            self.errors.push(MirError::unsupported(
                index.span,
                "matrix bracket slot list needs a slot per axis",
            ));
            return None;
        };
        let [HirArrayElement::Expr(row_slot), HirArrayElement::Expr(column_slot)] = elements.as_slice() else {
            self.errors.push(MirError::unsupported(
                index.span,
                "matrix bracket slot list needs one slot per axis",
            ));
            return None;
        };
        let rows = self.matrix_slot_indices(row_slot, index.span)?;
        let columns = self.matrix_slot_indices(column_slot, index.span)?;
        let row_is_range = matches!(row_slot.kind, HirExpressionKind::Intervallum { .. });
        let column_is_range = matches!(column_slot.kind, HirExpressionKind::Intervallum { .. });
        if !row_is_range && !column_is_range {
            // WHY: an all-integer slot list is a coordinate read; it cannot
            // reach here through `lower_index`, but keep the fallback honest.
            return self.lower_tensor_bracket_get(object, index, expr);
        }
        let place = self.lower_projectable_place(object)?;
        let ty = self.expr_ty(expr)?;
        let cells = |rows: &[u8], columns: &[u8]| {
            let mut items = Vec::with_capacity(rows.len().saturating_mul(columns.len()));
            for &row in rows {
                for &column in columns {
                    items.push(MirAggregateItem::Operand(runtime::matrix_cell_operand(&place, row, column)));
                }
            }
            items
        };
        let kind = if row_is_range && column_is_range {
            // WHY: two kept axes form a block; the cells are already in
            // row-major order.
            MirAggregateKind::Matrix
        } else {
            // WHY: one kept axis is a row or a column.
            MirAggregateKind::Vector
        };
        Some(self.construct_temp(kind, MirAggregateFields::Ordered(cells(&rows, &columns)), ty, expr.span))
    }

    /// Enumerate the literal coordinates a matrix bracket slot keeps: a range
    /// slot expands start to end by step, an integer slot is one coordinate.
    fn matrix_slot_indices(&mut self, slot: &HirExpression, span: Span) -> Option<Vec<u8>> {
        if let HirExpressionKind::Intervallum { start, end, step, kind } = &slot.kind {
            return self.literal_range_coordinates(
                start,
                end,
                step.as_deref(),
                matches!(kind, HirRangeKind::Inclusive),
                span,
                "matrix bracket range needs literal bounds",
                "matrix bracket slot coordinate out of range",
            );
        }
        let Some(value) = radix_semantic::passes::const_int_value(slot) else {
            self.errors
                .push(MirError::unsupported(span, "matrix bracket slot needs a literal coordinate"));
            return None;
        };
        let Ok(value) = u8::try_from(value) else {
            self.errors
                .push(MirError::unsupported(span, "matrix bracket slot coordinate out of range"));
            return None;
        };
        Some(vec![value])
    }

    /// Lower a statically bounded range to the vector lanes or matrix
    /// coordinates it selects.
    fn literal_range_coordinates(
        &mut self,
        start: &HirExpression,
        end: &HirExpression,
        step: Option<&HirExpression>,
        inclusive: bool,
        span: Span,
        bounds_error: &'static str,
        coordinate_error: &'static str,
    ) -> Option<Vec<u8>> {
        let step = step.map_or(Some(1), radix_semantic::passes::const_int_value);
        let (Some(start), Some(end), Some(step)) = (
            radix_semantic::passes::const_int_value(start),
            radix_semantic::passes::const_int_value(end),
            step,
        ) else {
            self.errors.push(MirError::unsupported(span, bounds_error));
            return None;
        };
        let Some(coordinates) = enumerate_literal_range(start, end, step, inclusive) else {
            self.errors.push(MirError::unsupported(span, bounds_error));
            return None;
        };
        let mut indices = Vec::with_capacity(coordinates.len());
        for coordinate in coordinates {
            let Ok(coordinate) = u8::try_from(coordinate) else {
                self.errors
                    .push(MirError::unsupported(span, coordinate_error));
                return None;
            };
            indices.push(coordinate);
        }
        Some(indices)
    }

    /// A one-element `lista` aggregate operand (one `TensorSliceNd` axis entry).
    fn one_element_list(&mut self, operand: MirOperand, list_ty: MirType, span: Span) -> MirOperand {
        self.element_list(vec![operand], list_ty, span)
    }

    /// An N-element `lista` aggregate operand (one entry per `TensorSliceNd` axis).
    fn element_list(&mut self, operands: Vec<MirOperand>, list_ty: MirType, span: Span) -> MirOperand {
        self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(
                operands
                    .into_iter()
                    .map(MirAggregateItem::Operand)
                    .collect(),
            ),
            list_ty,
            span,
        )
    }

    /// Lower a tensor bracket index to the list operand `TensorGet` / `TensorSet`
    /// expect. A scalar integer index is wrapped into a one-element array; a
    /// list-shaped index is lowered as-is.
    pub(super) fn lower_tensor_index_list_operand(&mut self, index: &HirExpression) -> Option<MirOperand> {
        if radix_semantic::tensor_bracket::is_bracket_index_list_expr(index, self.types) {
            return self.lower_expr_value(index);
        }
        // Scalar → wrap into a one-element `lista` aggregate at the MIR boundary.
        let scalar = self.lower_expr_value(index)?;
        let elem_ty = index.ty.map(MirType::semantic)?;
        let Some(list_ty) = self.types.find_array(elem_ty.semantic_id()) else {
            self.errors
                .push(MirError::missing_type(index.span, "tensor scalar bracket index list wrapper"));
            return None;
        };
        let list_ty = MirType::semantic(list_ty);
        Some(self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(vec![MirAggregateItem::Operand(scalar)]),
            list_ty,
            index.span,
        ))
    }

    /// Shallow `de`/`in` reference: preserve the addressable place of the inner
    /// expression without re-running borrow analysis.
    pub(super) fn lower_ref(&mut self, inner: &HirExpression, _expr: &HirExpression) -> Option<MirOperand> {
        let place = self.lower_projectable_place(inner)?;
        Some(MirOperand::Place(place))
    }

    /// Shallow dereference: load the referent value from an addressable place.
    pub(super) fn lower_deref(&mut self, inner: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        match self.lower_expr_value(inner)? {
            MirOperand::Place(place) => {
                Some(self.assign_temp(MirValueKind::Operand(MirOperand::Place(place)), ty, expr.span))
            }
            operand => Some(operand),
        }
    }

    /// The place of a record tensor element read with a coordinate bracket
    /// (`t[[i]]` over `tensor<Genus, …>`), as the decoded copy a `TensorGet`
    /// returns: the host tensor bracket is the `TensorGet` op, never an
    /// `Index` projection (validate refuses that for tensors). While an
    /// assignment target is lowered, the element is recorded so the caller
    /// stores it back after the field store (see `element_writebacks`).
    /// `None` is "not this case"; `Some(None)` is a lowering failure.
    fn lower_record_tensor_element_place(
        &mut self,
        object: &HirExpression,
        index: &HirExpression,
        expr: &HirExpression,
    ) -> Option<Option<MirPlace>> {
        if self.is_device_lowering()
            || !object
                .ty
                .is_some_and(|ty| radix_semantic::tensor_bracket::is_coordinate_bracket_receiver(self.types, ty))
            || matches!(index.kind, HirExpressionKind::Intervallum { .. })
            || tensor_bracket_index_has_range(index)
        {
            return None;
        }
        let ty = expr.ty?;
        if !matches!(self.normalized_type(self.types.peel_access_marker(ty).1), Type::Struct(..)) {
            return None;
        }
        // The receiver and the coordinates are evaluated once, outside any
        // enclosing target's write-back collection.
        let collecting = self.element_writebacks.take();
        let operands = self
            .lower_expr_value(object)
            .and_then(|receiver| Some((receiver, self.lower_tensor_index_list_operand(index)?)));
        self.element_writebacks = collecting;
        let Some((receiver, index_list)) = operands else {
            return Some(None);
        };
        let ty = MirType::semantic(ty);
        let value = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorGet),
            vec![receiver.clone(), index_list.clone()],
            ty,
            expr.span,
        );
        let element = self.materialize_operand_place(value, ty, expr.span);
        if let Some(writebacks) = self.element_writebacks.as_mut() {
            writebacks.push(ElementWriteback { receiver, index: index_list, element: element.clone() });
        }
        Some(Some(element))
    }

    pub(super) fn lower_projectable_place(&mut self, expr: &HirExpression) -> Option<MirPlace> {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                if let Some(place) = self.captured_bindings.get(def_id).cloned() {
                    let storage_ty = self
                        .captured_binding_metadata
                        .get(def_id)
                        .map(|(_, ty)| *ty);
                    return self.adapt_narrowed_place(place, storage_ty, expr);
                }
                let Some(binding) = self.bindings.get(def_id).copied() else {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "projection base that does not resolve to a local value",
                    ));
                    return None;
                };
                self.adapt_narrowed_place(MirPlace::local(binding.local), Some(binding.ty), expr)
            }
            HirExpressionKind::Field(object, name) => {
                // A folded imported module constant (`module.NAME`) has no
                // local base to project from: materialize its value first.
                if let HirExpressionKind::Path(module) = &object.kind
                    && self
                        .context
                        .imported_constants
                        .contains_key(&(*module, *name))
                {
                    let ty = self.expr_ty(expr)?;
                    let operand = self.lower_expr_value(expr)?;
                    return Some(self.materialize_operand_place(operand, ty, expr.span));
                }
                if let Some(result) = self.lower_union_field_read(object, *name, expr) {
                    let ty = self.expr_ty(expr)?;
                    let operand = result?;
                    return Some(self.materialize_operand_place(operand, ty, expr.span));
                }
                let mut place = self.lower_projectable_place(object)?;
                place.projections.push(MirProjection::Field(*name));
                Some(place)
            }
            HirExpressionKind::Index(object, index) => {
                if let Some(result) = self.lower_record_tensor_element_place(object, index, expr) {
                    return result;
                }
                let mut place = self.lower_projectable_place(object)?;
                let projection = self.lower_index_projection(&place, object.ty, index, expr.id)?;
                place.projections.push(projection);
                Some(place)
            }
            _ => {
                let ty = self.expr_ty(expr)?;
                let operand = self.lower_expr_value(expr)?;
                match operand {
                    MirOperand::Place(place) => Some(place),
                    MirOperand::Temp(temp) => Some(MirPlace::temp(temp)),
                    MirOperand::Constant(_) | MirOperand::Value(_) => {
                        Some(self.assign_temp_place(MirValueKind::Operand(operand), ty, expr.span))
                    }
                }
            }
        }
    }

    /// Lower an optional-chain expression into an option runtime value.
    ///
    /// The base and chain link remain explicit so later codegen can preserve
    /// short-circuit semantics instead of seeing a plain field or call.
    pub(super) fn lower_optional_chain(
        &mut self,
        object: &HirExpression,
        chain: &HirOptionalChainKind,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let base = self.lower_expr_value(object)?;
        let ty = self.expr_ty(expr)?;
        let operands_effect_free = match chain {
            HirOptionalChainKind::Member(_) => true,
            HirOptionalChainKind::Index(index) => index.is_effect_free_operand(),
            HirOptionalChainKind::Call(args) => args.iter().all(|arg| arg.expr.is_effect_free_operand()),
        };
        self.lower_option_chain_guarded(&base, object.ty, operands_effect_free, ty, expr.span, |this, captured_base| {
            this.lower_optional_chain_link(captured_base.clone(), chain)
        })
    }

    fn lower_optional_chain_link(
        &mut self,
        base: MirOperand,
        chain: &HirOptionalChainKind,
    ) -> Option<MirOptionChainLink> {
        match chain {
            HirOptionalChainKind::Member(name) => Some(MirOptionChainLink::Field(*name)),
            HirOptionalChainKind::Index(index) => Some(MirOptionChainLink::Index(self.lower_expr_value(index)?)),
            HirOptionalChainKind::Call(args) => {
                let mut lowered_args = Vec::with_capacity(args.len());
                for arg in args {
                    lowered_args.push(self.lower_expr_value(&arg.expr)?);
                }
                Some(MirOptionChainLink::Call { callee: MirCallee::Value(base), args: lowered_args })
            }
        }
    }

    /// Lower a non-null assertion (alone as a method receiver, or followed by
    /// a member/index/call) by asserting the base option into a temp-backed
    /// place before applying the requested projection.
    pub(super) fn lower_non_null(
        &mut self,
        object: &HirExpression,
        chain: &HirNonNullKind,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let (mut place, storage) = self.lower_non_null_base(object)?;
        match chain {
            HirNonNullKind::Value => {
                if let Some((storage, storage_ty)) = storage {
                    self.asserted_receivers
                        .push((storage, place.clone(), storage_ty));
                }
                Some(MirOperand::Place(place))
            }
            HirNonNullKind::Member(name) => {
                place.projections.push(MirProjection::Field(*name));
                Some(MirOperand::Place(place))
            }
            HirNonNullKind::Index(index) => {
                let projection = self.lower_index_projection(&place, object.ty, index, expr.id)?;
                place.projections.push(projection);
                Some(MirOperand::Place(place))
            }
            HirNonNullKind::Call(args) => {
                let lowered_args = self.lower_positional_call_args(args)?;
                let ty = self.expr_ty(expr)?;
                Some(self.call_value_operand(MirOperand::Place(place), lowered_args, ty, expr.span))
            }
        }
    }

    /// The asserted payload place of `object`, plus the option storage and its
    /// type when the payload is a temp unwrapped from that storage (a write to
    /// the payload then needs storing back; see `store_back_asserted_receivers`).
    fn lower_non_null_base(&mut self, object: &HirExpression) -> Option<(MirPlace, Option<(MirPlace, MirType)>)> {
        let place = self.lower_projectable_place(object)?;
        if let Some(inner_ty) = self.nullable_inner_ty(object) {
            // Typecheck wraps `sponte` field loads as `T ∪ nihil`, but MIR
            // stores those fields as `T` (omitted construction is `nihil` in
            // the raw slot). Unwrap only when the place is actually an option.
            if self
                .place_ty(&place)
                .is_some_and(|ty| self.types.nullable_inner(ty.semantic_id()).is_none())
            {
                return Some((place, None));
            }
            let storage_ty = MirType::semantic(object.ty?);
            let payload = self.assign_temp_place(
                MirValueKind::Option(MirOptionOp::Unwrap {
                    value: MirOperand::Place(place.clone()),
                    mode: MirOptionUnwrapMode::Assert,
                }),
                inner_ty,
                object.span,
            );
            return Some((payload, Some((place, storage_ty))));
        }
        Some((place, None))
    }

    /// Store each asserted receiver recorded since `mark` back into its option
    /// storage. A mutating method writes the unwrapped payload temp; MIR
    /// targets with value semantics (immutable lists) would otherwise lose the
    /// write, so the storage is re-wrapped from the payload.
    pub(super) fn store_back_asserted_receivers(&mut self, mark: usize, span: Span) {
        let recorded = self.asserted_receivers.split_off(mark);
        for (storage, payload, storage_ty) in recorded {
            let wrapped = self.assign_temp(
                MirValueKind::Option(MirOptionOp::Some(MirOperand::Place(payload))),
                storage_ty,
                span,
            );
            self.assign(storage, wrapped, storage_ty, span);
        }
    }

    fn place_ty(&self, place: &MirPlace) -> Option<MirType> {
        let mut ty = match place.base {
            MirPlaceBase::Local(id) => self
                .locals
                .iter()
                .find(|local| local.id == id)
                .map(|local| local.ty)?,
            MirPlaceBase::Temp(id) => self
                .temps
                .iter()
                .find(|temp| temp.id == id)
                .map(|temp| temp.ty)?,
        };
        for projection in &place.projections {
            ty = match projection {
                MirProjection::Field(name) => self.struct_field_storage_ty(ty, *name)?,
                _ => return None,
            };
        }
        Some(ty)
    }

    fn struct_field_storage_ty(&self, base: MirType, name: radix_lexer::Symbol) -> Option<MirType> {
        let def_id = match self.normalized_type(base.semantic_id()) {
            Type::Struct(def_id) => *def_id,
            Type::Applied(applied, _) => match self.normalized_type(*applied) {
                Type::Struct(def_id) => *def_id,
                _ => return None,
            },
            Type::Ref(_, inner) => return self.struct_field_storage_ty(MirType::semantic(*inner), name),
            _ => return None,
        };
        self.context
            .struct_field_types
            .get(&def_id)
            .and_then(|fields| fields.get(&name))
            .copied()
    }

    /// Adapt a storage place to a narrowed HIR use-site type.
    ///
    /// Flow checks narrow the HIR expression inside a guarded branch or after
    /// an assertion, but the backing MIR local keeps its declared carrier.
    /// Materialize the narrowed union member before adding a field/index
    /// projection or passing the path to a member call; otherwise validation
    /// quite correctly sees the original carrier instead of the member type.
    pub(super) fn adapt_narrowed_place(
        &mut self,
        place: MirPlace,
        storage_ty: Option<MirType>,
        expr: &HirExpression,
    ) -> Option<MirPlace> {
        let Some(storage_ty) = storage_ty else {
            return Some(place);
        };
        let Some(use_ty) = expr.ty.map(MirType::semantic) else {
            return Some(place);
        };
        if let Type::Union(members) = self.normalized_type(storage_ty.semantic_id()) {
            let member_match = members
                .iter()
                .any(|member| self.types.equals(*member, use_ty.semantic_id()));
            if member_match {
                return Some(self.assign_temp_place(
                    MirValueKind::Operand(MirOperand::Place(place)),
                    use_ty,
                    expr.span,
                ));
            }
        }
        // An access marker on the use type (`ref Node ∪ none cur` narrows to
        // `Ref(Node)`, Option A) is a compile-time fact: the MIR storage may
        // already have it peeled (`Option(Node)`). Compare payloads and
        // unwrap to the payload type; the marker is enforced in the checker.
        let (_, storage_payload) = self.types.peel_access_marker(storage_ty.semantic_id());
        let (_, use_payload) = self.types.peel_access_marker(use_ty.semantic_id());
        let Some(inner_ty) = self
            .types
            .nullable_inner(storage_payload)
            .map(MirType::semantic)
        else {
            return Some(place);
        };
        if self.types.nullable_inner(use_payload).is_some()
            || !self
                .types
                .is_assignable(inner_ty.semantic_id(), use_payload)
        {
            return Some(place);
        }
        // The MIR layer does not track access markers (the local lowering
        // already peels ref views), so both cases unwrap to the payload type.
        let unwrapped_ty = inner_ty;
        Some(self.assign_temp_place(
            MirValueKind::Option(MirOptionOp::Unwrap {
                value: MirOperand::Place(place),
                mode: MirOptionUnwrapMode::Assume,
            }),
            unwrapped_ty,
            expr.span,
        ))
    }

    fn assign_temp_place(&mut self, kind: MirValueKind, ty: MirType, span: Span) -> MirPlace {
        MirPlace::temp(self.assign_new_temp(kind, ty, span))
    }

    fn nullable_inner_ty(&self, expr: &HirExpression) -> Option<MirType> {
        let ty = expr.ty?;
        self.types.nullable_inner(ty).map(MirType::semantic)
    }
}

/// Enumerate an ascending literal range, stopping safely when its next step
/// would exceed the representable coordinate range.
fn enumerate_literal_range(start: i128, end: i128, step: i128, inclusive: bool) -> Option<Vec<i128>> {
    if step <= 0 {
        return None;
    }
    let mut coordinates = Vec::new();
    let mut coordinate = start;
    while coordinate < end || (inclusive && coordinate == end) {
        coordinates.push(coordinate);
        let Some(next) = coordinate.checked_add(step) else {
            break;
        };
        coordinate = next;
    }
    Some(coordinates)
}

#[cfg(test)]
#[path = "place_test.rs"]
mod place_test;

/// Whether a tensor bracket index is a range or a slot list containing one:
/// the forms that lower to `TensorSliceNd` rather than a coordinate read.
fn tensor_bracket_index_has_range(index: &HirExpression) -> bool {
    match &index.kind {
        HirExpressionKind::Intervallum { .. } => true,
        HirExpressionKind::Array(elements) => elements.iter().any(|element| {
            matches!(
                element,
                HirArrayElement::Expr(slot) if matches!(slot.kind, HirExpressionKind::Intervallum { .. })
            )
        }),
        _ => false,
    }
}
