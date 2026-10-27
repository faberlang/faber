//! Explicit size arguments in a generic call carry the `size` marker.
//!
//! The list binds by marker: a `size` argument fills the next `size`
//! parameter, an unmarked argument the next type parameter. The marker is
//! required, the way `axis` is, and a whole shape is one size argument.

use radix_types::{
    GenericBindingSource, GenericWitness, IndexExpr, Primitive, ResolvedGenericBinding,
};

use crate::test_support::AnalyzedFixture;

const MK: &str = r"functio mk<T, magnitudo N>(lista<T> xs) → tensor<T, [N]> ⇥ textus {
    redde xs ↦ tensor<T, [N]>
}

functio shaped<T, magnitudo S>(lista<T> data) → tensor<T, S> ⇥ textus {
    redde data ↦ tensor<T, S>
}
";

fn compile(body: &str) -> AnalyzedFixture {
    crate::test_support::analyze_source_la(&format!("{MK}\n{body}"))
}

fn accepts(fixture: &AnalyzedFixture) {
    assert!(
        fixture.is_ok(),
        "diagnostics: {:?}",
        fixture.result.diagnostics
    );
}

fn rejects_with_issue(fixture: &AnalyzedFixture, issue: &str) {
    assert!(
        fixture.is_err(),
        "expected failure, got: {:?}",
        fixture.result.diagnostics
    );
    assert!(
        fixture.result.diagnostics.iter().any(|diag| diag
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == issue)),
        "expected diagnostic issue {issue:?}, got: {:?}",
        fixture.result.diagnostics
    );
}

fn binding_named<'a>(fixture: &'a AnalyzedFixture, name: &str) -> &'a ResolvedGenericBinding {
    fixture
        .result
        .types
        .resolved_generic_calls
        .iter()
        .flat_map(|row| row.bindings.iter())
        .find(|binding| fixture.interner.resolve(binding.name) == name)
        .unwrap_or_else(|| {
            panic!(
                "no resolved binding for {name}: {:?}",
                fixture.result.types.resolved_generic_calls
            )
        })
}

fn assert_type_witness(
    fixture: &AnalyzedFixture,
    binding: &ResolvedGenericBinding,
    primitive: Primitive,
) {
    let GenericWitness::Typus(ty) = binding.witness else {
        panic!("expected a type witness, got {:?}", binding.witness);
    };
    assert!(
        fixture
            .result
            .types
            .equals(ty, fixture.result.types.primitive(primitive)),
        "expected {primitive:?} witness, got {:?}",
        fixture.result.types.get(ty)
    );
}

fn assert_size_witness(fixture: &AnalyzedFixture, binding: &ResolvedGenericBinding, value: u64) {
    let GenericWitness::Magnitudo(id) = binding.witness else {
        panic!("expected a size witness, got {:?}", binding.witness);
    };
    assert!(
        matches!(fixture.result.types.get_index(id), IndexExpr::Literal(actual) if *actual == value),
        "expected literal {value}, got {:?}",
        fixture.result.types.get_index(id)
    );
}

fn assert_shape_witness(fixture: &AnalyzedFixture, binding: &ResolvedGenericBinding, dims: &[u64]) {
    let GenericWitness::Magnitudo(id) = binding.witness else {
        panic!("expected a size witness, got {:?}", binding.witness);
    };
    let IndexExpr::Tuple(parts) = fixture.result.types.get_index(id) else {
        panic!(
            "expected a shape witness, got {:?}",
            fixture.result.types.get_index(id)
        );
    };
    let actual: Vec<u64> = parts
        .iter()
        .map(|part| match fixture.result.types.get_index(*part) {
            IndexExpr::Literal(value) => *value,
            other => panic!("expected a literal dimension, got {other:?}"),
        })
        .collect();
    assert_eq!(actual, dims);
}

#[test]
fn mixed_list_binds_the_type_then_the_marked_size() {
    let fixture = compile(
        r"incipit {
    sit lista<numerus> xs ← [1, 2, 3, 4]
    sit tensor<numerus, [4]> t ← mk<numerus, magnitudo 4>(xs)
}",
    );
    accepts(&fixture);
    let element = binding_named(&fixture, "T");
    assert_eq!(element.source, GenericBindingSource::Explicit);
    assert_type_witness(&fixture, element, Primitive::Numerus);
    let size = binding_named(&fixture, "N");
    assert_eq!(size.source, GenericBindingSource::Explicit);
    assert_size_witness(&fixture, size, 4);
}

#[test]
fn the_marker_decides_the_slot_not_the_position() {
    let fixture = compile(
        r"incipit {
    sit lista<numerus> xs ← [1, 2, 3, 4]
    sit tensor<numerus, [4]> t ← mk<magnitudo 4, numerus>(xs)
}",
    );
    accepts(&fixture);
    assert_type_witness(&fixture, binding_named(&fixture, "T"), Primitive::Numerus);
    assert_size_witness(&fixture, binding_named(&fixture, "N"), 4);
}

#[test]
fn a_shape_only_list_leaves_the_type_to_inference() {
    let fixture = compile(
        r"incipit {
    sit lista<numerus> xs ← [1, 2, 3, 4]
    sit tensor<numerus, [4]> t ← mk<magnitudo 4>(xs)
}",
    );
    accepts(&fixture);
    let element = binding_named(&fixture, "T");
    assert_eq!(element.source, GenericBindingSource::Inferred);
    assert_type_witness(&fixture, element, Primitive::Numerus);
    let size = binding_named(&fixture, "N");
    assert_eq!(size.source, GenericBindingSource::Explicit);
    assert_size_witness(&fixture, size, 4);
}

#[test]
fn a_whole_shape_is_one_size_argument() {
    let fixture = compile(
        r"incipit {
    sit lista<numerus> xs ← [1, 2, 3, 4, 5, 6]
    sit tensor<numerus, [2, 3]> t ← shaped<numerus, magnitudo [2, 3]>(xs)
}",
    );
    accepts(&fixture);
    assert_shape_witness(&fixture, binding_named(&fixture, "S"), &[2, 3]);
}

#[test]
fn a_whole_shape_fills_an_argument_position_with_no_annotation() {
    let fixture = compile(
        r"functio total(tensor<numerus, [4]> cells) → numerus {
    redde 0
}

incipit {
    sit lista<numerus> xs ← [1, 2, 3, 4]
    sit _ n ← total(shaped<numerus, magnitudo [4]>(xs))
}",
    );
    accepts(&fixture);
    assert_shape_witness(&fixture, binding_named(&fixture, "S"), &[4]);
}

#[test]
fn a_shape_that_agrees_with_the_store_is_legal() {
    let fixture = compile(
        r"incipit {
    sit lista<numerus> xs ← [1, 2, 3, 4]
    sit tensor<numerus, [4]> t ← shaped<numerus, magnitudo [4]>(xs)
}",
    );
    accepts(&fixture);
}

#[test]
fn a_shape_that_conflicts_with_the_store_is_an_incompatible_tensor_index() {
    let fixture = compile(
        r"incipit {
    sit lista<numerus> xs ← [1, 2, 3, 4]
    sit tensor<numerus, [5]> t ← shaped<numerus, magnitudo [4]>(xs)
}",
    );
    rejects_with_issue(&fixture, "incompatible_tensor_index");
}

#[test]
fn a_scalar_size_that_conflicts_with_the_store_is_the_same_error() {
    let fixture = compile(
        r"incipit {
    sit lista<numerus> xs ← [1, 2, 3, 4]
    sit tensor<numerus, [5]> t ← mk<numerus, magnitudo 4>(xs)
}",
    );
    rejects_with_issue(&fixture, "incompatible_tensor_index");
}

#[test]
fn a_bare_natural_in_the_list_requires_the_marker() {
    let fixture = compile(
        r"incipit {
    sit lista<numerus> xs ← [1, 2, 3, 4]
    sit tensor<numerus, [4]> t ← mk<numerus, 4>(xs)
}",
    );
    rejects_with_issue(&fixture, "size_marker_required");
}

#[test]
fn a_bare_shape_in_the_list_requires_the_marker() {
    let fixture = compile(
        r"incipit {
    sit lista<numerus> xs ← [1, 2, 3, 4]
    sit tensor<numerus, [4]> t ← shaped<numerus, [4]>(xs)
}",
    );
    rejects_with_issue(&fixture, "size_marker_required");
}

#[test]
fn a_bare_size_param_name_forwarded_requires_the_marker() {
    let fixture = compile(
        r"functio forward<magnitudo D>(lista<numerus> xs) → tensor<numerus, [D]> ⇥ textus {
    redde mk<numerus, D>(xs)
}",
    );
    rejects_with_issue(&fixture, "size_marker_required");
}

#[test]
fn a_marked_size_param_name_forwards_the_callers_size() {
    let fixture = compile(
        r"functio forward<magnitudo D>(lista<numerus> xs) → tensor<numerus, [D]> ⇥ textus {
    redde mk<numerus, magnitudo D>(xs)
}",
    );
    accepts(&fixture);
}

#[test]
fn the_marker_on_a_callee_without_size_params_is_named() {
    let fixture = compile(
        r"functio ident<T>(T x) → T {
    redde x
}

incipit {
    sit _ y ← ident<numerus, magnitudo 4>(1)
}",
    );
    assert!(
        fixture.is_err(),
        "diagnostics: {:?}",
        fixture.result.diagnostics
    );
    rejects_with_issue(&fixture, "size_marker_unexpected");
}

#[test]
fn more_marked_sizes_than_size_params_is_an_arity_error() {
    let fixture = compile(
        r"incipit {
    sit lista<numerus> xs ← [1, 2, 3, 4]
    sit tensor<numerus, [4]> t ← mk<numerus, magnitudo 4, magnitudo 4>(xs)
}",
    );
    rejects_with_issue(&fixture, "wrong_number_of_type_arguments");
}

#[test]
fn a_run_time_dimension_inside_an_explicit_shape_is_refused_by_name() {
    let fixture = compile(
        r"incipit {
    sit lista<numerus> xs ← [1, 2, 3, 4]
    sit numerus n ← 4
    sit _ t ← shaped<numerus, magnitudo [n]>(xs)
}",
    );
    rejects_with_issue(&fixture, "size_shape_dimension_unsupported");
}

#[test]
fn a_hole_inside_an_explicit_shape_is_refused_by_name() {
    let fixture = compile(
        r"incipit {
    sit lista<numerus> xs ← [1, 2, 3, 4]
    sit _ t ← shaped<numerus, magnitudo [_]>(xs)
}",
    );
    rejects_with_issue(&fixture, "size_shape_dimension_unsupported");
}

#[test]
fn a_shape_outside_a_call_list_is_refused_by_name() {
    let fixture = crate::test_support::analyze_source_la(
        r"incipit {
    sit tensor<numerus, [magnitudo [2, 3]]> t ← vacua
}",
    );
    assert!(
        fixture.is_err(),
        "diagnostics: {:?}",
        fixture.result.diagnostics
    );
}
