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
const ISSUE_ITERA_APUD_REQUIRES_EX: &str = "itera_apud_requires_ex";
pub(super) const ISSUE_ITERA_APUD_ARITY_MISMATCH: &str = "itera_apud_arity_mismatch";
pub(super) const ISSUE_ITERA_APUD_REQUIRES_SHAPED_ITERABLE: &str =
    "itera_apud_requires_shaped_iterable";
const ISSUE_CEDE_REQUIRES_PROMISSUM: &str = "cede_requires_promissum";
const ISSUE_CEDE_REQUIRES_GENERATOR: &str = "cede_requires_generator";
const ISSUE_REDDET_REQUIRES_ASYNC_FUNCTION: &str = "reddet_requires_async_function";
const ISSUE_TACEBIT_REQUIRES_ASYNC_CONTEXT: &str = "tacebit_requires_async_context";
const ISSUE_INVALID_LOWERED_EXPRESSION: &str = "invalid_lowered_expression";
/// Phase 2A fail-closed fact: the dedicated gradient node exists in HIR, but the
/// operand-eligibility and selector rules land with tensor-glyphs 2B. No `∇`
/// expression may pass typecheck untyped or silently.
const ISSUE_GRADIENT_EXPRESSION_UNSUPPORTED: &str = "gradient_expression_unsupported";

/// Rank (shape length) of a tensor shape when statically known.
///
/// `Literal` and `Param` shapes are rank-1 (`[N]` parses as a one-element
/// `Tuple`); compound shapes contribute their dimension count. Inference
/// holes and the pre-grammar unspecified bridge carry no static rank.
fn tensor_shape_rank(types: &TypeTable, shape: crate::IndexId) -> Option<u64> {
    match types.get_index(shape) {
        IndexExpr::Literal(_) => Some(1),
        IndexExpr::Tuple(dims) => Some(dims.len() as u64),
        IndexExpr::Param(_) => Some(1),
        IndexExpr::Infer(_) | IndexExpr::Unspecified => None,
    }
}

/// Shared `apud` arity law (AC-5; reused by the `summa ex` admission, SFR-3):
/// the coordinate count must equal the iterable's rank — tensor shape length
/// or matrix 2 — never a partial bind. A rank >= 2 iterable without an `apud`
/// clause is arity 0 != rank. Non-shaped iterables reject coordinates
/// fail-closed; coordinate-free iteration over a non-shaped iterable is left
/// to the owning construct's own iterable surface.
pub(super) enum ApudArity {
    /// Coordinate-free rank-1 shaped iterable, or exact coordinate/rank match.
    Admitted,
    /// Non-shaped iterable carrying `apud` coordinates (fail closed).
    RequiresShaped,
    /// Coordinate count does not equal the iterable's rank.
    Mismatch { expected: u64, actual: usize },
}

pub(super) fn apud_arity(
    types: &TypeTable,
    dispatch_ty: TypeId,
    has_coordinates: bool,
    coordinate_count: usize,
) -> ApudArity {
    let rank = match types.get(dispatch_ty) {
        Type::Tensor(_, shape) => tensor_shape_rank(types, *shape),
        Type::Matrix(_, _) => Some(2),
        _ => None,
    };
    match rank {
        Some(rank) if !has_coordinates && rank == 1 => ApudArity::Admitted,
        Some(rank) if rank == coordinate_count as u64 => ApudArity::Admitted,
        Some(rank) => ApudArity::Mismatch {
            expected: rank,
            actual: coordinate_count,
        },
        None if has_coordinates => ApudArity::RequiresShaped,
        None => ApudArity::Admitted,
    }
}

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
        let saved_pending_conversion = self.pending_conversion.take();
        let ty = match &mut expr.kind {
            HirExpressionKind::Path(def_id) => self.check_path(*def_id, expr.span),
            HirExpressionKind::Literal(lit) => self.literal_type(lit, expr.span, expected),
            HirExpressionKind::Vacua => self.check_vacua(expected, expr.span),
            HirExpressionKind::Binary(op, lhs, rhs) => self.check_binary(*op, lhs, rhs, expected),
            HirExpressionKind::Unary(op, operand) => self.check_unary(*op, operand, expected),
            HirExpressionKind::Call(callee, type_args, args, conversion) => {
                let ty = self.check_call(callee, type_args, args, expected);
                *conversion = self.pending_conversion.take();
                ty
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
            HirExpressionKind::Transfer { sink, payloads } => {
                self.check_transfer(sink, payloads, expr.span)
            }
            HirExpressionKind::MethodCall(receiver, name, type_args, args, conversion) => {
                let ty = self.check_method_call(receiver, name, type_args, args, expected);
                *conversion = self.pending_conversion.take();
                ty
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
            HirExpressionKind::Itera(mode, coordinate_bindings, binding, _, iter, block) => {
                let iter_ty = if matches!(mode, radix_hir::HirIteraMode::Ab) {
                    self.check_expr_allowing_direct_intervallum_step(iter)
                } else {
                    self.check_expr(iter)
                };
                // A collection field read through an immutable `ref` root
                // is a read-only view of the carrier collection. Use that
                // carrier for iterable-shape dispatch; field projections
                // retain their `Ref` type for access and borrow checks.
                let dispatch_ty = if matches!(mode, radix_hir::HirIteraMode::Ab) {
                    iter_ty
                } else {
                    self.types
                        .peel_access_marker(self.resolve_inferred_type(iter_ty))
                        .1
                };
                let has_coordinates = !coordinate_bindings.is_empty();

                // Mode rule: `apud` coordinates require `ex` iteration in v1.
                if has_coordinates && !matches!(mode, radix_hir::HirIteraMode::Ex) {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_ITERA_APUD_REQUIRES_EX,
                        iter.span,
                    );
                }

                // Arity rule: the coordinate count must equal the iterable's
                // rank (tensor shape length; matrix = 2) — never partial
                // binding. A rank >= 2 iterable without an `apud` clause is
                // arity 0 != rank, hence also a type error. Non-shaped
                // iterables reject `apud` fail-closed. Shared with the summa
                // admission (SFR-3): same rule, same causae.
                if matches!(mode, radix_hir::HirIteraMode::Ex) {
                    match apud_arity(
                        self.types,
                        dispatch_ty,
                        has_coordinates,
                        coordinate_bindings.len(),
                    ) {
                        ApudArity::Admitted => {}
                        ApudArity::RequiresShaped => self.error_issue(
                            SemanticErrorKind::TypeMismatch,
                            ISSUE_ITERA_APUD_REQUIRES_SHAPED_ITERABLE,
                            iter.span,
                        ),
                        ApudArity::Mismatch { expected, actual } => self.errors.push(
                            SemanticError::structured(SemanticErrorKind::TypeMismatch, iter.span)
                                .with_arg("issue", ISSUE_ITERA_APUD_ARITY_MISMATCH)
                                .with_arg("expected", expected.to_string())
                                .with_arg("actual", actual.to_string()),
                        ),
                    }
                }

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
                    match self.types.get(dispatch_ty) {
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
                        // `apud` coordinates with arity == rank bind the
                        // element type; coordinate-free rank-1 literal tensors
                        // (Route T) keep their existing element binding. Rank
                        // > 1 coordinate-free and symbolic shapes keep the
                        // numerus fallback so MIR's Unsupported diagnostic
                        // stays the decline site. `[N]` parses as a
                        // one-element Tuple.
                        Type::Tensor(elem, shape)
                            if matches!(mode, radix_hir::HirIteraMode::Ex) =>
                        {
                            let rank = tensor_shape_rank(self.types, *shape);
                            let route_t = !has_coordinates
                                && match self.types.get_index(*shape) {
                                    IndexExpr::Literal(_) => true,
                                    IndexExpr::Tuple(dims) => {
                                        dims.len() == 1
                                            && matches!(
                                                self.types.get_index(dims[0]),
                                                IndexExpr::Literal(_)
                                            )
                                    }
                                    _ => false,
                                };
                            if rank == Some(coordinate_bindings.len() as u64) || route_t {
                                *elem
                            } else {
                                self.numerus_type()
                            }
                        }
                        // Matrix is rank-2 by the width-sugar rule; the arity
                        // gate rejects coordinate counts other than 2 (and no
                        // clause on a matrix, arity 0 != 2).
                        Type::Matrix(elem, _)
                            if matches!(mode, radix_hir::HirIteraMode::Ex)
                                && coordinate_bindings.len() == 2 =>
                        {
                            *elem
                        }
                        // Route B (AEU-4): `lista<T, N>` binds the element type;
                        // mir/lower/control.rs owns the matching static-split
                        // arm and the N <= 10 / literal-capacity decline site.
                        Type::BoundedArray { element, .. }
                            if matches!(mode, radix_hir::HirIteraMode::Ex) =>
                        {
                            *element
                        }
                        // AEU-7 scalar buffer forms: the lane binding is the
                        // scalar index element row — `octeti<N>[i]` reads
                        // `numerus<u8>`, `ascii<N>[i]` / `textus<N>[i]` read
                        // `ascii<1>` (D14 / D17). Unrolls read only `0..len`,
                        // so the non-ASCII trap row can never fire from a
                        // dead slot.
                        Type::BoundedOcteti { .. }
                            if matches!(mode, radix_hir::HirIteraMode::Ex) =>
                        {
                            self.types
                                .sized_numeric(Primitive::Numerus, NumericWidth::U8)
                        }
                        Type::BoundedAscii { .. } | Type::BoundedTextus { .. }
                            if matches!(mode, radix_hir::HirIteraMode::Ex) =>
                        {
                            self.ascii_one_type()
                        }
                        _ => self.numerus_type(),
                    }
                };
                self.push_scope();
                let numerus = self.numerus_type();
                for (def_id, _name) in coordinate_bindings {
                    self.insert_binding(*def_id, numerus);
                }
                self.insert_binding(*binding, elem_ty);
                self.check_block(block, None);
                self.pop_scope();
                self.vacuum_type()
            }
            HirExpressionKind::Summa {
                source,
                coordinates,
                filum,
                binding,
                body,
            } => super::summa::check_summa_expr(
                self,
                source,
                coordinates,
                filum.as_ref(),
                binding,
                body,
                expr.span,
            ),
            HirExpressionKind::Extremum {
                op,
                source,
                coordinates,
                coalesce,
            } => super::extremum::check_extremum_expr(
                self,
                *op,
                source,
                coordinates,
                coalesce.as_mut(),
                expected,
                expr.span,
            ),
            HirExpressionKind::FirstMatch {
                source,
                coordinates,
                binding,
                body,
            } => super::first_match::check_first_match_expr(
                self,
                source,
                coordinates,
                binding,
                body,
                expr.span,
            ),
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
            HirExpressionKind::Clausura(params, ret, err, body, _) => {
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
        self.pending_conversion = saved_pending_conversion;
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
            Type::Array(_) | Type::BoundedArray { .. } | Type::Map(_, _) | Type::Set(_) => {
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
