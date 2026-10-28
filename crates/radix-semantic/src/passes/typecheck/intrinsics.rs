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
use crate::tensor_type_directed::{complete_tensor_type, is_tensor_type};
use crate::{
    FuncSig, GenericParamDescriptor, IndexExpr, IndexId, NumericWidth, ParamType, Primitive,
    SemanticErrorKind, SemanticGenericParamDefault, SemanticParamMode, Type, TypeId,
    TypeParamConstraint,
};
use radix_hir::{HirArrayElement, HirCallArg, HirExpression, HirExpressionKind};
use radix_intrinsics::{
    CatalogGenericDefault, CatalogGenericParam, IntrinsicMirKind, IntrinsicParamKind,
    IntrinsicReceiver, IntrinsicReturnKind, MethodIntrinsic, is_mutating_receiver,
    is_numeric_bitwise_operator, lookup_method, lookup_method_for_type, lookup_method_named,
    receivers_for_type,
};
use radix_lexer::Symbol;

const ISSUE_BITWISE_METHOD_REQUIRES_NUMERUS_RECEIVER: &str =
    "bitwise_method_requires_numerus_receiver";
const ISSUE_DENSATA_REQUIRES_SPARSA_RECEIVER: &str = "densata_requires_sparsa_receiver";
const ISSUE_ARGUMENT_TYPE_MISMATCH: &str = "argument_type_mismatch";
const ISSUE_NUMERIC_LISTA_ELEMENTS_REQUIRED: &str = "numeric_lista_elements_required";
/// The list join `extende` grows its receiver in place, so a fixed-capacity or
/// length-ranged list (which `appende` reports on `⇥`) is refused by name.
const ISSUE_EXTEND_REQUIRES_PLAIN_LIST: &str = "extend_requires_plain_list";
const ISSUE_ORDINA_KEY_NOT_ORDERED: &str = "ordina_key_not_ordered";
/// Need (ruling O2): the keyed `.sorted(key)` / `.ordinata(key)` is
/// retired in favour of the sort expression.
// FABER-2-SUNSET: delete with the retired-word diagnostics.
const ISSUE_SORTED_KEY_RETIRED: &str = "sorted_key_retired";
const ISSUE_TENSOR_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED: &str =
    "tensor_arithmetic_numeric_element_required";
/// K5: a lifted transcendental math function (`radix`, `exponentia`, ...)
/// over a non-float tensor element. The scalar function takes `fractus`
/// only, so an integer element crosses with an explicit `↦`.
const ISSUE_TENSOR_MATH_FLOAT_ELEMENT_REQUIRED: &str = "tensor_math_float_element_required";
const ISSUE_MATRIX_ARITHMETIC_NUMERIC_ELEMENT_REQUIRED: &str =
    "matrix_arithmetic_numeric_element_required";
pub(super) const ISSUE_TENSOR_INDEX_VECTOR_INTEGER_ELEMENTS: &str =
    "tensor_index_vector_integer_elements";
const ISSUE_TENSOR_MEDIA_DECIMAL: &str = "tensor_media_decimal_rejected";
const ISSUE_IMPERATIVUS_OPERATOR_METHOD_REQUIRES_VARIA_RECEIVER: &str =
    "imperativus_operator_method_requires_varia_receiver";
const ISSUE_MUTATING_RECEIVER_THROUGH_CONST: &str = "mutating_receiver_through_const";
const ISSUE_INTERVALLUM_VALUE_STEP_UNSUPPORTED: &str = "intervallum_value_step_unsupported";
const ISSUE_INTRINSIC_METHOD_TYPE_ARGS: &str = "intrinsic_method_type_args";
const ISSUE_TENSOR_FORMATA_ELEMENT_TYPE_MISMATCH: &str = "tensor_formata_element_type_mismatch";
const ISSUE_TENSOR_FORMATA_ELEMENT_COUNT_MISMATCH: &str = "tensor_formata_element_count_mismatch";
const ISSUE_TENSOR_TYPE_ARG_INCOMPLETE: &str = "tensor_type_arg_incomplete";
const ISSUE_TENSOR_SHIFT_NULLABLE_SOURCE_UNSUPPORTED: &str =
    "tensor_shift_nullable_source_unsupported";
pub(super) const ISSUE_TENSOR_EDGE_VIEW_REQUIRES_RESOLUTION: &str =
    "tensor_edge_view_requires_resolution";
const ISSUE_TENSOR_LIMES_REQUIRES_SHIFTED_VIEW: &str = "tensor_limes_requires_shifted_view";
const ISSUE_TENSOR_LIMES_POLICY_INVALID: &str = "tensor_limes_policy_invalid";
const ISSUE_TENSOR_SHIFT_OFFSET_RANK_MISMATCH: &str = "tensor_shift_offset_rank_mismatch";
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
            Type::Array(inner)
            | Type::BoundedArray { element: inner, .. }
            | Type::RangedArray { element: inner, .. }
            | Type::Queue(inner)
            | Type::BoundedQueue { element: inner, .. }
            | Type::Stack(inner)
            | Type::BoundedStack { element: inner, .. }
            | Type::Set(inner)
            | Type::Tensor(inner, _)
            | Type::Sparsa(inner, _)
            | Type::Matrix(inner, _)
            | Type::Vector(inner, _)
            | Type::Intervallum(inner, _)
            | Type::Atomic(inner) => Self {
                inner: Some(*inner),
                key: None,
                value: None,
                receiver_ty: Some(resolved),
            },
            Type::Map(key, value) | Type::Ledger(key, value) => Self {
                inner: None,
                key: Some(*key),
                value: Some(*value),
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

/// Intrinsic receiver families for a resolved receiver type.
///
/// A ledger (`tabulae<K, V>`) is the insertion-ordered twin of a map
/// (`tabula<K, V>`): it shares the map's `Tabula` registry rows, so it keeps
/// every method name and per-family locale word without a second table.
pub(super) fn receiver_families(receiver_ty: &Type) -> &'static [IntrinsicReceiver] {
    match receiver_ty {
        Type::Ledger(..) => &[IntrinsicReceiver::Tabula],
        _ => receivers_for_type(receiver_ty),
    }
}

/// Whether the receiver type can ever host compiler-owned intrinsics.
pub(super) fn receiver_supports_intrinsics(receiver_ty: &Type) -> bool {
    !receiver_families(receiver_ty).is_empty()
}

/// Intern catalog generic names into GPD descriptors.
///
/// Names are pre-interned during analysis setup. A missing symbol is a
/// programming error in that setup, not a caller-facing unknown-method miss.
pub(super) fn interned_catalog_generics(
    checker: &mut TypeChecker<'_>,
    generic: &[CatalogGenericParam],
) -> Vec<GenericParamDescriptor> {
    let mut out = Vec::with_capacity(generic.len());
    for param in generic {
        let name = interned_catalog_name(checker, param.name).unwrap_or(Symbol(0));
        let default = match param.default {
            Some(CatalogGenericDefault::Valor) => Some(SemanticGenericParamDefault::Typus(
                checker.types.primitive(Primitive::Valor),
            )),
            None => None,
        };
        out.push(GenericParamDescriptor {
            name,
            sort: param.sort,
            constraint: TypeParamConstraint::Any,
            default,
        });
    }
    out
}

fn lookup_intrinsic_row(
    receiver_ty: &Type,
    method: &str,
    arg_count: usize,
) -> Option<&'static MethodIntrinsic> {
    // A ledger dispatches through its map twin so the arity-checked primary
    // lookup sees the shared `Tabula` rows.
    let ledger_twin;
    let lookup_ty = if let Type::Ledger(key, value) = receiver_ty {
        ledger_twin = Type::Map(*key, *value);
        &ledger_twin
    } else {
        receiver_ty
    };
    lookup_method_for_type(lookup_ty, method, arg_count).or_else(|| {
        receiver_families(receiver_ty)
            .iter()
            .find_map(|family| lookup_method_named(*family, method))
            .filter(|entry| !entry.generic.is_empty())
    })
}

fn interned_catalog_name(checker: &TypeChecker<'_>, name: &str) -> Option<Symbol> {
    let interner = checker.interner?;
    interner.lookup(name)
}

/// Catalog-driven generic/value arity. Returns true when the call is rejected.
pub(super) fn reject_catalog_arity(
    checker: &mut TypeChecker<'_>,
    generic: &[CatalogGenericParam],
    arg_count: usize,
    missing_generic_issue: Option<&'static str>,
    excess_generic_issue: Option<&'static str>,
    value_arity_issue: Option<&'static str>,
    diagnostic_method: Option<&'static str>,
    explicit_type_args: &[TypeId],
    args: &mut [HirCallArg],
    span: radix_lexer::Span,
) -> bool {
    if arg_count == 0 && !args.is_empty() {
        let issue = value_arity_issue.unwrap_or("wrong_number_of_arguments");
        if value_arity_issue.is_some() {
            checker.invalid_operand_types(issue, span);
        } else {
            checker.wrong_arity(issue, span);
        }
        checker.typecheck_call_arg_exprs(args);
        return true;
    }
    let required = generic
        .iter()
        .filter(|param| param.default.is_none())
        .count();
    let max = generic.len();
    if explicit_type_args.len() < required {
        if let Some(issue) = missing_generic_issue {
            checker.invalid_operand_types(issue, span);
        } else {
            checker.wrong_arity("wrong_number_of_type_arguments", span);
        }
        checker.typecheck_call_arg_exprs(args);
        return true;
    }
    if explicit_type_args.len() > max {
        let issue = excess_generic_issue.unwrap_or("wrong_number_of_type_arguments");
        if let Some(method) = diagnostic_method {
            checker.invalid_operand_types_with_args(issue, span, &[("method", method.to_owned())]);
        } else if excess_generic_issue.is_some() {
            checker.invalid_operand_types(issue, span);
        } else {
            checker.wrong_arity(issue, span);
        }
        checker.typecheck_call_arg_exprs(args);
        return true;
    }
    false
}

/// Type-check a promoted intrinsic method call when the receiver type matches.
///
/// Whether `name` with `arg_count` arguments is a float intrinsic that returns
/// its receiver's own float type (`SameAsReceiver`: `cbrt`, `sqrt`, `abs`,
/// `sin`, ...), resolved through the reader pack exactly as
/// [`check_intrinsic_method_call`] resolves it. Pure: a miss reports nothing.
pub(super) fn is_float_same_as_receiver_method(
    checker: &mut TypeChecker<'_>,
    name: Symbol,
    arg_count: usize,
) -> bool {
    let Some(interner) = checker.interner else {
        return false;
    };
    let surface = interner.resolve(name).to_owned();
    let probe = checker.fractus_type();
    let probe = checker.types.get(probe);
    let canonical = match checker.locale_pack {
        Some(pack) if pack.metadata.id != "la" => receiver_families(probe)
            .iter()
            .find_map(|family| pack.intrinsic_canonical(*family, surface.as_str())),
        _ => Some(surface.as_str()),
    };
    canonical
        .and_then(|method| lookup_intrinsic_row(probe, method, arg_count))
        .is_some_and(|entry| matches!(entry.return_kind, IntrinsicReturnKind::SameAsReceiver))
}

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
    explicit_type_args: &[TypeId],
    args: &mut [HirCallArg],
    span: radix_lexer::Span,
) -> Option<TypeId> {
    // S0-G2: applied aliases are value-transparent — an alias receiver
    // dispatches on its substituted builtin carrier.
    let receiver_resolved = checker.value_carrier_type(receiver_ty);
    let (_, payload_ty) = checker.types.peel_access_marker(receiver_resolved);
    let method_name = checker.interner.map_or_else(
        || "method".to_owned(),
        |interner| interner.resolve(*name).to_owned(),
    );
    let mut effective_name = method_name.as_str();
    let mut entry = None;
    let is_modular = matches!(checker.types.get(payload_ty), Type::ModularWord(_));
    {
        let resolved = checker.types.get(payload_ty);
        match checker.locale_pack {
            Some(pack) if pack.metadata.id != "la" => {
                if let Some(canonical) = receiver_families(resolved)
                    .iter()
                    .find_map(|family| pack.intrinsic_canonical(*family, method_name.as_str()))
                    && let Some(canonical_entry) =
                        lookup_intrinsic_row(resolved, canonical, args.len())
                {
                    entry = Some(canonical_entry);
                    effective_name = canonical;
                }
                // WHY: under a non-la pack a spelling that resolves in the registry
                // but is not a declared pack surface is the canonical Latin name
                // used in the wrong locale — a compile error, not a Latin
                // fallback. Genuinely unknown methods (no pack row, no registry
                // entry) keep today's silent miss so unknown-method diagnostics
                // stay pack-agnostic.
                if entry.is_none()
                    && lookup_intrinsic_row(resolved, method_name.as_str(), args.len()).is_some()
                {
                    checker.undefined_member("unknown_method", span);
                    return Some(checker.error_type);
                }
            }
            // la pack or no pack: registry-direct lookup (canonical Latin identity).
            _ => {
                entry = lookup_intrinsic_row(resolved, method_name.as_str(), args.len());
            }
        }
    }
    // Rewrite the HIR method symbol to the canonical registry spelling so the
    // canonical name flows to lint and MIR lowering unchanged (they match the
    // registry by name and never see the pack). Only on a resolved entry: an
    // unresolvable surface keeps its spelling and today's miss behavior. The
    // canonical spelling was pre-interned by the pack-spelling registration
    // pass, so its symbol is recovered from the intern table (no mutable
    // interner needed here).
    if effective_name != method_name.as_str()
        && let Some(interner) = checker.interner
        && let Some(canonical_symbol) = interner.lookup(effective_name)
    {
        *name = canonical_symbol;
    }
    let entry = entry?;
    // WHY: on modular receivers `subtrahe` is the value-returning wrapping
    // twin of `subtracta` (the R3 width vector table), not the in-place
    // numerus mutation; both names wrap at the declared width.
    let entry = if is_modular && effective_name == "subtrahe" {
        lookup_method(IntrinsicReceiver::Numeric, "subtracta", args.len()).unwrap_or(entry)
    } else {
        entry
    };
    if entry.generic.is_empty() && !explicit_type_args.is_empty() {
        checker.invalid_operand_types(ISSUE_INTRINSIC_METHOD_TYPE_ARGS, span);
        checker.typecheck_call_arg_exprs(args);
        return Some(checker.error_type);
    }
    if !entry.generic.is_empty()
        && reject_catalog_arity(
            checker,
            entry.generic,
            entry.arg_count,
            entry.missing_generic_issue,
            entry.excess_generic_issue,
            entry.value_arity_issue,
            None,
            explicit_type_args,
            args,
            span,
        )
    {
        return Some(checker.error_type);
    }
    if matches!(entry.mir_kind, IntrinsicMirKind::TensorLimes) {
        return Some(check_tensor_limes(
            checker,
            receiver,
            receiver_ty,
            explicit_type_args,
            args,
            span,
        ));
    }
    if matches!(
        entry.mir_kind,
        IntrinsicMirKind::Collection(radix_types::MirCollectionOp::TensorShift)
    ) {
        let ctx = IntrinsicTypeContext::from_receiver_type(checker, payload_ty);
        let params = resolve_param_types(checker, entry, &ctx);
        let ret = resolve_return_type(checker, entry, &ctx, span);
        check_tensor_method_args(checker, entry, &params, args, ret, span);
        return Some(check_tensor_shift(
            checker,
            receiver,
            receiver_ty,
            args,
            span,
        ));
    }
    if checker
        .tensor_edge_policies
        .shifted_expressions
        .contains(&receiver.id)
    {
        checker.invalid_operand_types(ISSUE_TENSOR_EDGE_VIEW_REQUIRES_RESOLUTION, span);
        checker.typecheck_call_arg_exprs(args);
        return Some(checker.error_type);
    }
    let resolved = checker.types.get(payload_ty);
    // Ruling 2026-10-17 (bounded `lista<T, magnitudo N>`): the sink operator `⇜` dies on
    // overflow, its method form reports. `appende` and `immerge` are two names
    // for one append op on a bounded receiver (the `immerge` registry row lowers
    // to `collection Append`), so both carry the `⇥ textus` alternate exit.
    // Unbounded `lista<T>` / `set<T>` stay infallible.
    let bounded_appende = matches!(entry.faber_name, "appende" | "immerge")
        && matches!(
            resolved,
            Type::BoundedArray { .. }
                // A length range with an upper end reports a push past it on
                // `⇥`; `lo…∞` has no upper end and stays infallible.
                | Type::RangedArray { max: Some(_), .. }
                | Type::BoundedQueue { .. }
                | Type::BoundedStack { .. }
                | Type::BoundedTextus { .. }
                | Type::BoundedAscii { .. }
                | Type::BoundedOcteti { .. }
        );
    // A length range's low end is at least one, so `decapita` / `detrahe`
    // (the removals) would drop a list at its minimum below it. Like the push
    // past the maximum, the method form reports that on `⇥ textus`.
    let ranged_removal = matches!(entry.faber_name, "decapita" | "detrahe")
        && matches!(resolved, Type::RangedArray { .. });
    let bounded_appende = bounded_appende || ranged_removal;
    let plain_list_receiver = matches!(
        resolved,
        Type::Array(_) | Type::Primitive(Primitive::Octeti)
    );
    let ctx = IntrinsicTypeContext::from_receiver_type(checker, payload_ty);

    if matches!(entry.mir_kind, IntrinsicMirKind::ListExtend) && !plain_list_receiver {
        checker.invalid_operand_types(ISSUE_EXTEND_REQUIRES_PLAIN_LIST, span);
        checker.typecheck_call_arg_exprs(args);
        return Some(checker.error_type);
    }

    // The extrema twins are elementwise clamp methods on numeric lists. Keep
    // the element gate here with the method registry path so infix sugar and
    // an explicit `.maximus(n)` / `.minimus(n)` call cannot diverge.
    if matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Lista)
        && matches!(entry.faber_name, "maximus" | "minimus")
        && !ctx.inner.is_some_and(|inner| checker.is_numeric(inner))
    {
        checker.invalid_operand_types(ISSUE_NUMERIC_LISTA_ELEMENTS_REQUIRED, span);
        return Some(checker.error_type);
    }

    // `value!.method()` writes through `value`: look through the non-null
    // assertion so the mandatory spelling for an optional receiver meets the
    // same gates as the direct call on a plain binding.
    let place = TypeChecker::method_receiver_place(receiver);

    checker.check_loop_shape_method(place, entry, &method_name, span);

    if checker.expr_is_ref_view(place) && is_mutating_receiver(entry) {
        checker.error_issue(
            SemanticErrorKind::AssignToImmutableBorrow,
            crate::ISSUE_MUTATING_RECEIVER_THROUGH_REF,
            span,
        );
        // WHY: the binding-modifier gate below would report the same fault
        // again as SEM020 for this site.
        checker.typecheck_call_arg_exprs(args);
        return Some(checker.error_type);
    }

    // A mutating intrinsic (registry `is_mutating_receiver`: Vacuum returns,
    // in-place collection ops, atomic exchange) rewrites its receiver's
    // contents in place, so a direct path to an immutable binding is rejected
    // like a reassignment. Tensor elementwise `subtracta`/`multiplicata` share
    // names with the numeric perfect forms but return `SameAsReceiver`, so
    // the registry predicate alone is the gate. Non-path receivers keep the
    // shallow mutability-engine D-1 behavior: a `fixum` field's contents stay
    // mutable through calls.
    if let HirExpressionKind::Path(binding_id) = &place.kind
        && is_mutating_receiver(entry)
        && !checker.path_binding_is_mutable(place)
    {
        let binding = checker
            .binding_names
            .get(binding_id)
            .or_else(|| {
                checker
                    .resolver
                    .get_symbol(*binding_id)
                    .map(|symbol| &symbol.name)
            })
            .and_then(|symbol| checker.interner.map(|interner| interner.resolve(*symbol)))
            .unwrap_or("value")
            .to_owned();
        checker.error_issue_with_args(
            SemanticErrorKind::ImmutableAssignment,
            ISSUE_IMPERATIVUS_OPERATOR_METHOD_REQUIRES_VARIA_RECEIVER,
            span,
            &[("binding", binding), ("method", method_name.clone())],
        );
        return Some(checker.error_type);
    }

    // `const` is deep: a mutating intrinsic on a field or
    // element reached through a const local rewrites state reachable from that
    // name, exactly what a `ref` view refuses.
    if is_mutating_receiver(entry)
        && !checker.expr_is_ref_view(place)
        && let Some(root) = const_local_projection_root(checker, place)
    {
        let binding = checker
            .binding_names
            .get(&root)
            .and_then(|symbol| checker.interner.map(|interner| interner.resolve(*symbol)))
            .unwrap_or("value")
            .to_owned();
        checker.error_issue_with_args(
            SemanticErrorKind::ImmutableAssignment,
            ISSUE_MUTATING_RECEIVER_THROUGH_CONST,
            span,
            &[("binding", binding), ("method", method_name.clone())],
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

    if !tensor_float_element_allowed(checker, entry, ctx.inner, span) {
        return Some(checker.error_type);
    }

    if !matrix_numeric_element_allowed(checker, entry, ctx.inner, span) {
        return Some(checker.error_type);
    }

    if matches!(
        entry.receiver,
        radix_intrinsics::IntrinsicReceiver::Intervallum
    ) && entry.faber_name == "longitudo"
        && let Some(bound) = ctx.inner
        && !checker.check_intervallum_longitudo_bound(bound, span)
    {
        return Some(checker.error_type);
    }

    // `coercere`, `inter` and `union` do not yet have a ruled meaning on a
    // stepped receiver: the runtime helpers publish a step-one result, which
    // would silently drop the stride. Refuse them by name. The query helpers
    // `continet` and `longitudo` read the stride and stay accepted.
    if matches!(
        entry.receiver,
        radix_intrinsics::IntrinsicReceiver::Intervallum
    ) && matches!(entry.faber_name, "coercere" | "inter" | "union")
        && checker.intervallum_type_id(receiver_ty).is_some()
        && !checker.intervallum_step_is_one(receiver_ty)
    {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_INTERVALLUM_VALUE_STEP_UNSUPPORTED,
            span,
        );
        return Some(checker.error_type);
    }

    // Matrix `applica`: matrix[R,C] × vector[C] → vector[R] (CPU register subset).
    if matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Matrix)
        && entry.faber_name == "applica"
    {
        return Some(check_matrix_applica(checker, receiver_ty, args, span));
    }

    // Vector `multiplicata`: elementwise multiply by a same-shape peer or by a
    // scalar of the element type (S0-G8 scalar splat — the scalar operand
    // broadcasts across the lanes; the result keeps the receiver shape).
    if matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Vector)
        && entry.faber_name == "multiplicata"
    {
        return Some(check_vector_multiplicata(checker, receiver_ty, args, span));
    }

    // Tensor, vector and matrix `maximus`/`minimus` (`⤒`/`⤓`): the elementwise
    // extremum against a scalar of the element type or a same-shape peer (K4
    // lift; the same rule applies to the registers).
    if matches!(
        entry.receiver,
        radix_intrinsics::IntrinsicReceiver::Tensor
            | radix_intrinsics::IntrinsicReceiver::Vector
            | radix_intrinsics::IntrinsicReceiver::Matrix
    ) && matches!(entry.faber_name, "maximus" | "minimus")
    {
        return Some(checker.check_tensor_extremum(receiver_ty, args, span));
    }

    // Matrix `transpone`: [R, C] → [C, R] — the result shape is swapped, so
    // `SameAsReceiver` cannot publish it. The registry owns the zero-arity
    // contract; this helper only swaps the shape terms.
    if matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Matrix)
        && entry.faber_name == "transpone"
    {
        return Some(check_matrix_transpone(checker, receiver_ty));
    }

    if matches!(entry.mir_kind, IntrinsicMirKind::HigherOrderSortBy) {
        return Some(check_sort_by_key(checker, entry, ctx.inner, args, span));
    }

    if matches!(entry.mir_kind, IntrinsicMirKind::ListExtend) {
        return Some(check_list_extend(checker, ctx.inner, args, span));
    }

    // RX-5 (RD-5): a closure in the replacement slot selects the closure form;
    // any other argument is checked as the literal `textus` replacement below.
    if entry.params.contains(&IntrinsicParamKind::TextusOrReplacer)
        && args
            .get(1)
            .is_some_and(|arg| matches!(arg.expr.kind, HirExpressionKind::Clausura(..)))
    {
        return Some(check_regex_replace_closure(checker, args, span));
    }

    match entry.return_kind {
        IntrinsicReturnKind::MapperArray | IntrinsicReturnKind::MapperSet => {
            let Some(inner) = ctx.inner else {
                return Some(checker.error_type);
            };
            let mapped = checker.fresh_infer();
            let mapper_ty = checker.types.function(simple_sig(vec![inner], mapped));
            let sig = simple_sig(vec![mapper_ty], mapped);
            checker.check_call_args(&sig, args, span);
            let ret = checker.resolve_inferred_type(mapped);
            // F2-SET-MAP admissibility gate: `types.set` IS the existing
            // set-element admissibility relation (total over element types —
            // the same constructor every other set-construction site rides).
            // `set<U>` publishes only through it; no coercion, no new rule.
            match entry.return_kind {
                IntrinsicReturnKind::MapperSet => Some(checker.types.set(ret)),
                _ => Some(checker.types.array(ret)),
            }
        }
        IntrinsicReturnKind::ReducerInfer | IntrinsicReturnKind::ReducerArray => {
            let Some(inner) = ctx.inner else {
                return Some(checker.error_type);
            };
            let acc = checker.fresh_infer();
            let reducer_ty = checker.types.function(simple_sig(vec![acc, inner], acc));
            let sig = simple_sig(vec![reducer_ty, acc], acc);
            checker.check_call_args(&sig, args, span);
            let resolved = checker.resolve_inferred_type(acc);
            if matches!(entry.return_kind, IntrinsicReturnKind::ReducerArray) {
                Some(checker.types.array(resolved))
            } else {
                Some(resolved)
            }
        }
        _ => Some(check_registered_intrinsic(
            checker,
            entry,
            ctx,
            bounded_appende,
            receiver,
            receiver_ty,
            explicit_type_args,
            args,
            span,
        )),
    }
}

/// Resolve and check one registered intrinsic row whose params and return
/// shape are published from the receiver's element types (every row except
/// the mapper and reducer rows routed to their own signatures above).
fn check_registered_intrinsic(
    checker: &mut TypeChecker<'_>,
    entry: &MethodIntrinsic,
    ctx: IntrinsicTypeContext,
    bounded_appende: bool,
    receiver: &HirExpression,
    receiver_ty: TypeId,
    explicit_type_args: &[TypeId],
    args: &mut [HirCallArg],
    span: radix_lexer::Span,
) -> TypeId {
    let params = resolve_param_types(checker, entry, &ctx);
    let ret = resolve_return_type(checker, entry, &ctx, span);
    // WHY: tensor `NumerusArray` slots accept `lista<integer>` values
    // that fit the runtime i64 boundary (DEFER-064) so GPU/KIR source
    // can pass `lista<u32>` index vectors without a bare-`numerus`
    // detour. The generic unify path forces
    // `lista<numerus>` and rejects sized-integer lists under the v1
    // widening lattice; route tensor calls through a structural checker
    // instead of widening that lattice globally.
    let result = if !entry.generic.is_empty() {
        // Declared catalog generics use GPD's binder. Empty rows keep
        // the rigid/tensor paths below so receiver-owned shape params
        // stay unsolved.
        let type_params = interned_catalog_generics(checker, entry.generic);
        let sig = FuncSig {
            type_params,
            params,
            ret,
            err: None,
            is_async: false,
            is_generator: false,
        };
        checker.check_instantiated_call(&sig, explicit_type_args, args, span, None)
    } else if matches!(
        entry.receiver,
        radix_intrinsics::IntrinsicReceiver::Tensor
            | radix_intrinsics::IntrinsicReceiver::Sparsa
            | radix_intrinsics::IntrinsicReceiver::Matrix
    ) {
        let value_ty = check_tensor_method_args(checker, entry, &params, args, ret, span);
        if entry.failable {
            // A registry row that declares `⇥` is a failable call: the
            // same handler / `⊥` default / enclosing `⇥` rule (D17.8)
            // as a user function with a `⇥ textus` signature. A call
            // whose index argument is statically proven in range
            // cannot fail (the rule `numeric_conversio_can_fail`
            // applies to a literal that fits) and needs no handler; a
            // provably out-of-range index is the compile-time
            // `index_provably_out_of_bounds`, as the index form gives.
            let range = entry
                .range_checked_arg
                .and_then(|position| args.get(position))
                .map(|arg| {
                    let index = arg.expr.clone();
                    let range = super::tensor_index::classify_static_index_range(
                        checker,
                        receiver_ty,
                        &index,
                    );
                    (index, range)
                });
            match range {
                Some((_, super::tensor_index::StaticIndexRange::InRange)) => {}
                Some((index, range @ super::tensor_index::StaticIndexRange::OutOfRange { .. })) => {
                    super::tensor_index::report_static_index_oob(checker, &index, &range);
                }
                _ => {
                    let err = checker.textus_type();
                    let sig = FuncSig {
                        type_params: Vec::new(),
                        params: Vec::new(),
                        ret: value_ty,
                        err: Some(err),
                        is_async: false,
                        is_generator: false,
                    };
                    checker.reject_failable_call(&sig, span);
                }
            }
        }
        value_ty
    } else if entry.failable {
        // need: the `at` read (la `apud`) on `lista`, `tabula`
        // and `textus` is a failable call: an out-of-range index or a
        // missing key is reported on `⇥ textus`, so the call needs a
        // handler, a `⊥` default or an enclosing `⇥` (D17.8).
        //
        // need (slice 5): a list/text `at` or `slice` whose range is
        // proven (`0 ≤ i < len`; `0 ≤ start ≤ end ≤ len`) cannot fail and
        // needs none of those. A `⊥` default or `cape` already written on it
        // stays legal (the proof is the checker's strength, not the
        // program's), so a local handler still takes the error type.
        let value_ty = checker.check_known_method_args(args, params, ret, span);
        if !checker.list_text_access_proven(entry, receiver, args) || checker.has_local_error_sink()
        {
            let err = checker.textus_type();
            let sig = FuncSig {
                type_params: Vec::new(),
                params: Vec::new(),
                ret: value_ty,
                err: Some(err),
                is_async: false,
                is_generator: false,
            };
            checker.reject_failable_call(&sig, span);
        }
        value_ty
    } else if bounded_appende {
        // D6: bounded `appende` is a typed recoverable alternate exit.
        // Unbounded `lista<T>.appende` stays vacuum with no error channel.
        let err = checker.textus_type();
        let sig = FuncSig {
            type_params: Vec::new(),
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
    // need: a built-in row's `axis_arg` slot takes the `axis` marker
    // word. A bare argument there is refused; a marker on any other argument
    // is refused too.
    for (index, arg) in args.iter().enumerate() {
        let declared = entry.axis_arg == Some(index);
        checker.check_axis_marker(arg, declared);
    }
    let result = if matches!(entry.return_kind, IntrinsicReturnKind::GenericTypus) {
        validate_generic_typus_tensor(checker, receiver_ty, result, span)
    } else {
        result
    };
    if checker.reject_negative_potentia_exponent(entry, receiver_ty, args) {
        return checker.error_type;
    }
    let result = checker.integer_method_range_type(entry, receiver_ty, args, result);
    if entry.faber_name == "accipe" && !entry.failable && !args.is_empty() {
        super::tensor_index::reject_static_bounded_index_oob(checker, receiver_ty, &args[0].expr);
    }
    if matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Tensor) {
        super::tensor_index::apply_tensor_intrinsic_shape_constraints(
            checker,
            entry.faber_name,
            receiver,
            receiver_ty,
            result,
            args,
            span,
        );
    }
    result
}

/// `regex.muta(text, closure)` (RX-5): the closure takes the match and returns
/// the replacement `textus`, taken literally. The match type has no source
/// name (RD-3), so the closure's parameter is typed from this signature.
fn check_regex_replace_closure(
    checker: &mut TypeChecker<'_>,
    args: &mut [HirCallArg],
    span: radix_lexer::Span,
) -> TypeId {
    let found = checker.types.primitive(Primitive::Match);
    let textus = checker.textus_type();
    // WHY: MIR lowering runs the closure over `lista<match>` and joins a
    // `lista<textus>` through a read-only type table; intern both carriers
    // here where the form is admitted.
    checker.types.array(found);
    checker.types.array(textus);
    let replacer = checker.types.function(simple_sig(vec![found], textus));
    let sig = simple_sig(vec![textus, replacer], textus);
    checker.check_call_args(&sig, args, span);
    textus
}

/// `ordina(key)` (D1.7): the key selector maps an element to a value that is
/// ordered today — a number or `textus`. The keyed in-place sort returns
/// vacuum. The keyed `ordinata(key)` form is retired (need, ruling
/// O2): the sort expression `sort via asc from <source> const x { return key }`
/// replaces it, so it is refused by name.
fn check_sort_by_key(
    checker: &mut TypeChecker<'_>,
    entry: &MethodIntrinsic,
    inner: Option<TypeId>,
    args: &mut [HirCallArg],
    span: radix_lexer::Span,
) -> TypeId {
    let Some(inner) = inner else {
        return checker.error_type;
    };
    let key = checker.fresh_infer();
    let key_fn = checker.types.function(simple_sig(vec![inner], key));
    let vacuum = checker.vacuum_type();
    checker.check_call_args(&simple_sig(vec![key_fn], vacuum), args, span);
    if !matches!(entry.return_kind, IntrinsicReturnKind::Vacuum) {
        // FABER-2-SUNSET: delete with the retired-word diagnostics.
        checker.invalid_operand_types(ISSUE_SORTED_KEY_RETIRED, span);
        // The result stays the copy's `lista` so the refusal does not cascade.
        return checker.types.array(inner);
    }
    let key = checker.resolve_inferred_type(key);
    if key != checker.error_type && !checker.is_numeric(key) && !checker.is_textus(key) {
        checker.invalid_operand_types(ISSUE_ORDINA_KEY_NOT_ORDERED, span);
        return checker.error_type;
    }
    // WHY: MIR lowering carries the keys as a `lista<key>` through a
    // read-only type table; intern the carrier here where it is admitted.
    checker.types.array(key);
    vacuum
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

/// The lifted float-only math functions (rule 15) take float elements.
///
/// The set is the registry's own: every tensor row whose MIR op is a
/// `TensorMath` function that is float-only.
fn tensor_float_element_allowed(
    checker: &mut TypeChecker<'_>,
    entry: &MethodIntrinsic,
    inner: Option<TypeId>,
    span: radix_lexer::Span,
) -> bool {
    let float_only = matches!(entry.receiver, radix_intrinsics::IntrinsicReceiver::Tensor)
        && matches!(
            entry.mir_kind,
            IntrinsicMirKind::Collection(radix_types::MirCollectionOp::TensorMath(function))
                if function.float_only()
        );
    if !float_only {
        return true;
    }
    if inner
        .is_some_and(|inner| crate::numeric_width::is_tensor_float_element(checker.types, inner))
    {
        return true;
    }
    checker.invalid_operand_types(ISSUE_TENSOR_MATH_FLOAT_ELEMENT_REQUIRED, span);
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

/// Monomorphic owned-param call signature — the shape every intrinsic-built
/// closure and checked call in this file shares.
fn simple_sig(params: Vec<TypeId>, ret: TypeId) -> FuncSig {
    FuncSig {
        type_params: Vec::new(),
        params: params
            .into_iter()
            .map(|ty| ParamType {
                ty,
                mode: SemanticParamMode::Owned,
                optional: false,
                sponte: false,
            })
            .collect(),
        ret,
        err: None,
        is_async: false,
        is_generator: false,
    }
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
            sponte: false,
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
        IntrinsicParamKind::TensorIndexIds => {
            // WHY: the gather ids are a rank-1 `tensor<u32, [n]>`. The extent
            // `n` is a fresh hole unified with the argument's extent, so it
            // travels with the value (rule 11) and is read back as the
            // result's row count by the gather shape constraint.
            let u32_ty = checker
                .types
                .sized_numeric(crate::Primitive::Numerus, crate::NumericWidth::U32);
            let extent = checker.fresh_index_hole();
            let shape = checker
                .types
                .intern_index(crate::IndexExpr::Tuple(vec![extent]));
            checker.types.tensor_with_shape(u32_ty, shape)
        }
        IntrinsicParamKind::Textus | IntrinsicParamKind::TextusOrReplacer => checker.textus_type(),
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
            checker.types.function(simple_sig(vec![inner], bool_ty))
        }
        IntrinsicParamKind::Mapper => {
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            let ret = checker.fresh_infer();
            checker.types.function(simple_sig(vec![inner], ret))
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

/// `T ∪ nihil` for an optional read of an element or value of type `inner`.
///
/// WHY: an optional of an optional is the same optional. A read over an
/// element that already carries `nihil` (`lista<numerus ∪ nihil>.accipe`)
/// stays `numerus ∪ nihil`, so a plain optional cell takes it with one
/// unwrap and flow narrowing applies; wrapping it again would force two.
fn optional_of(checker: &mut TypeChecker<'_>, inner: TypeId) -> TypeId {
    let mut peeled = inner;
    while let Type::Alias(_, target) = checker.types.get(peeled) {
        peeled = *target;
    }
    let nihil = checker.types.primitive(Primitive::Nihil);
    let already_optional = match checker.types.get(peeled) {
        Type::Option(_) => true,
        Type::Union(members) => members
            .iter()
            .any(|member| checker.types.equals(*member, nihil)),
        _ => false,
    };
    if already_optional {
        inner
    } else {
        checker.types.option(inner)
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
        IntrinsicReturnKind::Numerus | IntrinsicReturnKind::NumerusFromFractus => {
            checker.numerus_type()
        }
        IntrinsicReturnKind::Textus => checker.textus_type(),
        IntrinsicReturnKind::NumericInner | IntrinsicReturnKind::NumericSum => {
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            if !checker.is_numeric(inner) {
                checker.invalid_operand_types(ISSUE_NUMERIC_LISTA_ELEMENTS_REQUIRED, span);
                return checker.error_type;
            }
            if matches!(entry.return_kind, IntrinsicReturnKind::NumericSum) {
                return checker.summa_result_type(inner);
            }
            inner
        }
        IntrinsicReturnKind::OptionInner => {
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            // A list whose length range starts at one or more is never empty:
            // `primus` / `ultimus` / `decapita` / `detrahe` return `T`, not
            // `T ∪ none` (the removals report a violated minimum on `⇥`).
            let non_empty = entry.reads_list_end()
                && ctx
                    .receiver_ty
                    .is_some_and(|ty| matches!(checker.types.get(ty), Type::RangedArray { .. }));
            if non_empty {
                return inner;
            }
            optional_of(checker, inner)
        }
        IntrinsicReturnKind::OptionNumerus => {
            // LSR-W0: the argmax/argmin index-or-nihil shape. The index is
            // `numerus`; the element must be ordered numeric (the scan
            // compares values), and the empty source yields `nihil` —
            // composing with `vel`/`coalesce` through the nullable plumbing,
            // never a −1 sentinel.
            let Some(inner) = ctx.inner else {
                return checker.error_type;
            };
            if !checker.is_numeric(inner) {
                checker.invalid_operand_types(ISSUE_NUMERIC_LISTA_ELEMENTS_REQUIRED, span);
                return checker.error_type;
            }
            let numerus = checker.numerus_type();
            checker.types.option(numerus)
        }
        IntrinsicReturnKind::Value => ctx.value.unwrap_or(checker.error_type),
        IntrinsicReturnKind::OptionValue => {
            let Some(value) = ctx.value else {
                return checker.error_type;
            };
            optional_of(checker, value)
        }
        IntrinsicReturnKind::ArrayInner => {
            if matches!(
                entry.receiver,
                radix_intrinsics::IntrinsicReceiver::Tensor
                    | radix_intrinsics::IntrinsicReceiver::Sparsa
            ) && entry.faber_name == "magnitudines"
            {
                // WHY: K14 gives a tensor a static rank with run-time extents,
                // so the shape list has a compile-time length — the rank — and
                // is the bounded `lista<numerus, magnitudo R>` whose elements are the
                // run-time dimension values. A receiver without a static rank
                // (an inference hole, or a bare `size` parameter standing for a
                // whole shape of any rank) keeps the unbounded list.
                let numerus = checker.numerus_type();
                let rank = ctx
                    .receiver_ty
                    .and_then(|receiver| match checker.types.get(receiver) {
                        Type::Tensor(_, shape) | Type::Sparsa(_, shape) => {
                            let shape = checker.resolve_index(*shape);
                            static_shape_rank(checker.types, shape)
                        }
                        _ => None,
                    });
                let Some(rank) = rank else {
                    return checker.types.array(numerus);
                };
                let capacity = checker.types.intern_index(IndexExpr::Literal(rank as u64));
                return checker.types.bounded_array(numerus, capacity);
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
            } else if matches!(
                checker.types.get(checker.resolve_inferred_type(inner)),
                Type::SizedNumeric(Primitive::Numerus, width) if width.is_decimal()
            ) {
                // decimal-widths D2 (wildcard audit): decimal elements are the
                // named tensor-media reject — teaching issue, not the generic
                // numeric-element one.
                checker.invalid_operand_types(ISSUE_TENSOR_MEDIA_DECIMAL, span);
                checker.error_type
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
        IntrinsicReturnKind::MatchArray => {
            let found = checker.types.primitive(Primitive::Match);
            checker.types.array(found)
        }
        IntrinsicReturnKind::OptionMatch => {
            let found = checker.types.primitive(Primitive::Match);
            checker.types.option(found)
        }
        IntrinsicReturnKind::OptionTextus => {
            let textus = checker.textus_type();
            checker.types.option(textus)
        }
        IntrinsicReturnKind::CursorLittera => {
            let element = checker.text_element_type();
            checker.types.cursor(element)
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
        IntrinsicReturnKind::MapperArray
        | IntrinsicReturnKind::MapperSet
        | IntrinsicReturnKind::ReducerInfer
        | IntrinsicReturnKind::ReducerArray => checker.error_type,
        IntrinsicReturnKind::GenericTypus => {
            let Some(name) = entry
                .generic
                .first()
                .and_then(|param| interned_catalog_name(checker, param.name))
            else {
                return checker.error_type;
            };
            checker.types.intern(Type::Param(name))
        }
    }
}

pub(super) fn validate_generic_typus_tensor(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    result: TypeId,
    span: radix_lexer::Span,
) -> TypeId {
    let resolved = checker.resolve_inferred_type(result);
    if !is_tensor_type(resolved, checker.types) {
        checker.invalid_operand_types(ISSUE_TENSOR_TYPE_ARG_INCOMPLETE, span);
        return checker.error_type;
    }
    let Some(complete) = complete_tensor_type(resolved, checker.types) else {
        checker.invalid_operand_types(ISSUE_TENSOR_TYPE_ARG_INCOMPLETE, span);
        return checker.error_type;
    };
    let recv = checker.resolve_inferred_type(receiver_ty);
    if let Type::Tensor(elem, _) = checker.types.get(recv)
        && !checker.types.equals(*elem, complete.elem)
    {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_TENSOR_FORMATA_ELEMENT_TYPE_MISMATCH,
            span,
        );
        return checker.error_type;
    }
    // Q-X10: when both receiver and target shapes are static, the reshape must
    // keep the element count. A run-time receiver extent emits nothing here;
    // the runner's ERR_FORMA_RESHAPE_COUNT failure stands for that case.
    if let Some(receiver_complete) = complete_tensor_type(recv, checker.types)
        && let Some(receiver_count) = tensor_element_count(&receiver_complete.dims)
        && let Some(target_count) = tensor_element_count(&complete.dims)
        && receiver_count != target_count
    {
        checker.error_issue(
            SemanticErrorKind::TypeMismatch,
            ISSUE_TENSOR_FORMATA_ELEMENT_COUNT_MISMATCH,
            span,
        );
        return checker.error_type;
    }
    checker
        .types
        .tensor_with_shape(complete.elem, complete.shape)
}

/// Product of the static dimensions of a complete tensor shape, or `None` when
/// the product overflows or any extent is negative.
fn tensor_element_count(dims: &[i64]) -> Option<i64> {
    dims.iter()
        .try_fold(1_i64, |count, dim| count.checked_mul(*dim))
        .filter(|count| *count >= 0)
}

/// Typecheck `matrix.applica(vector)`: peer width must equal matrix columns;
/// result is a vector of width = matrix rows with the same element type.
///
/// S0-G9: literal shapes keep the exact register widths and the CPU subset
/// bound; dimension-neutral (parametric) shapes publish their column/row
/// terms as the peer/result widths, which unifies inside a size-generic body.
fn check_matrix_applica(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    args: &mut [HirCallArg],
    span: radix_lexer::Span,
) -> TypeId {
    let resolved = checker.value_carrier_type(receiver_ty);
    let Type::Matrix(element_ty, shape) = checker.types.get(resolved).clone() else {
        return checker.error_type;
    };
    let (peer_width, result_width) = if let Some((rows, columns)) =
        crate::literal_matrix_shape(shape, checker.types)
    {
        if rows > 4 || columns > 4 {
            checker.invalid_operand_types("matrix_register_exceeds_cpu_subset", span);
            return checker.error_type;
        }
        (
            checker
                .types
                .intern_index(crate::IndexExpr::Literal(u64::from(columns))),
            checker
                .types
                .intern_index(crate::IndexExpr::Literal(u64::from(rows))),
        )
    } else {
        // Matrix shapes are always two-dim tuples; a parametric shape
        // carries the column and row terms directly.
        let Some((rows, columns)) = super::tensor_index::matrix_shape_dims(checker, shape) else {
            checker.invalid_operand_types("matrix_applica_requires_literal_shape", span);
            return checker.error_type;
        };
        (columns, rows)
    };
    if args.len() != 1 {
        checker.wrong_arity("wrong_number_of_arguments", span);
        return checker.error_type;
    }
    let peer_ty = checker.types.vector_with_width(element_ty, peer_width);
    let arg_ty = checker.check_expr_with_expected(&mut args[0].expr, Some(peer_ty));
    checker.unify_assign_issue(arg_ty, peer_ty, args[0].span, ISSUE_ARGUMENT_TYPE_MISMATCH);
    checker.types.vector_with_width(element_ty, result_width)
}

/// Typecheck `matrix.transpone()`: a zero-argument method returning the
/// receiver's shape with rows and columns swapped — `[R, C] → [C, R]`,
/// literal or parametric. Shape holes pass through unchanged, mirroring the
/// tensor transpose shape constraint.
fn check_matrix_transpone(checker: &mut TypeChecker<'_>, receiver_ty: TypeId) -> TypeId {
    let resolved = checker.value_carrier_type(receiver_ty);
    let Type::Matrix(element_ty, shape) = checker.types.get(resolved).clone() else {
        return checker.error_type;
    };
    let Some((rows, columns)) = super::tensor_index::matrix_shape_dims(checker, shape) else {
        return resolved;
    };
    let shape = checker
        .types
        .intern_index(crate::IndexExpr::Tuple(vec![columns, rows]));
    checker.types.matrix_with_shape(element_ty, shape)
}

/// Typecheck `xs.extende(other)`: `other` is any list of the receiver's element
/// type, including a fixed-capacity or length-ranged list (a bounded byte
/// buffer `list<b8, size N>` carries `b8` elements). The call returns nothing.
fn check_list_extend(
    checker: &mut TypeChecker<'_>,
    inner: Option<TypeId>,
    args: &mut [HirCallArg],
    span: radix_lexer::Span,
) -> TypeId {
    let vacuum = checker.vacuum_type();
    let Some(inner) = inner else {
        return checker.error_type;
    };
    if args.len() != 1 {
        checker.wrong_arity("wrong_number_of_arguments", span);
        return checker.error_type;
    }
    let source_ty = checker.types.array(inner);
    // A list literal takes its element type from the receiver (`xs.extend([])`);
    // any other source keeps its own type, so a bounded list is not coerced.
    let expected = matches!(args[0].expr.kind, HirExpressionKind::Array(_)).then_some(source_ty);
    let arg_ty = checker.check_expr_with_expected(&mut args[0].expr, expected);
    let resolved = checker.resolve_inferred_type(arg_ty);
    let source_element = match checker.types.get(resolved).clone() {
        Type::Array(element)
        | Type::BoundedArray { element, .. }
        | Type::RangedArray { element, .. } => Some(element),
        Type::BoundedOcteti { .. } => Some(checker.types.bits(radix_types::BitsWidth::B8)),
        _ => None,
    };
    if source_element.is_some_and(|element| checker.types.equals(element, inner)) {
        return vacuum;
    }
    checker.unify_assign_issue(
        arg_ty,
        source_ty,
        args[0].span,
        ISSUE_ARGUMENT_TYPE_MISMATCH,
    );
    vacuum
}

/// Typecheck `vector.multiplicata(operand)`: same-shape peer multiply, or a
/// scalar of the element type splat-multiplied across the lanes (S0-G8).
/// The result keeps the receiver shape at any width, parametric included.
fn check_vector_multiplicata(
    checker: &mut TypeChecker<'_>,
    receiver_ty: TypeId,
    args: &mut [HirCallArg],
    span: radix_lexer::Span,
) -> TypeId {
    let resolved = checker.resolve_inferred_type(receiver_ty);
    let Type::Vector(element_ty, _) = checker.types.get(resolved).clone() else {
        return checker.error_type;
    };
    if args.len() != 1 {
        checker.wrong_arity("wrong_number_of_arguments", span);
        return checker.error_type;
    }
    let arg_ty = checker.check_expr_with_expected(&mut args[0].expr, None);
    let arg_resolved = checker.resolve_inferred_type(arg_ty);
    if checker.types.equals(arg_resolved, element_ty) {
        return resolved;
    }
    checker.unify_assign_issue(arg_ty, resolved, args[0].span, ISSUE_ARGUMENT_TYPE_MISMATCH);
    resolved
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
    // The active call/return shape boundary belongs to the method expression
    // as a whole. Its peer operands are checked against intrinsic parameter
    // types, so inheriting that boundary would reject a fresh TensorPeer shape
    // hole before the intrinsic can relate it to the receiver.
    let previous_shape_argument = checker.current_shape_argument.take();
    let previous_shape_return_expr = checker.current_shape_return_expr.take();
    for (arg_index, ((arg, kind), param)) in args
        .iter_mut()
        .zip(entry.params.iter())
        .zip(params.iter())
        .enumerate()
    {
        if *kind == IntrinsicParamKind::NumerusArray {
            if hir_array_has_shape_hole(&arg.expr) {
                if tensor_shape_hole_argument(entry.faber_name, arg_index) {
                    check_tensor_shape_hole_argument(checker, arg);
                } else {
                    checker.invalid_operand_types(
                        super::aggregate::ISSUE_SHAPE_HOLE_REQUIRES_SHAPE_ARGUMENT,
                        arg.span,
                    );
                    check_tensor_shape_hole_argument(checker, arg);
                }
            } else {
                check_integer_index_vector_arg(checker, arg, span);
            }
        } else if *kind == IntrinsicParamKind::NumerusIndex
            && arg_is_symbolic_size(checker, &arg.expr)
        {
            // WHY (SGR-C1): a body-visible `magnitudo` parameter names its
            // compile-time size at an index slot. Lowering types it as the
            // rigid symbolic-size marker `Type::Param`; checked without an
            // expected type the marker stays rigid. Admission is slot-local:
            // the same marker in any other value position keeps failing
            // ordinary unification.
            checker.check_expr(&mut arg.expr);
        } else {
            let arg_ty = checker.check_expr_with_expected(&mut arg.expr, Some(param.ty));
            checker.unify_assign_issue(arg_ty, param.ty, arg.span, ISSUE_ARGUMENT_TYPE_MISMATCH);
        }
    }
    checker.current_shape_argument = previous_shape_argument;
    checker.current_shape_return_expr = previous_shape_return_expr;
    ret
}

/// A shifted tensor remains a lazy optional view until `limes` or tensor
/// coalescing resolves its edge cells. The semantic element type records that
/// fact as `T ∪ nihil`; the side table tracks which such tensors carry the
/// unresolved physical edge descriptor.
fn check_tensor_shift(
    checker: &mut TypeChecker<'_>,
    receiver: &HirExpression,
    receiver_ty: TypeId,
    args: &[HirCallArg],
    span: radix_lexer::Span,
) -> TypeId {
    let resolved = checker.resolve_inferred_type(receiver_ty);
    let Type::Tensor(element, shape) = checker.types.get(resolved).clone() else {
        checker.invalid_operand_types(ISSUE_TENSOR_SHIFT_NULLABLE_SOURCE_UNSUPPORTED, span);
        return checker.error_type;
    };
    let element = checker.resolve_inferred_type(element);
    if checker
        .tensor_edge_policies
        .shifted_expressions
        .contains(&receiver.id)
        || matches!(checker.types.get(element), Type::Option(_))
    {
        // A second shift would create nested optional edge state, which the
        // carrier intentionally cannot represent as an ordinary Tensor<T>.
        checker.invalid_operand_types(ISSUE_TENSOR_SHIFT_NULLABLE_SOURCE_UNSUPPORTED, span);
        return checker.error_type;
    }
    let Some(rank) = tensor_shape_rank_from_index(checker.types, shape) else {
        checker.invalid_operand_types(ISSUE_TENSOR_SHIFT_OFFSET_RANK_MISMATCH, span);
        return checker.error_type;
    };
    if let Some(HirCallArg {
        expr:
            HirExpression {
                kind: HirExpressionKind::Array(elements),
                ..
            },
        ..
    }) = args.first()
        && elements
            .iter()
            .all(|element| matches!(element, HirArrayElement::Expr(_)))
        && elements.len() != rank
    {
        checker.invalid_operand_types(ISSUE_TENSOR_SHIFT_OFFSET_RANK_MISMATCH, span);
        return checker.error_type;
    }

    let optional_element = checker.types.option(element);
    let result = checker.types.tensor_with_shape(optional_element, shape);
    if let (Some(call), Some(offsets)) = (checker.current_shape_call, args.first()) {
        checker
            .tensor_edge_policies
            .shifted_expressions
            .insert(call);
        checker
            .tensor_edge_policies
            .shifted_view_expressions
            .insert(
                call,
                crate::TensorShiftContext {
                    source_shape: shape,
                    shift_expression: offsets.expr.id,
                },
            );
    }
    result
}

/// Resolve `limes<Policy>()` only on a tensor known to carry an unresolved
/// shifted descriptor. Builtins are compiler identities; user policies are
/// structurally checked ordinary methods on a zero-instance-field class.
fn check_tensor_limes(
    checker: &mut TypeChecker<'_>,
    receiver: &HirExpression,
    receiver_ty: TypeId,
    explicit_type_args: &[TypeId],
    args: &mut [HirCallArg],
    span: radix_lexer::Span,
) -> TypeId {
    if !args.is_empty() {
        checker.typecheck_call_arg_exprs(args);
        checker.invalid_operand_types(ISSUE_TENSOR_LIMES_POLICY_INVALID, span);
        return checker.error_type;
    }
    if !checker
        .tensor_edge_policies
        .shifted_expressions
        .contains(&receiver.id)
    {
        checker.invalid_operand_types(ISSUE_TENSOR_LIMES_REQUIRES_SHIFTED_VIEW, span);
        return checker.error_type;
    }
    let Some(shift_context) = checker
        .tensor_edge_policies
        .shifted_view_expressions
        .get(&receiver.id)
        .copied()
    else {
        // The sidecar is also consumed by kernel legality and callback range
        // analysis. A shifted marker without its source provenance is not a
        // usable proof of an edge-policy call.
        checker.invalid_operand_types(ISSUE_TENSOR_LIMES_REQUIRES_SHIFTED_VIEW, span);
        return checker.error_type;
    };
    let resolved = checker.resolve_inferred_type(receiver_ty);
    let Type::Tensor(optional_element, shape) = checker.types.get(resolved).clone() else {
        checker.invalid_operand_types(ISSUE_TENSOR_LIMES_REQUIRES_SHIFTED_VIEW, span);
        return checker.error_type;
    };
    let optional_element = checker.resolve_inferred_type(optional_element);
    let Type::Option(element) = checker.types.get(optional_element).clone() else {
        checker.invalid_operand_types(ISSUE_TENSOR_LIMES_REQUIRES_SHIFTED_VIEW, span);
        return checker.error_type;
    };
    let resolved_element = checker.resolve_inferred_type(element);
    if matches!(checker.types.get(resolved_element), Type::Option(_)) {
        checker.invalid_operand_types(ISSUE_TENSOR_LIMES_REQUIRES_SHIFTED_VIEW, span);
        return checker.error_type;
    }
    let Some(&policy_type) = explicit_type_args.first() else {
        // The catalog arity check normally catches this. Keep the helper
        // fail-closed when reached from recovery or a synthetic HIR caller.
        checker.invalid_operand_types(ISSUE_TENSOR_LIMES_POLICY_INVALID, span);
        return checker.error_type;
    };
    let Some(rank) = tensor_shape_rank_from_index(checker.types, shape) else {
        checker.invalid_operand_types(ISSUE_TENSOR_LIMES_POLICY_INVALID, span);
        return checker.error_type;
    };
    let policy_type_resolved = checker.resolve_inferred_type(policy_type);
    let policy = if let Some(policy) = checker.types.tensor_edge_policy_kind(policy_type_resolved) {
        crate::ResolvedTensorEdgePolicy::Builtin(policy)
    } else {
        match checker.types.get(policy_type_resolved).clone() {
            Type::Param(_) if checker.is_tensor_edge_policy_type_param(policy_type_resolved) => {
                crate::ResolvedTensorEdgePolicy::Deferred { policy_type }
            }
            Type::Param(_) => {
                checker.invalid_operand_types(ISSUE_TENSOR_LIMES_POLICY_INVALID, span);
                return checker.error_type;
            }
            _ => {
                if let Some(policy) =
                    check_custom_tensor_edge_policy(checker, policy_type, element, rank)
                {
                    policy
                } else {
                    checker.invalid_operand_types(ISSUE_TENSOR_LIMES_POLICY_INVALID, span);
                    return checker.error_type;
                }
            }
        }
    };
    if let Some(call) = checker.current_shape_call {
        checker.tensor_edge_policies.calls.insert(call, policy);
        checker
            .tensor_edge_policies
            .limes_contexts
            .insert(call, shift_context);
    }
    checker.types.tensor_with_shape(element, shape)
}

/// The rank a tensor shape fixes at check time. A bare `size` parameter in
/// shape position binds a whole shape at instantiation, so it fixes no rank,
/// unlike `tensor_shape_rank_from_index`, which reads it as one dimension.
fn static_shape_rank(types: &crate::TypeTable, shape: IndexId) -> Option<usize> {
    match types.get_index(shape) {
        IndexExpr::Param(_) => None,
        _ => tensor_shape_rank_from_index(types, shape),
    }
}

pub(super) fn tensor_shape_rank_from_index(
    types: &crate::TypeTable,
    shape: IndexId,
) -> Option<usize> {
    match types.get_index(shape) {
        IndexExpr::Tuple(dims) => Some(dims.len()),
        IndexExpr::Literal(_)
        | IndexExpr::Param(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Affine { .. }
        | IndexExpr::Divide { .. } => Some(1),
        IndexExpr::Infer(_) | IndexExpr::Unspecified => None,
    }
}

pub(super) fn check_custom_tensor_edge_policy(
    checker: &mut TypeChecker<'_>,
    policy_type: TypeId,
    element: TypeId,
    rank: usize,
) -> Option<crate::ResolvedTensorEdgePolicy> {
    let mut nominal = checker.resolve_inferred_type(policy_type);
    while let Type::Alias(_, inner) = checker.types.get(nominal) {
        nominal = *inner;
    }
    let def = checker.struct_def_from_type(nominal)?;
    if checker.instance_field_counts.get(&def).copied()? != 0 {
        return None;
    }
    let remap_name = checker.interner?.lookup("remap")?;
    let transform_name = checker.interner?.lookup("transform");
    let remap_def = *checker.method_defs.get(&(def, remap_name))?;
    let remap_sig = checker.structs.get(&def)?.methods.get(&remap_name)?.clone();
    let remap_sig = instantiate_edge_policy_method(checker, nominal, def, &remap_sig)?;
    let coordinate =
        validate_edge_policy_callback_signature(checker, &remap_sig, element, rank, false)?;

    let transform = if let Some(name) = transform_name
        && let Some(def_id) = checker.method_defs.get(&(def, name)).copied()
    {
        let sig = checker.structs.get(&def)?.methods.get(&name)?.clone();
        let sig = instantiate_edge_policy_method(checker, nominal, def, &sig)?;
        validate_edge_policy_callback_signature(checker, &sig, element, rank, true)
            .filter(|transform_coord| checker.types.equals(*transform_coord, coordinate))?;
        Some(def_id)
    } else {
        None
    };
    Some(crate::ResolvedTensorEdgePolicy::Custom {
        policy_def: def,
        remap: remap_def,
        transform,
    })
}

fn instantiate_edge_policy_method(
    checker: &mut TypeChecker<'_>,
    nominal: TypeId,
    def: radix_hir::DefId,
    signature: &FuncSig,
) -> Option<FuncSig> {
    if !signature.type_params.is_empty() {
        return None;
    }
    let names = checker
        .type_params_by_def
        .get(&def)
        .cloned()
        .unwrap_or_default();
    let applied = match checker.types.get(nominal).clone() {
        Type::Applied(_, args) => args,
        Type::Struct(_) => Vec::new(),
        _ => return None,
    };
    if names.len() != applied.len() {
        return None;
    }
    let mut type_vars = handroll::FxHashMap::default();
    let mut shape_vars = handroll::FxHashMap::default();
    for (name, argument) in names.into_iter().zip(applied) {
        match argument {
            radix_types::AppliedArg::Type(ty) => {
                type_vars.insert(name, ty);
            }
            radix_types::AppliedArg::Size(size) => {
                shape_vars.insert(name, size);
            }
        }
    }
    Some(checker.substitute_func_sig(signature, &mut type_vars, Some(&mut shape_vars)))
}

fn validate_edge_policy_callback_signature(
    checker: &TypeChecker<'_>,
    signature: &FuncSig,
    element: TypeId,
    rank: usize,
    transform: bool,
) -> Option<TypeId> {
    if signature.err.is_some() || signature.is_async || signature.is_generator {
        return None;
    }
    let expected_arity = if transform { 2 } else { 1 };
    if signature.params.len() != expected_arity
        || signature
            .params
            .iter()
            .any(|param| param.optional || param.mode != SemanticParamMode::Owned)
    {
        return None;
    }
    let coordinate = checker.resolve_inferred_type(signature.params[0].ty);
    let Type::Vector(coordinate_element, width) = checker.types.get(coordinate) else {
        return None;
    };
    if !crate::numeric_width::is_integer_index_scalar(checker.types, *coordinate_element)
        || radix_types::evaluate_index_dimension(*width, checker.types)
            .ok()
            .flatten()
            != u64::try_from(rank).ok()
    {
        return None;
    }
    if transform {
        let value = checker.resolve_inferred_type(signature.params[1].ty);
        if !checker.types.equals(value, element) || !checker.types.equals(signature.ret, element) {
            return None;
        }
        // Transform's result is the tensor element, while its first argument
        // and the remap result are coordinates. The caller separately checks
        // the coordinate against `remap`.
        Some(coordinate)
    } else {
        if !checker.types.equals(coordinate, signature.ret) {
            return None;
        }
        Some(coordinate)
    }
}

fn tensor_shape_hole_argument(method: &str, arg_index: usize) -> bool {
    matches!(
        (method, arg_index),
        ("forma" | "expande", 0) | ("crea" | "strue", 1)
    )
}

fn hir_array_has_shape_hole(expr: &HirExpression) -> bool {
    matches!(&expr.kind, HirExpressionKind::Array(elements)
        if elements.iter().any(|element| matches!(element, HirArrayElement::Infer(_))))
}

fn check_tensor_shape_hole_argument(checker: &mut TypeChecker<'_>, arg: &mut HirCallArg) {
    let HirExpressionKind::Array(elements) = &mut arg.expr.kind else {
        // The shape-hole probe only admits array expressions; check anything
        // else as an ordinary argument.
        checker.check_expr(&mut arg.expr);
        return;
    };
    for element in elements {
        match element {
            HirArrayElement::Expr(expr) => {
                let ty = checker.check_expr(expr);
                if !crate::numeric_width::is_integer_index_scalar(
                    checker.types,
                    checker.resolve_inferred_type(ty),
                ) {
                    checker.invalid_operand_types(
                        ISSUE_TENSOR_INDEX_VECTOR_INTEGER_ELEMENTS,
                        expr.span,
                    );
                }
            }
            HirArrayElement::Spread(expr) => {
                checker.check_expr(expr);
                checker
                    .invalid_operand_types(ISSUE_TENSOR_INDEX_VECTOR_INTEGER_ELEMENTS, expr.span);
            }
            HirArrayElement::Infer(_) => {}
        }
    }
    let numerus = checker.numerus_type();
    arg.expr.ty = Some(checker.types.array(numerus));
}

/// True when the argument is a bare reference to a body-visible `magnitudo`
/// parameter, lowered as a `Path` to a `Param`-kind symbol typed
/// `Type::Param(name)`. Purely a peek: no diagnostics, no inference.
fn arg_is_symbolic_size(checker: &TypeChecker<'_>, expr: &HirExpression) -> bool {
    let HirExpressionKind::Path(def_id) = &expr.kind else {
        return false;
    };
    checker
        .resolver
        .get_symbol(*def_id)
        .and_then(|symbol| symbol.ty)
        .is_some_and(|ty| matches!(checker.types.get(ty), Type::Param(_)))
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

/// The const local at the root of a field/index/deref projection chain, when
/// `place` is such a projection (a bare path is the binding-level rule).
fn const_local_projection_root(
    checker: &TypeChecker<'_>,
    place: &HirExpression,
) -> Option<radix_hir::DefId> {
    let mut current = match &place.kind {
        HirExpressionKind::Field(object, _)
        | HirExpressionKind::Index(object, _)
        | HirExpressionKind::Deref(object) => object.as_ref(),
        _ => return None,
    };
    loop {
        match &current.kind {
            HirExpressionKind::Field(object, _)
            | HirExpressionKind::Index(object, _)
            | HirExpressionKind::Deref(object) => current = object.as_ref(),
            HirExpressionKind::Path(def_id) => {
                return checker.const_locals.contains(def_id).then_some(*def_id);
            }
            _ => return None,
        }
    }
}
