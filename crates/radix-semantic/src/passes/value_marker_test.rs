//! Value-side `copy`/`own` markers (operator ruling 2026-10-08): the marked
//! initializer has the effect the binding-side spelling has on the declaration.

use crate::test_support::{analyze_ok_la, assert_rejects_with_issue_la, entry_local_tys};
use crate::{ISSUE_SCALAR_ACCESS_MARKER, SemanticMutability, Type};

#[test]
fn value_marker_copy_types_the_binding_as_a_copy_value() {
    let result = analyze_ok_la(
        r"
incipit {
    sit lista<numerus> a ← [1, 2, 3]
    sit lista<numerus> b ← copy a
    nota a
    nota b
}
",
    );
    let tys = entry_local_tys(&result);
    assert!(matches!(
        result.types.get(tys[1]),
        Type::Ref(SemanticMutability::Copy, _)
    ));
}

#[test]
fn value_marker_own_types_the_binding_as_an_owned_value() {
    let result = analyze_ok_la(
        r"
incipit {
    sit lista<numerus> a ← own [1, 2, 3]
    sit lista<numerus> b ← own a
    nota b
}
",
    );
    let tys = entry_local_tys(&result);
    assert!(matches!(
        result.types.get(tys[0]),
        Type::Ref(SemanticMutability::Own, _)
    ));
    assert!(matches!(
        result.types.get(tys[1]),
        Type::Ref(SemanticMutability::Own, _)
    ));
}

#[test]
fn value_marker_own_use_after_transfer_is_rejected() {
    assert_rejects_with_issue_la(
        r"
incipit {
    sit lista<numerus> a ← own [1, 2, 3]
    sit lista<numerus> b ← own a
    nota a
    nota b
}
",
        "use_after_move",
    );
}

#[test]
fn value_marker_copy_assignment_checks() {
    let _ = analyze_ok_la(
        r"
incipit {
    sit lista<numerus> c ← [1, 2, 3]
    varia lista<numerus> b ← [4]
    b ← copy c
    nota b
}
",
    );
}

#[test]
fn value_marker_copy_on_a_scalar_is_rejected() {
    assert_rejects_with_issue_la(
        "incipit { sit numerus x ← 5\n sit numerus y ← copy x }",
        ISSUE_SCALAR_ACCESS_MARKER,
    );
    assert_rejects_with_issue_la(
        "incipit { sit numerus x ← 5\n varia numerus y ← 1\n y ← copy x }",
        ISSUE_SCALAR_ACCESS_MARKER,
    );
}
