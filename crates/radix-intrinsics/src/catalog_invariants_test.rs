//! Pure catalog invariants for the intrinsic registry (leaf-owned proof).
//!
//! The driver-integration suites (typecheck-through-pipeline, emitted Rust)
//! stay hosted in `radix-module/src/intrinsics/` until `radix-semantic`
//! extracts; this file proves the catalog's own contracts without the
//! compiler driver.

use super::{
    is_atomic_operator_method, is_mutating_receiver, lookup_method, lookup_method_for_type,
    lookup_method_for_type_id, mir_atomic_op, mir_collection_op, mir_is_higher_order_lista,
    receivers_for_type, receivers_for_type_id, IntrinsicMirKind, IntrinsicParamKind,
    IntrinsicReceiver, IntrinsicReturnKind, INTRINSICS,
};
use radix_types::{IndexExpr, MirAtomicOp, MirCollectionOp, Primitive, SemanticMutability, Type};

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
fn cumulata_is_a_lista_scan_row() {
    let entry =
        lookup_method(IntrinsicReceiver::Lista, "cumulata", 2).expect("lista cumulata scan row");
    assert_eq!(
        entry.params,
        &[IntrinsicParamKind::Reducer, IntrinsicParamKind::Accumulator]
    );
    assert_eq!(entry.return_kind, IntrinsicReturnKind::ReducerArray);
    assert_eq!(entry.mir_kind, IntrinsicMirKind::HigherOrderCumulata);
    let mut types = radix_types::TypeTable::new();
    let element = types.intern(Type::Primitive(Primitive::Numerus));
    let lista = Type::Array(element);
    assert_eq!(
        lookup_method_for_type(&lista, "cumulata", 2).map(|row| row.faber_name),
        Some("cumulata")
    );
    assert!(lookup_method(IntrinsicReceiver::Tensor, "cumulata", 2).is_none());
}

#[test]
fn receivers_for_type_maps_semantic_families() {
    let mut types = radix_types::TypeTable::new();
    let element = types.intern(Type::Primitive(Primitive::Numerus));
    let lista = Type::Array(element);
    assert_eq!(receivers_for_type(&lista), &[IntrinsicReceiver::Lista]);
    let capacity = types.intern_index(IndexExpr::Literal(8));
    let bounded = Type::BoundedArray { element, capacity };
    assert_eq!(receivers_for_type(&bounded), &[IntrinsicReceiver::Lista]);
    let bounded_textus = Type::BoundedTextus { capacity };
    assert_eq!(
        receivers_for_type(&bounded_textus),
        &[IntrinsicReceiver::Textus]
    );
    let bounded_ascii = Type::BoundedAscii { capacity };
    assert_eq!(
        receivers_for_type(&bounded_ascii),
        &[IntrinsicReceiver::Ascii]
    );
    let bounded_octeti = Type::BoundedOcteti { capacity };
    assert_eq!(
        receivers_for_type(&bounded_octeti),
        &[IntrinsicReceiver::Lista]
    );
    assert_eq!(
        lookup_method_for_type(&bounded_ascii, "longitudo", 0)
            .expect("ascii<N>.longitudo")
            .faber_name,
        "longitudo"
    );
    assert_eq!(
        lookup_method_for_type(&bounded_textus, "appende", 1)
            .expect("textus<N>.appende")
            .faber_name,
        "appende"
    );
    let fractus = Type::Primitive(Primitive::Fractus);
    assert_eq!(
        receivers_for_type(&fractus),
        &[IntrinsicReceiver::Fractus, IntrinsicReceiver::Numeric]
    );
    let queue = Type::Queue(element);
    let stack = Type::Stack(element);
    let bounded_queue = Type::BoundedQueue { element, capacity };
    let bounded_stack = Type::BoundedStack { element, capacity };
    assert_eq!(receivers_for_type(&queue), &[IntrinsicReceiver::Queue]);
    assert_eq!(
        receivers_for_type(&bounded_queue),
        &[IntrinsicReceiver::Queue]
    );
    assert_eq!(receivers_for_type(&stack), &[IntrinsicReceiver::Stack]);
    assert_eq!(
        receivers_for_type(&bounded_stack),
        &[IntrinsicReceiver::Stack]
    );
    assert!(receivers_for_type(&Type::Error).is_empty());
}

#[test]
fn queue_and_stack_methods_share_core_collection_walkers() {
    for (receiver, push, pop, push_op, pop_op) in [
        (
            IntrinsicReceiver::Queue,
            "appende",
            "decapita",
            MirCollectionOp::Append,
            MirCollectionOp::RemoveFirst,
        ),
        (
            IntrinsicReceiver::Stack,
            "appende",
            "detrahe",
            MirCollectionOp::Append,
            MirCollectionOp::RemoveLast,
        ),
    ] {
        let push_entry = lookup_method(receiver, push, 1).expect("queue/stack push row");
        assert_eq!(push_entry.return_kind, IntrinsicReturnKind::Vacuum);
        assert_eq!(mir_collection_op(push_entry), Some(push_op));
        let pop_entry = lookup_method(receiver, pop, 0).expect("queue/stack pop row");
        assert_eq!(pop_entry.return_kind, IntrinsicReturnKind::OptionInner);
        assert_eq!(mir_collection_op(pop_entry), Some(pop_op));
        let length_entry = lookup_method(receiver, "longitudo", 0).expect("queue/stack length row");
        assert_eq!(length_entry.return_kind, IntrinsicReturnKind::Numerus);
        assert_eq!(
            mir_collection_op(length_entry),
            Some(MirCollectionOp::Length)
        );
    }
}

#[test]
fn marked_receiver_dispatch_peels_access_marker() {
    let mut types = radix_types::TypeTable::new();
    let element = types.intern(Type::Primitive(Primitive::Numerus));
    let lista = types.intern(Type::Array(element));
    let marked = types.reference(SemanticMutability::Mutable, lista);

    assert_eq!(
        receivers_for_type_id(marked, &types),
        &[IntrinsicReceiver::Lista]
    );
    let entry = lookup_method_for_type_id(marked, &types, "longitudo", 0)
        .expect("marked lista receiver resolves read intrinsic");
    assert_eq!(entry.receiver, IntrinsicReceiver::Lista);
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

/// MC1 ratchet, catalog side: the five registry rows on the unit-cited
/// ratchet (module-lane bijection test derives the gap set by probing the
/// real module MIR lowerer) are `CodegenOnly` and unclassified by every MIR
/// accessor — the catalog shape of "registry row with no owner".
#[test]
fn ratchet_registry_rows_are_codegen_only_and_unclassified() {
    use super::IntrinsicMirKind;
    for (receiver, name, arity, unit) in [
        (IntrinsicReceiver::Numeric, "nega", 0, "MC2"),
        (IntrinsicReceiver::Numeric, "negativa", 0, "MC2"),
        (IntrinsicReceiver::Numeric, "complementa", 0, "MC2"),
        (IntrinsicReceiver::Numeric, "complementata", 0, "MC2"),
        (IntrinsicReceiver::Vector, "crea", 0, "MC3"),
    ] {
        let entry = lookup_method(receiver, name, arity).unwrap_or_else(|| {
            panic!("ratchet row {name}/0 must stay in the registry until {unit} lands")
        });
        assert_eq!(
            entry.mir_kind,
            IntrinsicMirKind::CodegenOnly,
            "{name}/0 must stay CodegenOnly until {unit} arms it"
        );
        assert_eq!(
            mir_collection_op(entry),
            None,
            "{name}/0 has no collection op"
        );
        assert_eq!(mir_atomic_op(entry), None, "{name}/0 has no atomic op");
        assert!(
            !mir_is_higher_order_lista(entry),
            "{name}/0 is not higher-order"
        );
    }
}

/// The eleven MC4 ratchet rows are AIR-lane method surface, NOT catalog rows:
/// the registry cannot own them, so direction 3 (AIR/device allowlist →
/// owner) is the only seam that can pin them. If one of these names gains a
/// Tensor registry row, the module-lane bijection probe re-derives the gap
/// set and this invariant forces the companion update.
#[test]
fn air_ratchet_rows_are_absent_from_the_tensor_catalog() {
    for name in [
        "maxima",
        "minima",
        "exponentia",
        "logarithmus",
        "radix",
        "activatio_tanh",
        "summa_axis",
        "reducta",
        "cumulata",
        "mappata",
        "filtrata",
    ] {
        for arity in 0..=4 {
            assert!(
                lookup_method(IntrinsicReceiver::Tensor, name, arity).is_none(),
                "AIR ratchet row {name} gained a Tensor registry row — update the MC1 ratchet"
            );
        }
    }
}
