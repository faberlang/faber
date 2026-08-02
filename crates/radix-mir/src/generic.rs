//! Shared MIR generic binding and type-substitution helpers.
//!
//! Validation and monomorphization both need the same structural witness rules:
//! call-site values bind `Type::Param` occurrences inside semantic MIR types,
//! then concrete functions consume the instantiated type graph. Keep that logic
//! here so target emitters never grow their own generic policies.

use crate::MirType;
use radix_lexer::{Span, Symbol};
use radix_types::{FuncSig, ParamType, Type, TypeId, TypeTable};
use rustc_hash::{FxHashMap, FxHashSet};
use std::hash::{Hash, Hasher};

pub type MirGenericBindings = FxHashMap<Symbol, MirType>;

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

pub fn bind_generic_type_witness(
    types: &TypeTable,
    expected: MirType,
    actual: MirType,
    generic_bindings: &mut MirGenericBindings,
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
    seen: &mut FxHashSet<TypeId>,
) -> bool {
    // Param binds to the actual TypeId as-is (including when actual is Alias).
    // Preserve this precedence over Alias peeling on the actual side.
    if let Type::Param(sym) = types.get(expected) {
        return bind_type_param(types, *sym, actual, generic_bindings);
    }

    // Peel expected Alias with path-local cycle guard.
    if let Type::Alias(_, inner) = types.get(expected) {
        if !seen.insert(expected) {
            return false;
        }
        let inner = *inner;
        let ok = try_bind_generic_type_witness(types, inner, actual, generic_bindings, seen);
        seen.remove(&expected);
        return ok;
    }

    // Peel actual Alias with path-local cycle guard (expected is not Param/Alias).
    if let Type::Alias(_, inner) = types.get(actual) {
        if !seen.insert(actual) {
            return false;
        }
        let inner = *inner;
        let ok = try_bind_generic_type_witness(types, expected, inner, generic_bindings, seen);
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
            Type::Sparsa(expected_inner, expected_shape),
            Type::Sparsa(actual_inner, actual_shape),
        ) => {
            types.index_equals(*expected_shape, *actual_shape)
                && try_bind_generic_type_witness(
                    types,
                    *expected_inner,
                    *actual_inner,
                    generic_bindings,
                    seen,
                )
        }
        (Type::Map(expected_key, expected_value), Type::Map(actual_key, actual_value)) => {
            try_bind_generic_type_witness(types, *expected_key, *actual_key, generic_bindings, seen)
                && try_bind_generic_type_witness(
                    types,
                    *expected_value,
                    *actual_value,
                    generic_bindings,
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
                            seen,
                        )
                    })
        }
        (Type::Union(expected_members), Type::Union(actual_members)) => try_bind_union_members(
            types,
            expected_members,
            actual_members,
            generic_bindings,
            seen,
        ),
        (Type::Func(expected_sig), Type::Func(actual_sig)) => {
            try_bind_func_signature(types, expected_sig, actual_sig, generic_bindings, seen)
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

fn try_bind_union_members(
    types: &TypeTable,
    expected_members: &[TypeId],
    actual_members: &[TypeId],
    generic_bindings: &mut MirGenericBindings,
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
    seen: &mut FxHashSet<TypeId>,
) -> bool {
    let Some(expected) = expected_members.get(index).copied() else {
        return true;
    };

    for actual_index in 0..actual_members.len() {
        if matched[actual_index] {
            continue;
        }

        // Snapshot keys present before this candidate. bind_type_param is
        // insert-only, so rolling back means removing keys not in the snapshot.
        let keys_before: FxHashSet<Symbol> = generic_bindings.keys().copied().collect();

        if !try_bind_generic_type_witness(
            types,
            expected,
            actual_members[actual_index],
            generic_bindings,
            seen,
        ) {
            undo_bindings_to(generic_bindings, &keys_before);
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
            seen,
        ) {
            matched[actual_index] = false;
            return true;
        }

        // Branch failed — restore bindings to pre-candidate state.
        undo_bindings_to(generic_bindings, &keys_before);
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

fn try_bind_func_signature(
    types: &TypeTable,
    expected_sig: &FuncSig,
    actual_sig: &FuncSig,
    generic_bindings: &mut MirGenericBindings,
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
                        seen,
                    )
            })
        && try_bind_generic_type_witness(
            types,
            expected_sig.ret,
            actual_sig.ret,
            generic_bindings,
            seen,
        )
        && match (expected_sig.err, actual_sig.err) {
            (Some(expected_err), Some(actual_err)) => try_bind_generic_type_witness(
                types,
                expected_err,
                actual_err,
                generic_bindings,
                seen,
            ),
            (None, None) => true,
            _ => false,
        }
}

/// # Errors
/// Returns an error when validation or type resolution fails. See [`MirGenericError`].
pub fn instantiate_existing_generic_type(
    types: &TypeTable,
    ty: MirType,
    generic_bindings: &MirGenericBindings,
    span: Span,
) -> Result<MirType, MirGenericError> {
    // Build once per top-level instantiate: O(T) setup, then O(1) probe per node.
    let index = ExistingTypeIndex::build(types);
    let instantiated =
        instantiate_existing_type_id(types, ty.semantic_id(), generic_bindings, span, &index)?;
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
            // TypeId is a u32 arena index by design (radix-types::types).
            #[expect(
                clippy::cast_possible_truncation,
                reason = "TypeId is a u32 arena index by design"
            )]
            let id = TypeId(index as u32);
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
            // TypeId is a u32 arena index by design (radix-types::types).
            #[expect(
                clippy::cast_possible_truncation,
                reason = "TypeId is a u32 arena index by design"
            )]
            let id = TypeId(index as u32);
            type_matches_candidate(types, id, candidate).then_some(id)
        })
    }
}

fn type_fingerprint(ty: &Type) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    hash_type_shape(ty, &mut hasher);
    hasher.finish()
}

fn hash_type_shape(ty: &Type, hasher: &mut impl Hasher) {
    match ty {
        Type::Primitive(prim) => {
            0u8.hash(hasher);
            prim.hash(hasher);
        }
        Type::Array(inner) => {
            1u8.hash(hasher);
            inner.0.hash(hasher);
        }
        Type::Map(key, value) => {
            2u8.hash(hasher);
            key.0.hash(hasher);
            value.0.hash(hasher);
        }
        Type::Record(fields) => {
            3u8.hash(hasher);
            let mut pairs = fields
                .iter()
                .map(|(name, field_ty)| (name.0, field_ty.0))
                .collect::<Vec<_>>();
            pairs.sort_unstable();
            pairs.hash(hasher);
        }
        Type::Set(inner) => {
            4u8.hash(hasher);
            inner.0.hash(hasher);
        }
        Type::Promissum(inner) => {
            5u8.hash(hasher);
            inner.0.hash(hasher);
        }
        Type::PromissumFailable(success, alternate) => {
            5u8.hash(hasher);
            success.0.hash(hasher);
            alternate.0.hash(hasher);
        }
        Type::AsyncCursor(item, alternate) => {
            31u8.hash(hasher);
            item.0.hash(hasher);
            alternate.0.hash(hasher);
        }
        Type::Cursor(inner) => {
            6u8.hash(hasher);
            inner.0.hash(hasher);
        }
        Type::Tensor(inner, shape) => {
            7u8.hash(hasher);
            inner.0.hash(hasher);
            shape.0.hash(hasher);
        }
        Type::Vector(inner, width) => {
            8u8.hash(hasher);
            inner.0.hash(hasher);
            width.0.hash(hasher);
        }
        Type::Matrix(inner, shape) => {
            9u8.hash(hasher);
            inner.0.hash(hasher);
            shape.0.hash(hasher);
        }
        Type::Sparsa(inner, shape) => {
            10u8.hash(hasher);
            inner.0.hash(hasher);
            shape.0.hash(hasher);
        }
        Type::Atomic(inner) => {
            11u8.hash(hasher);
            inner.0.hash(hasher);
        }
        Type::Intervallum(inner) => {
            12u8.hash(hasher);
            inner.0.hash(hasher);
        }
        Type::SizedNumeric(prim, width) => {
            13u8.hash(hasher);
            prim.hash(hasher);
            width.hash(hasher);
        }
        Type::ModularWord(width) => {
            14u8.hash(hasher);
            width.hash(hasher);
        }
        Type::SizedInstans(precision) => {
            15u8.hash(hasher);
            precision.hash(hasher);
        }
        Type::Option(inner) => {
            16u8.hash(hasher);
            inner.0.hash(hasher);
        }
        Type::Ref(mode, inner) => {
            17u8.hash(hasher);
            mode.hash(hasher);
            inner.0.hash(hasher);
        }
        Type::Struct(def) => {
            18u8.hash(hasher);
            def.hash(hasher);
        }
        Type::Enum(def) => {
            19u8.hash(hasher);
            def.hash(hasher);
        }
        Type::Interface(def) => {
            20u8.hash(hasher);
            def.hash(hasher);
        }
        Type::Alias(def, inner) => {
            21u8.hash(hasher);
            def.hash(hasher);
            inner.0.hash(hasher);
        }
        Type::Func(sig) => {
            22u8.hash(hasher);
            sig.hash(hasher);
        }
        Type::Param(sym) => {
            23u8.hash(hasher);
            sym.0.hash(hasher);
        }
        Type::Applied(base, args) => {
            24u8.hash(hasher);
            base.0.hash(hasher);
            for arg in args {
                arg.0.hash(hasher);
            }
        }
        Type::Infer(var) => {
            25u8.hash(hasher);
            var.0.hash(hasher);
        }
        Type::Union(members) => {
            26u8.hash(hasher);
            for member in members {
                member.0.hash(hasher);
            }
        }
        Type::Tuple(elems) => {
            27u8.hash(hasher);
            for elem in elems {
                elem.0.hash(hasher);
            }
        }
        Type::Error => {
            28u8.hash(hasher);
        }
    }
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
        Type::Array(inner) => {
            let inner = instantiate_existing_type_id(types, inner, generic_bindings, span, index)?;
            find_existing_type(types, &Type::Array(inner), span, index)
        }
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
    index_id: radix_types::IndexId,
    generic_bindings: &MirGenericBindings,
    span: Span,
    index: &ExistingTypeIndex,
    build: impl FnOnce(TypeId, radix_types::IndexId) -> Type,
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
        | (Type::Option(a), Type::Option(b)) => types.equals(*a, *b),
        (Type::Tensor(a_ty, a_shape), Type::Tensor(b_ty, b_shape))
        | (Type::Vector(a_ty, a_shape), Type::Vector(b_ty, b_shape))
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
        Type::Array(inner)
        | Type::Set(inner)
        | Type::Promissum(inner)
        | Type::Cursor(inner)
        | Type::Tensor(inner, _)
        | Type::Vector(inner, _)
        | Type::Matrix(inner, _)
        | Type::Sparsa(inner, _)
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
        | Type::Error => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use radix_types::{DefId, Primitive};

    /// Build a self-referential Alias: `Alias(_, TypeId(n))` interned at index `n`.
    fn self_cyclic_alias(types: &mut TypeTable, def: u32) -> TypeId {
        let next = u32::try_from(types.type_count()).expect("type table fits u32");
        types.intern(Type::Alias(DefId(def), TypeId(next)))
    }

    /// Two-node Alias cycle: A → B → A.
    fn mutual_cyclic_aliases(types: &mut TypeTable) -> (TypeId, TypeId) {
        let n = u32::try_from(types.type_count()).expect("type table fits u32");
        let a = types.intern(Type::Alias(DefId(1), TypeId(n + 1)));
        let b = types.intern(Type::Alias(DefId(2), TypeId(n)));
        assert_eq!(a, TypeId(n));
        assert_eq!(b, TypeId(n + 1));
        (a, b)
    }

    #[test]
    fn alias_cycle_self_terminates() {
        let mut types = TypeTable::new();
        let cyclic = self_cyclic_alias(&mut types, 10);
        // Pure cyclic Alias has no Param, so bind_generic_type_witness would
        // early-return true via type_contains_param. Pair the cycle with a
        // Param so try_bind walks the Alias arm and hits the seen-set guard.
        let param = types.intern(Type::Param(Symbol(99)));
        let expected = types.map(cyclic, param);
        let numerus = types.primitive(Primitive::Numerus);
        let actual = types.map(numerus, numerus);
        let mut bindings = MirGenericBindings::default();

        // Must return false rather than overflow the stack.
        assert!(!bind_generic_type_witness(
            &types,
            MirType::semantic(expected),
            MirType::semantic(actual),
            &mut bindings,
        ));
        assert!(bindings.is_empty());
    }

    #[test]
    fn alias_cycle_mutual_terminates() {
        let mut types = TypeTable::new();
        let (a, _b) = mutual_cyclic_aliases(&mut types);
        let param = types.intern(Type::Param(Symbol(98)));
        let expected = types.map(a, param);
        let numerus = types.primitive(Primitive::Numerus);
        let actual = types.map(numerus, numerus);
        let mut bindings = MirGenericBindings::default();

        assert!(!bind_generic_type_witness(
            &types,
            MirType::semantic(expected),
            MirType::semantic(actual),
            &mut bindings,
        ));
        assert!(bindings.is_empty());
    }

    #[test]
    fn acyclic_alias_binds_param() {
        let mut types = TypeTable::new();
        let param = types.intern(Type::Param(Symbol(42)));
        let alias = types.intern(Type::Alias(DefId(7), param));
        let numerus = types.primitive(Primitive::Numerus);
        let mut bindings = MirGenericBindings::default();

        assert!(bind_generic_type_witness(
            &types,
            MirType::semantic(alias),
            MirType::semantic(numerus),
            &mut bindings,
        ));
        assert_eq!(
            bindings.get(&Symbol(42)).copied().map(MirType::semantic_id),
            Some(numerus)
        );
    }

    #[test]
    fn shared_param_in_map_still_binds() {
        // Regression: path-local Alias seen-set must not break DAG sharing of
        // the same Param TypeId in map key and value positions.
        let mut types = TypeTable::new();
        let param = types.intern(Type::Param(Symbol(1)));
        let map_expected = types.map(param, param);
        let numerus = types.primitive(Primitive::Numerus);
        let map_actual = types.map(numerus, numerus);
        let mut bindings = MirGenericBindings::default();

        assert!(bind_generic_type_witness(
            &types,
            MirType::semantic(map_expected),
            MirType::semantic(map_actual),
            &mut bindings,
        ));
        assert_eq!(
            bindings.get(&Symbol(1)).copied().map(MirType::semantic_id),
            Some(numerus)
        );
    }

    #[test]
    fn shared_alias_in_map_still_binds() {
        let mut types = TypeTable::new();
        let param = types.intern(Type::Param(Symbol(3)));
        let alias = types.intern(Type::Alias(DefId(9), param));
        let map_expected = types.map(alias, alias);
        let textus = types.primitive(Primitive::Textus);
        let map_actual = types.map(textus, textus);
        let mut bindings = MirGenericBindings::default();

        assert!(bind_generic_type_witness(
            &types,
            MirType::semantic(map_expected),
            MirType::semantic(map_actual),
            &mut bindings,
        ));
        assert_eq!(
            bindings.get(&Symbol(3)).copied().map(MirType::semantic_id),
            Some(textus)
        );
    }

    #[test]
    fn union_backtracking_binds_without_clone() {
        let mut types = TypeTable::new();
        let t = types.intern(Type::Param(Symbol(10)));
        let u = types.intern(Type::Param(Symbol(11)));
        let v = types.intern(Type::Param(Symbol(12)));
        let expected = types.intern(Type::Union(vec![t, u, v]));

        let numerus = types.primitive(Primitive::Numerus);
        let textus = types.primitive(Primitive::Textus);
        let bivalens = types.primitive(Primitive::Bivalens);
        // Permuted actual order forces backtracking past first candidates.
        let actual = types.intern(Type::Union(vec![bivalens, textus, numerus]));

        let mut bindings = MirGenericBindings::default();
        assert!(bind_generic_type_witness(
            &types,
            MirType::semantic(expected),
            MirType::semantic(actual),
            &mut bindings,
        ));
        assert_eq!(bindings.len(), 3);
        assert_eq!(
            bindings.get(&Symbol(10)).copied().map(MirType::semantic_id),
            Some(textus)
        );
        assert_eq!(
            bindings.get(&Symbol(11)).copied().map(MirType::semantic_id),
            Some(numerus)
        );
        assert_eq!(
            bindings.get(&Symbol(12)).copied().map(MirType::semantic_id),
            Some(bivalens)
        );
    }

    #[test]
    fn union_mismatch_undoes_partial_bindings() {
        let mut types = TypeTable::new();
        let t = types.intern(Type::Param(Symbol(20)));
        let numerus = types.primitive(Primitive::Numerus);
        let expected = types.intern(Type::Union(vec![t, numerus]));

        let textus = types.primitive(Primitive::Textus);
        let fractus = types.primitive(Primitive::Fractus);
        let actual = types.intern(Type::Union(vec![textus, fractus]));

        let mut bindings = MirGenericBindings::default();
        assert!(!bind_generic_type_witness(
            &types,
            MirType::semantic(expected),
            MirType::semantic(actual),
            &mut bindings,
        ));
        // Failed candidate must not leak Param bindings into the caller's map.
        assert!(bindings.is_empty());
    }

    #[test]
    fn union_backtracking_bounded_n8() {
        // N=8 → up to 8! = 40320 permutation paths. Without per-candidate
        // clone this must terminate quickly with one shared bindings map.
        let mut types = TypeTable::new();
        let params: Vec<TypeId> = (0..8)
            .map(|i| types.intern(Type::Param(Symbol(100 + i))))
            .collect();
        let expected = types.intern(Type::Union(params.clone()));

        let primitives = [
            Primitive::Numerus,
            Primitive::Textus,
            Primitive::Bivalens,
            Primitive::Fractus,
            Primitive::Octeti,
            Primitive::Ascii,
            Primitive::Instans,
            Primitive::Json,
        ];
        // Reverse order so the first expected member matches last actual.
        let actual_members: Vec<TypeId> = primitives
            .iter()
            .rev()
            .map(|p| types.primitive(*p))
            .collect();
        let actual = types.intern(Type::Union(actual_members));

        let mut bindings = MirGenericBindings::default();
        assert!(bind_generic_type_witness(
            &types,
            MirType::semantic(expected),
            MirType::semantic(actual),
            &mut bindings,
        ));
        assert_eq!(bindings.len(), 8);
    }
}
