//! Typing laws for the sort expression `sort via asc|desc from|ref source
//! [const p { return key }]` (need, rulings O1 to O5).
//!
//! - `from` orders values: a list, a set (its elements) or a map (its values).
//! - `ref` orders keys and is admitted on a map only.
//! - A tensor source is refused by name.
//! - Without a body the ordered terms are the sort keys; with a body the
//!   returned expression is the key. A key is never a comparison.
//! - The result is always a new list of the ordered terms.

#[allow(clippy::wildcard_imports)]
// parent module provides shared imports for typechecking
use super::*;
use radix_hir::HirSortSource;
use radix_lexer::Span;

const ISSUE_SORT_REF_REQUIRES_MAP: &str = "sort_ref_requires_map";
const ISSUE_SORT_TENSOR_UNSUPPORTED: &str = "sort_tensor_unsupported";
const ISSUE_SORT_SOURCE_UNORDERED: &str = "sort_source_unordered";
const ISSUE_SORT_KEY_UNORDERED: &str = "sort_key_unordered";
const ISSUE_SORT_SOURCE_NOT_COLLECTION: &str = "sort_source_not_collection";
const ISSUE_SORT_BODY_REQUIRES_REDDE: &str = "sort_body_requires_redde";

enum SortSource {
    Array(TypeId),
    Set(TypeId),
    Map(TypeId, TypeId),
    Tensor,
    Other,
}

/// Type one sort expression and produce its `list<term>` result type.
pub(super) fn check_sort_expr(
    checker: &mut TypeChecker<'_>,
    source_mode: HirSortSource,
    source: &mut HirExpression,
    binding: Option<(DefId, Symbol)>,
    body: Option<&mut HirBlock>,
    span: Span,
) -> TypeId {
    let source_ty = checker.check_expr(source);
    let dispatch = checker
        .types
        .peel_access_marker(checker.resolve_inferred_type(source_ty))
        .1;
    if dispatch == checker.error_type {
        return checker.error_type;
    }
    let kind = match checker.types.get(dispatch) {
        Type::Array(element) | Type::BoundedArray { element, .. } => SortSource::Array(*element),
        Type::Set(element) => SortSource::Set(*element),
        Type::Map(key, value) => SortSource::Map(*key, *value),
        Type::Tensor(..) | Type::Matrix(..) | Type::Sparsa(..) => SortSource::Tensor,
        _ => SortSource::Other,
    };
    let element = match (kind, source_mode) {
        (SortSource::Tensor, _) => {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_SORT_TENSOR_UNSUPPORTED,
                source.span,
            );
            return checker.error_type;
        }
        (SortSource::Other, _) => {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_SORT_SOURCE_NOT_COLLECTION,
                source.span,
            );
            return checker.error_type;
        }
        (SortSource::Array(_) | SortSource::Set(_), HirSortSource::Keys) => {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_SORT_REF_REQUIRES_MAP,
                source.span,
            );
            return checker.error_type;
        }
        (SortSource::Array(element) | SortSource::Set(element), HirSortSource::Values) => element,
        (SortSource::Map(_, value), HirSortSource::Values) => value,
        (SortSource::Map(key, _), HirSortSource::Keys) => key,
    };
    let element = checker.resolve_inferred_type(element);
    let key_ty = if let (Some(binding), Some(body)) = (binding, body) {
        checker.push_scope();
        checker.insert_binding(binding.0, element);
        let prev_return = checker.current_return.take();
        let prev_inferred = checker.inferred_return.take();
        let prev_allow = checker.allow_inferred_return;
        checker.allow_inferred_return = true;
        checker.check_block(body, None);
        let inferred = checker.inferred_return;
        checker.current_return = prev_return;
        checker.inferred_return = prev_inferred;
        checker.allow_inferred_return = prev_allow;
        checker.pop_scope();
        let Some(key) = inferred else {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_SORT_BODY_REQUIRES_REDDE,
                body.span,
            );
            return checker.error_type;
        };
        let key = checker.resolve_inferred_type(key);
        if key != checker.error_type && !is_sort_orderable(checker, key) {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_SORT_KEY_UNORDERED,
                span,
            );
            return checker.error_type;
        }
        Some(key)
    } else {
        if element != checker.error_type && !is_sort_orderable(checker, element) {
            checker.error_issue(
                SemanticErrorKind::TypeMismatch,
                ISSUE_SORT_SOURCE_UNORDERED,
                span,
            );
            return checker.error_type;
        }
        None
    };
    // WHY: MIR lowering reads these carriers from a read-only type table, so
    // intern the element list, the key list and the key selector's function
    // type here where the form is admitted.
    let result = checker.types.array(element);
    if let Some(key) = key_ty {
        checker.types.array(key);
        let selector = FuncSig {
            type_params: Vec::new(),
            params: vec![ParamType {
                ty: element,
                mode: SemanticParamMode::Owned,
                optional: false,
                sponte: false,
            }],
            ret: key,
            err: None,
            is_async: false,
            is_generator: false,
        };
        checker.types.function(selector);
    }
    result
}

/// The orderings `.sort()` and the sort expression accept: numbers, text, bool.
fn is_sort_orderable(checker: &TypeChecker<'_>, ty: TypeId) -> bool {
    checker.is_numeric(ty) || checker.is_textus(ty) || checker.is_bool(ty)
}
