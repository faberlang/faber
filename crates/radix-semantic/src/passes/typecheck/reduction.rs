//! Admission and pure term laws for the closed reduction family.
//! Arithmetic retains its exact intermediate; only a consuming store narrows.
use super::expr::{ApudArity, ISSUE_ITERA_APUD_ARITY_MISMATCH, apud_arity};
#[allow(clippy::wildcard_imports)]
use super::*;
use crate::passes::fragment_definition::{
    is_allowed_device_method, is_device_value_type, struct_def_from_type,
};
use radix_hir::{HirAxisMethod, HirAxisSugar, HirCallArg, HirReductionOp};
use radix_lexer::Span;

const ISSUE_REDUCTA_REQUIRES_SHAPED_ITERABLE: &str = "reducta_requires_shaped_iterable";
const ISSUE_REDUCTA_TERM_TYPE_NON_SCALAR: &str = "reducta_term_type_non_scalar";
const ISSUE_REDUCTA_FILUM_REQUIRES_NUCLEUM_KERNEL: &str = "reducta_filum_requires_nucleum_kernel";
const ISSUE_REDUCTA_TAIL_NOT_ALIGNED: &str = "reducta_tail_not_aligned";
const ISSUE_REDUCTA_BODY_REQUIRES_SINGLE_REDDE: &str = "reducta_body_requires_single_redde";
const ISSUE_REDUCTA_BODY_FORBIDS_RUMPE: &str = "reducta_body_forbids_rumpe";
const ISSUE_REDUCTA_BODY_FORBIDS_CEDE: &str = "reducta_body_forbids_cede";
const ISSUE_REDUCTA_BODY_FORBIDS_CAPE: &str = "reducta_body_forbids_cape";
const ISSUE_REDUCTA_BODY_FORBIDS_IACE: &str = "reducta_body_forbids_iace";
const ISSUE_REDUCTA_BODY_EFFECT: &str = "reducta_body_effect";
const ISSUE_REDUCTA_BODY_CONTEXT: &str = "reducta_body_context";
const ISSUE_REDUCTA_GROUP_KEYS_NOT_LITERAL: &str = "reducta_group_keys_not_literal";
const ISSUE_REDUCTA_GROUP_RANK_MISMATCH: &str = "reducta_group_rank_mismatch";
const ISSUE_REDUCTA_FIRST_REQUIRES_BODY: &str = "reducta_first_requires_body";

/// Coordinate names the axis expansion binds; interned before typecheck. A
/// rank-N source binds the first N names, so `MAX_AXIS_REDUCTION_RANK` is the
/// length of this list.
pub(crate) const AXIS_REDUCTION_COORDINATES: [&str; MAX_AXIS_REDUCTION_RANK] = [
    "__axis0", "__axis1", "__axis2", "__axis3", "__axis4", "__axis5", "__axis6", "__axis7",
];

/// Highest source rank the axis sugar expands (need, P6).
const MAX_AXIS_REDUCTION_RANK: usize = 8;

const ISSUE_AXIS_REDUCTION_RANK_UNSUPPORTED: &str = "axis_reduction_rank_unsupported";
const ISSUE_AXIS_REDUCTION_NOT_CONSTANT: &str = "axis_reduction_not_constant";
const ISSUE_AXIS_REDUCTION_OUT_OF_RANGE: &str = "axis_reduction_out_of_range";
const ISSUE_AXIS_REDUCTION_REPEATED: &str = "axis_reduction_repeated";
const ISSUE_AXIS_REDUCTION_EXTENT_UNKNOWN: &str = "axis_reduction_extent_unknown";
const ISSUE_AXIS_REDUCTION_MEAN_REQUIRES_FLOAT: &str = "axis_reduction_mean_requires_float";
const ISSUE_SOFTMAX_AXIS_NOT_LAST: &str = "softmax_axis_not_last";

/// Outcome of the `t.sum(axis n)` / `t.mean(axis n)` expansion probe.
pub(super) enum AxisReduction {
    /// Not an axis reduction call: checked as written.
    NotApplicable,
    /// A named diagnostic was reported; the node is typed as an error.
    Refused,
    /// The call was rewritten to the grouped reduction.
    Expanded,
}

impl TypeChecker<'_> {
    /// Needs: on a rank-N tensor (2 to 8) `t.sum(axis n)`
    /// is `reduce via sum from t at [c0, .., cN-1] per [kept coordinates]`,
    /// the coordinates other than `n` in order, and `t.mean(axis n)` is that
    /// grouped sum divided by the extent of the removed axis. The axis is a
    /// compile-time constant in `0..rank`; the result is the receiver shape
    /// with that axis removed. Need item 6: several axes ride the
    /// rest form (`t.sum(axis n0, axis n1, ...)`), each numbered in the source
    /// tensor; order does not matter, a repeated axis is refused, and `mean`
    /// divides by the product of the removed extents. Need: a `mean`
    /// over an integer element sums exactly in the element family and converts
    /// the summed tensor to `fractus` before the division, matching the scalar
    /// `.mean()`; a float element keeps its own width.
    pub(super) fn desugar_axis_reduction(&mut self, expr: &mut HirExpression) -> AxisReduction {
        let HirExpressionKind::MethodCall(receiver, name, type_args, args, conversion) =
            &mut expr.kind
        else {
            return AxisReduction::NotApplicable;
        };
        if !type_args.is_empty()
            || conversion.is_some()
            || args.is_empty()
            || args.iter().any(|arg| {
                arg.marker != Some(radix_hir::HirArgMarker::Axis)
                    || arg.spread
                    || arg.name.is_some()
            })
        {
            return AxisReduction::NotApplicable;
        }
        let Some(interner) = self.interner else {
            return AxisReduction::NotApplicable;
        };
        let surface = interner.resolve(*name).to_owned();
        let canonical = self
            .locale_pack
            .and_then(|pack| {
                pack.intrinsic_canonical(radix_intrinsics::IntrinsicReceiver::Tensor, &surface)
            })
            .map_or(surface, str::to_owned);
        let mean = match canonical.as_str() {
            "summa" => false,
            "media" => true,
            _ => return AxisReduction::NotApplicable,
        };
        let Some(coordinate_names) = AXIS_REDUCTION_COORDINATES
            .iter()
            .map(|name| interner.lookup(name))
            .collect::<Option<Vec<_>>>()
        else {
            return AxisReduction::NotApplicable;
        };
        let span = expr.span;
        let receiver_ty = self.check_expr(receiver);
        // The receiver is checked exactly once, whether the probe declines or
        // the call is rewritten (see `twin_probed_receiver`).
        self.twin_probed_receiver = Some((receiver.id, receiver_ty));
        let resolved = self.resolve_inferred_type(receiver_ty);
        let dispatch = self.types.peel_access_marker(resolved).1;
        let (element, shape) = match self.types.get(dispatch) {
            Type::Tensor(element, shape) => (*element, *shape),
            _ => return AxisReduction::NotApplicable,
        };
        let element = self.resolve_inferred_type(element);
        let dims = match self.types.get_index(shape) {
            IndexExpr::Tuple(dims) if (2..=MAX_AXIS_REDUCTION_RANK).contains(&dims.len()) => {
                dims.clone()
            }
            _ => {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_AXIS_REDUCTION_RANK_UNSUPPORTED,
                    span,
                );
                return AxisReduction::Refused;
            }
        };
        let rank = dims.len();
        let Ok(rank_u32) = u32::try_from(rank) else {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_AXIS_REDUCTION_RANK_UNSUPPORTED,
                span,
            );
            return AxisReduction::Refused;
        };
        // Every axis is numbered in the source tensor. Order does not matter
        // and a repeated number is a named error; the removed set is kept in
        // source order only for the extent product.
        let mut axes: Vec<usize> = Vec::with_capacity(args.len());
        for arg in args.iter() {
            // Need: a named compile-time integer binding (an
            // immutable `const int` local or a module constant) is a constant
            // axis; a run-time variable has no recorded value and stays
            // refused.
            let Some(position) =
                super::integer_range::const_int_value_resolved(&arg.expr, &self.const_int_values)
            else {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_AXIS_REDUCTION_NOT_CONSTANT,
                    arg.span,
                );
                return AxisReduction::Refused;
            };
            if !(0..rank as i128).contains(&position) {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_AXIS_REDUCTION_OUT_OF_RANGE,
                    arg.span,
                );
                return AxisReduction::Refused;
            }
            let Ok(index) = usize::try_from(position) else {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_AXIS_REDUCTION_OUT_OF_RANGE,
                    arg.span,
                );
                return AxisReduction::Refused;
            };
            if axes.contains(&index) {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_AXIS_REDUCTION_REPEATED,
                    arg.span,
                );
                return AxisReduction::Refused;
            }
            axes.push(index);
        }
        // Removing every axis would leave a rank-zero result; the closed
        // family covers rank 2 to 8, not the scalar collapse.
        if axes.len() >= rank {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_AXIS_REDUCTION_RANK_UNSUPPORTED,
                span,
            );
            return AxisReduction::Refused;
        }
        let Some(axes_u32) = axes
            .iter()
            .copied()
            .map(u32::try_from)
            .collect::<Result<Vec<_>, _>>()
            .ok()
        else {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_AXIS_REDUCTION_RANK_UNSUPPORTED,
                span,
            );
            return AxisReduction::Refused;
        };
        let mut divisor = None;
        // An integer-element `mean` converts after the exact integer sum; a
        // float element keeps its own type.
        let mut mean_float_element = None;
        if mean {
            if self.is_integer(element) {
                mean_float_element = Some(self.types.primitive(Primitive::Fractus));
            } else if !self.is_fractus(element) {
                self.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_AXIS_REDUCTION_MEAN_REQUIRES_FLOAT,
                    span,
                );
                return AxisReduction::Refused;
            }
            let mut product: u64 = 1;
            for &axis in &axes {
                let IndexExpr::Literal(count) = self.types.get_index(dims[axis]) else {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_AXIS_REDUCTION_EXTENT_UNKNOWN,
                        span,
                    );
                    return AxisReduction::Refused;
                };
                let Some(next) = product.checked_mul(*count) else {
                    self.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_AXIS_REDUCTION_EXTENT_UNKNOWN,
                        span,
                    );
                    return AxisReduction::Refused;
                };
                product = next;
            }
            divisor = Some(product);
        }
        let kept: Vec<usize> = (0..rank).filter(|index| !axes.contains(index)).collect();
        let result_shape = if let [only] = kept[..] {
            dims[only]
        } else {
            let kept_dims = kept.iter().map(|&index| dims[index]).collect();
            self.types.intern_index(IndexExpr::Tuple(kept_dims))
        };
        let result_element = mean_float_element.unwrap_or(element);
        let result = self.types.tensor_with_shape(result_element, result_shape);
        let coordinate_ids: Vec<DefId> = (0..rank_u32)
            .map(|offset| DefId(self.next_synthetic_def + offset))
            .collect();
        self.next_synthetic_def += rank_u32;
        let key_paths: Vec<HirExpression> = kept
            .iter()
            .map(|&index| self.synthesized(HirExpressionKind::Path(coordinate_ids[index]), span))
            .collect();
        let group = match <[HirExpression; 1]>::try_from(key_paths) {
            Ok([only]) => only,
            Err(key_paths) => self.synthesized(
                HirExpressionKind::Array(
                    key_paths
                        .into_iter()
                        .map(radix_hir::HirArrayElement::Expr)
                        .collect(),
                ),
                span,
            ),
        };
        let kind = std::mem::replace(&mut expr.kind, HirExpressionKind::Vacua);
        let HirExpressionKind::MethodCall(receiver, ..) = kind else {
            return AxisReduction::NotApplicable;
        };
        let reduction = HirExpressionKind::Reduction {
            op: HirReductionOp::Sum,
            source: receiver,
            coordinates: coordinate_ids
                .iter()
                .zip(&coordinate_names)
                .map(|(id, name)| (*id, *name))
                .collect(),
            group: Some(Box::new(group)),
            filum: None,
            binding: None,
            body: None,
            identity: None,
            mutable: false,
        };
        // Need: record the authored axis and method against the
        // expanded node id (the `Reduction` for `sum`, the outer `Div` for
        // `mean`) so presentation emission prints `t.summa(axis n)` again. A
        // rest-form call records one entry per removed axis, in source order.
        for axis in axes_u32 {
            self.axis_sugar.push(HirAxisSugar {
                id: expr.id,
                axis,
                method: if mean {
                    HirAxisMethod::Mean
                } else {
                    HirAxisMethod::Sum
                },
            });
        }
        expr.kind = match divisor {
            None => {
                self.axis_reduction_results.insert(expr.id, result);
                reduction
            }
            Some(count) => {
                let sum = self.synthesized(reduction, span);
                self.axis_reduction_results.insert(sum.id, result);
                // The summed integer tensor becomes `fractus` through the
                // ordinary tensor element `↦` row (the same HIR a written
                // `grid.sum(axis 1) ↦ tf64[2]` yields) before the division.
                let numerator = match mean_float_element {
                    None => sum,
                    Some(float_element) => {
                        let target = self.types.tensor_with_shape(float_element, result_shape);
                        self.synthesized(
                            HirExpressionKind::Conversio {
                                source: Box::new(sum),
                                target: radix_hir::HirConversioTarget::Type(target),
                                params: Vec::new(),
                                recovery: None,
                                route: None,
                                implicit: None,
                            },
                            span,
                        )
                    }
                };
                let count = self.synthesized(
                    HirExpressionKind::Literal(radix_hir::HirLiteral::Float(
                        radix_types::Magnus::from_u64(count).to_f64(),
                    )),
                    span,
                );
                HirExpressionKind::Binary(
                    radix_hir::HirBinOp::Div,
                    Box::new(numerator),
                    Box::new(count),
                )
            }
        };
        AxisReduction::Expanded
    }
}

/// Outcome of the `t.softmax(axis n)` marker probe (SOFTAXIS).
pub(super) enum SoftmaxAxis {
    /// Not an explicit-axis softmax call: checked as written.
    NotApplicable,
    /// A named diagnostic was reported; the node is typed as an error.
    Refused,
    /// The marker was consumed; the call continues as the bare form.
    Cleared,
}

impl TypeChecker<'_> {
    /// SOFTAXIS: `t.softmax(axis n)` may name the axis
    /// explicitly, as `laminatio` / `rms_norm` do, but softmax normalizes over
    /// the last axis only. The axis is a compile-time constant in `0..rank`
    /// and must equal `rank - 1`; the marker is then cleared so every later
    /// pass sees the bare `t.softmax()` and the pipeline is unchanged.
    pub(super) fn desugar_softmax_axis(&mut self, expr: &mut HirExpression) -> SoftmaxAxis {
        let HirExpressionKind::MethodCall(receiver, name, type_args, args, conversion) =
            &mut expr.kind
        else {
            return SoftmaxAxis::NotApplicable;
        };
        if !type_args.is_empty()
            || conversion.is_some()
            || args.len() != 1
            || args[0].marker != Some(radix_hir::HirArgMarker::Axis)
            || args[0].spread
            || args[0].name.is_some()
        {
            return SoftmaxAxis::NotApplicable;
        }
        let Some(interner) = self.interner else {
            return SoftmaxAxis::NotApplicable;
        };
        let surface = interner.resolve(*name).to_owned();
        let canonical = self
            .locale_pack
            .and_then(|pack| {
                pack.intrinsic_canonical(radix_intrinsics::IntrinsicReceiver::Tensor, &surface)
            })
            .map_or(surface, str::to_owned);
        if canonical != "activatio_softmax" {
            return SoftmaxAxis::NotApplicable;
        }
        let receiver_ty = self.check_expr(receiver);
        // The receiver is checked exactly once, whether the probe declines or
        // the marker is consumed (see `twin_probed_receiver`).
        self.twin_probed_receiver = Some((receiver.id, receiver_ty));
        let resolved = self.resolve_inferred_type(receiver_ty);
        let dispatch = self.types.peel_access_marker(resolved).1;
        let shape = match self.types.get(dispatch) {
            Type::Tensor(_, shape) => *shape,
            _ => return SoftmaxAxis::NotApplicable,
        };
        let dims = match self.types.get_index(shape) {
            IndexExpr::Tuple(dims) if (1..=MAX_AXIS_REDUCTION_RANK).contains(&dims.len()) => {
                dims.clone()
            }
            _ => return SoftmaxAxis::NotApplicable,
        };
        let rank = dims.len();
        let Some(axis) =
            super::integer_range::const_int_value_resolved(&args[0].expr, &self.const_int_values)
        else {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_AXIS_REDUCTION_NOT_CONSTANT,
                args[0].span,
            );
            return SoftmaxAxis::Refused;
        };
        if !(0..rank as i128).contains(&axis) {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_AXIS_REDUCTION_OUT_OF_RANGE,
                args[0].span,
            );
            return SoftmaxAxis::Refused;
        }
        if axis != (rank - 1) as i128 {
            self.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_SOFTMAX_AXIS_NOT_LAST,
                args[0].span,
            );
            return SoftmaxAxis::Refused;
        }
        args.clear();
        SoftmaxAxis::Cleared
    }
}

/// Type one reduction from its source, body term, and expected bucket extent.
pub(super) fn check_reduction_expr(
    checker: &mut TypeChecker<'_>,
    op: HirReductionOp,
    source: &mut HirExpression,
    coordinates: &[(DefId, Symbol)],
    group: Option<&mut Box<HirExpression>>,
    filum: Option<(DefId, Symbol)>,
    binding: Option<(DefId, Symbol)>,
    body: Option<&mut HirBlock>,
    identity: Option<&mut Box<HirExpression>>,
    expected: Option<TypeId>,
    span: Span,
) -> TypeId {
    let source_ty = checker.check_expr(source);
    let dispatch = checker
        .types
        .peel_access_marker(checker.resolve_inferred_type(source_ty))
        .1;
    let (element, shape) = match checker.types.get(dispatch) {
        Type::Tensor(element, shape) | Type::Matrix(element, shape) => (*element, *shape),
        _ => {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_REDUCTA_REQUIRES_SHAPED_ITERABLE,
                span,
            );
            return checker.error_type;
        }
    };
    let element = checker.resolve_inferred_type(element);
    let dimensions = crate::i64s_from_literal_index(shape, checker.types);
    let rank = match super::expr::tensor_shape_rank(checker.types, shape) {
        Some(n) => match usize::try_from(n) {
            Ok(rank) => rank,
            Err(_) => {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_REDUCTA_REQUIRES_SHAPED_ITERABLE,
                    span,
                );
                return checker.error_type;
            }
        },
        None => coordinates.len().max(1),
    };
    match apud_arity(
        checker.types,
        dispatch,
        !coordinates.is_empty(),
        coordinates.len(),
    ) {
        ApudArity::Admitted => {}
        ApudArity::RequiresShaped => checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            super::expr::ISSUE_ITERA_APUD_REQUIRES_SHAPED_ITERABLE,
            span,
        ),
        ApudArity::Mismatch { expected, actual } => checker.errors.push(
            SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
                .with_arg("issue", ISSUE_ITERA_APUD_ARITY_MISMATCH)
                .with_arg("expected", expected.to_string())
                .with_arg("actual", actual.to_string()),
        ),
    }
    if filum.is_some() {
        if !checker.current_nucleum_entry {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_REDUCTA_FILUM_REQUIRES_NUCLEUM_KERNEL,
                span,
            );
        }
        if body.is_none() {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                "reducta_filum_requires_body",
                span,
            );
        }
        if let Some(dimensions) = &dimensions {
            let extent = dimensions
                .iter()
                .try_fold(1u64, |n, d| n.checked_mul(u64::try_from(*d).ok()?));
            if extent.is_some_and(|k| !k.is_multiple_of(32)) {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_REDUCTA_TAIL_NOT_ALIGNED,
                    span,
                );
            }
        }
    }
    checker.push_scope();
    let integer = if checker.current_nucleum_entry {
        checker.width_type(radix_types::NumericWidth::I32)
    } else {
        checker.numerus_type()
    };
    checker.types.array(integer);
    for (id, _) in coordinates {
        checker.insert_binding(*id, integer);
    }
    checker.seed_coordinates(dispatch, None, coordinates);
    if let Some((id, _)) = filum {
        checker.insert_binding(id, integer);
    }
    let expected_group = expected.and_then(|ty| {
        let ty = checker.resolve_inferred_type(ty);
        match checker.types.get(ty) {
            Type::Tensor(element, shape) => Some((*element, *shape)),
            _ => None,
        }
    });
    let grouped = group.is_some();
    if let Some(group) = group {
        if coordinates.is_empty() {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                "reducta_group_requires_apud",
                span,
            );
        }
        let group_ty = checker.check_expr(group);
        let group_ty = checker.resolve_inferred_type(group_ty);
        let key_count = check_group_keys(checker, group, group_ty);
        let rank_known = expected_group.is_some_and(|(_, shape)| {
            crate::i64s_from_literal_index(shape, checker.types)
                .is_some_and(|d| !d.is_empty() && d.iter().all(|extent| *extent >= 0))
        });
        let expected_rank = expected_group.and_then(|(_, shape)| {
            crate::i64s_from_literal_index(shape, checker.types).map(|d| d.len())
        });
        if !rank_known {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                "reducta_group_extent_unknown",
                span,
            );
        } else if key_count.is_some_and(|keys| Some(keys) != expected_rank) {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_REDUCTA_GROUP_RANK_MISMATCH,
                span,
            );
        }
        if matches!(
            op,
            HirReductionOp::ArgMax | HirReductionOp::ArgMin | HirReductionOp::First
        ) || (matches!(op, HirReductionOp::Max | HirReductionOp::Min) && identity.is_none())
        {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                "reducta_group_op_unsupported",
                span,
            );
        }
    }
    let body_missing = body.is_none();
    if op == HirReductionOp::First && body_missing {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_REDUCTA_FIRST_REQUIRES_BODY,
            span,
        );
    }
    let term_ty = if let (Some(binding), Some(body)) = (binding, body) {
        checker.insert_binding(binding.0, element);
        let prev_return = checker.current_return.take();
        let prev_inferred = checker.inferred_return.take();
        let prev_allow = checker.allow_inferred_return;
        checker.allow_inferred_return = true;
        checker.check_block(body, None);
        let term = checker.inferred_return.unwrap_or(checker.error_type);
        checker.current_return = prev_return;
        checker.inferred_return = prev_inferred;
        checker.allow_inferred_return = prev_allow;
        check_reducta_body_laws(checker, coordinates, filum, binding, body);
        checker.resolve_inferred_type(term)
    } else {
        element
    };
    checker.release_range_binders(coordinates.iter().map(|(id, _)| *id));
    checker.pop_scope();
    let boolean = checker.types.primitive(Primitive::Bivalens);
    let bool_op = matches!(
        op,
        HirReductionOp::All | HirReductionOp::Any | HirReductionOp::Count | HirReductionOp::First
    );
    // A missing `first` body is already refused by name; its element is not a
    // predicate, so the term law below would only repeat that error.
    if bool_op {
        if term_ty != boolean && !(op == HirReductionOp::First && body_missing) {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                "reducta_term_type_not_bool",
                span,
            );
        }
    } else if !is_numeric_scalar(checker.types, term_ty) {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_REDUCTA_TERM_TYPE_NON_SCALAR,
            span,
        );
    }
    let result = match op {
        HirReductionOp::All | HirReductionOp::Any => boolean,
        HirReductionOp::ArgMax | HirReductionOp::ArgMin if rank > 1 => {
            checker.types.tuple(vec![integer; rank])
        }
        HirReductionOp::Count | HirReductionOp::ArgMax | HirReductionOp::ArgMin => integer,
        HirReductionOp::Sum | HirReductionOp::Product => checker.summa_result_type(term_ty),
        // The selected element, not the predicate term; always optional.
        HirReductionOp::First => element,
        _ => term_ty,
    };
    let extremum = matches!(
        op,
        HirReductionOp::Max | HirReductionOp::Min | HirReductionOp::ArgMax | HirReductionOp::ArgMin
    );
    if let Some(identity) = identity {
        if !extremum {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                "reducta_identity_not_applicable",
                identity.span,
            );
        }
        let identity_ty = checker.check_expr_with_expected(identity, Some(result));
        checker.unify_assign_issue(
            identity_ty,
            result,
            identity.span,
            "reducta_identity_type_mismatch",
        );
    } else if (op == HirReductionOp::First && !grouped)
        || (extremum && !grouped && dimensions.is_none_or(|d| d.is_empty() || d.contains(&0)))
    {
        let nullable = checker.types.option(result);
        if let Some(expected) = expected {
            let expected = checker.resolve_inferred_type(expected);
            if !matches!(
                checker.types.get(expected),
                Type::Option(_) | Type::Union(_)
            ) {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    "reducta_nullable_result_requires_coalesce",
                    span,
                );
            }
        }
        return nullable;
    }
    if grouped {
        if let Some((_, shape)) = expected_group {
            checker.types.array(result);
            return checker.types.tensor_with_shape(result, shape);
        }
        return checker.error_type;
    }
    result
}

/// Type-check the keys of a grouped reduction's `per [i, j]` coordinate list.
///
/// `per k` is the one-key case of `per [k]`. Returns the key count when the
/// group is an admitted key list, `None` after a named refusal.
fn check_group_keys(
    checker: &mut TypeChecker<'_>,
    group: &HirExpression,
    group_ty: TypeId,
) -> Option<usize> {
    if let HirExpressionKind::Array(elements) = &group.kind {
        let plain = elements
            .iter()
            .all(|element| matches!(element, radix_hir::HirArrayElement::Expr(_)));
        if !plain || elements.is_empty() {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_REDUCTA_GROUP_KEYS_NOT_LITERAL,
                group.span,
            );
            return None;
        }
    } else if matches!(
        checker.types.get(group_ty),
        Type::Array(_)
            | Type::BoundedArray { .. }
            | Type::RangedArray { .. }
            | Type::Tuple(_)
            | Type::Tensor(..)
            | Type::Matrix(..)
    ) {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_REDUCTA_GROUP_KEYS_NOT_LITERAL,
            group.span,
        );
        return None;
    }
    let keys = radix_hir::reduction_group_keys(group);
    for key in &keys {
        let key_ty = checker.resolve_inferred_type(key.ty.unwrap_or(checker.error_type));
        if !matches!(
            checker.types.get(key_ty),
            Type::Primitive(Primitive::Numerus) | Type::SizedNumeric(Primitive::Numerus, _)
        ) {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                "reducta_group_key_not_integer",
                key.span,
            );
        }
    }
    Some(keys.len())
}

pub(super) fn is_numeric_scalar(types: &TypeTable, ty: TypeId) -> bool {
    matches!(
        types.get(ty),
        Type::Primitive(Primitive::Numerus | Primitive::Fractus)
            | Type::SizedNumeric(Primitive::Numerus | Primitive::Fractus, _)
    )
}

struct ReductionBodyState {
    /// Bindings the body owns: coordinates, term binder, filum binder, and
    /// body-declared locals. Writes to any other binding are captured-state
    /// mutation (impure) — `captured_mut_host` in the fragment vocabulary.
    own_ids: FxHashSet<DefId>,
    /// Total `redde` statements in the body tree.
    redde_count: usize,
    /// `redde` statements carrying a value.
    valued_redde: usize,
}

pub(super) fn check_reducta_body_laws(
    checker: &mut TypeChecker<'_>,
    coordinates: &[(DefId, Symbol)],
    filum: Option<(DefId, Symbol)>,
    binding: (DefId, Symbol),
    body: &HirBlock,
) {
    let mut state = ReductionBodyState {
        own_ids: coordinates
            .iter()
            .map(|(def_id, _)| *def_id)
            .chain(filum.map(|(def_id, _)| def_id))
            .chain(std::iter::once(binding.0))
            .collect(),
        redde_count: 0,
        valued_redde: 0,
    };
    state.walk_block(checker, body);
    if state.redde_count != 1 || state.valued_redde != 1 {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_REDUCTA_BODY_REQUIRES_SINGLE_REDDE,
            body.span,
        );
    }
}

impl ReductionBodyState {
    fn walk_block(&mut self, checker: &mut TypeChecker<'_>, block: &HirBlock) {
        for stmt in &block.statements {
            self.walk_stmt(checker, stmt);
        }
        if let Some(expr) = &block.expr {
            self.walk_expr(checker, expr);
        }
    }

    fn walk_stmt(&mut self, checker: &mut TypeChecker<'_>, stmt: &HirStatement) {
        match &stmt.kind {
            HirStatementKind::Local(local) => {
                self.own_ids.insert(local.def_id);
                if let Some(init) = &local.init {
                    self.walk_expr(checker, init);
                }
            }
            HirStatementKind::Redde(value) => {
                self.redde_count += 1;
                if let Some(value) = value {
                    self.valued_redde += 1;
                    self.walk_expr(checker, value);
                }
            }
            HirStatementKind::ReddeFilum(value) => {
                if let Some(value) = value {
                    self.walk_expr(checker, value);
                }
            }
            HirStatementKind::Expr(expr) => self.walk_expr(checker, expr),
            HirStatementKind::Rumpe => checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_REDUCTA_BODY_FORBIDS_RUMPE,
                stmt.span,
            ),
            HirStatementKind::Perge | HirStatementKind::Tacet => {}
            HirStatementKind::IncDec(inc_dec) => self.walk_place(checker, &inc_dec.target),
            HirStatementKind::Custodi(custodi) => {
                for clause in &custodi.clauses {
                    self.walk_expr(checker, &clause.cond);
                    self.walk_block(checker, &clause.body);
                }
            }
        }
    }

    fn walk_expr(&mut self, checker: &mut TypeChecker<'_>, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(_)
            | HirExpressionKind::Literal(_)
            | HirExpressionKind::Insere(_)
            | HirExpressionKind::Vacua
            | HirExpressionKind::Error => {}
            HirExpressionKind::Widen { operand, .. } | HirExpressionKind::Unary(_, operand) => {
                self.walk_expr(checker, operand);
            }
            HirExpressionKind::Binary(_, lhs, rhs) => {
                self.walk_expr(checker, lhs);
                self.walk_expr(checker, rhs);
            }
            HirExpressionKind::Call(callee, _, args, _) => {
                self.walk_call(checker, callee, args, expr.span);
            }
            // `target ⇇ value` writes existing tensor storage, an effect a term
            // body does not admit; keep the same rejection as `∇`.
            HirExpressionKind::CopyInto { target, value } => {
                effect(checker, expr.span, "unapproved_effect");
                self.walk_expr(checker, target);
                self.walk_expr(checker, value);
            }
            // `sink ⇜ item` writes through the sink, an effect a term body does
            // not admit; keep the same rejection as `⇇`.
            HirExpressionKind::SinkInto { sink, item } => {
                effect(checker, expr.span, "unapproved_effect");
                self.walk_expr(checker, sink);
                self.walk_expr(checker, item);
            }
            HirExpressionKind::Gradient {
                call,
                selected_arguments,
            } => {
                effect(checker, expr.span, "unapproved_effect");
                self.walk_expr(checker, call);
                for selector in selected_arguments {
                    self.walk_expr(checker, selector);
                }
            }
            HirExpressionKind::MethodCall(receiver, method, _, args, _) => {
                self.walk_method_call(checker, receiver, *method, args, expr.span);
            }
            HirExpressionKind::Field(object, _) => self.walk_expr(checker, object),
            HirExpressionKind::Index(object, index) => {
                self.walk_expr(checker, object);
                self.walk_expr(checker, index);
            }
            HirExpressionKind::OptionalChain(object, _) | HirExpressionKind::NonNull(object, _) => {
                effect(checker, expr.span, "unapproved_effect");
                self.walk_expr(checker, object);
            }
            HirExpressionKind::Marked { expr: inner, .. } | HirExpressionKind::Praefixum(inner) => {
                self.walk_expr(checker, inner);
            }
            HirExpressionKind::Block(block) | HirExpressionKind::Loop(block) => {
                self.walk_block(checker, block);
            }
            HirExpressionKind::Si {
                cond,
                then_block,
                then_catch,
                else_block,
            } => {
                self.walk_expr(checker, cond);
                if let Some(catch) = then_catch {
                    self.walk_cape(checker, catch);
                }
                self.walk_block(checker, then_block);
                if let Some(else_block) = else_block {
                    self.walk_block(checker, else_block);
                }
            }
            HirExpressionKind::Discerne {
                scrutinees, arms, ..
            } => {
                for scrutinee in scrutinees {
                    self.walk_expr(checker, scrutinee);
                }
                for arm in arms {
                    self.walk_expr(checker, &arm.body);
                }
            }
            HirExpressionKind::Dum(cond, block) => {
                self.walk_expr(checker, cond);
                self.walk_block(checker, block);
            }
            HirExpressionKind::Itera {
                iter: source,
                body: block,
                ..
            } => {
                self.walk_expr(checker, source);
                self.walk_block(checker, block);
            }
            HirExpressionKind::IteraLockstep { sources, body, .. } => {
                for source in sources {
                    self.walk_expr(checker, source);
                }
                self.walk_block(checker, body);
            }
            HirExpressionKind::IteraRange { ranges, body, .. } => {
                for range in ranges {
                    self.walk_expr(checker, range);
                }
                self.walk_block(checker, body);
            }
            // Nested reductions are admitted/declined by their own admission
            // check under the ordinary expression path; the fragment subset
            // treats the nested node as an unapproved effect, so the outer
            // body walk does not descend into it.
            HirExpressionKind::Reduction { .. }
            | HirExpressionKind::Scan { .. }
            | HirExpressionKind::Sort { .. } => {
                effect(checker, expr.span, "unapproved_effect");
            }
            HirExpressionKind::Intervallum {
                start, end, step, ..
            } => {
                self.walk_expr(checker, start);
                self.walk_expr(checker, end);
                if let Some(step) = step {
                    self.walk_expr(checker, step);
                }
            }
            HirExpressionKind::Assign(target, value) => {
                self.walk_place(checker, target);
                self.walk_expr(checker, value);
            }
            HirExpressionKind::ConversioAssign {
                target,
                source,
                recovery,
                ..
            } => {
                if recovery.is_some() {
                    effect(checker, expr.span, "alternate_exit");
                }
                self.walk_place(checker, target);
                self.walk_expr(checker, source);
                if let Some(recovery) = recovery {
                    self.walk_expr(checker, recovery);
                }
            }
            HirExpressionKind::Array(elements) => {
                effect(checker, expr.span, "host_allocation");
                for element in elements {
                    match element {
                        HirArrayElement::Expr(inner) | HirArrayElement::Spread(inner) => {
                            self.walk_expr(checker, inner);
                        }
                        HirArrayElement::Infer(_) => {}
                    }
                }
            }
            HirExpressionKind::Struct(_, fields) => {
                for (_, value) in fields {
                    self.walk_expr(checker, value);
                }
            }
            HirExpressionKind::Tuple(elements, _) => {
                for element in elements {
                    self.walk_expr(checker, element);
                }
            }
            HirExpressionKind::Scribe(_, elements) | HirExpressionKind::Scriptum(_, elements) => {
                effect(checker, expr.span, "host_io");
                for element in elements {
                    self.walk_expr(checker, element);
                }
            }
            HirExpressionKind::ReadLine(_) => effect(checker, expr.span, "host_io"),
            // `¶` builds text, like `scriptum`.
            HirExpressionKind::Format { value, .. } => {
                effect(checker, expr.span, "host_io");
                self.walk_expr(checker, value);
            }
            HirExpressionKind::Adfirma(cond, message) => {
                effect(checker, expr.span, "unapproved_effect");
                self.walk_expr(checker, cond);
                if let Some(message) = message {
                    self.walk_expr(checker, message);
                }
            }
            HirExpressionKind::Panic(message) => {
                effect(checker, expr.span, "unapproved_effect");
                self.walk_expr(checker, message);
            }
            HirExpressionKind::Throw(value) => {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_REDUCTA_BODY_FORBIDS_IACE,
                    expr.span,
                );
                self.walk_expr(checker, value);
            }
            HirExpressionKind::Handled { body, catch } => {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_REDUCTA_BODY_FORBIDS_CAPE,
                    expr.span,
                );
                self.walk_block(checker, body);
                self.walk_cape(checker, catch);
            }
            // UT: a trap inside a `summa` term body is an alternate-exit
            // dataflow the fold cannot host — same rejection class as cape.
            HirExpressionKind::Trap(body) => {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_REDUCTA_BODY_FORBIDS_CAPE,
                    expr.span,
                );
                self.walk_block(checker, body);
            }
            HirExpressionKind::CallDefault { body, default, .. } => {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_REDUCTA_BODY_FORBIDS_CAPE,
                    expr.span,
                );
                self.walk_block(checker, body);
                self.walk_expr(checker, default);
            }
            HirExpressionKind::Clausura(params, _, _, body) => {
                effect(checker, expr.span, "dynamic_function");
                for param in params {
                    if let Some(default) = &param.default {
                        self.walk_expr(checker, default);
                    }
                }
                self.walk_expr(checker, body);
            }
            HirExpressionKind::Cede(value) | HirExpressionKind::Yield(value) => {
                checker.error_issue(
                    SemanticErrorKind::TypeMismatch,
                    ISSUE_REDUCTA_BODY_FORBIDS_CEDE,
                    expr.span,
                );
                self.walk_expr(checker, value);
            }
            HirExpressionKind::Reddet(value) | HirExpressionKind::Tacebit(value) => {
                effect(checker, expr.span, "async_posture");
                self.walk_expr(checker, value);
            }
            HirExpressionKind::Verte {
                source, entries, ..
            } => {
                self.walk_expr(checker, source);
                if let Some(entries) = entries {
                    for field in entries {
                        self.walk_object_field(checker, field);
                    }
                }
            }
            HirExpressionKind::Conversio {
                source,
                target,
                recovery,
                ..
            } => {
                if recovery.is_some() {
                    effect(checker, expr.span, "alternate_exit");
                }
                self.walk_expr(checker, source);
                if let HirConversioTarget::Intervallum(operand) = target {
                    self.walk_expr(checker, operand);
                }
                if let Some(recovery) = recovery {
                    self.walk_expr(checker, recovery);
                }
            }
            HirExpressionKind::Ad { opener, .. } => {
                effect(checker, expr.span, "host_runtime");
                if let Some(opener) = opener {
                    self.walk_expr(checker, opener);
                }
            }
            HirExpressionKind::Ref(_, operand) | HirExpressionKind::Deref(operand) => {
                effect(checker, expr.span, "address_taking");
                self.walk_expr(checker, operand);
            }
            HirExpressionKind::TypeCheck { expr: inner, .. } => {
                effect(checker, expr.span, "unapproved_effect");
                self.walk_expr(checker, inner);
            }
        }
    }

    fn walk_place(&mut self, checker: &mut TypeChecker<'_>, expr: &HirExpression) {
        match &expr.kind {
            HirExpressionKind::Path(def_id) => {
                if !self.own_ids.contains(def_id) {
                    effect(checker, expr.span, "captured_mut_host");
                }
            }
            HirExpressionKind::Field(object, _) => self.walk_place(checker, object),
            HirExpressionKind::Index(object, index) => {
                self.walk_place(checker, object);
                self.walk_expr(checker, index);
            }
            _ => self.walk_expr(checker, expr),
        }
    }

    fn walk_cape(&mut self, checker: &mut TypeChecker<'_>, cape: &HirCape) {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_REDUCTA_BODY_FORBIDS_CAPE,
            cape.span,
        );
        self.walk_block(checker, &cape.body);
    }

    fn walk_object_field(&mut self, checker: &mut TypeChecker<'_>, field: &HirObjectField) {
        match &field.key {
            HirObjectKey::Ident(_) | HirObjectKey::String(_) => {}
            HirObjectKey::Computed(expr) | HirObjectKey::Spread(expr) => {
                self.walk_expr(checker, expr);
            }
        }
        if let Some(value) = &field.value {
            self.walk_expr(checker, value);
        }
    }

    fn walk_call(
        &mut self,
        checker: &mut TypeChecker<'_>,
        callee: &HirExpression,
        args: &[HirCallArg],
        span: Span,
    ) {
        match &callee.kind {
            HirExpressionKind::Path(def_id) => Self::classify_callee(checker, *def_id, span),
            other => {
                if !matches!(other, HirExpressionKind::Error) {
                    effect(checker, span, "dynamic_function");
                }
                self.walk_expr(checker, callee);
            }
        }
        for arg in args {
            self.walk_expr(checker, &arg.expr);
        }
    }

    fn walk_method_call(
        &mut self,
        checker: &mut TypeChecker<'_>,
        receiver: &HirExpression,
        method: Symbol,
        args: &[HirCallArg],
        span: Span,
    ) {
        if let Some(role) = resolve_method_role(checker, receiver, method) {
            match role {
                NucleumRole::Fragment => {}
                NucleumRole::Entry if is_inlinable_method(checker, receiver, method) => {}
                NucleumRole::Entry => context(checker, span, "entry_callee"),
            }
        } else if is_allowed_device_method(checker.interner.map_or("", |i| i.resolve(method)))
            && is_device_value_type(receiver.ty.unwrap_or(checker.error_type), checker.types)
        {
            // Approved device intrinsic method — admitted (fragment-compatible).
        } else {
            context(checker, span, "host_callable");
        }
        self.walk_expr(checker, receiver);
        for arg in args {
            self.walk_expr(checker, &arg.expr);
        }
    }

    /// Callee classification shared with the fragment definition rules:
    /// device intrinsics, `@ nucleum fragment` calls and inlinable kernels
    /// (public or private) are admitted; async or fallible kernels, graphics
    /// stages, and ordinary host callables are not.
    fn classify_callee(checker: &mut TypeChecker<'_>, def_id: DefId, span: Span) {
        if checker.resolver.gpu_builtin(def_id).is_some() {
            return;
        }
        match checker.nucleum_roles.get(&def_id).copied() {
            Some(NucleumRole::Fragment) => {}
            // Rule 12.7 inlines a kernel (public or private) into its caller,
            // so it is a term function here; its body is proved write-free
            // after typecheck (`filum_independence`).
            Some(NucleumRole::Entry) if checker.inlinable_kernels.contains(&def_id) => {}
            Some(NucleumRole::Entry) => context(checker, span, "entry_callee"),
            None => context(checker, span, "host_callable"),
        }
    }
}

/// Fragment-compatible effect rejection (`reducta_body_effect`).
fn effect(checker: &mut TypeChecker<'_>, span: Span, reason: &'static str) {
    checker.errors.push(
        SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
            .with_arg("issue", ISSUE_REDUCTA_BODY_EFFECT)
            .with_arg("reason", reason),
    );
}

/// Fragment-compatible call-context rejection (`reducta_body_context`).
fn context(checker: &mut TypeChecker<'_>, span: Span, reason: &'static str) {
    checker.errors.push(
        SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
            .with_arg("issue", ISSUE_REDUCTA_BODY_CONTEXT)
            .with_arg("reason", reason),
    );
}

/// Resolve a method call's nucleum role via the method role table. Methods
/// without a nucleum role fall through to the device-intrinsic and
/// host-callable checks in [`ReductionBodyState::walk_method_call`].
fn resolve_method_role(
    checker: &TypeChecker<'_>,
    receiver: &HirExpression,
    method: Symbol,
) -> Option<NucleumRole> {
    let receiver_ty = receiver.ty?;
    let struct_def = struct_def_from_type(receiver_ty, checker.types)?;
    checker
        .method_nucleum_roles
        .get(&(struct_def, method))
        .copied()
}

/// A method kernel the filum pass inlines into its caller (rule 12.7): the
/// receiver type names the class statically, so the method definition is the
/// one the type checker resolves the call to.
fn is_inlinable_method(
    checker: &TypeChecker<'_>,
    receiver: &HirExpression,
    method: Symbol,
) -> bool {
    receiver
        .ty
        .and_then(|ty| struct_def_from_type(ty, checker.types))
        .and_then(|class| checker.method_defs.get(&(class, method)))
        .is_some_and(|def| checker.inlinable_kernels.contains(def))
}
