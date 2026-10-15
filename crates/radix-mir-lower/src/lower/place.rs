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
        // WHY: tensor bracket read (`grid[idx]`) is sugar for `accipe`. Desugar
        // to the `TensorGet` collection op so MIR reuses the existing tensor
        // runtime path instead of a second `MirProjection::Index` protocol,
        // which validate rejects for tensors (DEFER-063).
        if !self.is_device_lowering()
            && object
                .ty
                .is_some_and(|ty| radix_semantic::tensor_bracket::is_tensor_type(self.types, ty))
        {
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
        let mut place = self.lower_projectable_place(object)?;
        let projection = self.lower_index_projection(&place, object.ty, index)?;
        place.projections.push(projection);
        Some(MirOperand::Place(place))
    }

    /// Whether `object[index]` is a list window: a list receiver and a range
    /// index.
    fn is_list_window(&self, object: &HirExpression, index: &HirExpression) -> bool {
        let (Some(object_ty), Some(index_ty)) = (object.ty, index.ty) else {
            return false;
        };
        let object_ty = self.types.peel_access_marker(object_ty).1;
        matches!(self.normalized_type(object_ty), Type::Array(_) | Type::BoundedArray { .. })
            && matches!(self.normalized_type(index_ty), Type::Intervallum(_))
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
    ) -> Option<MirProjection> {
        if base_ty.is_some_and(|ty| self.is_vector_lane_base(ty))
            && let Some(lane) = radix_semantic::passes::const_int_value(index).and_then(|lane| u8::try_from(lane).ok())
        {
            return Some(MirProjection::VectorLane(lane));
        }
        if let Some(linear) = self.lower_device_tensor_linear_index(base, base_ty, index) {
            return Some(MirProjection::Index(linear?));
        }
        Some(MirProjection::Index(self.lower_expr_value(index)?))
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
    /// (the base of `v![k]`), is a `vector<T, N>`.
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
                let mut place = self.lower_projectable_place(object)?;
                let projection = self.lower_index_projection(&place, object.ty, index)?;
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
        let link = self.lower_optional_chain_link(base.clone(), chain)?;
        let ty = self.expr_ty(expr)?;
        Some(self.assign_temp(MirValueKind::Option(MirOptionOp::Chain { base, link }), ty, expr.span))
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
                let projection = self.lower_index_projection(&place, object.ty, index)?;
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
