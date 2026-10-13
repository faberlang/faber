//! Built-in conformance to the ordering contract (DECISIONS D1.4).
//!
//! `numerus`, `fractus`, `textus` and `instans` conform to norma's
//! `Orderable<T>` without user code, and so does every tuple whose elements
//! all conform. Their `compare` is supplied by the compiler:
//!
//! - integers compare by value;
//! - `fractus` compares by IEEE 754 totalOrder (operator default F2):
//!   −NaN < −∞ < … < −0 < +0 < … < +∞ < NaN;
//! - `textus` compares by Unicode code point (D1.6);
//! - `instans` compares by point in time;
//! - tuples compare lexicographically, element by element.
//!
//! This module classifies types; the semantic pass and every backend read the
//! same answer so they cannot disagree about what conforms.

use crate::numeric_width::NumericWidth;
use crate::{DefId, Primitive, Type, TypeId, TypeTable};

/// The one method of the ordering contract (D1.4; name from D1.5).
pub const ORDERING_METHOD: &str = "compare";

/// How the compiler supplies `compare` for one conforming type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderingKind {
    /// Signed or unsigned integer, any width.
    Integer,
    /// `f32`/`f64`: IEEE 754 totalOrder.
    Float,
    /// `textus`: Unicode code-point order.
    Text,
    /// `instans`: point-in-time order.
    Instans,
    /// A user `genus` whose `implet` clause names the ordering contract with
    /// itself as the argument; `compare` is the genus's own method.
    Genus(DefId),
    /// A type parameter; the witness decides after instantiation.
    Param,
    /// Lexicographic over the element kinds.
    Tuple(Vec<OrderingKind>),
}

/// Classify `ty` for ordering.
///
/// `genus_conforms` answers whether a genus declares the ordering contract
/// over itself; `param_conforms` whether a type parameter is bounded by it.
/// Returns `None` when `ty` does not conform.
#[allow(clippy::match_same_arms)] // WHY: `Type::Bits` is ruling B2 and must stay its own arm, not merge into `numerus`.
pub fn ordering_kind(
    types: &TypeTable,
    ty: TypeId,
    genus_conforms: &dyn Fn(DefId) -> bool,
    param_conforms: &dyn Fn(radix_lexer::Symbol) -> bool,
) -> Option<OrderingKind> {
    let (_, ty) = types.peel_access_marker(ty);
    match types.get(ty) {
        Type::Primitive(Primitive::Numerus) => Some(OrderingKind::Integer),
        Type::SizedNumeric(Primitive::Numerus, width) if !width.is_decimal() => {
            Some(OrderingKind::Integer)
        }
        // Bits order by bit pattern read as unsigned (ruling B2).
        Type::Bits(_) => Some(OrderingKind::Integer),
        Type::Primitive(Primitive::Fractus) => Some(OrderingKind::Float),
        Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32 | NumericWidth::F64) => {
            Some(OrderingKind::Float)
        }
        Type::Primitive(Primitive::Textus) => Some(OrderingKind::Text),
        Type::Primitive(Primitive::Instans) | Type::SizedInstans(_) => Some(OrderingKind::Instans),
        Type::Struct(def) if genus_conforms(*def) => Some(OrderingKind::Genus(*def)),
        Type::Param(sym) if param_conforms(*sym) => Some(OrderingKind::Param),
        Type::Tuple(elements) if !elements.is_empty() => elements
            .iter()
            .map(|element| ordering_kind(types, *element, genus_conforms, param_conforms))
            .collect::<Option<Vec<_>>>()
            .map(OrderingKind::Tuple),
        _ => None,
    }
}

/// Classify `ty` against the contracts recorded in the type table alone:
/// built-ins and tuples of built-ins. Genus and parameter conformance need
/// declaration facts and are answered by the caller's closures.
#[must_use]
pub fn builtin_ordering_kind(types: &TypeTable, ty: TypeId) -> Option<OrderingKind> {
    ordering_kind(types, ty, &|_| false, &|_| false)
}

#[cfg(test)]
#[path = "ordering_test.rs"]
mod tests;
