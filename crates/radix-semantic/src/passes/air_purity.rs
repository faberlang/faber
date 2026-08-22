//! AIR-lane purity and subset policy.
//!
//! This pass is the fail-closed gate before `@ radix lane "air"` functions can
//! enter AIR representation. It runs after HIR lowering and typecheck, consuming
//! semantic lane metadata keyed by `DefId`; it does not inspect source
//! annotations, re-resolve names, or rebuild type facts.

use crate::{RadixLane, RadixLaneMetadata, SemanticError, SemanticErrorKind, TypeTable};
use radix_hir::{
    DefId, HirBlock, HirCallArg, HirCape, HirExpression, HirExpressionKind, HirFunction,
    HirItemKind, HirLocal, HirModule, HirObjectField, HirObjectKey, HirStatement, HirStatementKind,
};
use radix_lexer::{Interner, Span, Symbol};

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
        if block.breakable.is_some() {
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
                if !is_control_expr_statement(expr) {
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
            HirExpressionKind::Call(callee, _, args) => self.check_call(callee, args, expr.span),
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
            HirExpressionKind::MethodCall(receiver, method, _, args) => {
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
                self.check_expr(cond);
                if let Some(catch) = then_catch {
                    self.check_cape(catch, "conditional_recovery");
                }
                self.check_block(then_block);
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
                self.reject(expr.span, "iace");
                self.check_expr(value);
            }
            HirExpressionKind::Handled { body, catch } => {
                self.reject(expr.span, "recoverable_handler");
                self.check_block(body);
                self.check_cape(catch, "recoverable_handler");
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
                self.reject(expr.span, "runtime_conversion");
                self.check_expr(source);
                if let radix_hir::HirConversioTarget::Intervallum(operand) = target {
                    self.check_expr(operand);
                }
                if let Some(recovery) = recovery {
                    self.check_expr(recovery);
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

    fn check_cape(&mut self, cape: &HirCape, reason: &'static str) {
        self.reject(cape.span, reason);
        self.check_block(&cape.body);
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

fn is_allowed_tensor_intrinsic(method: &str) -> bool {
    matches!(
        method,
        "addita"
            | // S6-C2: the bounded bias-broadcast add is an AIR-lane tensor method.
            "addita_bias"
            | "subtrahe"
            | "multiplica"
            | "divisio"
            | "nega"
            | "summa"
            | "media"
            | "matmul"
            | "transpone"
            | "forma"
            | "exponentia"
            | "logarithmus"
            | "activatio_relu"
            | "radix"
            | "gelu"
            | "activatio_tanh"
            | "activatio_softmax"
            | "laminatio"
            | "summa_axis"
            | "crux_entropia"
            // CCI-1: the three common-case intrinsic source contracts are
            // AIR-lane tensor methods (forward-only in v1; reverse AD rejects
            // them upstream via `reverse_ad_no_vjp_rule`).
            | "rms_norm"
            | "rope_norm"
            | "gather"
            // ARC-3: higher-order tensor methods admit named AIR combines;
            // closure combines remain rejected by the purity traversal.
            | "reducta"
            | "cumulata"
            | "mappata"
            | "filtrata"
    )
}

fn is_control_expr_statement(expr: &HirExpression) -> bool {
    matches!(
        expr.kind,
        HirExpressionKind::Si { .. }
            | HirExpressionKind::Discerne { .. }
            | HirExpressionKind::Block(_)
    )
}
