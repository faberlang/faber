//! Expression synthesis and expected-type propagation for HIR type checking.
//!
//! This file is the central dispatcher for expression typing after name
//! resolution and HIR lowering have already chosen expression forms and
//! `DefId`s. Most expressions synthesize a type from their children, while a
//! smaller set accepts an expected type from declarations, arguments, closures,
//! or control-flow joins. That expected context is advisory until the final
//! `unify` at the bottom of `check_expr_with_expected`; helpers may use it
//! earlier for constructs such as `vacua`, array literals, blocks, branches,
//! and closures where syntax alone cannot recover the intended shape.
//!
//! ERROR STRATEGY
//! ==============
//! The checker keeps walking after local failures. Invalid lowered expressions
//! and missing contextual type information return the shared error type so later
//! phases can still attach diagnostics to neighboring code instead of losing the
//! rest of the file. `ignotum` remains an explicit escape type handled by the
//! specialized helpers; it is not used here as a silent replacement for missing
//! collection or nullability information.
//!
//! BOUNDARY
//! ========
//! This pass records resolved expression types on HIR nodes. It does not choose
//! target-specific lowering policy, library translation, or codegen fallbacks.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;

const ISSUE_IACE_REQUIRES_ALTERNATE_EXIT: &str = "iace_requires_alternate_exit";
const ISSUE_IACE_VALUE_TYPE_MISMATCH: &str = "iace_value_type_mismatch";
const ISSUE_ASSERT_CONDITION_TYPE_MISMATCH: &str = "assert_condition_type_mismatch";
const ISSUE_STREAM_AD_OPENER_TYPE_MISMATCH: &str = "stream_ad_opener_type_mismatch";
const ISSUE_EXPRESSION_TYPE_MISMATCH: &str = "expression_type_mismatch";
const ISSUE_ITERA_AB_SOURCE_REQUIRES_RANGE: &str = "itera_ab_source_requires_range";
const ISSUE_CURSOR_ITERATION_REQUIRES_EX: &str = "cursor_iteration_requires_ex";
const ISSUE_CEDE_REQUIRES_PROMISSUM: &str = "cede_requires_promissum";
const ISSUE_CEDE_REQUIRES_GENERATOR: &str = "cede_requires_generator";
const ISSUE_REDDET_REQUIRES_ASYNC_FUNCTION: &str = "reddet_requires_async_function";
const ISSUE_TACEBIT_REQUIRES_ASYNC_CONTEXT: &str = "tacebit_requires_async_context";
const ISSUE_INVALID_LOWERED_EXPRESSION: &str = "invalid_lowered_expression";
/// Phase 2A fail-closed fact: the dedicated gradient node exists in HIR, but the
/// operand-eligibility and selector rules land with tensor-glyphs 2B. No `∇`
/// expression may pass typecheck untyped or silently.
const ISSUE_GRADIENT_EXPRESSION_UNSUPPORTED: &str = "gradient_expression_unsupported";

impl TypeChecker<'_> {
    /// Synthesizes an expression type, optionally using an expected type as
    /// context for shape-sensitive expressions.
    ///
    /// The expected type is not a target annotation and does not bypass local
    /// checking. It gives ambiguous forms enough information to type themselves,
    /// then the synthesized result is unified with that expectation before the
    /// node's final `expr.ty` is recorded. Recovery paths return `error_type`
    /// after emitting diagnostics so callers can continue checking sibling
    /// expressions with a stable sentinel.
    pub fn check_expr_with_expected(
        &mut self,
        expr: &mut HirExpression,
        expected: Option<TypeId>,
    ) -> TypeId {
        let ty = match &mut expr.kind {
            HirExpressionKind::Path(def_id) => self.check_path(*def_id, expr.span),
            HirExpressionKind::Literal(lit) => self.literal_type(lit, expr.span, expected),
            HirExpressionKind::Vacua => self.check_vacua(expected, expr.span),
            HirExpressionKind::Binary(op, lhs, rhs) => self.check_binary(*op, lhs, rhs, expected),
            HirExpressionKind::Unary(op, operand) => self.check_unary(*op, operand, expected),
            HirExpressionKind::Call(callee, type_args, args) => {
                self.check_call(callee, type_args, args, expected)
            }
            HirExpressionKind::Gradient { .. } => {
                // Fail closed until tensor-glyphs 2B lands the operand
                // eligibility rule, selector resolution, and result carrier.
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_GRADIENT_EXPRESSION_UNSUPPORTED,
                    expr.span,
                );
                self.error_type
            }
            HirExpressionKind::MethodCall(receiver, name, type_args, args) => {
                self.check_method_call(receiver, name, type_args, args, expected)
            }
            HirExpressionKind::Field(object, name) => self.check_field(object, *name),
            HirExpressionKind::Index(object, index) => self.check_index(object, index),
            HirExpressionKind::OptionalChain(object, chain) => {
                self.check_optional_chain(object, chain, expr.span)
            }
            HirExpressionKind::NonNull(object, chain) => {
                self.check_non_null(object, chain, expr.span)
            }
            HirExpressionKind::Block(block) => self.check_block(block, expected),
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => self.check_if(
                cond,
                then_block,
                then_catch.as_deref_mut(),
                else_block.as_mut(),
                expected,
            ),
            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => self.check_match(scrutinees, arms, expected),
            HirExpressionKind::Loop(block) => {
                self.check_block(block, None);
                self.vacuum_type()
            }
            HirExpressionKind::Dum(cond, block) => {
                self.check_condition(cond);
                self.check_block(block, None);
                self.vacuum_type()
            }
            HirExpressionKind::Itera(mode, binding, _, iter, block) => {
                let iter_ty = if matches!(mode, radix_hir::HirIteraMode::Ab) {
                    self.check_expr_allowing_direct_intervallum_step(iter)
                } else {
                    self.check_expr(iter)
                };
                let elem_ty = if matches!(mode, radix_hir::HirIteraMode::Ab) {
                    if let Some(bound) = self.intervallum_bound_type(iter_ty) {
                        bound
                    } else {
                        self.error_issue(
                            SemanticErrorKind::TypeMismatch,
                            ISSUE_ITERA_AB_SOURCE_REQUIRES_RANGE,
                            iter.span,
                        );
                        self.numerus_type()
                    }
                } else {
                    match self.types.get(self.resolve_inferred_type(iter_ty)) {
                        Type::Array(inner) => match mode {
                            radix_hir::HirIteraMode::De => self.numerus_type(),
                            radix_hir::HirIteraMode::Ex => *inner,
                            radix_hir::HirIteraMode::Ab => {
                                self.error_issue(
                                    SemanticErrorKind::TypeMismatch,
                                    ISSUE_ITERA_AB_SOURCE_REQUIRES_RANGE,
                                    iter.span,
                                );
                                self.error_type
                            }
                        },
                        Type::Map(key, value) => match mode {
                            radix_hir::HirIteraMode::Ex => *value,
                            radix_hir::HirIteraMode::De => *key,
                            radix_hir::HirIteraMode::Ab => {
                                self.error_issue(
                                    SemanticErrorKind::TypeMismatch,
                                    ISSUE_ITERA_AB_SOURCE_REQUIRES_RANGE,
                                    iter.span,
                                );
                                self.error_type
                            }
                        },
                        Type::Cursor(inner) => match mode {
                            radix_hir::HirIteraMode::Ex => *inner,
                            radix_hir::HirIteraMode::De | radix_hir::HirIteraMode::Ab => {
                                self.error_issue(
                                    SemanticErrorKind::TypeMismatch,
                                    ISSUE_CURSOR_ITERATION_REQUIRES_EX,
                                    iter.span,
                                );
                                self.error_type
                            }
                        },
                        Type::AsyncCursor(inner, _) => match mode {
                            radix_hir::HirIteraMode::Ex => *inner,
                            radix_hir::HirIteraMode::De | radix_hir::HirIteraMode::Ab => {
                                self.error_issue(
                                    SemanticErrorKind::TypeMismatch,
                                    ISSUE_CURSOR_ITERATION_REQUIRES_EX,
                                    iter.span,
                                );
                                self.error_type
                            }
                        },
                        _ => self.numerus_type(),
                    }
                };
                self.push_scope();
                self.insert_binding(*binding, elem_ty);
                self.check_block(block, None);
                self.pop_scope();
                self.vacuum_type()
            }
            HirExpressionKind::Intervallum {
                start, end, step, ..
            } => self.check_intervallum_expr(start, end, step, expr.span),
            HirExpressionKind::Assign(target, value) => self.check_assign(target, value),
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
            } => self.check_conversio_assign(target, source, recovery.as_deref_mut(), expr.span),
            HirExpressionKind::Array(elements) => self.check_array(elements, expr.span, expected),
            HirExpressionKind::Struct(def_id, fields) => self.check_struct_literal(*def_id, fields),
            HirExpressionKind::Verte {
                source,
                target,
                entries,
            } => self.check_verte(source, *target, entries.as_mut(), expected, expr.span),
            HirExpressionKind::Conversio {
                source,
                target,
                params,
                recovery,
            } => self.check_conversio(source, target, params, recovery.as_deref_mut(), expr.span),
            HirExpressionKind::Tuple(items, type_args) => {
                self.check_tuple(items, type_args.as_ref(), expected)
            }
            HirExpressionKind::Scribe(_, items) => {
                for item in items {
                    self.check_expr(item);
                }
                self.vacuum_type()
            }
            HirExpressionKind::Scriptum(_template, args) => {
                for arg in args {
                    self.check_expr(arg);
                }
                self.textus_type()
            }
            HirExpressionKind::ReadLine(_) => {
                let textus = self.textus_type();
                self.types.option(textus)
            }
            HirExpressionKind::Adfirma(cond, message) => {
                let cond_ty = self.check_expr(cond);
                let bool_ty = self.bool_type();
                self.unify_assign_issue(
                    cond_ty,
                    bool_ty,
                    cond.span,
                    ISSUE_ASSERT_CONDITION_TYPE_MISMATCH,
                );
                if let Some(message) = message {
                    self.check_expr(message);
                }
                self.vacuum_type()
            }
            HirExpressionKind::Panic(value) => {
                self.check_expr(value);
                self.vacuum_type()
            }
            HirExpressionKind::Throw(value) => {
                let value_ty = self.check_expr(value);
                if let Some(err_ty) = self.current_error.map(|sink| match sink {
                    ErrorSink::Function(ty) | ErrorSink::Local(ty) => ty,
                }) {
                    self.unify_assign_issue(
                        value_ty,
                        err_ty,
                        value.span,
                        ISSUE_IACE_VALUE_TYPE_MISMATCH,
                    );
                } else {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_IACE_REQUIRES_ALTERNATE_EXIT,
                        expr.span,
                    );
                }
                self.vacuum_type()
            }
            HirExpressionKind::Handled { body, catch } => {
                self.check_handled_block(body, catch, expected)
            }
            HirExpressionKind::Clausura(params, ret, err, body) => {
                self.check_closure(params, ret.as_mut(), err.as_mut(), body, expected)
            }
            HirExpressionKind::Cede(inner) => {
                self.check_expr(inner);
                self.error_issue(
                    SemanticErrorKind::LoweringError,
                    ISSUE_INVALID_LOWERED_EXPRESSION,
                    expr.span,
                );
                self.error_type
            }
            HirExpressionKind::Reddet(inner) => {
                // `reddet future` — await a promissum<T> and produce T.
                if !self.current_await_return_context {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_REDDET_REQUIRES_ASYNC_FUNCTION,
                        expr.span,
                    );
                }
                let inner_ty = self.check_expr(inner);
                self.await_success_type(inner_ty, inner.span, ISSUE_CEDE_REQUIRES_PROMISSUM)
            }
            HirExpressionKind::Tacebit(inner) => {
                // `tacebit future` — await to completion, discard success T.
                if !self.current_await_context {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_TACEBIT_REQUIRES_ASYNC_CONTEXT,
                        expr.span,
                    );
                }
                let inner_ty = self.check_expr(inner);
                self.await_success_type(inner_ty, inner.span, ISSUE_CEDE_REQUIRES_PROMISSUM);
                self.vacuum_type()
            }
            HirExpressionKind::Yield(inner) => {
                // `cede value` — yield one item. It is no longer an await form.
                if !self.current_generator_function {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_CEDE_REQUIRES_GENERATOR,
                        inner.span,
                    );
                    return self.error_type;
                }
                self.check_expr(inner)
            }
            HirExpressionKind::Ref(kind, inner) => {
                let inner_ty = self.check_expr(inner);
                let mutability = match kind {
                    radix_hir::HirRefKind::Shared => crate::SemanticMutability::Immutable,
                    radix_hir::HirRefKind::Mutable => crate::SemanticMutability::Mutable,
                };
                self.types.reference(mutability, inner_ty)
            }
            HirExpressionKind::Deref(inner) => self.check_deref(inner, expr.span),
            HirExpressionKind::Ad { opener, .. } => {
                if let Some(opener) = opener {
                    let valor = self.types.primitive(Primitive::Valor);
                    let opener_ty = self.check_expr_with_expected(opener, Some(valor));
                    self.unify_assign_issue(
                        opener_ty,
                        valor,
                        opener.span,
                        ISSUE_STREAM_AD_OPENER_TYPE_MISMATCH,
                    );
                }
                if let Some(def_id) = crate::builtins::frame_types::sermo_def_id(self.resolver) {
                    crate::builtins::frame_types::sermo_type_id(self.types, def_id)
                } else {
                    self.missing_type_annotation("sermo_type_not_registered", expr.span);
                    self.types.primitive(Primitive::Ignotum)
                }
            }
            HirExpressionKind::TypeCheck {
                positive: _,
                expr,
                ty: _,
            } => {
                self.check_expr(expr);
                self.bool_type()
            }
            HirExpressionKind::Praefixum(inner) => self.check_expr(inner),
            HirExpressionKind::Error => {
                if self.errored_exprs.insert(expr.id) {
                    self.error_issue(
                        SemanticErrorKind::LoweringError,
                        ISSUE_INVALID_LOWERED_EXPRESSION,
                        expr.span,
                    );
                }
                self.error_type
            }
        };

        let ty = if let Some(expected) = expected {
            let src_marked = self
                .types
                .peel_access_marker(self.resolve_inferred_type(ty))
                .0
                .is_some();
            let dest_marked = self
                .types
                .peel_access_marker(self.resolve_inferred_type(expected))
                .0
                .is_some();
            if src_marked || dest_marked {
                // Keep the synthesized type so a `ref` Path retains its fact;
                // payload compatibility is checked separately.
                self.unify_carrier_assign(ty, expected, expr.span, ISSUE_EXPRESSION_TYPE_MISMATCH);
                ty
            } else {
                self.unify_assign_issue(ty, expected, expr.span, ISSUE_EXPRESSION_TYPE_MISMATCH)
            }
        } else {
            ty
        };
        expr.ty = Some(self.resolve_inferred_type(ty));
        ty
    }

    /// Checks the empty collection literal.
    ///
    /// `vacua` is deliberately expected-context driven: without an explicit
    /// declared collection type the checker cannot know whether it is a list,
    /// map, or set, and codegen must not guess that shape later.
    fn check_vacua(&mut self, expected: Option<TypeId>, span: radix_lexer::Span) -> TypeId {
        let Some(expected) = expected else {
            self.missing_type_annotation("vacua_requires_declared_type", span);
            return self.error_type;
        };

        let resolved = self
            .types
            .peel_access_marker(self.resolve_inferred_type(expected))
            .1;
        if self.is_infer(resolved) {
            self.missing_type_annotation("vacua_requires_declared_type", span);
            return self.error_type;
        }

        let tensor_shape = match self.types.get(resolved) {
            Type::Array(_) | Type::Map(_, _) | Type::Set(_) => {
                return resolved;
            }
            Type::Tensor(_, shape) | Type::Sparsa(_, shape) => Some(*shape),
            _ => {
                self.invalid_operand_types("vacua_requires_collection_type", span);
                return self.error_type;
            }
        };
        if let Some(shape) = tensor_shape {
            if self.is_unresolved_index_hole(shape) {
                let rank0 = self.types.rank0_shape();
                self.constrain_index_equal(shape, rank0, span);
            }
        }
        resolved
    }

    /// Synthesizes an expression type without contextual expectations.
    ///
    /// Callers should prefer `check_expr_with_expected` when a declaration,
    /// function parameter, branch join, or enclosing aggregate already supplies
    /// a meaningful target shape.
    pub fn check_expr(&mut self, expr: &mut HirExpression) -> TypeId {
        self.check_expr_with_expected(expr, None)
    }
}
