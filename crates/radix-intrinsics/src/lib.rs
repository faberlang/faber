//! Compiler-owned receiver intrinsic registry.
//!
//! Radix promotes a fixed catalog of `lista`, `queue`, `stack`, `tabula`,
//! `copia`, `textus`, `ascii`, `numerus`, `fractus`, `tensor`, `matrix`,
//! `vector`, `intervallum`, and `atomic`
//! methods. Semantic analysis, MIR lowering, and target backends consult this
//! module instead of maintaining parallel string tables.

mod registry;

pub use registry::{
    CatalogGenericParam, INTRINSICS, IntrinsicMirKind, IntrinsicParamKind, IntrinsicReceiver,
    IntrinsicReturnKind, MethodIntrinsic,
};

use radix_types::{MirAtomicOp, MirCollectionOp};
use radix_types::{Primitive, Type, TypeId, TypeTable};

/// Lookup a promoted intrinsic by receiver family, Faber name, and value arity.
///
/// Value-arity lookup remains unique while every row's generic list is empty.
/// When a later unit adds a generic row, [`lookup_method_generic`] distinguishes
/// it from a same-name value-arity twin. Typecheck uses this function, then
/// inspects [`MethodIntrinsic::generic`] so a declared row can bind through the
/// shared generic binder instead of falling through as an unknown method.
#[must_use]
pub fn lookup_method(
    receiver: IntrinsicReceiver,
    method: &str,
    arg_count: usize,
) -> Option<&'static MethodIntrinsic> {
    let mut found = None;
    for entry in INTRINSICS {
        if entry.receiver == receiver && entry.faber_name == method && entry.arg_count == arg_count
        {
            if found.is_some() {
                return None;
            }
            found = Some(entry);
        }
    }
    found
}

/// Lookup a promoted intrinsic by receiver, name, generic arity, and value arity.
#[must_use]
pub fn lookup_method_generic(
    receiver: IntrinsicReceiver,
    method: &str,
    generic_count: usize,
    arg_count: usize,
) -> Option<&'static MethodIntrinsic> {
    INTRINSICS.iter().find(|entry| {
        entry.receiver == receiver
            && entry.faber_name == method
            && entry.generic.len() == generic_count
            && entry.arg_count == arg_count
    })
}

/// Map a normalized semantic receiver type to intrinsic receiver families.
///
/// `fractus` values may satisfy both [`IntrinsicReceiver::Fractus`] and
/// [`IntrinsicReceiver::Numeric`] rows; callers should try fractus-specific rows
/// before shared numeric rows.
#[must_use]
pub fn receivers_for_type(ty: &Type) -> &'static [IntrinsicReceiver] {
    match ty {
        Type::Array(_) | Type::BoundedArray { .. } => &[IntrinsicReceiver::Lista],
        Type::Queue(_) | Type::BoundedQueue { .. } => &[IntrinsicReceiver::Queue],
        Type::Stack(_) | Type::BoundedStack { .. } => &[IntrinsicReceiver::Stack],
        // WHY: `octeti` is semantic sugar for `lista<numerus<u8>>`. Routing it to
        // the Lista receiver family gives octeti values every `lista` method.
        // The assignability rule in `semantic/types.rs` makes the two types
        // bidirectionally assignable; this arm makes method dispatch agree.
        // Bounded `octeti<N>` keeps the same family; `N` is identity, not a
        // second method table.
        Type::Primitive(Primitive::Octeti) | Type::BoundedOcteti { .. } => {
            &[IntrinsicReceiver::Lista]
        }
        Type::Map(_, _) => &[IntrinsicReceiver::Tabula],
        Type::Set(_) => &[IntrinsicReceiver::Copia],
        Type::Primitive(Primitive::Textus) | Type::BoundedTextus { .. } => {
            &[IntrinsicReceiver::Textus]
        }
        Type::Primitive(Primitive::Ascii) | Type::BoundedAscii { .. } => {
            &[IntrinsicReceiver::Ascii]
        }
        Type::Primitive(Primitive::Regex) => &[IntrinsicReceiver::Regex],
        Type::Primitive(Primitive::Numerus) => &[IntrinsicReceiver::Numeric],
        Type::Primitive(Primitive::Fractus) => {
            &[IntrinsicReceiver::Fractus, IntrinsicReceiver::Numeric]
        }
        Type::SizedNumeric(Primitive::Fractus, _) => {
            &[IntrinsicReceiver::Fractus, IntrinsicReceiver::Numeric]
        }
        Type::SizedNumeric(Primitive::Numerus, _) => &[IntrinsicReceiver::Numeric],
        Type::ModularWord(_) => &[IntrinsicReceiver::Numeric],
        Type::Tensor(_, _) => &[IntrinsicReceiver::Tensor],
        Type::Matrix(_, _) => &[IntrinsicReceiver::Matrix],
        Type::Vector(_, _) => &[IntrinsicReceiver::Vector],
        Type::Sparsa(_, _) => &[IntrinsicReceiver::Sparsa],
        Type::Intervallum(_) => &[IntrinsicReceiver::Intervallum],
        Type::Atomic(_) => &[IntrinsicReceiver::Atomic],
        _ => &[],
    }
}

/// Map a receiver type ID to intrinsic families after peeling aliases and
/// access markers. The plain [`receivers_for_type`] API accepts a fully
/// resolved [`Type`] and therefore cannot inspect a `Type::Ref` payload ID.
#[must_use]
pub fn receivers_for_type_id(type_id: TypeId, types: &TypeTable) -> &'static [IntrinsicReceiver] {
    let mut payload_id = type_id;
    while let Type::Alias(_, inner) | Type::Ref(_, inner) = types.get(payload_id) {
        payload_id = *inner;
    }
    receivers_for_type(types.get(payload_id))
}

/// Resolve a promoted intrinsic from a semantic receiver type.
#[must_use]
pub fn lookup_method_for_type(
    receiver_ty: &Type,
    method: &str,
    arg_count: usize,
) -> Option<&'static MethodIntrinsic> {
    if matches!(receiver_ty, Type::ModularWord(_)) && !is_modular_word_method(method) {
        return None;
    }
    for family in receivers_for_type(receiver_ty) {
        if let Some(entry) = lookup_method(*family, method, arg_count) {
            return Some(entry);
        }
    }
    bounded_ascii_cord_ops(receiver_ty, method, arg_count)
        .or_else(|| bounded_cord_appende(receiver_ty, method, arg_count))
}

/// Resolve a promoted intrinsic from a type-table receiver ID, peeling aliases
/// and access markers before receiver-family lookup.
#[must_use]
pub fn lookup_method_for_type_id(
    receiver_type: TypeId,
    types: &TypeTable,
    method: &str,
    arg_count: usize,
) -> Option<&'static MethodIntrinsic> {
    let (_, payload_id) = types.peel_access_marker(receiver_type);
    if matches!(types.get(payload_id), Type::ModularWord(_)) && !is_modular_word_method(method) {
        return None;
    }
    for family in receivers_for_type_id(receiver_type, types) {
        if let Some(entry) = lookup_method(*family, method, arg_count) {
            return Some(entry);
        }
    }
    let payload = types.get(payload_id);
    bounded_ascii_cord_ops(payload, method, arg_count)
        .or_else(|| bounded_cord_appende(payload, method, arg_count))
}

/// Bounded `ascii<N>` shares the live cord vocabulary with `textus`. Bare
/// `ascii` stays cursor-only (do not route unbounded ascii through Textus).
fn bounded_ascii_cord_ops(
    receiver_ty: &Type,
    method: &str,
    arg_count: usize,
) -> Option<&'static MethodIntrinsic> {
    if matches!(receiver_ty, Type::BoundedAscii { .. }) {
        lookup_method(IntrinsicReceiver::Textus, method, arg_count)
    } else {
        None
    }
}

/// Bounded cords reuse lista `appende` (Inner, Vacuum, Append) so overflow is
/// the same recoverable alternate exit. Unbounded `textus`/`ascii` stay
/// without `appende` — concatenation is `+`, not a capacity-checked grow.
fn bounded_cord_appende(
    receiver_ty: &Type,
    method: &str,
    arg_count: usize,
) -> Option<&'static MethodIntrinsic> {
    if method == "appende"
        && arg_count == 1
        && matches!(
            receiver_ty,
            Type::BoundedTextus { .. } | Type::BoundedAscii { .. }
        )
    {
        lookup_method(IntrinsicReceiver::Lista, "appende", 1)
    } else {
        None
    }
}

/// Modular words expose only operations with an explicit u32 lowering. Do not
/// let the shared numeric registry admit methods whose Rust implementation
/// would rely on signed/float behavior or an unproven target contract.
fn is_modular_word_method(method: &str) -> bool {
    matches!(
        method,
        "addita"
            | "subtracta"
            | "multiplicata"
            | "divisa"
            | "modulata"
            | "negativa"
            | "adde"
            | "subtrahe"
            | "multiplica"
            | "divida"
            | "modula"
            | "nega"
            | "minor"
            | "maior"
            | "minime"
            | "maxime"
            | "coniuncta"
            | "disiuncta"
            | "alternata"
            | "complementata"
            | "coniunge"
            | "disiunge"
            | "alterna"
            | "complementa"
            | "sinistrata"
            | "dextrata"
            | "sinistra"
            | "dextra"
    )
}

/// Return the MIR collection op for a registry row, when MIR models the method.
#[must_use]
pub fn mir_collection_op(entry: &MethodIntrinsic) -> Option<MirCollectionOp> {
    match entry.mir_kind {
        IntrinsicMirKind::Collection(op) => Some(op),
        IntrinsicMirKind::Atomic(_) => None,
        IntrinsicMirKind::HigherOrderAll
        | IntrinsicMirKind::HigherOrderAny
        | IntrinsicMirKind::HigherOrderFiltrata
        | IntrinsicMirKind::HigherOrderMap
        | IntrinsicMirKind::HigherOrderReduce
        | IntrinsicMirKind::HigherOrderCumulata
        | IntrinsicMirKind::CodegenOnly => None,
    }
}

/// Return the MIR atomic op for a registry row, when MIR models the method.
#[must_use]
pub fn mir_atomic_op(entry: &MethodIntrinsic) -> Option<MirAtomicOp> {
    match entry.mir_kind {
        IntrinsicMirKind::Atomic(op) => Some(op),
        IntrinsicMirKind::Collection(_)
        | IntrinsicMirKind::HigherOrderAll
        | IntrinsicMirKind::HigherOrderAny
        | IntrinsicMirKind::HigherOrderFiltrata
        | IntrinsicMirKind::HigherOrderMap
        | IntrinsicMirKind::HigherOrderReduce
        | IntrinsicMirKind::HigherOrderCumulata
        | IntrinsicMirKind::CodegenOnly => None,
    }
}

/// Whether MIR lowers this method through the higher-order lista helpers.
#[must_use]
pub fn mir_is_higher_order_lista(entry: &MethodIntrinsic) -> bool {
    matches!(
        entry.mir_kind,
        IntrinsicMirKind::HigherOrderAll
            | IntrinsicMirKind::HigherOrderAny
            | IntrinsicMirKind::HigherOrderFiltrata
            | IntrinsicMirKind::HigherOrderMap
            | IntrinsicMirKind::HigherOrderReduce
            | IntrinsicMirKind::HigherOrderCumulata
    )
}

/// Catalog fact: this intrinsic mutates its receiver in place.
///
/// Authority is the registry row (`return_kind` and in-place MIR ops), never a
/// post-typecheck void-return heuristic on a call expression. `Lista "appende"`
/// is the named example: `IntrinsicReturnKind::Vacuum`.
#[must_use]
pub fn is_mutating_receiver(entry: &MethodIntrinsic) -> bool {
    match entry.return_kind {
        IntrinsicReturnKind::Vacuum => true,
        _ => matches!(
            entry.mir_kind,
            IntrinsicMirKind::Collection(
                MirCollectionOp::Delete
                    | MirCollectionOp::RemoveFirst
                    | MirCollectionOp::RemoveLast
            ) | IntrinsicMirKind::Atomic(MirAtomicOp::Exchange | MirAtomicOp::CompareExchange)
        ),
    }
}

/// Imperativus numeric operator methods that mutate a `varia` receiver in place.
#[must_use]
pub fn is_numeric_imperativus_operator(method: &str) -> bool {
    matches!(
        method,
        "adde"
            | "subtrahe"
            | "multiplica"
            | "divida"
            | "modula"
            | "nega"
            | "coniunge"
            | "disiunge"
            | "alterna"
            | "complementa"
            | "dextra"
            | "sinistra"
    )
}

/// Bitwise and shift operator methods (catalog + receiver) — bare `numerus` only in v1.
#[must_use]
pub fn is_numeric_bitwise_operator(method: &str) -> bool {
    matches!(
        method,
        "coniuncta"
            | "coniunge"
            | "disiuncta"
            | "disiunge"
            | "alternata"
            | "alterna"
            | "complementata"
            | "complementa"
            | "dextra"
            | "dextrata"
            | "sinistra"
            | "sinistrata"
    )
}

/// Compiler-owned atomic method names reserved for `atomic<T>` receivers.
#[must_use]
pub fn is_atomic_operator_method(method: &str) -> bool {
    matches!(method, "load" | "store" | "exchange" | "compare_exchange")
}

#[cfg(test)]
#[path = "catalog_invariants_test.rs"]
mod catalog_invariants_tests;
