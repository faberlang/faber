//! Re-exports index expression types for the semantic module's public API.
//!
//! This file is the re-export boundary for `IndexExpr`, `IndexId`, and `IndexVar`.
//! The canonical source is the `radix-types` foundation crate.

pub use radix_types::{IndexExpr, IndexId, IndexVar, RuntimeExtentId};

use crate::{Type, TypeId, TypeTable};

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
