//! Shared MIR generic binding and type-substitution helpers.
//!
//! Validation and monomorphization both need the same structural witness rules:
//! call-site values bind `Type::Param` occurrences inside semantic MIR types,
//! then concrete functions consume the instantiated type graph. Keep that logic
//! here so target emitters never grow their own generic policies.

use crate::{
    MirAggregate, MirAggregateFields, MirAggregateItem, MirCallee, MirCollectionOp, MirConstant,
    MirFunction, MirIntrinsic, MirOperand, MirOptionChainLink, MirOptionOp, MirPlace,
    MirProjection, MirRuntimeCall, MirStatement, MirStatementKind, MirTerminator,
    MirTerminatorKind, MirType, MirValue, MirValueKind,
};
use handroll::{FxHashMap, FxHashSet, FxHasher};
use radix_lexer::{Span, Symbol};
use radix_types::{
    FuncSig, IndexExpr, IndexId, ParamType, Type, TypeId, TypeTable, i64s_from_literal_index,
};
use std::cell::RefCell;
use std::hash::{Hash, Hasher};

/// Message carried by a [`MirGenericError`] when a substituted type has no
/// interned semantic identity yet.
///
/// The read-only existing-type instantiation helpers cannot intern the missing
/// type themselves, so they report this message. Callers that can intern (the
/// monomorphizer's finalize pass) recognize it through
/// [`MirGenericError::is_missing_metadata`] and defer instead of failing.
pub const MISSING_SEMANTIC_METADATA: &str =
    "instantiated generic type is missing semantic metadata";

pub type MirGenericBindings = FxHashMap<Symbol, MirType>;

/// Shape-param bindings (`magnitudo N`) parallel to [`MirGenericBindings`].
///
/// Shape params are compile-time index facts, not types: the witness binds
/// them from call-site argument/destination shapes and substitution replaces
/// `IndexExpr::Param` nodes inside indexed types.
pub type MirShapeBindings = FxHashMap<Symbol, IndexId>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirGenericError {
    pub message: String,
    pub span: Span,
}

impl MirGenericError {
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            span,
        }
    }

    /// Whether this error is the deferred-interning signal
    /// [`MISSING_SEMANTIC_METADATA`]: the substituted type is well formed but
    /// not yet interned in the caller's table.
    #[must_use]
    pub fn is_missing_metadata(&self) -> bool {
        self.message == MISSING_SEMANTIC_METADATA
    }
}

// ---------------------------------------------------------------------------
// Structural witness
// ---------------------------------------------------------------------------

pub fn bind_generic_type_witness(
    types: &TypeTable,
    expected: MirType,
    actual: MirType,
    generic_bindings: &mut MirGenericBindings,
) -> bool {
    let mut shape_bindings = MirShapeBindings::default();
    bind_generic_type_witness_with_shapes(
        types,
        expected,
        actual,
        generic_bindings,
        &mut shape_bindings,
    )
}

/// [`bind_generic_type_witness`] with shape-param binding (4A).
///
/// `magnitudo` shape params inside indexed types bind from the call-site
/// shapes (D1), parallel to the type-param treatment. Validation's lenient
/// definition-callee path keeps the 4-arg entry; monomorphization uses this
/// one because it needs the bound shapes for the instance key.
pub fn bind_generic_type_witness_with_shapes(
    types: &TypeTable,
    expected: MirType,
    actual: MirType,
    generic_bindings: &mut MirGenericBindings,
    shape_bindings: &mut MirShapeBindings,
) -> bool {
    if !type_contains_param(types, expected.semantic_id()) {
        return true;
    }
    let mut seen = FxHashSet::default();
    try_bind_generic_type_witness(
        types,
        expected.semantic_id(),
        actual.semantic_id(),
        generic_bindings,
        shape_bindings,
        &mut seen,
    )
}

/// Structural witness for generic type parameters.
///
/// `seen` tracks `Type::Alias` nodes on the current resolution path (PRP-059 /
/// BO-H-13). Insert before peeling an Alias; remove after the recursive call
/// returns. Path-local membership detects cyclic Alias chains without rejecting
/// legitimate DAG sharing of the same `TypeId` in sibling structural positions
/// (e.g. `map<T, T>` or the same Alias used twice in a compound type).
///
/// Alias peeling runs **before** [`TypeTable::nullable_inner`], which also walks
/// Alias chains but has no seen-set (tracked separately for `radix-types`).
fn try_bind_generic_type_witness(
    types: &TypeTable,
    expected: TypeId,
    actual: TypeId,
    generic_bindings: &mut MirGenericBindings,
    shape_bindings: &mut MirShapeBindings,
    seen: &mut FxHashSet<TypeId>,
) -> bool {
    // Param binds to the actual TypeId as-is, including when actual is Alias.
    // That precedence must stay above Alias peeling on the actual side.
    if let Type::Param(sym) = types.get(expected) {
        return bind_type_param(types, *sym, actual, generic_bindings);
    }

    if let Type::Alias(_, inner) = types.get(expected) {
        if !seen.insert(expected) {
            return false;
        }
        let inner = *inner;
        let ok = try_bind_generic_type_witness(
            types,
            inner,
            actual,
            generic_bindings,
            shape_bindings,
            seen,
        );
        seen.remove(&expected);
        return ok;
    }

    if let Type::Alias(_, inner) = types.get(actual) {
        if !seen.insert(actual) {
            return false;
        }
        let inner = *inner;
        let ok = try_bind_generic_type_witness(
            types,
            expected,
            inner,
            generic_bindings,
            shape_bindings,
            seen,
        );
        seen.remove(&actual);
        return ok;
    }

    // Neither side is Alias — nullable_inner will not walk an Alias cycle here.
    if let Some(expected_inner) = types.nullable_inner(expected)
        && let Some(actual_inner) = types.nullable_inner(actual)
    {
        return try_bind_generic_type_witness(
            types,
            expected_inner,
            actual_inner,
            generic_bindings,
            shape_bindings,
            seen,
        );
    }

    match (types.get(expected), types.get(actual)) {
        (Type::Array(expected_inner), Type::Array(actual_inner))
        | (Type::Set(expected_inner), Type::Set(actual_inner))
        | (Type::Promissum(expected_inner), Type::Promissum(actual_inner))
        | (Type::Cursor(expected_inner), Type::Cursor(actual_inner))
        | (Type::Queue(expected_inner), Type::Queue(actual_inner))
        | (Type::Stack(expected_inner), Type::Stack(actual_inner))
        | (Type::Intervallum(expected_inner, _), Type::Intervallum(actual_inner, _))
        | (Type::Option(expected_inner), Type::Option(actual_inner)) => {
            try_bind_generic_type_witness(
                types,
                *expected_inner,
                *actual_inner,
                generic_bindings,
                shape_bindings,
                seen,
            )
        }
        (
            Type::Tensor(expected_inner, expected_shape),
            Type::Tensor(actual_inner, actual_shape),
        )
        | (
            Type::Vector(expected_inner, expected_shape),
            Type::Vector(actual_inner, actual_shape),
        )
        | (
            Type::Matrix(expected_inner, expected_shape),
            Type::Matrix(actual_inner, actual_shape),
        )
        | (
            Type::Sparsa(expected_inner, expected_shape),
            Type::Sparsa(actual_inner, actual_shape),
        ) => {
            try_bind_index_witness(types, *expected_shape, *actual_shape, shape_bindings)
                && try_bind_generic_type_witness(
                    types,
                    *expected_inner,
                    *actual_inner,
                    generic_bindings,
                    shape_bindings,
                    seen,
                )
        }
        (
            Type::BoundedArray {
                element: expected_element,
                capacity: expected_capacity,
            },
            Type::BoundedArray {
                element: actual_element,
                capacity: actual_capacity,
            },
        )
        | (
            Type::BoundedQueue {
                element: expected_element,
                capacity: expected_capacity,
            },
            Type::BoundedQueue {
                element: actual_element,
                capacity: actual_capacity,
            },
        )
        | (
            Type::BoundedStack {
                element: expected_element,
                capacity: expected_capacity,
            },
            Type::BoundedStack {
                element: actual_element,
                capacity: actual_capacity,
            },
        ) => {
            try_bind_index_witness(types, *expected_capacity, *actual_capacity, shape_bindings)
                && try_bind_generic_type_witness(
                    types,
                    *expected_element,
                    *actual_element,
                    generic_bindings,
                    shape_bindings,
                    seen,
                )
        }
        (
            Type::RangedArray {
                element: expected_element,
                min: expected_min,
                max: expected_max,
            },
            Type::RangedArray {
                element: actual_element,
                min: actual_min,
                max: actual_max,
            },
        ) => {
            expected_min == actual_min
                && expected_max == actual_max
                && try_bind_generic_type_witness(
                    types,
                    *expected_element,
                    *actual_element,
                    generic_bindings,
                    shape_bindings,
                    seen,
                )
        }
        (Type::Map(expected_key, expected_value), Type::Map(actual_key, actual_value)) => {
            try_bind_generic_type_witness(
                types,
                *expected_key,
                *actual_key,
                generic_bindings,
                shape_bindings,
                seen,
            ) && try_bind_generic_type_witness(
                types,
                *expected_value,
                *actual_value,
                generic_bindings,
                shape_bindings,
                seen,
            )
        }
        (Type::Tuple(expected_elements), Type::Tuple(actual_elements)) => {
            expected_elements.len() == actual_elements.len()
                && expected_elements.iter().zip(actual_elements.iter()).all(
                    |(expected_element, actual_element)| {
                        try_bind_generic_type_witness(
                            types,
                            *expected_element,
                            *actual_element,
                            generic_bindings,
                            shape_bindings,
                            seen,
                        )
                    },
                )
        }
        (Type::Ref(expected_mode, expected_inner), Type::Ref(actual_mode, actual_inner))
            if expected_mode == actual_mode =>
        {
            try_bind_generic_type_witness(
                types,
                *expected_inner,
                *actual_inner,
                generic_bindings,
                shape_bindings,
                seen,
            )
        }
        (Type::Applied(expected_base, expected_args), Type::Applied(actual_base, actual_args)) => {
            expected_args.len() == actual_args.len()
                && try_bind_generic_type_witness(
                    types,
                    *expected_base,
                    *actual_base,
                    generic_bindings,
                    shape_bindings,
                    seen,
                )
                && expected_args
                    .iter()
                    .zip(actual_args.iter())
                    .all(
                        |(expected_arg, actual_arg)| match (expected_arg, actual_arg) {
                            (
                                radix_types::AppliedArg::Type(expected),
                                radix_types::AppliedArg::Type(actual),
                            ) => try_bind_generic_type_witness(
                                types,
                                *expected,
                                *actual,
                                generic_bindings,
                                shape_bindings,
                                seen,
                            ),
                            (
                                radix_types::AppliedArg::Size(expected),
                                radix_types::AppliedArg::Size(actual),
                            ) => try_bind_index_witness(types, *expected, *actual, shape_bindings),
                            _ => false,
                        },
                    )
        }
        (Type::Union(expected_members), Type::Union(actual_members)) => try_bind_union_members(
            types,
            expected_members,
            actual_members,
            generic_bindings,
            shape_bindings,
            seen,
        ),
        (Type::Func(expected_sig), Type::Func(actual_sig)) => try_bind_function_signature(
            types,
            expected_sig,
            actual_sig,
            generic_bindings,
            shape_bindings,
            seen,
        ),
        _ => types.is_assignable(actual, expected),
    }
}

/// Bind a type parameter to a concrete type.
///
/// Insert-only: when `param` is already bound, compares for equality and never
/// overwrites. Union-member undo-on-backtrack (PRP-060) relies on this invariant —
/// failed candidates can be rolled back by removing keys inserted during the attempt.
fn bind_type_param(
    types: &TypeTable,
    param: Symbol,
    actual: TypeId,
    generic_bindings: &mut MirGenericBindings,
) -> bool {
    let actual = MirType::semantic(actual);
    if let Some(bound) = generic_bindings.get(&param).copied() {
        types.equals(bound.semantic_id(), actual.semantic_id())
    } else {
        generic_bindings.insert(param, actual);
        true
    }
}

/// Structural witness over an indexed type's shape term (D1 parallel).
///
/// `IndexExpr::Param` nodes on the expected side bind to the actual index
/// node; tuple shapes bind elementwise; everything else falls back to
/// [`TypeTable::index_equals`]. Insert-only, mirroring [`bind_type_param`], so
/// union backtracking can roll back failed candidates.
fn index_contains_runtime(types: &TypeTable, index: IndexId) -> bool {
    match types.get_index(index) {
        IndexExpr::Runtime(_) | IndexExpr::Infer(_) => true,
        IndexExpr::Affine { base, .. } => index_contains_runtime(types, *base),
        IndexExpr::Divide { numerator, .. } => index_contains_runtime(types, *numerator),
        IndexExpr::Tuple(items) => items
            .iter()
            .any(|item| index_contains_runtime(types, *item)),
        _ => false,
    }
}

fn try_bind_index_witness(
    types: &TypeTable,
    expected: IndexId,
    actual: IndexId,
    shape_bindings: &mut MirShapeBindings,
) -> bool {
    if let IndexExpr::Param(sym) = types.get_index(expected) {
        return bind_shape_param(types, *sym, actual, shape_bindings);
    }
    match (types.get_index(expected), types.get_index(actual)) {
        (
            IndexExpr::Affine { base, offset },
            IndexExpr::Affine {
                base: actual_base,
                offset: actual_offset,
            },
        ) if offset == actual_offset => {
            if let IndexExpr::Param(parameter) = types.get_index(*base) {
                bind_shape_param(types, *parameter, *actual_base, shape_bindings)
            } else {
                try_bind_index_witness(types, *base, *actual_base, shape_bindings)
            }
        }
        (IndexExpr::Affine { base, offset }, IndexExpr::Literal(value)) => {
            let IndexExpr::Param(parameter) = types.get_index(*base) else {
                return false;
            };
            let Some(base_value) = i128::from(*value)
                .checked_sub(*offset)
                .and_then(|value| u64::try_from(value).ok())
            else {
                return false;
            };
            let Some(witness) = existing_literal_index(types, base_value) else {
                return false;
            };
            bind_shape_param(types, *parameter, witness, shape_bindings)
        }
        (
            IndexExpr::Divide { numerator, divisor },
            IndexExpr::Divide {
                numerator: actual_numerator,
                divisor: actual_divisor,
            },
        ) if divisor == actual_divisor && *divisor != 0 => {
            if let IndexExpr::Param(parameter) = types.get_index(*numerator) {
                bind_shape_param(types, *parameter, *actual_numerator, shape_bindings)
            } else {
                try_bind_index_witness(types, *numerator, *actual_numerator, shape_bindings)
            }
        }
        (IndexExpr::Divide { numerator, divisor }, IndexExpr::Literal(value)) if *divisor != 0 => {
            let IndexExpr::Param(parameter) = types.get_index(*numerator) else {
                return false;
            };
            let Some(numerator_value) = value.checked_mul(*divisor) else {
                return false;
            };
            let Some(witness) = existing_literal_index(types, numerator_value) else {
                return false;
            };
            bind_shape_param(types, *parameter, witness, shape_bindings)
        }
        (IndexExpr::Tuple(expected_dims), IndexExpr::Tuple(actual_dims)) => {
            expected_dims.len() == actual_dims.len()
                && expected_dims
                    .iter()
                    .zip(actual_dims.iter())
                    .all(|(expected_dim, actual_dim)| {
                        try_bind_index_witness(types, *expected_dim, *actual_dim, shape_bindings)
                    })
        }
        _ => {
            // Structural equality first. A pair whose caller-side axis is
            // runtime-origin carries its equality in the emitted runtime
            // shape check (store/call obligation), so binding accepts it.
            types.index_equals(expected, actual) || index_contains_runtime(types, actual)
        }
    }
}

fn existing_literal_index(types: &TypeTable, value: u64) -> Option<IndexId> {
    (0..types.index_count()).find_map(|ordinal| {
        let id = u32::try_from(ordinal).ok().map(IndexId)?;
        matches!(types.get_index(id), IndexExpr::Literal(found) if *found == value).then_some(id)
    })
}

/// Bind a shape parameter to a concrete index node.
///
/// Insert-only: when `param` is already bound, compares for structural index
/// equality and never overwrites (mirrors [`bind_type_param`] so union
/// backtracking can roll back via [`undo_bindings_to`]).
fn bind_shape_param(
    types: &TypeTable,
    param: Symbol,
    actual: IndexId,
    shape_bindings: &mut MirShapeBindings,
) -> bool {
    if let Some(bound) = shape_bindings.get(&param).copied() {
        types.index_equals(bound, actual)
    } else {
        shape_bindings.insert(param, actual);
        true
    }
}

fn try_bind_union_members(
    types: &TypeTable,
    expected_members: &[TypeId],
    actual_members: &[TypeId],
    generic_bindings: &mut MirGenericBindings,
    shape_bindings: &mut MirShapeBindings,
    seen: &mut FxHashSet<TypeId>,
) -> bool {
    if expected_members.len() != actual_members.len() {
        return false;
    }

    let mut matched = vec![false; actual_members.len()];
    try_bind_union_member_at(
        types,
        0,
        expected_members,
        actual_members,
        &mut matched,
        generic_bindings,
        shape_bindings,
        seen,
    )
}

/// Match expected union members against actual members via recursive backtracking.
///
/// Mutates one shared `generic_bindings` map and rolls back keys inserted by a
/// failed candidate (PRP-060 / BO-H-5). Does not clone the bindings map per
/// candidate. Undo is correct because [`bind_type_param`] only inserts, never
/// overwrites existing bindings.
#[allow(clippy::too_many_arguments)]
fn try_bind_union_member_at(
    types: &TypeTable,
    index: usize,
    expected_members: &[TypeId],
    actual_members: &[TypeId],
    matched: &mut [bool],
    generic_bindings: &mut MirGenericBindings,
    shape_bindings: &mut MirShapeBindings,
    seen: &mut FxHashSet<TypeId>,
) -> bool {
    let Some(expected) = expected_members.get(index).copied() else {
        return true;
    };

    for actual_index in 0..actual_members.len() {
        if matched[actual_index] {
            continue;
        }

        // Snapshot keys present before this candidate. bind_type_param and
        // bind_shape_param are insert-only, so rolling back means removing
        // keys not in the snapshot.
        let keys_before: FxHashSet<Symbol> = generic_bindings.keys().copied().collect();
        let shape_keys_before: FxHashSet<Symbol> = shape_bindings.keys().copied().collect();

        if !try_bind_generic_type_witness(
            types,
            expected,
            actual_members[actual_index],
            generic_bindings,
            shape_bindings,
            seen,
        ) {
            undo_bindings_to(generic_bindings, &keys_before);
            undo_bindings_to(shape_bindings, &shape_keys_before);
            continue;
        }

        matched[actual_index] = true;
        if try_bind_union_member_at(
            types,
            index + 1,
            expected_members,
            actual_members,
            matched,
            generic_bindings,
            shape_bindings,
            seen,
        ) {
            return true;
        }

        undo_bindings_to(generic_bindings, &keys_before);
        undo_bindings_to(shape_bindings, &shape_keys_before);
        matched[actual_index] = false;
    }

    false
}

/// Remove any keys inserted after `keys_before` was captured.
///
/// Safe only while bindings are insert-only (see [`bind_type_param`] and
/// [`bind_shape_param`]).
fn undo_bindings_to<V>(bindings: &mut FxHashMap<Symbol, V>, keys_before: &FxHashSet<Symbol>) {
    bindings.retain(|key, _| keys_before.contains(key));
}

fn try_bind_function_signature(
    types: &TypeTable,
    expected_sig: &FuncSig,
    actual_sig: &FuncSig,
    generic_bindings: &mut MirGenericBindings,
    shape_bindings: &mut MirShapeBindings,
    seen: &mut FxHashSet<TypeId>,
) -> bool {
    expected_sig.params.len() == actual_sig.params.len()
        && expected_sig.is_async == actual_sig.is_async
        && expected_sig.is_generator == actual_sig.is_generator
        && expected_sig
            .params
            .iter()
            .zip(actual_sig.params.iter())
            .all(|(expected_param, actual_param)| {
                expected_param.mode == actual_param.mode
                    && expected_param.optional == actual_param.optional
                    && expected_param.sponte == actual_param.sponte
                    && try_bind_generic_type_witness(
                        types,
                        expected_param.ty,
                        actual_param.ty,
                        generic_bindings,
                        shape_bindings,
                        seen,
                    )
            })
        && try_bind_generic_type_witness(
            types,
            expected_sig.ret,
            actual_sig.ret,
            generic_bindings,
            shape_bindings,
            seen,
        )
        && match (expected_sig.err, actual_sig.err) {
            (Some(expected_err), Some(actual_err)) => try_bind_generic_type_witness(
                types,
                expected_err,
                actual_err,
                generic_bindings,
                shape_bindings,
                seen,
            ),
            (None, None) => true,
            _ => false,
        }
}

// ---------------------------------------------------------------------------
// Existing-type instantiation
// ---------------------------------------------------------------------------

/// # Errors
/// Returns [`MirGenericError`] when a parameter is unbound or the instantiated
/// shape is not already interned in `types`.
pub fn instantiate_existing_generic_type(
    types: &TypeTable,
    ty: MirType,
    generic_bindings: &MirGenericBindings,
    span: Span,
) -> Result<MirType, MirGenericError> {
    instantiate_existing_generic_type_with_shapes(
        types,
        ty,
        generic_bindings,
        &MirShapeBindings::default(),
        span,
    )
}

/// Instantiate a generic type with read-only shape-param substitution.
///
/// Unlike [`instantiate_generic_type_with_shapes`], this validation helper
/// never interns a type or index. It only returns an already-existing semantic
/// type, which keeps validation a read-only walk over the caller's table.
///
/// # Errors
/// Returns [`MirGenericError`] when the substituted type is not already
/// represented in `types`.
pub fn instantiate_existing_generic_type_with_shapes(
    types: &TypeTable,
    ty: MirType,
    generic_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    span: Span,
) -> Result<MirType, MirGenericError> {
    let index = ExistingTypeIndex::build(types);
    instantiate_existing_generic_type_with_index(
        types,
        ty,
        generic_bindings,
        shape_bindings,
        span,
        &index,
    )
}

/// Read-only instantiation using a caller-held [`ExistingTypeIndex`].
///
/// `index` must have been built from the current contents of `types` (the
/// table is read-only here, so a validation-scoped index stays valid for the
/// whole walk). Result is identical to
/// [`instantiate_existing_generic_type_with_shapes`], without the per-call
/// full-table index build.
///
/// # Errors
/// Returns [`MirGenericError`] when the substituted type is not already
/// represented in `types`.
pub fn instantiate_existing_generic_type_with_index(
    types: &TypeTable,
    ty: MirType,
    generic_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    span: Span,
    index: &ExistingTypeIndex,
) -> Result<MirType, MirGenericError> {
    let mut walk = types;
    let instantiated = instantiate_type_id(
        &mut walk,
        ty.semantic_id(),
        generic_bindings,
        shape_bindings,
        span,
        index,
    )?;
    Ok(MirType::semantic(instantiated))
}

/// Instantiate a generic type with shape-param substitution (4A).
///
/// Substituted shapes (e.g. a transposed `[D,B]` → `[8,2]`) and their concrete
/// indexed types may be new to the unit, so this walk interns them into
/// `types` instead of requiring them to pre-exist. Monomorphization uses this
/// variant; type-param-only consumers keep the read-only
/// [`instantiate_existing_generic_type`].
///
/// # Errors
/// Returns [`MirGenericError`] when a type parameter is unbound.
pub fn instantiate_generic_type_with_shapes(
    types: &mut TypeTable,
    ty: MirType,
    generic_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    span: Span,
) -> Result<MirType, MirGenericError> {
    let index = ExistingTypeIndex::build(types);
    instantiate_generic_type_with_shapes_with_index(
        types,
        ty,
        generic_bindings,
        shape_bindings,
        span,
        &index,
    )
}

/// Instantiate a generic type with shape substitution using a batch-scoped
/// index. The index records newly interned types, so it remains equivalent to
/// rebuilding it from the growing type table while avoiding per-occurrence
/// full-table scans.
///
/// # Errors
/// Returns [`MirGenericError`] when a type parameter is unbound.
pub fn instantiate_generic_type_with_shapes_with_index(
    types: &mut TypeTable,
    ty: MirType,
    generic_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    span: Span,
    index: &ExistingTypeIndex,
) -> Result<MirType, MirGenericError> {
    let mut walk = types;
    let instantiated = instantiate_type_id(
        &mut walk,
        ty.semantic_id(),
        generic_bindings,
        shape_bindings,
        span,
        index,
    )?;
    Ok(MirType::semantic(instantiated))
}

/// Content-addressed index of interned types by structural fingerprint.
///
/// Fingerprints use child `TypeId` identity (not [`TypeTable::equals`]). Exact
/// shape hits are O(1); equals-only matches (alias peel, numeric coercion) fall
/// back to a linear `type_matches_candidate` scan so behavior is preserved.
/// The candidate memo keeps positive results permanently and drops misses when
/// the mutable path interns a new type.
pub struct ExistingTypeIndex {
    by_fingerprint: RefCell<FxHashMap<u64, Vec<TypeId>>>,
    candidate_resolutions: RefCell<FxHashMap<Type, Option<TypeId>>>,
}

impl ExistingTypeIndex {
    #[must_use]
    pub fn build(types: &TypeTable) -> Self {
        let mut by_fingerprint: FxHashMap<u64, Vec<TypeId>> = FxHashMap::default();
        for index in 0..types.type_count() {
            let id = type_id_at(index);
            let fingerprint = type_fingerprint(types.get(id));
            by_fingerprint.entry(fingerprint).or_default().push(id);
        }
        Self {
            by_fingerprint: RefCell::new(by_fingerprint),
            candidate_resolutions: RefCell::new(FxHashMap::default()),
        }
    }

    fn find(&self, types: &TypeTable, candidate: &Type) -> Option<TypeId> {
        if let Some(&resolution) = self.candidate_resolutions.borrow().get(candidate) {
            return resolution;
        }
        let fingerprint = type_fingerprint(candidate);
        let resolution = self
            .by_fingerprint
            .borrow()
            .get(&fingerprint)
            .into_iter()
            .flatten()
            .find_map(|&id| type_matches_candidate(types, id, candidate).then_some(id))
            .or_else(|| {
                // Equals-based matches can differ in child TypeId identity from
                // the fingerprint key; preserve prior semantics with a full scan.
                (0..types.type_count()).find_map(|index| {
                    let id = type_id_at(index);
                    type_matches_candidate(types, id, candidate).then_some(id)
                })
            });
        self.candidate_resolutions
            .borrow_mut()
            .insert(candidate.clone(), resolution);
        resolution
    }

    fn record_intern(&self, types: &TypeTable, candidate: &Type, id: TypeId) {
        self.candidate_resolutions
            .borrow_mut()
            .retain(|_, resolution| resolution.is_some());
        self.candidate_resolutions
            .borrow_mut()
            .insert(candidate.clone(), Some(id));
        self.by_fingerprint
            .borrow_mut()
            .entry(type_fingerprint(types.get(id)))
            .or_default()
            .push(id);
    }
}

fn type_id_at(index: usize) -> TypeId {
    // TypeId is a u32 arena index by design (radix-types::types).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "TypeId is a u32 arena index by design"
    )]
    TypeId(index as u32)
}

fn type_fingerprint(ty: &Type) -> u64 {
    let mut hasher = FxHasher::default();
    ty.hash(&mut hasher);
    hasher.finish()
}

trait InstantiateWalk {
    fn get(&self, ty: TypeId) -> Type;

    fn resolve(
        &mut self,
        candidate: &Type,
        span: Span,
        index: &ExistingTypeIndex,
    ) -> Result<TypeId, MirGenericError>;

    fn substitute_index(
        &mut self,
        index_id: IndexId,
        shape_bindings: &MirShapeBindings,
        span: Span,
    ) -> Result<IndexId, MirGenericError>;
}

impl InstantiateWalk for &TypeTable {
    fn get(&self, ty: TypeId) -> Type {
        (*self).get(ty).clone()
    }

    fn resolve(
        &mut self,
        candidate: &Type,
        span: Span,
        index: &ExistingTypeIndex,
    ) -> Result<TypeId, MirGenericError> {
        find_existing_type(self, candidate, span, index)
    }

    fn substitute_index(
        &mut self,
        index_id: IndexId,
        shape_bindings: &MirShapeBindings,
        span: Span,
    ) -> Result<IndexId, MirGenericError> {
        substitute_existing_index_params(self, index_id, shape_bindings, span)
    }
}

impl InstantiateWalk for &mut TypeTable {
    fn get(&self, ty: TypeId) -> Type {
        (**self).get(ty).clone()
    }

    fn resolve(
        &mut self,
        candidate: &Type,
        _span: Span,
        index: &ExistingTypeIndex,
    ) -> Result<TypeId, MirGenericError> {
        Ok(intern_or_find(self, candidate, index))
    }

    fn substitute_index(
        &mut self,
        index_id: IndexId,
        shape_bindings: &MirShapeBindings,
        _span: Span,
    ) -> Result<IndexId, MirGenericError> {
        Ok(substitute_index_params(self, index_id, shape_bindings))
    }
}

fn resolve_type<W: InstantiateWalk>(
    types: &mut W,
    candidate: &Type,
    span: Span,
    index: &ExistingTypeIndex,
) -> Result<TypeId, MirGenericError> {
    types.resolve(candidate, span, index)
}

fn instantiate_type_id<W: InstantiateWalk>(
    types: &mut W,
    ty: TypeId,
    generic_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    span: Span,
    index: &ExistingTypeIndex,
) -> Result<TypeId, MirGenericError> {
    match types.get(ty).clone() {
        Type::Param(sym) => generic_bindings
            .get(&sym)
            .map(|ty| ty.semantic_id())
            .ok_or_else(|| {
                MirGenericError::new(span, "generic call signature has unresolved type parameter")
            }),
        Type::Array(inner) => instantiate_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Array,
        ),
        Type::Map(key, value) => {
            let key =
                instantiate_type_id(types, key, generic_bindings, shape_bindings, span, index)?;
            let value =
                instantiate_type_id(types, value, generic_bindings, shape_bindings, span, index)?;
            resolve_type(types, &Type::Map(key, value), span, index)
        }
        Type::Ledger(key, value) => {
            let key =
                instantiate_type_id(types, key, generic_bindings, shape_bindings, span, index)?;
            let value =
                instantiate_type_id(types, value, generic_bindings, shape_bindings, span, index)?;
            resolve_type(types, &Type::Ledger(key, value), span, index)
        }
        Type::Record(fields) => {
            let fields = fields
                .iter()
                .map(|(name, field_ty)| {
                    instantiate_type_id(
                        types,
                        *field_ty,
                        generic_bindings,
                        shape_bindings,
                        span,
                        index,
                    )
                    .map(|field_ty| (*name, field_ty))
                })
                .collect::<Result<FxHashMap<_, _>, _>>()?;
            resolve_type(types, &Type::Record(fields), span, index)
        }
        Type::Tuple(elements) => {
            let elements = elements
                .iter()
                .map(|element| {
                    instantiate_type_id(
                        types,
                        *element,
                        generic_bindings,
                        shape_bindings,
                        span,
                        index,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            resolve_type(types, &Type::Tuple(elements), span, index)
        }
        Type::Set(inner) => instantiate_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Set,
        ),
        Type::Promissum(inner) => instantiate_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Promissum,
        ),
        Type::PromissumFailable(success, alternate) => {
            let success = instantiate_type_id(
                types,
                success,
                generic_bindings,
                shape_bindings,
                span,
                index,
            )?;
            let alternate = instantiate_type_id(
                types,
                alternate,
                generic_bindings,
                shape_bindings,
                span,
                index,
            )?;
            resolve_type(
                types,
                &Type::PromissumFailable(success, alternate),
                span,
                index,
            )
        }
        Type::Cursor(inner) => instantiate_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Cursor,
        ),
        Type::Queue(inner) => instantiate_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Queue,
        ),
        Type::Stack(inner) => instantiate_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Stack,
        ),
        Type::AsyncCursor(item, alternate) => {
            let item =
                instantiate_type_id(types, item, generic_bindings, shape_bindings, span, index)?;
            let alternate = instantiate_type_id(
                types,
                alternate,
                generic_bindings,
                shape_bindings,
                span,
                index,
            )?;
            resolve_type(types, &Type::AsyncCursor(item, alternate), span, index)
        }
        Type::Intervallum(inner, step) => {
            let inner =
                instantiate_type_id(types, inner, generic_bindings, shape_bindings, span, index)?;
            let step = types.substitute_index(step, shape_bindings, span)?;
            resolve_type(types, &Type::Intervallum(inner, step), span, index)
        }
        Type::Option(inner) => instantiate_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Option,
        ),
        Type::Ref(mode, inner) => {
            let inner =
                instantiate_type_id(types, inner, generic_bindings, shape_bindings, span, index)?;
            resolve_type(types, &Type::Ref(mode, inner), span, index)
        }
        Type::Tensor(inner, shape) => instantiate_indexed_type(
            types,
            inner,
            shape,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Tensor,
        ),
        Type::Vector(inner, width) => instantiate_indexed_type(
            types,
            inner,
            width,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Vector,
        ),
        Type::Matrix(inner, shape) => instantiate_indexed_type(
            types,
            inner,
            shape,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Matrix,
        ),
        Type::Sparsa(inner, shape) => instantiate_indexed_type(
            types,
            inner,
            shape,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Sparsa,
        ),
        Type::RangedArray { element, min, max } => {
            let element = instantiate_type_id(
                types,
                element,
                generic_bindings,
                shape_bindings,
                span,
                index,
            )?;
            resolve_type(types, &Type::RangedArray { element, min, max }, span, index)
        }
        Type::BoundedArray { element, capacity } => instantiate_indexed_type(
            types,
            element,
            capacity,
            generic_bindings,
            shape_bindings,
            span,
            index,
            |element, capacity| Type::BoundedArray { element, capacity },
        ),
        Type::BoundedQueue { element, capacity } => instantiate_indexed_type(
            types,
            element,
            capacity,
            generic_bindings,
            shape_bindings,
            span,
            index,
            |element, capacity| Type::BoundedQueue { element, capacity },
        ),
        Type::BoundedStack { element, capacity } => instantiate_indexed_type(
            types,
            element,
            capacity,
            generic_bindings,
            shape_bindings,
            span,
            index,
            |element, capacity| Type::BoundedStack { element, capacity },
        ),
        Type::BoundedTextus { capacity } => {
            let capacity = types.substitute_index(capacity, shape_bindings, span)?;
            resolve_type(types, &Type::BoundedTextus { capacity }, span, index)
        }
        Type::BoundedAscii { capacity } => {
            let capacity = types.substitute_index(capacity, shape_bindings, span)?;
            resolve_type(types, &Type::BoundedAscii { capacity }, span, index)
        }
        Type::BoundedOcteti { capacity } => {
            let capacity = types.substitute_index(capacity, shape_bindings, span)?;
            resolve_type(types, &Type::BoundedOcteti { capacity }, span, index)
        }
        Type::Atomic(inner) => instantiate_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Atomic,
        ),
        Type::Alias(def_id, inner) => {
            let inner =
                instantiate_type_id(types, inner, generic_bindings, shape_bindings, span, index)?;
            resolve_type(types, &Type::Alias(def_id, inner), span, index)
        }
        Type::Applied(base, args) => {
            let base =
                instantiate_type_id(types, base, generic_bindings, shape_bindings, span, index)?;
            let args = args
                .iter()
                .map(|arg| match arg {
                    radix_types::AppliedArg::Type(id) => instantiate_type_id(
                        types,
                        *id,
                        generic_bindings,
                        shape_bindings,
                        span,
                        index,
                    )
                    .map(radix_types::AppliedArg::Type),
                    radix_types::AppliedArg::Size(id) => types
                        .substitute_index(*id, shape_bindings, span)
                        .map(radix_types::AppliedArg::Size),
                })
                .collect::<Result<Vec<_>, _>>()?;
            resolve_type(types, &Type::Applied(base, args), span, index)
        }
        Type::Union(members) => {
            let members = members
                .iter()
                .map(|member| {
                    instantiate_type_id(
                        types,
                        *member,
                        generic_bindings,
                        shape_bindings,
                        span,
                        index,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            resolve_type(types, &Type::Union(members), span, index)
        }
        Type::Intersection(members) => {
            let members = members
                .iter()
                .map(|member| {
                    instantiate_type_id(
                        types,
                        *member,
                        generic_bindings,
                        shape_bindings,
                        span,
                        index,
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            resolve_type(types, &Type::Intersection(members), span, index)
        }
        Type::Func(sig) => {
            let sig = FuncSig {
                type_params: sig.type_params.clone(),
                params: sig
                    .params
                    .iter()
                    .map(|param| {
                        instantiate_type_id(
                            types,
                            param.ty,
                            generic_bindings,
                            shape_bindings,
                            span,
                            index,
                        )
                        .map(|ty| ParamType {
                            ty,
                            mode: param.mode,
                            optional: param.optional,
                            sponte: param.sponte,
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                ret: instantiate_type_id(
                    types,
                    sig.ret,
                    generic_bindings,
                    shape_bindings,
                    span,
                    index,
                )?,
                err: sig
                    .err
                    .map(|err| {
                        instantiate_type_id(
                            types,
                            err,
                            generic_bindings,
                            shape_bindings,
                            span,
                            index,
                        )
                    })
                    .transpose()?,
                is_async: sig.is_async,
                is_generator: sig.is_generator,
            };
            resolve_type(types, &Type::Func(sig), span, index)
        }
        Type::Primitive(_)
        | Type::TensorEdgePolicy(_)
        | Type::SizedNumeric(_, _)
        | Type::ModularWord(_)
        | Type::Bits(_)
        | Type::SaturatingWord(_)
        | Type::RangedInt { .. }
        | Type::PolicyFloat(_, _)
        | Type::SizedInstans(_)
        | Type::Struct(_)
        | Type::Enum(_)
        | Type::Interface(_)
        | Type::Infer(_)
        | Type::InferUnion(_)
        | Type::MarkerHole { .. }
        | Type::Error => Ok(ty),
    }
}

fn instantiate_unary_type<W: InstantiateWalk>(
    types: &mut W,
    inner: TypeId,
    generic_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    span: Span,
    index: &ExistingTypeIndex,
    build: impl FnOnce(TypeId) -> Type,
) -> Result<TypeId, MirGenericError> {
    let inner = instantiate_type_id(types, inner, generic_bindings, shape_bindings, span, index)?;
    resolve_type(types, &build(inner), span, index)
}

#[allow(clippy::too_many_arguments)]
fn instantiate_indexed_type<W: InstantiateWalk>(
    types: &mut W,
    inner: TypeId,
    index_id: IndexId,
    generic_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    span: Span,
    index: &ExistingTypeIndex,
    build: impl FnOnce(TypeId, IndexId) -> Type,
) -> Result<TypeId, MirGenericError> {
    let inner = instantiate_type_id(types, inner, generic_bindings, shape_bindings, span, index)?;
    let shape = types.substitute_index(index_id, shape_bindings, span)?;
    resolve_type(types, &build(inner, shape), span, index)
}

fn find_existing_type(
    types: &TypeTable,
    candidate: &Type,
    span: Span,
    index: &ExistingTypeIndex,
) -> Result<TypeId, MirGenericError> {
    index
        .find(types, candidate)
        .ok_or_else(|| MirGenericError::new(span, MISSING_SEMANTIC_METADATA))
}

// ---------------------------------------------------------------------------
// Shape-param substitution (4A)
// ---------------------------------------------------------------------------

/// Replace bound shape params in one index term without interning.
///
/// A changed tuple must already exist in the caller's type table. In
/// validation that is guaranteed by the concrete operand or destination type;
/// refusing to synthesize a new index keeps this helper read-only.
fn substitute_existing_index_params(
    types: &TypeTable,
    index_id: IndexId,
    shape_bindings: &MirShapeBindings,
    span: Span,
) -> Result<IndexId, MirGenericError> {
    match types.get_index(index_id).clone() {
        IndexExpr::Param(sym) => Ok(shape_bindings.get(&sym).copied().unwrap_or(index_id)),
        IndexExpr::Tuple(dims) => {
            let substituted = dims
                .iter()
                .map(|dim| substitute_existing_index_params(types, *dim, shape_bindings, span))
                .collect::<Result<Vec<_>, _>>()?;
            if substituted.iter().zip(dims.iter()).all(|(a, b)| a == b) {
                return Ok(index_id);
            }
            find_existing_index(types, &IndexExpr::Tuple(substituted), span)
        }
        IndexExpr::Affine { base, offset } => {
            let substituted = substitute_existing_index_params(types, base, shape_bindings, span)?;
            if substituted == base {
                return Ok(index_id);
            }
            let candidate = match radix_types::evaluate_index_dimension(substituted, types) {
                Ok(Some(value)) => {
                    let value = i128::from(value).checked_add(offset).ok_or_else(|| {
                        MirGenericError::new(span, "invalid affine shape arithmetic")
                    })?;
                    if value < 0 {
                        return Err(MirGenericError::new(span, "negative affine shape extent"));
                    }
                    IndexExpr::Literal(u64::try_from(value).map_err(|_| {
                        MirGenericError::new(span, "affine shape extent exceeds the natural range")
                    })?)
                }
                Ok(None) if offset == 0 => return Ok(substituted),
                Ok(None) => IndexExpr::Affine {
                    base: substituted,
                    offset,
                },
                Err(_) => {
                    return Err(MirGenericError::new(
                        span,
                        "invalid affine shape arithmetic",
                    ));
                }
            };
            find_existing_index(types, &candidate, span)
        }
        IndexExpr::Divide { numerator, divisor } => {
            let substituted =
                substitute_existing_index_params(types, numerator, shape_bindings, span)?;
            if substituted == numerator {
                return Ok(index_id);
            }
            let candidate = match radix_types::evaluate_index_dimension(substituted, types) {
                Ok(Some(value)) if divisor != 0 && value % divisor == 0 => {
                    IndexExpr::Literal(value / divisor)
                }
                Ok(Some(_)) | Err(_) => {
                    return Err(MirGenericError::new(
                        span,
                        "invalid exact-division shape arithmetic",
                    ));
                }
                Ok(None) if divisor == 1 => return Ok(substituted),
                Ok(None) => IndexExpr::Divide {
                    numerator: substituted,
                    divisor,
                },
            };
            find_existing_index(types, &candidate, span)
        }
        IndexExpr::Literal(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Infer(_)
        | IndexExpr::Unspecified => Ok(index_id),
    }
}

fn find_existing_index(
    types: &TypeTable,
    candidate: &IndexExpr,
    span: Span,
) -> Result<IndexId, MirGenericError> {
    (0..types.index_count())
        .map(index_id_at)
        .find(|id| index_expr_matches(types, *id, candidate))
        .ok_or_else(|| MirGenericError::new(span, MISSING_SEMANTIC_METADATA))
}

fn index_expr_matches(types: &TypeTable, id: IndexId, candidate: &IndexExpr) -> bool {
    match (types.get_index(id), candidate) {
        (IndexExpr::Literal(a), IndexExpr::Literal(b)) => a == b,
        (IndexExpr::Param(a), IndexExpr::Param(b)) => a == b,
        (IndexExpr::Runtime(a), IndexExpr::Runtime(b)) => a == b,
        (
            IndexExpr::Affine {
                base: a_base,
                offset: a_offset,
            },
            IndexExpr::Affine {
                base: b_base,
                offset: b_offset,
            },
        ) => a_offset == b_offset && types.index_equals(*a_base, *b_base),
        (
            IndexExpr::Divide {
                numerator: a_numerator,
                divisor: a_divisor,
            },
            IndexExpr::Divide {
                numerator: b_numerator,
                divisor: b_divisor,
            },
        ) => a_divisor == b_divisor && types.index_equals(*a_numerator, *b_numerator),
        (IndexExpr::Tuple(a), IndexExpr::Tuple(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b.iter())
                    .all(|(a, b)| types.index_equals(*a, *b))
        }
        (IndexExpr::Infer(a), IndexExpr::Infer(b)) => a == b,
        (IndexExpr::Unspecified, IndexExpr::Unspecified) => true,
        _ => false,
    }
}

/// Replace bound `IndexExpr::Param` nodes inside one index term.
///
/// Interning substituted tuple nodes needs `&mut TypeTable` (index nodes are
/// not content-addressed), so the mutable instantiation walk calls this while
/// holding `&mut`. Unbound params pass through unchanged.
pub fn substitute_index_params(
    types: &mut TypeTable,
    index_id: IndexId,
    shape_bindings: &MirShapeBindings,
) -> IndexId {
    match types.get_index(index_id).clone() {
        IndexExpr::Param(sym) => shape_bindings.get(&sym).copied().unwrap_or(index_id),
        IndexExpr::Tuple(dims) => {
            let mut substituted = Vec::with_capacity(dims.len());
            let mut changed = false;
            for dim in dims {
                let sub = substitute_index_params(types, dim, shape_bindings);
                changed |= sub != dim;
                substituted.push(sub);
            }
            if changed {
                types.intern_index(IndexExpr::Tuple(substituted))
            } else {
                index_id
            }
        }
        IndexExpr::Affine { base, offset } => {
            let base = substitute_index_params(types, base, shape_bindings);
            radix_types::add_index_constant(types, base, offset)
                .unwrap_or_else(|_| types.intern_index(IndexExpr::Affine { base, offset }))
        }
        IndexExpr::Divide { numerator, divisor } => {
            let numerator = substitute_index_params(types, numerator, shape_bindings);
            radix_types::divide_index_exact(types, numerator, divisor)
                .unwrap_or_else(|_| types.intern_index(IndexExpr::Divide { numerator, divisor }))
        }
        IndexExpr::Literal(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Infer(_)
        | IndexExpr::Unspecified => index_id,
    }
}

/// Intern-or-find for the mutable instantiation walk.
///
/// Concrete shape-generic types (e.g. a transposed tensor shape) may be new to
/// the unit; unlike the read-only path's "must already be interned" contract,
/// the first occurrence interns the candidate so the monomorphized instance
/// carries a valid type graph.
fn intern_or_find(types: &mut TypeTable, candidate: &Type, index: &ExistingTypeIndex) -> TypeId {
    if let Some(id) = index.find(types, candidate) {
        return id;
    }
    let id = types.intern(candidate.clone());
    index.record_intern(types, candidate, id);
    id
}

/// Canonical index identity for monomorphization keys.
///
/// Index arena entries are not content-addressed, so one concrete shape can
/// carry several `IndexId`s. Keys must not diverge on that, so each shape
/// binding is collapsed to the first structurally-equal arena entry. Returns
/// `id` itself when no earlier entry matches.
#[must_use]
pub fn canonical_index(types: &TypeTable, id: IndexId) -> IndexId {
    (0..types.index_count())
        .find_map(|index| {
            let candidate = index_id_at(index);
            types.index_equals(candidate, id).then_some(candidate)
        })
        .unwrap_or(id)
}

fn index_id_at(index: usize) -> IndexId {
    // IndexId is a u32 arena index by design (radix-types::index).
    #[expect(
        clippy::cast_possible_truncation,
        reason = "IndexId is a u32 arena index by design"
    )]
    IndexId(index as u32)
}

// ---------------------------------------------------------------------------
// Shape equality for interned candidates
// ---------------------------------------------------------------------------

fn type_matches_candidate(types: &TypeTable, id: TypeId, candidate: &Type) -> bool {
    match (types.get(id), candidate) {
        (Type::Primitive(a), Type::Primitive(b)) => a == b,
        (Type::SizedNumeric(ap, aw), Type::SizedNumeric(bp, bw)) => ap == bp && aw == bw,
        (Type::SizedInstans(a), Type::SizedInstans(b)) => a == b,
        (Type::Map(ak, av), Type::Map(bk, bv)) | (Type::Ledger(ak, av), Type::Ledger(bk, bv)) => {
            types.equals(*ak, *bk) && types.equals(*av, *bv)
        }
        (Type::Record(a), Type::Record(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(name, a_ty)| b.get(name).is_some_and(|b_ty| types.equals(*a_ty, *b_ty)))
        }
        (Type::Array(a), Type::Array(b))
        | (Type::Set(a), Type::Set(b))
        | (Type::Promissum(a), Type::Promissum(b))
        | (Type::Cursor(a), Type::Cursor(b))
        | (Type::Queue(a), Type::Queue(b))
        | (Type::Stack(a), Type::Stack(b))
        | (Type::Intervallum(a, _), Type::Intervallum(b, _))
        | (Type::Option(a), Type::Option(b))
        | (Type::Atomic(a), Type::Atomic(b)) => types.equals(*a, *b),
        (Type::AsyncCursor(a_item, a_alternate), Type::AsyncCursor(b_item, b_alternate)) => {
            types.equals(*a_item, *b_item) && types.equals(*a_alternate, *b_alternate)
        }
        (
            Type::PromissumFailable(a_success, a_alternate),
            Type::PromissumFailable(b_success, b_alternate),
        ) => types.equals(*a_success, *b_success) && types.equals(*a_alternate, *b_alternate),
        (Type::Tensor(a_ty, a_shape), Type::Tensor(b_ty, b_shape))
        | (Type::Vector(a_ty, a_shape), Type::Vector(b_ty, b_shape))
        | (Type::Matrix(a_ty, a_shape), Type::Matrix(b_ty, b_shape))
        | (Type::Sparsa(a_ty, a_shape), Type::Sparsa(b_ty, b_shape)) => {
            types.equals(*a_ty, *b_ty) && types.index_equals(*a_shape, *b_shape)
        }
        (
            Type::RangedArray {
                element: a_elem,
                min: a_min,
                max: a_max,
            },
            Type::RangedArray {
                element: b_elem,
                min: b_min,
                max: b_max,
            },
        ) => types.equals(*a_elem, *b_elem) && a_min == b_min && a_max == b_max,
        (
            Type::BoundedArray {
                element: a_elem,
                capacity: a_cap,
            },
            Type::BoundedArray {
                element: b_elem,
                capacity: b_cap,
            },
        )
        | (
            Type::BoundedQueue {
                element: a_elem,
                capacity: a_cap,
            },
            Type::BoundedQueue {
                element: b_elem,
                capacity: b_cap,
            },
        )
        | (
            Type::BoundedStack {
                element: a_elem,
                capacity: a_cap,
            },
            Type::BoundedStack {
                element: b_elem,
                capacity: b_cap,
            },
        ) => types.equals(*a_elem, *b_elem) && types.index_equals(*a_cap, *b_cap),
        (Type::BoundedTextus { capacity: a }, Type::BoundedTextus { capacity: b })
        | (Type::BoundedAscii { capacity: a }, Type::BoundedAscii { capacity: b })
        | (Type::BoundedOcteti { capacity: a }, Type::BoundedOcteti { capacity: b }) => {
            types.index_equals(*a, *b)
        }
        (Type::Ref(a_mode, a_ty), Type::Ref(b_mode, b_ty)) => {
            a_mode == b_mode && types.equals(*a_ty, *b_ty)
        }
        (Type::Struct(a), Type::Struct(b))
        | (Type::Enum(a), Type::Enum(b))
        | (Type::Interface(a), Type::Interface(b)) => a == b,
        (Type::Alias(a_def, a_inner), Type::Alias(b_def, b_inner)) => {
            a_def == b_def && types.equals(*a_inner, *b_inner)
        }
        (Type::Func(a), Type::Func(b)) => function_signature_matches(types, a, b),
        (Type::Applied(a_base, a_args), Type::Applied(b_base, b_args)) => {
            applied_type_args_match(types, *a_base, *b_base, a_args, b_args)
        }
        (Type::Union(a), Type::Union(b)) | (Type::Intersection(a), Type::Intersection(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|a_ty| b.iter().any(|b_ty| types.equals(*a_ty, *b_ty)))
        }
        (Type::Tuple(a_elems), Type::Tuple(b_elems)) => {
            a_elems.len() == b_elems.len()
                && a_elems
                    .iter()
                    .zip(b_elems.iter())
                    .all(|(a, b)| types.equals(*a, *b))
        }
        (Type::Param(a), Type::Param(b)) => a == b,
        (Type::Infer(a), Type::Infer(b)) => a == b,
        (Type::Error, Type::Error) => true,
        _ => false,
    }
}

fn applied_type_args_match(
    types: &TypeTable,
    a_base: TypeId,
    b_base: TypeId,
    a_args: &[radix_types::AppliedArg],
    b_args: &[radix_types::AppliedArg],
) -> bool {
    types.equals(a_base, b_base)
        && a_args.len() == b_args.len()
        && a_args
            .iter()
            .zip(b_args.iter())
            .all(|(a_arg, b_arg)| match (a_arg, b_arg) {
                (radix_types::AppliedArg::Type(a), radix_types::AppliedArg::Type(b)) => {
                    types.equals(*a, *b)
                }
                (radix_types::AppliedArg::Size(a), radix_types::AppliedArg::Size(b)) => {
                    types.index_equals(*a, *b)
                }
                _ => false,
            })
}

fn function_signature_matches(types: &TypeTable, a: &FuncSig, b: &FuncSig) -> bool {
    a.params.len() == b.params.len()
        && a.params
            .iter()
            .zip(b.params.iter())
            .all(|(a_param, b_param)| {
                a_param.mode == b_param.mode
                    && a_param.optional == b_param.optional
                    && a_param.sponte == b_param.sponte
                    && types.equals(a_param.ty, b_param.ty)
            })
        && types.equals(a.ret, b.ret)
        && match (a.err, b.err) {
            (Some(a_err), Some(b_err)) => types.equals(a_err, b_err),
            (None, None) => true,
            _ => false,
        }
        && a.is_async == b.is_async
        && a.is_generator == b.is_generator
}

// ---------------------------------------------------------------------------
// Param occupancy
// ---------------------------------------------------------------------------

#[must_use]
pub fn type_contains_param(types: &TypeTable, ty: TypeId) -> bool {
    let mut seen = FxHashSet::default();
    type_contains_param_inner(types, ty, &mut seen)
}

fn type_contains_param_inner(types: &TypeTable, ty: TypeId, seen: &mut FxHashSet<TypeId>) -> bool {
    if !seen.insert(ty) {
        return false;
    }
    let ty = types.get(ty);
    match ty {
        Type::Param(_) => true,
        Type::Tensor(inner, shape)
        | Type::Vector(inner, shape)
        | Type::Matrix(inner, shape)
        | Type::Sparsa(inner, shape) => {
            index_contains_param(types, *shape) || type_contains_param_inner(types, *inner, seen)
        }
        Type::Array(inner)
        | Type::Set(inner)
        | Type::Promissum(inner)
        | Type::Cursor(inner)
        | Type::Queue(inner)
        | Type::Stack(inner)
        | Type::Atomic(inner)
        | Type::Intervallum(inner, _)
        | Type::Option(inner)
        | Type::Ref(_, inner)
        | Type::Alias(_, inner) => type_contains_param_inner(types, *inner, seen),
        Type::RangedArray { element, .. } => type_contains_param_inner(types, *element, seen),
        Type::BoundedArray { element, capacity }
        | Type::BoundedQueue { element, capacity }
        | Type::BoundedStack { element, capacity } => {
            index_contains_param(types, *capacity)
                || type_contains_param_inner(types, *element, seen)
        }
        Type::BoundedTextus { capacity }
        | Type::BoundedAscii { capacity }
        | Type::BoundedOcteti { capacity } => index_contains_param(types, *capacity),
        Type::Map(key, value)
        | Type::Ledger(key, value)
        | Type::PromissumFailable(key, value)
        | Type::AsyncCursor(key, value) => {
            type_contains_param_inner(types, *key, seen)
                || type_contains_param_inner(types, *value, seen)
        }
        Type::Record(fields) => fields
            .values()
            .any(|field_ty| type_contains_param_inner(types, *field_ty, seen)),
        Type::Tuple(elements) => elements
            .iter()
            .any(|element| type_contains_param_inner(types, *element, seen)),
        Type::Func(sig) => {
            sig.params
                .iter()
                .any(|param| type_contains_param_inner(types, param.ty, seen))
                || type_contains_param_inner(types, sig.ret, seen)
                || sig
                    .err
                    .is_some_and(|err| type_contains_param_inner(types, err, seen))
        }
        Type::Applied(base, args) => {
            type_contains_param_inner(types, *base, seen)
                || args.iter().any(|arg| match arg {
                    radix_types::AppliedArg::Type(id) => {
                        type_contains_param_inner(types, *id, seen)
                    }
                    radix_types::AppliedArg::Size(id) => index_contains_param(types, *id),
                })
        }
        Type::Union(members) | Type::Intersection(members) => members
            .iter()
            .any(|member| type_contains_param_inner(types, *member, seen)),
        Type::Primitive(_)
        | Type::TensorEdgePolicy(_)
        | Type::SizedNumeric(_, _)
        | Type::ModularWord(_)
        | Type::Bits(_)
        | Type::SaturatingWord(_)
        | Type::RangedInt { .. }
        | Type::PolicyFloat(_, _)
        | Type::SizedInstans(_)
        | Type::Struct(_)
        | Type::Enum(_)
        | Type::Interface(_)
        | Type::Infer(_)
        | Type::InferUnion(_)
        | Type::MarkerHole { .. }
        | Type::Error => false,
    }
}

/// Whether an index term contains a `magnitudo` shape param (4A).
fn index_contains_param(types: &TypeTable, id: IndexId) -> bool {
    match types.get_index(id) {
        IndexExpr::Param(_) => true,
        IndexExpr::Tuple(dims) => dims.iter().any(|dim| index_contains_param(types, *dim)),
        IndexExpr::Affine { base, .. } => index_contains_param(types, *base),
        IndexExpr::Divide { numerator, .. } => index_contains_param(types, *numerator),
        IndexExpr::Literal(_)
        | IndexExpr::Runtime(_)
        | IndexExpr::Infer(_)
        | IndexExpr::Unspecified => false,
    }
}

// ---------------------------------------------------------------------------
// Shape-size scalar substitution (Phase-4 producer fact)
// ---------------------------------------------------------------------------

/// Rewrite every `MirConstant` reachable from a function body — operands,
/// place index projections, closure/callee payloads, aggregate fields,
/// terminator operands, and switch-case dispatch values — in deterministic
/// storage order.
///
/// `rewrite` replaces a constant in place; returning `false` aborts the walk
/// early. Centralizes operand/constant traversal so monomorphization and
/// validation share one seam instead of growing per-shape matches.
#[must_use]
pub fn rewrite_function_constants(
    function: &mut MirFunction,
    rewrite: &mut impl FnMut(&mut MirConstant) -> bool,
) -> bool {
    for block in &mut function.blocks {
        for statement in &mut block.statements {
            if !rewrite_statement_constants(statement, rewrite) {
                return false;
            }
        }
        if !rewrite_terminator_constants(&mut block.terminator, rewrite) {
            return false;
        }
    }
    true
}

/// Fold one shape-size scalar term to its concrete constant: a bare `Param`
/// size scalar (`axis: None`) substitutes to the bound integer
/// (`MirConstant::Int` — size operands such as rope dims are integers),
/// while tuple shapes fold to the element count (`axis: None`) or one axis
/// extent as `MirConstant::Float`. `magnitudo` shape params substitute
/// through `shape_bindings` first. Returns `None` — the fail-closed signal
/// — when any dim stays symbolic (`Param`, `Infer`, `Unspecified`) or the
/// axis is out of range. Extent folds mirror the AIR fold semantics
/// (`AirExpr::ShapeSize` → `AirLiteral::Float`), including the f64 carrier.
#[must_use]
pub fn fold_shape_size_constant(
    types: &mut TypeTable,
    shape: IndexId,
    axis: Option<i64>,
    shape_bindings: &MirShapeBindings,
) -> Option<MirConstant> {
    let size_param = axis.is_none() && matches!(types.get_index(shape), IndexExpr::Param(_));
    let substituted = substitute_index_params(types, shape, shape_bindings);
    let dims = i64s_from_literal_index(substituted, types)?;
    let value = match axis {
        None => dims.iter().product::<i64>(),
        Some(a) => *dims.get(axis_extent_index(a, dims.len())?)?,
    };
    if size_param {
        return Some(MirConstant::Int(value));
    }
    // Semantic: extent and tuple-size folds keep the concrete size as f64
    // (the same carrier the literal companion path always emitted and the
    // lista→tensor conversion consumes); precision loss only above 2^53.
    #[expect(clippy::cast_precision_loss)]
    Some(MirConstant::Float(value as f64))
}

/// Resolve a possibly-negative axis against a rank, failing closed when the
/// axis is out of range or its magnitude does not fit in `usize`.
fn axis_extent_index(axis: i64, rank: usize) -> Option<usize> {
    if axis >= 0 {
        // Guarded: `axis >= 0`; `try_from` rejects values beyond usize
        // instead of truncating on 32-bit targets.
        usize::try_from(axis).ok()
    } else {
        // `checked_neg` fails closed for `axis == i64::MIN` instead of
        // panicking on the negation in debug builds.
        let neg = usize::try_from(axis.checked_neg()?).ok()?;
        rank.checked_sub(neg)
    }
}

/// Substitute every `MirConstant::ShapeSize` term in a function body to its
/// concrete constant from `shape_bindings` (Phase-4 monomorphization
/// producer fact): the bound integer for a bare size param, the f64 value
/// for extent folds. Terms that cannot fold — missing or
/// unresolved shape bindings, `Infer`/`Unspecified` dims, or out-of-range
/// axes — are left untouched and `false` is returned so the caller can fail
/// closed through one stable diagnostic.
#[must_use]
pub fn substitute_shape_size_constants(
    types: &mut TypeTable,
    function: &mut MirFunction,
    shape_bindings: &MirShapeBindings,
) -> bool {
    rewrite_function_constants(function, &mut |constant| match constant {
        MirConstant::ShapeSize { shape, axis } => {
            match fold_shape_size_constant(types, *shape, *axis, shape_bindings) {
                Some(folded) => {
                    *constant = folded;
                    true
                }
                None => false,
            }
        }
        _ => true,
    })
}

/// Repair `ShapeSize` operands in a `TensorFromFlat` shape aggregate after a
/// package merge. `ShapeSize` carries an `IndexId`, while package type-table
/// import remaps the surrounding tensor type; a source-local index can
/// therefore point at an unrelated symbolic node in the merged table. The
/// tensor result's shape is the authoritative shape for each axis of the
/// shape aggregate produced by `lista ↦ tensor`, and it is already finalized
/// from the instance key at this point.
///
/// Terms outside this producer remain untouched and are still handled by
/// [`substitute_shape_size_constants`]. Returning `false` preserves the
/// caller's fail-closed survivor net when the result shape itself is not
/// concrete or an axis cannot be folded.
#[must_use]
pub fn substitute_tensor_from_flat_shape_size_constants(
    types: &mut TypeTable,
    function: &mut MirFunction,
    shape_bindings: &MirShapeBindings,
) -> bool {
    let mut all_folded = true;
    for block in &mut function.blocks {
        let shape_targets = block
            .statements
            .iter()
            .filter_map(|statement| {
                let MirStatementKind::RuntimeCall { call, .. } = &statement.kind else {
                    return None;
                };
                let MirIntrinsic::Collection(MirCollectionOp::TensorFromFlat) = &call.intrinsic
                else {
                    return None;
                };
                let MirOperand::Temp(shape_temp) = call.args.get(2)? else {
                    return None;
                };
                let shape = tensor_shape(types, call.return_ty)?;
                Some((*shape_temp, shape))
            })
            .collect::<Vec<_>>();

        for (shape_temp, target_shape) in shape_targets {
            let Some(aggregate) = block.statements.iter().find_map(|statement| {
                let MirStatementKind::Construct {
                    destination,
                    aggregate,
                } = &statement.kind
                else {
                    return None;
                };
                matches!(&destination.base, crate::MirPlaceBase::Temp(temp) if *temp == shape_temp)
                    .then_some(aggregate)
            }) else {
                all_folded = false;
                continue;
            };
            let dimension_temps = {
                let MirAggregateFields::Ordered(items) = &aggregate.fields else {
                    all_folded = false;
                    continue;
                };
                items
                    .iter()
                    .map(|item| match item {
                        MirAggregateItem::Operand(MirOperand::Temp(temp)) => Some(*temp),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            };
            for dimension_temp in dimension_temps {
                let Some(dimension_temp) = dimension_temp else {
                    all_folded = false;
                    continue;
                };
                let Some(call) = block.statements.iter_mut().find_map(|statement| {
                    let MirStatementKind::RuntimeCall { destination, call } = &mut statement.kind
                    else {
                        return None;
                    };
                    matches!(
                        destination.as_ref().map(|place| &place.base),
                        Some(crate::MirPlaceBase::Temp(temp)) if *temp == dimension_temp
                    )
                    .then_some(call)
                }) else {
                    all_folded = false;
                    continue;
                };
                let Some(MirOperand::Constant(MirConstant::ShapeSize { axis, .. })) =
                    call.args.first_mut()
                else {
                    all_folded = false;
                    continue;
                };
                let Some(folded) =
                    fold_shape_size_constant(types, target_shape, *axis, shape_bindings)
                else {
                    all_folded = false;
                    continue;
                };
                call.args[0] = MirOperand::Constant(folded);
            }
        }
    }
    all_folded
}

fn tensor_shape(types: &TypeTable, ty: MirType) -> Option<IndexId> {
    let mut current = ty.semantic_id();
    let mut seen = FxHashSet::default();
    loop {
        if !seen.insert(current) {
            return None;
        }
        match types.get(current) {
            Type::Tensor(_, shape) => return Some(*shape),
            Type::Alias(_, target) => current = *target,
            _ => return None,
        }
    }
}

/// Whether a function body still contains a `MirConstant::ShapeSize` term in
/// any operand/constant position — the post-monomorphization survivor signal.
#[must_use]
pub fn function_contains_shape_size(function: &mut MirFunction) -> bool {
    // The rewriter returns `false` as soon as the visitor does, so a `false`
    // result means a `ShapeSize` term was seen.
    !rewrite_function_constants(function, &mut |constant| {
        !matches!(constant, MirConstant::ShapeSize { .. })
    })
}

fn rewrite_statement_constants(
    statement: &mut MirStatement,
    rewrite: &mut impl FnMut(&mut MirConstant) -> bool,
) -> bool {
    match &mut statement.kind {
        MirStatementKind::Assign { place, value } => {
            rewrite_place_constants(place, rewrite) && rewrite_value_constants(value, rewrite)
        }
        MirStatementKind::Call {
            destination,
            callee,
            args,
        } => {
            destination
                .as_mut()
                .is_none_or(|place| rewrite_place_constants(place, rewrite))
                && rewrite_callee_constants(callee, rewrite)
                && args
                    .iter_mut()
                    .all(|arg| rewrite_operand_constants(arg, rewrite))
        }
        MirStatementKind::RuntimeCall { destination, call } => {
            destination
                .as_mut()
                .is_none_or(|place| rewrite_place_constants(place, rewrite))
                && rewrite_runtime_call_constants(call, rewrite)
        }
        MirStatementKind::Construct {
            destination,
            aggregate,
        } => {
            rewrite_place_constants(destination, rewrite)
                && rewrite_aggregate_constants(aggregate, rewrite)
        }
    }
}

fn rewrite_terminator_constants(
    terminator: &mut MirTerminator,
    rewrite: &mut impl FnMut(&mut MirConstant) -> bool,
) -> bool {
    match &mut terminator.kind {
        MirTerminatorKind::Return(Some(operand)) | MirTerminatorKind::ReturnError(operand) => {
            rewrite_operand_constants(operand, rewrite)
        }
        MirTerminatorKind::TryCall {
            destination,
            callee,
            args,
            error_place,
            ..
        } => {
            destination
                .as_mut()
                .is_none_or(|place| rewrite_place_constants(place, rewrite))
                && rewrite_callee_constants(callee, rewrite)
                && args
                    .iter_mut()
                    .all(|arg| rewrite_operand_constants(arg, rewrite))
                && rewrite_place_constants(error_place, rewrite)
        }
        MirTerminatorKind::Branch { condition, .. } => {
            rewrite_operand_constants(condition, rewrite)
        }
        MirTerminatorKind::Switch { value, cases, .. } => {
            rewrite_operand_constants(value, rewrite)
                && cases.iter_mut().all(|case| rewrite(&mut case.value))
        }
        MirTerminatorKind::Return(None)
        | MirTerminatorKind::ReturnThread
        | MirTerminatorKind::Goto(_)
        | MirTerminatorKind::Unreachable => true,
    }
}

fn rewrite_value_constants(
    value: &mut MirValue,
    rewrite: &mut impl FnMut(&mut MirConstant) -> bool,
) -> bool {
    match &mut value.kind {
        MirValueKind::Operand(operand) | MirValueKind::Unary { operand, .. } => {
            rewrite_operand_constants(operand, rewrite)
        }
        MirValueKind::Closure(closure) => {
            rewrite_operand_constants(&mut closure.environment, rewrite)
        }
        MirValueKind::Binary { lhs, rhs, .. } => {
            rewrite_operand_constants(lhs, rewrite) && rewrite_operand_constants(rhs, rewrite)
        }
        MirValueKind::Option(op) => rewrite_option_op_constants(op, rewrite),
        MirValueKind::TensorExtent { tensor, .. } => rewrite_operand_constants(tensor, rewrite),
    }
}

fn rewrite_operand_constants(
    operand: &mut MirOperand,
    rewrite: &mut impl FnMut(&mut MirConstant) -> bool,
) -> bool {
    match operand {
        MirOperand::Place(place) => rewrite_place_constants(place, rewrite),
        MirOperand::Temp(_) | MirOperand::Value(_) => true,
        MirOperand::Constant(constant) => rewrite(constant),
    }
}

fn rewrite_place_constants(
    place: &mut MirPlace,
    rewrite: &mut impl FnMut(&mut MirConstant) -> bool,
) -> bool {
    for projection in &mut place.projections {
        if let MirProjection::Index(index) | MirProjection::ProvenIndex(index) = projection
            && !rewrite_operand_constants(index, rewrite)
        {
            return false;
        }
    }
    true
}

fn rewrite_callee_constants(
    callee: &mut MirCallee,
    rewrite: &mut impl FnMut(&mut MirConstant) -> bool,
) -> bool {
    match callee {
        MirCallee::Closure(closure) => rewrite_operand_constants(&mut closure.environment, rewrite),
        MirCallee::Value(operand) => rewrite_operand_constants(operand, rewrite),
        MirCallee::Function(_) | MirCallee::Definition { .. } => true,
    }
}

fn rewrite_runtime_call_constants(
    call: &mut MirRuntimeCall,
    rewrite: &mut impl FnMut(&mut MirConstant) -> bool,
) -> bool {
    call.args
        .iter_mut()
        .all(|arg| rewrite_operand_constants(arg, rewrite))
        && match &mut call.intrinsic {
            MirIntrinsic::Convert(conversion) => {
                conversion
                    .recovery
                    .as_mut()
                    .is_none_or(|recovery| rewrite_operand_constants(recovery, rewrite))
                    && conversion.struct_defaults.iter_mut().all(|defaults| {
                        defaults
                            .fields
                            .iter_mut()
                            .all(|field| rewrite_operand_constants(&mut field.value, rewrite))
                    })
            }
            _ => true,
        }
}

fn rewrite_aggregate_constants(
    aggregate: &mut MirAggregate,
    rewrite: &mut impl FnMut(&mut MirConstant) -> bool,
) -> bool {
    match &mut aggregate.fields {
        MirAggregateFields::Ordered(items) => items.iter_mut().all(|item| match item {
            MirAggregateItem::Operand(operand) | MirAggregateItem::Spread(operand) => {
                rewrite_operand_constants(operand, rewrite)
            }
        }),
        MirAggregateFields::Named(items) => items
            .iter_mut()
            .all(|item| rewrite_operand_constants(&mut item.value, rewrite)),
        MirAggregateFields::Keyed(items) => items.iter_mut().all(|item| {
            rewrite_operand_constants(&mut item.key, rewrite)
                && rewrite_operand_constants(&mut item.value, rewrite)
        }),
    }
}

fn rewrite_option_op_constants(
    op: &mut MirOptionOp,
    rewrite: &mut impl FnMut(&mut MirConstant) -> bool,
) -> bool {
    match op {
        MirOptionOp::None => true,
        MirOptionOp::Some(value)
        | MirOptionOp::IsNil(value)
        | MirOptionOp::IsNotNil(value)
        | MirOptionOp::Unwrap { value, .. } => rewrite_operand_constants(value, rewrite),
        MirOptionOp::Coalesce { value, fallback } => {
            rewrite_operand_constants(value, rewrite)
                && rewrite_operand_constants(fallback, rewrite)
        }
        MirOptionOp::Chain { base, link } => {
            rewrite_operand_constants(base, rewrite)
                && rewrite_option_chain_link_constants(link, rewrite)
        }
    }
}

fn rewrite_option_chain_link_constants(
    link: &mut MirOptionChainLink,
    rewrite: &mut impl FnMut(&mut MirConstant) -> bool,
) -> bool {
    match link {
        MirOptionChainLink::Field(_) | MirOptionChainLink::VariantField { .. } => true,
        MirOptionChainLink::Index(operand) => rewrite_operand_constants(operand, rewrite),
        MirOptionChainLink::Call { callee, args } => {
            rewrite_callee_constants(callee, rewrite)
                && args
                    .iter_mut()
                    .all(|arg| rewrite_operand_constants(arg, rewrite))
        }
    }
}

#[cfg(test)]
#[path = "generic_test.rs"]
mod tests;
