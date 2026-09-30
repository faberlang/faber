//! Expression dispatch for HIR-to-MIR lowering.
//!
//! This module is the expression-facing boundary of the function builder. It
//! maps the HIR expression taxonomy onto narrower lowering routines that own
//! particular MIR contracts: value creation, addressable-place projection,
//! control-flow construction, aggregate construction, and runtime/intrinsic
//! calls. The dispatch is deliberately explicit so unsupported HIR shapes stay
//! visible as diagnostics instead of being silently approximated in MIR.
//!
//! ERROR STRATEGY
//! ==============
//! MIR lowering only consumes HIR that semantic analysis has already typed and
//! resolved. When a shape is not represented in MIR yet, or when required type
//! information is absent, this layer records a `MirError` and returns `None`.
//! That fail-closed behavior prevents later codegen from guessing about
//! upstream semantic facts.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for lowering
use super::*;
use crate::hir::HirCallArg;

/// Visitor contract for lowering a HIR expression when the caller needs a MIR
/// operand.
///
/// The default dispatcher encodes the supported expression set for this MIR
/// phase. Implementors may return places, temps, constants, or runtime values,
/// but they must report unsupported forms through `MirError` rather than
/// manufacturing placeholder MIR.
pub(super) trait HirExpressionLoweringVisitor {
    /// Lower a typed HIR expression into the operand form expected by MIR users.
    ///
    /// Control-producing expressions may still terminate or switch the current
    /// block as part of producing their operand. Callers must therefore treat a
    /// `None` result as both "no value" and "current CFG state may have been
    /// intentionally sealed after a diagnostic."
    fn lower_expr_value(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => self.visit_path_expr(*def_id, expr),
            HirExpressionKind::Literal(literal) => self.visit_literal_expr(literal, expr),
            HirExpressionKind::Unary(op, operand) => self.visit_unary_expr(*op, operand, expr),
            HirExpressionKind::Binary(HirBinOp::Coalesce, lhs, rhs) => self.visit_coalesce_expr(lhs, rhs, expr),
            HirExpressionKind::Binary(op, lhs, rhs) => self.visit_binary_expr(*op, lhs, rhs, expr),
            HirExpressionKind::Call(callee, type_args, args, conversion) => {
                self.visit_call_expr(callee, type_args, args, *conversion, expr)
            }
            HirExpressionKind::Transfer { sink, payloads } => self.visit_transfer_expr(sink, payloads, expr),
            HirExpressionKind::MethodCall(receiver, method, generic_args, args, conversion) => {
                self.visit_method_call_expr(receiver, *method, generic_args, args, *conversion, expr)
            }
            HirExpressionKind::Field(object, name) => self.visit_field_expr(object, *name, expr),
            HirExpressionKind::Index(object, index) => self.visit_index_expr(object, index, expr),
            HirExpressionKind::OptionalChain(object, chain) => self.visit_optional_chain_expr(object, chain, expr),
            HirExpressionKind::NonNull(object, chain) => self.visit_non_null_expr(object, chain, expr),
            HirExpressionKind::Vacua => self.visit_vacua_expr(expr),
            HirExpressionKind::Array(elements) => self.visit_array_expr(elements, expr),
            HirExpressionKind::Struct(def_id, fields) => self.visit_struct_expr(*def_id, fields, expr),
            HirExpressionKind::Tuple(items, _) => self.visit_tuple_expr(items, expr),
            HirExpressionKind::Verte { source, target, entries } => {
                self.visit_verte_expr(source, *target, entries.as_ref(), expr)
            }
            HirExpressionKind::Scribe(kind, args) => self.visit_scribe_expr(*kind, args, expr),
            HirExpressionKind::Scriptum(template, args) => self.visit_scriptum_expr(*template, args, expr),
            HirExpressionKind::Format { value, spec } => self.visit_format_expr(value, *spec, expr),
            HirExpressionKind::Adfirma(cond, message) => self.visit_adfirma_expr(cond, message.as_deref(), expr),
            HirExpressionKind::Ad { route, opener } => self.visit_ad_expr(*route, opener.as_deref(), expr),
            HirExpressionKind::Intervallum { start, end, step, kind } => {
                self.visit_interval_expr(start, end, step.as_deref(), *kind, expr)
            }
            HirExpressionKind::Conversio { source, target, params, recovery, route, implicit } => {
                if let Some(route) = route {
                    return self.visit_conversio_route_expr(source, route, recovery.as_deref(), expr);
                }
                let target_ty = match target {
                    HirConversioTarget::Type(ty) => *ty,
                    HirConversioTarget::Intervallum(target) => {
                        return self.visit_interval_clamp(source, target, recovery.as_deref(), expr);
                    }
                };
                self.visit_conversio_expr(source, target_ty, params, recovery.as_deref(), implicit.clone(), expr)
            }
            HirExpressionKind::Block(block) => self.visit_block_expr(block, expr),
            HirExpressionKind::Si { cond, then_block, then_catch, else_block } => {
                self.visit_si_expr(cond, then_block, then_catch.as_deref(), else_block.as_ref(), expr)
            }
            HirExpressionKind::Discerne { scrutinees, arms, .. } => self.visit_discerne_expr(scrutinees, arms, expr),
            HirExpressionKind::Loop(block) => self.visit_loop_expr(block, expr),
            HirExpressionKind::Dum(cond, block) => self.visit_dum_expr(cond, block, expr),
            HirExpressionKind::Itera(mode, coordinates, binding, name, iter, block) => {
                self.visit_itera_expr(*mode, coordinates, *binding, *name, iter, block, expr)
            }
            HirExpressionKind::IteraLockstep { sources, coordinates, bindings, body } => {
                self.visit_itera_lockstep_expr(sources, coordinates, bindings, body, expr)
            }
            HirExpressionKind::IteraRange { ranges, bindings, body } => {
                self.visit_itera_range_expr(ranges, bindings, body, expr)
            }
            HirExpressionKind::Summa { source, coordinates, filum, binding, body } => {
                self.visit_summa_expr(source, coordinates, filum.as_ref(), binding, body, expr)
            }
            HirExpressionKind::Extremum { op, source, coalesce, .. } => {
                self.visit_extremum_expr(*op, source, coalesce.as_deref(), expr)
            }
            HirExpressionKind::FirstMatch { source, coordinates, binding, body } => {
                self.visit_first_match_expr(source, coordinates, binding, body, expr)
            }
            HirExpressionKind::Handled { body, catch } => self.visit_handled_expr(body, catch, expr),
            HirExpressionKind::Trap(body) => self.visit_trap_expr(body, expr),
            HirExpressionKind::CallDefault { body, default, err_ty } => {
                self.visit_call_default_expr(body, default, *err_ty, expr)
            }
            HirExpressionKind::Assign(_, _) => self.visit_assignment_expr(expr),
            HirExpressionKind::ConversioAssign { .. } => self.visit_conversio_assign_expr(expr),
            HirExpressionKind::Ref(_, inner) => self.visit_ref_expr(inner, expr),
            HirExpressionKind::Deref(inner) => self.visit_deref_expr(inner, expr),
            HirExpressionKind::ReadLine(_) => self.visit_read_line_expr(expr),
            HirExpressionKind::Clausura(params, ret_ty, err_ty, body, modifier) => match modifier {
                crate::syntax::ClosureModifier::Nucleum => {
                    self.visit_nucleum_closure_expr(params, ret_ty.as_ref(), err_ty.as_ref(), body, expr)
                }
                crate::syntax::ClosureModifier::None | crate::syntax::ClosureModifier::Libera => {
                    self.visit_clausura_expr(params, ret_ty.as_ref(), err_ty.as_ref(), body, expr)
                }
            },
            HirExpressionKind::Yield(inner) => self.visit_cede_expr(inner, expr),
            HirExpressionKind::Tacebit(inner) => self.visit_tacebit_expr(inner, expr),
            HirExpressionKind::Reddet(inner) => self.visit_reddet_expr(inner, expr),
            HirExpressionKind::Throw(value) => self.visit_throw_expr(value, expr),
            HirExpressionKind::Panic(value) => self.visit_panic_expr(value, expr),
            HirExpressionKind::TypeCheck { positive, expr: inner, ty } => {
                self.visit_type_check_expr(*positive, *ty, inner, expr)
            }
            other => self.visit_unsupported_expr(other, expr),
        }
    }

    fn visit_path_expr(&mut self, def_id: DefId, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_literal_expr(&mut self, literal: &HirLiteral, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_unary_expr(&mut self, op: HirUnOp, operand: &HirExpression, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_binary_expr(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_coalesce_expr(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_call_expr(
        &mut self,
        callee: &HirExpression,
        type_args: &[TypeId],
        args: &[HirCallArg],
        conversion: Option<DefId>,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_transfer_expr(
        &mut self,
        sink: &HirExpression,
        payloads: &[HirExpression],
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_method_call_expr(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        generic_args: &[TypeId],
        args: &[HirCallArg],
        conversion: Option<DefId>,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_field_expr(&mut self, object: &HirExpression, name: Symbol, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_index_expr(
        &mut self,
        object: &HirExpression,
        index: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_optional_chain_expr(
        &mut self,
        object: &HirExpression,
        chain: &HirOptionalChainKind,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_non_null_expr(
        &mut self,
        object: &HirExpression,
        chain: &HirNonNullKind,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_vacua_expr(&mut self, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_array_expr(&mut self, elements: &[HirArrayElement], expr: &HirExpression) -> Option<MirOperand>;
    fn visit_struct_expr(
        &mut self,
        def_id: DefId,
        fields: &[(Symbol, HirExpression)],
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_tuple_expr(&mut self, items: &[HirExpression], expr: &HirExpression) -> Option<MirOperand>;
    fn visit_verte_expr(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        entries: Option<&Vec<HirObjectField>>,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_scribe_expr(
        &mut self,
        kind: HirScribeKind,
        args: &[HirExpression],
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_scriptum_expr(
        &mut self,
        template: Symbol,
        args: &[HirExpression],
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_format_expr(&mut self, value: &HirExpression, spec: Symbol, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_adfirma_expr(
        &mut self,
        cond: &HirExpression,
        message: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_ad_expr(
        &mut self,
        route: Symbol,
        opener: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_interval_expr(
        &mut self,
        start: &HirExpression,
        end: &HirExpression,
        step: Option<&HirExpression>,
        kind: HirRangeKind,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_interval_clamp(
        &mut self,
        source: &HirExpression,
        target: &HirExpression,
        recovery: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_conversio_expr(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        params: &[Symbol],
        recovery: Option<&HirExpression>,
        implicit: Option<radix_hir::HirStoreSite>,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_conversio_route_expr(
        &mut self,
        source: &HirExpression,
        route: &crate::hir::HirConversioRoute,
        recovery: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_block_expr(&mut self, block: &HirBlock, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_si_expr(
        &mut self,
        cond: &HirExpression,
        then_block: &HirBlock,
        then_catch: Option<&HirCape>,
        else_block: Option<&HirBlock>,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_discerne_expr(
        &mut self,
        scrutinees: &[HirExpression],
        arms: &[HirCasuArm],
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_loop_expr(&mut self, block: &HirBlock, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_dum_expr(&mut self, cond: &HirExpression, block: &HirBlock, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_itera_expr(
        &mut self,
        mode: HirIteraMode,
        coordinates: &[(DefId, Symbol)],
        binding: DefId,
        name: Symbol,
        iter: &HirExpression,
        block: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_itera_lockstep_expr(
        &mut self,
        sources: &[HirExpression],
        coordinates: &[(DefId, Symbol)],
        bindings: &[(DefId, Symbol)],
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_itera_range_expr(
        &mut self,
        ranges: &[HirExpression],
        bindings: &[(DefId, Symbol)],
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_summa_expr(
        &mut self,
        source: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        filum: Option<&(DefId, Symbol)>,
        binding: &(DefId, Symbol),
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_extremum_expr(
        &mut self,
        op: HirExtremumOp,
        source: &HirExpression,
        coalesce: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_first_match_expr(
        &mut self,
        source: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        binding: &(DefId, Symbol),
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_handled_expr(&mut self, body: &HirBlock, catch: &HirCape, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_trap_expr(&mut self, body: &HirBlock, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_call_default_expr(
        &mut self,
        body: &HirBlock,
        default: &HirExpression,
        err_ty: Option<crate::semantic::TypeId>,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_ref_expr(&mut self, inner: &HirExpression, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_deref_expr(&mut self, inner: &HirExpression, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_read_line_expr(&mut self, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_nucleum_closure_expr(
        &mut self,
        params: &[crate::hir::HirParam],
        ret_ty: Option<&crate::semantic::TypeId>,
        err_ty: Option<&crate::semantic::TypeId>,
        body: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_clausura_expr(
        &mut self,
        params: &[crate::hir::HirParam],
        ret_ty: Option<&crate::semantic::TypeId>,
        err_ty: Option<&crate::semantic::TypeId>,
        body: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_cede_expr(&mut self, inner: &HirExpression, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_tacebit_expr(&mut self, inner: &HirExpression, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_reddet_expr(&mut self, inner: &HirExpression, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_assignment_expr(&mut self, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_conversio_assign_expr(&mut self, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_throw_expr(&mut self, value: &HirExpression, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_panic_expr(&mut self, value: &HirExpression, expr: &HirExpression) -> Option<MirOperand>;
    fn visit_type_check_expr(
        &mut self,
        positive: bool,
        ty: TypeId,
        expr: &HirExpression,
        outer: &HirExpression,
    ) -> Option<MirOperand>;
    fn visit_unsupported_expr(&mut self, kind: &HirExpressionKind, expr: &HirExpression) -> Option<MirOperand>;
}

impl HirExpressionLoweringVisitor for FunctionBuilder<'_> {
    fn visit_path_expr(&mut self, def_id: DefId, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_path(def_id, expr)
    }

    fn visit_literal_expr(&mut self, literal: &HirLiteral, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_literal(literal, expr.ty, expr.span)
    }

    fn visit_unary_expr(&mut self, op: HirUnOp, operand: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        if self.reject_inf_unary(op, operand, expr) {
            return None;
        }
        self.lower_unary(op, operand, expr)
    }

    fn visit_binary_expr(
        &mut self,
        op: HirBinOp,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if self.reject_inf_binary(op, lhs, rhs, expr) {
            return None;
        }
        self.lower_binary(op, lhs, rhs, expr)
    }

    fn visit_coalesce_expr(
        &mut self,
        lhs: &HirExpression,
        rhs: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_coalesce(lhs, rhs, expr)
    }

    fn visit_call_expr(
        &mut self,
        callee: &HirExpression,
        type_args: &[TypeId],
        args: &[HirCallArg],
        conversion: Option<DefId>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_call(callee, type_args, args, conversion, expr)
    }

    fn visit_transfer_expr(
        &mut self,
        sink: &HirExpression,
        payloads: &[HirExpression],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        // EOT-3M single-evaluation contract: `lower_expr_value(sink)` fully
        // evaluates the sink exactly once into an operand; reusing that operand
        // as the callee of each per-payload call never re-executes the sink
        // expression. Payloads are evaluated and invoked in source order, one
        // invocation each, and the transfer itself produces no value operand.
        let sink_operand = self.lower_expr_value(sink)?;
        let vacuum = self.expr_ty(expr)?;
        let error_ty = self.callable_error_ty(sink);
        for payload in payloads {
            let arg = self.lower_expr_value(payload)?;
            if let Some(err_ty) = error_ty {
                let callee = self
                    .closure_callee_for_operand(&sink_operand)
                    .map_or_else(|| MirCallee::Value(sink_operand.clone()), MirCallee::Closure);
                self.lower_failable_call(callee, vec![arg], vacuum, err_ty, None, payload.span);
            } else {
                self.call_value_operand(sink_operand.clone(), vec![arg], vacuum, payload.span);
            }
        }
        Some(MirOperand::Constant(MirConstant::Unit))
    }

    fn visit_method_call_expr(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        generic_args: &[TypeId],
        args: &[HirCallArg],
        conversion: Option<DefId>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if self.reject_inf_method(receiver, method, expr) {
            return None;
        }
        self.lower_method_call(receiver, method, generic_args, args, conversion, expr)
    }

    fn visit_field_expr(&mut self, object: &HirExpression, name: Symbol, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_field(object, name, expr)
    }

    fn visit_index_expr(
        &mut self,
        object: &HirExpression,
        index: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_index(object, index, expr)
    }

    fn visit_optional_chain_expr(
        &mut self,
        object: &HirExpression,
        chain: &HirOptionalChainKind,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_optional_chain(object, chain, expr)
    }

    fn visit_non_null_expr(
        &mut self,
        object: &HirExpression,
        chain: &HirNonNullKind,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_non_null(object, chain, expr)
    }

    fn visit_vacua_expr(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_vacua(expr)
    }

    fn visit_array_expr(&mut self, elements: &[HirArrayElement], expr: &HirExpression) -> Option<MirOperand> {
        self.lower_array(elements, expr, MirAggregateKind::Array)
    }

    fn visit_struct_expr(
        &mut self,
        def_id: DefId,
        fields: &[(Symbol, HirExpression)],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_struct_literal(def_id, fields, expr)
    }

    fn visit_tuple_expr(&mut self, items: &[HirExpression], expr: &HirExpression) -> Option<MirOperand> {
        self.lower_tuple(items, expr)
    }

    fn visit_verte_expr(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        entries: Option<&Vec<HirObjectField>>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_verte(source, target, entries, expr)
    }

    fn visit_scribe_expr(
        &mut self,
        kind: HirScribeKind,
        args: &[HirExpression],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_scribe(kind, args, expr)
    }

    fn visit_scriptum_expr(
        &mut self,
        template: Symbol,
        args: &[HirExpression],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_scriptum(template, args, expr)
    }

    fn visit_format_expr(&mut self, value: &HirExpression, spec: Symbol, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_format(value, spec, expr)
    }

    fn visit_adfirma_expr(
        &mut self,
        cond: &HirExpression,
        message: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_adfirma(cond, message, expr)
    }

    fn visit_ad_expr(
        &mut self,
        route: Symbol,
        opener: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_ad_expr(route, opener, expr)
    }

    fn visit_interval_expr(
        &mut self,
        start: &HirExpression,
        end: &HirExpression,
        step: Option<&HirExpression>,
        kind: HirRangeKind,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_interval_expr(start, end, step, kind, expr)
    }

    fn visit_interval_clamp(
        &mut self,
        source: &HirExpression,
        target: &HirExpression,
        recovery: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_interval_clamp(source, target, recovery, expr)
    }

    fn visit_conversio_expr(
        &mut self,
        source: &HirExpression,
        target: TypeId,
        params: &[Symbol],
        recovery: Option<&HirExpression>,
        implicit: Option<radix_hir::HirStoreSite>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if self.reject_inf_conversio(source, target, expr) {
            return None;
        }
        self.lower_conversio(source, target, params, recovery, implicit, expr)
    }

    fn visit_conversio_route_expr(
        &mut self,
        source: &HirExpression,
        route: &crate::hir::HirConversioRoute,
        recovery: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_conversio_route(source, route, recovery, expr)
    }

    fn visit_block_expr(&mut self, block: &HirBlock, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_block_expr(block, expr)
    }

    fn visit_si_expr(
        &mut self,
        cond: &HirExpression,
        then_block: &HirBlock,
        then_catch: Option<&HirCape>,
        else_block: Option<&HirBlock>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_si_expr(cond, then_block, then_catch, else_block, expr)
    }

    fn visit_discerne_expr(
        &mut self,
        scrutinees: &[HirExpression],
        arms: &[HirCasuArm],
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_discerne_expr(scrutinees, arms, expr)
    }

    fn visit_loop_expr(&mut self, block: &HirBlock, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_loop_expr(block, expr)
    }

    fn visit_dum_expr(&mut self, cond: &HirExpression, block: &HirBlock, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_dum_expr(cond, block, expr)
    }

    fn visit_itera_expr(
        &mut self,
        mode: HirIteraMode,
        coordinates: &[(DefId, Symbol)],
        binding: DefId,
        name: Symbol,
        iter: &HirExpression,
        block: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_itera_expr(mode, coordinates, binding, name, iter, block, expr)
    }

    fn visit_itera_lockstep_expr(
        &mut self,
        sources: &[HirExpression],
        coordinates: &[(DefId, Symbol)],
        bindings: &[(DefId, Symbol)],
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_itera_lockstep_expr(sources, coordinates, bindings, body, expr)
    }

    fn visit_itera_range_expr(
        &mut self,
        ranges: &[HirExpression],
        bindings: &[(DefId, Symbol)],
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_itera_range_expr(ranges, bindings, body, expr)
    }

    fn visit_handled_expr(&mut self, body: &HirBlock, catch: &HirCape, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_handled_expr(body, catch, expr)
    }

    fn visit_trap_expr(&mut self, body: &HirBlock, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_trap_expr(body, expr)
    }

    fn visit_call_default_expr(
        &mut self,
        body: &HirBlock,
        default: &HirExpression,
        err_ty: Option<crate::semantic::TypeId>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_call_default_expr(body, default, err_ty, expr)
    }

    fn visit_ref_expr(&mut self, inner: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_ref(inner, expr)
    }

    fn visit_deref_expr(&mut self, inner: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_deref(inner, expr)
    }

    fn visit_read_line_expr(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_read_line(expr)
    }

    fn visit_nucleum_closure_expr(
        &mut self,
        params: &[crate::hir::HirParam],
        ret_ty: Option<&crate::semantic::TypeId>,
        err_ty: Option<&crate::semantic::TypeId>,
        body: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_nucleum_closure_expr(params, ret_ty, err_ty, body, expr)
    }

    fn visit_clausura_expr(
        &mut self,
        params: &[crate::hir::HirParam],
        ret_ty: Option<&crate::semantic::TypeId>,
        err_ty: Option<&crate::semantic::TypeId>,
        body: &HirExpression,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_clausura_expr(params, ret_ty, err_ty, body, expr)
    }

    fn visit_cede_expr(&mut self, inner: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_cede_expr(inner, expr)
    }

    fn visit_tacebit_expr(&mut self, inner: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_tacebit_expr(inner, expr)
    }

    fn visit_reddet_expr(&mut self, inner: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_reddet_expr(inner, expr)
    }

    fn visit_assignment_expr(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_assignment_expr(expr)
    }

    fn visit_conversio_assign_expr(&mut self, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_conversio_assign_expr(expr)
    }

    fn visit_throw_expr(&mut self, value: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_iace(value, expr.span)
    }

    fn visit_panic_expr(&mut self, value: &HirExpression, expr: &HirExpression) -> Option<MirOperand> {
        self.lower_mori(value, expr.span)
    }

    fn visit_type_check_expr(
        &mut self,
        positive: bool,
        ty: TypeId,
        expr: &HirExpression,
        outer: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_type_check(positive, ty, expr, outer)
    }

    fn visit_summa_expr(
        &mut self,
        source: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        filum: Option<&(DefId, Symbol)>,
        binding: &(DefId, Symbol),
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        if self.reject_inf_summa(expr) {
            return None;
        }
        self.lower_summa_expr(source, coordinates, filum, binding, body, expr)
    }

    fn visit_extremum_expr(
        &mut self,
        op: HirExtremumOp,
        source: &HirExpression,
        coalesce: Option<&HirExpression>,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        let source = self.lower_expr_value(source)?;
        let mut args = vec![source];
        if let Some(identity) = coalesce {
            args.push(self.lower_expr_value(identity)?);
        }
        let Some(type_id) = expr.ty else {
            self.errors
                .push(MirError::missing_type(expr.span, "extremum result"));
            return None;
        };
        let op = match op {
            HirExtremumOp::Max => MirCollectionOp::TensorMax,
            HirExtremumOp::Min => MirCollectionOp::TensorMin,
        };
        Some(self.runtime_call_value(MirIntrinsic::Collection(op), args, MirType::semantic(type_id), expr.span))
    }

    fn visit_first_match_expr(
        &mut self,
        source: &HirExpression,
        coordinates: &[(DefId, Symbol)],
        binding: &(DefId, Symbol),
        body: &HirBlock,
        expr: &HirExpression,
    ) -> Option<MirOperand> {
        self.lower_first_match_expr(source, coordinates, binding, body, expr)
    }

    fn visit_unsupported_expr(&mut self, kind: &HirExpressionKind, expr: &HirExpression) -> Option<MirOperand> {
        self.errors
            .push(MirError::unsupported(expr.span, unsupported_expr_kind_name(kind)));
        None
    }
}
