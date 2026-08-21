//! Type-checking for compiler-owned receiver intrinsics via the shared registry.
//!
//! COMPILER PHASE: Semantic typecheck
//! INPUT: Resolved receiver type and method name for compiler-owned intrinsic
//! calls on lista, tabula, tensor, matrix, vector, atomic, intervallum, sparsa,
//! octeti, modulus, and numeric receivers
//! OUTPUT: Checked argument/return types via the intrinsic registry; structured
//! diagnostics for type mismatches, unsupported receivers, or non-numeric element
//! requirements on arithmetic intrinsics

use super::TypeChecker;
use crate::{
    FuncSig, NumericWidth, ParamType, Primitive, SemanticErrorKind, SemanticParamMode, Type, TypeId,
};
use radix_hir::{HirCallArg, HirExpression, HirExpressionKind};
use radix_intrinsics::{
    is_mutating_receiver, is_numeric_bitwise_operator, is_numeric_imperativus_operator,
    lookup_method, lookup_method_for_type, receivers_for_type, IntrinsicParamKind,
    IntrinsicReceiver, IntrinsicReturnKind, MethodIntrinsic,
};
use radix_lexer::Symbol;

const ISSUE_BITWISE_METHOD_REQUIRES_NUMERUS_RECEIVER: &str =
    "bitwise_method_requires_numerus_receiver";
const ISSUE_DENSATA_REQUIRES_SPARSA_RECEIVER: &str = "densata_requires_sparsa_receiver";
const ISSUE_ARGUMENT_TYPE_MISMATCH: &str = "argument_type_mismatch";
const ISSUE_NUMERIC_LISTA_ELEMENTS_REQUIRED: &str = "numeric_lista_elements_required";
const ISSUE_TENSOR_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED: &str =
    "tensor_arithmetic_numeric_element_required";
const ISSUE_MATRIX_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED: &str =
    "matrix_arithmetic_numeric_element_required";
const ISSUE_TENSOR_INDEX_VECTOR_INTEGER_ELEMENTS: &str = "tensor_index_vector_integer_elements";
const ISSUE_IMPERATIVUS_OPERATOR_METHOD_REQUIRES_VARIA_RECEIVER: &str =
    "imperativus_operator_method_requires_varia_receiver";
pub(super) const ISSUE_ATOMIC_METHOD_REQUIRES_ATOMIC_RECEIVER: &str =
    "atomic_method_requires_atomic_receiver";
pub(super) const ISSUE_UNSUPPORTED_ATOMIC_METHOD: &str = "unsupported_atomic_method";

/// Receiver element types needed to resolve intrinsic param/return shapes.
#[derive(Debug, Clone, Copy)]
struct IntrinsicTypeContext {
    inner: Option<TypeId>,
    key: Option<TypeId>,
    value: Option<TypeId>,
    receiver_ty: Option<TypeId>,
}

impl IntrinsicTypeContext {
    fn from_receiver_type(checker: &mut TypeChecker<'_>, receiver_ty: TypeId) -> Self {
        let resolved = checker.resolve_inferred_type(receiver_ty);
        let bounded_cord = matches!(
            checker.types.get(resolved),
            Type::BoundedTextus { .. } | Type::BoundedAscii { .. }
        );
        if bounded_cord {
            let inner = checker.ascii_one_type();
            return Self {
                inner: Some(inner),
                key: None,
                value: None,
                receiver_ty: Some(resolved),
            };
        }
        match checker.types.get(resolved) {
            Type::Array(inner) | Type::BoundedArray { element: inner, .. } => Self {
                inner: Some(*inner),
                key: None,
                value: None,
                receiver_ty: Some(resolved),
            },
            Type::Map(key, value) => Self {
                inner: None,
                key: Some(*key),
                value: Some(*value),
                receiver_ty: Some(resolved),
            },
            Type::Set(inner) => Self {
                inner: Some(*inner),
                key: None,
                value: None,
                receiver_ty: Some(resolved),
            },
            Type::Tensor(inner, _) | Type::Sparsa(inner, _) => Self {
                inner: Some(*inner),
                key: None,
                value: None,
                receiver_ty: Some(resolved),
            },
            Type::Matrix(inner, _) => Self {
                inner: Some(*inner),
                key: None,
                value: None,
                receiver_ty: Some(resolved),
            },
            Type::Vector(inner, _) => Self {
                inner: Some(*inner),
                key: None,
                value: None,
                receiver_ty: Some(resolved),
            },
            Type::Intervallum(bound) => Self {
                inner: Some(*bound),
                key: None,
                value: None,
                receiver_ty: Some(resolved),
            },
            Type::Atomic(inner) => Self {
                inner: Some(*inner),
                key: None,
                value: None,
                receiver_ty: Some(resolved),
            },
            // WHY: `octeti` unifies with `lista<numerus<u8>>` (DEFER-029). Its
            // element type for intrinsic param/return resolution is `numerus<u8>`.
            Type::Primitive(Primitive::Octeti) | Type::BoundedOcteti { .. } => {
                let u8 = checker
                    .types
                    .find_sized_numeric(Primitive::Numerus, NumericWidth::U8)
                    .unwrap_or_else(|| {
                        checker
                            .types
                            .sized_numeric(Primitive::Numerus, NumericWidth::U8)
                    });
                Self {
                    inner: Some(u8),
                    key: None,
                    value: None,
                    receiver_ty: Some(resolved),
                }
            }
            _ => Self {
                inner: None,
                key: None,
                value: None,
                receiver_ty: Some(resolved),
            },
        }
    }
}

/// Whether the receiver type can ever host compiler-owned intrinsics.
pub(super) fn receiver_supports_intrinsics(receiver_ty: &Type) -> bool {
    !receivers_for_type(receiver_ty).is_empty()
}

/// Type-check a promoted intrinsic method call when the receiver type matches.
///
/// Lookup is pack-authoritative (db54b442): under any non-la reader pack, a
/// localized surface spelling (e.g. en `contains`) is resolved through the
/// pack's family-scoped surface→canonical cache to the unchanged canonical
/// registry name (`continet`) before matching; the canonical Latin spelling
/// itself is a miss and a compile error (no Latin fallback, no registry
/// bypass). Under the la pack or no pack, lookup is registry-direct (canonical
/// Latin identity). When the canonical entry resolves, `name` is rewritten on
/// the HIR so lint and MIR lowering keep seeing canonical names without
/// threading the pack into them. A miss keeps the surface spelling (identity)
/// and today's miss behavior.
pub(super) fn check_intrinsic_method_call(
    checker: &mut TypeChecker<'_>,
    receiver: &HirExpression,
    receiver_ty: TypeId,
    name: &mut Symbol,
    args: &mut [HirCallArg],
    span: radix_lexer::Span,
) -> Option<TypeId> {
    let receiver_resolved = checker.resolve_inferred_type(receiver_ty);
    let (_, payload_ty) = checker.types.peel_access_marker(receiver_resolved);
    let resolved = checker.types.get(payload_ty);
    let method_name = checker.interner.map_or_else(
        || "method".to_owned(),
        |interner| interner.resolve(*name).to_owned(),
    );
    // Pack-authoritative lookup (db54b442): the active pack is the sole
    // authority for intrinsic spellings under non-la locales. Under a NON-la
    // pack, only spellings the pack declares as a surface for the receiver's
    // family resolve — through the family-scoped surface→canonical cache; the
    // canonical Latin spelling is a miss and a compile error (no registry
    // bypass, no Latin fallback). Under the la pack or no pack, lookup is
    // registry-direct: the spelling IS the canonical Latin identity.
    let mut effective_name = method_name.as_str();
    let mut entry = None;
    match checker.locale_pack {
        Some(pack) if pack.metadata.id != "la" => {
            if let Some(canonical) = receivers_for_type(resolved)
                .iter()
                .find_map(|family| pack.intrinsic_canonical(*family, method_name.as_str()))
            {
                if let Some(canonical_entry) =
                    lookup_method_for_type(resolved, canonical, args.len())
                {
                    entry = Some(canonical_entry);
                    effective_name = canonical;
                }
            }
            // WHY: under a non-la pack a spelling that resolves in the registry
            // but is not a declared pack surface is the canonical Latin name
            // used in the wrong locale — a compile error, not a Latin
            // fallback. Genuinely unknown methods (no pack row, no registry
            // entry) keep today's silent miss so unknown-method diagnostics
            // stay pack-agnostic.
            if entry.is_none()
                && lookup_method_for_type(resolved, method_name.as_str(), args.len()).is_some()
            {
                checker.undefined_member("unknown_method", span);
                return Some(checker.error_type);
            }
        }
        // la pack or no pack: registry-direct lookup (canonical Latin identity).
        _ => {
            entry = lookup_method_for_type(resolved, method_name.as_str(), args.len());
        }
    }
    // Rewrite the HIR method symbol to the canonical registry spelling so the
    // canonical name flows to lint and MIR lowering unchanged (they match the
    // registry by name and never see the pack). Only on a resolved entry: an
    // unresolvable surface keeps its spelling and today's miss behavior. The
    // canonical spelling was pre-interned by the pack-spelling registration
    // pass, so its symbol is recovered from the intern table (no mutable
    // interner needed here).
    if effective_name != method_name.as_str() {
        if let Some(interner) = checker.interner {
            if let Some(index) = interner.strings().iter().position(|s| s == effective_name) {
                #[allow(clippy::cast_possible_truncation)]
                let canonical_symbol = Symbol(index as u32);
                *name = canonical_symbol;
            }
        }
    }
    let entry = entry?;
    // WHY: on modular receivers `subtrahe` is the value-returning wrapping
    // twin of `subtracta` (the R3 width vector table), not the in-place
    // numerus mutation; both names wrap at the declared width.
    let entry = if matches!(resolved, Type::ModularWord(_)) && effective_name == "subtrahe" {
        lookup_method(IntrinsicReceiver::Numeric, "subtracta", args.len()).unwrap_or(entry)
    } else {
        entry
    };
    let bounded_appende = entry.faber_name == "appende"
        && matches!(
            resolved,
            Type::BoundedArray { .. }
                | Type::BoundedTextus { .. }
                | Type::BoundedAscii { .. }
                | Type::BoundedOcteti { .. }
        );
    let ctx = IntrinsicTypeContext::from_receiver_type(checker, payload_ty);

    if checker.expr_is_ref_view(receiver) && is_mutating_receiver(entry) {
        checker.error_issue(
            SemanticErrorKind::AssignToImmutableBorrow,
            crate::ISSUE_MUTATING_RECEIVER_THROUGH_REF,
            span,
        );
    }

    // WHY: tensor elementwise `subtrahe`/`multiplica` share names with numeric
    // imperativus ops but return `SameAsReceiver`; only `Vacuum` returns mutate.
    if is_numeric_imperativus_operator(effective_name)
        && matches!(entry.return_kind, IntrinsicReturnKind::Vacuum)
        && !checker.path_binding_is_mutable(receiver)
    {
        checker.error_issue(
            SemanticErrorKind::ImmutableAssignment,
            ISSUE_IMPERATIVUS_OPERATOR_METHOD_REQUIRES_VARIA_RECEIVER,
            span,
        );
        return Some(checker.error_type);
    }

    if is_numeric_bitwise_operator(effective_name) && checker.is_fractus(receiver_ty) {
        checker.invalid_operand_types(ISSUE_BITWISE_METHOD_REQUIRES_NUMERUS_RECEIVER, span);
        return Some(checker.error_type);
    }

    if !tensor_numeric_element_allowed(checker, entry, ctx.inner, span) {
        return Some(checker.error_type);
    }

    if !matrix_numeric_element_allowed(checker, entry, ctx.inner, span) {
        return Some(checker.error_type);
    }

    if matches!(
        entry.receiver,
        radix_intrinsics::IntrinsicReceiver::Intervallum
    ) && entry.faber_name == "longitudo"
    {
        if let Some(bound) = ctx.inner {
            if !checker.check_intervallum_longitudo_bound(bound, span) {
                return Some(checker.error_type);
            }
        }
    }

    // Matrix `applica`: matrix[R,C] × vector[C] → vector[R] (CPU register subset).
    if matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Matrix)
        && entry.faber_name == "applica"
    {
        return Some(check_matrix_applica(checker, receiver_ty, args, span));
    }

    match entry.return_kind {
        IntrinsicReturnKind::MapperArray => {
            let Some(inner) = ctx.inner else {
                return Some(checker.error_type);
            };
            let mapped = checker.fresh_infer();
            let mapper_ty = checker.types.function(FuncSig {
                type_params: Vec::new(),
                type_param_constraints: Vec::new(),
                params: vec![ParamType {
                    ty: inner,
                    mode: SemanticParamMode::Owned,
                    optional: false,
                }],
                ret: mapped,
                err: None,
                is_async: false,
                is_generator: false,
            });
            let sig = FuncSig {
                type_params: Vec::new(),
                type_param_constraints: Vec::new(),
                params: vec![ParamType {
                    ty: mapper_ty,
                    mode: SemanticParamMode::Owned,
                    optional: false,
                }],
                ret: mapped,
                err: None,
                is_async: false,
                is_generator: false,
            };
            checker.check_call_args(&sig, args, span);
            let ret = checker.resolve_inferred_type(mapped);
            Some(checker.types.array(ret))
        }
        IntrinsicReturnKind::ReducerInfer => {
            let Some(inner) = ctx.inner else {
                return Some(checker.error_type);
            };
            let acc = checker.fresh_infer();
            let reducer_ty = checker.types.function(FuncSig {
                type_params: Vec::new(),
                type_param_constraints: Vec::new(),
                params: vec![
                    ParamType {
                        ty: acc,
                        mode: SemanticParamMode::Owned,
                        optional: false,
                    },
                    ParamType {
                        ty: inner,
                        mode: SemanticParamMode::Owned,
                        optional: false,
                    },
                ],
                ret: acc,
                err: None,
                is_async: false,
                is_generator: false,
            });
            let sig = FuncSig {
                type_params: Vec::new(),
                type_param_constraints: Vec::new(),
                params: vec![
                    ParamType {
                        ty: reducer_ty,
                        mode: SemanticParamMode::Owned,
                        optional: false,
                    },
                    ParamType {
                        ty: acc,
                        mode: SemanticParamMode::Owned,
                        optional: false,
                    },
                ],
                ret: acc,
                err: None,
                is_async: false,
                is_generator: false,
            };
            checker.check_call_args(&sig, args, span);
            Some(checker.resolve_inferred_type(acc))
        }
        _ => {
            let params = resolve_param_types(checker, entry, &ctx);
            let ret = resolve_return_type(checker, entry, &ctx, span);
            // WHY: tensor `NumerusArray` slots accept `lista<integer>` values
            // that fit the runtime i64 boundary (DEFER-064) so GPU/KIR source
            // can pass `lista<u32>` index vectors without a bare-`numerus`
            // detour. The generic unify path forces
            // `lista<numerus>` and rejects sized-integer lists under the v1
            // widening lattice; route tensor calls through a structural checker
            // instead of widening that lattice globally.
            let result = if matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Tensor)
                || matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Sparsa)
            {
                check_tensor_method_args(checker, entry, &params, args, ret, span)
            } else if bounded_appende {
                // D6: bounded `appende` is a typed recoverable alternate exit.
                // Unbounded `lista<T>.appende` stays vacuum with no error channel.
                let err = checker.textus_type();
                let sig = FuncSig {
                    type_params: Vec::new(),
                    type_param_constraints: Vec::new(),
                    params,
                    ret,
                    err: Some(err),
                    is_async: false,
                    is_generator: false,
                };
                checker.check_instantiated_call(&sig, &[], args, span, None)
            } else {
                checker.check_known_method_args(args, params, ret, span)
            };
            if matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Tensor) {
                super::tensor_index::apply_tensor_intrinsic_shape_constraints(
                    checker,
                    entry.faber_name,
                    receiver_ty,
                    result,
                    args,
                    span,
                );
            }
            Some(result)
        }
    }
}

/// Deferred tensor arithmetic intrinsics require a numeric element type.
fn tensor_numeric_element_allowed(
    checker: &mut TypeChecker<'_>,
    entry: &MethodIntrinsic,
    inner: Option<TypeId>,
    span: radix_lexer::Span,
) -> bool {
    let requires_numeric = matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Sparsa)
        || (matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Tensor)
            && crate::numeric_width::tensor_method_requires_numeric_element(entry.faber_name));
    if !requires_numeric {
        return true;
    }
    let Some(inner) = inner else {
        return false;
    };
    if crate::numeric_width::is_tensor_numeric_element(checker.types, inner) {
        return true;
    }
    checker.invalid_operand_types(ISSUE_TENSOR_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED, span);
    false
}

fn matrix_numeric_element_allowed(
    checker: &mut TypeChecker<'_>,
    entry: &MethodIntrinsic,
    inner: Option<TypeId>,
    span: radix_lexer::Span,
) -> bool {
    if !matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Matrix) {
        return true;
    }
    let Some(inner) = inner else {
        return false;
    };
    if crate::numeric_width::is_tensor_numeric_element(checker.types, inner) {
        return true;
    }
    checker.invalid_operand_types(ISSUE_MATRIX_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED, span);
    false
}

fn resolve_param_types(
    checker: &mut TypeChecker<'_>,
    entry: &MethodIntrinsic,
    ctx: &IntrinsicTypeContext,
) -> Vec<ParamType> {
    entry
        .params
        .iter()
        .map(|kind| ParamType {
            ty: resolve_param_kind(checker, *kind, ctx),
            mode: SemanticParamMode::Owned,
            optional: false,
        })
        .collect()
}

fn resolve_param_kind(
    checker: &mut TypeChecker<'_>,
    kind: IntrinsicParamKind,
    ctx: &IntrinsicTypeContext,
) -> TypeId {
    match kind {
        IntrinsicParamKind::Inner => ctx.inner.or(ctx.receiver_ty).unwrap_or(checker.error_type),
        IntrinsicParamKind::AtomicElement => ctx.inner.unwrap_or(checker.error_type),
        IntrinsicParamKind::Key => ctx.key.unwrap_or(checker.error_type),
        IntrinsicParamKind::Value => ctx.value.unwrap_or(checker.error_type),
        IntrinsicParamKind::NumerusIndex => checker.numerus_type(),
        IntrinsicParamKind::NumerusArray => {
            let numerus = checker.numerus_type();
            checker.types.array(numerus)
        }
        IntrinsicParamKind::ElementArray => {
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            checker.types.array(inner)
        }
        IntrinsicParamKind::Textus => checker.textus_type(),
        IntrinsicParamKind::SetInner => {
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            checker.types.set(inner)
        }
        IntrinsicParamKind::Predicate => {
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            let bool_ty = checker.bool_type();
            checker.types.function(FuncSig {
                type_params: Vec::new(),
                type_param_constraints: Vec::new(),
                params: vec![ParamType {
                    ty: inner,
                    mode: SemanticParamMode::Owned,
                    optional: false,
                }],
                ret: bool_ty,
                err: None,
                is_async: false,
                is_generator: false,
            })
        }
        IntrinsicParamKind::Mapper => {
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            let ret = checker.fresh_infer();
            checker.types.function(FuncSig {
                type_params: Vec::new(),
                type_param_constraints: Vec::new(),
                params: vec![ParamType {
                    ty: inner,
                    mode: SemanticParamMode::Owned,
                    optional: false,
                }],
                ret,
                err: None,
                is_async: false,
                is_generator: false,
            })
        }
        IntrinsicParamKind::ReceiverSelf => ctx.receiver_ty.unwrap_or(checker.error_type),
        IntrinsicParamKind::TensorPeer => {
            // WHY: broadcast elementwise operands share the receiver's element
            // type (strict) but carry an independent shape hole so the argument
            // shape is not forced equal to the receiver shape. The broadcast
            // merge forms the result shape in `apply_tensor_intrinsic_shape_constraints`.
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            let shape = checker.fresh_index_hole();
            checker.types.tensor_with_shape(inner, shape)
        }
        IntrinsicParamKind::TensorMatmulPeer => {
            // WHY: matmul peer operand shares the receiver's element type (strict)
            // but carries an independent shape hole. The contraction shape policy
            // in `constrain_matmul_result_shape` enforces rank-2 + inner-dim match
            // and produces a concrete [M,N] result shape, not a hole.
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            let shape = checker.fresh_index_hole();
            checker.types.tensor_with_shape(inner, shape)
        }
        IntrinsicParamKind::Reducer | IntrinsicParamKind::Accumulator => checker.error_type,
    }
}

fn resolve_return_type(
    checker: &mut TypeChecker<'_>,
    entry: &MethodIntrinsic,
    ctx: &IntrinsicTypeContext,
    span: radix_lexer::Span,
) -> TypeId {
    match entry.return_kind {
        IntrinsicReturnKind::Vacuum => checker.vacuum_type(),
        IntrinsicReturnKind::Bool => checker.bool_type(),
        IntrinsicReturnKind::Numerus => checker.numerus_type(),
        IntrinsicReturnKind::Textus => checker.textus_type(),
        IntrinsicReturnKind::NumericInner => {
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            if !checker.is_numeric(inner) {
                checker.invalid_operand_types(ISSUE_NUMERIC_LISTA_ELEMENTS_REQUIRED, span);
                return checker.error_type;
            }
            inner
        }
        IntrinsicReturnKind::OptionInner => {
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            checker.types.option(inner)
        }
        IntrinsicReturnKind::OptionValue => {
            let Some(value) = ctx.value else {
                return checker.error_type;
            };
            checker.types.option(value)
        }
        IntrinsicReturnKind::ArrayInner => {
            if matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Tensor)
                || matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Sparsa)
            {
                if entry.faber_name == "magnitudines" {
                    let numerus = checker.numerus_type();
                    return checker.types.array(numerus);
                }
                let Some(inner) = ctx.inner else {
                    return checker.error_type;
                };
                return checker.types.array(inner);
            }
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            checker.types.array(inner)
        }
        IntrinsicReturnKind::ArrayKey => {
            let Some(key) = ctx.key else {
                return checker.error_type;
            };
            checker.types.array(key)
        }
        IntrinsicReturnKind::ArrayValue => {
            let Some(value) = ctx.value else {
                return checker.error_type;
            };
            checker.types.array(value)
        }
        IntrinsicReturnKind::SetInner => {
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            checker.types.set(inner)
        }
        IntrinsicReturnKind::SameAsReceiver => {
            if matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Tensor)
                && super::tensor_index::tensor_intrinsic_changes_shape(entry.faber_name)
            {
                let Some(inner) = ctx.inner else {
                    return checker.error_type;
                };
                return super::tensor_index::tensor_with_fresh_shape(checker, inner);
            }
            ctx.receiver_ty.unwrap_or(checker.error_type)
        }
        IntrinsicReturnKind::OptionReceiver => {
            let Some(receiver_ty) = ctx.receiver_ty else {
                return checker.error_type;
            };
            checker.types.option(receiver_ty)
        }
        IntrinsicReturnKind::NumerusFromFractus => checker.numerus_type(),
        IntrinsicReturnKind::FractusFromElement => {
            // WHY: tensor `media` reduces to a float to dodge integer division;
            // the width follows the goal's dtype table (f64 for numerus, matching
            // width for fractus). Numeric element is pre-gated by
            // `tensor_method_requires_numeric_element`, so a missing width is a
            // typecheck contract violation worth surfacing.
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            if let Some(width) = crate::numeric_width::tensor_media_width(checker.types, inner) {
                checker.types.sized_numeric(Primitive::Fractus, width)
            } else {
                checker
                    .invalid_operand_types(ISSUE_TENSOR_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED, span);
                checker.error_type
            }
        }
        IntrinsicReturnKind::TextusArray => {
            let textus = checker.textus_type();
            checker.types.array(textus)
        }
        IntrinsicReturnKind::CursorAsciiOne => {
            let one = checker.ascii_one_type();
            checker.types.cursor(one)
        }
        IntrinsicReturnKind::Inner => {
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            // WHY: sparsa `accipe` returns the element type directly (zero for
            // absent entries), unlike tensor `accipe` which returns `T ∪ nihil`.
            inner
        }
        IntrinsicReturnKind::DenseFromSparsa => {
            // `sparsa.densata()` materializes to `tensor<T, Figura>` with the
            // same element type and shape as the receiver sparsa.
            if let Some(ty) = ctx.receiver_ty.and_then(|rt| {
                let resolved = checker.resolve_inferred_type(rt);
                match checker.types.get(resolved) {
                    Type::Sparsa(inner, shape) => {
                        Some(checker.types.tensor_with_shape(*inner, *shape))
                    }
                    _ => None,
                }
            }) {
                ty
            } else {
                checker.invalid_operand_types(ISSUE_DENSATA_REQUIRES_SPARSA_RECEIVER, span);
                checker.error_type
            }
        }
        IntrinsicReturnKind::GatherShape => {
            // CCI-1: `gather` returns `tensor<same element, [T, last-of-receiver]>`
            // where T is the id-vector length. The typecheck admits a fresh
            // shape hole (the args are already checked as an i64-fit
            // `lista<integer>`); the concrete `[T, last-of-receiver]` shape
            // fact is carried by the HIR→AIR `validate_gather_contract` static
            // contract and the MIR `TensorGather` op.
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            super::tensor_index::tensor_with_fresh_shape(checker, inner)
        }
        IntrinsicReturnKind::MapperArray | IntrinsicReturnKind::ReducerInfer => checker.error_type,
    }
}

/// Typecheck `matrix.applica(vector)`: peer width must equal matrix columns;
/// result is a vector of width = matrix rows with the same element type.
fn check_matrix_applica(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    args: &mut [HirCallArg],
    span: radix_lexer::Span,
) -> TypeId {
    let resolved = checker.resolve_inferred_type(receiver_ty);
    let Type::Matrix(element_ty, shape) = checker.types.get(resolved).clone() else {
        return checker.error_type;
    };
    let Some((rows, columns)) = crate::literal_matrix_shape(shape, checker.types) else {
        checker.invalid_operand_types("matrix_applica_requires_literal_shape", span);
        return checker.error_type;
    };
    if rows > 4 || columns > 4 {
        checker.invalid_operand_types("matrix_register_exceeds_cpu_subset", span);
        return checker.error_type;
    }
    if args.len() != 1 {
        checker.wrong_arity("wrong_number_of_arguments", span);
        return checker.error_type;
    }
    let width_id = checker
        .types
        .intern_index(crate::IndexExpr::Literal(u64::from(columns)));
    let peer_ty = checker.types.vector_with_width(element_ty, width_id);
    let arg_ty = checker.check_expr_with_expected(&mut args[0].expr, Some(peer_ty));
    checker.unify_assign_issue(arg_ty, peer_ty, args[0].span, ISSUE_ARGUMENT_TYPE_MISMATCH);
    let result_width = checker
        .types
        .intern_index(crate::IndexExpr::Literal(u64::from(rows)));
    checker.types.vector_with_width(element_ty, result_width)
}

/// Validate tensor method call arguments under the DEFER-064 index-vector policy.
///
/// `NumerusArray` slots accept `lista<integer>` values that fit the runtime i64
/// boundary (validated structurally instead of forced to `lista<numerus>`); all
/// other slots use the normal expected-type unify path. Arity is checked once.
/// Returns the intrinsic's declared return type regardless of per-argument
/// diagnostics so downstream shape constraints still see the published result
/// shape.
fn check_tensor_method_args(
    checker: &mut TypeChecker<'_>,
    entry: &MethodIntrinsic,
    params: &[ParamType],
    args: &mut [HirCallArg],
    ret: TypeId,
    span: radix_lexer::Span,
) -> TypeId {
    let required = params.iter().filter(|param| !param.optional).count();
    if args.len() < required || args.len() > params.len() {
        checker.wrong_arity("wrong_number_of_arguments", span);
    }
    for ((arg, kind), param) in args.iter_mut().zip(entry.params.iter()).zip(params.iter()) {
        if *kind == IntrinsicParamKind::NumerusArray {
            check_integer_index_vector_arg(checker, arg, span);
        } else {
            let arg_ty = checker.check_expr_with_expected(&mut arg.expr, Some(param.ty));
            checker.unify_assign_issue(arg_ty, param.ty, arg.span, ISSUE_ARGUMENT_TYPE_MISMATCH);
        }
    }
    ret
}

/// Typecheck a tensor index-vector argument and validate it is an i64-fit
/// `lista<integer>`.
///
/// The argument is checked without an expected type so inline literals like
/// `[`id`]` infer their natural element width (`lista<u32>`, …) rather than being
/// coerced toward `lista<numerus>`. Accepts integer element widths that fit the
/// runtime `i64` boundary; rejects `u64`, non-integer lists, and non-list types
/// with a deliberate diagnostic.
///
/// An **empty** literal `[]` is admitted as `lista<numerus>`: an empty vector
/// has no elements, so no element can violate the i64 boundary. Its element
/// type resolves from the `NumerusArray` index position (the slot's canonical
/// `lista<numerus>` param type) instead of staying an unresolved inference
/// variable — the empty case is part of the frozen gather contract
/// (`gather([])` → `[0,D]`).
fn check_integer_index_vector_arg(
    checker: &mut TypeChecker<'_>,
    arg: &mut HirCallArg,
    span: radix_lexer::Span,
) {
    // WHY (empty literal): without an expected type an empty literal is typed
    // `lista<?infer>` (`check_array`), and the structural check below must
    // reject an unresolved element type — so the empty case could never be
    // spelled. The empty vector is compatible with the shared `NumerusArray`
    // boundary (no elements to exceed it), so check `[]` against the slot's
    // canonical `lista<numerus>`: the element type resolves to `numerus`
    // (`i64`) from the index position. Non-empty literals stay on the
    // expected-free path so `[id]` with a `u32` id keeps its natural width.
    let arg_ty = if matches!(&arg.expr.kind, HirExpressionKind::Array(elements) if elements.is_empty())
    {
        let numerus = checker.numerus_type();
        let canonical = checker.types.array(numerus);
        checker.check_expr_with_expected(&mut arg.expr, Some(canonical))
    } else {
        checker.check_expr(&mut arg.expr)
    };
    if !crate::numeric_width::is_integer_index_vector(checker.types, arg_ty) {
        checker.invalid_operand_types(ISSUE_TENSOR_INDEX_VECTOR_INTEGER_ELEMENTS, span);
    }
}
