//! AIR-lane purity and subset policy.
//!
//! This pass is the fail-closed gate before `@ radix lane "air"` functions can
//! enter AIR representation. It runs after HIR lowering and typecheck, consuming
//! semantic lane metadata keyed by `DefId`; it does not inspect source
//! annotations, re-resolve names, or rebuild type facts.

use super::air_tensor_intrinsics::is_allowed_tensor_intrinsic;
use crate::{RadixLane, RadixLaneMetadata, SemanticError, SemanticErrorKind, TypeTable};
use radix_hir::{
    DefId, HirBlock, HirCallArg, HirCape, HirExpression, HirExpressionKind, HirFunction,
    HirItemKind, HirLocal, HirModule, HirObjectField, HirObjectKey, HirStatement, HirStatementKind,
};
use radix_lexer::{Interner, Span, Symbol};
use radix_types::TypeId;

/// Check every `@ radix lane "air"` function against the accepted v1 pure subset.
///
/// # Errors
///
/// Returns [`SemanticError`]s for `@ radix lane "air"` functions that violate
/// the accepted v1 pure subset.
pub fn check(
    hir: &mut HirModule,
    lanes: &mut RadixLaneMetadata,
    interner: &mut Interner,
    types: &TypeTable,
) -> Result<(), Vec<SemanticError>> {
    let mut errors = match super::air_closure_lift::lift(hir, lanes, interner, types) {
        Ok(()) => Vec::new(),
        Err(errors) => errors,
    };
    let mut checker = AirPurityChecker {
        lanes,
        interner,
        types,
        errors: Vec::new(),
        handler_regions: Vec::new(),
    };
    checker.check_program(hir);
    errors.extend(checker.errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

struct AirPurityChecker<'a> {
    lanes: &'a RadixLaneMetadata,
    interner: &'a Interner,
    types: &'a TypeTable,
    errors: Vec<SemanticError>,
    /// ATH-2 (AIR Tier 0.5): nearest enclosing handler scope, if any. A
    /// `Throw` whose nearest region is `Try` is caught in-region (the v1
    /// throw source); in `Catch` it is a rethrow (Tier 2, declined); with no
    /// region it keeps the unhandled `iace` decline.
    handler_regions: Vec<HandlerRegion>,
}

#[derive(Clone, Copy)]
enum HandlerRegion {
    Try,
    Catch,
}

impl AirPurityChecker<'_> {
    fn check_program(&mut self, hir: &HirModule) {
        for item in &hir.items {
            if self.lanes.lane_for(item.def_id) != Some(RadixLane::Air) {
                continue;
            }
            let HirItemKind::Function(function) = &item.kind else {
                continue;
            };
            self.check_air_function(function, item.def_id, item.span);
        }
    }

    fn check_air_function(&mut self, function: &HirFunction, def_id: DefId, span: Span) {
        if function.is_async {
            self.reject(span, "async_function");
        }
        if function.is_generator {
            self.reject(span, "generator_function");
        }
        // S3-A2 (council B3; goal.md "explicit device intent, e.g. @ nucleum"):
        // a DIFFERENTIABLE device function (`@ nucleum`/`@ nucleum fragment` +
        // `@ radix backward`) is admitted to the AIR lane — its AIR form
        // generates the backward companion while its kernel form is the device
        // forward. The purity gate still rejects plain kernels/fragments
        // without a backward annotation, and never admits placement/lifecycle
        // into generated AIR (the purity ledger is untouched).
        if (function.is_kernel || function.nucleum_fragment)
            && self.lanes.backward_for(def_id).is_none()
        {
            self.reject(span, "kernel_device_function");
        }
        if function.cli_args.is_some() {
            self.reject(span, "cli_mounted_function");
        }

        for param in &function.params {
            if param.default.is_some() {
                self.reject(param.span, "defaulted_parameter");
            }
        }

        let Some(body) = &function.body else {
            self.reject(span, "missing_body");
            return;
        };
        self.check_block(body);
    }

    fn check_block(&mut self, block: &HirBlock) {
        // ATH-2: a `fac` body carries its breakable marker unconditionally,
        // but inside a handler try-region an escape is already independently
        // declined (`loop_control` rejects every `rumpe`/`perge` statement),
        // so the inert marker must not block a locally-resolved handler.
        if block.breakable.is_some()
            && !matches!(self.handler_regions.last(), Some(HandlerRegion::Try))
        {
            self.reject(block.span, "breakable_block");
        }
        for stmt in &block.statements {
            self.check_stmt(stmt);
        }
        if let Some(expr) = &block.expr {
            self.check_expr(expr);
        }
    }

    fn check_stmt(&mut self, stmt: &HirStatement) {
        match &stmt.kind {
            HirStatementKind::Local(local) => self.check_local(local, stmt.span),
            HirStatementKind::Redde(Some(expr)) => self.check_expr(expr),
            HirStatementKind::Redde(None) => {}
            HirStatementKind::Expr(expr) => {
                // ATH-2: a `Throw` statement inside a handler scope is
                // control flow (it exits the region), not a discarded value;
                // outside any handler it keeps the ordinary statement rule.
                let is_region_throw = matches!(expr.kind, HirExpressionKind::Throw(_))
                    && !self.handler_regions.is_empty();
                if !is_control_expr_statement(expr) && !is_region_throw {
                    self.reject(stmt.span, "expression_statement");
                }
                self.check_expr(expr);
            }
            HirStatementKind::Rumpe | HirStatementKind::Perge => {
                self.reject(stmt.span, "loop_control");
            }
            HirStatementKind::Tacet => self.reject(stmt.span, "tacet"),
            HirStatementKind::IncDec(_) => {
                self.reject(stmt.span, "inc_dec_mutation");
            }
            HirStatementKind::Custodi(custodi) => {
                for clause in &custodi.clauses {
                    self.check_expr(&clause.cond);
                    self.check_block(&clause.body);
                }
            }
        }
    }

    fn check_local(&mut self, local: &HirLocal, span: Span) {
        if local.mutable {
            self.reject(span, "mutable_varia");
        }
        if local.runtime_provided {
            self.reject(span, "runtime_provided_binding");
        }
        match &local.init {
            Some(init) => self.check_expr(init),
            None => self.reject(span, "deferred_local_init"),
        }
    }

    fn check_expr(&mut self, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(_)
            | HirExpressionKind::Literal(_)
            | HirExpressionKind::Vacua => {}
            HirExpressionKind::Binary(_, lhs, rhs) => {
                self.check_expr(lhs);
                self.check_expr(rhs);
            }
            HirExpressionKind::Assign(lhs, rhs) => {
                self.reject(expr.span, "assignment_mutation");
                self.check_expr(lhs);
                self.check_expr(rhs);
            }
            // `↤` is a mutation of the target place too: AIR-lane purity
            // rejects it exactly like ordinary assignment.
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
            } => {
                self.reject(expr.span, "assignment_mutation");
                self.check_expr(target);
                self.check_expr(source);
                if let Some(recovery) = recovery {
                    self.check_expr(recovery);
                }
            }
            HirExpressionKind::Unary(_, operand) => self.check_expr(operand),
            HirExpressionKind::Call(callee, _, args, _) => self.check_call(callee, args, expr.span),
            // EOT-3M (exhaustive-match completion): a transfer invokes an
            // arbitrary sink callable once per payload. A value sink cannot be
            // proven AIR-pure, so reject with the ordinary non-AIR-call issue
            // instead of silently admitting it.
            HirExpressionKind::Transfer { sink, payloads } => {
                self.reject(expr.span, "non_air_call");
                self.check_expr(sink);
                for payload in payloads {
                    self.check_expr(payload);
                }
            }
            HirExpressionKind::Gradient {
                call,
                selected_arguments,
            } => {
                // `∇` is not part of the AIR-lane surface until tensor-glyphs
                // 2B routes it through the backward-companion seam. Fail closed
                // here so no gradient can pass admission silently.
                self.reject(expr.span, "gradient_expression_unsupported");
                self.check_expr(call);
                for selector in selected_arguments {
                    self.check_expr(selector);
                }
            }
            HirExpressionKind::MethodCall(receiver, method, _, args, _) => {
                self.check_method_call(receiver, *method, args, expr.span);
            }
            HirExpressionKind::Field(object, _) => self.check_expr(object),
            HirExpressionKind::Index(object, index) => {
                self.check_expr(object);
                self.check_expr(index);
            }
            HirExpressionKind::OptionalChain(object, _) | HirExpressionKind::NonNull(object, _) => {
                self.reject(expr.span, "optional_null_chain");
                self.check_expr(object);
            }
            HirExpressionKind::Block(block) => self.check_block(block),
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => {
                // ATH-2: a per-arm `cape` riding `Si.then_catch` admits when
                // the arm is locally resolved — the catch arm's value type
                // must match the whole `si` expression's type, throws from the
                // then arm are the in-region throw source, and a throw from
                // the catch arm itself declines as a rethrow.
                self.check_expr(cond);
                if let Some(catch) = then_catch {
                    self.handler_regions.push(HandlerRegion::Try);
                    self.check_block(then_block);
                    self.handler_regions.pop();
                    self.check_recovery(catch, self.normal_exit_type(then_block));
                } else {
                    self.check_block(then_block);
                }
                if let Some(else_block) = else_block {
                    self.check_block(else_block);
                }
            }
            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => {
                for scrutinee in scrutinees {
                    self.check_expr(scrutinee);
                }
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        self.check_expr(guard);
                    }
                    self.check_expr(&arm.body);
                }
            }
            HirExpressionKind::Loop(block) => {
                self.reject(expr.span, "loop");
                self.check_block(block);
            }
            HirExpressionKind::Dum(cond, block) => {
                self.reject(expr.span, "dum_loop");
                self.check_expr(cond);
                self.check_block(block);
            }
            HirExpressionKind::Itera(_, _, _, _, source, block) => {
                self.reject(expr.span, "itera_loop");
                self.check_expr(source);
                self.check_block(block);
            }
            HirExpressionKind::IteraLockstep { sources, body, .. } => {
                self.reject(expr.span, "itera_loop");
                for source in sources {
                    self.check_expr(source);
                }
                self.check_block(body);
            }
            HirExpressionKind::Summa { source, body, .. } => {
                self.reject(expr.span, "summa_expression_unsupported");
                self.check_expr(source);
                self.check_block(body);
            }
            HirExpressionKind::Extremum {
                source, coalesce, ..
            } => {
                self.reject(expr.span, "extremum_expression_unsupported");
                self.check_expr(source);
                if let Some(identity) = coalesce {
                    self.check_expr(identity);
                }
            }
            HirExpressionKind::FirstMatch {
                source,
                coordinates,
                body,
                ..
            } => {
                // AFM-4: bounded sources admit on the AIR lane (forward
                // execution; reverse AD is fail-closed — see the vjp.rs
                // forward-only note). The predicate is still traversed so
                // impure bodies keep their own decline causes.
                self.check_expr(source);
                self.check_block(body);
                if !coordinates.is_empty() {
                    self.reject(expr.span, "first_match_expression_unsupported");
                } else if super::air_unroll::first_match_source_route(source, self.types).is_none()
                {
                    self.reject_with_arg(
                        expr.span,
                        "first_match_unbounded_source",
                        "causa",
                        "not_bounded",
                    );
                }
            }
            HirExpressionKind::Intervallum {
                start, end, step, ..
            } => {
                self.check_expr(start);
                self.check_expr(end);
                if let Some(step) = step {
                    self.check_expr(step);
                }
            }
            HirExpressionKind::Array(elements) => {
                for element in elements {
                    match element {
                        radix_hir::HirArrayElement::Expr(expr)
                        | radix_hir::HirArrayElement::Spread(expr) => {
                            self.check_expr(expr);
                        }
                    }
                }
            }
            HirExpressionKind::Struct(_, fields) => {
                for (_, value) in fields {
                    self.check_expr(value);
                }
            }
            HirExpressionKind::Tuple(elements, _) | HirExpressionKind::Scriptum(_, elements) => {
                for element in elements {
                    self.check_expr(element);
                }
            }
            HirExpressionKind::Scribe(_, elements) => {
                self.reject(expr.span, "diagnostic_output");
                for element in elements {
                    self.check_expr(element);
                }
            }
            HirExpressionKind::ReadLine(_) => self.reject(expr.span, "input_read"),
            HirExpressionKind::Adfirma(cond, message) => {
                self.reject(expr.span, "runtime_assertion");
                self.check_expr(cond);
                if let Some(message) = message {
                    self.check_expr(message);
                }
            }
            HirExpressionKind::Panic(message) => {
                self.reject(expr.span, "mori");
                self.check_expr(message);
            }
            HirExpressionKind::Throw(value) => {
                // ATH-2: a caught `iace` inside an admitted handler region is
                // the v1 throw source; a rethrow from the catch arm declines
                // into Tier 2; anything else keeps the unhandled `iace` keep-out.
                match self.handler_regions.last() {
                    Some(HandlerRegion::Try) => {}
                    Some(HandlerRegion::Catch) => self.reject(expr.span, "rethrow_in_recovery"),
                    None => self.reject(expr.span, "iace"),
                }
                self.check_expr(value);
            }
            HirExpressionKind::Handled { body, catch } => {
                // ATH-2: a locally-resolved handler admits as union/branch
                // dataflow. The body is the try region (its `iace`s are the
                // in-region throw source), and the recovery arm must resolve
                // to the whole expression's type. Keep-out forms inside the
                // body (loops, effects) still decline under their own causes.
                self.handler_regions.push(HandlerRegion::Try);
                self.check_block(body);
                self.handler_regions.pop();
                self.check_recovery(catch, self.normal_exit_type(body));
            }
            // UT: the capture boundary locally resolves the body's alternate
            // exits into a value union — the same ATH-2 admission shape as a
            // local handler, with no recovery arm (the value is the recovery).
            HirExpressionKind::Trap(body) => {
                self.handler_regions.push(HandlerRegion::Try);
                self.check_block(body);
                self.handler_regions.pop();
            }
            HirExpressionKind::Clausura(params, _, _, body, _) => {
                // ARC-4 rewrites higher-order capture-free combines before this
                // walker. A closure that remains here is outside that callback
                // surface (or was rejected with a capture-specific diagnostic).
                self.reject(expr.span, "closure");
                for param in params {
                    if let Some(default) = &param.default {
                        self.check_expr(default);
                    }
                }
                self.check_expr(body);
            }
            HirExpressionKind::Cede(value)
            | HirExpressionKind::Reddet(value)
            | HirExpressionKind::Tacebit(value)
            | HirExpressionKind::Yield(value) => {
                self.reject(expr.span, "cede/await/yield");
                self.check_expr(value);
            }
            HirExpressionKind::Verte {
                source, entries, ..
            } => {
                self.check_expr(source);
                if let Some(entries) = entries {
                    for field in entries {
                        self.check_object_field(field);
                    }
                }
            }
            HirExpressionKind::Conversio {
                source,
                target,
                recovery,
                ..
            } => {
                // ATH-3 (AIR Tier 0.5): an inline `⇥` recovery resolves the
                // conversion failure inside its own expression — self-resolved
                // union dataflow, admitted like a locally-resolved handler. A
                // bare conversion (`recovery: None`) is a boundary throw and
                // keeps the `runtime_conversion` decline everywhere in v1 —
                // including inside admitted handler regions — with a cause
                // distinct from every handler cause.
                if recovery.is_none() {
                    self.reject(expr.span, "runtime_conversion");
                }
                self.check_expr(source);
                if let radix_hir::HirConversioTarget::Intervallum(operand) = target {
                    self.check_expr(operand);
                }
                if let Some(recovery) = recovery {
                    // The inline recovery arm is the catch side of the
                    // self-resolved region: a throw from it is a rethrow
                    // (Tier 2 keep-out).
                    self.handler_regions.push(HandlerRegion::Catch);
                    self.check_expr(recovery);
                    self.handler_regions.pop();
                }
            }
            HirExpressionKind::Ref(_, operand) | HirExpressionKind::Deref(operand) => {
                self.reject(expr.span, "reference_place_op");
                self.check_expr(operand);
            }
            HirExpressionKind::Ad { opener, .. } => {
                self.reject(expr.span, "expression_ad");
                if let Some(opener) = opener {
                    self.check_expr(opener);
                }
            }
            HirExpressionKind::TypeCheck { expr: inner, .. } => {
                self.reject(expr.span, "runtime_variant_check");
                self.check_expr(inner);
            }
            HirExpressionKind::Praefixum(inner) => self.check_expr(inner),
            HirExpressionKind::Error => {}
        }
    }

    fn check_call(&mut self, callee: &HirExpression, args: &[HirCallArg], span: Span) {
        if !matches!(&callee.kind, HirExpressionKind::Path(def_id) if self.lanes.lane_for(*def_id) == Some(RadixLane::Air))
        {
            self.reject(span, "non_air_call");
        }
        self.check_call_args(args);
    }

    fn check_method_call(
        &mut self,
        receiver: &HirExpression,
        method: Symbol,
        args: &[HirCallArg],
        span: Span,
    ) {
        let method = self.interner.resolve(method).to_owned();
        if !is_allowed_tensor_intrinsic(&method) {
            self.reject_with_arg(span, "method_call", "method", &method);
        } else if !self.is_typed_tensor_receiver(receiver) {
            self.reject_with_arg(span, "tensor_method_receiver", "method", &method);
        }
        if super::air_closure_lift::is_higher_order_method(&method) {
            self.check_higher_order_combine(args);
        }
        self.check_expr(receiver);
        self.check_call_args(args);
    }

    /// Higher-order AIR methods may receive a named AIR function as their
    /// combine/mapper. Keep the callback as a DefId path so the existing AIR
    /// call graph can discover its companion; closure literals are still
    /// rejected by `check_expr` with the stable `closure` issue.
    fn check_higher_order_combine(&mut self, args: &[HirCallArg]) {
        let Some(combine) = args.first() else {
            return;
        };
        if let HirExpressionKind::Path(def_id) = &combine.expr.kind {
            if self.lanes.lane_for(*def_id) != Some(RadixLane::Air) {
                self.reject(combine.span, "non_air_call");
            }
        }
    }

    fn check_call_args(&mut self, args: &[HirCallArg]) {
        for arg in args {
            if arg.name.is_some() {
                self.reject(arg.span, "named_call_arg");
            }
            if arg.spread {
                self.reject(arg.span, "spread_call_arg");
            }
            self.check_expr(&arg.expr);
        }
    }

    fn check_object_field(&mut self, field: &HirObjectField) {
        match &field.key {
            HirObjectKey::Ident(_) | HirObjectKey::String(_) => {}
            HirObjectKey::Computed(expr) | HirObjectKey::Spread(expr) => self.check_expr(expr),
        }
        if let Some(value) = &field.value {
            self.check_expr(value);
        }
    }

    /// Check a locally-resolved handler's recovery arm (ATH-2).
    ///
    /// The recovery arm's value type is compared against the region's
    /// normal-exit value type: on the AIR lane, `redde` is reinterpreted as
    /// the block's value (existing `If`/arm lowering), so a recovery that
    /// exits without a value of the region's type declines with a named
    /// cause. A region whose only exit is the throw constrains nothing (the
    /// recovery arm owns every exit), and a rethrowing catch declines under
    /// its own cause instead.
    fn check_recovery(&mut self, cape: &HirCape, outer_ty: Option<TypeId>) {
        if !block_has_unconditional_throw(&cape.body) {
            if let Some(outer) = outer_ty {
                if self.normal_exit_type(&cape.body) != Some(outer) {
                    self.reject(cape.span, "recovery_type_mismatch");
                }
            }
        }
        self.handler_regions.push(HandlerRegion::Catch);
        self.check_block(&cape.body);
        self.handler_regions.pop();
    }

    /// Value-level type of a block's normal exit: the last `redde` value's
    /// type (AIR-lane value reinterpretation) or the trailing expression's
    /// construct-exit type. `None` when the block never completes normally
    /// (an unconditional `iace`) or produces no value at all.
    fn normal_exit_type(&self, block: &HirBlock) -> Option<TypeId> {
        let mut ty = block
            .expr
            .as_ref()
            .and_then(|expr| self.construct_exit_type(expr));
        for stmt in &block.statements {
            match &stmt.kind {
                HirStatementKind::Expr(expr) => {
                    if matches!(expr.kind, HirExpressionKind::Throw(_)) {
                        return None;
                    }
                }
                HirStatementKind::Redde(Some(value)) => ty = value.ty,
                _ => {}
            }
        }
        ty
    }

    /// Value type a control construct produces on its non-throwing exits. A
    /// throwing arm has no normal exit and constrains nothing; a construct
    /// whose every exit throws produces no value, so the recovery arm owns
    /// every exit and no outer type applies.
    fn construct_exit_type(&self, expr: &HirExpression) -> Option<TypeId> {
        match &expr.kind {
            HirExpressionKind::Throw(_) => None,
            HirExpressionKind::Block(block) => self.normal_exit_type(block),
            HirExpressionKind::Si {
                then_block,
                else_block,
                ..
            } => {
                let arms = [Some(then_block), else_block.as_ref()];
                arms.iter()
                    .flatten()
                    .find_map(|block| self.normal_exit_type(block))
            }
            HirExpressionKind::Discerne { arms, .. } => arms
                .iter()
                .find_map(|arm| self.construct_exit_type(&arm.body)),
            _ => expr.ty,
        }
    }

    fn reject(&mut self, span: Span, issue: &'static str) {
        self.errors.push(
            SemanticError::structured(SemanticErrorKind::AirPurityViolation, span)
                .with_arg("issue", issue)
                .with_arg("lane", "air"),
        );
    }

    fn reject_with_arg(
        &mut self,
        span: Span,
        issue: &'static str,
        name: &'static str,
        value: impl Into<String>,
    ) {
        self.errors.push(
            SemanticError::structured(SemanticErrorKind::AirPurityViolation, span)
                .with_arg("issue", issue)
                .with_arg("lane", "air")
                .with_arg(name, value),
        );
    }

    fn is_typed_tensor_receiver(&self, receiver: &HirExpression) -> bool {
        receiver
            .ty
            .is_some_and(|ty| crate::tensor_type_directed::is_tensor_type(ty, self.types))
    }
}

fn block_has_unconditional_throw(block: &HirBlock) -> bool {
    block.statements.iter().any(|stmt| {
        matches!(
            &stmt.kind,
            HirStatementKind::Expr(expr) if matches!(expr.kind, HirExpressionKind::Throw(_))
        )
    })
}

fn is_control_expr_statement(expr: &HirExpression) -> bool {
    matches!(
        expr.kind,
        HirExpressionKind::Si { .. }
            | HirExpressionKind::Discerne { .. }
            | HirExpressionKind::Block(_)
            | HirExpressionKind::Handled { .. }
            | HirExpressionKind::Trap(_)
    )
}
