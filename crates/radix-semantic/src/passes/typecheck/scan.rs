//! Type and finite-domain checks for ordered prefix scans.

#[allow(clippy::wildcard_imports)]
use super::*;
use radix_hir::{
    HirArrayElement, HirExpressionKind, HirReductionOp, HirScanGroupDomain, HirScanKeyDomain,
    HirScanTotal,
};
use radix_types::{NumericWidth, Primitive, Type};

const ISSUE_SCAN_SOURCE_REQUIRES_SHAPE: &str = "scan_requires_shaped_iterable";
const ISSUE_SCAN_REQUIRES_KERNEL: &str = "scan_thread_requires_nucleum_kernel";
const ISSUE_SCAN_TERM_TYPE_NON_SCALAR: &str = "scan_term_type_non_scalar";
const ISSUE_SCAN_TERM_TYPE_NOT_BOOL: &str = "scan_term_type_not_bool";
const ISSUE_SCAN_OPERATOR_UNSUPPORTED: &str = "scan_operator_unsupported";
const ISSUE_SCAN_EXCLUSIVE_EXTREMUM_UNSUPPORTED: &str = "scan_exclusive_extremum_unsupported";
const ISSUE_SCAN_GROUP_KEYS_NOT_LITERAL: &str = "scan_group_keys_not_literal";
const ISSUE_SCAN_GROUP_KEY_NOT_INTEGER: &str = "scan_group_key_not_integer";
const ISSUE_SCAN_GROUP_DOMAIN_UNBOUNDED: &str = "scan_group_domain_unbounded";

/// Type one source-shaped prefix scan and attach its proven finite key domain.
pub(super) fn check_scan_expr(
    checker: &mut TypeChecker<'_>,
    op: HirReductionOp,
    total: HirScanTotal,
    source: &mut HirExpression,
    coordinates: &[(radix_hir::DefId, Symbol)],
    group: Option<&mut Box<HirExpression>>,
    group_domain: &mut Option<HirScanGroupDomain>,
    filum: (radix_hir::DefId, Symbol),
    binding: (radix_hir::DefId, Symbol),
    body: &mut HirBlock,
    span: radix_lexer::Span,
) -> TypeId {
    *group_domain = None;
    if matches!(op, HirReductionOp::ArgMax | HirReductionOp::ArgMin) {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_SCAN_OPERATOR_UNSUPPORTED,
            span,
        );
    }
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
                ISSUE_SCAN_SOURCE_REQUIRES_SHAPE,
                span,
            );
            return checker.error_type;
        }
    };
    let element = checker.resolve_inferred_type(element);
    let dimensions = crate::i64s_from_literal_index(shape, checker.types);
    match super::expr::apud_arity(checker.types, dispatch, true, coordinates.len()) {
        super::expr::ApudArity::Admitted => {}
        super::expr::ApudArity::RequiresShaped => checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            super::expr::ISSUE_ITERA_APUD_REQUIRES_SHAPED_ITERABLE,
            span,
        ),
        super::expr::ApudArity::Mismatch { expected, actual } => checker.errors.push(
            SemanticError::structured(SemanticErrorKind::TypeMismatch, span)
                .with_arg("issue", super::expr::ISSUE_ITERA_APUD_ARITY_MISMATCH)
                .with_arg("expected", expected.to_string())
                .with_arg("actual", actual.to_string()),
        ),
    }
    if !checker.current_nucleum_entry {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_SCAN_REQUIRES_KERNEL,
            span,
        );
    }
    if matches!(total, HirScanTotal::Exclusive)
        && matches!(op, HirReductionOp::Max | HirReductionOp::Min)
    {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_SCAN_EXCLUSIVE_EXTREMUM_UNSUPPORTED,
            span,
        );
    }

    checker.push_scope();
    let coordinate_ty = if checker.current_nucleum_entry {
        checker.width_type(NumericWidth::I32)
    } else {
        checker.numerus_type()
    };
    checker.types.array(coordinate_ty);
    for (id, _) in coordinates {
        checker.insert_binding(*id, coordinate_ty);
    }
    checker.seed_coordinates(dispatch, None, coordinates);
    checker.insert_binding(filum.0, coordinate_ty);
    let mut grouped = false;
    if let Some(group) = group {
        grouped = true;
        let checked_group_ty = checker.check_expr(group);
        let group_ty = checker.resolve_inferred_type(checked_group_ty);
        let keys = scan_group_components(checker, group, group_ty, span);
        if let Some(keys) = keys {
            let key_count = keys.len();
            let mut domains = Vec::with_capacity(key_count);
            let mut proven = true;
            for key in keys {
                let key_ty = checker.resolve_inferred_type(key.ty.unwrap_or(checker.error_type));
                let Some(width) = radix_types::numeric_width::numerus_integer_width(
                    checker.types,
                    checker.types.peel_access_marker(key_ty).1,
                ) else {
                    checker.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_SCAN_GROUP_KEY_NOT_INTEGER,
                        key.span,
                    );
                    proven = false;
                    continue;
                };
                let domain =
                    scan_key_domain(checker, key, width, coordinates, dimensions.as_deref());
                if let Some(domain) = domain {
                    domains.push(domain);
                } else {
                    checker.error_issue(
                        SemanticErrorKind::TypeMismatch,
                        ISSUE_SCAN_GROUP_DOMAIN_UNBOUNDED,
                        key.span,
                    );
                    proven = false;
                }
            }
            if proven && domains.len() == key_count {
                *group_domain = Some(HirScanGroupDomain {
                    key_domains: domains,
                });
            }
        }
    }
    checker.insert_binding(binding.0, element);
    let previous_return = checker.current_return.take();
    let previous_inferred = checker.inferred_return.take();
    let previous_allow = checker.allow_inferred_return;
    checker.allow_inferred_return = true;
    checker.check_block(body, None);
    let term_ty = checker.inferred_return.unwrap_or(checker.error_type);
    checker.current_return = previous_return;
    checker.inferred_return = previous_inferred;
    checker.allow_inferred_return = previous_allow;
    super::reduction::check_reducta_body_laws(checker, coordinates, Some(filum), binding, body);
    checker.release_range_binders(coordinates.iter().map(|(id, _)| *id));
    checker.pop_scope();

    let term_ty = checker.resolve_inferred_type(term_ty);
    let boolean = checker.types.primitive(Primitive::Bivalens);
    if matches!(
        op,
        HirReductionOp::All | HirReductionOp::Any | HirReductionOp::Count
    ) {
        if term_ty != boolean {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_SCAN_TERM_TYPE_NOT_BOOL,
                body.span,
            );
        }
    } else if !super::reduction::is_numeric_scalar(checker.types, term_ty) {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_SCAN_TERM_TYPE_NON_SCALAR,
            body.span,
        );
    }

    let output_element = match op {
        HirReductionOp::Count => coordinate_ty,
        HirReductionOp::All | HirReductionOp::Any => boolean,
        HirReductionOp::Sum | HirReductionOp::Product => checker.summa_result_type(term_ty),
        HirReductionOp::Max | HirReductionOp::Min => term_ty,
        HirReductionOp::ArgMax | HirReductionOp::ArgMin => checker.error_type,
    };
    checker.types.array(output_element);
    let result = checker.types.tensor_with_shape(output_element, shape);
    if grouped && group_domain.is_none() {
        checker.error_type
    } else {
        result
    }
}

fn scan_group_components<'a>(
    checker: &mut TypeChecker<'_>,
    group: &'a HirExpression,
    group_ty: TypeId,
    span: radix_lexer::Span,
) -> Option<Vec<&'a HirExpression>> {
    if let HirExpressionKind::Array(elements) = &group.kind {
        if elements.is_empty()
            || elements
                .iter()
                .any(|element| !matches!(element, HirArrayElement::Expr(_)))
        {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_SCAN_GROUP_KEYS_NOT_LITERAL,
                group.span,
            );
            return None;
        }
        return Some(
            elements
                .iter()
                .filter_map(|element| match element {
                    HirArrayElement::Expr(expr) => Some(expr),
                    HirArrayElement::Spread(_) | HirArrayElement::Infer(_) => None,
                })
                .collect(),
        );
    }
    if matches!(
        checker.types.get(group_ty),
        Type::Array(_)
            | Type::BoundedArray { .. }
            | Type::Tuple(_)
            | Type::Tensor(..)
            | Type::Matrix(..)
    ) {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_SCAN_GROUP_KEYS_NOT_LITERAL,
            span,
        );
        None
    } else {
        Some(vec![group])
    }
}

fn scan_key_domain(
    checker: &TypeChecker<'_>,
    key: &HirExpression,
    width: NumericWidth,
    coordinates: &[(radix_hir::DefId, Symbol)],
    dimensions: Option<&[i64]>,
) -> Option<HirScanKeyDomain> {
    if let Some(value) =
        super::integer_range::const_int_value_resolved(key, &checker.const_int_values)
    {
        return Some(HirScanKeyDomain::IntegerRange {
            min: value,
            max: value,
        });
    }
    let coordinate_axis = match &key.kind {
        HirExpressionKind::Path(def_id) => coordinates
            .iter()
            .position(|(coordinate, _)| coordinate == def_id),
        _ => None,
    };
    if let Some(axis) = coordinate_axis
        && let Some(extent) = dimensions.and_then(|dims| dims.get(axis))
    {
        return Some(HirScanKeyDomain::IntegerRange {
            min: 0,
            max: i128::from(*extent) - 1,
        });
    }
    let (min, max) = scan_key_interval(checker, key, coordinates, dimensions)?;
    // The shared i128 range analysis saturates when an exact result exceeds
    // its carrier. A saturated endpoint is not a proven key bound, so decline
    // instead of treating that sentinel as a finite interval.
    if min == i128::MIN || max == i128::MAX {
        return None;
    }
    if (scan_key_is_stored_boundary(key) || coordinate_axis.is_some())
        && width.integer_range() == Some((min, max))
    {
        Some(HirScanKeyDomain::IntegerWidth(width))
    } else {
        Some(HirScanKeyDomain::IntegerRange { min, max })
    }
}

fn scan_key_interval(
    checker: &TypeChecker<'_>,
    key: &HirExpression,
    coordinates: &[(radix_hir::DefId, Symbol)],
    dimensions: Option<&[i64]>,
) -> Option<(i128, i128)> {
    if let Some(value) =
        super::integer_range::const_int_value_resolved(key, &checker.const_int_values)
    {
        return Some((value, value));
    }
    match &key.kind {
        HirExpressionKind::Path(def_id) => {
            if let Some(axis) = coordinates
                .iter()
                .position(|(coordinate, _)| coordinate == def_id)
            {
                if let Some(extent) = dimensions.and_then(|dims| dims.get(axis)) {
                    Some((0, i128::from(*extent) - 1))
                } else {
                    key.ty
                        .and_then(|ty| {
                            radix_types::numeric_width::numerus_integer_width(checker.types, ty)
                        })
                        .and_then(NumericWidth::integer_range)
                }
            } else {
                checker.value_range(key, key.ty)
            }
        }
        HirExpressionKind::Binary(op, lhs, rhs) => TypeChecker::binary_value_range(
            *op,
            scan_key_interval(checker, lhs, coordinates, dimensions)?,
            scan_key_interval(checker, rhs, coordinates, dimensions)?,
        ),
        HirExpressionKind::Unary(radix_hir::HirUnOp::Neg, operand) => {
            scan_key_interval(checker, operand, coordinates, dimensions)
                .map(radix_types::numeric_range::neg)
        }
        HirExpressionKind::Unary(radix_hir::HirUnOp::BitNot, operand) => {
            scan_key_interval(checker, operand, coordinates, dimensions)
                .map(radix_types::numeric_range::bit_not)
        }
        _ if scan_key_is_stored_boundary(key) => checker.value_range(key, key.ty),
        _ => None,
    }
}

fn scan_key_is_stored_boundary(key: &HirExpression) -> bool {
    matches!(
        &key.kind,
        HirExpressionKind::Path(_)
            | HirExpressionKind::Field(..)
            | HirExpressionKind::Index(..)
            | HirExpressionKind::Call(..)
            | HirExpressionKind::MethodCall(..)
            | HirExpressionKind::Verte { .. }
            | HirExpressionKind::Conversio { .. }
            | HirExpressionKind::Widen { .. }
    )
}
