//! Type-model coverage through the semantic re-export boundary.
//!
//! These tests prove assignability, equality, snapshots, constructor behavior,
//! and the primitive / collection / tensor-family surface rows the rest of the
//! semantic pipeline consumes.

use super::{
    CollectionKind, Primitive, TensorFamilyKind, Type, TypeTable, collection_type_specs,
    is_atomic_element_type, is_intervallum_bound_type, lookup_collection_type_spec,
    lookup_primitive_type_spec, lookup_tensor_family_type_spec, primitive_type_specs,
    tensor_family_type_specs,
};
use crate::NumericWidth;
use crate::index::IndexExpr;
use radix_hir::DefId;

#[test]
fn allows_assigning_concrete_type_to_ignotum() {
    let types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let ignotum = types.primitive(Primitive::Ignotum);

    assert!(types.is_assignable(textus, ignotum));
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
fn union_equality_ignores_member_order() {
    let mut types = TypeTable::new();
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);

    let first = types.intern(Type::Union(vec![textus, numerus]));
    let second = types.intern(Type::Union(vec![numerus, textus]));

    assert!(types.equals(first, second));
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
    let millis = types.sized_instans(crate::InstansPrecision::Millis);
    assert!(!types.equals(bare, millis));
}

#[test]
fn instans_coarse_to_fine_widening_assignable() {
    let mut types = TypeTable::new();
    let bare = types.primitive(Primitive::Instans);
    let millis = types.sized_instans(crate::InstansPrecision::Millis);
    let nanos = types.sized_instans(crate::InstansPrecision::Nanos);

    assert!(types.is_assignable(bare, millis));
    assert!(types.is_assignable(millis, nanos));
    assert!(!types.is_assignable(nanos, millis));
    assert!(!types.is_assignable(millis, bare));
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
    assert_eq!(
        lookup_collection_type_spec("cursor").map(|spec| spec.kind),
        Some(CollectionKind::Cursor)
    );
    assert_eq!(lookup_collection_type_spec("tensor"), None);
}

#[test]
fn tensor_family_type_surface_is_closed_and_disjoint_from_collections() {
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
    assert_eq!(lookup_tensor_family_type_spec("lista"), None);
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
fn atomic_element_type_accepts_only_i32_and_u32() {
    let mut types = TypeTable::new();
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let u32 = types.sized_numeric(Primitive::Numerus, NumericWidth::U32);
    let i64 = types.sized_numeric(Primitive::Numerus, NumericWidth::I64);
    let u8 = types.sized_numeric(Primitive::Numerus, NumericWidth::U8);
    let bare = types.primitive(Primitive::Numerus);
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);
    let textus = types.primitive(Primitive::Textus);

    assert!(is_atomic_element_type(&types, i32));
    assert!(is_atomic_element_type(&types, u32));
    assert!(!is_atomic_element_type(&types, i64));
    assert!(!is_atomic_element_type(&types, u8));
    assert!(!is_atomic_element_type(&types, bare));
    assert!(!is_atomic_element_type(&types, f32));
    assert!(!is_atomic_element_type(&types, textus));
}

#[test]
fn intervallum_bound_type_accepts_numerus_and_instans() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let i32 = types.sized_numeric(Primitive::Numerus, NumericWidth::I32);
    let instans = types.primitive(Primitive::Instans);
    let millis = types.sized_instans(crate::InstansPrecision::Millis);
    let fractus = types.primitive(Primitive::Fractus);
    let textus = types.primitive(Primitive::Textus);
    let f32 = types.sized_numeric(Primitive::Fractus, NumericWidth::F32);

    assert!(is_intervallum_bound_type(&types, numerus));
    assert!(is_intervallum_bound_type(&types, i32));
    assert!(is_intervallum_bound_type(&types, instans));
    assert!(is_intervallum_bound_type(&types, millis));
    assert!(!is_intervallum_bound_type(&types, fractus));
    assert!(!is_intervallum_bound_type(&types, textus));
    assert!(!is_intervallum_bound_type(&types, f32));
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
fn nullable_inner_resolves_option_union_and_alias() {
    let mut types = TypeTable::new();
    let numerus = types.primitive(Primitive::Numerus);
    let nihil = types.primitive(Primitive::Nihil);
    let textus = types.primitive(Primitive::Textus);

    // Option<T> -> Some(T)
    let opt_numerus = types.option(numerus);
    assert_eq!(types.nullable_inner(opt_numerus), Some(numerus));

    // T ∪ nihil with exactly one non-null member -> Some(T)
    let union_nullable = types.intern(Type::Union(vec![textus, nihil]));
    assert_eq!(types.nullable_inner(union_nullable), Some(textus));

    // Union without nihil, or with two non-null arms, is not nullable.
    let union_plain = types.intern(Type::Union(vec![textus, numerus]));
    assert_eq!(types.nullable_inner(union_plain), None);
    let union_multi = types.intern(Type::Union(vec![textus, numerus, nihil]));
    assert_eq!(types.nullable_inner(union_multi), None);

    // Alias chain traverses to the underlying nullable.
    let alias = types.intern(Type::Alias(DefId(0), opt_numerus));
    assert_eq!(types.nullable_inner(alias), Some(numerus));

    // Non-nullable primitive.
    assert_eq!(types.nullable_inner(numerus), None);
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
        "octeti should widen to lista<numerus<u8>>"
    );
    assert!(
        types.is_assignable(lista_u8, octeti),
        "lista<numerus<u8>> should narrow to octeti"
    );
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

#[test]
fn nihil_is_not_assignable_to_non_nullable() {
    let types = TypeTable::new();
    let nihil = types.primitive(Primitive::Nihil);
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);

    // nihil should not assign to a non-nullable type...
    assert!(!types.is_assignable(nihil, textus));
    assert!(!types.is_assignable(nihil, numerus));
    // ...but a non-nullable type should not assign to nihil either.
    assert!(!types.is_assignable(textus, nihil));
    assert!(!types.is_assignable(numerus, nihil));
    // nihil does assign to itself.
    assert!(types.is_assignable(nihil, nihil));
}

#[test]
fn empty_union_equality() {
    let mut types = TypeTable::new();
    let empty = types.intern(Type::Union(vec![]));
    let also_empty = types.intern(Type::Union(vec![]));
    assert!(
        types.equals(empty, also_empty),
        "two empty unions must be equal"
    );
}

#[test]
fn nihil_in_union_makes_assignable_to_nullable_union() {
    let mut types = TypeTable::new();
    let nihil = types.primitive(Primitive::Nihil);
    let textus = types.primitive(Primitive::Textus);
    let numerus = types.primitive(Primitive::Numerus);
    let textus_opt = types.intern(Type::Union(vec![textus, nihil]));

    assert!(
        types.is_assignable(textus, textus_opt),
        "textus should assign to textus ∪ nihil"
    );
    assert!(
        types.is_assignable(nihil, textus_opt),
        "nihil should assign to textus ∪ nihil"
    );
    assert!(
        !types.is_assignable(numerus, textus_opt),
        "numerus should not assign to textus ∪ nihil"
    );
}
