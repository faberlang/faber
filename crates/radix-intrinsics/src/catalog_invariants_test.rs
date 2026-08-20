//! Pure catalog invariants for the intrinsic registry (leaf-owned proof).
//!
//! The driver-integration suites (typecheck-through-pipeline, emitted Rust)
//! stay hosted in `radix-module/src/intrinsics/` until `radix-semantic`
//! extracts; this file proves the catalog's own contracts without the
//! compiler driver.

use super::{
    is_atomic_operator_method, is_mutating_receiver, lookup_method, lookup_method_for_type,
    mir_atomic_op, mir_collection_op, mir_is_higher_order_lista, receivers_for_type,
    IntrinsicReceiver, INTRINSICS,
};
use radix_types::{MirAtomicOp, MirCollectionOp, Primitive, Type};

#[test]
fn registry_rows_are_unique_per_receiver_name_and_arity() {
    let mut seen = std::collections::HashSet::new();
    for entry in INTRINSICS {
        let key = (entry.receiver, entry.faber_name, entry.arg_count);
        assert!(seen.insert(key), "duplicate registry row: {key:?}");
    }
}

#[test]
fn lookup_resolves_registered_rows() {
    assert!(lookup_method(IntrinsicReceiver::Tabula, "claves", 0).is_some());
    assert!(lookup_method(IntrinsicReceiver::Regex, "consentit", 1).is_some());
    assert!(
        lookup_method(IntrinsicReceiver::Tabula, "claves", 1).is_none(),
        "arity must be exact"
    );
    assert!(lookup_method(IntrinsicReceiver::Lista, "nonexistent", 0).is_none());
}

#[test]
fn receivers_for_type_maps_semantic_families() {
    let mut types = radix_types::TypeTable::new();
    let element = types.intern(Type::Primitive(Primitive::Numerus));
    let lista = Type::Array(element);
    assert_eq!(receivers_for_type(&lista), &[IntrinsicReceiver::Lista]);
    let fractus = Type::Primitive(Primitive::Fractus);
    assert_eq!(
        receivers_for_type(&fractus),
        &[IntrinsicReceiver::Fractus, IntrinsicReceiver::Numeric]
    );
    assert!(receivers_for_type(&Type::Error).is_empty());
}

#[test]
fn lookup_for_type_prefers_fractus_rows_before_shared_numeric() {
    let fractus = Type::Primitive(Primitive::Fractus);
    let entry =
        lookup_method_for_type(&fractus, "rotunda", 0).expect("fractus rounding row resolves");
    assert_eq!(entry.receiver, IntrinsicReceiver::Fractus);
}

#[test]
fn mir_classification_accessors_partition_rows() {
    for entry in INTRINSICS {
        let collection = mir_collection_op(entry);
        let atomic = mir_atomic_op(entry);
        let higher_order = mir_is_higher_order_lista(entry);
        // A row is exactly one of: collection-op, atomic-op, higher-order, or codegen-only.
        let classified =
            u8::from(collection.is_some()) + u8::from(atomic.is_some()) + u8::from(higher_order);
        assert!(
            classified <= 1,
            "row {:?}/{} carries multiple MIR classifications",
            entry.receiver,
            entry.faber_name
        );
    }
    let summa = lookup_method(IntrinsicReceiver::Lista, "summa", 0).expect("lista summa row");
    assert_eq!(mir_collection_op(summa), Some(MirCollectionOp::Sum));
    let load = lookup_method(IntrinsicReceiver::Atomic, "load", 0).expect("atomic load row");
    assert_eq!(mir_atomic_op(load), Some(MirAtomicOp::Load));
}

#[test]
fn mutating_receiver_predicate_keys_on_catalog_rows() {
    let appende = lookup_method(IntrinsicReceiver::Lista, "appende", 1).expect("lista appende");
    assert!(
        is_mutating_receiver(appende),
        "Lista appende is the catalog mutating-receiver fact"
    );
    let longitudo =
        lookup_method(IntrinsicReceiver::Lista, "longitudo", 0).expect("lista longitudo");
    assert!(
        !is_mutating_receiver(longitudo),
        "Lista longitudo is a read"
    );
    let addita = lookup_method(IntrinsicReceiver::Lista, "addita", 1).expect("lista addita");
    assert!(
        !is_mutating_receiver(addita),
        "Lista addita returns a new list"
    );
    let detrahe = lookup_method(IntrinsicReceiver::Lista, "detrahe", 0).expect("lista detrahe");
    assert!(
        is_mutating_receiver(detrahe),
        "in-place pop mutates without a Vacuum return"
    );
}

#[test]
fn atomic_operator_predicate_admits_only_atomic_surface() {
    for method in ["load", "store", "exchange", "compare_exchange"] {
        assert!(
            is_atomic_operator_method(method),
            "{method} must be an atomic operator"
        );
    }
    assert!(
        !is_atomic_operator_method("fetch_add"),
        "unregistered atomic name must not pass"
    );
}
