//! Pure catalog invariants for the intrinsic registry (leaf-owned proof).
//!
//! The driver-integration suites (typecheck-through-pipeline, emitted Rust)
//! stay hosted in `radix-module/src/intrinsics/` until `radix-semantic`
//! extracts; this file proves the catalog's own contracts without the
//! compiler driver.

use std::collections::HashSet;

use super::{
    CatalogGenericParam, INTRINSICS, IntrinsicMirKind, IntrinsicParamKind, IntrinsicReceiver,
    IntrinsicReturnKind, MethodIntrinsic, NAMESPACE_INTRINSICS, NamespaceIntrinsic,
    first_bucket_row, is_atomic_operator_method, is_mutating_receiver, lookup_method,
    lookup_method_for_type, lookup_method_for_type_id, lookup_method_generic, lookup_method_named,
    lookup_namespace_method, lookup_namespace_method_by_name, method_index, method_positions,
    mir_atomic_op, mir_collection_op, mir_is_higher_order_lista, namespace_index,
    namespace_positions, receivers_for_type, receivers_for_type_id, unique_bucket_row,
};
use radix_types::{
    GenericParamSort, IndexExpr, MirAtomicOp, MirCollectionOp, Primitive, SemanticMutability, Type,
};

#[test]
fn registry_rows_are_unique_per_receiver_name_generic_arity_and_value_arity() {
    let mut seen = std::collections::HashSet::new();
    for entry in INTRINSICS {
        let key = (
            entry.receiver,
            entry.faber_name,
            entry.generic.len(),
            entry.arg_count,
        );
        assert!(seen.insert(key), "duplicate registry row: {key:?}");
    }
}

/// The `intrinsic!` helper centralizes the policy defaults (`failable`,
/// `range_checked_arg`, and the three generic-diagnostic slots). Only the rows
/// named here may deviate, so a row that grows a policy field has to be added
/// deliberately instead of inheriting one by accident.
#[test]
fn macro_rows_keep_default_policy_unless_named() {
    let special: HashSet<(IntrinsicReceiver, &str, usize)> = [
        (IntrinsicReceiver::Lista, "apud", 1),
        (IntrinsicReceiver::Tabula, "apud", 1),
        (IntrinsicReceiver::Textus, "apud", 1),
        (IntrinsicReceiver::Tensor, "accipe", 1),
        (IntrinsicReceiver::Tensor, "ponde", 2),
        (IntrinsicReceiver::Tensor, "formata", 0),
        (IntrinsicReceiver::Tensor, "rope_norm", 2),
        (IntrinsicReceiver::Tensor, "limes", 0),
    ]
    .into_iter()
    .collect();
    for entry in INTRINSICS {
        let key = (entry.receiver, entry.faber_name, entry.arg_count);
        if special.contains(&key) {
            continue;
        }
        assert_eq!(entry.missing_generic_issue, None, "{key:?}");
        assert_eq!(entry.excess_generic_issue, None, "{key:?}");
        assert_eq!(entry.value_arity_issue, None, "{key:?}");
        assert!(!entry.failable, "{key:?}");
        assert_eq!(entry.range_checked_arg, None, "{key:?}");
    }
}

#[test]
fn empty_generic_rows_reject_angle_brackets_and_declared_rows_are_explicit() {
    let formata = lookup_method(IntrinsicReceiver::Tensor, "formata", 0).expect("formata");
    assert_eq!(formata.generic_arity(), 1);
    assert_eq!(formata.generic[0].sort, GenericParamSort::Typus);
    let limes = lookup_method(IntrinsicReceiver::Tensor, "limes", 0).expect("limes");
    assert_eq!(limes.generic_arity(), 1);
    assert_eq!(limes.generic[0].sort, GenericParamSort::Typus);
    for entry in INTRINSICS {
        if matches!(entry.faber_name, "formata" | "rope_norm" | "limes") {
            continue;
        }
        assert!(
            entry.generic.is_empty(),
            "{:?}::{} keeps an empty generic list",
            entry.receiver,
            entry.faber_name
        );
    }
}

#[test]
fn namespace_catalog_rows_are_unique_and_generic() {
    let mut seen = std::collections::HashSet::new();
    for entry in NAMESPACE_INTRINSICS {
        let key = (
            entry.namespace,
            entry.faber_name,
            entry.generic.len(),
            entry.arg_count,
        );
        assert!(seen.insert(key), "duplicate namespace row: {key:?}");
        assert!(
            !entry.generic.is_empty(),
            "{}::{} must declare generics",
            entry.namespace,
            entry.faber_name
        );
    }
    assert!(lookup_namespace_method("tensor", "creata", 1).is_some());
    assert!(lookup_namespace_method_by_name("sermo", "tuus").is_some());
    assert!(lookup_namespace_method("tensor", "creata", 2).is_none());
}

#[test]
fn lookup_resolves_registered_rows() {
    assert!(lookup_method(IntrinsicReceiver::Tabula, "claves", 0).is_some());
    assert!(lookup_method(IntrinsicReceiver::Regex, "consentit", 1).is_some());
    assert!(lookup_method(IntrinsicReceiver::Regex, "inventa", 1).is_some());
    assert!(lookup_method(IntrinsicReceiver::Match, "inventum", 0).is_some());
    assert!(lookup_method(IntrinsicReceiver::Match, "coetus", 1).is_some());
    assert!(
        lookup_method(IntrinsicReceiver::Tabula, "claves", 1).is_none(),
        "arity must be exact"
    );
    assert!(lookup_method(IntrinsicReceiver::Lista, "nonexistent", 0).is_none());
}

#[test]
fn lookup_generic_arity_distinguishes_empty_rows() {
    assert!(lookup_method_generic(IntrinsicReceiver::Lista, "longitudo", 0, 0).is_some());
    assert!(
        lookup_method_generic(IntrinsicReceiver::Lista, "longitudo", 1, 0).is_none(),
        "empty-generic rows do not match a nonempty generic arity"
    );
    let rope = lookup_method_generic(IntrinsicReceiver::Tensor, "rope_norm", 1, 2)
        .expect("generic rope_norm row");
    assert_eq!(rope.generic[0].sort, GenericParamSort::Magnitudo);
    assert!(lookup_method_generic(IntrinsicReceiver::Tensor, "rope_norm", 0, 2).is_none());
}

#[test]
fn catalog_generic_param_sorts_are_declaration_data() {
    const DIM: &[CatalogGenericParam] = &[CatalogGenericParam::magnitudo("dim")];
    let typus = CatalogGenericParam::typus("T");
    let magnitudo = CatalogGenericParam::magnitudo("dim");
    assert_eq!(typus.sort, GenericParamSort::Typus);
    assert_eq!(magnitudo.sort, GenericParamSort::Magnitudo);
    let empty_row = lookup_method(IntrinsicReceiver::Lista, "longitudo", 0).expect("longitudo");
    let declared = MethodIntrinsic {
        generic: DIM,
        ..*empty_row
    };
    assert_eq!(empty_row.generic_arity(), 0);
    assert_eq!(declared.generic_arity(), 1);
    assert_ne!(
        (
            empty_row.receiver,
            empty_row.faber_name,
            empty_row.generic.len(),
            empty_row.arg_count
        ),
        (
            declared.receiver,
            declared.faber_name,
            declared.generic.len(),
            declared.arg_count
        ),
        "generic arity is part of the uniqueness key"
    );
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
fn predicate_twins_are_boolean_lista_higher_order_rows() {
    for (name, kind) in [
        ("omnia", IntrinsicMirKind::HigherOrderAll),
        ("quilibet", IntrinsicMirKind::HigherOrderAny),
    ] {
        let entry = lookup_method(IntrinsicReceiver::Lista, name, 1)
            .unwrap_or_else(|| panic!("lista {name} predicate row"));
        assert_eq!(entry.params, &[IntrinsicParamKind::Predicate]);
        assert_eq!(entry.return_kind, IntrinsicReturnKind::Bool);
        assert_eq!(entry.mir_kind, kind);
        assert!(mir_is_higher_order_lista(entry));
        assert_eq!(mir_collection_op(entry), None);
        assert_eq!(mir_atomic_op(entry), None);
    }

    let mut types = radix_types::TypeTable::new();
    let element = types.intern(Type::Primitive(Primitive::Numerus));
    let lista = Type::Array(element);
    assert_eq!(
        lookup_method_for_type(&lista, "omnia", 1).map(|row| row.faber_name),
        Some("omnia")
    );
    assert_eq!(
        lookup_method_for_type(&lista, "quilibet", 1).map(|row| row.faber_name),
        Some("quilibet")
    );
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
    // ascii<N> shares the textus cord vocabulary, so the family list names
    // Textus too: a family-scoped consumer (the locale pack's surface spelling
    // lookup) must see `length` / `is_empty` on it.
    assert_eq!(
        receivers_for_type(&bounded_ascii),
        &[IntrinsicReceiver::Ascii, IntrinsicReceiver::Textus]
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

/// The MC4 ratchet rows (eleven at MC4; the LSR-W0 twins joined) are AIR-lane
/// method surface, NOT catalog rows: the registry cannot own them on the
/// Tensor receiver, so direction 3 (AIR/device allowlist → owner) is the only
/// seam that can pin them. If one of these names gains a
/// Tensor registry row, the module-lane bijection probe re-derives the gap
/// set and this invariant forces the companion update.
#[test]
fn air_ratchet_rows_are_absent_from_the_tensor_catalog() {
    for name in [
        "maxima",
        "minima",
        // LSR-W0: twins own Lista rows only; the Tensor receiver stays
        // without a row until the device lane admits them.
        "argmaxima",
        "argminima",
        // K5 armed `exponentia`, `logarithmus` and `radix` as Tensor rows
        // (the lifted math functions); they left this list with that unit.
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

// PD-9: the five keyed lookups must stay byte-identical to the catalog's
// linear-scan semantics. The references below mirror the pre-index scan
// bodies exactly; catalog rows carry no `PartialEq`, so results are compared
// as catalog positions (pointer identity into the static catalog).

fn method_position(entry: Option<&'static MethodIntrinsic>) -> Option<usize> {
    entry.map(|row| {
        INTRINSICS
            .iter()
            .position(|candidate| std::ptr::eq(candidate, row))
            .expect("row is a catalog member")
    })
}

fn namespace_position(entry: Option<&'static NamespaceIntrinsic>) -> Option<usize> {
    entry.map(|row| {
        NAMESPACE_INTRINSICS
            .iter()
            .position(|candidate| std::ptr::eq(candidate, row))
            .expect("namespace row is a catalog member")
    })
}

fn linear_lookup_method(
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

fn linear_lookup_method_named(
    receiver: IntrinsicReceiver,
    method: &str,
) -> Option<&'static MethodIntrinsic> {
    let mut found = None;
    for entry in INTRINSICS {
        if entry.receiver == receiver && entry.faber_name == method {
            if found.is_some() {
                return None;
            }
            found = Some(entry);
        }
    }
    found
}

fn linear_lookup_method_generic(
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

fn linear_lookup_namespace_method(
    namespace: &str,
    method: &str,
    arg_count: usize,
) -> Option<&'static NamespaceIntrinsic> {
    let mut found = None;
    for entry in NAMESPACE_INTRINSICS {
        if entry.namespace == namespace
            && entry.faber_name == method
            && entry.arg_count == arg_count
        {
            if found.is_some() {
                return None;
            }
            found = Some(entry);
        }
    }
    found
}

fn linear_lookup_namespace_method_by_name(
    namespace: &str,
    method: &str,
) -> Option<&'static NamespaceIntrinsic> {
    let mut found = None;
    for entry in NAMESPACE_INTRINSICS {
        if entry.namespace == namespace && entry.faber_name == method {
            if found.is_some() {
                return None;
            }
            found = Some(entry);
        }
    }
    found
}

#[test]
fn keyed_lookups_match_linear_scan_reference_across_the_full_key_space() {
    let receivers: HashSet<IntrinsicReceiver> =
        INTRINSICS.iter().map(|entry| entry.receiver).collect();
    let mut method_names: Vec<&str> = INTRINSICS.iter().map(|entry| entry.faber_name).collect();
    method_names.extend(["nonexistent", "unregistra"]);
    for receiver in &receivers {
        for name in &method_names {
            for arg_count in 0..=5 {
                assert_eq!(
                    method_position(lookup_method(*receiver, name, arg_count)),
                    method_position(linear_lookup_method(*receiver, name, arg_count)),
                    "lookup_method({receiver:?}, {name}, {arg_count}) drifted from the linear reference"
                );
                assert_eq!(
                    method_position(lookup_method_named(*receiver, name)),
                    method_position(linear_lookup_method_named(*receiver, name)),
                    "lookup_method_named({receiver:?}, {name}) drifted from the linear reference"
                );
            }
            for generic_count in 0..=2 {
                for arg_count in 0..=5 {
                    assert_eq!(
                        method_position(lookup_method_generic(
                            *receiver,
                            name,
                            generic_count,
                            arg_count
                        )),
                        method_position(linear_lookup_method_generic(
                            *receiver,
                            name,
                            generic_count,
                            arg_count
                        )),
                        "lookup_method_generic({receiver:?}, {name}, {generic_count}, {arg_count}) drifted from the linear reference"
                    );
                }
            }
        }
    }

    let namespaces: HashSet<&str> = NAMESPACE_INTRINSICS
        .iter()
        .map(|entry| entry.namespace)
        .collect();
    let mut namespace_names: Vec<&str> = NAMESPACE_INTRINSICS
        .iter()
        .map(|entry| entry.faber_name)
        .collect();
    namespace_names.extend(["nonexistent", "unregistra"]);
    for namespace in &namespaces {
        for name in &namespace_names {
            for arg_count in 0..=3 {
                assert_eq!(
                    namespace_position(lookup_namespace_method(namespace, name, arg_count)),
                    namespace_position(linear_lookup_namespace_method(namespace, name, arg_count)),
                    "lookup_namespace_method({namespace}, {name}, {arg_count}) drifted from the linear reference"
                );
                assert_eq!(
                    namespace_position(lookup_namespace_method_by_name(namespace, name)),
                    namespace_position(linear_lookup_namespace_method_by_name(namespace, name)),
                    "lookup_namespace_method_by_name({namespace}, {name}) drifted from the linear reference"
                );
            }
        }
    }
}

#[test]
fn keyed_index_buckets_keep_catalog_order_and_full_membership() {
    for (receiver, rows) in method_index() {
        for (name, positions) in rows {
            assert!(!positions.is_empty(), "empty bucket {receiver:?}/{name}");
            for pair in positions.windows(2) {
                assert!(
                    pair[0] < pair[1],
                    "bucket {receiver:?}/{name} left catalog order"
                );
            }
            for &position in positions {
                let row = &INTRINSICS[position];
                assert_eq!(row.receiver, *receiver);
                assert_eq!(row.faber_name, *name);
            }
        }
    }
    for (position, entry) in INTRINSICS.iter().enumerate() {
        assert!(
            method_positions(entry.receiver, entry.faber_name).contains(&position),
            "row {position} ({:?}/{}) missing from its key bucket",
            entry.receiver,
            entry.faber_name
        );
    }

    for (namespace, rows) in namespace_index() {
        for (name, positions) in rows {
            assert!(!positions.is_empty(), "empty bucket {namespace}/{name}");
            for pair in positions.windows(2) {
                assert!(
                    pair[0] < pair[1],
                    "bucket {namespace}/{name} left catalog order"
                );
            }
            for &position in positions {
                let row = &NAMESPACE_INTRINSICS[position];
                assert_eq!(row.namespace, *namespace);
                assert_eq!(row.faber_name, *name);
            }
        }
    }
    for (position, entry) in NAMESPACE_INTRINSICS.iter().enumerate() {
        assert!(
            namespace_positions(entry.namespace, entry.faber_name).contains(&position),
            "namespace row {position} ({}/{}) missing from its key bucket",
            entry.namespace,
            entry.faber_name
        );
    }
}

#[test]
fn synthetic_duplicates_return_none_and_first_match_keeps_catalog_position() {
    // The real catalogs cannot carry duplicate keys (uniqueness invariants
    // above), so the duplicate-key and first-match semantics are pinned on a
    // synthetic twin pair through the shared bucket helpers.
    let template = *lookup_method(IntrinsicReceiver::Lista, "longitudo", 0).expect("template row");
    let catalog: &[MethodIntrinsic] = &[
        MethodIntrinsic { ..template },
        MethodIntrinsic { ..template },
    ];

    assert!(
        unique_bucket_row(&[0, 1], catalog, |_| true).is_none(),
        "a duplicate-key bucket must resolve to None"
    );
    assert!(std::ptr::eq(
        unique_bucket_row(&[1], catalog, |_| true).expect("unique row"),
        &catalog[1]
    ));
    assert!(std::ptr::eq(
        first_bucket_row(&[0, 1], catalog, |_| true).expect("first match"),
        &catalog[0]
    ));
    assert!(
        std::ptr::eq(
            first_bucket_row(&[1, 0], catalog, |_| true).expect("first match"),
            &catalog[1]
        ),
        "first match follows bucket (catalog) order, not row identity"
    );
}
