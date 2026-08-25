//! Runtime and intrinsic call lowering.
//!
//! This module is the MIR boundary for operations that are not plain local
//! function calls or primitive expressions. Provider imports, collection
//! methods, diagnostic output, string formatting, conversions, and panic all
//! lower to explicit MIR runtime/intrinsic calls. Ordinary direct calls stay as
//! definition calls, while enum variant calls are redirected to aggregate
//! construction.
//!
//! BOUNDARIES
//! ==========
//! - Indirect calls are not guessed into MIR callable values; they fail closed.
//! - Collection methods are recognized only through resolved receiver type,
//!   method name, and arity.
//! - Failable function calls require an active local `cape` handler so the MIR
//!   can encode both success and error CFG edges.
//! - Runtime calls with `vacuum` return type emit no destination; value-returning
//!   calls allocate a temp and return that temp as the expression operand.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;
use crate::hir::HirCallArg;
use crate::semantic::lista_tensor_conversio::{
    analyze_rectangular_lista_literal, lista_numerus_type, resolve_bounded_lista_padded_shape,
    resolve_lista_to_tensor_shape, validate_lista_to_tensor_source, ListaToTensorShape,
};
use crate::semantic::IndexExpr;

/// Whether a `⇥` recovery expression is side-effecting: a call or method call
/// whose execution has observable effects. Pure recovery expressions (literals,
/// paths, compound values) need no deferral — evaluating them eagerly is
/// side-effect-free (CTO-3 recovery-eval fix).
fn is_side_effecting_recovery(recovery: &HirExpression) -> bool {
    matches!(&recovery.kind, HirExpressionKind::Call(..) | HirExpressionKind::MethodCall(..))
}

struct HandledConversio<'a> {
    source: MirOperand,
    source_ty: MirType,
    target: TypeId,
    params: &'a [Symbol],
    return_ty: MirType,
    handler: HandlerContext,
    span: Span,
}

impl FunctionBuilder<'_> {
    fn lower_hir_call_args(&mut self, args: &[HirCallArg]) -> Option<Vec<MirOperand>> {
        let mut lowered = Vec::with_capacity(args.len());
        for arg in args {
            lowered.push(self.lower_expr_value(&arg.expr)?);
        }
        Some(lowered)
    }

    /// Lower a method call into a provider or collection intrinsic.
    ///
    /// Provider method calls are recognized when the receiver path resolves to a
    /// provider import. Collection methods are deliberately name/type/arity
    /// gated so unsupported methods do not masquerade as generic runtime calls.
    pub(super) fn lower_method_call(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        args: &[HirCallArg],
        conversion: Option<DefId>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if let Some(target) = self.genus_method_target(receiver, method) {
            let mut lowered_args = Vec::with_capacity(args.len() + 1);
            lowered_args.push(self.lower_expr_value(receiver)?);
            lowered_args.extend(self.lower_hir_call_args(args)?);
            return self.lower_definition_call(target.def_id, Vec::new(), lowered_args, conversion, expr);
        }

        let imported_method_target = receiver.ty.and_then(|receiver_ty| {
            let def_id = self.method_receiver_struct_def(receiver_ty)?;
            self.context
                .imported_method_providers
                .get(&(def_id, method))
                .cloned()
        });
        if let Some(target) = imported_method_target {
            let mut lowered_args = Vec::with_capacity(args.len() + 1);
            lowered_args.push(self.lower_expr_value(receiver)?);
            lowered_args.extend(self.lower_hir_call_args(args)?);
            let ty = self.expr_ty(expr)?;
            let mut module = target.provider.module;
            module.push(target.provider.item);
            return Some(self.runtime_call_value(
                MirIntrinsic::Provider(MirProvider { kind: target.provider.kind, module, name: target.canonical_name }),
                lowered_args,
                ty,
                expr.span,
            ));
        }

        if let HirExpressionKind::Path(def_id) = receiver.kind {
            if let Some(import) = self.context.provider_imports.get(&def_id).cloned() {
                // D-PA1/D-PA2: a resolvable user-code provider call (the
                // import resolved to a sibling module under package identity
                // context) lowers to a signature-known external definition
                // keyed to the VERB — the direct cross-module symbol reference,
                // not a versioned host ABI call and not the raw provider route.
                if let Some(verb_def) = import
                    .file_interface_functions
                    .as_ref()
                    .and_then(|functions| functions.get(&method))
                {
                    let lowered_args = self.lower_hir_call_args(args)?;
                    return self.lower_definition_call(*verb_def, Vec::new(), lowered_args, conversion, expr);
                }
                let lowered_args = self.lower_hir_call_args(args)?;
                let ty = self.expr_ty(expr)?;
                let mut module = import.module;
                module.push(import.item);
                return Some(self.runtime_call_value(
                    MirIntrinsic::Provider(MirProvider { kind: import.kind, module, name: method }),
                    lowered_args,
                    ty,
                    expr.span,
                ));
            }
        }

        let method_name = self.resolve_method_name(method)?.to_owned();
        if let Some(lowered) = self.lower_sermo_method(receiver, &method_name, args, expr) {
            return Some(lowered);
        }
        if let Some(lowered) = self.lower_vector_method(receiver, &method_name, args, expr) {
            return Some(lowered);
        }
        if let Some(lowered) = self.lower_matrix_method(receiver, &method_name, args, expr) {
            return Some(lowered);
        }
        if let Some(lowered) = self.lower_numeric_operator_method(receiver, &method_name, args, expr) {
            return Some(lowered);
        }
        if let Some(lowered) = self.lower_atomic_method(receiver, &method_name, args, expr) {
            return Some(lowered);
        }
        if let Some(lowered) = self.lower_lista_ordina(receiver, &method_name, args, expr) {
            return Some(lowered);
        }
        if method_name == "silu" {
            if let Some(receiver_ty) = receiver.ty {
                let payload = self.types.get(self.method_receiver_payload_id(receiver_ty));
                if let Some(entry) = crate::intrinsics::lookup_method_for_type(payload, &method_name, args.len()) {
                    if matches!(entry.receiver, crate::intrinsics::IntrinsicReceiver::Tensor) {
                        return self.lower_tensor_silu(receiver, expr);
                    }
                }
            }
        }

        if let Some(receiver_ty) = receiver.ty {
            let payload = self.types.get(self.method_receiver_payload_id(receiver_ty));
            if let Some(entry) = crate::intrinsics::lookup_method_for_type(payload, &method_name, args.len()) {
                if crate::intrinsics::mir_is_higher_order_lista(entry) {
                    let closure = &args[0].expr;
                    return match entry.mir_kind {
                        crate::intrinsics::IntrinsicMirKind::HigherOrderFiltrata => {
                            self.lower_filtrata(receiver, closure, expr)
                        }
                        crate::intrinsics::IntrinsicMirKind::HigherOrderMap => {
                            self.lower_mappata(receiver, closure, expr)
                        }
                        crate::intrinsics::IntrinsicMirKind::HigherOrderReduce => {
                            let initial = &args[1].expr;
                            self.lower_reducta(receiver, closure, initial, expr)
                        }
                        crate::intrinsics::IntrinsicMirKind::HigherOrderCumulata => {
                            let initial = &args[1].expr;
                            self.lower_cumulata(receiver, closure, initial, expr)
                        }
                        _ => None,
                    };
                }
            }
        }

        let Some(op) = self.collection_method_op(receiver, method, args, expr.span) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "method call before runtime/provider MIR lowering",
            ));
            return None;
        };

        let mut lowered_args = Vec::with_capacity(args.len() + 1);
        lowered_args.push(self.lower_expr_value(receiver)?);
        lowered_args.extend(self.lower_hir_call_args(args)?);
        let expr_ty = self.expr_ty(expr)?;
        // WHY: semantic expected-type unification can annotate a scalar
        // collection intrinsic with its nullable enclosing type at the call
        // site. The MIR operation contract remains scalar; adapt the value at
        // the expression boundary instead of weakening intrinsic validation.
        //
        // Tier 2 selects the checked read at this lowering seam from the
        // receiver's declared type. The runner sees the strict element return
        // type and traps on OOB; the source-level nullable carrier is restored
        // with `Some` so existing `vel`/`coalesce` expressions stay typed.
        let call_ty = match op {
            MirCollectionOp::Length => MirType::semantic(self.types.primitive(Primitive::Numerus)),
            MirCollectionOp::Index | MirCollectionOp::TensorGet => self
                .checked_read_result_type(op, receiver.ty, expr_ty)
                .unwrap_or(expr_ty),
            _ => expr_ty,
        };
        let value = self.runtime_call_value(MirIntrinsic::Collection(op), lowered_args, call_ty, expr.span);
        if call_ty != expr_ty && self.types.nullable_inner(expr_ty.semantic_id()) == Some(call_ty.semantic_id()) {
            return Some(self.assign_temp(MirValueKind::Option(MirOptionOp::Some(value)), expr_ty, expr.span));
        }
        Some(value)
    }

    /// Lower A1/C5 numeric collection arithmetic.
    ///
    /// Dynamic pairs use a dedicated collection runtime op so the runner can
    /// enforce the hard same-live-length error. A pair whose two lista extents
    /// are literal `BoundedArray` capacities is expanded into indexed MIR
    /// cells; queue/stack capacities always remain storage limits, never live
    /// lengths.
    pub(super) fn lower_lista_elementwise(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let lhs_shape = lhs.ty.and_then(|ty| self.lista_shape(ty));
        let rhs_shape = rhs.ty.and_then(|ty| self.lista_shape(ty));
        let queue_stack_operand = [lhs.ty, rhs.ty].into_iter().flatten().any(|ty| {
            matches!(
                self.normalized_type(ty),
                Type::Queue(_) | Type::BoundedQueue { .. } | Type::Stack(_) | Type::BoundedStack { .. }
            )
        });
        if lhs_shape.is_none() && rhs_shape.is_none() && !queue_stack_operand {
            self.errors.push(MirError::unsupported(
                expr.span,
                "lista arithmetic without a collection operand",
            ));
            return None;
        }
        let collection_op = match op {
            HirBinOp::Add => MirCollectionOp::ArrayAdd,
            HirBinOp::Sub => MirCollectionOp::ArraySub,
            HirBinOp::Mul => MirCollectionOp::ArrayMul,
            HirBinOp::Div => MirCollectionOp::ArrayDiv,
            _ => unreachable!("lista elementwise lowering only accepts arithmetic operators"),
        };
        let lhs_value = self.lower_expr_value(lhs)?;
        let rhs_value = self.lower_expr_value(rhs)?;
        let result_ty = self.expr_ty(expr)?;

        // Queue/stack capacities are storage limits, not live lengths. Keep
        // every queue/stack operation on the ArrayAdd family runtime seam so
        // Collection(Index) observes the current window and the runtime
        // enforces same-length pairs.
        if queue_stack_operand {
            return Some(self.runtime_call_value(
                MirIntrinsic::Collection(collection_op),
                vec![lhs_value, rhs_value],
                result_ty,
                expr.span,
            ));
        }

        let static_capacity = match (lhs_shape, rhs_shape) {
            (Some((_, Some(lhs_capacity))), Some((_, Some(rhs_capacity)))) if lhs_capacity == rhs_capacity => {
                Some(lhs_capacity)
            }
            (Some((_, Some(capacity))), None) | (None, Some((_, Some(capacity)))) => Some(capacity),
            _ => None,
        };
        let Some(capacity) = static_capacity else {
            return Some(self.runtime_call_value(
                MirIntrinsic::Collection(collection_op),
                vec![lhs_value, rhs_value],
                result_ty,
                expr.span,
            ));
        };

        let lhs_place = if lhs_shape.is_some() {
            Some(self.value_base_place(lhs_value.clone(), MirType::semantic(lhs.ty?), lhs.span)?)
        } else {
            None
        };
        let rhs_place = if rhs_shape.is_some() {
            Some(self.value_base_place(rhs_value.clone(), MirType::semantic(rhs.ty?), rhs.span)?)
        } else {
            None
        };
        let scalar = if lhs_shape.is_none() { lhs_value } else { rhs_value };
        let element_ty = lhs_shape
            .map(|(element, _)| element)
            .or_else(|| rhs_shape.map(|(element, _)| element))?;
        let mir_op = match collection_op {
            MirCollectionOp::ArrayAdd => MirBinOp::Add,
            MirCollectionOp::ArraySub => MirBinOp::Sub,
            MirCollectionOp::ArrayMul => MirBinOp::Mul,
            MirCollectionOp::ArrayDiv => MirBinOp::Div,
            _ => unreachable!("lista collection op mapping"),
        };
        let mut fields = Vec::with_capacity(capacity);
        for index in 0..capacity {
            let lhs = if let Some(place) = lhs_place.as_ref() {
                lista_index_operand(place, index)?
            } else {
                scalar.clone()
            };
            let rhs = if let Some(place) = rhs_place.as_ref() {
                lista_index_operand(place, index)?
            } else {
                scalar.clone()
            };
            let value = self.assign_temp(
                MirValueKind::Binary { op: mir_op, lhs, rhs },
                MirType::semantic(element_ty),
                expr.span,
            );
            fields.push(MirAggregateItem::Operand(value));
        }
        Some(self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(fields),
            result_ty,
            expr.span,
        ))
    }

    fn lista_shape(&self, ty: TypeId) -> Option<(TypeId, Option<usize>)> {
        match self.normalized_type(ty) {
            Type::Array(element) => Some((*element, None)),
            Type::BoundedArray { element, capacity } => {
                let IndexExpr::Literal(capacity) = self.types.get_index(*capacity) else {
                    return Some((*element, None));
                };
                let Some(capacity) = usize::try_from(*capacity).ok() else {
                    return Some((*element, None));
                };
                if i64::try_from(capacity).is_err() {
                    return Some((*element, None));
                }
                Some((*element, Some(capacity)))
            }
            _ => None,
        }
    }

    /// Lower a glyph-product binop (`·` `×` `⊗` `⊙` `⊘`) by operand family,
    /// reusing the vector/matrix method machinery and the collection-op
    /// intrinsic seam. Every unsupported shape/type combination fails closed
    /// with a stable structured diagnostic — never a silent pass.
    pub(super) fn lower_glyph_product(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(lhs_ty_id) = lhs.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "glyph product lhs operand"));
            return None;
        };
        let Some(rhs_ty_id) = rhs.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "glyph product rhs operand"));
            return None;
        };
        let lhs_ty = self.normalized_type(lhs_ty_id).clone();
        let rhs_ty = self.normalized_type(rhs_ty_id).clone();

        // `⊗` outer: vector⊗vector builds the matrix-register aggregate; the
        // Kronecker forms (matrix⊗matrix / tensor⊗tensor) stay fail-closed —
        // no new MIR op is invented for them.
        if op == HirBinOp::TensorProduct {
            if matches!((&lhs_ty, &rhs_ty), (Type::Vector(..), Type::Vector(..))) {
                return self.lower_vector_outer(lhs, rhs, expr);
            }
            let detail = if matches!((&lhs_ty, &rhs_ty), (Type::Matrix(..), Type::Matrix(..))) {
                "matrix tensor product (Kronecker)"
            } else if matches!((&lhs_ty, &rhs_ty), (Type::Tensor(..), Type::Tensor(..))) {
                "tensor tensor product (Kronecker)"
            } else {
                "tensor product"
            };
            self.glyph_fail_closed(detail, expr.span);
            return None;
        }

        // OQ2 pinned default: the column-side vector·matrix form stays
        // fail-closed on the MIR lane (host-Rust 1C residual posture).
        if op == HirBinOp::Dot && matches!((&lhs_ty, &rhs_ty), (Type::Vector(..), Type::Matrix(..))) {
            self.glyph_fail_closed("vector·matrix dot product", expr.span);
            return None;
        }

        let Some(method_name) = glyph_method_name(op, &lhs_ty, &rhs_ty) else {
            self.glyph_fail_closed("glyph product operand types", expr.span);
            return None;
        };

        match (&lhs_ty, &rhs_ty) {
            (Type::Tensor(..), Type::Tensor(..)) => self.lower_glyph_collection_op(lhs, method_name, rhs, expr),
            (Type::Vector(..), Type::Vector(..)) => {
                let arg = HirCallArg { name: None, spread: false, expr: rhs.clone(), span: rhs.span };
                self.lower_vector_method(lhs, method_name, std::slice::from_ref(&arg), expr)
            }
            (Type::Matrix(..), Type::Matrix(..) | Type::Vector(..)) => {
                let arg = HirCallArg { name: None, spread: false, expr: rhs.clone(), span: rhs.span };
                self.lower_matrix_method(lhs, method_name, std::slice::from_ref(&arg), expr)
            }
            _ => {
                self.glyph_fail_closed("glyph product operand types", expr.span);
                None
            }
        }
    }

    /// Fail closed for a glyph-product shape/type combination with a stable
    /// structured diagnostic.
    fn glyph_fail_closed(&mut self, detail: &str, span: Span) {
        self.errors.push(MirError::unsupported(
            span,
            format!("{detail} is not supported on the MIR target"),
        ));
    }

    /// Tensor glyphs route through the collection-op intrinsic seam exactly
    /// like the method twins (`a.matmul(b)` / `a.multiplica(b)`): the registry
    /// row is resolved by receiver type + name + arity, then lowered to a
    /// `MirIntrinsic::Collection` runtime call.
    fn lower_glyph_collection_op(
        &mut self,
        lhs: &HirExpression,
        method_name: &str,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(receiver_ty) = lhs.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "glyph collection receiver"));
            return None;
        };
        let receiver_ty = self.normalized_type(receiver_ty);
        let entry = crate::intrinsics::lookup_method_for_type(receiver_ty, method_name, 1)?;
        let op = crate::intrinsics::mir_collection_op(entry)?;
        let lowered_args = vec![self.lower_expr_value(lhs)?, self.lower_expr_value(rhs)?];
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(MirIntrinsic::Collection(op), lowered_args, ty, expr.span))
    }

    /// Outer product `v ⊗ w` → `matrix<elem, [n, m]>` register aggregate built
    /// from lane-product cells (row-major: result row i = lhs lane i). The
    /// smallest matrix-register construct — no new MIR node type.
    fn lower_vector_outer(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let (Type::Vector(lhs_elem, lhs_width), Type::Vector(_, rhs_width)) =
            (self.normalized_type(lhs.ty?).clone(), self.normalized_type(rhs.ty?).clone())
        else {
            self.errors
                .push(MirError::unsupported(expr.span, "outer product requires vector operands"));
            return None;
        };
        let Some(rows) = crate::semantic::literal_vector_width(lhs_width, self.types) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "outer product with non-literal width before MIR lowering",
            ));
            return None;
        };
        let Some(columns) = crate::semantic::literal_vector_width(rhs_width, self.types) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "outer product with non-literal width before MIR lowering",
            ));
            return None;
        };
        // Register-matrix CPU subset: the result matrix must stay ≤4×4.
        if rows > 4 || columns > 4 {
            self.errors.push(MirError::unsupported(
                expr.span,
                "outer product matrix register exceeds 4x4 CPU subset",
            ));
            return None;
        }
        let lhs_operand = self.lower_expr_value(lhs)?;
        let lhs_place = self.value_base_place(lhs_operand, MirType::semantic(lhs.ty?), lhs.span)?;
        let rhs_operand = self.lower_expr_value(rhs)?;
        let rhs_place = self.value_base_place(rhs_operand, MirType::semantic(rhs.ty?), rhs.span)?;
        let elem_ty = MirType::semantic(lhs_elem);
        let matrix_ty = self.expr_ty(expr)?;
        let mut fields = Vec::with_capacity(usize::from(rows) * usize::from(columns));
        for row in 0..rows {
            for column in 0..columns {
                let lhs = vector_lane_operand(&lhs_place, row);
                let rhs = vector_lane_operand(&rhs_place, column);
                let value = self.assign_temp(MirValueKind::Binary { op: MirBinOp::Mul, lhs, rhs }, elem_ty, expr.span);
                fields.push(MirAggregateItem::Operand(value));
            }
        }
        self.construct_temp(
            MirAggregateKind::Matrix,
            MirAggregateFields::Ordered(fields),
            matrix_ty,
            expr.span,
        )
        .into()
    }

    fn lower_sermo_method(
        &mut self,
        receiver: &HirExpression,
        method_name: &str,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let receiver_ty = receiver.ty?;
        let receiver_def = match self.normalized_type(receiver_ty) {
            Type::Struct(def_id) => Some(*def_id),
            Type::Applied(base, _) => match self.normalized_type(*base) {
                Type::Struct(def_id) => Some(*def_id),
                _ => None,
            },
            _ => None,
        }?;
        let op = if Some(receiver_def) == self.context.sermo_def {
            match method_name {
                "meus" => crate::mir::MirSermoOp::ViewMeus,
                "tuus" => crate::mir::MirSermoOp::ViewTuus,
                _ => return None,
            }
        } else if Some(receiver_def) == self.context.meus_def {
            match method_name {
                "da" => crate::mir::MirSermoOp::Send,
                "fini" => crate::mir::MirSermoOp::Finish,
                _ => return None,
            }
        } else if Some(receiver_def) == self.context.tuus_def {
            match method_name {
                "accipe" => crate::mir::MirSermoOp::Receive {
                    frame: self.context.scrinium_def?,
                    data_field: self.context.scrinium_data_field?,
                },
                "cursor" => crate::mir::MirSermoOp::Cursor {
                    frame: self.context.scrinium_def?,
                    data_field: self.context.scrinium_data_field?,
                },
                "fini" => crate::mir::MirSermoOp::Finish,
                _ => return None,
            }
        } else {
            return None;
        };
        let mut lowered_args = Vec::with_capacity(args.len() + 1);
        lowered_args.push(self.lower_expr_value(receiver)?);
        lowered_args.extend(self.lower_hir_call_args(args)?);
        let return_ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(MirIntrinsic::Sermo(op), lowered_args, return_ty, expr.span))
    }

    fn lower_atomic_method(
        &mut self,
        receiver: &HirExpression,
        method_name: &str,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let receiver_ty = receiver.ty?;
        if !matches!(self.normalized_type(receiver_ty), Type::Atomic(_)) {
            return None;
        }
        let entry =
            crate::intrinsics::lookup_method_for_type(self.normalized_type(receiver_ty), method_name, args.len())?;
        let op = crate::intrinsics::mir_atomic_op(entry)?;
        let mut lowered_args = Vec::with_capacity(args.len() + 1);
        lowered_args.push(self.lower_expr_value(receiver)?);
        lowered_args.extend(self.lower_hir_call_args(args)?);
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(MirIntrinsic::Atomic(op), lowered_args, ty, expr.span))
    }

    fn lower_vector_method(
        &mut self,
        receiver: &HirExpression,
        method_name: &str,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let method = vector_method_kind(method_name)?;
        let receiver_ty = receiver.ty?;
        let Type::Vector(element_ty, width) = self.normalized_type(receiver_ty).clone() else {
            return None;
        };
        #[expect(
            clippy::cast_possible_truncation,
            reason = "vector width literal is bounded to 1..=4 by the pattern"
        )]
        let width = match self.types.get_index(width) {
            IndexExpr::Literal(width @ 1..=4) => *width as u8,
            IndexExpr::Literal(_) => {
                self.errors
                    .push(MirError::unsupported(expr.span, "vector method width before MIR lowering"));
                return None;
            }
            _ => {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "vector method with non-literal width before MIR lowering",
                ));
                return None;
            }
        };
        let lhs = self.lower_expr_value(receiver)?;
        let lhs = self.value_base_place(lhs, MirType::semantic(receiver_ty), receiver.span)?;
        let elem_ty = MirType::semantic(element_ty);
        let vector_ty = MirType::semantic(receiver_ty);

        if matches!(method, VectorMethodKind::Longitudo | VectorMethodKind::Normalizata) {
            if !args.is_empty() {
                return None;
            }
            if matches!(method, VectorMethodKind::Longitudo) {
                return Some(self.lower_vector_longitudo(lhs, width, elem_ty, expr));
            }
            return Some(self.lower_vector_normalizata(lhs, width, elem_ty, vector_ty, expr));
        }

        let [arg] = args else {
            return None;
        };
        if matches!(method, VectorMethodKind::Swizzle) {
            return self.lower_vector_swizzle(lhs, width, vector_ty, &arg.expr, expr);
        }

        let rhs = self.lower_expr_value(&arg.expr)?;
        let rhs_ty = arg.expr.ty.map_or(vector_ty, MirType::semantic);
        let rhs = self.value_base_place(rhs, rhs_ty, arg.expr.span)?;
        match method {
            VectorMethodKind::Elementwise(op) => {
                Some(self.lower_vector_elementwise_op(lhs, rhs, width, op, elem_ty, vector_ty, expr))
            }
            VectorMethodKind::Cross => {
                if width != 3 {
                    self.errors
                        .push(MirError::unsupported(expr.span, "vector cross width before MIR lowering"));
                    return None;
                }
                Some(self.lower_vector_cross(lhs, rhs, elem_ty, vector_ty, expr))
            }
            VectorMethodKind::Dot => Some(self.lower_vector_dot(lhs, rhs, width, elem_ty, expr)),
            VectorMethodKind::Swizzle => unreachable!("swizzle branch returns before vector peer lowering"),
            VectorMethodKind::Longitudo | VectorMethodKind::Normalizata => {
                unreachable!("zero-arg vector methods handled above")
            }
        }
    }

    /// `||v|| = sqrt(dot(v, v))`.
    fn lower_vector_longitudo(
        &mut self,
        vector: MirPlace,
        width: u8,
        elem_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let sum_sq = self.lower_vector_dot(vector.clone(), vector, width, elem_ty, expr);
        self.assign_temp(MirValueKind::Unary { op: MirUnOp::Sqrt, operand: sum_sq }, elem_ty, expr.span)
    }

    /// Unit vector; near-zero length (≤ 1e-6) yields the zero vector (Triga policy).
    fn lower_vector_normalizata(
        &mut self,
        vector: MirPlace,
        width: u8,
        elem_ty: MirType,
        vector_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let length = self.lower_vector_longitudo(vector.clone(), width, elem_ty, expr);
        let eps = MirOperand::Constant(MirConstant::Float(0.000_001));
        let nonzero = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Gt, lhs: length.clone(), rhs: eps },
            MirType::semantic(self.types.primitive(crate::semantic::Primitive::Bivalens)),
            expr.span,
        );

        let result = self.push_temp(vector_ty, expr.span);
        let scale_id = self.fresh_block(expr.span);
        let zero_id = self.fresh_block(expr.span);
        let join_id = self.fresh_block(expr.span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: nonzero, then_block: scale_id, else_block: zero_id },
            expr.span,
        );

        self.switch_to(scale_id);
        {
            let mut fields = Vec::with_capacity(usize::from(width));
            for lane in 0..width {
                let lane_op = vector_lane_operand(&vector, lane);
                let scaled = self.assign_temp(
                    MirValueKind::Binary { op: MirBinOp::Div, lhs: lane_op, rhs: length.clone() },
                    elem_ty,
                    expr.span,
                );
                fields.push(MirAggregateItem::Operand(scaled));
            }
            let scaled = self.construct_temp(
                MirAggregateKind::Vector,
                MirAggregateFields::Ordered(fields),
                vector_ty,
                expr.span,
            );
            let value = self.new_value(MirValueKind::Operand(scaled), vector_ty, expr.span);
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Assign { place: MirPlace::temp(result), value },
                span: expr.span,
            });
            self.terminate_current(MirTerminatorKind::Goto(join_id), expr.span);
        }

        self.switch_to(zero_id);
        {
            let mut zeros = Vec::with_capacity(usize::from(width));
            for _ in 0..width {
                let z = self.assign_temp(
                    MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(0.0))),
                    elem_ty,
                    expr.span,
                );
                zeros.push(MirAggregateItem::Operand(z));
            }
            let zero = self.construct_temp(
                MirAggregateKind::Vector,
                MirAggregateFields::Ordered(zeros),
                vector_ty,
                expr.span,
            );
            let value = self.new_value(MirValueKind::Operand(zero), vector_ty, expr.span);
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Assign { place: MirPlace::temp(result), value },
                span: expr.span,
            });
            self.terminate_current(MirTerminatorKind::Goto(join_id), expr.span);
        }

        self.switch_to(join_id);
        MirOperand::Temp(result)
    }

    fn lower_matrix_method(
        &mut self,
        receiver: &HirExpression,
        method_name: &str,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let receiver_ty = receiver.ty?;
        let Type::Matrix(element_ty, shape) = self.normalized_type(receiver_ty).clone() else {
            return None;
        };
        let Some((rows, columns)) = crate::semantic::literal_matrix_shape(shape, self.types) else {
            self.errors.push(MirError::unsupported(
                expr.span,
                "matrix method with non-literal shape before MIR lowering",
            ));
            return None;
        };
        // Reject oversized register matrices before emission (CPU subset: ≤4×4).
        if rows > 4 || columns > 4 {
            self.errors.push(MirError::unsupported(
                expr.span,
                "matrix register method exceeds 4x4 CPU subset",
            ));
            return None;
        }
        let lhs = self.lower_expr_value(receiver)?;
        let matrix_ty = MirType::semantic(receiver_ty);
        let lhs = self.value_base_place(lhs, matrix_ty, receiver.span)?;
        let elem_ty = MirType::semantic(element_ty);

        if method_name == "inversa" {
            if !args.is_empty() {
                return None;
            }
            let result_ty = self.expr_ty(expr)?;
            return Some(self.lower_matrix_inversa(lhs, rows, columns, elem_ty, matrix_ty, result_ty, expr));
        }

        let [arg] = args else {
            return None;
        };
        let rhs = self.lower_expr_value(&arg.expr)?;
        let rhs_ty = arg.expr.ty.map_or(matrix_ty, MirType::semantic);
        let rhs = self.value_base_place(rhs, rhs_ty, arg.expr.span)?;
        if method_name == "matmul" {
            // Full matmul needs the peer's literal [K,N] shape: the result is
            // [M,N], never the receiver's own [M,K] (which would silently drop
            // RHS columns for non-square products).
            let Some(peer_ty) = arg.expr.ty else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix matmul peer without a type before MIR lowering",
                ));
                return None;
            };
            let Type::Matrix(_, peer_shape) = self.normalized_type(peer_ty).clone() else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix matmul peer must be a matrix before MIR lowering",
                ));
                return None;
            };
            let Some((rhs_rows, rhs_columns)) = crate::semantic::literal_matrix_shape(peer_shape, self.types) else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix matmul peer with non-literal shape before MIR lowering",
                ));
                return None;
            };
            return Some(self.lower_matrix_matmul(lhs, rhs, rows, columns, rhs_rows, rhs_columns, elem_ty, expr));
        }
        if method_name == "applica" {
            // Peer is vector width = columns; result is vector width = rows.
            let arg_ty = arg.expr.ty?;
            let Type::Vector(_, width) = self.normalized_type(arg_ty).clone() else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix applica requires vector peer before MIR lowering",
                ));
                return None;
            };
            #[expect(
                clippy::cast_possible_truncation,
                reason = "matrix peer vector width literal is bounded to 1..=4 by the pattern"
            )]
            let peer_width = if let IndexExpr::Literal(w @ 1..=4) = self.types.get_index(width) {
                *w as u8
            } else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix applica vector width before MIR lowering",
                ));
                return None;
            };
            if peer_width != columns {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix applica vector width must match matrix columns",
                ));
                return None;
            }
            let result_ty = self.expr_ty(expr)?;
            return Some(self.lower_matrix_applica(lhs, rhs, rows, columns, elem_ty, result_ty, expr));
        }
        let op = matrix_method_kind(method_name)?;
        Some(self.lower_matrix_elementwise_op(lhs, rhs, rows, columns, op, elem_ty, matrix_ty, expr))
    }

    /// Flatten nested `[[…], …]` lista literal into row-major matrix cells.
    fn lower_matrix_from_nested_lista(
        &mut self,
        elements: &[HirArrayElement],
        rows: u8,
        columns: u8,
        ty: MirType,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if rows > 4 || columns > 4 {
            self.errors.push(MirError::unsupported(
                expr.span,
                "matrix register materialize exceeds 4x4 CPU subset",
            ));
            return None;
        }
        if elements.len() != usize::from(rows) {
            self.errors.push(MirError::unsupported(
                expr.span,
                "matrix materialize row count mismatch before MIR lowering",
            ));
            return None;
        }
        let mut fields = Vec::with_capacity(usize::from(rows) * usize::from(columns));
        for row in elements {
            let HirArrayElement::Expr(row_expr) = row else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix materialize forbids spreads before MIR lowering",
                ));
                return None;
            };
            let HirExpressionKind::Array(cells) = &row_expr.kind else {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix materialize requires nested lista rows before MIR lowering",
                ));
                return None;
            };
            if cells.len() != usize::from(columns) {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix materialize column count mismatch before MIR lowering",
                ));
                return None;
            }
            for cell in cells {
                let HirArrayElement::Expr(cell_expr) = cell else {
                    self.errors.push(MirError::unsupported(
                        expr.span,
                        "matrix materialize forbids spreads before MIR lowering",
                    ));
                    return None;
                };
                fields.push(MirAggregateItem::Operand(self.lower_expr_value(cell_expr)?));
            }
        }
        Some(self.construct_temp(MirAggregateKind::Matrix, MirAggregateFields::Ordered(fields), ty, expr.span))
    }

    /// Expand matrix×vector product into cell×lane mul/add and a vector construct.
    fn lower_matrix_applica(
        &mut self,
        matrix: MirPlace,
        vector: MirPlace,
        rows: u8,
        columns: u8,
        elem_ty: MirType,
        vector_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let mut fields = Vec::with_capacity(usize::from(rows));
        for row in 0..rows {
            let mut acc = self.assign_temp(
                MirValueKind::Binary {
                    op: MirBinOp::Mul,
                    lhs: matrix_cell_operand(&matrix, row, 0),
                    rhs: vector_lane_operand(&vector, 0),
                },
                elem_ty,
                expr.span,
            );
            for column in 1..columns {
                let product = self.assign_temp(
                    MirValueKind::Binary {
                        op: MirBinOp::Mul,
                        lhs: matrix_cell_operand(&matrix, row, column),
                        rhs: vector_lane_operand(&vector, column),
                    },
                    elem_ty,
                    expr.span,
                );
                acc = self.assign_temp(
                    MirValueKind::Binary { op: MirBinOp::Add, lhs: acc, rhs: product },
                    elem_ty,
                    expr.span,
                );
            }
            fields.push(MirAggregateItem::Operand(acc));
        }
        self.construct_temp(
            MirAggregateKind::Vector,
            MirAggregateFields::Ordered(fields),
            vector_ty,
            expr.span,
        )
    }

    /// Square matrix inverse with singular → nihil (Option).
    ///
    /// - 2×2: full inverse via determinant
    /// - 4×4: affine upper-3×3 inverse matching Triga `matrix4_inversa_affinis`
    ///
    /// Other ranks reject before emission.
    fn lower_matrix_inversa(
        &mut self,
        matrix: MirPlace,
        rows: u8,
        columns: u8,
        elem_ty: MirType,
        matrix_ty: MirType,
        option_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        if rows != columns {
            self.errors
                .push(MirError::unsupported(expr.span, "matrix inversa requires square shape"));
            return self.assign_temp(MirValueKind::Option(MirOptionOp::None), option_ty, expr.span);
        }
        match rows {
            2 => self.lower_matrix_inversa_2x2(matrix, elem_ty, matrix_ty, option_ty, expr),
            4 => self.lower_matrix_inversa_affine_4x4(matrix, elem_ty, matrix_ty, option_ty, expr),
            _ => {
                self.errors.push(MirError::unsupported(
                    expr.span,
                    "matrix inversa CPU subset admits only 2x2 and affine 4x4",
                ));
                self.assign_temp(MirValueKind::Option(MirOptionOp::None), option_ty, expr.span)
            }
        }
    }

    fn lower_matrix_inversa_2x2(
        &mut self,
        matrix: MirPlace,
        elem_ty: MirType,
        matrix_ty: MirType,
        option_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let a = matrix_cell_operand(&matrix, 0, 0);
        let b = matrix_cell_operand(&matrix, 0, 1);
        let c = matrix_cell_operand(&matrix, 1, 0);
        let d = matrix_cell_operand(&matrix, 1, 1);
        let ad = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: a.clone(), rhs: d.clone() },
            elem_ty,
            expr.span,
        );
        let bc = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: b.clone(), rhs: c.clone() },
            elem_ty,
            expr.span,
        );
        let det = self.assign_temp(MirValueKind::Binary { op: MirBinOp::Sub, lhs: ad, rhs: bc }, elem_ty, expr.span);
        self.lower_matrix_inversa_with_det(det, elem_ty, option_ty, expr, |this, inv_det| {
            this.lower_matrix_2x2_inverse_with_det(a, b, c, d, inv_det, elem_ty, matrix_ty, expr)
        })
    }

    /// Affine 4×4 inverse (upper 3×3 + translation), Triga column-vector layout
    /// expressed with row-major language cells.
    fn lower_matrix_inversa_affine_4x4(
        &mut self,
        matrix: MirPlace,
        elem_ty: MirType,
        matrix_ty: MirType,
        option_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        // Upper-left 3×3 cells (row-major language matrix).
        let a00 = matrix_cell_operand(&matrix, 0, 0);
        let a01 = matrix_cell_operand(&matrix, 0, 1);
        let a02 = matrix_cell_operand(&matrix, 0, 2);
        let a10 = matrix_cell_operand(&matrix, 1, 0);
        let a11 = matrix_cell_operand(&matrix, 1, 1);
        let a12 = matrix_cell_operand(&matrix, 1, 2);
        let a20 = matrix_cell_operand(&matrix, 2, 0);
        let a21 = matrix_cell_operand(&matrix, 2, 1);
        let a22 = matrix_cell_operand(&matrix, 2, 2);
        let tx = matrix_cell_operand(&matrix, 0, 3);
        let ty = matrix_cell_operand(&matrix, 1, 3);
        let tz = matrix_cell_operand(&matrix, 2, 3);

        // det = a00*(a11*a22-a12*a21) - a01*(a10*a22-a12*a20) + a02*(a10*a21-a11*a20)
        let t0 = self.mul_sub_mul(&a11, &a22, &a12, &a21, elem_ty, expr.span);
        let t1 = self.mul_sub_mul(&a10, &a22, &a12, &a20, elem_ty, expr.span);
        let t2 = self.mul_sub_mul(&a10, &a21, &a11, &a20, elem_ty, expr.span);
        let p0 = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: a00.clone(), rhs: t0 },
            elem_ty,
            expr.span,
        );
        let p1 = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: a01.clone(), rhs: t1 },
            elem_ty,
            expr.span,
        );
        let p2 = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: a02.clone(), rhs: t2 },
            elem_ty,
            expr.span,
        );
        let det01 = self.assign_temp(MirValueKind::Binary { op: MirBinOp::Sub, lhs: p0, rhs: p1 }, elem_ty, expr.span);
        let det = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Add, lhs: det01, rhs: p2 },
            elem_ty,
            expr.span,
        );

        self.lower_matrix_inversa_with_det(det, elem_ty, option_ty, expr, |this, inv_det| {
            this.lower_matrix_3x3_inverse_with_det(
                a00, a01, a02, a10, a11, a12, a20, a21, a22, tx, ty, tz, inv_det, elem_ty, matrix_ty, expr,
            )
        })
    }

    /// Shared singular test: |det| ≤ 1e-6 → nihil; else build inverse via `build`.
    fn lower_matrix_inversa_with_det<F>(
        &mut self,
        det: MirOperand,
        elem_ty: MirType,
        option_ty: MirType,
        expr: &HirExpression,
        build: F,
    ) -> MirOperand
    where
        F: FnOnce(&mut Self, MirOperand) -> MirOperand,
    {
        let eps = MirOperand::Constant(MirConstant::Float(0.000_001));
        let neg_eps = MirOperand::Constant(MirConstant::Float(-0.000_001));
        let gt = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Gt, lhs: det.clone(), rhs: eps },
            MirType::semantic(self.types.primitive(crate::semantic::Primitive::Bivalens)),
            expr.span,
        );
        let lt = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Lt, lhs: det.clone(), rhs: neg_eps },
            MirType::semantic(self.types.primitive(crate::semantic::Primitive::Bivalens)),
            expr.span,
        );
        let invertible = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Or, lhs: gt, rhs: lt },
            MirType::semantic(self.types.primitive(crate::semantic::Primitive::Bivalens)),
            expr.span,
        );

        let result = self.push_temp(option_ty, expr.span);
        let ok_id = self.fresh_block(expr.span);
        let nil_id = self.fresh_block(expr.span);
        let join_id = self.fresh_block(expr.span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: invertible, then_block: ok_id, else_block: nil_id },
            expr.span,
        );

        self.switch_to(ok_id);
        {
            let one = MirOperand::Constant(MirConstant::Float(1.0));
            let inv_det = self.assign_temp(
                MirValueKind::Binary { op: MirBinOp::Div, lhs: one, rhs: det },
                elem_ty,
                expr.span,
            );
            let inverse = build(self, inv_det);
            let some = self.assign_temp(MirValueKind::Option(MirOptionOp::Some(inverse)), option_ty, expr.span);
            let value = self.new_value(MirValueKind::Operand(some), option_ty, expr.span);
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Assign { place: MirPlace::temp(result), value },
                span: expr.span,
            });
            self.terminate_current(MirTerminatorKind::Goto(join_id), expr.span);
        }

        self.switch_to(nil_id);
        {
            let none = self.assign_temp(MirValueKind::Option(MirOptionOp::None), option_ty, expr.span);
            let value = self.new_value(MirValueKind::Operand(none), option_ty, expr.span);
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Assign { place: MirPlace::temp(result), value },
                span: expr.span,
            });
            self.terminate_current(MirTerminatorKind::Goto(join_id), expr.span);
        }

        self.switch_to(join_id);
        MirOperand::Temp(result)
    }

    /// Build the 2×2 matrix inverse given `inv_det` = 1/det.
    ///
    /// For matrix [[a, b], [c, d]], the inverse is:
    /// `[[ d, -b], [-c,  a]] * inv_det`
    fn lower_matrix_2x2_inverse_with_det(
        &mut self,
        a: MirOperand,
        b: MirOperand,
        c: MirOperand,
        d: MirOperand,
        inv_det: MirOperand,
        elem_ty: MirType,
        matrix_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let d_s = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: d, rhs: inv_det.clone() },
            elem_ty,
            expr.span,
        );
        let neg_b = self.assign_temp(MirValueKind::Unary { op: MirUnOp::Neg, operand: b }, elem_ty, expr.span);
        let neg_b_s = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: neg_b, rhs: inv_det.clone() },
            elem_ty,
            expr.span,
        );
        let neg_c = self.assign_temp(MirValueKind::Unary { op: MirUnOp::Neg, operand: c }, elem_ty, expr.span);
        let neg_c_s = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: neg_c, rhs: inv_det.clone() },
            elem_ty,
            expr.span,
        );
        let a_s = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: a, rhs: inv_det },
            elem_ty,
            expr.span,
        );
        self.construct_temp(
            MirAggregateKind::Matrix,
            MirAggregateFields::Ordered(vec![
                MirAggregateItem::Operand(d_s),
                MirAggregateItem::Operand(neg_b_s),
                MirAggregateItem::Operand(neg_c_s),
                MirAggregateItem::Operand(a_s),
            ]),
            matrix_ty,
            expr.span,
        )
    }

    /// Build the 4×4 affine inverse given `inv_det` = 1/det of the upper-left 3×3.
    ///
    /// Computes the 3×3 inverse (i00–i22), then the negated rotated translation
    /// (ntx–ntz), and assembles a 4×4 matrix in Triga column-vector layout.
    fn lower_matrix_3x3_inverse_with_det(
        &mut self,
        a00: MirOperand,
        a01: MirOperand,
        a02: MirOperand,
        a10: MirOperand,
        a11: MirOperand,
        a12: MirOperand,
        a20: MirOperand,
        a21: MirOperand,
        a22: MirOperand,
        tx: MirOperand,
        ty: MirOperand,
        tz: MirOperand,
        inv_det: MirOperand,
        elem_ty: MirType,
        matrix_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let i00 = self.scaled_mul_sub_mul(&a11, &a22, &a12, &a21, &inv_det, elem_ty, expr.span);
        let i01 = self.scaled_mul_sub_mul(&a02, &a21, &a01, &a22, &inv_det, elem_ty, expr.span);
        let i02 = self.scaled_mul_sub_mul(&a01, &a12, &a02, &a11, &inv_det, elem_ty, expr.span);
        let i10 = self.scaled_mul_sub_mul(&a12, &a20, &a10, &a22, &inv_det, elem_ty, expr.span);
        let i11 = self.scaled_mul_sub_mul(&a00, &a22, &a02, &a20, &inv_det, elem_ty, expr.span);
        let i12 = self.scaled_mul_sub_mul(&a02, &a10, &a00, &a12, &inv_det, elem_ty, expr.span);
        let i20 = self.scaled_mul_sub_mul(&a10, &a21, &a11, &a20, &inv_det, elem_ty, expr.span);
        let i21 = self.scaled_mul_sub_mul(&a01, &a20, &a00, &a21, &inv_det, elem_ty, expr.span);
        let i22 = self.scaled_mul_sub_mul(&a00, &a11, &a01, &a10, &inv_det, elem_ty, expr.span);

        // translation' = -R^{-1} * t
        let ntx = self.neg_dot3(&i00, &i01, &i02, &tx, &ty, &tz, elem_ty, expr.span);
        let nty = self.neg_dot3(&i10, &i11, &i12, &tx, &ty, &tz, elem_ty, expr.span);
        let ntz = self.neg_dot3(&i20, &i21, &i22, &tx, &ty, &tz, elem_ty, expr.span);
        let zero = self.assign_temp(
            MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(0.0))),
            elem_ty,
            expr.span,
        );
        let one = self.assign_temp(
            MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(1.0))),
            elem_ty,
            expr.span,
        );
        self.construct_temp(
            MirAggregateKind::Matrix,
            MirAggregateFields::Ordered(vec![
                MirAggregateItem::Operand(i00),
                MirAggregateItem::Operand(i01),
                MirAggregateItem::Operand(i02),
                MirAggregateItem::Operand(ntx),
                MirAggregateItem::Operand(i10),
                MirAggregateItem::Operand(i11),
                MirAggregateItem::Operand(i12),
                MirAggregateItem::Operand(nty),
                MirAggregateItem::Operand(i20),
                MirAggregateItem::Operand(i21),
                MirAggregateItem::Operand(i22),
                MirAggregateItem::Operand(ntz),
                MirAggregateItem::Operand(zero.clone()),
                MirAggregateItem::Operand(zero.clone()),
                MirAggregateItem::Operand(zero),
                MirAggregateItem::Operand(one),
            ]),
            matrix_ty,
            expr.span,
        )
    }

    fn mul_sub_mul(
        &mut self,
        a: &MirOperand,
        b: &MirOperand,
        c: &MirOperand,
        d: &MirOperand,
        elem_ty: MirType,
        span: crate::lexer::Span,
    ) -> MirOperand {
        let ab = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: a.clone(), rhs: b.clone() },
            elem_ty,
            span,
        );
        let cd = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: c.clone(), rhs: d.clone() },
            elem_ty,
            span,
        );
        self.assign_temp(MirValueKind::Binary { op: MirBinOp::Sub, lhs: ab, rhs: cd }, elem_ty, span)
    }

    fn scaled_mul_sub_mul(
        &mut self,
        a: &MirOperand,
        b: &MirOperand,
        c: &MirOperand,
        d: &MirOperand,
        scale: &MirOperand,
        elem_ty: MirType,
        span: crate::lexer::Span,
    ) -> MirOperand {
        let raw = self.mul_sub_mul(a, b, c, d, elem_ty, span);
        self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: raw, rhs: scale.clone() },
            elem_ty,
            span,
        )
    }

    fn neg_dot3(
        &mut self,
        r0: &MirOperand,
        r1: &MirOperand,
        r2: &MirOperand,
        x: &MirOperand,
        y: &MirOperand,
        z: &MirOperand,
        elem_ty: MirType,
        span: crate::lexer::Span,
    ) -> MirOperand {
        let p0 = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: r0.clone(), rhs: x.clone() },
            elem_ty,
            span,
        );
        let p1 = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: r1.clone(), rhs: y.clone() },
            elem_ty,
            span,
        );
        let p2 = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: r2.clone(), rhs: z.clone() },
            elem_ty,
            span,
        );
        let s01 = self.assign_temp(MirValueKind::Binary { op: MirBinOp::Add, lhs: p0, rhs: p1 }, elem_ty, span);
        let sum = self.assign_temp(MirValueKind::Binary { op: MirBinOp::Add, lhs: s01, rhs: p2 }, elem_ty, span);
        self.assign_temp(MirValueKind::Unary { op: MirUnOp::Neg, operand: sum }, elem_ty, span)
    }

    /// Expand matrix product into typed cell multiplies and adds (row-major cells).
    ///
    /// The receiver is [M,K] and the peer is [K,N]; the result is the full
    /// [M,N] grid contracted over K. A peer whose row count differs from the
    /// receiver's K fails closed — columns are never truncated.
    fn lower_matrix_matmul(
        &mut self,
        lhs: MirPlace,
        rhs: MirPlace,
        rows: u8,
        columns: u8,
        rhs_rows: u8,
        rhs_columns: u8,
        elem_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let Some(result_ty) = self.expr_ty(expr) else {
            return self.assign_temp(
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(0.0))),
                elem_ty,
                expr.span,
            );
        };
        if rhs_rows != columns {
            self.errors.push(MirError::unsupported(
                expr.span,
                "matrix matmul inner dimension mismatch before MIR lowering",
            ));
            return self.assign_temp(
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(0.0))),
                elem_ty,
                expr.span,
            );
        }
        // The declared result type must be exactly [rows, rhs_columns]; any
        // other shape means the semantic layer disagrees with the K
        // contraction, which the MIR lane fails closed on rather than
        // truncating or stretching the grid.
        let declared = match self.types.get(result_ty.semantic_id()) {
            Type::Matrix(_, shape) => crate::semantic::literal_matrix_shape(*shape, self.types),
            _ => None,
        };
        if !matches!(declared, Some((r, c)) if r == rows && c == rhs_columns) {
            self.errors.push(MirError::unsupported(
                expr.span,
                "matrix matmul result shape before MIR lowering",
            ));
            return self.assign_temp(
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(0.0))),
                elem_ty,
                expr.span,
            );
        }
        if rows > 4 || rhs_columns > 4 {
            self.errors
                .push(MirError::unsupported(expr.span, "matrix matmul result exceeds 4x4 CPU subset"));
            return self.assign_temp(
                MirValueKind::Operand(MirOperand::Constant(MirConstant::Float(0.0))),
                elem_ty,
                expr.span,
            );
        }
        let mut fields = Vec::with_capacity(usize::from(rows) * usize::from(rhs_columns));
        for row in 0..rows {
            for column in 0..rhs_columns {
                let mut acc = self.assign_temp(
                    MirValueKind::Binary {
                        op: MirBinOp::Mul,
                        lhs: matrix_cell_operand(&lhs, row, 0),
                        rhs: matrix_cell_operand(&rhs, 0, column),
                    },
                    elem_ty,
                    expr.span,
                );
                for k in 1..columns {
                    let product = self.assign_temp(
                        MirValueKind::Binary {
                            op: MirBinOp::Mul,
                            lhs: matrix_cell_operand(&lhs, row, k),
                            rhs: matrix_cell_operand(&rhs, k, column),
                        },
                        elem_ty,
                        expr.span,
                    );
                    acc = self.assign_temp(
                        MirValueKind::Binary { op: MirBinOp::Add, lhs: acc, rhs: product },
                        elem_ty,
                        expr.span,
                    );
                }
                fields.push(MirAggregateItem::Operand(acc));
            }
        }
        self.construct_temp(
            MirAggregateKind::Matrix,
            MirAggregateFields::Ordered(fields),
            result_ty,
            expr.span,
        )
    }

    fn lower_matrix_elementwise_op(
        &mut self,
        lhs: MirPlace,
        rhs: MirPlace,
        rows: u8,
        columns: u8,
        op: MirBinOp,
        elem_ty: MirType,
        matrix_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let mut fields = Vec::with_capacity(usize::from(rows) * usize::from(columns));
        for row in 0..rows {
            for column in 0..columns {
                let lhs = matrix_cell_operand(&lhs, row, column);
                let rhs = matrix_cell_operand(&rhs, row, column);
                let value = self.assign_temp(MirValueKind::Binary { op, lhs, rhs }, elem_ty, expr.span);
                fields.push(MirAggregateItem::Operand(value));
            }
        }
        self.construct_temp(
            MirAggregateKind::Matrix,
            MirAggregateFields::Ordered(fields),
            matrix_ty,
            expr.span,
        )
    }

    fn lower_vector_elementwise_op(
        &mut self,
        lhs: MirPlace,
        rhs: MirPlace,
        width: u8,
        op: MirBinOp,
        elem_ty: MirType,
        vector_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let mut fields = Vec::with_capacity(width as usize);
        for lane in 0..width {
            let lhs = vector_lane_operand(&lhs, lane);
            let rhs = vector_lane_operand(&rhs, lane);
            let value = self.assign_temp(MirValueKind::Binary { op, lhs, rhs }, elem_ty, expr.span);
            fields.push(MirAggregateItem::Operand(value));
        }
        self.construct_temp(
            MirAggregateKind::Vector,
            MirAggregateFields::Ordered(fields),
            vector_ty,
            expr.span,
        )
    }

    fn lower_vector_cross(
        &mut self,
        lhs: MirPlace,
        rhs: MirPlace,
        elem_ty: MirType,
        vector_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let lanes = [(1, 2, 2, 1), (2, 0, 0, 2), (0, 1, 1, 0)];
        let fields = lanes
            .into_iter()
            .map(|(lhs_a, rhs_a, lhs_b, rhs_b)| {
                let a = self.assign_temp(
                    MirValueKind::Binary {
                        op: MirBinOp::Mul,
                        lhs: vector_lane_operand(&lhs, lhs_a),
                        rhs: vector_lane_operand(&rhs, rhs_a),
                    },
                    elem_ty,
                    expr.span,
                );
                let b = self.assign_temp(
                    MirValueKind::Binary {
                        op: MirBinOp::Mul,
                        lhs: vector_lane_operand(&lhs, lhs_b),
                        rhs: vector_lane_operand(&rhs, rhs_b),
                    },
                    elem_ty,
                    expr.span,
                );
                MirAggregateItem::Operand(self.assign_temp(
                    MirValueKind::Binary { op: MirBinOp::Sub, lhs: a, rhs: b },
                    elem_ty,
                    expr.span,
                ))
            })
            .collect();
        self.construct_temp(
            MirAggregateKind::Vector,
            MirAggregateFields::Ordered(fields),
            vector_ty,
            expr.span,
        )
    }

    fn lower_vector_dot(
        &mut self,
        lhs: MirPlace,
        rhs: MirPlace,
        width: u8,
        elem_ty: MirType,
        expr: &HirExpression,
    ) -> MirOperand {
        let mut products = Vec::with_capacity(width as usize);
        for lane in 0..width {
            let lhs = vector_lane_operand(&lhs, lane);
            let rhs = vector_lane_operand(&rhs, lane);
            products.push(self.assign_temp(MirValueKind::Binary { op: MirBinOp::Mul, lhs, rhs }, elem_ty, expr.span));
        }
        let mut acc = products.remove(0);
        for product in products {
            acc = self.assign_temp(
                MirValueKind::Binary { op: MirBinOp::Add, lhs: acc, rhs: product },
                elem_ty,
                expr.span,
            );
        }
        acc
    }

    fn lower_vector_swizzle(
        &mut self,
        source: MirPlace,
        width: u8,
        vector_ty: MirType,
        pattern_expr: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let Some(interner) = self.context.interner else {
            self.errors
                .push(MirError::unsupported(expr.span, "vector swizzle interner before MIR lowering"));
            return None;
        };
        let HirExpressionKind::Literal(HirLiteral::String(pattern)) = &pattern_expr.kind else {
            self.errors.push(MirError::unsupported(
                pattern_expr.span,
                "vector swizzle literal before MIR lowering",
            ));
            return None;
        };
        let pattern_span = pattern_expr.span;
        let pattern = interner.resolve(*pattern);
        if pattern.chars().count() != usize::from(width) {
            self.errors
                .push(MirError::unsupported(pattern_span, "vector swizzle width before MIR lowering"));
            return None;
        }

        let mut fields = Vec::with_capacity(usize::from(width));
        for lane in pattern.chars().map(vector_swizzle_lane) {
            let Some(lane) = lane else {
                self.errors
                    .push(MirError::unsupported(pattern_span, "vector swizzle lane before MIR lowering"));
                return None;
            };
            if lane >= width {
                self.errors
                    .push(MirError::unsupported(pattern_span, "vector swizzle lane before MIR lowering"));
                return None;
            }
            fields.push(MirAggregateItem::Operand(vector_lane_operand(&source, lane)));
        }

        Some(self.construct_temp(
            MirAggregateKind::Vector,
            MirAggregateFields::Ordered(fields),
            vector_ty,
            expr.span,
        ))
    }

    fn value_base_place(&mut self, operand: MirOperand, ty: MirType, span: Span) -> Option<MirPlace> {
        match operand {
            MirOperand::Place(place) => Some(place),
            MirOperand::Temp(id) => Some(MirPlace::temp(id)),
            MirOperand::Value(_) | MirOperand::Constant(_) => {
                let temp = self.push_temp(ty, span);
                self.assign(MirPlace::temp(temp), operand, ty, span);
                Some(MirPlace::temp(temp))
            }
        }
    }

    /// Lower the GI3-1 SiLU activation as existing tensor elementwise MIR.
    ///
    /// `silu(x) = x / (1 + exp(-x))`: TensorNeg, tensor Exp, TensorAdd, and
    /// TensorDiv are the complete composition. Keeping this expansion here
    /// means the registry admits the method without inventing TensorSilu.
    fn lower_tensor_silu(&mut self, receiver: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        let tensor_ty = self.expr_ty(expr)?;
        let x = self.lower_expr_value(receiver)?;
        let negated = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorNeg),
            vec![x.clone()],
            tensor_ty,
            expr.span,
        );
        let exponent =
            self.assign_temp(MirValueKind::Unary { op: MirUnOp::Exp, operand: negated }, tensor_ty, expr.span);
        let denominator = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorAdd),
            vec![exponent, MirOperand::Constant(MirConstant::Float(1.0))],
            tensor_ty,
            expr.span,
        );
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorDiv),
            vec![x, denominator],
            tensor_ty,
            expr.span,
        ))
    }

    /// Lower compiler-owned numeric operator methods into ordinary MIR.
    ///
    /// WHY: these receiver names are glyph twins (`x.addita(y)` means `x + y`,
    /// `acc.adde(y)` means `acc ← acc + y`). Keeping them as runtime calls would
    /// make the runner execute a late provider surface that the language treats
    /// as primitive arithmetic.
    fn lower_numeric_operator_method(
        &mut self,
        receiver: &HirExpression,
        method_name: &str,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let receiver_ty = receiver.ty?;
        let normalized = self.normalized_type(receiver_ty);
        let entry = crate::intrinsics::lookup_method_for_type(normalized, method_name, args.len())?;
        if !matches!(entry.receiver, crate::intrinsics::IntrinsicReceiver::Numeric) {
            return None;
        }

        // `potentia` is a CodegenOnly registry row (no `MirCollectionOp`
        // surface). Fractus receivers use the exponentiation identity the Rust
        // codegen uses (`emit_potentia_expr`): `base ≤ 0 → 0`, else
        // `(exp · ln(base)).exp()` — the "e^x via exponentiation intrinsic"
        // identity gradus nn.fab `_exponens` documents. Numerus receivers use
        // exact integer exponentiation below; both paths compose into ordinary
        // MIR so the runner does not see a late provider method call.
        if method_name == "potentia" {
            return self.lower_potentia(receiver, receiver_ty, args, expr);
        }

        if let Some(op) = numeric_operator_binop(method_name) {
            let [arg] = args else {
                return None;
            };
            let lhs = self.lower_expr_value(receiver)?;
            let rhs = self.lower_expr_value(&arg.expr)?;
            let ty = self.expr_ty(expr)?;
            return Some(self.assign_temp(MirValueKind::Binary { op, lhs, rhs }, ty, expr.span));
        }

        // WHY: on modular receivers `subtrahe` is the value-returning wrapping
        // twin (matching typecheck), not the in-place numerus mutation.
        if matches!(normalized, Type::ModularWord(_)) && method_name == "subtrahe" {
            let [arg] = args else {
                return None;
            };
            let lhs = self.lower_expr_value(receiver)?;
            let rhs = self.lower_expr_value(&arg.expr)?;
            let ty = self.expr_ty(expr)?;
            return Some(self.assign_temp(MirValueKind::Binary { op: MirBinOp::Sub, lhs, rhs }, ty, expr.span));
        }

        if let Some(op) = numeric_imperativus_binop(method_name) {
            let [arg] = args else {
                return None;
            };
            let ty = self.expr_ty(receiver)?;
            let (place, ty) = self.lower_assignment_place_with_fallback(receiver, ty)?;
            let lhs = MirOperand::Place(place.clone());
            let rhs = self.lower_expr_value(&arg.expr)?;
            let value = self.assign_temp(MirValueKind::Binary { op, lhs, rhs }, ty, expr.span);
            self.assign(place, value, ty, expr.span);
            return Some(MirOperand::Constant(MirConstant::Unit));
        }

        None
    }

    /// Lower `potentia` by receiver family.
    fn lower_potentia(
        &mut self,
        receiver: &HirExpression,
        receiver_ty: TypeId,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        match self.normalized_type(receiver_ty) {
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _) => {
                self.lower_integer_potentia(receiver, args, expr)
            }
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
                self.lower_fractus_potentia(receiver, args, expr)
            }
            _ => None,
        }
    }

    /// Lower integer `potentia` with exponentiation by repeated squaring.
    ///
    /// Shape: `accumulator = 1`; while `exponent > 0`, multiply the
    /// accumulator for an odd exponent, halve the exponent, then square the
    /// base only when another iteration remains. This is O(log exponent) in
    /// multiplications, preserves negative-base signs without a float round
    /// trip, and keeps overflow under checked integer MIR arithmetic.
    fn lower_integer_potentia(
        &mut self,
        receiver: &HirExpression,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let [arg] = args else {
            return None;
        };
        let base = self.lower_expr_value(receiver)?;
        let exponent = self.lower_expr_value(&arg.expr)?;
        let destination = self.push_temp(ty, expr.span);
        let accumulator = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: accumulator, name: None, ty, mutable: true, span: expr.span });
        let base_local = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: base_local, name: None, ty, mutable: true, span: expr.span });
        let exponent_local = self.next_local_id();
        self.locals
            .push(MirLocalDecl { id: exponent_local, name: None, ty, mutable: true, span: expr.span });
        let accumulator_place = MirPlace::local(accumulator);
        let base_place = MirPlace::local(base_local);
        let exponent_place = MirPlace::local(exponent_local);
        self.assign(
            accumulator_place.clone(),
            MirOperand::Constant(MirConstant::Int(1)),
            ty,
            expr.span,
        );
        self.assign(base_place.clone(), base, ty, expr.span);
        self.assign(exponent_place.clone(), exponent, ty, expr.span);

        let bool_ty = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let loop_check = self.fresh_block(expr.span);
        let body = self.fresh_block(expr.span);
        let multiply = self.fresh_block(expr.span);
        let shift = self.fresh_block(expr.span);
        let square = self.fresh_block(expr.span);
        let done = self.fresh_block(expr.span);
        self.terminate_current(MirTerminatorKind::Goto(loop_check), expr.span);

        self.switch_to(loop_check);
        let has_exponent = self.binary_temp(
            MirBinOp::Gt,
            MirOperand::Place(exponent_place.clone()),
            MirOperand::Constant(MirConstant::Int(0)),
            bool_ty,
            expr.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: has_exponent, then_block: body, else_block: done },
            expr.span,
        );

        self.switch_to(body);
        let odd_part = self.binary_temp(
            MirBinOp::Mod,
            MirOperand::Place(exponent_place.clone()),
            MirOperand::Constant(MirConstant::Int(2)),
            ty,
            expr.span,
        );
        let is_odd = self.binary_temp(
            MirBinOp::NotEq,
            odd_part,
            MirOperand::Constant(MirConstant::Int(0)),
            bool_ty,
            expr.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: is_odd, then_block: multiply, else_block: shift },
            expr.span,
        );

        self.switch_to(multiply);
        let product = self.binary_temp(
            MirBinOp::Mul,
            MirOperand::Place(accumulator_place.clone()),
            MirOperand::Place(base_place.clone()),
            ty,
            expr.span,
        );
        self.assign(accumulator_place.clone(), product, ty, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(shift), expr.span);

        self.switch_to(shift);
        let half = self.binary_temp(
            MirBinOp::Div,
            MirOperand::Place(exponent_place.clone()),
            MirOperand::Constant(MirConstant::Int(2)),
            ty,
            expr.span,
        );
        self.assign(exponent_place.clone(), half, ty, expr.span);
        let has_more = self.binary_temp(
            MirBinOp::Gt,
            MirOperand::Place(exponent_place.clone()),
            MirOperand::Constant(MirConstant::Int(0)),
            bool_ty,
            expr.span,
        );
        self.terminate_current(
            MirTerminatorKind::Branch { condition: has_more, then_block: square, else_block: loop_check },
            expr.span,
        );

        self.switch_to(square);
        let squared = self.binary_temp(
            MirBinOp::Mul,
            MirOperand::Place(base_place.clone()),
            MirOperand::Place(base_place.clone()),
            ty,
            expr.span,
        );
        self.assign(base_place, squared, ty, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(loop_check), expr.span);

        self.switch_to(done);
        self.assign(MirPlace::temp(destination), MirOperand::Place(accumulator_place), ty, expr.span);
        Some(MirOperand::Temp(destination))
    }

    /// Lower `potentia` for a fractus-family receiver as the exponentiation
    /// identity (see the call site in [`Self::lower_numeric_operator_method`]).
    ///
    /// Shape: `base ≤ 0 → 0`, else `(exp · ln(base)).exp()`, composed from
    /// existing `MirUnOp::{Log, Exp}`, `MirBinOp::{LtEq, Mul}`, and a guard
    /// branch. The guard is mandatory: the runner's `MirUnOp::Log` rejects
    /// non-positive input, and the Rust codegen lane returns `0` for
    /// `base ≤ 0` — matching it keeps lanes aligned.
    fn lower_fractus_potentia(
        &mut self,
        receiver: &HirExpression,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let base = self.lower_expr_value(receiver)?;
        let [arg] = args else {
            return None;
        };
        let exponent = self.lower_expr_value(&arg.expr)?;

        let destination = self.push_temp(ty, expr.span);
        let place = MirPlace::temp(destination);

        let bool_ty = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let condition = self.assign_temp(
            MirValueKind::Binary {
                op: MirBinOp::LtEq,
                lhs: base.clone(),
                rhs: MirOperand::Constant(MirConstant::Float(0.0)),
            },
            bool_ty,
            expr.span,
        );

        let guard_block = self.fresh_block(expr.span);
        let power_block = self.fresh_block(expr.span);
        let join = self.fresh_block(expr.span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition, then_block: guard_block, else_block: power_block },
            expr.span,
        );

        // base ≤ 0 → 0
        self.switch_to(guard_block);
        self.assign(place.clone(), MirOperand::Constant(MirConstant::Float(0.0)), ty, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(join), expr.span);

        // base > 0 → (exponent · ln(base)).exp()
        self.switch_to(power_block);
        let log = self.assign_temp(MirValueKind::Unary { op: MirUnOp::Log, operand: base }, ty, expr.span);
        let product = self.assign_temp(
            MirValueKind::Binary { op: MirBinOp::Mul, lhs: exponent, rhs: log },
            ty,
            expr.span,
        );
        let raised = self.assign_temp(MirValueKind::Unary { op: MirUnOp::Exp, operand: product }, ty, expr.span);
        self.assign(place, raised, ty, expr.span);
        self.terminate_current(MirTerminatorKind::Goto(join), expr.span);

        self.switch_to(join);
        Some(MirOperand::Temp(destination))
    }

    /// Lower `lista.ordina()` as an in-place update using the existing sorted
    /// collection result operation.
    fn lower_lista_ordina(
        &mut self,
        receiver: &HirExpression,
        method_name: &str,
        args: &[HirCallArg],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if method_name != "ordina" || !args.is_empty() {
            return None;
        }
        let receiver_ty = receiver.ty?;
        if !matches!(self.normalized_type(receiver_ty), Type::Array(_)) {
            return None;
        }

        let collection_ty = self.expr_ty(receiver)?;
        let (place, ty) = self.lower_assignment_place_with_fallback(receiver, collection_ty)?;
        let sorted = self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::Sort),
            vec![MirOperand::Place(place.clone())],
            ty,
            expr.span,
        );
        self.assign(place, sorted, ty, expr.span);
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    pub(super) fn lower_direct_call_args(
        &mut self,
        callee: DefId,
        args: &[HirCallArg],
        span: Span,
    ) -> Option<Vec<MirOperand>> {
        if args.len() == 1 && args[0].spread {
            if let Some(params) = self.context.function_params.get(&callee) {
                if params.len() > 1 && params.iter().all(|param| !param.optional) {
                    return self.lower_spread_call_args(&args[0].expr, params.len(), span);
                }
            }
        }

        if let Some(params) = self.context.callable_param_lower_meta(callee) {
            let mut lowered = Vec::with_capacity(params.len());
            for (index, (optional, default)) in params.iter().enumerate() {
                if let Some(arg) = args.get(index) {
                    lowered.push(self.lower_expr_value(&arg.expr)?);
                } else if let Some(default) = default {
                    lowered.push(self.lower_expr_value(default)?);
                } else if *optional {
                    lowered.push(self.omitted_optional_operand(span));
                } else {
                    self.errors.push(MirError::unsupported(
                        span,
                        "call missing required argument before MIR lowering completed",
                    ));
                    return None;
                }
            }
            return Some(lowered);
        }

        let mut lowered = Vec::with_capacity(args.len());
        for arg in args {
            lowered.push(self.lower_expr_value(&arg.expr)?);
        }
        Some(lowered)
    }

    fn lower_spread_call_args(&mut self, spread: &HirExpression, arity: usize, span: Span) -> Option<Vec<MirOperand>> {
        let collection = self.lower_expr_value(spread)?;
        let collection_ty = self.expr_ty(spread)?;
        let temp = self.push_temp(collection_ty, span);
        self.assign(MirPlace::temp(temp), collection, collection_ty, span);
        let mut args = Vec::with_capacity(arity);
        for index in 0..arity {
            #[expect(
                clippy::cast_possible_wrap,
                reason = "call arity is bounded by source program size, far below i64::MAX"
            )]
            let index_operand = MirOperand::Constant(MirConstant::Int(index as i64));
            args.push(MirOperand::Place(MirPlace {
                base: crate::mir::MirPlaceBase::Temp(temp),
                projections: vec![MirProjection::Index(index_operand)],
            }));
        }
        Some(args)
    }

    fn genus_method_target(&self, receiver: &HirExpression, method: Symbol) -> Option<MethodTarget> {
        let receiver_ty = receiver.ty?;
        let def_id = self.method_receiver_struct_def(receiver_ty)?;
        self.context.method_targets.get(&(def_id, method)).copied()
    }

    /// Lower a direct call, enum variant constructor, or failable call.
    ///
    /// Path callees resolve to definition calls, provider imports, or variant
    /// constructors. Locals holding callable values lower through
    /// [`MirCallee::Value`].
    pub(super) fn lower_call(
        &mut self,
        callee: &HirExpression,
        type_args: &[TypeId],
        args: &[HirCallArg],
        conversion: Option<DefId>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if let HirExpressionKind::NonNull(_, HirNonNullKind::Member(name)) = callee.kind {
            return self.lower_method_call(callee, name, args, conversion, expr);
        }

        // Callable value: a local binding or non-path expression. Positional args,
        // indirect call via MirCallee::Value.
        let value_callee = match &callee.kind {
            HirExpressionKind::Path(def_id) if self.bindings.contains_key(def_id) => {
                Some(self.lower_path(*def_id, callee)?)
            }
            HirExpressionKind::Path(_) => None,
            _ => Some(self.lower_expr_value(callee)?),
        };
        if let Some(callee_operand) = value_callee {
            if !type_args.is_empty() {
                self.errors
                    .push(MirError::unsupported(expr.span, "call-site type arguments on callable values"));
                return None;
            }
            let lowered_args = self.lower_positional_call_args(args)?;
            let ty = self.expr_ty(expr)?;
            if let Some(error_ty) = self.callable_error_ty(callee) {
                return self.lower_failable_call(
                    self.closure_callee_for_operand(&callee_operand)
                        .map_or_else(|| MirCallee::Value(callee_operand.clone()), MirCallee::Closure),
                    lowered_args,
                    ty,
                    error_ty,
                    conversion,
                    expr.span,
                );
            }
            return Some(self.call_value_operand(callee_operand, lowered_args, ty, expr.span));
        }

        // Direct definition call: spread/default-aware arg lowering, then dispatch
        // to provider import, variant constructor, or definition body.
        let HirExpressionKind::Path(def_id) = &callee.kind else {
            unreachable!("non-path callees are handled by the value_callee branch");
        };

        if let Some(builtin) = self.context.gpu_builtins.get(def_id).copied() {
            if !args.is_empty() {
                self.errors
                    .push(MirError::unsupported(expr.span, "GPU builtin call with arguments"));
                return None;
            }
            let ty = self.expr_ty(expr)?;
            return Some(self.runtime_call_value(MirIntrinsic::GpuBuiltin(builtin), Vec::new(), ty, expr.span));
        }

        let lowered_args = self.lower_direct_call_args(*def_id, args, expr.span)?;
        let ty = self.expr_ty(expr)?;

        if let Some(import) = self.context.provider_imports.get(def_id).cloned() {
            return Some(self.runtime_call_value(
                MirIntrinsic::Provider(MirProvider { kind: import.kind, module: import.module, name: import.item }),
                lowered_args,
                ty,
                expr.span,
            ));
        }

        if self.context.variant_parents.contains_key(def_id) {
            let fields = self.variant_payload(*def_id, lowered_args, expr.span);
            return Some(self.construct_temp(MirAggregateKind::EnumVariant(*def_id), fields, ty, expr.span));
        }

        let explicit_type_args = type_args.iter().copied().map(MirType::semantic).collect();

        // Package-MIR failable-call fallback: a callee rewritten to a
        // synthetic definition id (the faber linker's `Path(synthetic)` for a
        // linked library export) is absent from this unit's `function_errors`
        // registry — the id is allocated after analysis. The callee's HIR
        // type is then the only source of truth for failable-ness. Without
        // the try-call edge the cape handler is never referenced and its
        // `si`-chain blocks are orphaned (PKG001 pkg001-mir-fix). Known
        // generator/cursor-stream defs stay on their own route.
        if !self.context.function_errors.contains_key(def_id)
            && !self
                .context
                .function_generator
                .get(def_id)
                .copied()
                .unwrap_or(false)
        {
            if let Some(error_ty) = self.callable_error_ty(callee) {
                let callee = MirCallee::definition_with_type_args(*def_id, explicit_type_args);
                return self.lower_failable_call(callee, lowered_args, ty, error_ty, conversion, expr.span);
            }
        }

        self.lower_definition_call(*def_id, explicit_type_args, lowered_args, conversion, expr)
    }

    pub(super) fn lower_positional_call_args(&mut self, args: &[HirCallArg]) -> Option<Vec<MirOperand>> {
        let mut lowered = Vec::with_capacity(args.len());
        for arg in args {
            lowered.push(self.lower_expr_value(&arg.expr)?);
        }
        Some(lowered)
    }

    fn lower_propagated_error(
        &mut self,
        error_temp: MirTempId,
        destination_ty: MirType,
        conversion: Option<DefId>,
        span: Span,
    ) -> MirOperand {
        let Some(arm) = conversion else {
            return MirOperand::Temp(error_temp);
        };
        let converted = self.push_temp(destination_ty, span);
        self.append_stmt(MirStatement {
            kind: MirStatementKind::Call {
                destination: Some(MirPlace::temp(converted)),
                callee: MirCallee::definition(arm),
                args: vec![MirOperand::Temp(error_temp)],
            },
            span,
        });
        MirOperand::Temp(converted)
    }

    // consumes type_args and args into MIR statements
    fn lower_definition_call(
        &mut self,
        def_id: DefId,
        explicit_type_args: Vec<MirType>,
        lowered_args: Vec<MirOperand>,
        conversion: Option<DefId>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        let callee = MirCallee::definition_with_type_args(def_id, explicit_type_args);

        if self
            .context
            .function_generator
            .get(&def_id)
            .copied()
            .unwrap_or(false)
        {
            return Some(self.runtime_call_value(
                MirIntrinsic::CursorStream(crate::mir::MirCursorStream { callee: def_id }),
                lowered_args,
                ty,
                expr.span,
            ));
        }

        if let Some(err_ty) = self.context.function_errors.get(&def_id).copied() {
            return self.lower_failable_call(callee, lowered_args, ty, err_ty, conversion, expr.span);
        }

        if self.is_vacuum(ty) {
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Call { destination: None, callee: callee.clone(), args: lowered_args },
                span: expr.span,
            });
            return Some(MirOperand::Constant(MirConstant::Unit));
        }

        let destination = self.push_temp(ty, expr.span);
        self.append_stmt(MirStatement {
            kind: MirStatementKind::Call { destination: Some(MirPlace::temp(destination)), callee, args: lowered_args },
            span: expr.span,
        });
        Some(MirOperand::Temp(destination))
    }

    pub(super) fn callable_error_ty(&self, callee: &HirExpression) -> Option<MirType> {
        fn error_ty(types: &TypeTable, ty: TypeId) -> Option<MirType> {
            match types.get(ty) {
                Type::Func(signature) => signature.err.map(MirType::semantic),
                Type::Alias(_, inner) => error_ty(types, *inner),
                _ => None,
            }
        }
        callee.ty.and_then(|ty| error_ty(self.types, ty))
    }

    // consumes callee and args into MIR statements
    pub(super) fn lower_failable_call(
        &mut self,
        callee: MirCallee,
        lowered_args: Vec<MirOperand>,
        ty: MirType,
        err_ty: MirType,
        conversion: Option<DefId>,
        span: Span,
    ) -> Option<MirOperand> {
        if let Some(handler) = self.handlers.last().cloned() {
            let ok_block = self.fresh_block(span);
            if self.is_vacuum(ty) {
                self.terminate_current(
                    MirTerminatorKind::TryCall {
                        destination: None,
                        callee,
                        args: lowered_args,
                        ok_block,
                        error_place: handler.error_place,
                        error_block: handler.error_block,
                    },
                    span,
                );
                self.switch_to(ok_block);
                return Some(MirOperand::Constant(MirConstant::Unit));
            }

            let destination = self.push_temp(ty, span);
            self.terminate_current(
                MirTerminatorKind::TryCall {
                    destination: Some(MirPlace::temp(destination)),
                    callee,
                    args: lowered_args,
                    ok_block,
                    error_place: handler.error_place,
                    error_block: handler.error_block,
                },
                span,
            );
            self.switch_to(ok_block);
            return Some(MirOperand::Temp(destination));
        }

        let Some(destination_ty) = self.error_ty else {
            self.errors.push(MirError::unsupported(
                span,
                "failable call without an active local cape handler",
            ));
            return None;
        };

        let ok_block = self.fresh_block(span);
        let error_block = self.fresh_block(span);
        let error_temp = self.push_temp(err_ty, span);
        if self.is_vacuum(ty) {
            self.terminate_current(
                MirTerminatorKind::TryCall {
                    destination: None,
                    callee,
                    args: lowered_args,
                    ok_block,
                    error_place: MirPlace::temp(error_temp),
                    error_block,
                },
                span,
            );
            self.switch_to(error_block);
            let propagated = self.lower_propagated_error(error_temp, destination_ty, conversion, span);
            self.terminate_current(MirTerminatorKind::ReturnError(propagated), span);
            self.switch_to(ok_block);
            return Some(MirOperand::Constant(MirConstant::Unit));
        }

        let destination = self.push_temp(ty, span);
        self.terminate_current(
            MirTerminatorKind::TryCall {
                destination: Some(MirPlace::temp(destination)),
                callee,
                args: lowered_args,
                ok_block,
                error_place: MirPlace::temp(error_temp),
                error_block,
            },
            span,
        );
        self.switch_to(error_block);
        let propagated = self.lower_propagated_error(error_temp, destination_ty, conversion, span);
        self.terminate_current(MirTerminatorKind::ReturnError(propagated), span);
        self.switch_to(ok_block);
        Some(MirOperand::Temp(destination))
    }

    /// Select the strict read carrier for a declared literal-bound receiver.
    ///
    /// MIR values do not carry the declared receiver type at runtime. Using a
    /// non-optional return type on the existing read op is the lower-time
    /// checked/lookup variant: the runner traps for strict reads and keeps
    /// `Option` lookup behavior for dynamic receivers.
    pub(super) fn checked_read_result_type(
        &self,
        op: MirCollectionOp,
        receiver_ty: Option<TypeId>,
        expr_ty: MirType,
    ) -> Option<MirType> {
        let receiver_ty = receiver_ty?;
        let result = self.types.nullable_inner(expr_ty.semantic_id())?;
        let checked = match op {
            MirCollectionOp::Index => self.declared_literal_bound(receiver_ty),
            MirCollectionOp::TensorGet => self.declared_literal_tensor_shape(receiver_ty),
            _ => false,
        };
        checked.then_some(MirType::semantic(result))
    }

    fn declared_literal_bound(&self, ty: TypeId) -> bool {
        let ty = self.normalized_type(ty);
        let capacity = match ty {
            Type::BoundedArray { capacity, .. }
            | Type::BoundedTextus { capacity }
            | Type::BoundedAscii { capacity }
            | Type::BoundedOcteti { capacity } => *capacity,
            _ => return false,
        };
        matches!(self.types.get_index(capacity), radix_types::IndexExpr::Literal(_))
    }

    fn declared_literal_tensor_shape(&self, ty: TypeId) -> bool {
        let Type::Tensor(_, shape) = self.normalized_type(ty) else {
            return false;
        };
        match self.types.get_index(*shape) {
            radix_types::IndexExpr::Literal(_) => true,
            radix_types::IndexExpr::Tuple(dims) => dims
                .iter()
                .all(|dim| matches!(self.types.get_index(*dim), radix_types::IndexExpr::Literal(_))),
            radix_types::IndexExpr::Param(_)
            | radix_types::IndexExpr::Infer(_)
            | radix_types::IndexExpr::Unspecified => false,
        }
    }

    /// Resolve a HIR method call into the small set of collection intrinsics
    /// currently represented in MIR.
    fn collection_method_op(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        args: &[HirCallArg],
        span: Span,
    ) -> Option<MirCollectionOp> {
        let Some(receiver_ty) = receiver.ty else {
            self.errors
                .push(MirError::missing_type(span, "collection method receiver"));
            return None;
        };
        let method_name = self.resolve_method_name(method)?;
        let payload = self.types.get(self.method_receiver_payload_id(receiver_ty));
        let entry = crate::intrinsics::lookup_method_for_type(payload, method_name, args.len())?;
        crate::intrinsics::mir_collection_op(entry)
    }

    fn resolve_method_name(&self, method: Symbol) -> Option<&str> {
        self.context
            .interner
            .map(|interner| interner.resolve(method))
    }

    /// Lower `mori`/panic into a runtime call followed by an unreachable
    /// terminator.
    pub(super) fn lower_mori(&mut self, value: &HirExpression, span: Span) -> Option<MirOperand> {
        let value = self.lower_expr_value(value)?;
        let numquam = MirType::semantic(self.types.primitive(Primitive::Numquam));
        self.append_stmt(MirStatement {
            kind: MirStatementKind::RuntimeCall {
                destination: None,
                call: crate::mir::MirRuntimeCall {
                    intrinsic: crate::mir::MirIntrinsic::Panic,
                    args: vec![value],
                    return_ty: numquam,
                },
            },
            span,
        });
        self.terminate_current(MirTerminatorKind::Unreachable, span);
        None
    }

    /// Lower `lege` into a target-neutral stdin read intrinsic.
    pub(super) fn lower_read_line(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(MirIntrinsic::ReadLine, Vec::new(), ty, expr.span))
    }

    /// Lower diagnostic output expressions to diagnostic intrinsics.
    pub(super) fn lower_scribe(
        &mut self,
        kind: HirScribeKind,
        args: &[HirExpression],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let intrinsic = MirIntrinsic::Diagnostic(mir_diagnostic_kind(kind));
        let return_ty = self.expr_ty(expr)?;
        let template = args
            .len()
            .checked_sub(1)
            .and_then(|index| self.context.diagnostic_format_templates.get(index))
            .copied();
        let has_literal_template = args
            .first()
            .is_some_and(|arg| matches!(arg.kind, HirExpressionKind::Literal(HirLiteral::String(_))));
        if let Some(template) = template {
            if has_literal_template && args.len() > 1 && args.len() <= 3 {
                let mut lowered_args = Vec::with_capacity(args.len());
                for arg in args {
                    lowered_args.push(self.lower_expr_value(arg)?);
                }
                let text_ty = MirType::semantic(self.types.primitive(Primitive::Textus));
                let formatted =
                    self.runtime_call_value(MirIntrinsic::FormatString { template }, lowered_args, text_ty, expr.span);
                self.runtime_call_value(intrinsic, vec![formatted], return_ty, expr.span);
                return Some(MirOperand::Constant(MirConstant::Unit));
            }
        }
        // L23 (d57baa50): the multi-arg grouping carries the HIR-Rust lane's
        // space-join for EVERY arg count — one diagnostic call with the joined
        // payload instead of one call per argument. The previous gate limited
        // the grouping to `> 3` scalar arguments, so `nota a, b` emitted two
        // separate lines while the Rust oracle prints one space-joined line.
        // The grouping is shared lowering: every MIR consumer (LLVM host,
        // runner, wasm) renders the joined textus the same way.
        let text_ty = MirType::semantic(self.types.primitive(Primitive::Textus));
        if args.len() > 1
            && args.iter().all(|arg| {
                arg.ty
                    .is_some_and(|ty| self.scribe_arg_renders_to_text(MirType::semantic(ty), text_ty))
            })
        {
            let separator = MirOperand::Constant(MirConstant::String(self.context.diagnostic_separator));
            let mut rendered = None;
            for arg in args {
                let arg_ty = self.expr_ty(arg)?;
                let value = self.lower_expr_value(arg)?;
                let value = self.render_scribe_value_text(value, arg_ty, text_ty, expr.span);
                rendered = Some(match rendered {
                    None => value,
                    Some(current) => {
                        let separated = self.assign_temp(
                            MirValueKind::Binary { op: MirBinOp::Add, lhs: current, rhs: separator.clone() },
                            text_ty,
                            expr.span,
                        );
                        self.assign_temp(
                            MirValueKind::Binary { op: MirBinOp::Add, lhs: separated, rhs: value },
                            text_ty,
                            expr.span,
                        )
                    }
                });
            }
            let rendered = rendered?;
            self.runtime_call_value(intrinsic, vec![rendered], return_ty, expr.span);
            return Some(MirOperand::Constant(MirConstant::Unit));
        }
        for arg in args {
            let arg = self.lower_expr_value(arg)?;
            self.runtime_call_value(intrinsic.clone(), vec![arg], return_ty, expr.span);
        }
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    /// Whether a scribe argument type can render to textus through shared MIR
    /// operations (the grouping predicate). Mirrors the HIR-Rust lane's
    /// per-argument display: textus passes through, bare numeric primitives
    /// convert or format (`display_fractus` semantics for fractus), opaque
    /// collections format in their Debug shape, and option values render
    /// their payload or `nihil` via an explicit presence branch. Sized
    /// numeric/fractus widths and types without a shared render (instans
    /// Debug shape, regex, structs, nested options) keep the per-argument
    /// diagnostic path below, which retains its specialized carriers.
    fn scribe_arg_renders_to_text(&self, arg_ty: MirType, text_ty: MirType) -> bool {
        if arg_ty == text_ty {
            return true;
        }
        match self.normalized_type(arg_ty.semantic_id()) {
            Type::Primitive(
                Primitive::Bivalens | Primitive::Numerus | Primitive::Fractus | Primitive::Octeti | Primitive::Valor,
            )
            | Type::Array(_)
            | Type::Map(_, _)
            | Type::Set(_) => true,
            // L28 (ab91f49f): sized `fractus<f32>` and `instans` values now
            // render through shared MIR operations too — f32 via the
            // FormatString f32 display carrier (`display_fractus` semantics)
            // and instans via its Rust-oracle Debug-shape opaque format
            // carrier — so multi-arg notas carrying them join like the
            // HIR-Rust lane instead of falling to the per-argument path.
            // This is the FLAGGED continuation of the L23 grouping predicate
            // (the rows L23 explicitly left open); bounded to these two arms.
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32) => true,
            Type::Primitive(Primitive::Instans) | Type::SizedInstans(_) => true,
            // Option payloads render as a bare value (`display_option`
            // semantics); nested options are not grouped.
            Type::Option(inner) => {
                let inner_ty = MirType::semantic(*inner);
                inner_ty != arg_ty && self.scribe_arg_renders_to_text(inner_ty, text_ty)
            }
            _ => false,
        }
    }

    /// Render one scribe argument to a textus operand. Only called for types
    /// [`Self::scribe_arg_renders_to_text`] admits, so it never falls back.
    fn render_scribe_value_text(&mut self, value: MirOperand, ty: MirType, text_ty: MirType, span: Span) -> MirOperand {
        if ty == text_ty {
            return value;
        }
        match self.normalized_type(ty.semantic_id()) {
            Type::Primitive(Primitive::Textus) => value,
            Type::Primitive(Primitive::Bivalens | Primitive::Fractus) => self.runtime_call_value(
                MirIntrinsic::FormatString { template: self.context.diagnostic_format_templates[0] },
                vec![value],
                text_ty,
                span,
            ),
            // L28 (ab91f49f): `fractus<f32>` renders through the same
            // `display_fractus` FormatString carrier (the emitter/runtime f32
            // display ABI); `instans` renders its Rust-oracle Debug shape
            // through the opaque format carrier (the runtime's arena lookup
            // renders the handle like `__faber_rt_v1_instans_display`).
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32)
            | Type::Primitive(Primitive::Instans)
            | Type::SizedInstans(_) => self.runtime_call_value(
                MirIntrinsic::FormatString { template: self.context.diagnostic_format_templates[0] },
                vec![value],
                text_ty,
                span,
            ),
            Type::Primitive(Primitive::Numerus) => self.runtime_call_value(
                MirIntrinsic::Convert(MirConversion {
                    flavor: MirConversionFlavor::Runtime,
                    source_ty: ty,
                    target_ty: text_ty,
                    params: Vec::new(),
                    recovery: None,
                    struct_defaults: Vec::new(),
                }),
                vec![value],
                text_ty,
                span,
            ),
            // Opaque collections (lista, octeti, valor, tabula, copia) format
            // through the single-opaque-arg format carrier, which every MIR
            // consumer renders in the Rust-oracle Debug shape.
            Type::Primitive(Primitive::Octeti | Primitive::Valor) | Type::Array(_) | Type::Map(_, _) | Type::Set(_) => {
                self.runtime_call_value(
                    MirIntrinsic::FormatString { template: self.context.diagnostic_format_templates[0] },
                    vec![value],
                    text_ty,
                    span,
                )
            }
            Type::Option(inner) => {
                let inner_ty = MirType::semantic(*inner);
                self.lower_option_scribe_text(value, inner_ty, text_ty, span)
            }
            _ => {
                // Unreachable for the grouped path (the pre-check admits only
                // renderable types); keep the MIR type-valid defensively.
                self.errors.push(MirError::unsupported(
                    span,
                    "scribe argument type outside the grouped display set",
                ));
                MirOperand::Constant(MirConstant::String(self.context.empty_textus))
            }
        }
    }

    /// Render an option (`T ∪ nihil`) scribe argument as textus: branch on the
    /// payload's presence, unwrap and render the payload when present, and
    /// substitute the literal `nihil` text when absent — mirroring the Rust
    /// oracle's `display_option` semantics.
    fn lower_option_scribe_text(
        &mut self,
        option: MirOperand,
        inner_ty: MirType,
        text_ty: MirType,
        span: Span,
    ) -> MirOperand {
        let bivalens = MirType::semantic(self.types.primitive(Primitive::Bivalens));
        let present = self.assign_temp(MirValueKind::Option(MirOptionOp::IsNotNil(option.clone())), bivalens, span);
        let result = self.push_temp(text_ty, span);
        let some_id = self.fresh_block(span);
        let nil_id = self.fresh_block(span);
        let join_id = self.fresh_block(span);
        self.terminate_current(
            MirTerminatorKind::Branch { condition: present, then_block: some_id, else_block: nil_id },
            span,
        );

        self.switch_to(some_id);
        {
            let payload = self.assign_temp(
                MirValueKind::Option(MirOptionOp::Unwrap { value: option, mode: MirOptionUnwrapMode::Assume }),
                inner_ty,
                span,
            );
            let rendered = self.render_scribe_value_text(payload, inner_ty, text_ty, span);
            let value = self.new_value(MirValueKind::Operand(rendered), text_ty, span);
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Assign { place: MirPlace::temp(result), value },
                span,
            });
            self.terminate_current(MirTerminatorKind::Goto(join_id), span);
        }

        self.switch_to(nil_id);
        {
            let nihil = MirOperand::Constant(MirConstant::String(self.context.diagnostic_nihil));
            let value = self.new_value(MirValueKind::Operand(nihil), text_ty, span);
            self.append_stmt(MirStatement {
                kind: MirStatementKind::Assign { place: MirPlace::temp(result), value },
                span,
            });
            self.terminate_current(MirTerminatorKind::Goto(join_id), span);
        }

        self.switch_to(join_id);
        MirOperand::Temp(result)
    }

    /// Lower `adfirma` into a target-neutral assertion runtime intrinsic.
    ///
    /// The condition has already been typechecked as `bivalens`; MIR validation
    /// rechecks that invariant so backend probes can trust the first operand.
    pub(super) fn lower_adfirma(
        &mut self,
        cond: &HirExpression,
        message: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let mut args = vec![self.lower_expr_value(cond)?];
        if let Some(message) = message {
            args.push(self.lower_expr_value(message)?);
        }
        let return_ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(MirIntrinsic::Assert, args, return_ty, expr.span))
    }

    /// Lower a string-template application to a format-string intrinsic.
    pub(super) fn lower_scriptum(
        &mut self,
        template: Symbol,
        args: &[HirExpression],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let mut lowered_args = Vec::with_capacity(args.len());
        for arg in args {
            lowered_args.push(self.lower_expr_value(arg)?);
        }
        let ty = self.expr_ty(expr)?;
        Some(self.runtime_call_value(MirIntrinsic::FormatString { template }, lowered_args, ty, expr.span))
    }

    /// Lower runtime conversions, including optional inline recovery expressions.
    ///
    /// Aggregate `verte` construction is handled in `aggregate.rs`; this path
    /// represents conversion work that remains a runtime intrinsic.
    pub(super) fn lower_conversio(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        params: &[Symbol],
        recovery: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let source_ty = self.expr_ty(source)?;
        let source_type_id = source_ty.semantic_id();
        let ty = self.expr_ty(expr)?;
        let target = self.materialized_conversio_target(target, ty);
        if recovery.is_none()
            && matches!(self.normalized_type(source_type_id), Type::Intervallum(_))
            && matches!(self.normalized_type(target), Type::Array(_) | Type::Tensor(_, _))
        {
            let source = self.lower_expr_value(source)?;
            return Some(self.runtime_call_value(
                MirIntrinsic::Collection(MirCollectionOp::IntervalMaterialize),
                vec![source],
                ty,
                expr.span,
            ));
        }
        if recovery.is_none() && matches!(self.normalized_type(target), Type::Vector(_, _)) {
            if let HirExpressionKind::Array(elements) = &source.kind {
                return self.lower_array(elements, expr, MirAggregateKind::Vector);
            }
        }
        if recovery.is_none() {
            if let Type::Matrix(_, shape) = self.normalized_type(target).clone() {
                if let Some((rows, columns)) = crate::semantic::literal_matrix_shape(shape, self.types) {
                    if let HirExpressionKind::Array(elements) = &source.kind {
                        return self.lower_matrix_from_nested_lista(elements, rows, columns, ty, expr);
                    }
                }
            }
        }
        // Route C (AEU-5): bounded numeric lista snapshots into a padded
        // tensor. The trailing `TensorFromFlat` fill operand is a real value —
        // the caller's `⇥` fill or the numeric zero — never a `MaybeUninit`
        // read; a `⇥` on the flat path below stays a failure recovery, so the
        // padded arm runs first and even with a recovery present.
        if resolve_bounded_lista_padded_shape(source_type_id, target, self.types).is_some() {
            return self.lower_bounded_lista_to_padded_tensor_conversio(
                source,
                source_type_id,
                target,
                ty,
                recovery,
                expr.span,
            );
        }
        if recovery.is_none() && validate_lista_to_tensor_source(source_type_id, target, self.types).is_ok() {
            return self.lower_lista_to_tensor_conversio(source, target, ty, expr.span);
        }
        if recovery.is_none() && analyze_rectangular_lista_literal(source, target, self.types).is_ok() {
            return self.lower_rectangular_lista_literal_to_tensor_conversio(source, target, ty, expr.span);
        }

        let conversion_source_ty = self.lowered_conversio_source_ty(source, source_type_id, target);
        let source = self.lower_conversio_source(source, target)?;
        let struct_defaults = self.valor_genus_struct_defaults(source_type_id, target, expr.span)?;
        let recovery = match recovery {
            Some(recovery) => Some(self.lower_conversio_recovery(recovery, expr.span)?),
            None => None,
        };
        if recovery.is_none() {
            if let Some(handler) = self.handlers.last().cloned() {
                return Some(self.lower_handled_conversio(HandledConversio {
                    source,
                    source_ty: conversion_source_ty,
                    target,
                    params,
                    return_ty: ty,
                    handler,
                    span: expr.span,
                }));
            }
        }
        Some(self.runtime_call_value(
            MirIntrinsic::Convert(MirConversion {
                flavor: MirConversionFlavor::Runtime,
                source_ty: conversion_source_ty,
                target_ty: MirType::semantic(target),
                params: params.to_vec(),
                recovery,
                struct_defaults,
            }),
            vec![source],
            ty,
            expr.span,
        ))
    }

    /// Finalize the conversion target when semantic hole solving only updates
    /// the HIR result type. A surface `ascii<_, Radix>` target can retain its
    /// inference index in `HirConversioTarget`, while `expr.ty` already carries
    /// the literal capacity witnessed by the source. MIR validation requires
    /// the conversion target and runtime-call result to agree, so reuse that
    /// materialized result type for the bounded-ascii pack family only.
    fn materialized_conversio_target(&self, target: TypeId, return_ty: MirType) -> TypeId {
        let Type::BoundedAscii { capacity } = self.normalized_type(target) else {
            return target;
        };
        if matches!(self.types.get_index(*capacity), IndexExpr::Literal(_)) {
            return target;
        }

        let result = return_ty.semantic_id();
        let Type::BoundedAscii { capacity } = self.normalized_type(result) else {
            return target;
        };
        if matches!(self.types.get_index(*capacity), IndexExpr::Literal(_)) {
            result
        } else {
            target
        }
    }

    fn lower_conversio_source(&mut self, source: &HirExpression, target: TypeId) -> Option<MirOperand> {
        if let HirExpressionKind::Literal(HirLiteral::JsonValor(value)) = &source.kind {
            if matches!(self.normalized_type(target), Type::Primitive(Primitive::Valor)) {
                return self.lower_json_valor_literal(value, Some(target), source.span);
            }
        }
        self.lower_expr_value(source)
    }

    /// CTO-3 — defer a SIDE-EFFECTING `⇥` recovery call. A `⇥ fb()` fallback's
    /// CALL must run ONLY when the conversion fails (fallback-not-run-on-
    /// success): the eager `%0 = call fb()` statement previously executed even
    /// when the conversion succeeded (an eval/dup-work hazard, AC4 deviation).
    /// A call/method-call recovery is lowered into an ORPHAN block (never
    /// reached by the main flow), so its side effects are not in the success
    /// path; the runner invokes it lazily at the conversion-failure recovery
    /// point. Pure recovery expressions (literals, paths, compound values) stay
    /// eagerly lowered — evaluating them is side-effect-free, so the success
    /// path stays byte-identical for them.
    fn lower_conversio_recovery(&mut self, recovery: &HirExpression, span: Span) -> Option<MirOperand> {
        if !is_side_effecting_recovery(recovery) {
            return self.lower_expr_value(recovery);
        }
        let Some(main_block) = self.current else {
            self.errors
                .push(MirError::unsupported(span, "conversio recovery after a sealed MIR block"));
            return None;
        };
        let recovery_block = self.fresh_block(span);
        self.switch_to(recovery_block);
        let operand = self.lower_expr_value(recovery)?;
        // The orphan block is never entered by the main flow; the runner
        // evaluates the recovery call lazily at the failure point. Sealing it
        // with `unreachable` keeps it dead code to every pass.
        self.terminate_current(MirTerminatorKind::Unreachable, span);
        self.switch_to(main_block);
        Some(operand)
    }

    fn lowered_conversio_source_ty(&self, source: &HirExpression, source_type_id: TypeId, target: TypeId) -> MirType {
        if matches!(
            (&source.kind, self.normalized_type(target)),
            (
                HirExpressionKind::Literal(HirLiteral::JsonValor(_)),
                Type::Primitive(Primitive::Valor)
            )
        ) {
            return MirType::semantic(target);
        }
        MirType::semantic(source_type_id)
    }

    pub(super) fn lower_ad_expr(
        &mut self,
        route: Symbol,
        opener: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let return_ty = self.expr_ty(expr)?;
        let route_operand = MirOperand::Constant(MirConstant::String(route));
        let sermo = self.runtime_call_value(MirIntrinsic::SermoOpen, vec![route_operand], return_ty, expr.span);
        let Some(opener) = opener else {
            return Some(sermo);
        };

        let opener_operand = self.lower_expr_value(opener)?;
        let opener_ty = self.expr_ty(opener)?;
        let valor_ty = self.types.primitive(Primitive::Valor);
        let opener_valor = self.runtime_call_value(
            MirIntrinsic::Convert(MirConversion {
                flavor: MirConversionFlavor::Runtime,
                source_ty: opener_ty,
                target_ty: MirType::semantic(valor_ty),
                params: Vec::new(),
                recovery: None,
                struct_defaults: Vec::new(),
            }),
            vec![opener_operand],
            MirType::semantic(valor_ty),
            opener.span,
        );
        self.runtime_call_to_place(
            None,
            MirIntrinsic::SermoSetOpener,
            vec![sermo.clone(), opener_valor],
            MirType::semantic(self.types.primitive(Primitive::Vacuum)),
            expr.span,
        );
        Some(sermo)
    }

    fn valor_genus_struct_defaults(
        &mut self,
        source: TypeId,
        target: TypeId,
        span: Span,
    ) -> Option<Vec<MirStructFieldDefaults>> {
        if !matches!(self.types.get(source), Type::Primitive(Primitive::Valor)) {
            return Some(Vec::new());
        }
        let mut seen = FxHashSet::default();
        let mut defaults = Vec::new();
        self.collect_type_struct_defaults(target, &mut seen, &mut defaults, span)?;
        Some(defaults)
    }

    fn collect_type_struct_defaults(
        &mut self,
        ty: TypeId,
        seen: &mut FxHashSet<DefId>,
        defaults: &mut Vec<MirStructFieldDefaults>,
        span: Span,
    ) -> Option<()> {
        match self.types.get(ty).clone() {
            Type::Struct(def_id) => self.collect_struct_defaults(def_id, seen, defaults, span),
            Type::Array(inner)
            | Type::Map(_, inner)
            | Type::Set(inner)
            | Type::Tensor(inner, _)
            | Type::Option(inner) => self.collect_type_struct_defaults(inner, seen, defaults, span),
            _ => Some(()),
        }
    }

    fn collect_struct_defaults(
        &mut self,
        def_id: DefId,
        seen: &mut FxHashSet<DefId>,
        defaults: &mut Vec<MirStructFieldDefaults>,
        span: Span,
    ) -> Option<()> {
        if !seen.insert(def_id) {
            return Some(());
        }
        let fields = self.materialize_struct_fields(def_id, &[], aggregate::StructFieldFill::Construction, span)?;
        if !fields.is_empty() {
            defaults.push(MirStructFieldDefaults { def_id, fields });
        }
        let field_defs = self
            .context
            .structs
            .get(&def_id)
            .cloned()
            .unwrap_or_default();
        for field in field_defs {
            self.collect_type_struct_defaults(field.ty, seen, defaults, span)?;
        }
        Some(())
    }

    fn lower_lista_to_tensor_conversio(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        return_ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let flat = self.lower_expr_value(source)?;
        self.lower_tensor_from_flat_conversio(flat, target, return_ty, span)
    }

    /// Route C padded snapshot: `lista<T, N> ↦ tensor<T, [N]>` with the
    /// caller's fill (a `⇥` value, element-typed) or the numeric zero. The
    /// runtime `TensorFromFlat` pads slots `len..N` with that real value; the
    /// bounded carrier's dead slots are never read.
    fn lower_bounded_lista_to_padded_tensor_conversio(
        &mut self,
        source: &HirExpression,
        source_ty: TypeId,
        target: TypeId,
        return_ty: MirType,
        recovery: Option<&HirExpression>,
        span: Span,
    ) -> Option<MirOperand> {
        let (elem, _) = resolve_bounded_lista_padded_shape(source_ty, target, self.types)?;
        let flat = self.lower_expr_value(source)?;
        let fill = match recovery {
            Some(fill) => self.lower_expr_value(fill)?,
            None => MirOperand::Constant(numeric_zero_constant(elem, self.types)),
        };
        let shape = self.lower_lista_to_tensor_shape(flat.clone(), target, span)?;
        let seed = self.construct_temp(
            MirAggregateKind::Tensor,
            MirAggregateFields::Ordered(Vec::new()),
            return_ty,
            span,
        );
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorFromFlat),
            vec![seed, flat, shape, fill],
            return_ty,
            span,
        ))
    }

    fn lower_rectangular_lista_literal_to_tensor_conversio(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        return_ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let rectangular = analyze_rectangular_lista_literal(source, target, self.types).ok()?;
        let flat_ty = self.types.find_array(rectangular.leaf_elem).or_else(|| {
            self.errors.push(MirError::unsupported(
                span,
                "lista element type missing before rectangular lista literal tensor lowering",
            ));
            None
        })?;
        let fields = rectangular
            .flat_leaves
            .into_iter()
            .map(|leaf| self.lower_expr_value(leaf).map(MirAggregateItem::Operand))
            .collect::<Option<Vec<_>>>()?;
        let flat = self.construct_temp(
            MirAggregateKind::Array,
            MirAggregateFields::Ordered(fields),
            MirType::semantic(flat_ty),
            span,
        );
        self.lower_tensor_from_flat_conversio(flat, target, return_ty, span)
    }

    fn lower_tensor_from_flat_conversio(
        &mut self,
        flat: MirOperand,
        target: TypeId,
        return_ty: MirType,
        span: Span,
    ) -> Option<MirOperand> {
        let shape = self.lower_lista_to_tensor_shape(flat.clone(), target, span)?;
        let seed = self.construct_temp(
            MirAggregateKind::Tensor,
            MirAggregateFields::Ordered(Vec::new()),
            return_ty,
            span,
        );
        Some(self.runtime_call_value(
            MirIntrinsic::Collection(MirCollectionOp::TensorFromFlat),
            vec![seed, flat, shape],
            return_ty,
            span,
        ))
    }

    fn lower_lista_to_tensor_shape(&mut self, flat: MirOperand, target: TypeId, span: Span) -> Option<MirOperand> {
        let shape_type = lista_numerus_type(self.types).or_else(|| {
            self.errors.push(MirError::unsupported(
                span,
                "lista<numerus> shape type missing before lista-to-tensor MIR lowering",
            ));
            None
        })?;
        let shape_ty = MirType::semantic(shape_type);

        let fields = match resolve_lista_to_tensor_shape(target, self.types)? {
            ListaToTensorShape::Literal(dims) => dims
                .into_iter()
                .map(|dim| MirAggregateItem::Operand(MirOperand::Constant(MirConstant::Int(dim))))
                .collect(),
            ListaToTensorShape::InferOneD => {
                let len = self.runtime_call_value(
                    MirIntrinsic::Collection(MirCollectionOp::Length),
                    vec![flat],
                    MirType::semantic(self.types.primitive(Primitive::Numerus)),
                    span,
                );
                vec![MirAggregateItem::Operand(len)]
            }
        };

        Some(self.construct_temp(MirAggregateKind::Array, MirAggregateFields::Ordered(fields), shape_ty, span))
    }

    fn lower_handled_conversio(&mut self, request: HandledConversio<'_>) -> MirOperand {
        let HandledConversio { source, source_ty, target, params, return_ty, handler, span } = request;
        let callee = self.push_conversio_helper(source_ty, target, params, return_ty, handler.error_ty, span);
        let ok_block = self.fresh_block(span);
        if self.is_vacuum(return_ty) {
            self.terminate_current(
                MirTerminatorKind::TryCall {
                    destination: None,
                    callee: MirCallee::Function(callee),
                    args: vec![source],
                    ok_block,
                    error_place: handler.error_place,
                    error_block: handler.error_block,
                },
                span,
            );
            self.switch_to(ok_block);
            return MirOperand::Constant(MirConstant::Unit);
        }

        let destination = self.push_temp(return_ty, span);
        self.terminate_current(
            MirTerminatorKind::TryCall {
                destination: Some(MirPlace::temp(destination)),
                callee: MirCallee::Function(callee),
                args: vec![source],
                ok_block,
                error_place: handler.error_place,
                error_block: handler.error_block,
            },
            span,
        );
        self.switch_to(ok_block);
        MirOperand::Temp(destination)
    }

    fn push_conversio_helper(
        &mut self,
        source_ty: MirType,
        target: TypeId,
        params: &[Symbol],
        return_ty: MirType,
        error_ty: MirType,
        span: Span,
    ) -> MirFunctionId {
        #[expect(
            clippy::cast_possible_truncation,
            reason = "MirFunctionId is u32 by design; synthetic function count is bounded by u32::MAX"
        )]
        let id = MirFunctionId(self.parent_function_id.0 + 1 + self.synthetic_functions.len() as u32);
        let mut nested = FunctionBuilder::for_function(
            self.types,
            Some(error_ty),
            Self::clone_context(&self.context),
            id,
            self.device_role,
            None,
        );
        let param = nested.next_local_id();
        nested
            .params
            .push(MirParam { local: param, name: None, ty: source_ty, mode: MirParamMode::Owned, span });
        nested
            .locals
            .push(MirLocalDecl { id: param, name: None, ty: source_ty, mutable: false, span });

        let entry = nested.fresh_block(span);
        nested.switch_to(entry);
        let destination = nested.push_temp(return_ty, span);
        nested.runtime_call_to_place(
            Some(MirPlace::temp(destination)),
            MirIntrinsic::Convert(MirConversion {
                flavor: MirConversionFlavor::Runtime,
                source_ty,
                target_ty: MirType::semantic(target),
                params: params.to_vec(),
                recovery: None,
                struct_defaults: Vec::new(),
            }),
            vec![MirOperand::Place(MirPlace::local(param))],
            return_ty,
            span,
        );
        nested.terminate_current(MirTerminatorKind::Return(Some(MirOperand::Temp(destination))), span);
        let blocks = nested.finish_blocks();
        self.errors.extend(nested.errors);
        self.synthetic_functions.push(MirFunction {
            id,
            source: None,
            name: None,
            params: nested.params,
            locals: nested.locals,
            temps: nested.temps,
            blocks,
            return_ty,
            error_ty: Some(error_ty),
            is_async: false,
            is_generator: false,
            shader_stage: None,
            span,
        });
        id
    }

    /// Emit a runtime call and return the operand shape promised by its return
    /// type.
    ///
    /// This keeps runtime-call lowering consistent across providers,
    /// diagnostics, formatting, and conversions: `vacuum` calls are statements,
    /// while value calls allocate one destination temp.
    pub(super) fn runtime_call_value(
        &mut self,
        intrinsic: MirIntrinsic,
        args: Vec<MirOperand>,
        return_ty: MirType,
        span: Span,
    ) -> MirOperand {
        if self.is_vacuum(return_ty) {
            self.runtime_call_to_place(None, intrinsic, args, return_ty, span);
            return MirOperand::Constant(MirConstant::Unit);
        }

        let temp = self.push_temp(return_ty, span);
        self.runtime_call_to_place(Some(MirPlace::temp(temp)), intrinsic, args, return_ty, span);
        MirOperand::Temp(temp)
    }

    pub(super) fn runtime_call_to_place(
        &mut self,
        destination: Option<MirPlace>,
        intrinsic: MirIntrinsic,
        args: Vec<MirOperand>,
        return_ty: MirType,
        span: Span,
    ) {
        self.append_stmt(MirStatement {
            kind: MirStatementKind::RuntimeCall { destination, call: MirRuntimeCall { intrinsic, args, return_ty } },
            span,
        });
    }
}

fn mir_diagnostic_kind(kind: HirScribeKind) -> MirDiagnosticKind {
    match kind {
        HirScribeKind::Nota => MirDiagnosticKind::Nota,
        HirScribeKind::Vide => MirDiagnosticKind::Vide,
        HirScribeKind::Mone => MirDiagnosticKind::Mone,
        HirScribeKind::Scribe => MirDiagnosticKind::Scribe,
    }
}

fn lista_index_operand(place: &MirPlace, index: usize) -> Option<MirOperand> {
    let index = i64::try_from(index).ok()?;
    let mut place = place.clone();
    place
        .projections
        .push(MirProjection::Index(MirOperand::Constant(MirConstant::Int(index))));
    Some(MirOperand::Place(place))
}

fn numeric_operator_binop(method_name: &str) -> Option<MirBinOp> {
    match method_name {
        "addita" => Some(MirBinOp::Add),
        "subtracta" => Some(MirBinOp::Sub),
        "multiplicata" => Some(MirBinOp::Mul),
        "divisa" => Some(MirBinOp::Div),
        "modulata" => Some(MirBinOp::Mod),
        "minor" => Some(MirBinOp::Lt),
        "maior" => Some(MirBinOp::Gt),
        "minime" => Some(MirBinOp::LtEq),
        "maxime" => Some(MirBinOp::GtEq),
        "coniuncta" => Some(MirBinOp::BitAnd),
        "disiuncta" => Some(MirBinOp::BitOr),
        "alternata" => Some(MirBinOp::BitXor),
        "sinistrata" => Some(MirBinOp::Shl),
        "dextrata" => Some(MirBinOp::Shr),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum VectorMethodKind {
    Dot,
    Cross,
    Swizzle,
    Longitudo,
    Normalizata,
    Elementwise(MirBinOp),
}

fn vector_method_kind(method_name: &str) -> Option<VectorMethodKind> {
    match method_name {
        "productum" => Some(VectorMethodKind::Dot),
        "transversum" => Some(VectorMethodKind::Cross),
        "swizzle" => Some(VectorMethodKind::Swizzle),
        "longitudo" => Some(VectorMethodKind::Longitudo),
        "normalizata" => Some(VectorMethodKind::Normalizata),
        "addita" => Some(VectorMethodKind::Elementwise(MirBinOp::Add)),
        "subtrahe" => Some(VectorMethodKind::Elementwise(MirBinOp::Sub)),
        "multiplica" => Some(VectorMethodKind::Elementwise(MirBinOp::Mul)),
        "divida" => Some(VectorMethodKind::Elementwise(MirBinOp::Div)),
        _ => None,
    }
}

fn matrix_method_kind(method_name: &str) -> Option<MirBinOp> {
    match method_name {
        "addita" => Some(MirBinOp::Add),
        "subtrahe" => Some(MirBinOp::Sub),
        "multiplica" => Some(MirBinOp::Mul),
        "divida" => Some(MirBinOp::Div),
        _ => None,
    }
}

/// Glyph product → method-name mapping by operand family. Returns `None` for
/// combinations without a supported lowering (callers fail them closed).
///
/// The names are the shipped method twins (`productum`/`transversum`/
/// `matmul`/`applica`/`multiplica`), so glyph lowering reuses the exact
/// vector/matrix/tensor machinery those methods already prove.
fn glyph_method_name(op: HirBinOp, lhs_ty: &Type, rhs_ty: &Type) -> Option<&'static str> {
    match (op, lhs_ty, rhs_ty) {
        (HirBinOp::Dot, Type::Vector(..), Type::Vector(..)) => Some("productum"),
        (HirBinOp::Dot, Type::Matrix(..), Type::Matrix(..)) => Some("matmul"),
        (HirBinOp::Dot, Type::Matrix(..), Type::Vector(..)) => Some("applica"),
        (HirBinOp::Dot, Type::Tensor(..), Type::Tensor(..)) => Some("matmul"),
        (HirBinOp::Cross, Type::Vector(..), Type::Vector(..)) => Some("transversum"),
        (HirBinOp::Hadamard, Type::Vector(..), Type::Vector(..)) => Some("multiplica"),
        (HirBinOp::Hadamard, Type::Matrix(..), Type::Matrix(..)) => Some("multiplica"),
        (HirBinOp::Hadamard, Type::Tensor(..), Type::Tensor(..)) => Some("multiplica"),
        (HirBinOp::HadamardDiv, Type::Vector(..), Type::Vector(..)) => Some("divida"),
        (HirBinOp::HadamardDiv, Type::Matrix(..), Type::Matrix(..)) => Some("divida"),
        (HirBinOp::HadamardDiv, Type::Tensor(..), Type::Tensor(..)) => Some("divisio"),
        (HirBinOp::Add, Type::Vector(..), Type::Vector(..)) => Some("addita"),
        (HirBinOp::Add, Type::Matrix(..), Type::Matrix(..)) => Some("addita"),
        (HirBinOp::Add, Type::Tensor(..), Type::Tensor(..)) => Some("addita"),
        (HirBinOp::Sub, Type::Vector(..), Type::Vector(..)) => Some("subtrahe"),
        (HirBinOp::Sub, Type::Matrix(..), Type::Matrix(..)) => Some("subtrahe"),
        (HirBinOp::Sub, Type::Tensor(..), Type::Tensor(..)) => Some("subtrahe"),
        _ => None,
    }
}

fn vector_swizzle_lane(ch: char) -> Option<u8> {
    match ch {
        'x' => Some(0),
        'y' => Some(1),
        'z' => Some(2),
        'w' => Some(3),
        _ => None,
    }
}

fn projected_operand(place: &MirPlace, projection: MirProjection) -> MirOperand {
    let mut projections = place.projections.clone();
    projections.push(projection);
    MirOperand::Place(MirPlace { base: place.base.clone(), projections })
}

fn vector_lane_operand(place: &MirPlace, lane: u8) -> MirOperand {
    projected_operand(place, MirProjection::VectorLane(lane))
}

fn matrix_cell_operand(place: &MirPlace, row: u8, column: u8) -> MirOperand {
    projected_operand(place, MirProjection::MatrixCell { row, column })
}

fn numeric_imperativus_binop(method_name: &str) -> Option<MirBinOp> {
    match method_name {
        "adde" => Some(MirBinOp::Add),
        "subtrahe" => Some(MirBinOp::Sub),
        "multiplica" => Some(MirBinOp::Mul),
        "divida" => Some(MirBinOp::Div),
        "modula" => Some(MirBinOp::Mod),
        "coniunge" => Some(MirBinOp::BitAnd),
        "disiunge" => Some(MirBinOp::BitOr),
        "alterna" => Some(MirBinOp::BitXor),
        "sinistra" => Some(MirBinOp::Shl),
        "dextra" => Some(MirBinOp::Shr),
        _ => None,
    }
}

/// Numeric zero of a Route C element type: the default dead-slot fill when the
/// caller does not name one with `⇥`.
fn numeric_zero_constant(elem: TypeId, types: &TypeTable) -> MirConstant {
    let mut ty = elem;
    loop {
        match types.get(ty) {
            Type::Alias(_, inner) => ty = *inner,
            Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
                return MirConstant::Float(0.0);
            }
            _ => return MirConstant::Int(0),
        }
    }
}
