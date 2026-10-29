//! [`TypeTable`] semantic contract coverage: assignability, equality, snapshots,
//! and type constructor behavior (bare, sized, instans, collection, atomic,
//! octeti, sparsa, matrix).
//!
//! Includes regression for Option→ignotum assignment (F2 regression) and
//! bidirectional octeti↔lista<numerus<u8>> unify.

use crate::BitsWidth;
use crate::DefId;
use crate::InstansPrecision;
use crate::NumericWidth;
use crate::TypeId;
use crate::index::{
    IndexArithmeticError, IndexExpr, IndexId, RuntimeExtentId, add_index_constant,
    compose_index_affine_constant, divide_index_exact, evaluate_index_dimension,
    i64s_from_literal_index,
};
use crate::{
    AccessMarker, AppliedArg, BuiltinTensorEdgePolicy, CollectionKind, FloatPolicy, FuncSig,
    GenericParamDescriptor, GenericParamSort, InferVar, IntPolicy, ParamType, Primitive,
    SemanticGenericParamDefault, SemanticMutability, SemanticParamMode, TensorFamilyKind, Type,
    TypeParamConstraint, TypeTable, collection_type_specs, is_atomic_element_type,
    is_intervallum_bound_type, lookup_collection_type_spec, lookup_primitive_type_spec,
    lookup_tensor_family_type_spec, primitive_type_specs, tensor_family_type_specs,
};

#[test]
fn tensor_edge_policy_markers_keep_compiler_identity_and_are_non_value_types() {
    let mut types = TypeTable::new();
    let clamp = types.tensor_edge_policy(BuiltinTensorEdgePolicy::Clamp);
    let reflect = types.tensor_edge_policy(BuiltinTensorEdgePolicy::Reflect);
    let wrap = types.tensor_edge_policy(BuiltinTensorEdgePolicy::Wrap);
    assert_eq!(
        types.tensor_edge_policy_kind(clamp),
        Some(BuiltinTensorEdgePolicy::Clamp)
    );
    assert_eq!(
        types.tensor_edge_policy_kind(reflect),
        Some(BuiltinTensorEdgePolicy::Reflect)
    );
    assert_eq!(
        types.tensor_edge_policy_kind(wrap),
        Some(BuiltinTensorEdgePolicy::Wrap)
    );

    let user_type = types.intern(Type::Struct(crate::DefId(9001)));
    assert_eq!(types.tensor_edge_policy_kind(user_type), None);
    assert!(!types.contains_tensor_edge_policy(user_type));

    let stored_marker = types.array(clamp);
    assert!(types.contains_tensor_edge_policy(clamp));
    assert!(types.contains_tensor_edge_policy(stored_marker));
    assert!(!types.contains_tensor_edge_policy(types.primitive(Primitive::Numerus)));
}

/// One unconstrained, default-free `typus` descriptor for `FuncSig` tests.
fn typus_param(symbol: radix_lexer::Symbol) -> GenericParamDescriptor {
    GenericParamDescriptor {
        name: symbol,
        sort: GenericParamSort::Typus,
        constraint: TypeParamConstraint::Any,
        default: None,
    }
}

#[test]
fn generic_applied_arg_preserves_index_kind() {
    let mut types = TypeTable::new();
    let base = types.intern(Type::Param(radix_lexer::Symbol(1)));
    let size = types.intern_index(IndexExpr::Literal(3));
    let applied = types.intern(Type::Applied(base, vec![AppliedArg::Size(size)]));
    assert!(matches!(
        types.get(applied),
        Type::Applied(_, args) if matches!(args.as_slice(), [AppliedArg::Size(found)] if *found == size)
    ));
}

#[test]
fn access_marker_facts_intern_as_ref_wrappers() {
    let mut types = TypeTable::new();
    let lista = types.array(types.primitive(Primitive::Numerus));
    let own = types.access_marker(AccessMarker::Own, lista);
    let copy = types.access_marker(AccessMarker::Copy, lista);
    let shared = types.access_marker(AccessMarker::Ref, lista);
    let exclusive = types.access_marker(AccessMarker::Mut, lista);

    assert!(matches!(
        types.get(own),
        Type::Ref(SemanticMutability::Own, inner) if *inner == lista
    ));
    assert!(matches!(
        types.get(copy),
        Type::Ref(SemanticMutability::Copy, inner) if *inner == lista
    ));
    assert!(matches!(
        types.get(shared),
        Type::Ref(SemanticMutability::Immutable, inner) if *inner == lista
    ));
    assert!(matches!(
        types.get(exclusive),
        Type::Ref(SemanticMutability::Mutable, inner) if *inner == lista
    ));
    assert!(!types.equals(own, copy));
    assert!(!types.equals(own, shared));
    assert!(types.is_ref_view(shared));
    assert!(!types.is_ref_view(own));
    assert!(types.is_mut_view(exclusive));
    assert!(!types.is_mut_view(shared));
    assert!(!types.is_mut_view(own));
    assert!(types.is_mut_or_own_view(exclusive));
    assert!(types.is_mut_or_own_view(own));
    assert!(!types.is_mut_or_own_view(shared));
    assert!(types.is_own_view(own));
    assert!(!types.is_own_view(copy));
    assert!(!types.is_own_view(shared));
    assert!(types.is_copy_view(copy));
    assert!(!types.is_copy_view(own));
    assert!(!types.is_copy_view(shared));
    assert!(types.is_exclusive_value(own));
    assert!(types.is_exclusive_value(copy));
    assert!(!types.is_exclusive_value(shared));
    assert!(!types.is_exclusive_value(exclusive));
    let (marker, payload) = types.peel_access_marker(shared);
    assert_eq!(marker, Some(AccessMarker::Ref));
    assert_eq!(payload, lista);
}

#[test]
fn scalar_carriers_reject_access_markers() {
    let mut types = TypeTable::new();
    let scalar_primitives = [
        Primitive::Numerus,
        Primitive::Fractus,
        Primitive::Bivalens,
        Primitive::Nihil,
        Primitive::Vacuum,
        Primitive::Numquam,
        Primitive::Instans,
    ];
    for primitive in scalar_primitives {
        assert!(
            types.is_scalar_for_access_marker(types.primitive(primitive)),
            "{primitive:?} is scalar"
        );
    }
    for primitive in [Primitive::Textus, Primitive::Ascii, Primitive::Octeti] {
        assert!(
            !types.is_scalar_for_access_marker(types.primitive(primitive)),
            "{primitive:?} is not scalar for marker checks"
        );
    }

    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let b8 = types.bits(BitsWidth::B8);
    let modular = types.intern(Type::ModularWord(NumericWidth::U32));
    let saturating = types.intern(Type::SaturatingWord(NumericWidth::U32));
    let ranged = types.ranged_int(IntPolicy::Trapping, NumericWidth::I8, -4, 4);
    let policy_float = types.intern(Type::PolicyFloat(FloatPolicy::Trapping, NumericWidth::F32));
    let sized_instans = types.sized_instans(InstansPrecision::Millis);
    let alias_i32 = types.intern(Type::Alias(DefId(77), i32));
    for scalar in [
        i32,
        f32,
        b8,
        modular,
        saturating,
        ranged,
        policy_float,
        sized_instans,
        alias_i32,
    ] {
        assert!(types.is_scalar_for_access_marker(scalar), "{scalar:?}");
    }
    let array = types.array(i32);
    let inference_hole = types.intern(Type::Infer(InferVar(0)));
    assert!(!types.is_scalar_for_access_marker(array));
    assert!(!types.is_scalar_for_access_marker(inference_hole));
}

#[test]
fn allows_assigning_concrete_type_to_ignotum() {
    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let ignotum = types.primitive(Primitive::Ignotum);

    assert!(types.is_assignable(textus, ignotum));
}

// F2 regression: Option<iuncta> must be assignable to ignotum.
// Before the fix the Option→Tuple rejection (line ~633) fired before the
// ignotum catch-all, so Option<iuncta<...>> → ignotum was rejected.
#[test]
fn option_iuncta_assignable_to_ignotum() {
    let mut types = TypeTable::new();
    let f32 = types.primitive(Primitive::Fractus);
    let pair = types.tuple(vec![f32, f32]);
    let option_pair = types.option(pair);
    let ignotum = types.primitive(Primitive::Ignotum);

    assert!(types.is_assignable(option_pair, ignotum));
    // Sanity: Option→Tuple unwrap is still rejected for concrete destinations.
    assert!(!types.is_assignable(option_pair, f32));
}

#[test]
fn rejects_assigning_ignotum_to_concrete_type() {
    let types = TypeTable::new();
    let ignotum = types.primitive(Primitive::Ignotum);
    let textus = types.primitive(Primitive::Textus);

    assert!(!types.is_assignable(ignotum, textus));
}

#[test]
fn still_assigns_same_type_id() {
    let types = TypeTable::new();
    let ignotum = types.primitive(Primitive::Ignotum);

    assert!(types.is_assignable(ignotum, ignotum));
}

#[test]
fn find_array_returns_the_interned_structural_entry() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let array = types.array(numerus);
    let count = types.type_count();

    assert_eq!(types.find_array(numerus), Some(array));
    assert_eq!(types.ensure_array(numerus), array);
    assert_eq!(types.type_count(), count);
}

#[test]
fn union_equality_ignores_member_order() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);

    let first = types.intern(Type::Union(vec![textus, numerus]));
    let second = types.intern(Type::Union(vec![numerus, textus]));

    assert!(types.equals(first, second));
}

#[test]
fn union_equality_falls_back_when_nested_numeric_keys_differ() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let numerus_i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let textus = types.primitive(Primitive::Textus);
    let lista_numerus = types.array(numerus);
    let lista_numerus_i64 = types.array(numerus_i64);

    let first = types.intern(Type::Union(vec![lista_numerus_i64, textus]));
    let second = types.intern(Type::Union(vec![lista_numerus, textus]));

    assert!(types.equals(first, second));
}

#[test]
fn union_equality_matches_semantic_duplicates_by_sort_key_run() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let numerus_i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let textus = types.primitive(Primitive::Textus);
    let alias = types.intern(Type::Alias(DefId(0), numerus));

    let first = types.intern(Type::Union(vec![alias, textus, numerus_i64, textus]));
    let second = types.intern(Type::Union(vec![textus, numerus, textus, numerus]));
    assert!(types.equals(first, second));

    let different_multiplicity = types.intern(Type::Union(vec![numerus, textus, textus, textus]));
    assert!(!types.equals(first, different_multiplicity));
}

#[test]
fn union_equality_still_matches_each_member_once() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);

    let duplicate_text = types.intern(Type::Union(vec![textus, textus]));
    let mixed = types.intern(Type::Union(vec![textus, numerus]));

    assert!(!types.equals(duplicate_text, mixed));
}

#[test]
fn union_member_sort_key_peels_deep_acyclic_alias_chains() {
    // REV-3 (AUD-REV residual da1969ca): `union_member_sort_key` capped its
    // alias peel at 32 hops while `equals`/`type_equal` peel unboundedly, so
    // a >32-hop acyclic chain made `equals(a, b)` true but `key(a) != key(b)`.
    // The key now terminates by cycle detection (visited set) and peels
    // acyclic chains unboundedly — `equals(a, b) ⇒ key(a) == key(b)` is
    // structural for all acyclic inputs. Pin it with a 40-hop chain.
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);

    // a0 -> numerus, a1 -> a0, ..., a39 -> a38 (40 alias hops, no cycle).
    let mut chain = Vec::with_capacity(40);
    let mut inner = numerus;
    for i in 0..40 {
        inner = types.intern(Type::Alias(DefId(i), inner));
        chain.push(inner);
    }

    // equals peels the whole chain; the key must too, at every depth.
    for alias in &chain {
        assert!(types.equals(*alias, numerus));
        assert_eq!(
            types.union_member_sort_key(*alias),
            types.union_member_sort_key(numerus),
            "equals-true members must share a sort key at any alias depth"
        );
    }
    let deepest = *chain.last().expect("chain has 40 aliases");

    // Lowering sequence (resolve.rs / hir/lower: sort by key, then
    // dedup_by(equals)). A member sorting between the equals-true pair lets
    // the redundant alias survive a fixed-hop peel; a structural key cannot
    // leave it non-adjacent.
    let mut members = vec![deepest, textus, numerus];
    members.sort_by_key(|id| (types.union_member_sort_key(*id).0, id.0));
    members.dedup_by(|a, b| types.equals(*a, *b));
    assert_eq!(
        members.len(),
        2,
        "redundant alias member must be deduplicated"
    );
    assert!(!members.contains(&deepest));

    // Union canonical equality: both spellings compare equal after dedup.
    let spelled_deep = types.intern(Type::Union(members.clone()));
    let spelled_plain = types.intern(Type::Union(vec![numerus, textus]));
    assert!(types.equals(spelled_deep, spelled_plain));
}

#[test]
fn type_table_snapshot_round_trips_compound_types_and_indices() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);
    let len = types.intern_index(IndexExpr::Literal(3));
    let tensor = types.tensor_with_shape(numerus, len);
    let record = types.record([
        (radix_lexer::Symbol(7), textus),
        (radix_lexer::Symbol(8), tensor),
    ]);

    let restored = TypeTable::from_snapshot(types.snapshot()).expect("snapshot should restore");

    assert_eq!(restored.type_count(), types.type_count());
    assert!(restored.equals(record, record));
    assert_eq!(restored.get_index(len), &IndexExpr::Literal(3));
}

#[test]
fn primitive_from_name_maps_builtin_aliases() {
    assert_eq!(Primitive::from_name("ascii"), Some(Primitive::Ascii));
    assert_eq!(Primitive::from_name("textus"), Some(Primitive::Textus));
    assert_eq!(Primitive::from_name("regex"), Some(Primitive::Regex));
    assert_eq!(Primitive::from_name("json"), Some(Primitive::Json));
    assert_eq!(Primitive::from_name("objectum"), Some(Primitive::Ignotum));
    assert_eq!(Primitive::from_name("quidlibet"), Some(Primitive::Ignotum));
    assert_eq!(Primitive::from_name("valor"), Some(Primitive::Valor));
    assert_eq!(Primitive::from_name("instans"), Some(Primitive::Instans));
    assert_eq!(Primitive::from_name("incognitus"), None);
}

/// RD-3: `Primitive::Match` is internal only: no source spelling resolves to
/// it and no type-surface row names it, yet the table seeds the primitive.
#[test]
fn match_primitive_has_no_source_spelling() {
    assert_eq!(Primitive::from_name("match"), None);
    assert!(
        primitive_type_specs()
            .iter()
            .all(|spec| spec.primitive != Primitive::Match)
    );
    let types = TypeTable::new();
    let id = types.primitive(Primitive::Match);
    assert_eq!(types.get(id), &Type::Primitive(Primitive::Match));
}

#[test]
fn primitive_type_surface_tracks_canonical_rows_and_aliases() {
    let canonical = primitive_type_specs()
        .iter()
        .filter(|spec| spec.canonical)
        .map(|spec| spec.text)
        .collect::<Vec<_>>();

    assert!(canonical.contains(&"textus"));
    assert!(canonical.contains(&"numerus"));
    assert!(canonical.contains(&"json"));
    assert!(canonical.contains(&"instans"));

    let objectum = lookup_primitive_type_spec("objectum").expect("objectum alias row");
    let quidlibet = lookup_primitive_type_spec("quidlibet").expect("quidlibet alias row");

    assert!(!objectum.canonical);
    assert!(!quidlibet.canonical);
    assert_eq!(objectum.primitive, Primitive::Ignotum);
    assert_eq!(quidlibet.primitive, Primitive::Ignotum);
    assert_eq!(
        Primitive::from_name("objectum"),
        Primitive::from_name("ignotum")
    );
}

#[test]
fn instans_precision_types_are_distinct_from_bare_instans() {
    let mut types = TypeTable::new();
    let bare = types.primitive(Primitive::Instans);
    let millis = types.sized_instans(InstansPrecision::Millis);
    assert!(!types.equals(bare, millis));
}

#[test]
fn instans_coarse_to_fine_widening_assignable() {
    let mut types = TypeTable::new();
    let bare = types.primitive(Primitive::Instans);
    let millis = types.sized_instans(InstansPrecision::Millis);
    let micros = types.sized_instans(InstansPrecision::Micros);
    let nanos = types.sized_instans(InstansPrecision::Nanos);

    // Coarse → fine: assignable.
    assert!(types.is_assignable(bare, millis));
    assert!(types.is_assignable(bare, micros));
    assert!(types.is_assignable(bare, nanos));
    assert!(types.is_assignable(millis, micros));
    assert!(types.is_assignable(millis, nanos));
    assert!(types.is_assignable(micros, nanos));
}

#[test]
fn instans_fine_to_coarse_not_assignable() {
    let mut types = TypeTable::new();
    let bare = types.primitive(Primitive::Instans);
    let millis = types.sized_instans(InstansPrecision::Millis);
    let micros = types.sized_instans(InstansPrecision::Micros);
    let nanos = types.sized_instans(InstansPrecision::Nanos);

    // Fine → coarse: not assignable.
    assert!(!types.is_assignable(nanos, micros));
    assert!(!types.is_assignable(nanos, millis));
    assert!(!types.is_assignable(nanos, bare));
    assert!(!types.is_assignable(micros, millis));
    assert!(!types.is_assignable(micros, bare));
    assert!(!types.is_assignable(millis, bare));

    // Same precision is still assignable.
    assert!(types.is_assignable(millis, millis));
    assert!(types.is_assignable(bare, bare));
}

#[test]
fn collection_kind_lowers_collection_types() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);

    let lista = CollectionKind::Lista.lower(&mut types, &[textus]);
    let tabula = CollectionKind::Tabula.lower(&mut types, &[textus, numerus]);
    let copia = CollectionKind::Copia.lower(&mut types, &[textus]);

    assert!(matches!(types.get(lista), Type::Array(inner) if *inner == textus));
    assert!(
        matches!(types.get(tabula), Type::Map(key, value) if *key == textus && *value == numerus)
    );
    assert!(matches!(types.get(copia), Type::Set(inner) if *inner == textus));
}

#[test]
fn collection_kind_lowers_queue_and_stack_types() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);
    let capacity = types.intern_index(IndexExpr::Literal(4));

    let queue = CollectionKind::Queue.lower(&mut types, &[numerus]);
    let stack = CollectionKind::Stack.lower(&mut types, &[textus]);
    let bounded_queue = types.bounded_queue(numerus, capacity);
    let bounded_stack = types.bounded_stack(textus, capacity);

    assert!(matches!(types.get(queue), Type::Queue(inner) if *inner == numerus));
    assert!(matches!(types.get(stack), Type::Stack(inner) if *inner == textus));
    assert!(matches!(
        types.get(bounded_queue),
        Type::BoundedQueue { element, capacity: found }
            if *element == numerus && *found == capacity
    ));
    assert!(matches!(
        types.get(bounded_stack),
        Type::BoundedStack { element, capacity: found }
            if *element == textus && *found == capacity
    ));
    assert!(!types.equals(queue, bounded_queue));
    assert!(!types.equals(stack, bounded_stack));
}

#[test]
fn collection_kind_lowers_promissum_and_cursor() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);

    let promissum = CollectionKind::Promissum.lower(&mut types, &[numerus]);
    let cursor = CollectionKind::Cursor.lower(&mut types, &[textus]);

    assert!(matches!(types.get(promissum), Type::Promissum(inner) if *inner == numerus));
    assert!(matches!(types.get(cursor), Type::Cursor(inner) if *inner == textus));
}

#[test]
fn collection_kind_from_name_includes_host_route_collections() {
    assert_eq!(
        CollectionKind::from_name("promissum"),
        Some(CollectionKind::Promissum)
    );
    assert_eq!(
        CollectionKind::from_name("cursor"),
        Some(CollectionKind::Cursor)
    );
    assert_eq!(
        CollectionKind::from_name("queue"),
        Some(CollectionKind::Queue)
    );
    assert_eq!(
        CollectionKind::from_name("stack"),
        Some(CollectionKind::Stack)
    );
    assert_eq!(CollectionKind::from_name("tensor"), None);
    assert_eq!(CollectionKind::from_name("sparsa"), None);
}

#[test]
fn collection_type_surface_tracks_canonical_rows() {
    let rows = collection_type_specs()
        .iter()
        .map(|spec| (spec.text, spec.kind, spec.canonical))
        .collect::<Vec<_>>();

    assert!(rows.contains(&("lista", CollectionKind::Lista, true)));
    assert!(rows.contains(&("tabula", CollectionKind::Tabula, true)));
    assert!(rows.contains(&("promissum", CollectionKind::Promissum, true)));
    assert!(rows.contains(&("queue", CollectionKind::Queue, true)));
    assert!(rows.contains(&("stack", CollectionKind::Stack, true)));
    assert_eq!(
        lookup_collection_type_spec("cursor").map(|spec| spec.kind),
        Some(CollectionKind::Cursor)
    );
    assert_eq!(lookup_collection_type_spec("tensor"), None);
}

#[test]
fn tensor_family_registry_is_closed_and_disjoint_from_collections() {
    // The tensor family is a parallel kind (D3), not a CollectionKind growth:
    // its six canonical rows resolve through their own registry, while
    // CollectionKind::from_name stays None for the family spellings.
    let rows = tensor_family_type_specs()
        .iter()
        .map(|spec| (spec.text, spec.kind, spec.canonical))
        .collect::<Vec<_>>();
    let expected = [
        ("tensor", TensorFamilyKind::Tensor),
        ("vector", TensorFamilyKind::Vector),
        ("matrix", TensorFamilyKind::Matrix),
        ("sparsa", TensorFamilyKind::Sparsa),
        ("atomic", TensorFamilyKind::Atomic),
        ("intervallum", TensorFamilyKind::Intervallum),
    ];
    assert_eq!(
        rows.len(),
        expected.len(),
        "registry must stay a closed set"
    );
    for (text, kind) in expected {
        assert!(
            rows.contains(&(text, kind, true)),
            "missing canonical row {text}"
        );
        assert_eq!(
            lookup_tensor_family_type_spec(text).map(|spec| spec.kind),
            Some(kind)
        );
        assert_eq!(CollectionKind::from_name(text), None);
    }
}

#[test]
fn sparsa_with_shape_and_format_roundtrip() {
    let mut types = TypeTable::new();
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim2 = types.intern_index(IndexExpr::Literal(2));
    let dim3 = types.intern_index(IndexExpr::Literal(3));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim2, dim3]));
    let sparsa_id = types.sparsa_with_shape(elem, shape);
    assert!(matches!(types.get(sparsa_id), Type::Sparsa(e, s) if *e == elem && *s == shape));
}

#[test]
fn matrix_with_shape_is_distinct_from_tensor() {
    let mut types = TypeTable::new();
    let elem = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let dim2 = types.intern_index(IndexExpr::Literal(2));
    let dim3 = types.intern_index(IndexExpr::Literal(3));
    let shape = types.intern_index(IndexExpr::Tuple(vec![dim2, dim3]));
    let matrix_a = types.matrix_with_shape(elem, shape);
    let matrix_b = types.matrix_with_shape(elem, shape);
    let tensor = types.tensor_with_shape(elem, shape);

    assert!(matches!(types.get(matrix_a), Type::Matrix(e, s) if *e == elem && *s == shape));
    assert!(
        types.equals(matrix_a, matrix_b),
        "same element and shape must be equal"
    );
    assert!(
        !types.equals(matrix_a, tensor),
        "matrix must not equal tensor"
    );
    assert!(
        !types.is_assignable(matrix_a, tensor),
        "matrix must not assign to tensor implicitly"
    );
    assert!(
        !types.is_assignable(tensor, matrix_a),
        "tensor must not assign to matrix implicitly"
    );
}

#[test]
fn atomic_is_distinct_from_its_element_type() {
    let mut types = TypeTable::new();
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let u32 = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let atomic_i32_a = types.atomic(i32);
    let atomic_i32_b = types.atomic(i32);
    let atomic_u32 = types.atomic(u32);

    assert!(matches!(types.get(atomic_i32_a), Type::Atomic(elem) if *elem == i32));
    assert!(types.equals(atomic_i32_a, atomic_i32_b));
    assert!(!types.equals(atomic_i32_a, atomic_u32));
    assert!(!types.equals(atomic_i32_a, i32));
    assert!(!types.is_assignable(i32, atomic_i32_a));
    assert!(!types.is_assignable(atomic_i32_a, i32));
}

#[test]
fn sized_numeric_widening_assignable_in_type_table() {
    let mut types = TypeTable::new();
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    assert!(types.is_assignable(i32, i64));
    assert!(!types.is_assignable(i64, i32));
}

#[test]
fn nullable_inner_option_returns_inner() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    let opt_numerus = types.option(numerus);
    assert_eq!(types.nullable_inner(opt_numerus), Some(numerus));
}

#[test]
fn nullable_inner_union_with_nihil_returns_inner() {
    let mut types = TypeTable::new();
    let nihil = types.primitive(Primitive::Nihil);
    let textus = types.primitive(Primitive::Textus);

    let union_nullable = types.intern(Type::Union(vec![textus, nihil]));
    assert_eq!(types.nullable_inner(union_nullable), Some(textus));
}

#[test]
fn nullable_inner_union_without_nihil_returns_none() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let textus = types.primitive(Primitive::Textus);

    let union_plain = types.intern(Type::Union(vec![textus, numerus]));
    assert_eq!(types.nullable_inner(union_plain), None);
}

#[test]
fn nullable_inner_union_multi_arm_with_nihil_returns_none() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let nihil = types.primitive(Primitive::Nihil);
    let textus = types.primitive(Primitive::Textus);

    let union_multi = types.intern(Type::Union(vec![textus, numerus, nihil]));
    assert_eq!(types.nullable_inner(union_multi), None);
}

#[test]
fn nullable_inner_alias_traverses_to_underlying() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    let opt_numerus = types.option(numerus);
    let alias = types.intern(Type::Alias(DefId(0), opt_numerus));
    assert_eq!(types.nullable_inner(alias), Some(numerus));
}

#[test]
fn nullable_inner_alias_chain_to_option() {
    // Multi-hop Alias peel still reaches Option payload (acyclic path).
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let opt = types.option(numerus);
    let mid = types.intern(Type::Alias(DefId(1), opt));
    let outer = types.intern(Type::Alias(DefId(2), mid));
    assert_eq!(types.nullable_inner(outer), Some(numerus));
}

#[test]
fn nullable_inner_self_cyclic_alias_returns_none() {
    // PRP-059 residual: self-referential Alias must fail closed (no stack overflow).
    let mut types = TypeTable::new();
    let next = u32::try_from(types.type_count()).expect("type table fits u32");
    let cyclic = types.intern(Type::Alias(DefId(0), TypeId(next)));
    assert_eq!(types.nullable_inner(cyclic), None);
}

#[test]
fn nullable_inner_mutual_cyclic_aliases_returns_none() {
    // Two-node Alias cycle A → B → A.
    let mut types = TypeTable::new();
    let n = u32::try_from(types.type_count()).expect("type table fits u32");
    let a = types.intern(Type::Alias(DefId(1), TypeId(n + 1)));
    let b = types.intern(Type::Alias(DefId(2), TypeId(n)));
    assert_eq!(a, TypeId(n));
    assert_eq!(b, TypeId(n + 1));
    assert_eq!(types.nullable_inner(a), None);
    assert_eq!(types.nullable_inner(b), None);
}

#[test]
fn nullable_inner_non_nullable_primitive_returns_none() {
    let types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    assert_eq!(types.nullable_inner(numerus), None);
}

#[test]
fn nullable_inner_nihil_is_not_a_nullable_carrier() {
    // `nihil` is the null marker itself, not `Option<T>` / `T ∪ nihil`.
    // Payload extraction returns None (doc contract; not "unwrap to self").
    let types = TypeTable::new();
    let nihil = types.primitive(Primitive::Nihil);

    assert_eq!(types.nullable_inner(nihil), None);
}

#[test]
fn octeti_unifies_bidirectionally_with_lista_numerus_u8() {
    // WHY: `octeti` is semantic sugar for `lista<numerus<u8>>`. Both directions
    // must be assignable so octeti carries every `lista` method and either slot
    // accepts the other. See `docs/factory/octeti-unify-lista/goal.md`, DEFER-029.
    let mut types = TypeTable::new();
    let octeti = types.primitive(Primitive::Octeti);
    let u8 = types.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let lista_u8 = types.array(u8);

    assert!(
        types.is_assignable(octeti, lista_u8),
        "octeti should widen to lista<u8>"
    );
    assert!(
        types.is_assignable(lista_u8, octeti),
        "lista<u8> should narrow to octeti"
    );
}

/// D10.4 (L2-10): `octeti` is exactly `lista<octetus>` — type identity, not
/// mutual assignability — while interning keeps the two carriers distinct.
#[test]
fn octeti_is_identical_to_lista_octetus() {
    let mut types = TypeTable::new();
    let octeti = types.primitive(Primitive::Octeti);
    let u8 = types.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let lista_u8 = types.array(u8);

    assert!(types.equals(octeti, lista_u8));
    assert!(types.equals(lista_u8, octeti));
    assert_ne!(octeti, lista_u8, "interning keeps the byte-buffer carrier");
    assert_eq!(
        types.find_equal(&Type::Array(u8)),
        Some(lista_u8),
        "find_equal never answers a lista probe with the byte buffer row"
    );
}

/// D10.5: `ascii<1> → littera → textus`; nothing widens the other way.
#[test]
fn littera_widening_chain() {
    let mut types = TypeTable::new();
    let littera = types.primitive(Primitive::Littera);
    let textus = types.primitive(Primitive::Textus);
    let ascii = types.primitive(Primitive::Ascii);
    let one = types.intern_index(IndexExpr::Literal(1));
    let two = types.intern_index(IndexExpr::Literal(2));
    let ascii_one = types.bounded_ascii(one);
    let ascii_two = types.bounded_ascii(two);

    assert!(types.is_assignable(ascii_one, littera));
    assert!(types.is_assignable(littera, textus));
    assert!(!types.is_assignable(textus, littera));
    assert!(!types.is_assignable(littera, ascii_one));
    assert!(!types.is_assignable(ascii, littera));
    assert!(!types.is_assignable(ascii_two, littera));
    assert!(!types.equals(littera, ascii_one));
}

#[test]
fn octeti_does_not_unify_with_non_u8_lista() {
    // WHY: octeti is specifically `lista<numerus<u8>>`, not any lista. Other
    // element widths and other element types must not silently cross over.
    let mut types = TypeTable::new();
    let octeti = types.primitive(Primitive::Octeti);
    let u16 = types.sized_numeric(Primitive::Numerus, NumericWidth::U16);
    let lista_u16 = types.array(u16);
    let numerus = types.primitive(Primitive::Numerus);
    let lista_numerus = types.array(numerus);
    let textus = types.primitive(Primitive::Textus);
    let lista_textus = types.array(textus);

    assert!(!types.is_assignable(octeti, lista_u16));
    assert!(!types.is_assignable(lista_u16, octeti));
    assert!(!types.is_assignable(octeti, lista_numerus));
    assert!(!types.is_assignable(octeti, lista_textus));
}

#[test]
fn find_sized_numeric_returns_existing_intern() {
    // WHY: codegen holds `&TypeTable` (immutable) and must look up sized
    // numeric ids without interning. Octeti method routing depends on this.
    let mut types = TypeTable::new();
    assert!(
        types
            .find_sized_numeric(Primitive::Numerus, NumericWidth::U8)
            .is_none()
    );
    let u8 = types.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    assert_eq!(
        types.find_sized_numeric(Primitive::Numerus, NumericWidth::U8),
        Some(u8)
    );
    assert!(
        types
            .find_sized_numeric(Primitive::Numerus, NumericWidth::U16)
            .is_none()
    );
}

#[test]
fn sparsa_equals_compares_element_and_shape() {
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let f64 = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    let lit2 = types.intern_index(IndexExpr::Literal(2));
    let lit3 = types.intern_index(IndexExpr::Literal(3));
    let shape_2x3 = types.intern_index(IndexExpr::Tuple(vec![lit2, lit3]));
    let shape_3x2 = types.intern_index(IndexExpr::Tuple(vec![lit3, lit2]));

    let s1 = types.sparsa_with_shape(f32, shape_2x3);
    let s2 = types.sparsa_with_shape(f32, shape_2x3);
    let s3 = types.sparsa_with_shape(f64, shape_2x3);
    let s4 = types.sparsa_with_shape(f32, shape_3x2);
    let rank0 = types.rank0_shape();
    let tensor_f32 = types.tensor_with_shape(f32, rank0);

    assert!(types.equals(s1, s2), "same element and shape must be equal");
    assert!(!types.equals(s1, s3), "different element must not be equal");
    assert!(!types.equals(s1, s4), "different shape must not be equal");
    assert!(!types.equals(s1, tensor_f32), "sparsa != tensor");
}

// --- PRP-017 / PRP-061 regression: intern hash-cons canonicalization ---

#[test]
fn intern_canonicalizes_compound_types() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    let first = types.intern(Type::Array(numerus));
    let second = types.intern(Type::Array(numerus));
    assert_eq!(
        first, second,
        "structurally equal compound types must canonicalize to one TypeId"
    );

    let opt1 = types.option(numerus);
    let opt2 = types.option(numerus);
    assert_eq!(opt1, opt2, "Option canonicalizes");

    let set1 = types.set(numerus);
    let set2 = types.set(numerus);
    assert_eq!(set1, set2, "Set canonicalizes");

    let map1 = types.map(numerus, numerus);
    let map2 = types.map(numerus, numerus);
    assert_eq!(map1, map2, "Map canonicalizes");
}

#[test]
fn intern_hash_cons_union_order_insensitive() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);

    let before = types.type_count();
    let first = types.intern(Type::Union(vec![textus, numerus]));
    let second = types.intern(Type::Union(vec![numerus, textus]));
    assert_eq!(
        first, second,
        "union member order must not allocate a second TypeId under hash-cons"
    );
    assert_eq!(
        types.type_count(),
        before + 1,
        "only one union entry is interned"
    );
    assert!(types.equals(first, second));
}

#[test]
fn equals_is_stable_under_repeated_comparison() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);
    let a = types.array(textus);
    let b = types.array(textus);
    let c = types.array(numerus);

    assert!(types.equals(a, b));
    assert!(types.equals(a, b), "memoized equals must remain true");
    assert!(!types.equals(a, c));
    assert!(!types.equals(a, c), "memoized equals must remain false");
}

#[test]
fn assignable_memo_keeps_directional_answers_stable() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let nihil = types.primitive(Primitive::Nihil);
    let ignotum = types.primitive(Primitive::Ignotum);
    let option_numerus = types.option(numerus);

    // Repeated same-direction queries agree (memo hit must not flip answers).
    assert!(types.is_assignable(nihil, option_numerus));
    assert!(
        types.is_assignable(nihil, option_numerus),
        "memoized assignable must remain true"
    );

    // Directional: `option<T>` does NOT flow into `nihil` even though the
    // reverse (`nihil` → `option<T>`) is accepted. A min/max-ordered memo key
    // (the `equals` shape) would conflate the reversed pair and return the
    // cached wrong answer, so `is_assignable` keys the memo by (from, to) order.
    assert!(!types.is_assignable(option_numerus, nihil));
    assert!(
        !types.is_assignable(option_numerus, nihil),
        "memoized assignable must remain false"
    );

    // One-way `ignotum` sink: concrete flows in, unknown does not flow out.
    assert!(types.is_assignable(numerus, ignotum));
    assert!(!types.is_assignable(ignotum, numerus));
    assert!(
        !types.is_assignable(ignotum, numerus),
        "directional memo key must not conflate (ignotum, numerus) with (numerus, ignotum)"
    );
}

// --- REV-1: assignable memo must not survive a snapshot/restore boundary ---
//
// The memo is runtime-only state: `TypeTableSnapshot` serializes the type and
// index arenas but never the memos, so `from_snapshot` rebuilds
// `assignable_memo` empty (types.rs). A restored table must recompute every
// query against its own arena rather than serving an entry cached during the
// source table's lifetime.

#[test]
fn assignable_memo_is_rebuilt_fresh_by_snapshot_restore() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let nihil = types.primitive(Primitive::Nihil);
    let ignotum = types.primitive(Primitive::Ignotum);
    let option_numerus = types.option(numerus);

    // Populate the memo with directional answers.
    assert!(types.is_assignable(nihil, option_numerus));
    assert!(!types.is_assignable(option_numerus, nihil));
    assert!(
        !types.assignable_memo.borrow().is_empty(),
        "directional queries must populate the memo"
    );

    // Snapshot/restore rebuilds the table from the serialized arenas only.
    // The memo must come back empty so a re-query recomputes against the
    // restored arena (CTM E.2: "caches rebuilt").
    let mut restored = TypeTable::from_snapshot(types.snapshot()).expect("snapshot should restore");
    assert!(
        restored.assignable_memo.borrow().is_empty(),
        "restore must rebuild assignable_memo fresh"
    );

    // Re-query recomputes and agrees with the pre-restore answers.
    assert!(restored.is_assignable(nihil, option_numerus));
    assert!(!restored.is_assignable(option_numerus, nihil));

    // Append-only arena: interning more types after restore must not flip any
    // answer computed against the restored arena.
    let _extra = restored.array(ignotum);
    assert!(restored.is_assignable(nihil, option_numerus));
    assert!(!restored.is_assignable(option_numerus, nihil));
}

// --- PRP-017 F2 regression: find_equal uses equals, not raw TypeId == ---

#[test]
fn find_equal_uses_semantic_equality() {
    let mut types = TypeTable::new();
    // Primitive::Numerus and SizedNumeric(Numerus, I64) are semantically equal
    // (equals returns true) but representationally distinct (derived PartialEq false).
    let numerus = types.primitive(Primitive::Numerus);
    let numerus_i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    assert!(types.equals(numerus, numerus_i64));

    // Intern Array(numerus).
    let array_numerus = types.array(numerus);

    // find_equal for Array(numerus_i64) must find the existing Array(numerus)
    // because equals(numerus, numerus_i64) is true. A raw TypeId == comparison
    // would miss this because numerus != numerus_i64 as TypeId values.
    let found = types.find_equal(&Type::Array(numerus_i64));
    assert_eq!(
        found,
        Some(array_numerus),
        "find_equal must delegate to equals for child TypeId comparison"
    );
}

// --- BO-C-1 regression: find_equal memoization (positive-only cache) ---

#[test]
fn find_equal_memo_hit_returns_stable_id() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let array_textus = types.array(textus);

    let first = types.find_equal(&Type::Array(textus));
    let second = types.find_equal(&Type::Array(textus));
    assert_eq!(first, Some(array_textus));
    assert_eq!(
        second, first,
        "repeated find_equal for the same candidate must return the same TypeId"
    );
}

#[test]
fn find_equal_memo_no_stale_negative() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);

    // Candidate absent from the table: two consecutive misses must both be
    // None — misses are not cached, so a later intern is always observable.
    let candidate = Type::Array(numerus);
    assert_eq!(types.find_equal(&candidate), None);
    assert_eq!(types.find_equal(&candidate), None);

    let array_numerus = types.intern(Type::Array(numerus));
    assert_eq!(
        types.find_equal(&candidate),
        Some(array_numerus),
        "interning a matching type must not be hidden by a stale negative"
    );
}

#[test]
fn find_equal_memo_semantic_result_preserved() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let numerus_i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    assert!(types.equals(numerus, numerus_i64));

    // Arena order matters: Array(numerus_i64) is interned first, so the linear
    // scan's first semantic match for candidate Array(numerus) is the earlier
    // coerced entry, not the later structurally-identical Array(numerus). The
    // memo must cache the scan's own result — an intern_map-style structural
    // fast path would return the later representative instead and change which
    // TypeId codegen renders.
    let array_i64 = types.array(numerus_i64);
    let array_plain = types.array(numerus);
    assert_ne!(
        array_i64, array_plain,
        "structural entries must stay distinct"
    );

    let candidate = Type::Array(numerus);
    let first = types.find_equal(&candidate);
    assert_eq!(first, Some(array_i64), "first semantic match wins the scan");

    let second = types.find_equal(&candidate);
    assert_eq!(second, first, "memo must return the cached scan result");
    assert_ne!(
        second,
        Some(array_plain),
        "must not be an intern_map structural representative"
    );
}

// --- PRP-026 regression: snapshot validation ---

#[test]
fn from_snapshot_rejects_dangling_type_ref() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let _array = types.array(numerus);
    let mut snap = types.snapshot();
    let last = snap.types.len() - 1;
    snap.types[last] = Type::Array(TypeId(u32::MAX));
    assert!(
        TypeTable::from_snapshot(snap).is_err(),
        "snapshot with out-of-bounds TypeId must be rejected"
    );
}

#[test]
fn from_snapshot_rejects_dangling_index_ref() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let shape = types.rank0_shape();
    let tensor = types.tensor_with_shape(numerus, shape);
    let mut snap = types.snapshot();
    if let Type::Tensor(_, ref mut idx) = snap.types[tensor.0 as usize] {
        *idx = IndexId(u32::MAX);
    }
    assert!(
        TypeTable::from_snapshot(snap).is_err(),
        "snapshot with out-of-bounds IndexId must be rejected"
    );
}

// --- PRP-027 regression: DefId sentinel ---

#[test]
fn def_id_invalid_is_sentinel() {
    assert_eq!(DefId::INVALID, DefId(u32::MAX));
    assert_ne!(DefId::INVALID, DefId(0));
    assert_ne!(DefId::INVALID, DefId(crate::USER_DEF_ID_BASE));
}

// --- Edge coverage: emptiness, cross-primitive rejection ---

#[test]
fn assignable_rejects_cross_primitives() {
    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);

    assert!(!types.is_assignable(textus, numerus));
    assert!(!types.is_assignable(numerus, textus));
    assert!(!types.is_assignable(textus, types.primitive(Primitive::Fractus)));
}

#[test]
fn assignable_accepts_same_primitive() {
    let types = TypeTable::new();
    let a = types.primitive(Primitive::Textus);
    let b = types.primitive(Primitive::Textus);

    assert!(types.is_assignable(a, b));
}

#[test]
fn equals_rejects_cross_primitives() {
    let types = TypeTable::new();
    assert!(!types.equals(
        types.primitive(Primitive::Textus),
        types.primitive(Primitive::Numerus),
    ));
}

#[test]
fn primitive_from_name_returns_none_for_empty() {
    assert_eq!(Primitive::from_name(""), None);
}

#[test]
fn collection_kind_from_name_returns_none_for_empty() {
    assert_eq!(CollectionKind::from_name(""), None);
}

#[test]
fn sized_numeric_mixed_sign_not_assignable() {
    let mut types = TypeTable::new();
    let i8 = types.sized_numeric(Primitive::Numerus, NumericWidth::I8);
    let u8 = types.sized_numeric(Primitive::Numerus, NumericWidth::U8);

    assert!(!types.is_assignable(i8, u8));
    assert!(!types.is_assignable(u8, i8));
}

#[test]
fn type_table_new_seeds_primitives() {
    // `TypeTable::new` preloads every `Primitive` for stable well-known IDs.
    let types = TypeTable::new();
    assert!(types.type_count() > 0);
    let _ = types.primitive(Primitive::Numerus);
    let _ = types.primitive(Primitive::Nihil);
}

#[test]
fn first_modular_word_width_tracks_intern() {
    let mut types = TypeTable::new();
    assert_eq!(types.first_modular_word_width(), None);
    let _ = types.intern(Type::ModularWord(NumericWidth::U32));
    assert_eq!(types.first_modular_word_width(), Some(NumericWidth::U32));
    // Later modular words do not replace the first.
    let _ = types.intern(Type::ModularWord(NumericWidth::U64));
    assert_eq!(types.first_modular_word_width(), Some(NumericWidth::U32));
}

#[test]
fn first_modular_word_width_restored_from_snapshot() {
    let mut types = TypeTable::new();
    let _ = types.intern(Type::ModularWord(NumericWidth::U16));
    let restored = TypeTable::from_snapshot(types.snapshot()).expect("snapshot");
    assert_eq!(restored.first_modular_word_width(), Some(NumericWidth::U16));
}

#[test]
fn first_authored_bits_snapshot_round_trip() {
    for width in [
        BitsWidth::B8,
        BitsWidth::B16,
        BitsWidth::B32,
        BitsWidth::B64,
    ] {
        let mut types = TypeTable::new();
        let _ = types.intern(Type::Bits(width));
        let restored = TypeTable::from_snapshot(types.snapshot()).expect("snapshot");
        assert_eq!(restored.first_authored_bits(), Some(width));
    }
}

#[test]
fn seeded_octeti_byte_list_is_not_an_authored_bits_after_restore() {
    let types = TypeTable::new();
    assert_eq!(types.first_authored_bits(), None);
    let restored = TypeTable::from_snapshot(types.snapshot()).expect("snapshot");
    assert_eq!(restored.first_authored_bits(), None);
}

// --- F-003: Alias wrappers break assignability / numeric carriers ---

#[test]
fn f003_alias_wrapped_sized_numeric_widening_assignable() {
    // Intern `Alias(_, SizedNumeric(I32))` vs `Alias(_, SizedNumeric(I64))`;
    // expect assignable because the alias wrappers should not hide the
    // underlying numeric carrier widening rule.
    let mut types = TypeTable::new();
    let i32_id = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let i64_id = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let alias_i32 = types.intern(Type::Alias(DefId(1), i32_id));
    let alias_i64 = types.intern(Type::Alias(DefId(2), i64_id));

    assert!(
        types.is_assignable(alias_i32, alias_i64),
        "F-003: alias-wrapped SizedNumeric I32 must be assignable to alias-wrapped I64"
    );
    // Reverse: narrowing is still rejected.
    assert!(
        !types.is_assignable(alias_i64, alias_i32),
        "F-003: alias-wrapped SizedNumeric I64 must NOT be assignable to I32"
    );
}

#[test]
fn f003_alias_wrapped_numerus_to_fractus_not_assignable() {
    // Alias wrapping must not reopen the integer -> float family crossing.
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let fractus = types.primitive(Primitive::Fractus);
    let alias_numerus = types.intern(Type::Alias(DefId(3), numerus));
    let alias_fractus = types.intern(Type::Alias(DefId(4), fractus));

    assert!(
        !types.is_assignable(alias_numerus, alias_fractus),
        "F-003: alias-wrapped numerus must not be assignable to alias-wrapped fractus"
    );
}

#[test]
fn f003_alias_wrapped_octeti_lista_unify_still_works() {
    // Alias-wrapped Octeti and lista<numerus<u8>> must still unify bidirectionally.
    let mut types = TypeTable::new();
    let octeti = types.primitive(Primitive::Octeti);
    let u8_id = types.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let lista_u8 = types.array(u8_id);
    let alias_octeti = types.intern(Type::Alias(DefId(5), octeti));
    let alias_lista_u8 = types.intern(Type::Alias(DefId(6), lista_u8));

    assert!(
        types.is_assignable(alias_octeti, alias_lista_u8),
        "F-003: alias-wrapped octeti must widen to alias-wrapped lista<u8>"
    );
    assert!(
        types.is_assignable(alias_lista_u8, alias_octeti),
        "F-003: alias-wrapped lista<u8> must narrow to alias-wrapped octeti"
    );
}

#[test]
fn f003_alias_wrapped_instans_carrier_assignable() {
    // Alias wrapping must not block instans coarse→fine widening.
    let mut types = TypeTable::new();
    let bare = types.primitive(Primitive::Instans);
    let millis = types.sized_instans(InstansPrecision::Millis);
    let alias_bare = types.intern(Type::Alias(DefId(7), bare));
    let alias_millis = types.intern(Type::Alias(DefId(8), millis));

    assert!(
        types.is_assignable(alias_bare, alias_millis),
        "F-003: alias-wrapped bare instans must be assignable to alias-wrapped instans<ms>"
    );
    // Fine→coarse through alias is still rejected.
    assert!(
        !types.is_assignable(alias_millis, alias_bare),
        "F-003: alias-wrapped instans<ms> must NOT be assignable to alias-wrapped bare instans"
    );
}

// --- F-004: FuncSig equals vs PartialEq/Hash disagree on type_params ---

#[test]
fn f004_func_sig_equals_includes_type_params() {
    // Two FuncSig values that differ only in type_param names must NOT be
    // semantically equal because PartialEq includes type_params → distinct
    // TypeIds. equals must agree: different type_params → not equal.
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);

    let sig_t = FuncSig {
        type_params: vec![typus_param(radix_lexer::Symbol(100))], // "T"
        params: vec![ParamType {
            ty: numerus,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: vacuum,
        err: None,
        is_async: false,
        is_generator: false,
    };
    let sig_u = FuncSig {
        type_params: vec![typus_param(radix_lexer::Symbol(200))], // "U"
        params: vec![ParamType {
            ty: numerus,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: vacuum,
        err: None,
        is_async: false,
        is_generator: false,
    };

    let id_t = types.function(sig_t);
    let id_u = types.function(sig_u);

    assert_ne!(
        id_t, id_u,
        "F-004: f<T> and f<U> must have distinct TypeIds because PartialEq includes type_params"
    );
    assert!(
        !types.equals(id_t, id_u),
        "F-004: equals must NOT report true when PartialEq-based intern gives distinct TypeIds"
    );
}

#[test]
fn f004_func_sig_equal_when_type_params_match() {
    // Two FuncSig values with identical type_params must still be equal.
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);

    let sig_a = FuncSig {
        type_params: vec![typus_param(radix_lexer::Symbol(42))], // "T"
        params: vec![ParamType {
            ty: numerus,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: vacuum,
        err: None,
        is_async: false,
        is_generator: false,
    };
    let sig_b = FuncSig {
        type_params: vec![typus_param(radix_lexer::Symbol(42))], // same "T"
        params: vec![ParamType {
            ty: numerus,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: vacuum,
        err: None,
        is_async: false,
        is_generator: false,
    };

    let id_a = types.function(sig_a);
    let id_b = types.function(sig_b);

    assert_eq!(
        id_a, id_b,
        "F-004: identical FuncSig must hash-cons to same TypeId"
    );
    assert!(types.equals(id_a, id_b));
}

// --- GPD-2: ordered generic descriptor ---

#[test]
fn gpd2_descriptor_equality_distinguishes_sort_and_default() {
    let name = radix_lexer::Symbol(7);
    let typus = typus_param(name);
    let magnitudo = GenericParamDescriptor {
        sort: GenericParamSort::Magnitudo,
        ..typus.clone()
    };
    let defaulted = GenericParamDescriptor {
        default: Some(SemanticGenericParamDefault::Magnitudo(64)),
        ..magnitudo.clone()
    };
    let renamed = GenericParamDescriptor {
        name: radix_lexer::Symbol(8),
        ..typus.clone()
    };

    assert_eq!(typus, typus.clone(), "descriptor equals itself");
    assert_ne!(typus, magnitudo, "same name, different sort");
    assert_ne!(magnitudo, defaulted, "same sort, default added");
    assert_ne!(typus, renamed, "different name");
    assert_ne!(
        typus,
        GenericParamDescriptor {
            constraint: TypeParamConstraint::OneOf(vec![TypeId(1)]),
            ..typus.clone()
        },
        "different constraint"
    );
}

#[test]
fn gpd2_descriptor_hash_agrees_with_equality() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let hash_of = |descriptor: &GenericParamDescriptor| {
        let mut hasher = DefaultHasher::new();
        descriptor.hash(&mut hasher);
        hasher.finish()
    };
    let typus = typus_param(radix_lexer::Symbol(7));
    let magnitudo = GenericParamDescriptor {
        sort: GenericParamSort::Magnitudo,
        default: Some(SemanticGenericParamDefault::Magnitudo(64)),
        ..typus.clone()
    };

    assert_eq!(
        hash_of(&typus),
        hash_of(&typus.clone()),
        "equal descriptors hash equal"
    );
    assert_ne!(
        hash_of(&typus),
        hash_of(&magnitudo),
        "descriptor hash covers sort and default"
    );
}

#[test]
fn gpd2_descriptor_serde_round_trip_preserves_fields() {
    let typus_default = {
        let types = TypeTable::new();
        let numerus = types.primitive(Primitive::Numerus);
        SemanticGenericParamDefault::Typus(numerus)
    };
    let descriptor = GenericParamDescriptor {
        name: radix_lexer::Symbol(7),
        sort: GenericParamSort::Magnitudo,
        constraint: TypeParamConstraint::OneOf(vec![TypeId(3), TypeId(4)]),
        default: Some(SemanticGenericParamDefault::Magnitudo(64)),
    };
    let other = GenericParamDescriptor {
        name: radix_lexer::Symbol(9),
        sort: GenericParamSort::Typus,
        constraint: TypeParamConstraint::OneOf(vec![TypeId(3), TypeId(4)]),
        default: Some(typus_default),
    };

    for value in [descriptor, other] {
        let encoded = serde_json::to_string(&value).expect("descriptor serializes");
        let decoded: GenericParamDescriptor =
            serde_json::from_str(&encoded).expect("descriptor deserializes");
        assert_eq!(value, decoded, "serde round trip preserves descriptor");
    }
}

#[test]
fn gpd2_func_sig_equals_includes_sort_and_default() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);
    let sig_of = |default: Option<SemanticGenericParamDefault>, sort: GenericParamSort| FuncSig {
        type_params: vec![GenericParamDescriptor {
            name: radix_lexer::Symbol(42),
            sort,
            constraint: TypeParamConstraint::Any,
            default,
        }],
        params: vec![ParamType {
            ty: numerus,
            mode: SemanticParamMode::Owned,
            optional: false,
            sponte: false,
        }],
        ret: vacuum,
        err: None,
        is_async: false,
        is_generator: false,
    };

    let bare_id = types.function(sig_of(None, GenericParamSort::Typus));
    let defaulted_id = types.function(sig_of(
        Some(SemanticGenericParamDefault::Magnitudo(64)),
        GenericParamSort::Magnitudo,
    ));
    let magnitudo_bare_id = types.function(sig_of(None, GenericParamSort::Magnitudo));

    assert_ne!(
        bare_id, magnitudo_bare_id,
        "typus and magnitudo rows are distinct sigs"
    );
    assert_ne!(
        magnitudo_bare_id, defaulted_id,
        "a defaulted size param is a different signature"
    );
    assert!(
        !types.equals(bare_id, defaulted_id),
        "equals must consult descriptor sort/default, not only names"
    );
}

// --- F-005: Index u64::MAX clamp ---

#[test]
fn f005_i64s_from_literal_index_rejects_u64_max() {
    // u64::MAX must return None, not silently clamp to i64::MAX.
    let mut types = TypeTable::new();
    let max_id = types.intern_index(IndexExpr::Literal(u64::MAX));
    assert!(
        i64s_from_literal_index(max_id, &types).is_none(),
        "F-005: u64::MAX literal index must not silently clamp to i64::MAX"
    );
}

#[test]
fn f005_i64s_from_literal_index_still_accepts_i64_max_below() {
    // Values that DO fit in i64 must still be accepted.
    let mut types = TypeTable::new();
    let ok_id = types.intern_index(IndexExpr::Literal(42));
    assert_eq!(
        i64s_from_literal_index(ok_id, &types),
        Some(vec![42]),
        "F-005: literal index 42 must still resolve"
    );

    let max_i64_id = types.intern_index(IndexExpr::Literal(i64::MAX as u64));
    assert_eq!(
        i64s_from_literal_index(max_i64_id, &types),
        Some(vec![i64::MAX]),
        "F-005: literal index i64::MAX must still resolve"
    );

    // i64::MAX + 1 (first value that overflows i64)
    let overflow_id = types.intern_index(IndexExpr::Literal((i64::MAX as u64) + 1));
    assert!(
        i64s_from_literal_index(overflow_id, &types).is_none(),
        "F-005: literal index i64::MAX + 1 must return None"
    );
}

// --- F-064: nullable_inner counts arms by TypeId not equals ---

#[test]
fn f064_nullable_inner_union_with_alias_wrapped_nihil() {
    // Nihil wrapped behind an alias must still be recognized as the null
    // marker so the single non-null arm is extracted (F-064).
    let mut types = TypeTable::new();
    let nihil = types.primitive(Primitive::Nihil);
    let textus = types.primitive(Primitive::Textus);
    let alias_nihil = types.intern(Type::Alias(DefId(10), nihil));

    let union_with_alias_nihil = types.intern(Type::Union(vec![textus, alias_nihil]));
    assert_eq!(
        types.nullable_inner(union_with_alias_nihil),
        Some(textus),
        "F-064: alias-wrapped nihil must still be recognized as null marker"
    );
}

#[test]
fn f064_nullable_inner_alias_wrapped_union_with_aliased_nihil() {
    // Full chain: union wrapped in alias, with alias-wrapped nihil member.
    let mut types = TypeTable::new();
    let nihil = types.primitive(Primitive::Nihil);
    let textus = types.primitive(Primitive::Textus);
    let alias_nihil = types.intern(Type::Alias(DefId(11), nihil));
    let union = types.intern(Type::Union(vec![textus, alias_nihil]));
    let alias_union = types.intern(Type::Alias(DefId(12), union));

    assert_eq!(
        types.nullable_inner(alias_union),
        Some(textus),
        "F-064: alias-wrapped union with alias-wrapped nihil must still extract payload"
    );
}

#[test]
fn f064_nullable_inner_alias_wrapped_nihil_union_multi_arm_still_returns_none() {
    // More than one non-nihil member is still not a nullable carrier,
    // even when nihil is alias-wrapped (F-064).
    let mut types = TypeTable::new();
    let nihil = types.primitive(Primitive::Nihil);
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);
    let alias_nihil = types.intern(Type::Alias(DefId(13), nihil));

    let union_multi = types.intern(Type::Union(vec![textus, numerus, alias_nihil]));
    assert_eq!(
        types.nullable_inner(union_multi),
        None,
        "F-064: multi-arm union with alias-wrapped nihil is still not nullable"
    );
}

// --- F-065: snapshot nested IndexId unvalidated ---

#[test]
fn f065_from_snapshot_rejects_dangling_nested_index_ref() {
    // A snapshot whose IndexExpr::Tuple contains an out-of-bounds IndexId
    // must be rejected. Pre-F-065 only top-level index refs in Type nodes
    // were validated; nested Tuple children went unchecked (F-065).
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let dim2 = types.intern_index(IndexExpr::Literal(2));
    let dim3 = types.intern_index(IndexExpr::Literal(3));
    let shape_2x3 = types.intern_index(IndexExpr::Tuple(vec![dim2, dim3]));
    // Intern a tensor to anchor the shape in the snapshot.
    let _tensor = types.tensor_with_shape(numerus, shape_2x3);

    let mut snap = types.snapshot();
    // Corrupt the Tuple child: point dim2 to an out-of-bounds IndexId.
    if let IndexExpr::Tuple(ref mut dims) = snap.indices[shape_2x3.0 as usize] {
        dims[0] = IndexId(u32::MAX);
    }
    assert!(
        TypeTable::from_snapshot(snap).is_err(),
        "F-065: snapshot with dangling nested IndexId must be rejected"
    );

    // Confirm the non-corrupt snapshot still restores.
    let mut types2 = TypeTable::new();
    let dim_a = types2.intern_index(IndexExpr::Literal(5));
    let dim_b = types2.intern_index(IndexExpr::Literal(6));
    let shape = types2.intern_index(IndexExpr::Tuple(vec![dim_a, dim_b]));
    let _t2 = types2.tensor_with_shape(types2.primitive(Primitive::Numerus), shape);
    let snap2 = types2.snapshot();
    let _restored = TypeTable::from_snapshot(snap2).expect("valid snapshot must restore");
}

// --- F-066: structural assignability arms ---

#[test]
fn f066_tuple_members_invariant() {
    // Tuple members are type-argument positions: Tuple<i32, i64> is not a
    // Tuple<i64, i64> in either direction (ruling 21 row 25).
    let mut types = TypeTable::new();
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let tuple_i32_i64 = types.tuple(vec![i32, i64]);
    let tuple_i64_i64 = types.tuple(vec![i64, i64]);

    assert!(!types.is_assignable(tuple_i32_i64, tuple_i64_i64));
    assert!(!types.is_assignable(tuple_i64_i64, tuple_i32_i64));
    assert!(types.is_assignable(tuple_i32_i64, tuple_i32_i64));
}

#[test]
fn f066_tuple_different_length_not_assignable() {
    let mut types = TypeTable::new();
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let pair = types.tuple(vec![i32, i32]);
    let triple = types.tuple(vec![i32, i32, i32]);

    assert!(
        !types.is_assignable(pair, triple),
        "F-066: different-length tuples must not be assignable"
    );
}

#[test]
fn f066_applied_invariant_assignable() {
    // Applied<Vec, [numerus]> is assignable only to itself (invariant).
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let base = types.intern(Type::Struct(DefId(100)));
    let applied_numerus = types.intern(Type::Applied(base, vec![AppliedArg::Type(numerus)]));
    let applied_i64 = types.intern(Type::Applied(base, vec![AppliedArg::Type(i64)]));

    // numerus == i64 via equals, so Applied should still be equal.
    assert!(
        types.equals(applied_numerus, applied_i64),
        "Applied<numerus> equals Applied<i64> because numerus == i64"
    );
    assert!(
        types.is_assignable(applied_numerus, applied_i64),
        "F-066: Applied<numerus> assignable to Applied<i64> via invariant equals"
    );
    assert!(
        types.is_assignable(applied_numerus, applied_numerus),
        "F-066: same Applied must be self-assignable"
    );
}

#[test]
fn f066_intervallum_covariant_assignable() {
    // Intervallum<i32> must be assignable to Intervallum<i64> (covariant bound).
    let mut types = TypeTable::new();
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let inter_i32 = types.intervallum(i32);
    let inter_i64 = types.intervallum(i64);

    assert!(
        types.is_assignable(inter_i32, inter_i64),
        "F-066: Intervallum<i32> must be assignable to Intervallum<i64> (covariant)"
    );
    assert!(
        !types.is_assignable(inter_i64, inter_i32),
        "F-066: Intervallum<i64> must NOT be assignable to Intervallum<i32> (narrowing bound)"
    );
}

#[test]
fn intervallum_step_is_part_of_type_identity() {
    // `intervallum<T>` is step `Literal(1)`; an explicit step is a distinct
    // type identity, and equality never converts across steps.
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let step_one = types.intern_index(IndexExpr::Literal(1));
    let step_two = types.intern_index(IndexExpr::Literal(2));

    let implicit_one = types.intervallum(numerus);
    let explicit_one = types.intervallum_with_step(numerus, step_one);
    let explicit_two = types.intervallum_with_step(numerus, step_two);

    assert_eq!(
        implicit_one, explicit_one,
        "intervallum<T> interns as step Literal(1)"
    );
    assert_ne!(
        implicit_one, explicit_two,
        "different steps are different types"
    );
    assert!(types.equals(implicit_one, explicit_one));
    assert!(!types.equals(implicit_one, explicit_two));
    assert!(!types.is_assignable(implicit_one, explicit_two));
    assert!(!types.is_assignable(explicit_two, implicit_one));
}

#[test]
fn f066_collection_type_arguments_invariant() {
    // lista<i32> is not a lista<i64>, in either direction: both names would
    // share one storage and the store policy belongs to the destination cell
    // (ruling 21 row 25, ruling 51 A7).
    let mut types = TypeTable::new();
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let lista_i32 = types.array(i32);
    let lista_i64 = types.array(i64);
    assert!(!types.is_assignable(lista_i32, lista_i64));
    assert!(!types.is_assignable(lista_i64, lista_i32));
    assert!(types.is_assignable(lista_i32, lista_i32));

    let set_i32 = types.set(i32);
    let set_i64 = types.set(i64);
    assert!(!types.is_assignable(set_i32, set_i64));
    assert!(!types.is_assignable(set_i64, set_i32));

    let prom_i32 = types.promissum(i32);
    let prom_i64 = types.promissum(i64);
    assert!(!types.is_assignable(prom_i32, prom_i64));
    assert!(!types.is_assignable(prom_i64, prom_i32));

    let cur_i32 = types.cursor(i32);
    let cur_i64 = types.cursor(i64);
    assert!(!types.is_assignable(cur_i32, cur_i64));
    assert!(!types.is_assignable(cur_i64, cur_i32));

    let queue_i32 = types.queue(i32);
    let queue_i64 = types.queue(i64);
    assert!(!types.is_assignable(queue_i32, queue_i64));
    let stack_i32 = types.stack(i32);
    let stack_i64 = types.stack(i64);
    assert!(!types.is_assignable(stack_i32, stack_i64));
}

#[test]
fn f066_map_type_arguments_invariant() {
    let mut types = TypeTable::new();
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let map_i32_i32 = types.map(i32, i32);
    let map_i64_i64 = types.map(i64, i64);
    let map_i64_i32 = types.map(i64, i32);
    let map_i32_i64 = types.map(i32, i64);

    assert!(!types.is_assignable(map_i32_i32, map_i64_i64));
    assert!(!types.is_assignable(map_i64_i64, map_i32_i32));
    // Key and value are each invariant on their own.
    assert!(!types.is_assignable(map_i32_i32, map_i64_i32));
    assert!(!types.is_assignable(map_i32_i32, map_i32_i64));
    assert!(types.is_assignable(map_i32_i32, map_i32_i32));
}

#[test]
fn f066_tensor_vector_matrix_sparsa_element_and_shape_invariant() {
    // tensor<F32, [2,3]> is not a tensor<F64, [2,3]> (element and shape both
    // invariant).
    let mut types = TypeTable::new();
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let f64 = types.sized_numeric(Primitive::Fractus, NumericWidth::F64);
    let dim2 = types.intern_index(IndexExpr::Literal(2));
    let dim3 = types.intern_index(IndexExpr::Literal(3));
    let shape_2x3 = types.intern_index(IndexExpr::Tuple(vec![dim2, dim3]));
    let dim4 = types.intern_index(IndexExpr::Literal(4));
    let shape_2x4 = types.intern_index(IndexExpr::Tuple(vec![dim2, dim4]));

    let t_f32_2x3 = types.tensor_with_shape(f32, shape_2x3);
    let t_f64_2x3 = types.tensor_with_shape(f64, shape_2x3);
    let t_f32_2x4 = types.tensor_with_shape(f32, shape_2x4);

    assert!(
        !types.is_assignable(t_f32_2x3, t_f64_2x3),
        "tensor<F32,[2,3]> must NOT be assignable to tensor<F64,[2,3]> (element invariant)"
    );
    assert!(types.is_assignable(t_f32_2x3, t_f32_2x3));
    assert!(
        !types.is_assignable(t_f32_2x3, t_f32_2x4),
        "F-066: tensor<F32,[2,3]> must NOT be assignable to tensor<F32,[2,4]> (shape invariant)"
    );

    // Vector: element and width invariant.
    let v_f32_w2 = types.vector_with_width(f32, dim2);
    let v_f64_w2 = types.vector_with_width(f64, dim2);
    let v_f32_w3 = types.vector_with_width(f32, dim3);
    assert!(!types.is_assignable(v_f32_w2, v_f64_w2));
    assert!(!types.is_assignable(v_f32_w2, v_f32_w3));

    // Matrix: element and shape invariant.
    let m_f32_2x3 = types.matrix_with_shape(f32, shape_2x3);
    let m_f64_2x3 = types.matrix_with_shape(f64, shape_2x3);
    let m_f32_2x4 = types.matrix_with_shape(f32, shape_2x4);
    assert!(!types.is_assignable(m_f32_2x3, m_f64_2x3));
    assert!(!types.is_assignable(m_f32_2x3, m_f32_2x4));

    // Sparsa: element and shape invariant.
    let s_f32_2x3 = types.sparsa_with_shape(f32, shape_2x3);
    let s_f64_2x3 = types.sparsa_with_shape(f64, shape_2x3);
    let s_f32_2x4 = types.sparsa_with_shape(f32, shape_2x4);
    assert!(!types.is_assignable(s_f32_2x3, s_f64_2x3));
    assert!(!types.is_assignable(s_f32_2x3, s_f32_2x4));
}

#[test]
fn f066_atomic_not_assignable_across_elements() {
    // Atomic wrappers are storage-sensitive: atomic<i32> must not be assignable
    // to atomic<i64> even though i32→i64 is widening for bare numerus (F-066
    // intentional exclusion). Same-TypeId atomic is trivially assignable via
    // the top-level `equals` fast-path in `is_assignable`.
    let mut types = TypeTable::new();
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let atomic_i32 = types.atomic(i32);
    let atomic_i64 = types.atomic(i64);

    assert!(
        !types.is_assignable(atomic_i32, atomic_i64),
        "F-066: atomic<i32> must NOT be assignable to atomic<i64> (storage-sensitive)"
    );
    // Same element → same TypeId is trivially assignable (top-level equals).
    assert!(types.is_assignable(atomic_i32, atomic_i32));

    // Element type not assignable to atomic wrapper.
    assert!(!types.is_assignable(i32, atomic_i32));
    assert!(!types.is_assignable(atomic_i32, i32));
}

#[test]
fn infallible_promissum_widens_to_failable() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let error_ty = types.primitive(Primitive::Textus);
    let infallible = types.promissum(textus);
    let failable = types.promissum_failable(textus, error_ty);

    // promissum<T> widens to promissum<T ⇥ E> (numquam bottom channel).
    assert!(
        types.is_assignable(infallible, failable),
        "infallible promissum<T> must widen to promissum<T ⇥ E>"
    );
}

#[test]
fn failable_promissum_does_not_narrow_to_infallible() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let error_ty = types.primitive(Primitive::Textus);
    let infallible = types.promissum(textus);
    let failable = types.promissum_failable(textus, error_ty);

    // Failable promises must NOT narrow to infallible (erases alternate channel).
    assert!(
        !types.is_assignable(failable, infallible),
        "failable promissum<T ⇥ E> must NOT narrow to promissum<T>"
    );
}

#[test]
fn failable_promissum_assignable_checks_both_channels() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);

    let a = types.promissum_failable(textus, textus);
    let b = types.promissum_failable(textus, numerus);

    // Alternate channel mismatch: textus not assignable to numerus.
    assert!(
        !types.is_assignable(a, b),
        "failable promissum assignability must check both channels"
    );

    // Same channels: assignable.
    assert!(types.is_assignable(a, a));
}

// --- BB-2: bounded buffer type identity ---

#[test]
fn bounded_forms_are_distinct_from_bare_primitives_and_each_other() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let ascii = types.primitive(Primitive::Ascii);
    let textus = types.primitive(Primitive::Textus);
    let octeti = types.primitive(Primitive::Octeti);
    let n8 = types.intern_index(IndexExpr::Literal(8));
    let n9 = types.intern_index(IndexExpr::Literal(9));
    let n64 = types.intern_index(IndexExpr::Literal(64));
    let n4 = types.intern_index(IndexExpr::Literal(4));

    let ascii8 = types.bounded_ascii(n8);
    let ascii9 = types.bounded_ascii(n9);
    let ascii64 = types.bounded_ascii(n64);
    let textus4 = types.bounded_textus(n4);
    let lista8 = types.bounded_array(numerus, n8);
    let lista = types.array(numerus);
    let octeti8 = types.bounded_octeti(n8);

    assert!(!types.equals(ascii8, ascii), "ascii<magnitudo 8> ≠ ascii");
    assert!(
        !types.equals(ascii8, ascii9),
        "ascii<magnitudo 8> ≠ ascii<magnitudo 9>"
    );
    assert!(!types.equals(ascii, ascii64), "ascii ≠ ascii<magnitudo 64>");
    assert!(
        !types.equals(textus4, textus),
        "textus<magnitudo 4> ≠ textus"
    );
    assert!(
        !types.equals(lista8, lista),
        "lista<numerus, magnitudo 8> ≠ lista<numerus>"
    );
    assert!(!types.equals(octeti8, octeti), "octeti<8> ≠ octeti");

    assert!(!types.is_assignable(ascii8, ascii));
    assert!(!types.is_assignable(ascii, ascii8));
    assert!(types.is_assignable(ascii8, ascii9));
    assert!(!types.is_assignable(ascii9, ascii8));
    assert!(!types.is_assignable(lista8, lista));
    assert!(!types.is_assignable(lista, lista8));
    assert!(types.is_assignable(ascii8, ascii8));
    assert!(types.is_assignable(lista8, lista8));

    assert!(matches!(types.get(lista), Type::Array(_)));
    assert!(matches!(
        types.get(lista8),
        Type::BoundedArray { element, capacity }
            if *element == numerus && types.index_equals(*capacity, n8)
    ));
}

#[test]
fn bounded_array_of_b8_is_bounded_octeti() {
    // Need: `lista<b8, magnitudo N>` is the bounded byte buffer. Its element
    // is raw bits, so the bounded list lowers to `Type::BoundedOcteti`
    // rather than an element-typed bounded array.
    let mut types = TypeTable::new();
    let b8 = types.bits(BitsWidth::B8);
    let n8 = types.intern_index(IndexExpr::Literal(8));
    let n16 = types.intern_index(IndexExpr::Literal(16));

    let byte_list = types.bounded_array(b8, n8);
    let buffer = types.bounded_octeti(n8);
    assert_eq!(byte_list, buffer);

    let u8_sized = types.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let numeric_list = types.bounded_array(u8_sized, n8);
    assert_ne!(numeric_list, buffer);
    assert!(matches!(types.get(numeric_list), Type::BoundedArray { .. }));

    let other_capacity = types.bounded_array(b8, n16);
    assert_ne!(other_capacity, byte_list);
}

#[test]
fn lista_accepts_one_or_two_applied_arguments() {
    assert_eq!(CollectionKind::Lista.arity(), 1);
    assert!(CollectionKind::Lista.accepts_arity(1));
    assert!(CollectionKind::Lista.accepts_arity(2));
    assert!(!CollectionKind::Lista.accepts_arity(0));
    assert!(!CollectionKind::Lista.accepts_arity(3));
    assert_eq!(
        CollectionKind::Lista.arity_error(),
        "lista requires one or two type parameters"
    );
    assert!(!CollectionKind::Tabula.accepts_arity(1));
}

#[test]
fn bounded_ascii_one_is_not_bare_ascii() {
    let mut types = TypeTable::new();
    let ascii = types.primitive(Primitive::Ascii);
    let n1 = types.intern_index(IndexExpr::Literal(1));
    let ascii1 = types.bounded_ascii(n1);
    assert!(matches!(
        types.get(ascii1),
        Type::BoundedAscii { capacity } if types.index_equals(*capacity, n1)
    ));
    assert!(!types.equals(ascii1, ascii));
}

#[test]
fn bounded_ascii_one_assigns_to_textus_like_bare_ascii() {
    let mut types = TypeTable::new();
    let ascii = types.primitive(Primitive::Ascii);
    let textus = types.primitive(Primitive::Textus);
    let n1 = types.intern_index(IndexExpr::Literal(1));
    let n8 = types.intern_index(IndexExpr::Literal(8));
    let ascii1 = types.bounded_ascii(n1);
    let ascii8 = types.bounded_ascii(n8);

    assert!(types.is_assignable(ascii, textus), "bare ascii → textus");
    assert!(
        types.is_assignable(ascii1, textus),
        "ascii<magnitudo 1> → textus (D17 payload widening)"
    );
    assert!(
        !types.is_assignable(ascii8, textus),
        "ascii<magnitudo 8> reaches textus via ↦ conversio, not implicit widening"
    );
    assert!(
        !types.is_assignable(textus, ascii1),
        "textus ↛ ascii<magnitudo 1>"
    );
    assert!(
        types.is_assignable(ascii1, ascii),
        "ascii<magnitudo 1> → ascii (D17)"
    );
}

#[test]
fn parse_natural_capacity_accepts_decimal_digits_only() {
    assert_eq!(crate::parse_natural_capacity("8"), Some(8));
    assert_eq!(crate::parse_natural_capacity("0"), Some(0));
    assert_eq!(crate::parse_natural_capacity(""), None);
    assert_eq!(crate::parse_natural_capacity("N"), None);
    assert_eq!(crate::parse_natural_capacity("N+1"), None);
    assert_eq!(crate::parse_natural_capacity("8u"), None);
}

// ── U0: Type::Intersection canonicalization ──────────────────────────────────

#[test]
fn intersection_interning_is_order_insensitive() {
    let mut types = TypeTable::new();
    let a = types.intern(Type::Struct(DefId(1)));
    let b = types.intern(Type::Struct(DefId(2)));
    let first = types.intern(Type::Intersection(vec![a, b]));
    let second = types.intern(Type::Intersection(vec![b, a]));
    assert_eq!(first, second, "member order must not affect identity");
}

#[test]
fn intersection_equality_ignores_member_order() {
    let mut types = TypeTable::new();
    let a = types.intern(Type::Struct(DefId(1)));
    let b = types.intern(Type::Struct(DefId(2)));
    let first = types.intern(Type::Intersection(vec![a, b]));
    let second = types.intern(Type::Intersection(vec![b, a]));
    assert!(types.equals(first, second));
    assert!(!types.equals(first, a));
}

// --- N7E-1: policy-float cell type (plumbing; nothing constructs it yet) ---

#[test]
fn policy_float_interns_by_policy_and_width() {
    let mut types = TypeTable::new();
    let trapping_f32 = types.intern(Type::PolicyFloat(FloatPolicy::Trapping, NumericWidth::F32));
    assert_eq!(
        types.intern(Type::PolicyFloat(FloatPolicy::Trapping, NumericWidth::F32)),
        trapping_f32
    );
    let saturating_f32 = types.intern(Type::PolicyFloat(
        FloatPolicy::Saturating,
        NumericWidth::F32,
    ));
    let trapping_f64 = types.intern(Type::PolicyFloat(FloatPolicy::Trapping, NumericWidth::F64));
    assert_ne!(trapping_f32, saturating_f32);
    assert_ne!(trapping_f32, trapping_f64);
    assert!(types.equals(trapping_f32, trapping_f32));
    assert!(!types.equals(trapping_f32, saturating_f32));
    assert!(!types.equals(trapping_f32, trapping_f64));
    // A cell is its own type, never the bare float of its width.
    let bare_f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    assert_ne!(trapping_f32, bare_f32);
    assert!(!types.equals(trapping_f32, bare_f32));
}

#[test]
fn policy_float_is_a_scalar_for_access_markers() {
    let mut types = TypeTable::new();
    let cell = types.intern(Type::PolicyFloat(
        FloatPolicy::Saturating,
        NumericWidth::F16,
    ));
    assert!(types.is_scalar_for_access_marker(cell));
}

#[test]
fn first_policy_float_tracks_the_table_and_survives_snapshot() {
    let mut types = TypeTable::new();
    assert_eq!(types.first_policy_float(), None);
    let _ = types.intern(Type::PolicyFloat(
        FloatPolicy::Saturating,
        NumericWidth::BF16,
    ));
    let _ = types.intern(Type::PolicyFloat(FloatPolicy::Trapping, NumericWidth::F64));
    assert_eq!(
        types.first_policy_float(),
        Some((FloatPolicy::Saturating, NumericWidth::BF16))
    );
    let restored = TypeTable::from_snapshot(types.snapshot()).expect("snapshot");
    assert_eq!(restored.first_policy_float(), types.first_policy_float());
}

#[test]
fn policy_float_policy_words_are_the_surface_words() {
    assert_eq!(FloatPolicy::Trapping.word(), "trapping");
    assert_eq!(FloatPolicy::Saturating.word(), "saturating");
}

#[test]
fn affine_index_arithmetic_is_structured_exact_and_natural() {
    let mut types = TypeTable::new();
    let parameter = types.intern_index(IndexExpr::Param(radix_lexer::Symbol(31)));
    let minus_one = add_index_constant(&mut types, parameter, -1).expect("symbolic offset");
    assert!(matches!(
        types.get_index(minus_one),
        IndexExpr::Affine { base, offset: -1 } if *base == parameter
    ));
    assert_eq!(evaluate_index_dimension(minus_one, &types), Ok(None));

    let plus_two = add_index_constant(&mut types, minus_one, 3).expect("combine offsets");
    assert!(matches!(
        types.get_index(plus_two),
        IndexExpr::Affine { base, offset: 2 } if *base == parameter
    ));

    let canceled = compose_index_affine_constant(&mut types, minus_one, 1)
        .expect("compose affine cancellation");
    assert_eq!(canceled, parameter, "N - 1 + 1 normalizes to N");
    let zero = types.intern_index(IndexExpr::Literal(0));
    let negative_intermediate = compose_index_affine_constant(&mut types, zero, -1)
        .expect("an additive chain may carry an intermediate offset");
    let concrete_after_cancellation =
        compose_index_affine_constant(&mut types, negative_intermediate, 1)
            .expect("final natural extent remains representable");
    assert_eq!(
        evaluate_index_dimension(concrete_after_cancellation, &types),
        Ok(Some(0)),
        "natural validity applies after the complete additive chain"
    );

    let five = types.intern_index(IndexExpr::Literal(5));
    let six = types.intern_index(IndexExpr::Literal(6));
    assert_eq!(
        divide_index_exact(&mut types, five, 2),
        Err(IndexArithmeticError::NonIntegral)
    );
    assert_eq!(
        divide_index_exact(&mut types, six, 0),
        Err(IndexArithmeticError::DivideByZero)
    );
    let three = divide_index_exact(&mut types, six, 2).expect("exact division");
    assert!(matches!(types.get_index(three), IndexExpr::Literal(3)));

    let zero = types.intern_index(IndexExpr::Literal(0));
    assert_eq!(
        add_index_constant(&mut types, zero, -1),
        Err(IndexArithmeticError::Negative)
    );
    let shape = types.intern_index(IndexExpr::Tuple(vec![minus_one, three]));
    assert_eq!(i64s_from_literal_index(shape, &types), None);
    let concrete = types.intern_index(IndexExpr::Tuple(vec![three, six]));
    assert_eq!(i64s_from_literal_index(concrete, &types), Some(vec![3, 6]));
}

/// Raw-bits family (`b8`/`b16`/`b32`/`b64`): interned per width, equal only to
/// the same width, and joined to no numeric type or other width by any
/// assignability or lattice edge. Bits are not numbers.
#[test]
fn bits_types_intern_per_width_and_equal_only_to_same_width() {
    let mut types = TypeTable::new();
    let b8 = types.bits(BitsWidth::B8);
    let b16 = types.bits(BitsWidth::B16);
    let b32 = types.bits(BitsWidth::B32);
    let b64 = types.bits(BitsWidth::B64);

    assert_eq!(types.bits(BitsWidth::B8), b8, "interning is stable");
    assert_eq!(types.bits(BitsWidth::B64), b64);
    let all = [b8, b16, b32, b64];
    for (i, a) in all.iter().enumerate() {
        for (j, b) in all.iter().enumerate() {
            assert_eq!(types.equals(*a, *b), i == j, "equals {i} vs {j}");
            assert_eq!(types.is_assignable(*a, *b), i == j, "assignable {i} vs {j}");
        }
    }
    assert_eq!(BitsWidth::from_marker("b16"), Some(BitsWidth::B16));
    assert_eq!(BitsWidth::B64.bits(), 64);
    assert_eq!(BitsWidth::B32.faber_marker(), "b32");
    assert_eq!(BitsWidth::from_marker("b7"), None);
    assert_eq!(BitsWidth::from_marker("u8"), None);
}

#[test]
fn bits_types_are_not_assignable_to_or_from_numeric_types() {
    let mut types = TypeTable::new();
    let b8 = types.bits(BitsWidth::B8);
    let b64 = types.bits(BitsWidth::B64);
    let numerics = [
        types.sized_numeric(Primitive::Numerus, NumericWidth::U8),
        types.sized_numeric(Primitive::Numerus, NumericWidth::U64),
        types.sized_numeric(Primitive::Numerus, NumericWidth::I64),
        types.primitive(Primitive::Numerus),
        types.primitive(Primitive::Fractus),
        types.primitive(Primitive::Octeti),
    ];
    for numeric in numerics {
        for bits in [b8, b64] {
            assert!(!types.equals(bits, numeric));
            assert!(!types.is_assignable(bits, numeric), "bits -> numeric");
            assert!(!types.is_assignable(numeric, bits), "numeric -> bits");
        }
    }
    // The no-widening rule lives on the element: lista<b8> is not lista<u8>.
    let lista_b8 = types.array(b8);
    let u8 = types.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let lista_u8 = types.array(u8);
    assert!(!types.is_assignable(lista_b8, lista_u8));
    assert!(!types.is_assignable(lista_u8, lista_b8));
    let b16 = types.bits(BitsWidth::B16);
    let lista_b16 = types.array(b16);
    assert!(!types.is_assignable(lista_b8, lista_b16));
}

#[test]
fn bits_types_order_by_bit_pattern_as_unsigned_integer() {
    use crate::ordering::{OrderingKind, ordering_kind};
    let mut types = TypeTable::new();
    let b8 = types.bits(BitsWidth::B8);
    let kind = ordering_kind(&types, b8, &|_| false, &|_| false);
    assert_eq!(kind, Some(OrderingKind::Integer));
}

#[test]
fn ledger_is_a_distinct_two_argument_collection_from_map() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);
    let map = CollectionKind::Tabula.lower(&mut types, &[textus, numerus]);
    let ledger = CollectionKind::Tabulae.lower(&mut types, &[textus, numerus]);

    assert_eq!(
        CollectionKind::from_name("tabulae"),
        Some(CollectionKind::Tabulae)
    );
    assert_eq!(CollectionKind::Tabulae.arity(), 2);
    assert!(matches!(types.get(ledger), Type::Ledger(k, v) if *k == textus && *v == numerus));
    assert_ne!(map, ledger);
    assert!(!types.equals(map, ledger));
    assert!(!types.is_assignable(map, ledger));
    assert!(!types.is_assignable(ledger, map));
}

#[test]
fn func_sig_params_differing_only_in_sponte_are_not_equal() {
    // Need: a `sponte` slot and a merely defaulted slot are distinct
    // parameter contracts; their function types must not unify.
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let vacuum = types.primitive(Primitive::Vacuum);
    let sig_of = |sponte: bool| FuncSig {
        type_params: Vec::new(),
        params: vec![ParamType {
            ty: numerus,
            mode: SemanticParamMode::Owned,
            optional: true,
            sponte,
        }],
        ret: vacuum,
        err: None,
        is_async: false,
        is_generator: false,
    };
    let sponte_fn = types.function(sig_of(true));
    let defaulted_fn = types.function(sig_of(false));
    assert!(!types.equals(sponte_fn, defaulted_fn));
    let sponte_again = types.function(sig_of(true));
    assert!(types.equals(sponte_fn, sponte_again));
}

// --- Direct behavior coverage for type-table helpers with boundary paths ---

#[test]
fn record_holes_struct_lookup_and_unit_tuple_helpers() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let struct_id = types.intern(Type::Struct(DefId(900)));
    assert_eq!(types.find_struct(DefId(900)), Some(struct_id));
    assert_eq!(types.find_struct(DefId(901)), None);

    assert_eq!(types.find_tuple(&[]), None);
    let unit = types.unit();
    assert!(matches!(types.get(unit), Type::Tuple(elements) if elements.is_empty()));
    assert_eq!(types.find_tuple(&[]), Some(unit));
    assert_eq!(types.unit(), unit, "unit is reused once interned");

    let pair = types.tuple(vec![numerus, struct_id]);
    assert_eq!(types.find_tuple(&[numerus, struct_id]), Some(pair));
    assert_eq!(types.find_tuple(&[struct_id, numerus]), None);

    let hole = types.intern(Type::Infer(InferVar(44)));
    let other_hole = types.intern(Type::Infer(InferVar(45)));
    let union_hole = types.intern(Type::InferUnion(InferVar(44)));
    assert!(!types.is_default_record_hole(hole));
    types.mark_default_record_hole(InferVar(44));
    assert!(types.is_default_record_hole(hole));
    assert!(!types.is_default_record_hole(other_hole));
    assert!(
        !types.is_default_record_hole(union_hole),
        "the default-record marker applies only to an Infer hole"
    );

    assert_eq!(types.fresh_index_infer_var(), crate::index::IndexVar(0));
    assert_eq!(types.fresh_index_infer_var(), crate::index::IndexVar(1));
}

#[test]
fn ranged_array_constructor_normalizes_the_unbounded_and_capacity_edges() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let unbounded = types.ranged_array(textus, 0, None);
    assert!(matches!(types.get(unbounded), Type::Array(element) if *element == textus));

    let explicit_capacity = types.intern_index(IndexExpr::Literal(4));
    let zero_to_four = types.ranged_array(textus, 0, Some(4));
    let bounded = types.bounded_array(textus, explicit_capacity);
    assert!(matches!(types.get(zero_to_four), Type::BoundedArray { .. }));
    assert!(types.equals(zero_to_four, bounded));

    let positive_minimum = types.ranged_array(textus, 2, None);
    let finite_range = types.ranged_array(textus, 2, Some(9));
    assert!(matches!(
        types.get(positive_minimum),
        Type::RangedArray { element, min: 2, max: None } if *element == textus
    ));
    assert!(matches!(
        types.get(finite_range),
        Type::RangedArray { element, min: 2, max: Some(9) } if *element == textus
    ));
    assert!(!types.equals(positive_minimum, finite_range));
}

#[test]
fn ranged_integer_constructors_normalize_full_widths_and_find_the_base() {
    let mut types = TypeTable::new();
    let full_trapping = types.ranged_int(IntPolicy::Trapping, NumericWidth::I8, -128, 127);
    let full_wrapping = types.ranged_int(IntPolicy::Wrapping, NumericWidth::U8, 0, 255);
    let full_saturating = types.ranged_int(IntPolicy::Saturating, NumericWidth::U8, 0, 255);
    assert!(matches!(
        types.get(full_trapping),
        Type::SizedNumeric(Primitive::Numerus, NumericWidth::I8)
    ));
    assert!(matches!(
        types.get(full_wrapping),
        Type::ModularWord(NumericWidth::U8)
    ));
    assert!(matches!(
        types.get(full_saturating),
        Type::SaturatingWord(NumericWidth::U8)
    ));
    assert_eq!(types.ranged_int_parts(full_trapping), None);

    let narrow = types.ranged_int(IntPolicy::Trapping, NumericWidth::I8, -5, 7);
    let alias = types.intern(Type::Alias(DefId(902), narrow));
    assert_eq!(
        types.ranged_int_parts(narrow),
        Some((IntPolicy::Trapping, NumericWidth::I8, -5, 7))
    );
    assert_eq!(
        types.ranged_int_parts(alias),
        types.ranged_int_parts(narrow)
    );
    assert_eq!(types.ranged_int_base_existing(narrow), full_trapping);
    assert_eq!(types.ranged_int_base_existing(alias), full_trapping);
    assert_eq!(types.ranged_int_base(narrow), full_trapping);
    assert_eq!(types.ranged_int_base(alias), full_trapping);
    assert_eq!(types.ranged_int_base_existing(full_trapping), full_trapping);

    let wrapped_narrow = types.ranged_int(IntPolicy::Wrapping, NumericWidth::U8, 2, 20);
    let saturated_narrow = types.ranged_int(IntPolicy::Saturating, NumericWidth::U8, 2, 20);
    assert!(matches!(
        types.get(types.ranged_int_base_existing(wrapped_narrow)),
        Type::ModularWord(NumericWidth::U8)
    ));
    assert!(matches!(
        types.get(types.ranged_int_base_existing(saturated_narrow)),
        Type::SaturatingWord(NumericWidth::U8)
    ));
}

#[test]
fn indexed_type_equality_compares_expression_structure_recursively() {
    let mut types = TypeTable::new();
    let literal_a = types.intern_index(IndexExpr::Literal(8));
    let literal_b = types.intern_index(IndexExpr::Literal(8));
    let literal_other = types.intern_index(IndexExpr::Literal(9));
    let parameter_a = types.intern_index(IndexExpr::Param(radix_lexer::Symbol(70)));
    let parameter_b = types.intern_index(IndexExpr::Param(radix_lexer::Symbol(70)));
    let parameter_other = types.intern_index(IndexExpr::Param(radix_lexer::Symbol(71)));
    assert!(types.index_equals(literal_a, literal_b));
    assert!(!types.index_equals(literal_a, literal_other));
    assert!(types.index_equals(parameter_a, parameter_b));
    assert!(!types.index_equals(parameter_a, parameter_other));

    let runtime_a = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarParameter {
        parameter: 2,
    }));
    let runtime_b = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarParameter {
        parameter: 2,
    }));
    let runtime_other = types.intern_index(IndexExpr::Runtime(RuntimeExtentId::ScalarParameter {
        parameter: 3,
    }));
    assert!(types.index_equals(runtime_a, runtime_b));
    assert!(!types.index_equals(runtime_a, runtime_other));

    let affine_a = types.intern_index(IndexExpr::Affine {
        base: parameter_a,
        offset: -1,
    });
    let affine_b = types.intern_index(IndexExpr::Affine {
        base: parameter_b,
        offset: -1,
    });
    let affine_other = types.intern_index(IndexExpr::Affine {
        base: parameter_b,
        offset: 1,
    });
    assert!(types.index_equals(affine_a, affine_b));
    assert!(!types.index_equals(affine_a, affine_other));
    let divide_a = types.intern_index(IndexExpr::Divide {
        numerator: literal_a,
        divisor: 2,
    });
    let divide_b = types.intern_index(IndexExpr::Divide {
        numerator: literal_b,
        divisor: 2,
    });
    let divide_other = types.intern_index(IndexExpr::Divide {
        numerator: literal_b,
        divisor: 4,
    });
    assert!(types.index_equals(divide_a, divide_b));
    assert!(!types.index_equals(divide_a, divide_other));

    let infer_a = types.intern_index(IndexExpr::Infer(crate::index::IndexVar(5)));
    let infer_b = types.intern_index(IndexExpr::Infer(crate::index::IndexVar(5)));
    let infer_other = types.intern_index(IndexExpr::Infer(crate::index::IndexVar(6)));
    let unspecified_a = types.intern_index(IndexExpr::Unspecified);
    let unspecified_b = types.intern_index(IndexExpr::Unspecified);
    assert!(types.index_equals(infer_a, infer_b));
    assert!(!types.index_equals(infer_a, infer_other));
    assert!(types.index_equals(unspecified_a, unspecified_b));
    assert!(!types.index_equals(unspecified_a, literal_a));

    let tuple_a = types.intern_index(IndexExpr::Tuple(vec![literal_a, affine_a]));
    let tuple_b = types.intern_index(IndexExpr::Tuple(vec![literal_b, affine_b]));
    let tuple_reordered = types.intern_index(IndexExpr::Tuple(vec![affine_b, literal_b]));
    let tuple_short = types.intern_index(IndexExpr::Tuple(vec![literal_b]));
    assert!(types.index_equals(tuple_a, tuple_b));
    assert!(!types.index_equals(tuple_a, tuple_reordered));
    assert!(!types.index_equals(tuple_a, tuple_short));
    assert!(!types.index_equals(tuple_a, parameter_a));
}

#[test]
fn type_family_predicates_accept_only_the_documented_shapes() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let fractus = types.primitive(Primitive::Fractus);
    let instans = types.primitive(Primitive::Instans);
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let u32 = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let sized_instans = types.sized_instans(InstansPrecision::Millis);

    for accepted in [numerus, instans, i32, u32, i64, sized_instans] {
        assert!(is_intervallum_bound_type(&types, accepted), "{accepted:?}");
    }
    for rejected in [fractus, types.primitive(Primitive::Textus)] {
        assert!(!is_intervallum_bound_type(&types, rejected), "{rejected:?}");
    }
    assert!(is_atomic_element_type(&types, i32));
    assert!(is_atomic_element_type(&types, u32));
    for rejected in [numerus, i64, fractus, sized_instans] {
        assert!(!is_atomic_element_type(&types, rejected), "{rejected:?}");
    }

    assert_eq!(types.first_saturating_word_width(), None);
    let _first = types.intern(Type::SaturatingWord(NumericWidth::U16));
    let _later = types.intern(Type::SaturatingWord(NumericWidth::I32));
    assert_eq!(types.first_saturating_word_width(), Some(NumericWidth::U16));
    let restored = TypeTable::from_snapshot(types.snapshot()).expect("snapshot");
    assert_eq!(
        restored.first_saturating_word_width(),
        Some(NumericWidth::U16)
    );
}

#[test]
fn policy_spellings_keep_english_words_and_latin_identities_distinct() {
    assert_eq!(IntPolicy::Trapping.word(), "trapping");
    assert_eq!(IntPolicy::Wrapping.word(), "wrapping");
    assert_eq!(IntPolicy::Saturating.word(), "saturating");
    assert_eq!(IntPolicy::Trapping.latin(), "exactus");
    assert_eq!(IntPolicy::Wrapping.latin(), "modulus");
    assert_eq!(IntPolicy::Saturating.latin(), "saturatus");

    for policy in BuiltinTensorEdgePolicy::ALL {
        assert_eq!(
            BuiltinTensorEdgePolicy::from_latin(policy.latin()),
            Some(policy)
        );
    }
    assert_eq!(BuiltinTensorEdgePolicy::from_latin("clamp"), None);
}
