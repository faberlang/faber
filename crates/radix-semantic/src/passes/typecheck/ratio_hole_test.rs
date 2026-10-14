//! RR-3 — ratio element-hole solving and union-hole rejection.

use crate::passes::typecheck::test_support;
use crate::{NumericWidth, Primitive, Type};

#[test]
fn ratio_element_hole_solves_from_constructor_witness() {
    let result = test_support::analyze_source_la(
        r"
incipit {
    fixum ratio<g: _, w: numerus> r ← ratio<g: f32, w: numerus> { g = 1.0, w = 2 }
    nota r.g + 0.0
}
",
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);

    let local_ty = test_support::entry_local_tys(&result)
        .into_iter()
        .next()
        .expect("r local");
    let Type::Record(fields) = result.types.get(local_ty) else {
        panic!(
            "expected record local type, got {:?}",
            result.types.get(local_ty)
        );
    };
    assert_eq!(fields.len(), 2);
    let mut saw_f32 = false;
    let mut saw_numerus = false;
    for field_ty in fields.values() {
        match result.types.get(*field_ty) {
            Type::SizedNumeric(Primitive::Fractus, NumericWidth::F32) => saw_f32 = true,
            Type::Primitive(Primitive::Numerus) => saw_numerus = true,
            other => panic!("unexpected field type {other:?}"),
        }
    }
    assert!(saw_f32, "hole should solve to f32: {fields:?}");
    assert!(saw_numerus, "w should remain numerus: {fields:?}");
}

#[test]
fn ratio_element_hole_solves_from_callee_return_witness() {
    let result = test_support::analyze_source_la(
        r"
functio make_ratio() → ratio<g: f32> {
    redde ratio<g: f32> { g = 1.0 }
}

incipit {
    fixum ratio<g: _> r ← make_ratio()
    nota r.g + 0.0
}
",
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}

#[test]
fn ratio_union_hole_is_rejected_with_targeted_issue() {
    test_support::assert_rejects_with_issue_la(
        r"
incipit {
    fixum ratio<g: ∪> r ← ratio<g: f32> { g = 1.0 }
}
",
        "ratio_union_type_arg_unsupported",
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
fn ratio_binary_union_element_remains_legal() {
    let result = test_support::analyze_source_la(
        r#"
incipit {
    fixum ratio<g: textus ∪ nihil> r ← ratio<g: textus ∪ nihil> { g = "x" }
    nota r.g
}
"#,
    );
    assert!(result.is_ok(), "diagnostics: {:?}", result.diagnostics);
}
