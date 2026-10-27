//! GPD-3: kind-aware call binder with ordered precedence.
//!
//! Explicit prefix, then inference, then the declared `=` default, then a
//! structured unresolved diagnostic. Every checked generic call records its
//! complete ordered resolved vector on the type table.

use radix_types::{
    GenericBindingSource, GenericWitness, IndexExpr, Primitive, ResolvedGenericBinding,
};

use crate::test_support::AnalyzedFixture;

fn compile(source: &str) -> AnalyzedFixture {
    crate::test_support::analyze_source_la(source)
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

/// The recorded binding for `name` in the (single) resolved vector whose
/// declaration order puts `name` first — fixtures keep one generic call.
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

#[test]
fn explicit_type_arg_beats_declared_default() {
    let fixture = compile(
        r"functio pick<T = textus>(T x) → T {
    redde x
}

incipit {
    sit _ n ← pick<numerus>(1)
}",
    );
    accepts(&fixture);
    let binding = binding_named(&fixture, "T");
    assert_eq!(binding.source, GenericBindingSource::Explicit);
    assert_type_witness(&fixture, binding, Primitive::Numerus);
}

#[test]
fn inferred_witness_beats_declared_default() {
    let fixture = compile(
        r"functio pick<T = textus>(T x) → T {
    redde x
}

incipit {
    sit _ n ← pick(1)
}",
    );
    accepts(&fixture);
    let binding = binding_named(&fixture, "T");
    assert_eq!(binding.source, GenericBindingSource::Inferred);
    assert_type_witness(&fixture, binding, Primitive::Numerus);
}

#[test]
fn unconstrained_type_param_uses_declared_default() {
    let fixture = compile(
        r#"functio fallback<T = textus>() → textus {
    redde "salve"
}

incipit {
    nota fallback()
}"#,
    );
    accepts(&fixture);
    let binding = binding_named(&fixture, "T");
    assert_eq!(binding.source, GenericBindingSource::Defaulted);
    assert_type_witness(&fixture, binding, Primitive::Textus);
}

#[test]
fn size_param_absent_from_all_shapes_still_defaults() {
    let fixture = compile(
        r"functio sized<magnitudo D = 4>(numerus x) → numerus {
    redde x
}

incipit {
    sit _ n ← sized(7)
}",
    );
    accepts(&fixture);
    let binding = binding_named(&fixture, "D");
    assert_eq!(binding.source, GenericBindingSource::Defaulted);
    assert_size_witness(&fixture, binding, 4);
}

#[test]
fn partial_type_prefix_defaults_trailing_suffix() {
    let fixture = compile(
        r"functio duo<T, U = textus>(T a) → T {
    redde a
}

incipit {
    sit _ n ← duo<numerus>(1)
}",
    );
    accepts(&fixture);
    let explicit = binding_named(&fixture, "T");
    assert_eq!(explicit.source, GenericBindingSource::Explicit);
    assert_type_witness(&fixture, explicit, Primitive::Numerus);
    let defaulted = binding_named(&fixture, "U");
    assert_eq!(defaulted.source, GenericBindingSource::Defaulted);
    assert_type_witness(&fixture, defaulted, Primitive::Textus);
}

#[test]
fn partial_size_prefix_defaults_trailing_suffix() {
    let fixture = compile(
        r"functio tag<magnitudo M, magnitudo N = 2>(numerus x) → numerus {
    redde x
}

incipit {
    sit _ n ← tag<magnitudo 9>(1)
}",
    );
    accepts(&fixture);
    let explicit = binding_named(&fixture, "M");
    assert_eq!(explicit.source, GenericBindingSource::Explicit);
    assert_size_witness(&fixture, explicit, 9);
    let defaulted = binding_named(&fixture, "N");
    assert_eq!(defaulted.source, GenericBindingSource::Defaulted);
    assert_size_witness(&fixture, defaulted, 2);
}

#[test]
fn mixed_sort_vector_records_defaults_for_both_kinds() {
    let fixture = compile(
        r"functio trio<T = textus, magnitudo D = 8>(numerus x) → numerus {
    redde x
}

incipit {
    sit _ n ← trio(5)
}",
    );
    accepts(&fixture);
    let typus = binding_named(&fixture, "T");
    assert_eq!(typus.source, GenericBindingSource::Defaulted);
    assert_type_witness(&fixture, typus, Primitive::Textus);
    let magnitudo = binding_named(&fixture, "D");
    assert_eq!(magnitudo.source, GenericBindingSource::Defaulted);
    assert_size_witness(&fixture, magnitudo, 8);
}

#[test]
fn unresolved_size_param_without_default_still_rejected() {
    let fixture = compile(
        r"functio need<magnitudo D>(numerus x) → numerus {
    redde x
}

incipit {
    sit _ n ← need(1)
}",
    );
    rejects_with_issue(&fixture, "cannot_infer_tensor_shape");
}

#[test]
fn unresolved_type_param_without_default_keeps_binding_incomplete() {
    let fixture = compile(
        r"functio open<T>() → T {
    redde vacua
}

incipit {
    nota open()
}",
    );
    // No explicit arg, no inference witness, no default: the slot stays
    // unresolved, so no complete resolved vector is recorded for the call.
    assert!(
        fixture
            .result
            .types
            .resolved_generic_calls
            .iter()
            .all(|row| row
                .bindings
                .iter()
                .all(|binding| { fixture.interner.resolve(binding.name) != "T" }))
    );
}

#[test]
fn size_arg_on_typus_only_callee_rejects_unexpected_marker() {
    let fixture = compile(
        r#"functio id<T>(T x) → T {
    redde x
}

incipit {
    sit _ x ← id<magnitudo 64>("salve")
}"#,
    );
    rejects_with_issue(&fixture, "size_marker_unexpected");
}

#[test]
fn bare_natural_in_explicit_list_requires_the_size_marker() {
    let fixture = compile(
        r#"functio id<T>(T x) → T {
    redde x
}

incipit {
    sit _ x ← id<64>("salve")
}"#,
    );
    rejects_with_issue(&fixture, "size_marker_required");
}

#[test]
fn bare_natural_beside_defaulted_size_requires_the_size_marker() {
    let fixture = compile(
        r"functio duo<T, magnitudo D = 4>(numerus x) → numerus {
    redde x
}

incipit {
    sit _ n ← duo<numerus, 7>(1)
}",
    );
    rejects_with_issue(&fixture, "size_marker_required");
}

#[test]
fn marked_size_after_type_binds_the_defaulted_size_param() {
    let fixture = compile(
        r"functio duo<T, magnitudo D = 4>(numerus x) → numerus {
    redde x
}

incipit {
    sit _ n ← duo<numerus, magnitudo 7>(1)
}",
    );
    accepts(&fixture);
    let explicit = binding_named(&fixture, "T");
    assert_eq!(explicit.source, GenericBindingSource::Explicit);
    let size = binding_named(&fixture, "D");
    assert_eq!(size.source, GenericBindingSource::Explicit);
    assert_size_witness(&fixture, size, 7);
}

#[test]
fn too_many_explicit_type_args_rejected() {
    let fixture = compile(
        r"functio one<T>(T x) → T {
    redde x
}

incipit {
    sit _ x ← one<numerus, textus>(1)
}",
    );
    rejects_with_issue(&fixture, "wrong_number_of_type_arguments");
}

#[test]
fn prefix_shorter_than_required_params_rejected() {
    let fixture = compile(
        r"functio duo<T, U>(T a, U b) → T {
    redde a
}

incipit {
    sit _ n ← duo<numerus>(1, 2)
}",
    );
    rejects_with_issue(&fixture, "wrong_number_of_type_arguments");
}

#[test]
fn explicit_witness_conflicting_with_argument_still_errors() {
    let fixture = compile(
        r#"functio id<T>(T x) → T {
    redde x
}

incipit {
    sit _ x ← id<numerus>("salve")
}"#,
    );
    rejects_with_issue(&fixture, "argument_type_mismatch");
}
