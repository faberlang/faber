//! Compiler-owned receiver intrinsic registry.
//!
//! Radix promotes a fixed catalog of `lista`, `queue`, `stack`, `tabula`,
//! `copia`, `textus`, `ascii`, `numerus`, `fractus`, `tensor`, `matrix`,
//! `vector`, `intervallum`, and `atomic`
//! methods. Semantic analysis, MIR lowering, and target backends consult this
//! module instead of maintaining parallel string tables.

mod registry;

pub use registry::{
    CatalogGenericDefault, CatalogGenericParam, INTRINSICS, IntrinsicMirKind, IntrinsicParamKind,
    IntrinsicReceiver, IntrinsicReturnKind, MethodIntrinsic, NAMESPACE_INTRINSICS,
    NamespaceIntrinsic, NamespaceReturnKind, SERMO_VIEW_METHODS, SermoViewKind, SermoViewMethod,
    TENSOR_CONSTRUCTOR_ENTRY,
};

use std::collections::HashMap;
use std::sync::OnceLock;

use radix_types::{MirAtomicOp, MirCollectionOp, Primitive, Type, TypeId, TypeTable};

/// Once-built key index over [`INTRINSICS`]: catalog row positions grouped by
/// `(receiver, faber_name)`, every bucket in catalog order. The static catalog
/// stays the single source of truth; `registry.rs` is never mutated.
fn method_index() -> &'static HashMap<IntrinsicReceiver, HashMap<&'static str, Vec<usize>>> {
    static INDEX: OnceLock<HashMap<IntrinsicReceiver, HashMap<&'static str, Vec<usize>>>> =
        OnceLock::new();
    INDEX.get_or_init(|| {
        let mut index: HashMap<IntrinsicReceiver, HashMap<&'static str, Vec<usize>>> =
            HashMap::new();
        for (position, entry) in INTRINSICS.iter().enumerate() {
            index
                .entry(entry.receiver)
                .or_default()
                .entry(entry.faber_name)
                .or_default()
                .push(position);
        }
        index
    })
}

/// Bucket positions for a `(receiver, name)` key, empty when unregistered.
fn method_positions(receiver: IntrinsicReceiver, method: &str) -> &'static [usize] {
    method_index()
        .get(&receiver)
        .and_then(|rows| rows.get(method))
        .map_or(&[], Vec::as_slice)
}

/// Once-built key index over [`NAMESPACE_INTRINSICS`]: catalog row positions
/// grouped by `(namespace, faber_name)`, every bucket in catalog order.
fn namespace_index() -> &'static HashMap<&'static str, HashMap<&'static str, Vec<usize>>> {
    static INDEX: OnceLock<HashMap<&'static str, HashMap<&'static str, Vec<usize>>>> =
        OnceLock::new();
    INDEX.get_or_init(|| {
        let mut index: HashMap<&'static str, HashMap<&'static str, Vec<usize>>> = HashMap::new();
        for (position, entry) in NAMESPACE_INTRINSICS.iter().enumerate() {
            index
                .entry(entry.namespace)
                .or_default()
                .entry(entry.faber_name)
                .or_default()
                .push(position);
        }
        index
    })
}

/// Bucket positions for a `(namespace, name)` key, empty when unregistered.
fn namespace_positions(namespace: &str, method: &str) -> &'static [usize] {
    namespace_index()
        .get(namespace)
        .and_then(|rows| rows.get(method))
        .map_or(&[], Vec::as_slice)
}

/// Unique-row semantics over a keyed bucket: `None` when the bucket holds
/// more than one row passing the filter.
fn unique_bucket_row<'a, T>(
    bucket: &[usize],
    catalog: &'a [T],
    keep: impl Fn(&T) -> bool,
) -> Option<&'a T> {
    let mut found = None;
    for &position in bucket {
        let entry = &catalog[position];
        if keep(entry) {
            if found.is_some() {
                return None;
            }
            found = Some(entry);
        }
    }
    found
}

/// First-match semantics over a keyed bucket: the earliest row in bucket
/// (catalog) order passing the filter.
fn first_bucket_row<'a, T>(
    bucket: &[usize],
    catalog: &'a [T],
    keep: impl Fn(&T) -> bool,
) -> Option<&'a T> {
    bucket
        .iter()
        .map(|&position| &catalog[position])
        .find(|entry| keep(entry))
}

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
    unique_bucket_row(method_positions(receiver, method), INTRINSICS, |entry| {
        entry.arg_count == arg_count
    })
}

/// Lookup the unique row for a receiver and name, ignoring value arity.
///
/// Returns `None` when two rows share the name with different value arities.
#[must_use]
pub fn lookup_method_named(
    receiver: IntrinsicReceiver,
    method: &str,
) -> Option<&'static MethodIntrinsic> {
    unique_bucket_row(method_positions(receiver, method), INTRINSICS, |_| true)
}

/// Lookup a promoted intrinsic by receiver, name, generic arity, and value arity.
#[must_use]
pub fn lookup_method_generic(
    receiver: IntrinsicReceiver,
    method: &str,
    generic_count: usize,
    arg_count: usize,
) -> Option<&'static MethodIntrinsic> {
    first_bucket_row(method_positions(receiver, method), INTRINSICS, |entry| {
        entry.generic.len() == generic_count && entry.arg_count == arg_count
    })
}

/// Lookup a namespace/struct catalog row by owner, name, and value arity.
#[must_use]
pub fn lookup_namespace_method(
    namespace: &str,
    method: &str,
    arg_count: usize,
) -> Option<&'static NamespaceIntrinsic> {
    unique_bucket_row(
        namespace_positions(namespace, method),
        NAMESPACE_INTRINSICS,
        |entry| entry.arg_count == arg_count,
    )
}

/// Lookup a namespace/struct catalog row by name, ignoring value arity.
#[must_use]
pub fn lookup_namespace_method_by_name(
    namespace: &str,
    method: &str,
) -> Option<&'static NamespaceIntrinsic> {
    unique_bucket_row(
        namespace_positions(namespace, method),
        NAMESPACE_INTRINSICS,
        |_| true,
    )
}

/// Map a normalized semantic receiver type to intrinsic receiver families.
///
/// `fractus` values may satisfy both [`IntrinsicReceiver::Fractus`] and
/// [`IntrinsicReceiver::Numeric`] rows; callers should try fractus-specific rows
/// before shared numeric rows.
#[must_use]
pub fn receivers_for_type(ty: &Type) -> &'static [IntrinsicReceiver] {
    match ty {
        Type::Array(_) | Type::BoundedArray { .. } | Type::RangedArray { .. } => {
            &[IntrinsicReceiver::Lista]
        }
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
        Type::Primitive(Primitive::Ascii) => &[IntrinsicReceiver::Ascii],
        // WHY: bounded `ascii<N>` shares the live cord vocabulary with `textus`
        // (bare `ascii` stays cursor-only). Naming the Textus family here lets
        // every family-scoped consumer — notably the locale pack's
        // surface→canonical spelling lookup — see `length` / `is_empty` on it,
        // not just the registry row lookup.
        Type::BoundedAscii { .. } => &[IntrinsicReceiver::Ascii, IntrinsicReceiver::Textus],
        Type::Primitive(Primitive::Regex) => &[IntrinsicReceiver::Regex],
        Type::Primitive(Primitive::Match) => &[IntrinsicReceiver::Match],
        Type::Primitive(Primitive::Numerus)
        | Type::SizedNumeric(Primitive::Numerus, _)
        | Type::ModularWord(_) => &[IntrinsicReceiver::Numeric],
        Type::Primitive(Primitive::Fractus) | Type::SizedNumeric(Primitive::Fractus, _) => {
            &[IntrinsicReceiver::Fractus, IntrinsicReceiver::Numeric]
        }
        Type::Tensor(_, _) => &[IntrinsicReceiver::Tensor],
        Type::Matrix(_, _) => &[IntrinsicReceiver::Matrix],
        Type::Vector(_, _) => &[IntrinsicReceiver::Vector],
        Type::Sparsa(_, _) => &[IntrinsicReceiver::Sparsa],
        Type::Intervallum(..) => &[IntrinsicReceiver::Intervallum],
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
    bounded_cord_appende(receiver_ty, method, arg_count)
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
    bounded_cord_appende(payload, method, arg_count)
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
        IntrinsicMirKind::Atomic(_)
        | IntrinsicMirKind::HigherOrderAll
        | IntrinsicMirKind::HigherOrderAny
        | IntrinsicMirKind::HigherOrderFind
        | IntrinsicMirKind::ListExtend
        | IntrinsicMirKind::HigherOrderFiltrata
        | IntrinsicMirKind::HigherOrderMap
        | IntrinsicMirKind::HigherOrderReduce
        | IntrinsicMirKind::HigherOrderSortBy
        | IntrinsicMirKind::HigherOrderCumulata
        | IntrinsicMirKind::TensorLimes
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
        | IntrinsicMirKind::HigherOrderFind
        | IntrinsicMirKind::ListExtend
        | IntrinsicMirKind::HigherOrderFiltrata
        | IntrinsicMirKind::HigherOrderMap
        | IntrinsicMirKind::HigherOrderReduce
        | IntrinsicMirKind::HigherOrderSortBy
        | IntrinsicMirKind::HigherOrderCumulata
        | IntrinsicMirKind::TensorLimes
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
            | IntrinsicMirKind::HigherOrderFind
            | IntrinsicMirKind::HigherOrderFiltrata
            | IntrinsicMirKind::HigherOrderMap
            | IntrinsicMirKind::HigherOrderReduce
            | IntrinsicMirKind::HigherOrderSortBy
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

#[cfg(test)]
#[path = "tensor_math_test.rs"]
mod tensor_math_tests;
