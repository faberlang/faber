//! AIR-lane purity and subset policy.
//!
//! This pass is the fail-closed gate before `@ radix lane "air"` functions can
//! enter AIR representation. It runs after HIR lowering and typecheck, consuming
//! semantic lane metadata keyed by `DefId`; it does not inspect source
//! annotations, re-resolve names, or rebuild type facts.

use super::air_tensor_intrinsics::is_allowed_tensor_intrinsic;
use super::typecheck::integer_range::const_int_value;
use crate::{RadixLane, RadixLaneMetadata, SemanticError, SemanticErrorKind, TypeTable};
use radix_hir::HirBinOp;
use radix_hir::{
    DefId, HirBlock, HirCallArg, HirCape, HirExpression, HirExpressionKind, HirFunction,
    HirItemKind, HirLocal, HirModule, HirObjectField, HirObjectKey, HirStatement, HirStatementKind,
};
use radix_lexer::{Interner, Span, Symbol};
use radix_types::numeric_range as range;
use radix_types::{NumericWidth, Primitive, Type, TypeId};

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
        unbounded_sites: Vec::new(),
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
    /// Spans already reported as `air_unbounded_integer` (one report per site).
    unbounded_sites: Vec<Span>,
}

/// Ruling 39(c): an `inf` type anywhere in an AIR-lane function.
const ISSUE_AIR_UNBOUNDED_INTEGER: &str = "air_unbounded_integer";
/// A list with no upper size (`list<T, size 1…∞>`): reverse AD unrolls every
/// slot up to the maximum, so the lane needs a finite constant one.
const ISSUE_AIR_UNBOUNDED_LIST_SIZE: &str = "air_unbounded_list_size";

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
            // Symbolic generic AIR templates are source/FHIR declarations. The
            // concrete clones produced before this pass own executable AIR
            // admission; checking the template would reject its intentional
            // symbolic loop bounds a second time.
            if !function.type_params.is_empty() {
                continue;
            }
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
            self.check_unbounded_type(Some(param.ty), param.span);
        }
        self.check_unbounded_type(function.ret_ty, span);

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

    /// Check a handler body as the try region (ATH-2) and return its normal
    /// exit type for the recovery-arity check.
    fn check_handler_region(&mut self, body: &HirBlock) -> Option<TypeId> {
        self.handler_regions.push(HandlerRegion::Try);
        self.check_block(body);
        self.handler_regions.pop();
        self.normal_exit_type(body)
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
            HirStatementKind::Rumpe | HirStatementKind::Perge | HirStatementKind::ReddeFilum(_) => {
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
        self.check_unbounded_type(local.ty, span);
        match &local.init {
            Some(init) => self.check_expr(init),
            None => self.reject(span, "deferred_local_init"),
        }
    }

    fn check_expr(&mut self, expr: &HirExpression) {
        self.check_unbounded_type(expr.ty, expr.span);
        match &expr.kind {
            HirExpressionKind::Path(_)
            | HirExpressionKind::Literal(_)
            | HirExpressionKind::Insere(_)
            | HirExpressionKind::Vacua
            | HirExpressionKind::Error => {}
            HirExpressionKind::Widen { operand, .. } | HirExpressionKind::Unary(_, operand) => {
                self.check_expr(operand);
            }
            HirExpressionKind::Binary(op, lhs, rhs) => {
                if self.integer_op_unproven(*op, lhs, rhs) {
                    self.reject(expr.span, "air_integer_range_unproven");
                }
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
                ..
            } => {
                self.reject(expr.span, "assignment_mutation");
                self.check_expr(target);
                self.check_expr(source);
                if let Some(recovery) = recovery {
                    self.check_expr(recovery);
                }
            }
            HirExpressionKind::Call(callee, _, args, _) => self.check_call(callee, args, expr.span),
            // `target ⇇ value` copies into storage the target already names:
            // a mutation of the target place, rejected exactly like `←`.
            HirExpressionKind::CopyInto { target, value } => {
                self.reject(expr.span, "assignment_mutation");
                self.check_expr(target);
                self.check_expr(value);
            }
            // `sink ⇜ item` writes through the sink, the same mutation of a
            // place that `←` is; reject it exactly like `⇇`.
            HirExpressionKind::SinkInto { sink, item } => {
                self.reject(expr.span, "assignment_mutation");
                self.check_expr(sink);
                self.check_expr(item);
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
            HirExpressionKind::Marked { expr: inner, .. } => self.check_expr(inner),
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
                    let outer = self.check_handler_region(then_block);
                    self.check_recovery(catch, outer);
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
            HirExpressionKind::Itera {
                iter: source,
                body: block,
                ..
            } => {
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
            HirExpressionKind::IteraRange { ranges, body, .. } => {
                // Same AIR fail-closed stance as the other itera loops: the
                // product is rejected as a loop effect while its ranges and
                // body still walk for nested purity facts.
                self.reject(expr.span, "itera_loop");
                for range in ranges {
                    self.check_expr(range);
                }
                self.check_block(body);
            }
            HirExpressionKind::Reduction {
                op,
                source,
                group,
                body,
                identity,
                ..
            } => {
                // The first reduction admits over a statically bounded source
                // (forward execution; reverse AD is fail-closed, see the vjp.rs
                // forward-only note). Every other reduction stays outside the
                // v1 subset. The body is still traversed so impure predicates
                // keep their own decline causes.
                if *op != radix_hir::HirReductionOp::First {
                    self.reject(expr.span, "reducta_expression_unsupported");
                } else if !super::air_unroll::first_source_is_bounded(source, self.types) {
                    self.reject_with_arg(
                        expr.span,
                        "reducta_first_unbounded_source",
                        "causa",
                        "not_bounded",
                    );
                }
                self.check_expr(source);
                if let Some(group) = group {
                    self.check_expr(group);
                }
                if let Some(body) = body {
                    self.check_block(body);
                }
                if let Some(identity) = identity {
                    self.check_expr(identity);
                }
            }
            HirExpressionKind::Scan {
                source,
                group,
                body,
                ..
            } => {
                self.reject(expr.span, "unapproved_effect");
                self.check_expr(source);
                if let Some(group) = group {
                    self.check_expr(group);
                }
                self.check_block(body);
            }
            HirExpressionKind::Sort { source, body, .. } => {
                self.reject(expr.span, "sort_expression_unsupported");
                self.check_expr(source);
                if let Some(body) = body {
                    self.check_block(body);
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
                        radix_hir::HirArrayElement::Infer(_) => {}
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
                let outer = self.check_handler_region(body);
                self.check_recovery(catch, outer);
            }
            // UT: the capture boundary locally resolves the body's alternate
            // exits into a value union — the same ATH-2 admission shape as a
            // local handler, with no recovery arm (the value is the recovery).
            HirExpressionKind::Trap(body) => {
                self.check_handler_region(body);
            }
            // D17.7: the call default is a trap whose recovery arm is the
            // default value, evaluated outside the handler region.
            HirExpressionKind::CallDefault { body, default, .. } => {
                self.check_handler_region(body);
                self.check_expr(default);
            }
            HirExpressionKind::Clausura(params, _, _, body) => {
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
                implicit,
                ..
            } => {
                // ATH-3 (AIR Tier 0.5): an inline `⊥` default resolves the
                // conversion failure inside its own expression — self-resolved
                // union dataflow, admitted like a locally-resolved handler. A
                // bare conversion (`recovery: None`) is a boundary throw and
                // keeps the `runtime_conversion` decline everywhere in v1 —
                // including inside admitted handler regions — with a cause
                // distinct from every handler cause.
                // Ruling 25: an implicit store conversion is a runtime check
                // AIR has no node for; the range rule failed to prove it fits.
                if recovery.is_none() {
                    let issue = if implicit.is_some() {
                        "air_store_not_proven"
                    } else {
                        "runtime_conversion"
                    };
                    self.reject(expr.span, issue);
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
            HirExpressionKind::Praefixum(inner)
            | HirExpressionKind::Format { value: inner, .. } => {
                self.check_expr(inner);
            }
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
    /// combine/mapper. Keep the callback as a `DefId` path so the existing AIR
    /// call graph can discover its companion; closure literals are still
    /// rejected by `check_expr` with the stable `closure` issue.
    fn check_higher_order_combine(&mut self, args: &[HirCallArg]) {
        let Some(combine) = args.first() else {
            return;
        };
        if let HirExpressionKind::Path(def_id) = &combine.expr.kind
            && self.lanes.lane_for(*def_id) != Some(RadixLane::Air)
        {
            self.reject(combine.span, "non_air_call");
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
        if !block_has_unconditional_throw(&cape.body)
            && let Some(outer) = outer_ty
            && self.normal_exit_type(&cape.body) != Some(outer)
        {
            self.reject(cape.span, "recovery_type_mismatch");
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

    /// Ruling 25: an integer `+ - * / % ⇐` whose exact result, computed from
    /// the operands' declared ranges (constants by value), may leave the
    /// single 64-bit range [-2^63, 2^64 - 1]. AIR has no trap for it.
    fn integer_op_unproven(&self, op: HirBinOp, lhs: &HirExpression, rhs: &HirExpression) -> bool {
        let (Some(a), Some(b)) = (self.operand_range(lhs), self.operand_range(rhs)) else {
            return false;
        };
        binary_range(op, a, b).is_some_and(|result| !range::fits_container(result))
    }

    /// The range the operand is known to lie in: a constant's exact value;
    /// for an integer `+ - * / % ⇐` the exact result range of its operands'
    /// ranges (ruling 12: intermediates are exact, never rounded up to a
    /// width), met with the declared plain or `saturating` integer type;
    /// `None` for non-integers. Every fact is sound on all paths: constants,
    /// declared types, and exact arithmetic over those.
    fn operand_range(&self, expr: &HirExpression) -> Option<range::IntRange> {
        if let Some(value) = const_int_value(expr) {
            return Some((value, value));
        }
        let declared = self.declared_range(expr);
        let HirExpressionKind::Binary(op, lhs, rhs) = &expr.kind else {
            return declared;
        };
        let derived = declared.and(
            self.operand_range(lhs)
                .zip(self.operand_range(rhs))
                .and_then(|(a, b)| binary_range(*op, a, b)),
        );
        match (declared, derived) {
            (Some(d), Some(e)) => {
                let (lo, hi) = (d.0.max(e.0), d.1.min(e.1));
                Some(if lo <= hi { (lo, hi) } else { e })
            }
            (declared, _) => declared,
        }
    }

    fn declared_range(&self, expr: &HirExpression) -> Option<range::IntRange> {
        let mut ty = self.types.peel_access_marker(expr.ty?).1;
        while let Type::Alias(_, inner) = self.types.get(ty) {
            ty = *inner;
        }
        match self.types.get(ty) {
            Type::Primitive(Primitive::Numerus) => NumericWidth::I64.integer_range(),
            // `inf` has no range; AIR rejects it by type (`air_unbounded_integer`).
            Type::SizedNumeric(Primitive::Numerus, width) | Type::SaturatingWord(width) => {
                width.integer_range()
            }
            _ => None,
        }
    }

    /// Ruling 39(c): AIR has no representation for a heap integer, so an
    /// `inf` value (or a container of one) never enters the AIR lane. One
    /// diagnostic per site, however many nested nodes share it.
    fn check_unbounded_type(&mut self, ty: Option<TypeId>, span: Span) {
        let Some(ty) = ty else { return };
        // A nested node inside an already-reported site (a local's own
        // initializer) adds no information.
        let covered = self
            .unbounded_sites
            .iter()
            .any(|site| site.start <= span.start && span.end <= site.end);
        let issue = if self.mentions_unbounded(ty) {
            ISSUE_AIR_UNBOUNDED_INTEGER
        } else if self.mentions_unbounded_list(ty) {
            ISSUE_AIR_UNBOUNDED_LIST_SIZE
        } else {
            return;
        };
        if covered {
            return;
        }
        self.unbounded_sites.push(span);
        self.reject(span, issue);
    }

    fn mentions_unbounded_list(&self, ty: TypeId) -> bool {
        let ty = self.types.peel_access_marker(ty).1;
        match self.types.get(ty) {
            Type::RangedArray { max: None, .. } => true,
            Type::Alias(_, inner) | Type::Option(inner) | Type::Array(inner) => {
                self.mentions_unbounded_list(*inner)
            }
            Type::RangedArray { element, .. } | Type::BoundedArray { element, .. } => {
                self.mentions_unbounded_list(*element)
            }
            _ => false,
        }
    }

    fn mentions_unbounded(&self, ty: TypeId) -> bool {
        let ty = self.types.peel_access_marker(ty).1;
        match self.types.get(ty) {
            Type::Alias(_, inner) | Type::Option(inner) | Type::Array(inner) => {
                self.mentions_unbounded(*inner)
            }
            Type::SizedNumeric(Primitive::Numerus, width) => width.is_unbounded(),
            _ => false,
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
            | HirExpressionKind::CallDefault { .. }
    )
}

/// Exact result range of the integer operators the range rule covers.
fn binary_range(op: HirBinOp, a: range::IntRange, b: range::IntRange) -> Option<range::IntRange> {
    Some(match op {
        HirBinOp::Add => range::add(a, b),
        HirBinOp::Sub => range::sub(a, b),
        HirBinOp::Mul => range::mul(a, b),
        HirBinOp::Div => range::floor_div(a, b),
        HirBinOp::Mod => range::floor_rem(a, b),
        HirBinOp::Shl => range::shl(a, b),
        _ => return None,
    })
}
