//! Re-exports index expression types for the semantic module's public API.
//!
//! This file is the re-export boundary for `IndexExpr`, `IndexId`, and `IndexVar`.
//! The canonical source is the `radix-types` foundation crate.

pub use radix_types::{IndexExpr, IndexId, IndexVar, RuntimeExtentId};

use crate::{Type, TypeId, TypeTable};
use radix_lexer::Symbol;
use rustc_hash::FxHashMap;

/// Substitute declared type parameters in an alias body with lowered arguments.
///
/// A parameter absent from `vars` remains unchanged, as it belongs to an
/// enclosing generic declaration rather than this alias application.
pub(crate) fn substitute_alias_params(
    types: &mut TypeTable,
    ty: TypeId,
    vars: &FxHashMap<Symbol, TypeId>,
) -> TypeId {
    match types.get(ty).clone() {
        Type::Param(symbol) => vars.get(&symbol).copied().unwrap_or(ty),
        Type::Array(inner) => {
            let inner = substitute_alias_params(types, inner, vars);
            types.array(inner)
        }
        Type::BoundedArray { element, capacity } => {
            let element = substitute_alias_params(types, element, vars);
            types.bounded_array(element, capacity)
        }
        Type::Queue(inner) => {
            let inner = substitute_alias_params(types, inner, vars);
            types.queue(inner)
        }
        Type::BoundedQueue { element, capacity } => {
            let element = substitute_alias_params(types, element, vars);
            types.bounded_queue(element, capacity)
        }
        Type::Stack(inner) => {
            let inner = substitute_alias_params(types, inner, vars);
            types.stack(inner)
        }
        Type::BoundedStack { element, capacity } => {
            let element = substitute_alias_params(types, element, vars);
            types.bounded_stack(element, capacity)
        }
        Type::Map(key, value) => {
            let key = substitute_alias_params(types, key, vars);
            let value = substitute_alias_params(types, value, vars);
            types.map(key, value)
        }
        Type::Ledger(key, value) => {
            let key = substitute_alias_params(types, key, vars);
            let value = substitute_alias_params(types, value, vars);
            types.ledger(key, value)
        }
        Type::Record(fields) => {
            let fields = fields
                .into_iter()
                .map(|(name, field)| (name, substitute_alias_params(types, field, vars)))
                .collect::<FxHashMap<_, _>>();
            types.intern(Type::Record(fields))
        }
        Type::Set(inner) => {
            let inner = substitute_alias_params(types, inner, vars);
            types.set(inner)
        }
        Type::Promissum(inner) => {
            let inner = substitute_alias_params(types, inner, vars);
            types.promissum(inner)
        }
        Type::PromissumFailable(success, alternate) => {
            let success = substitute_alias_params(types, success, vars);
            let alternate = substitute_alias_params(types, alternate, vars);
            types.promissum_failable(success, alternate)
        }
        Type::Cursor(inner) => {
            let inner = substitute_alias_params(types, inner, vars);
            types.cursor(inner)
        }
        Type::AsyncCursor(item, alternate) => {
            let item = substitute_alias_params(types, item, vars);
            let alternate = substitute_alias_params(types, alternate, vars);
            types.async_cursor(item, alternate)
        }
        Type::Tensor(element, shape) => {
            let element = substitute_alias_params(types, element, vars);
            types.tensor_with_shape(element, shape)
        }
        Type::Vector(element, width) => {
            let element = substitute_alias_params(types, element, vars);
            types.vector_with_width(element, width)
        }
        Type::Matrix(element, shape) => {
            let element = substitute_alias_params(types, element, vars);
            types.matrix_with_shape(element, shape)
        }
        Type::Sparsa(element, shape) => {
            let element = substitute_alias_params(types, element, vars);
            types.sparsa_with_shape(element, shape)
        }
        Type::Atomic(inner) => {
            let inner = substitute_alias_params(types, inner, vars);
            types.atomic(inner)
        }
        Type::Intervallum(inner, _) => {
            let inner = substitute_alias_params(types, inner, vars);
            types.intervallum(inner)
        }
        Type::Option(inner) => {
            let inner = substitute_alias_params(types, inner, vars);
            types.option(inner)
        }
        Type::Ref(mode, inner) => {
            let inner = substitute_alias_params(types, inner, vars);
            types.reference(mode, inner)
        }
        Type::Alias(def, inner) => {
            let inner = substitute_alias_params(types, inner, vars);
            types.intern(Type::Alias(def, inner))
        }
        Type::Func(sig) => {
            let params = sig
                .params
                .into_iter()
                .map(|param| crate::ParamType {
                    ty: substitute_alias_params(types, param.ty, vars),
                    ..param
                })
                .collect();
            let ret = substitute_alias_params(types, sig.ret, vars);
            let err = sig.err.map(|err| substitute_alias_params(types, err, vars));
            types.function(crate::FuncSig {
                params,
                ret,
                err,
                ..sig
            })
        }
        Type::Applied(base, args) => {
            let base = substitute_alias_params(types, base, vars);
            let args = args
                .into_iter()
                .map(|arg| match arg {
                    radix_types::AppliedArg::Type(ty) => {
                        radix_types::AppliedArg::Type(substitute_alias_params(types, ty, vars))
                    }
                    radix_types::AppliedArg::Size(size) => radix_types::AppliedArg::Size(size),
                })
                .collect();
            types.intern(Type::Applied(base, args))
        }
        Type::Union(members) => {
            let members = members
                .into_iter()
                .map(|member| substitute_alias_params(types, member, vars))
                .collect();
            types.intern(Type::Union(members))
        }
        Type::Intersection(members) => {
            let members = members
                .into_iter()
                .map(|member| substitute_alias_params(types, member, vars))
                .collect();
            types.intern(Type::Intersection(members))
        }
        Type::Tuple(elements) => {
            let elements = elements
                .into_iter()
                .map(|element| substitute_alias_params(types, element, vars))
                .collect();
            types.intern(Type::Tuple(elements))
        }
        _ => ty,
    }
}

/// Copy a type, replacing each capacity `IndexExpr::Infer` with a fresh hole.
///
/// Alias use sites share one interned body. Freshening so each use of
/// `typus X = ascii<_>` solves independently (CS-2).
pub fn freshen_bounded_capacity_holes(types: &mut TypeTable, ty: TypeId) -> TypeId {
    match types.get(ty).clone() {
        Type::BoundedAscii { capacity } => {
            let capacity = freshen_index_infer(types, capacity);
            types.bounded_ascii(capacity)
        }
        Type::BoundedTextus { capacity } => {
            let capacity = freshen_index_infer(types, capacity);
            types.bounded_textus(capacity)
        }
        Type::BoundedOcteti { capacity } => {
            let capacity = freshen_index_infer(types, capacity);
            types.bounded_octeti(capacity)
        }
        Type::BoundedArray { element, capacity } => {
            let element = freshen_bounded_capacity_holes(types, element);
            let capacity = freshen_index_infer(types, capacity);
            types.bounded_array(element, capacity)
        }
        Type::Alias(def, inner) => {
            let inner = freshen_bounded_capacity_holes(types, inner);
            types.intern(Type::Alias(def, inner))
        }
        _ => ty,
    }
}

fn freshen_index_infer(types: &mut TypeTable, id: IndexId) -> IndexId {
    if matches!(types.get_index(id), IndexExpr::Infer(_)) {
        let var = types.fresh_index_infer_var();
        types.intern_index(IndexExpr::Infer(var))
    } else {
        id
    }
}
