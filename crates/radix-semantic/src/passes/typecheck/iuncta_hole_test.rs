//! IL-3 — iuncta element-hole solving and union-hole rejection.

use crate::passes::typecheck::test_support;
use crate::{Primitive, Type};

#[test]
fn iuncta_element_hole_solves_from_constructor_witness() {
    let result = test_support::analyze_source_la(
        r"
incipit {
    fixum iuncta<f32, _> pair ← iuncta<f32, numerus> [1.0, 2]
    nota pair[1] + 0
}
",
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);

    let local_ty = test_support::entry_local_tys(&result)
        .into_iter()
        .next()
        .expect("pair local");
    let Type::Tuple(elements) = result.types.get(local_ty) else {
        panic!(
            "expected tuple local type, got {:?}",
            result.types.get(local_ty)
        );
    };
    assert_eq!(elements.len(), 2);
    assert!(matches!(
        result.types.get(elements[1]),
        Type::Primitive(Primitive::Numerus)
    ));
}

#[test]
fn iuncta_element_hole_solves_from_callee_return_witness() {
    let result = test_support::analyze_source_la(
        r"
functio make_pair() → iuncta<f32, numerus> {
    redde iuncta<f32, numerus> [1.0, 2]
}

incipit {
    fixum iuncta<f32, _> pair ← make_pair()
    nota pair[1] + 0
}
",
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn iuncta_union_type_arg_is_rejected_with_targeted_issue() {
    test_support::assert_rejects_with_issue_la(
        r"
incipit {
    fixum iuncta<f32, ∪> pair ← iuncta<f32, numerus> [1.0, 2]
}
",
        "iuncta_union_type_arg_unsupported",
    );
}

#[test]
fn lista_union_hole_keeps_existing_warn017_behavior() {
    let diagnostics = test_support::analyze_and_get_diagnostics_la(
        r"
incipit {
    fixum lista<∪> values ← [1, 2]
    nota values[0]
}
",
    );
    assert!(
        diagnostics.iter().all(|diagnostic| !diagnostic.is_error()),
        "lista<∪> must remain legal: {diagnostics:?}"
    );
    assert_eq!(
        diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.issue() == Some("union_hole_solved_monomorphically"))
            .count(),
        1,
        "lista<∪> keeps WARN017: {diagnostics:?}"
    );
}

#[test]
fn iuncta_binary_union_element_remains_legal() {
    let result = test_support::analyze_source_la(
        r#"
incipit {
    fixum iuncta<f32, textus ∪ nihil> pair ← iuncta<f32, textus ∪ nihil> [1.0, "x"]
    nota pair[1]
}
"#,
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn iuncta_labeled_element_hole_remains_element_wise() {
    let result = test_support::analyze_source_la(
        r"
incipit {
    fixum iuncta<loss: _, numerus> pair ← iuncta<f32, numerus> [1.0, 2]
    nota pair[0]
}
",
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}
