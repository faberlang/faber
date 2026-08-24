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
        name: crate::lexer::Symbol,
        _expr: &HirExpression,
    ) -> Option<MirOperand> {
        if self.is_handler_error_nuntius_projection(object, name) {
            return self.lower_expr_value(object);
        }
        let mut place = self.lower_projectable_place(object)?;
        place.projections.push(MirProjection::Field(name));
        Some(MirOperand::Place(place))
    }

    /// String-backed handler errors expose `.nuntius` as an identity projection.
    fn is_handler_error_nuntius_projection(&self, object: &HirExpression, field: crate::lexer::Symbol) -> bool {
        let Some(interner) = self.context.interner else {
            return false;
        };
        if interner.resolve(field) != crate::mir::HANDLER_MESSAGE_FIELD {
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
                .is_some_and(|ty| crate::semantic::tensor_bracket::is_tensor_type(self.types, ty))
        {
            return self.lower_tensor_bracket_get(object, index, expr);
        }
        let mut place = self.lower_projectable_place(object)?;
        let index = self.lower_expr_value(index)?;
        place.projections.push(MirProjection::Index(index));
        Some(MirOperand::Place(place))
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
        if crate::semantic::tensor_bracket::is_bracket_index_list_expr(index, self.types) {
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

    fn lower_projectable_place(&mut self, expr: &HirExpression) -> Option<MirPlace> {
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
                let mut place = self.lower_projectable_place(object)?;
                place.projections.push(MirProjection::Field(*name));
                Some(place)
            }
            HirExpressionKind::Index(object, index) => {
                let mut place = self.lower_projectable_place(object)?;
                let index = self.lower_expr_value(index)?;
                place.projections.push(MirProjection::Index(index));
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

    /// A non-null member receiver whose expression type is the asserted
    /// nominal type is the lowered shape of `value!.method()`, not a field
    /// projection. Keep this check metadata-driven so fields are never
    /// invented to recover a method call.
    fn non_null_member_is_method(&self, expr: &HirExpression, name: crate::lexer::Symbol) -> bool {
        let Some(receiver_ty) = expr.ty else {
            return false;
        };
        let Some(def_id) = self.method_receiver_struct_def(receiver_ty) else {
            return false;
        };
        self.context.method_targets.contains_key(&(def_id, name))
            || self
                .context
                .imported_method_providers
                .contains_key(&(def_id, name))
    }

    /// Lower non-null member/index access by asserting the base option into a
    /// temp-backed place before applying the requested projection.
    pub(super) fn lower_non_null(
        &mut self,
        object: &HirExpression,
        chain: &HirNonNullKind,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let method_receiver =
            matches!(chain, HirNonNullKind::Member(name) if self.non_null_member_is_method(expr, *name));
        let mut place = self.lower_non_null_base(object)?;
        match chain {
            HirNonNullKind::Member(name) => {
                if method_receiver {
                    return Some(MirOperand::Place(place));
                }
                place.projections.push(MirProjection::Field(*name));
                Some(MirOperand::Place(place))
            }
            HirNonNullKind::Index(index) => {
                let index = self.lower_expr_value(index)?;
                place.projections.push(MirProjection::Index(index));
                Some(MirOperand::Place(place))
            }
            HirNonNullKind::Call(args) => {
                let lowered_args = self.lower_positional_call_args(args)?;
                let ty = self.expr_ty(expr)?;
                Some(self.call_value_operand(MirOperand::Place(place), lowered_args, ty, expr.span))
            }
        }
    }

    fn lower_non_null_base(&mut self, object: &HirExpression) -> Option<MirPlace> {
        let place = self.lower_projectable_place(object)?;
        if let Some(inner_ty) = self.nullable_inner_ty(object) {
            // Typecheck wraps `sponte` field loads as `T ∪ nihil`, but MIR
            // stores those fields as `T` (omitted construction is `nihil` in
            // the raw slot). Unwrap only when the place is actually an option.
            if self
                .place_ty(&place)
                .is_some_and(|ty| self.types.nullable_inner(ty.semantic_id()).is_none())
            {
                return Some(place);
            }
            return Some(self.assign_temp_place(
                MirValueKind::Option(MirOptionOp::Unwrap {
                    value: MirOperand::Place(place),
                    mode: MirOptionUnwrapMode::Assert,
                }),
                inner_ty,
                object.span,
            ));
        }
        Some(place)
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

    fn struct_field_storage_ty(&self, base: MirType, name: crate::lexer::Symbol) -> Option<MirType> {
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
    /// Null checks narrow the HIR expression inside the guarded branch, but
    /// the backing MIR local keeps its declared nullable carrier. Projection
    /// lowering must materialize that carrier's payload before adding a field
    /// or index projection; otherwise validation quite correctly sees an
    /// `Option<T>` where a collection or struct is required.
    fn adapt_narrowed_place(
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
        let Some(inner_ty) = self
            .types
            .nullable_inner(storage_ty.semantic_id())
            .map(MirType::semantic)
        else {
            return Some(place);
        };
        if self.types.nullable_inner(use_ty.semantic_id()).is_some()
            || !self
                .types
                .is_assignable(inner_ty.semantic_id(), use_ty.semantic_id())
        {
            return Some(place);
        }
        Some(self.assign_temp_place(
            MirValueKind::Option(MirOptionOp::Unwrap {
                value: MirOperand::Place(place),
                mode: MirOptionUnwrapMode::Assume,
            }),
            inner_ty,
            expr.span,
        ))
    }

    fn assign_temp_place(&mut self, kind: MirValueKind, ty: MirType, span: Span) -> MirPlace {
        let temp = self.assign_temp(kind, ty, span);
        match temp {
            MirOperand::Temp(temp) => MirPlace::temp(temp),
            _ => unreachable!("assign_temp always returns a temp operand"),
        }
    }

    fn nullable_inner_ty(&self, expr: &HirExpression) -> Option<MirType> {
        let ty = expr.ty?;
        self.types.nullable_inner(ty).map(MirType::semantic)
    }
}
