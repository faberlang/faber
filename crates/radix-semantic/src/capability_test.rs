use super::*;
use radix_hir::HirId;

fn assert_missing<P>(answer: &CapabilityAnswer<P>) {
    assert_eq!(answer.reason(), Some(CapabilityReason::MissingFact));
    assert!(answer.proof().is_none());
}

#[test]
fn fail_closed_provider_rejects_every_type_and_index_query() {
    let provider = FailClosedCapabilityQueries;
    let type_id = TypeId(1);
    let index_id = IndexId(2);

    for kind in TypeCapabilityKind::ALL {
        assert_missing(&provider.type_capability(type_id, kind));
    }

    for kind in IndexCapabilityKind::ALL {
        assert_missing(&provider.index_capability(index_id, kind));
    }
}

#[test]
fn fail_closed_provider_rejects_every_place_query() {
    let provider = FailClosedCapabilityQueries;
    let hir_id = HirId(3);

    for kind in PlaceCapabilityKind::ALL {
        assert_missing(&provider.place_capability(hir_id, kind));
    }
}

#[test]
fn fail_closed_provider_rejects_every_function_query() {
    let provider = FailClosedCapabilityQueries;
    let def_id = DefId(4);

    for kind in FunctionCapabilityKind::ALL {
        assert_missing(&provider.function_capability(def_id, kind));
    }
}

#[test]
fn place_summary_is_keyed_by_hir_identity_and_fail_closed() {
    let summary = PlaceCapabilitySummary::missing(HirId(9));
    assert_eq!(summary.hir_id, HirId(9));

    for kind in PlaceCapabilityKind::ALL {
        assert_missing(summary.capability(kind));
    }
}

#[test]
fn every_classified_reason_remains_an_answer_not_a_negative_proof() {
    for reason in CapabilityReason::ALL {
        let answer: CapabilityAnswer<()> = CapabilityAnswer::Rejected(reason);
        assert_eq!(answer.reason(), Some(reason));
        assert!(answer.proof().is_none());
    }
}

#[test]
fn combined_query_interface_is_implemented_by_fail_closed_provider() {
    fn accepts_combined_provider<T: CapabilityQueries>(_: &T) {}

    fn accepts_singular_provider<T: CapabilityQuery>(_: &T) {}

    accepts_combined_provider(&FailClosedCapabilityQueries);
    accepts_singular_provider(&FailClosedCapabilityQueries);
}

// ── SCR-03 derived queries ───────────────────────────────────────────────────

use super::DerivedCapabilityQueries;
use crate::AccessFactTable;
use crate::function_facts::analyze_function_facts;
use crate::test_support::analyze_ok_la;
use radix_lexer::Symbol;
use radix_types::{
    IndexExpr, IndexVar, NumericWidth, Primitive as Prim, SemanticMutability, Type, TypeTable,
};

fn la_pack() -> crate::locale::LocalePack {
    crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("la"))
        .expect("load Latin locale pack")
}

/// A query layer plus the owned authorities it borrows.
struct Fixture {
    queries: Box<DerivedCapabilityQueries<'static>>,
}

impl std::ops::Deref for Fixture {
    type Target = DerivedCapabilityQueries<'static>;
    fn deref(&self) -> &Self::Target {
        &self.queries
    }
}

/// Build the query layer over a locally-constructed `TypeTable` with empty
/// authority rows.
fn table_queries(types: TypeTable) -> Fixture {
    let empty = analyze_ok_la("incipit { }");
    let hir = empty.result.hir.expect("hir");
    let types: &'static TypeTable = Box::leak(Box::new(types));
    let access: &'static AccessFactTable = Box::leak(Box::default());
    let functions: &'static crate::function_facts::FunctionFactTable = Box::leak(Box::default());
    Fixture {
        queries: Box::new(DerivedCapabilityQueries::new(
            types,
            Box::leak(Box::new(hir)),
            access,
            functions,
        )),
    }
}

/// Analyze Faber source and build the full derived query layer. The borrowed
/// authorities are leaked test fixtures of one coherent snapshot.
fn source_queries(source: &str) -> (Fixture, &'static radix_hir::HirModule, &'static TypeTable) {
    let fixture = analyze_ok_la(source);
    let crate::SemanticResult {
        hir,
        types,
        access_facts,
        ..
    } = fixture.result;
    let hir: &'static radix_hir::HirModule =
        Box::leak(Box::new(hir.expect("successful analysis produces HIR")));
    let types: &'static TypeTable = Box::leak(Box::new(types));
    let access_facts: &'static AccessFactTable = Box::leak(Box::new(
        access_facts.expect("successful analysis produces an access-fact table"),
    ));
    let function_facts: &'static crate::function_facts::FunctionFactTable =
        Box::leak(Box::new(analyze_function_facts(hir, types)));
    let queries = Box::new(DerivedCapabilityQueries::new(
        types,
        hir,
        access_facts,
        function_facts,
    ));
    (Fixture { queries }, hir, types)
}

fn first_function_def(hir: &radix_hir::HirModule) -> radix_hir::DefId {
    hir.items
        .iter()
        .find(|item| matches!(item.kind, radix_hir::HirItemKind::Function(_)))
        .expect("function item")
        .def_id
}

fn first_local_init_id(hir: &radix_hir::HirModule) -> HirId {
    let entry = hir.entry.as_ref().expect("entry body");
    entry
        .statements
        .iter()
        .find_map(|statement| match &statement.kind {
            radix_hir::HirStatementKind::Local(local) => Some(local),
            _ => None,
        })
        .expect("local statement")
        .init
        .as_ref()
        .expect("initializer")
        .id
}

fn nth_local_init_path_id(hir: &radix_hir::HirModule, index: usize) -> HirId {
    let entry = hir.entry.as_ref().expect("entry body");
    let locals: Vec<_> = entry
        .statements
        .iter()
        .filter_map(|statement| match &statement.kind {
            radix_hir::HirStatementKind::Local(local) => Some(local),
            _ => None,
        })
        .collect();
    locals
        .get(index)
        .unwrap_or_else(|| panic!("local {index}"))
        .init
        .as_ref()
        .expect("initializer")
        .id
}

// ── index capability ─────────────────────────────────────────────────────────

#[test]
fn index_capability_proves_solved_literal_param_and_tuple_shapes() {
    let mut table = TypeTable::new();
    let literal = table.intern_index(IndexExpr::Literal(4));
    let param = table.intern_index(IndexExpr::Param(Symbol(1)));
    let tuple = table.intern_index(IndexExpr::Tuple(vec![literal, param]));
    let queries = table_queries(table);

    for id in [literal, param, tuple] {
        assert!(matches!(
            queries.index_capability(id, IndexCapabilityKind::SolvedIdentity),
            CapabilityAnswer::Proven(IndexCapabilityProof::Solved)
        ));
    }
}

#[test]
fn index_capability_classifies_infer_and_unspecified_fail_closed() {
    let mut table = TypeTable::new();
    let infer = table.intern_index(IndexExpr::Infer(IndexVar(0)));
    let unspecified = table.intern_index(IndexExpr::Unspecified);
    let literal = table.intern_index(IndexExpr::Literal(2));
    let tuple = table.intern_index(IndexExpr::Tuple(vec![literal, infer]));
    let queries = table_queries(table);

    assert_eq!(
        queries
            .index_capability(infer, IndexCapabilityKind::SolvedIdentity)
            .reason(),
        Some(CapabilityReason::UnresolvedInference)
    );
    assert_eq!(
        queries
            .index_capability(unspecified, IndexCapabilityKind::SolvedIdentity)
            .reason(),
        Some(CapabilityReason::UnspecifiedObservableIndex)
    );
    // An unsolved member poisons the whole observable shape.
    assert_eq!(
        queries
            .index_capability(tuple, IndexCapabilityKind::SolvedIdentity)
            .reason(),
        Some(CapabilityReason::UnresolvedInference)
    );
}

// ── type capability: fail-closed classifications ─────────────────────────────

#[test]
fn type_capability_rejects_error_inference_and_generic_parameters() {
    let mut table = TypeTable::new();
    let error = table.intern(Type::Error);
    let infer = table.intern(Type::Infer(radix_types::InferVar(0)));
    let param = table.intern(Type::Param(Symbol(2)));
    let queries = table_queries(table);

    for kind in TypeCapabilityKind::ALL {
        assert_eq!(
            queries.type_capability(error, kind).reason(),
            Some(CapabilityReason::Error)
        );
        assert_eq!(
            queries.type_capability(infer, kind).reason(),
            Some(CapabilityReason::UnresolvedInference)
        );
        assert_eq!(
            queries.type_capability(param, kind).reason(),
            Some(CapabilityReason::UnconstrainedGeneric)
        );
    }
}

// ── type capability: transfer, duplication, and copy laws ────────────────────

#[test]
fn transfer_and_duplication_laws_distinguish_markers_and_containers() {
    let mut table = TypeTable::new();
    let num = table.primitive(Prim::Numerus);
    let list = table.array(num);
    let shared = table.reference(SemanticMutability::Immutable, list);
    let own = table.reference(SemanticMutability::Own, list);
    let queries = table_queries(table);

    // An unmarked value transfers and duplicates explicitly.
    assert!(matches!(
        queries.type_capability(num, TypeCapabilityKind::Transferable),
        CapabilityAnswer::Proven(TypeCapabilityProof::Transferable)
    ));
    // A solved collection duplicates explicitly but not trivially.
    assert!(matches!(
        queries.type_capability(list, TypeCapabilityKind::ExplicitlyDuplicable),
        CapabilityAnswer::Proven(TypeCapabilityProof::ExplicitlyDuplicable)
    ));
    assert_eq!(
        queries
            .type_capability(list, TypeCapabilityKind::TriviallyDuplicable)
            .reason(),
        Some(CapabilityReason::UnsupportedRepresentation)
    );
    // A shared view is trivially duplicable but never transfers ownership.
    assert!(matches!(
        queries.type_capability(shared, TypeCapabilityKind::TriviallyDuplicable),
        CapabilityAnswer::Proven(TypeCapabilityProof::TriviallyDuplicable)
    ));
    assert_eq!(
        queries
            .type_capability(shared, TypeCapabilityKind::Transferable)
            .reason(),
        Some(CapabilityReason::UnsupportedRepresentation)
    );
    // An `own` view is the transferable acquisition.
    assert!(matches!(
        queries.type_capability(own, TypeCapabilityKind::Transferable),
        CapabilityAnswer::Proven(TypeCapabilityProof::Transferable)
    ));
    // Trivial duplication is not proved for own views: the `copy` acquisition
    // is an explicit duplicate-then-own event, distinct from trivial copy.
    assert_eq!(
        queries
            .type_capability(own, TypeCapabilityKind::TriviallyDuplicable)
            .reason(),
        Some(CapabilityReason::UnsupportedRepresentation)
    );
    // A scalar is trivially duplicable.
    assert!(matches!(
        queries.type_capability(num, TypeCapabilityKind::TriviallyDuplicable),
        CapabilityAnswer::Proven(TypeCapabilityProof::TriviallyDuplicable)
    ));
}

#[test]
fn the_unbounded_integer_duplicates_explicitly_never_trivially() {
    // `inf` is a heap bignum: the backends clone it, so the ownership plan
    // must not classify its duplication as a bitwise copy (inf U6r).
    let mut table = TypeTable::new();
    let inf = table.intern(Type::SizedNumeric(Prim::Numerus, NumericWidth::Inf));
    let queries = table_queries(table);

    assert_eq!(
        queries
            .type_capability(inf, TypeCapabilityKind::TriviallyDuplicable)
            .reason(),
        Some(CapabilityReason::UnsupportedRepresentation)
    );
    for kind in [
        TypeCapabilityKind::Transferable,
        TypeCapabilityKind::ExplicitlyDuplicable,
        TypeCapabilityKind::StrictEquality,
        TypeCapabilityKind::ReflexiveEquality,
    ] {
        assert!(
            queries.type_capability(inf, kind).proof().is_some(),
            "{kind:?} should be proven for inf"
        );
    }
}

#[test]
fn option_propagates_trivial_copy_and_structural_equality_but_not_ordering() {
    let mut table = TypeTable::new();
    let num = table.primitive(Prim::Numerus);
    let opt = table.option(num);
    let queries = table_queries(table);

    for kind in [
        TypeCapabilityKind::TriviallyDuplicable,
        TypeCapabilityKind::StrictEquality,
        TypeCapabilityKind::ReflexiveEquality,
    ] {
        assert!(
            queries.type_capability(opt, kind).proof().is_some(),
            "{kind:?} should be proven for option<numerus>"
        );
    }
    // Option ordering is not a settled language law.
    assert_eq!(
        queries
            .type_capability(opt, TypeCapabilityKind::Ordering)
            .reason(),
        Some(CapabilityReason::UnsupportedRepresentation)
    );
}

// ── type capability: equality, reflexivity, hash, ordering ───────────────────

#[test]
fn strict_and_reflexive_equality_stay_distinct_for_floats() {
    let mut table = TypeTable::new();
    let f32v = table.intern(Type::SizedNumeric(Prim::Fractus, NumericWidth::F32));
    let i32v = table.intern(Type::SizedNumeric(Prim::Numerus, NumericWidth::I32));
    let queries = table_queries(table);

    // Float strict equality holds, but it is not reflexive for every value.
    assert!(matches!(
        queries.type_capability(f32v, TypeCapabilityKind::StrictEquality),
        CapabilityAnswer::Proven(TypeCapabilityProof::StrictlyEqual)
    ));
    assert_eq!(
        queries
            .type_capability(f32v, TypeCapabilityKind::ReflexiveEquality)
            .reason(),
        Some(CapabilityReason::UnsupportedRepresentation)
    );
    // Integer widths are reflexive.
    assert!(matches!(
        queries.type_capability(i32v, TypeCapabilityKind::ReflexiveEquality),
        CapabilityAnswer::Proven(TypeCapabilityProof::ReflexivelyEqual)
    ));
}

#[test]
fn canonical_hash_eligibility_follows_the_hashable_key_law() {
    let mut table = TypeTable::new();
    let num = table.primitive(Prim::Numerus);
    let text = table.primitive(Prim::Textus);
    let f32v = table.intern(Type::SizedNumeric(Prim::Fractus, NumericWidth::F32));
    let tuple = table.intern(Type::Tuple(vec![num, text]));
    let queries = table_queries(table);

    assert!(matches!(
        queries.type_capability(num, TypeCapabilityKind::CanonicalHashEligibility),
        CapabilityAnswer::Proven(TypeCapabilityProof::CanonicalHashEligible)
    ));
    assert!(matches!(
        queries.type_capability(tuple, TypeCapabilityKind::CanonicalHashEligibility),
        CapabilityAnswer::Proven(TypeCapabilityProof::CanonicalHashEligible)
    ));
    assert_eq!(
        queries
            .type_capability(f32v, TypeCapabilityKind::CanonicalHashEligibility)
            .reason(),
        Some(CapabilityReason::UnsupportedRepresentation)
    );
}

#[test]
fn ordering_is_proved_for_numeric_scalars_only() {
    let mut table = TypeTable::new();
    let num = table.primitive(Prim::Numerus);
    let text = table.primitive(Prim::Textus);
    let list = table.array(num);
    let queries = table_queries(table);

    assert!(matches!(
        queries.type_capability(num, TypeCapabilityKind::Ordering),
        CapabilityAnswer::Proven(TypeCapabilityProof::Ordered)
    ));
    for ty in [text, list] {
        assert_eq!(
            queries
                .type_capability(ty, TypeCapabilityKind::Ordering)
                .reason(),
            Some(CapabilityReason::UnsupportedRepresentation)
        );
    }
}

// ── type capability: solved indexed identity (T-CAP-08) ─────────────────────

#[test]
fn solved_indexed_identity_records_width_capacity_and_dimensions() {
    let mut table = TypeTable::new();
    let num = table.primitive(Prim::Numerus);
    let four = table.intern_index(IndexExpr::Literal(4));
    let two = table.intern_index(IndexExpr::Literal(2));
    let three = table.intern_index(IndexExpr::Literal(3));
    let shape = table.intern_index(IndexExpr::Tuple(vec![two, three]));
    let vector = table.vector_with_width(num, four);
    let bounded = table.bounded_array(num, four);
    let tensor = table.tensor_with_shape(num, shape);
    let queries = table_queries(table);

    let identity = |ty| match queries.type_capability(ty, TypeCapabilityKind::SolvedIndexedIdentity)
    {
        CapabilityAnswer::Proven(TypeCapabilityProof::SolvedIndexedIdentity(identity)) => identity,
        other => panic!("expected solved indexed identity, got {other:?}"),
    };

    let vector_identity = identity(vector);
    assert_eq!(vector_identity.width, Some(four));
    let bounded_identity = identity(bounded);
    assert_eq!(bounded_identity.capacity, Some(four));
    let tensor_identity = identity(tensor);
    assert_eq!(tensor_identity.dimensions, vec![two, three]);
    assert_eq!(tensor_identity.tuple_order, vec![two, three]);
}

#[test]
fn solved_indexed_identity_fails_closed_for_unspecified_and_inferred_shapes() {
    let mut table = TypeTable::new();
    let num = table.primitive(Prim::Numerus);
    let unspecified = table.intern_index(IndexExpr::Unspecified);
    let infer = table.intern_index(IndexExpr::Infer(IndexVar(3)));
    let inferred_vector = table.vector_with_width(num, infer);
    let _inferred_tensor = table.tensor_with_shape(num, infer);
    let unspecified_tensor = table.tensor_with_shape(num, unspecified);
    let queries = table_queries(table);

    assert_eq!(
        queries
            .type_capability(
                unspecified_tensor,
                TypeCapabilityKind::SolvedIndexedIdentity
            )
            .reason(),
        Some(CapabilityReason::UnspecifiedObservableIndex)
    );
    assert_eq!(
        queries
            .type_capability(inferred_vector, TypeCapabilityKind::SolvedIndexedIdentity)
            .reason(),
        Some(CapabilityReason::UnresolvedInference)
    );
}

#[test]
fn solved_indexed_identity_rejects_conflicting_observable_widths() {
    let mut table = TypeTable::new();
    let num = table.primitive(Prim::Numerus);
    let four = table.intern_index(IndexExpr::Literal(4));
    let five = table.intern_index(IndexExpr::Literal(5));
    let a = table.vector_with_width(num, four);
    let b = table.vector_with_width(num, five);
    let tuple = table.tuple(vec![a, b]);
    let queries = table_queries(table);

    // One summary cannot represent two distinct observable widths; the
    // summary shape itself is unsupported for this type.
    assert_eq!(
        queries
            .type_capability(tuple, TypeCapabilityKind::SolvedIndexedIdentity)
            .reason(),
        Some(CapabilityReason::UnsupportedRepresentation)
    );
}

// ── type capability: nominal declarations ────────────────────────────────────

#[test]
fn nominal_struct_equality_follows_field_laws() {
    let (queries, hir, _types) = source_queries(
        r"
genus Mixtum {
    varia numerus n
    varia lista<numerus> items
}
functio clare(Mixtum m) → numerus {
    redde 1
}
incipit { }
",
    );
    let mixtum = first_param_type(hir);

    // A collection field has no settled equality, so neither has the genus.
    assert_eq!(
        queries
            .type_capability(mixtum, TypeCapabilityKind::StrictEquality)
            .reason(),
        Some(CapabilityReason::UnsupportedRepresentation)
    );
}

#[test]
fn nominal_struct_reflexivity_rejects_float_fields() {
    let (queries, hir, _types) = source_queries(
        r"
genus Vivum {
    varia f32 v
}
functio clare(Vivum v) → numerus {
    redde 1
}
incipit { }
",
    );
    let vivum = first_param_type(hir);

    assert!(
        queries
            .type_capability(vivum, TypeCapabilityKind::StrictEquality)
            .proof()
            .is_some()
    );
    assert_eq!(
        queries
            .type_capability(vivum, TypeCapabilityKind::ReflexiveEquality)
            .reason(),
        Some(CapabilityReason::UnsupportedRepresentation)
    );
}

#[test]
fn recursive_nominal_shape_fails_closed_as_unsupported_recursion() {
    let (queries, hir, _types) = source_queries(
        r"
genus Catena {
    varia de Catena cauda
}
functio pingam(de Catena c) → numerus {
    redde 1
}
incipit { }
",
    );
    let catena = first_param_type(hir);

    for kind in TypeCapabilityKind::ALL {
        assert_eq!(
            queries.type_capability(catena, kind).reason(),
            Some(CapabilityReason::UnsupportedRecursion),
            "{kind:?} must fail closed for a recursive nominal"
        );
    }
}

/// Type of the first parameter of the first function: a real analyzed
/// `TypeId` owned by the snapshot's `TypeTable`.
fn first_param_type(hir: &radix_hir::HirModule) -> TypeId {
    let function = hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            radix_hir::HirItemKind::Function(function) => Some(function),
            _ => None,
        })
        .expect("function");
    function.params.first().expect("parameter").ty
}

// ── function-boundary capability ─────────────────────────────────────────────

#[test]
fn parameter_access_posture_is_proven_in_declaration_order() {
    let (queries, hir, _types) = source_queries(
        r"
functio mistura(numerus plena, de numerus lecta, varia numerus mutata) → numerus {
    redde plena
}
incipit { }
",
    );
    let def = first_function_def(hir);

    match queries.function_capability(def, FunctionCapabilityKind::ParameterAccessPosture) {
        CapabilityAnswer::Proven(FunctionCapabilityProof::ParameterAccess(params)) => {
            assert_eq!(params.len(), 3);
            let postures: Vec<_> = params.iter().map(|p| p.posture).collect();
            assert!(matches!(postures[0], super::ParameterAccessPosture::Owned));
            assert!(matches!(postures[1], super::ParameterAccessPosture::Shared));
            assert!(matches!(
                postures[2],
                super::ParameterAccessPosture::Mutable
            ));
        }
        other => panic!("expected proven parameter access, got {other:?}"),
    }
}

#[test]
fn capture_posture_proves_empty_for_ordinary_functions_and_rows_for_captures() {
    let (queries, hir, _types) = source_queries(
        r"
functio plana() → numerus {
    redde 1
}
functio facit() → numerus {
    sit _ duplo ← (numerus value) → numerus ∴ value + plana()
    redde duplo(3)
}
incipit { }
",
    );
    let defs: Vec<_> = hir
        .items
        .iter()
        .filter(|item| matches!(item.kind, radix_hir::HirItemKind::Function(_)))
        .map(|item| item.def_id)
        .collect();
    let (plana, facit) = (defs[0], defs[1]);

    match queries.function_capability(facit, FunctionCapabilityKind::CapturePosture) {
        CapabilityAnswer::Proven(FunctionCapabilityProof::Capture(captures)) => {
            assert_eq!(captures.len(), 1, "the plana definition is captured");
            assert_eq!(captures[0].definition, plana);
            assert!(matches!(captures[0].posture, super::CapturePosture::Shared));
        }
        other => panic!("expected proven captures, got {other:?}"),
    }
    match queries.function_capability(plana, FunctionCapabilityKind::CapturePosture) {
        CapabilityAnswer::Proven(FunctionCapabilityProof::Capture(captures)) => {
            assert_eq!(captures, []);
        }
        other => panic!("expected proven empty captures, got {other:?}"),
    }
}

#[test]
fn effects_posture_consumes_failable_async_and_alternate_exit_facts() {
    let (queries, hir, _types) = source_queries(
        r#"
functio cadit() → vacuum ⇥ textus {
    iace "nope"
}
@ futura
functio laborat() → numerus {
    redde 1
}
functio stabilis() → numerus {
    redde 2
}
incipit { }
"#,
    );
    let defs: Vec<_> = hir
        .items
        .iter()
        .filter(|item| matches!(item.kind, radix_hir::HirItemKind::Function(_)))
        .map(|item| item.def_id)
        .collect();
    let (cadit, laborat, stabilis) = (defs[0], defs[1], defs[2]);

    let effects = |def| match queries
        .function_capability(def, FunctionCapabilityKind::EffectsAndAlternateExit)
    {
        CapabilityAnswer::Proven(FunctionCapabilityProof::Effects(posture)) => posture,
        other => panic!("expected proven effects, got {other:?}"),
    };

    let failing = effects(cadit);
    assert!(failing.may_fail);
    assert!(failing.has_alternate_exit);
    assert!(!failing.may_suspend);

    let asynchronous = effects(laborat);
    assert!(asynchronous.may_suspend);
    assert!(!asynchronous.may_fail);

    let plain = effects(stabilis);
    assert!(!plain.may_fail && !plain.may_suspend && !plain.is_generator);
}

#[test]
fn exported_visibility_consumes_the_supplied_tier_view_and_fails_closed_without_it() {
    let (queries_raw, hir, types) = source_queries(
        r"
functio aperta() → numerus {
    redde 1
}
incipit { }
",
    );
    let def = first_function_def(hir);
    assert_eq!(
        queries_raw
            .function_capability(def, FunctionCapabilityKind::ExportedVisibility)
            .reason(),
        Some(CapabilityReason::MissingFact),
        "no visibility view attached: fail closed"
    );
    // A second query layer over the same authorities with a visibility view;
    // memoized answers must not leak across layers.
    let access = crate::analyze_access_facts_with_types(hir, types);
    let functions = analyze_function_facts(hir, types);
    let attached = DerivedCapabilityQueries::new(
        types,
        hir,
        Box::leak(Box::new(access)),
        Box::leak(Box::new(functions)),
    )
    .with_visibility(std::iter::once((
        def,
        crate::passes::visibility_annotation::VisibilityTier::Publica,
    )));
    assert!(matches!(
        attached.function_capability(def, FunctionCapabilityKind::ExportedVisibility),
        CapabilityAnswer::Proven(FunctionCapabilityProof::ExportedVisibility(
            super::ExportVisibility::Public
        ))
    ));
}

#[test]
fn region_and_provenance_rows_fail_closed_without_a_durable_summary() {
    let (queries, hir, _types) = source_queries(
        r"
functio f() → numerus {
    redde 1
}
incipit { }
",
    );
    let def = first_function_def(hir);

    for kind in [
        FunctionCapabilityKind::ReturnedBorrowProvenance,
        FunctionCapabilityKind::RequiredRegionRelationships,
    ] {
        assert_eq!(
            queries.function_capability(def, kind).reason(),
            Some(CapabilityReason::MissingFact)
        );
    }
}

#[test]
fn durable_provenance_answers_the_region_rows_through_the_application() {
    // SCR-04: the region rows answer from the durable summary carried by
    // AnalyzedApplication — never by re-running region inference.
    let lex = radix_lexer::lex_with_locale_pack(
        r"
functio eadem(de lista<numerus> xs) → de lista<numerus> {
    redde xs
}
incipit { }
",
        &la_pack(),
    );
    let mut parsed = radix_parser::parse(lex);
    assert!(parsed.success(), "parse: {:?}", parsed.errors);
    let program = parsed.program.take().expect("program");
    let result = crate::analyze(&program, &crate::PassConfig::base(), &mut parsed.interner);
    assert!(result.is_ok(), "analyze: {:?}", result.diagnostics);
    let parts = crate::AnalyzedApplicationParts {
        hir: result.hir.expect("hir"),
        types: result.types,
        resolver: result.resolver,
        annotation_contracts: result.annotation_contracts,
        libraries: radix_hir::LibraryRegistry::default(),
        radix_lanes: result.radix_lanes,
        gpu_builtins: result.gpu_builtins,
        tensor_edge_policies: result.tensor_edge_policies,
        kernel_requirements: result.kernel_requirements,
        access_facts: result.access_facts.expect("access facts"),
        region_facts: result.region_facts,
        semantic_diagnostics: result.diagnostics,
    };
    let app = crate::AnalyzedApplication::build(parts, &parsed.interner).expect("build");
    let def = app
        .hir()
        .items
        .iter()
        .find(|item| matches!(item.kind, radix_hir::HirItemKind::Function(_)))
        .expect("function item")
        .def_id;
    let param = match &app.hir().items[0].kind {
        radix_hir::HirItemKind::Function(function) => function.params[0].def_id,
        _ => panic!("function item"),
    };

    let queries = DerivedCapabilityQueries::from_application(&app);
    assert!(matches!(
        queries.function_capability(def, FunctionCapabilityKind::ReturnedBorrowProvenance),
        CapabilityAnswer::Proven(FunctionCapabilityProof::ReturnedBorrow(
            super::ReturnedBorrowProvenance::Parameter(p)
        )) if p == param
    ));
    assert!(matches!(
        queries.function_capability(def, FunctionCapabilityKind::RequiredRegionRelationships),
        CapabilityAnswer::Proven(FunctionCapabilityProof::RequiredRegions(
            super::RequiredRegionRelationships { relationships }
        )) if relationships.is_empty()
    ));
}

#[test]
fn function_query_without_a_fact_row_fails_closed() {
    let (queries, _hir, _types) = source_queries("incipit { }");
    let unknown = DefId(9_999);
    for kind in FunctionCapabilityKind::ALL {
        assert_eq!(
            queries.function_capability(unknown, kind).reason(),
            Some(CapabilityReason::MissingFact)
        );
    }
}

// ── place/value capability ───────────────────────────────────────────────────

#[test]
fn own_transfer_site_proves_move_and_dead_liveness() {
    let (queries, hir, _types) = source_queries(
        r"
incipit {
    sit lista<numerus> source ← own [1, 2, 3]
    sit lista<numerus> destination ← own source
}
",
    );
    let transfer_site = nth_local_init_path_id(hir, 1);

    assert!(matches!(
        queries.place_capability(transfer_site, PlaceCapabilityKind::Movable),
        CapabilityAnswer::Proven(PlaceCapabilityProof::Movable)
    ));
    assert!(matches!(
        queries.place_capability(transfer_site, PlaceCapabilityKind::LivenessAtUse),
        CapabilityAnswer::Proven(PlaceCapabilityProof::Liveness(Liveness::Dead))
    ));
}

#[test]
fn plain_scalar_use_proves_read_move_and_live_liveness() {
    let (queries, hir, _types) = source_queries(
        r"
incipit {
    sit numerus x ← 5
    sit numerus y ← x
}
",
    );
    let use_site = nth_local_init_path_id(hir, 1);

    for kind in [
        PlaceCapabilityKind::Readable,
        PlaceCapabilityKind::Movable,
        PlaceCapabilityKind::SharedBorrowable,
    ] {
        assert!(
            queries.place_capability(use_site, kind).proof().is_some(),
            "{kind:?} should be proven for a plain scalar read"
        );
    }
    assert!(matches!(
        queries.place_capability(use_site, PlaceCapabilityKind::LivenessAtUse),
        CapabilityAnswer::Proven(PlaceCapabilityProof::Liveness(Liveness::Live))
    ));
    // A scalar read is not a writable or mutable place.
    assert_eq!(
        queries
            .place_capability(use_site, PlaceCapabilityKind::Writable)
            .reason(),
        Some(CapabilityReason::UnsupportedRepresentation)
    );
    // A scalar destination duplicates by trivial copy.
    assert!(matches!(
        queries.place_capability(use_site, PlaceCapabilityKind::DuplicateThenOwn),
        CapabilityAnswer::Proven(PlaceCapabilityProof::DuplicateThenOwn)
    ));
}

#[test]
fn mut_marker_param_use_proves_mutable_place_laws() {
    let (queries, hir, _types) = source_queries(
        r"
functio mutat(varia numerus cella) → numerus {
    redde cella
}
incipit { }
",
    );
    // Find the `cella` path use inside the function body.
    let function = hir
        .items
        .iter()
        .find_map(|item| match &item.kind {
            radix_hir::HirItemKind::Function(function) => Some(function),
            _ => None,
        })
        .expect("function");
    let body = function.body.as_ref().expect("body");
    let use_expr = body
        .statements
        .iter()
        .find_map(|statement| match &statement.kind {
            radix_hir::HirStatementKind::Redde(Some(value)) => Some(value),
            _ => None,
        })
        .expect("redde value expression");
    let radix_hir::HirExpressionKind::Path(_) = use_expr.kind else {
        panic!("expected a path use of cella");
    };
    let use_site = use_expr.id;

    for kind in [
        PlaceCapabilityKind::Writable,
        PlaceCapabilityKind::Mutable,
        PlaceCapabilityKind::MutablyBorrowable,
        PlaceCapabilityKind::Readable,
    ] {
        assert!(
            queries.place_capability(use_site, kind).proof().is_some(),
            "{kind:?} should be proven at a mut-marker use"
        );
    }
}

#[test]
fn non_path_and_unknown_place_queries_fail_closed() {
    let (queries, hir, _types) = source_queries(
        r"
incipit {
    sit numerus x ← 1 + 2
}
",
    );
    let init_id = first_local_init_id(hir);
    for kind in PlaceCapabilityKind::ALL {
        assert_eq!(
            queries.place_capability(init_id, kind).reason(),
            Some(CapabilityReason::MissingFact),
            "{kind:?}: a literal initializer is not a place use"
        );
    }
    for kind in PlaceCapabilityKind::ALL {
        assert_eq!(
            queries.place_capability(HirId(98_765), kind).reason(),
            Some(CapabilityReason::MissingFact)
        );
    }
}

// ── snapshot construction ────────────────────────────────────────────────────

#[test]
fn derived_queries_implement_the_combined_interface_and_attach_regions() {
    fn accepts_combined<T: CapabilityQueries>(_: &T) {}

    let (queries, hir, types) = source_queries("incipit { sit numerus x ← 1 }");
    accepts_combined(&*queries.queries);
    assert!(!queries.has_regions());

    let access = analyze_access_facts_table(hir, types);
    let regions = crate::passes::borrow_region::analyze(hir, types, &access)
        .expect("region analysis succeeds");
    let with_regions = queries.queries.with_regions(Box::leak(Box::new(regions)));
    assert!(with_regions.has_regions());
}

fn analyze_access_facts_table(hir: &radix_hir::HirModule, types: &TypeTable) -> AccessFactTable {
    crate::analyze_access_facts_with_types(hir, types)
}
