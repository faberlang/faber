//! Shared MIR generic binding and type-substitution helpers.
//!
//! Validation and monomorphization both need the same structural witness rules:
//! call-site values bind `Type::Param` occurrences inside semantic MIR types,
//! then concrete functions consume the instantiated type graph. Keep that logic
//! here so target emitters never grow their own generic policies.

use crate::MirType;
use radix_lexer::{Span, Symbol};
use radix_types::{FuncSig, IndexExpr, IndexId, ParamType, Type, TypeId, TypeTable};
use rustc_hash::{FxHashMap, FxHashSet, FxHasher};
use std::hash::{Hash, Hasher};

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
    bind_generic_type_witness_with_shapes(types, expected, actual, generic_bindings, &mut shape_bindings)
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
        let ok = try_bind_generic_type_witness(types, inner, actual, generic_bindings, shape_bindings, seen);
        seen.remove(&expected);
        return ok;
    }

    if let Type::Alias(_, inner) = types.get(actual) {
        if !seen.insert(actual) {
            return false;
        }
        let inner = *inner;
        let ok = try_bind_generic_type_witness(types, expected, inner, generic_bindings, shape_bindings, seen);
        seen.remove(&actual);
        return ok;
    }

    // Neither side is Alias — nullable_inner will not walk an Alias cycle here.
    if let Some(expected_inner) = types.nullable_inner(expected) {
        if let Some(actual_inner) = types.nullable_inner(actual) {
            return try_bind_generic_type_witness(
                types,
                expected_inner,
                actual_inner,
                generic_bindings,
                shape_bindings,
                seen,
            );
        }
    }

    match (types.get(expected), types.get(actual)) {
        (Type::Array(expected_inner), Type::Array(actual_inner))
        | (Type::Set(expected_inner), Type::Set(actual_inner))
        | (Type::Promissum(expected_inner), Type::Promissum(actual_inner))
        | (Type::Cursor(expected_inner), Type::Cursor(actual_inner))
        | (Type::Intervallum(expected_inner), Type::Intervallum(actual_inner))
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
        (Type::Map(expected_key, expected_value), Type::Map(actual_key, actual_value)) => {
            try_bind_generic_type_witness(types, *expected_key, *actual_key, generic_bindings, shape_bindings, seen)
                && try_bind_generic_type_witness(
                    types,
                    *expected_value,
                    *actual_value,
                    generic_bindings,
                    shape_bindings,
                    seen,
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
                    .all(|(expected_arg, actual_arg)| {
                        try_bind_generic_type_witness(
                            types,
                            *expected_arg,
                            *actual_arg,
                            generic_bindings,
                            shape_bindings,
                            seen,
                        )
                    })
        }
        (Type::Union(expected_members), Type::Union(actual_members)) => try_bind_union_members(
            types,
            expected_members,
            actual_members,
            generic_bindings,
            shape_bindings,
            seen,
        ),
        (Type::Func(expected_sig), Type::Func(actual_sig)) => {
            try_bind_func_signature(types, expected_sig, actual_sig, generic_bindings, shape_bindings, seen)
        }
        _ => types.assignable(actual, expected),
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
        (IndexExpr::Tuple(expected_dims), IndexExpr::Tuple(actual_dims)) => {
            expected_dims.len() == actual_dims.len()
                && expected_dims
                    .iter()
                    .zip(actual_dims.iter())
                    .all(|(expected_dim, actual_dim)| {
                        try_bind_index_witness(types, *expected_dim, *actual_dim, shape_bindings)
                    })
        }
        _ => types.index_equals(expected, actual),
    }
}

/// Bind a shape parameter to a concrete index node.
///
/// Insert-only: when `param` is already bound, compares for structural index
/// equality and never overwrites (mirrors [`bind_type_param`] so union
/// backtracking can roll back via [`undo_shape_bindings_to`]).
fn bind_shape_param(types: &TypeTable, param: Symbol, actual: IndexId, shape_bindings: &mut MirShapeBindings) -> bool {
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
            undo_shape_bindings_to(shape_bindings, &shape_keys_before);
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
            matched[actual_index] = false;
            return true;
        }

        undo_bindings_to(generic_bindings, &keys_before);
        undo_shape_bindings_to(shape_bindings, &shape_keys_before);
        matched[actual_index] = false;
    }

    false
}

/// Remove any keys inserted after `keys_before` was captured.
///
/// Safe only while bindings are insert-only (see [`bind_type_param`]).
fn undo_bindings_to(generic_bindings: &mut MirGenericBindings, keys_before: &FxHashSet<Symbol>) {
    generic_bindings.retain(|key, _| keys_before.contains(key));
}

/// Shape-binding rollback mirroring [`undo_bindings_to`].
fn undo_shape_bindings_to(shape_bindings: &mut MirShapeBindings, keys_before: &FxHashSet<Symbol>) {
    shape_bindings.retain(|key, _| keys_before.contains(key));
}

fn try_bind_func_signature(
    types: &TypeTable,
    expected_sig: &FuncSig,
    actual_sig: &FuncSig,
    generic_bindings: &mut MirGenericBindings,
    shape_bindings: &mut MirShapeBindings,
    seen: &mut FxHashSet<TypeId>,
) -> bool {
    expected_sig.params.len() == actual_sig.params.len()
        && expected_sig
            .params
            .iter()
            .zip(actual_sig.params.iter())
            .all(|(expected_param, actual_param)| {
                expected_param.mode == actual_param.mode
                    && expected_param.optional == actual_param.optional
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
    let index = ExistingTypeIndex::build(types);
    let instantiated =
        instantiate_existing_type_id(types, ty.semantic_id(), generic_bindings, span, &index)?;
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
    let instantiated =
        instantiate_mutable_type_id(types, ty.semantic_id(), generic_bindings, shape_bindings, span, &index)?;
    Ok(MirType::semantic(instantiated))
}

/// Content-addressed index of interned types by structural fingerprint.
///
/// Fingerprints use child `TypeId` identity (not [`TypeTable::equals`]). Exact
/// shape hits are O(1); equals-only matches (alias peel, numeric coercion) fall
/// back to a linear `type_matches_candidate` scan so behavior is preserved.
struct ExistingTypeIndex {
    by_fingerprint: FxHashMap<u64, Vec<TypeId>>,
}

impl ExistingTypeIndex {
    fn build(types: &TypeTable) -> Self {
        let mut by_fingerprint: FxHashMap<u64, Vec<TypeId>> = FxHashMap::default();
        for index in 0..types.type_count() {
            let id = type_id_at(index);
            let fingerprint = type_fingerprint(types.get(id));
            by_fingerprint.entry(fingerprint).or_default().push(id);
        }
        Self { by_fingerprint }
    }

    fn find(&self, types: &TypeTable, candidate: &Type) -> Option<TypeId> {
        let fingerprint = type_fingerprint(candidate);
        if let Some(ids) = self.by_fingerprint.get(&fingerprint) {
            for &id in ids {
                if type_matches_candidate(types, id, candidate) {
                    return Some(id);
                }
            }
        }
        // Equals-based matches can differ in child TypeId identity from the
        // fingerprint key; preserve prior semantics with a full scan.
        (0..types.type_count()).find_map(|index| {
            let id = type_id_at(index);
            type_matches_candidate(types, id, candidate).then_some(id)
        })
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

fn instantiate_existing_type_id(
    types: &TypeTable,
    ty: TypeId,
    generic_bindings: &MirGenericBindings,
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
        Type::Array(inner) => instantiate_existing_unary_type(
            types,
            inner,
            generic_bindings,
            span,
            index,
            Type::Array,
        ),
        Type::Map(key, value) => {
            let key = instantiate_existing_type_id(types, key, generic_bindings, span, index)?;
            let value = instantiate_existing_type_id(types, value, generic_bindings, span, index)?;
            find_existing_type(types, &Type::Map(key, value), span, index)
        }
        Type::Record(fields) => {
            let fields = fields
                .iter()
                .map(|(name, field_ty)| {
                    instantiate_existing_type_id(types, *field_ty, generic_bindings, span, index)
                        .map(|field_ty| (*name, field_ty))
                })
                .collect::<Result<FxHashMap<_, _>, _>>()?;
            find_existing_type(types, &Type::Record(fields), span, index)
        }
        Type::Set(inner) => {
            instantiate_existing_unary_type(types, inner, generic_bindings, span, index, Type::Set)
        }
        Type::Promissum(inner) => instantiate_existing_unary_type(
            types,
            inner,
            generic_bindings,
            span,
            index,
            Type::Promissum,
        ),
        Type::PromissumFailable(success, alternate) => {
            let success =
                instantiate_existing_type_id(types, success, generic_bindings, span, index)?;
            let alternate =
                instantiate_existing_type_id(types, alternate, generic_bindings, span, index)?;
            find_existing_type(
                types,
                &Type::PromissumFailable(success, alternate),
                span,
                index,
            )
        }
        Type::Cursor(inner) => instantiate_existing_unary_type(
            types,
            inner,
            generic_bindings,
            span,
            index,
            Type::Cursor,
        ),
        Type::AsyncCursor(item, alternate) => {
            let item = instantiate_existing_type_id(types, item, generic_bindings, span, index)?;
            let alternate =
                instantiate_existing_type_id(types, alternate, generic_bindings, span, index)?;
            find_existing_type(types, &Type::AsyncCursor(item, alternate), span, index)
        }
        Type::Intervallum(inner) => instantiate_existing_unary_type(
            types,
            inner,
            generic_bindings,
            span,
            index,
            Type::Intervallum,
        ),
        Type::Option(inner) => instantiate_existing_unary_type(
            types,
            inner,
            generic_bindings,
            span,
            index,
            Type::Option,
        ),
        Type::Ref(mode, inner) => {
            let inner = instantiate_existing_type_id(types, inner, generic_bindings, span, index)?;
            find_existing_type(types, &Type::Ref(mode, inner), span, index)
        }
        Type::Tensor(inner, shape) => instantiate_existing_indexed_type(
            types,
            inner,
            shape,
            generic_bindings,
            span,
            index,
            Type::Tensor,
        ),
        Type::Vector(inner, width) => instantiate_existing_indexed_type(
            types,
            inner,
            width,
            generic_bindings,
            span,
            index,
            Type::Vector,
        ),
        Type::Matrix(inner, shape) => instantiate_existing_indexed_type(
            types,
            inner,
            shape,
            generic_bindings,
            span,
            index,
            Type::Matrix,
        ),
        Type::Sparsa(inner, shape) => instantiate_existing_indexed_type(
            types,
            inner,
            shape,
            generic_bindings,
            span,
            index,
            Type::Sparsa,
        ),
        Type::Atomic(inner) => instantiate_existing_unary_type(
            types,
            inner,
            generic_bindings,
            span,
            index,
            Type::Atomic,
        ),
        Type::Alias(def_id, inner) => {
            let inner = instantiate_existing_type_id(types, inner, generic_bindings, span, index)?;
            find_existing_type(types, &Type::Alias(def_id, inner), span, index)
        }
        Type::Applied(base, args) => {
            let base = instantiate_existing_type_id(types, base, generic_bindings, span, index)?;
            let args = args
                .iter()
                .map(|arg| instantiate_existing_type_id(types, *arg, generic_bindings, span, index))
                .collect::<Result<Vec<_>, _>>()?;
            find_existing_type(types, &Type::Applied(base, args), span, index)
        }
        Type::Union(members) => {
            let members = members
                .iter()
                .map(|member| {
                    instantiate_existing_type_id(types, *member, generic_bindings, span, index)
                })
                .collect::<Result<Vec<_>, _>>()?;
            find_existing_type(types, &Type::Union(members), span, index)
        }
        Type::Func(sig) => {
            let sig = FuncSig {
                type_params: sig.type_params.clone(),
                type_param_constraints: sig.type_param_constraints.clone(),
                params: sig
                    .params
                    .iter()
                    .map(|param| {
                        instantiate_existing_type_id(types, param.ty, generic_bindings, span, index)
                            .map(|ty| ParamType {
                                ty,
                                mode: param.mode,
                                optional: param.optional,
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                ret: instantiate_existing_type_id(types, sig.ret, generic_bindings, span, index)?,
                err: sig
                    .err
                    .map(|err| {
                        instantiate_existing_type_id(types, err, generic_bindings, span, index)
                    })
                    .transpose()?,
                is_async: sig.is_async,
                is_generator: sig.is_generator,
            };
            find_existing_type(types, &Type::Func(sig), span, index)
        }
        Type::Primitive(_)
        | Type::SizedNumeric(_, _)
        | Type::ModularWord(_)
        | Type::SizedInstans(_)
        | Type::Tuple(_)
        | Type::Struct(_)
        | Type::Enum(_)
        | Type::Interface(_)
        | Type::Infer(_)
        | Type::InferUnion(_)
        | Type::Error => Ok(ty),
    }
}

fn instantiate_existing_unary_type(
    types: &TypeTable,
    inner: TypeId,
    generic_bindings: &MirGenericBindings,
    span: Span,
    index: &ExistingTypeIndex,
    build: impl FnOnce(TypeId) -> Type,
) -> Result<TypeId, MirGenericError> {
    let inner = instantiate_existing_type_id(types, inner, generic_bindings, span, index)?;
    find_existing_type(types, &build(inner), span, index)
}

fn instantiate_existing_indexed_type(
    types: &TypeTable,
    inner: TypeId,
    index_id: IndexId,
    generic_bindings: &MirGenericBindings,
    span: Span,
    index: &ExistingTypeIndex,
    build: impl FnOnce(TypeId, IndexId) -> Type,
) -> Result<TypeId, MirGenericError> {
    let inner = instantiate_existing_type_id(types, inner, generic_bindings, span, index)?;
    find_existing_type(types, &build(inner, index_id), span, index)
}

fn find_existing_type(
    types: &TypeTable,
    candidate: &Type,
    span: Span,
    index: &ExistingTypeIndex,
) -> Result<TypeId, MirGenericError> {
    index.find(types, candidate).ok_or_else(|| {
        MirGenericError::new(
            span,
            "instantiated generic type is missing semantic metadata",
        )
    })
}

// ---------------------------------------------------------------------------
// Shape-param substitution (4A)
// ---------------------------------------------------------------------------

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
        IndexExpr::Literal(_) | IndexExpr::Infer(_) | IndexExpr::Unspecified => index_id,
    }
}

/// Intern-or-find for the mutable instantiation walk.
///
/// Concrete shape-generic types (e.g. a transposed tensor shape) may be new to
/// the unit; unlike the read-only path's "must already be interned" contract,
/// the first occurrence interns the candidate so the monomorphized instance
/// carries a valid type graph.
fn intern_or_find(types: &mut TypeTable, candidate: &Type, index: &ExistingTypeIndex) -> TypeId {
    index.find(types, candidate).unwrap_or_else(|| types.intern(candidate.clone()))
}

/// Mutable mirror of [`instantiate_existing_type_id`] with shape substitution.
///
/// Shape params bind to their concrete index nodes via
/// [`substitute_index_params`] and concrete indexed types are interned when
/// absent (see [`intern_or_find`]).
fn instantiate_mutable_type_id(
    types: &mut TypeTable,
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
        Type::Array(inner) => instantiate_mutable_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Array,
        ),
        Type::Map(key, value) => {
            let key = instantiate_mutable_type_id(types, key, generic_bindings, shape_bindings, span, index)?;
            let value = instantiate_mutable_type_id(types, value, generic_bindings, shape_bindings, span, index)?;
            Ok(intern_or_find(types, &Type::Map(key, value), index))
        }
        Type::Record(fields) => {
            let fields = fields
                .iter()
                .map(|(name, field_ty)| {
                    instantiate_mutable_type_id(types, *field_ty, generic_bindings, shape_bindings, span, index)
                        .map(|field_ty| (*name, field_ty))
                })
                .collect::<Result<FxHashMap<_, _>, _>>()?;
            Ok(intern_or_find(types, &Type::Record(fields), index))
        }
        Type::Set(inner) => instantiate_mutable_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Set,
        ),
        Type::Promissum(inner) => instantiate_mutable_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Promissum,
        ),
        Type::PromissumFailable(success, alternate) => {
            let success = instantiate_mutable_type_id(types, success, generic_bindings, shape_bindings, span, index)?;
            let alternate = instantiate_mutable_type_id(types, alternate, generic_bindings, shape_bindings, span, index)?;
            Ok(intern_or_find(types, &Type::PromissumFailable(success, alternate), index))
        }
        Type::Cursor(inner) => instantiate_mutable_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Cursor,
        ),
        Type::AsyncCursor(item, alternate) => {
            let item = instantiate_mutable_type_id(types, item, generic_bindings, shape_bindings, span, index)?;
            let alternate = instantiate_mutable_type_id(types, alternate, generic_bindings, shape_bindings, span, index)?;
            Ok(intern_or_find(types, &Type::AsyncCursor(item, alternate), index))
        }
        Type::Intervallum(inner) => instantiate_mutable_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Intervallum,
        ),
        Type::Option(inner) => instantiate_mutable_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Option,
        ),
        Type::Ref(mode, inner) => {
            let inner = instantiate_mutable_type_id(types, inner, generic_bindings, shape_bindings, span, index)?;
            Ok(intern_or_find(types, &Type::Ref(mode, inner), index))
        }
        Type::Tensor(inner, shape) => instantiate_mutable_indexed_type(
            types,
            inner,
            shape,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Tensor,
        ),
        Type::Vector(inner, width) => instantiate_mutable_indexed_type(
            types,
            inner,
            width,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Vector,
        ),
        Type::Matrix(inner, shape) => instantiate_mutable_indexed_type(
            types,
            inner,
            shape,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Matrix,
        ),
        Type::Sparsa(inner, shape) => instantiate_mutable_indexed_type(
            types,
            inner,
            shape,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Sparsa,
        ),
        Type::Atomic(inner) => instantiate_mutable_unary_type(
            types,
            inner,
            generic_bindings,
            shape_bindings,
            span,
            index,
            Type::Atomic,
        ),
        Type::Alias(def_id, inner) => {
            let inner = instantiate_mutable_type_id(types, inner, generic_bindings, shape_bindings, span, index)?;
            Ok(intern_or_find(types, &Type::Alias(def_id, inner), index))
        }
        Type::Applied(base, args) => {
            let base = instantiate_mutable_type_id(types, base, generic_bindings, shape_bindings, span, index)?;
            let args = args
                .iter()
                .map(|arg| instantiate_mutable_type_id(types, *arg, generic_bindings, shape_bindings, span, index))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(intern_or_find(types, &Type::Applied(base, args), index))
        }
        Type::Union(members) => {
            let members = members
                .iter()
                .map(|member| instantiate_mutable_type_id(types, *member, generic_bindings, shape_bindings, span, index))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(intern_or_find(types, &Type::Union(members), index))
        }
        Type::Func(sig) => {
            let sig = FuncSig {
                type_params: sig.type_params.clone(),
                type_param_constraints: sig.type_param_constraints.clone(),
                params: sig
                    .params
                    .iter()
                    .map(|param| {
                        instantiate_mutable_type_id(types, param.ty, generic_bindings, shape_bindings, span, index)
                            .map(|ty| ParamType {
                                ty,
                                mode: param.mode,
                                optional: param.optional,
                            })
                    })
                    .collect::<Result<Vec<_>, _>>()?,
                ret: instantiate_mutable_type_id(types, sig.ret, generic_bindings, shape_bindings, span, index)?,
                err: sig
                    .err
                    .map(|err| instantiate_mutable_type_id(types, err, generic_bindings, shape_bindings, span, index))
                    .transpose()?,
                is_async: sig.is_async,
                is_generator: sig.is_generator,
            };
            Ok(intern_or_find(types, &Type::Func(sig), index))
        }
        Type::Primitive(_)
        | Type::SizedNumeric(_, _)
        | Type::ModularWord(_)
        | Type::SizedInstans(_)
        | Type::Tuple(_)
        | Type::Struct(_)
        | Type::Enum(_)
        | Type::Interface(_)
        | Type::Infer(_)
        | Type::InferUnion(_)
        | Type::Error => Ok(ty),
    }
}

fn instantiate_mutable_unary_type(
    types: &mut TypeTable,
    inner: TypeId,
    generic_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    span: Span,
    index: &ExistingTypeIndex,
    build: impl FnOnce(TypeId) -> Type,
) -> Result<TypeId, MirGenericError> {
    let inner = instantiate_mutable_type_id(types, inner, generic_bindings, shape_bindings, span, index)?;
    Ok(intern_or_find(types, &build(inner), index))
}

fn instantiate_mutable_indexed_type(
    types: &mut TypeTable,
    inner: TypeId,
    index_id: IndexId,
    generic_bindings: &MirGenericBindings,
    shape_bindings: &MirShapeBindings,
    span: Span,
    index: &ExistingTypeIndex,
    build: impl FnOnce(TypeId, IndexId) -> Type,
) -> Result<TypeId, MirGenericError> {
    let inner = instantiate_mutable_type_id(types, inner, generic_bindings, shape_bindings, span, index)?;
    let shape = substitute_index_params(types, index_id, shape_bindings);
    Ok(intern_or_find(types, &build(inner, shape), index))
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
        (Type::Array(a), Type::Array(b)) => types.equals(*a, *b),
        (Type::Map(ak, av), Type::Map(bk, bv)) => types.equals(*ak, *bk) && types.equals(*av, *bv),
        (Type::Record(a), Type::Record(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(name, a_ty)| b.get(name).is_some_and(|b_ty| types.equals(*a_ty, *b_ty)))
        }
        (Type::Set(a), Type::Set(b))
        | (Type::Promissum(a), Type::Promissum(b))
        | (Type::Cursor(a), Type::Cursor(b))
        | (Type::Intervallum(a), Type::Intervallum(b))
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
        (Type::Ref(a_mode, a_ty), Type::Ref(b_mode, b_ty)) => {
            a_mode == b_mode && types.equals(*a_ty, *b_ty)
        }
        (Type::Struct(a), Type::Struct(b))
        | (Type::Enum(a), Type::Enum(b))
        | (Type::Interface(a), Type::Interface(b)) => a == b,
        (Type::Alias(a_def, a_inner), Type::Alias(b_def, b_inner)) => {
            a_def == b_def && types.equals(*a_inner, *b_inner)
        }
        (Type::Func(a), Type::Func(b)) => func_signature_matches(types, a, b),
        (Type::Applied(a_base, a_args), Type::Applied(b_base, b_args)) => {
            a_args.len() == b_args.len()
                && types.equals(*a_base, *b_base)
                && a_args
                    .iter()
                    .zip(b_args.iter())
                    .all(|(a_arg, b_arg)| types.equals(*a_arg, *b_arg))
        }
        (Type::Union(a), Type::Union(b)) => {
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

fn func_signature_matches(types: &TypeTable, a: &FuncSig, b: &FuncSig) -> bool {
    a.params.len() == b.params.len()
        && a.params
            .iter()
            .zip(b.params.iter())
            .all(|(a_param, b_param)| {
                a_param.mode == b_param.mode
                    && a_param.optional == b_param.optional
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
    match types.get(ty) {
        Type::Param(_) => true,
        Type::Tensor(inner, shape)
        | Type::Vector(inner, shape)
        | Type::Matrix(inner, shape)
        | Type::Sparsa(inner, shape) => {
            index_contains_param(types, *shape)
                || type_contains_param_inner(types, *inner, seen)
        }
        Type::Array(inner)
        | Type::Set(inner)
        | Type::Promissum(inner)
        | Type::Cursor(inner)
        | Type::Atomic(inner)
        | Type::Intervallum(inner)
        | Type::Option(inner)
        | Type::Ref(_, inner)
        | Type::Alias(_, inner) => type_contains_param_inner(types, *inner, seen),
        Type::Map(key, value)
        | Type::PromissumFailable(key, value)
        | Type::AsyncCursor(key, value) => {
            type_contains_param_inner(types, *key, seen)
                || type_contains_param_inner(types, *value, seen)
        }
        Type::Record(fields) => fields
            .values()
            .any(|field_ty| type_contains_param_inner(types, *field_ty, seen)),
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
                || args
                    .iter()
                    .any(|arg| type_contains_param_inner(types, *arg, seen))
        }
        Type::Union(members) => members
            .iter()
            .any(|member| type_contains_param_inner(types, *member, seen)),
        Type::Primitive(_)
        | Type::SizedNumeric(_, _)
        | Type::ModularWord(_)
        | Type::SizedInstans(_)
        | Type::Tuple(_)
        | Type::Struct(_)
        | Type::Enum(_)
        | Type::Interface(_)
        | Type::Infer(_)
        | Type::InferUnion(_)
        | Type::Error => false,
    }
}

/// Whether an index term contains a `magnitudo` shape param (4A).
fn index_contains_param(types: &TypeTable, id: IndexId) -> bool {
    match types.get_index(id) {
        IndexExpr::Param(_) => true,
        IndexExpr::Tuple(dims) => dims.iter().any(|dim| index_contains_param(types, *dim)),
        IndexExpr::Literal(_) | IndexExpr::Infer(_) | IndexExpr::Unspecified => false,
    }
}

#[cfg(test)]
#[path = "generic_test.rs"]
mod tests;
