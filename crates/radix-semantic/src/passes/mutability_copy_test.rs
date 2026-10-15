//! MU-7 `copy` acquisition pins.
//!
//! Copy binds and copy parameters acquire fresh own values while leaving their
//! sources usable. Own transfer remains consuming, and nested markers stay
//! rejected at the type boundary.

use crate::test_support::{analyze_ok_la, assert_rejects_with_issue_la};

#[test]
fn copy_bind_is_independent_and_source_stays_live() {
    let _ = analyze_ok_la(
        r"
incipit {
    sit lista<numerus> source ← [1, 2, 3]
    varia lista<numerus> copied ← copy source
    copied.appende(4)
    nota source.longitudo()
}
",
    );
}

#[test]
fn copy_assignment_leaves_source_live() {
    let _ = analyze_ok_la(
        r"
incipit {
    sit lista<numerus> source ← own [1, 2, 3]
    varia lista<numerus> copied ← copy [4]
    copied ← source
    nota source.longitudo()
}
",
    );
}

#[test]
fn copy_parameter_entry_leaves_caller_source_live() {
    let _ = analyze_ok_la(
        r"
functio append(copy lista<numerus> xs) → vacuum {
    varia lista<numerus> local ← copy xs
    local.appende(4)
}
incipit {
    sit lista<numerus> source ← own [1, 2, 3]
    append(copy source)
    nota source.longitudo()
}
",
    );
}

#[test]
fn copy_bind_through_to_own_parameter_is_explicit() {
    let _ = analyze_ok_la(
        r"
functio take(own lista<numerus> xs) → vacuum {
    nota xs.longitudo()
}
incipit {
    sit lista<numerus> source ← [1, 2, 3]
    sit lista<numerus> copied ← copy source
    take(own copied)
    nota source.longitudo()
}
",
    );
}

#[test]
fn nested_access_marker_rejects_with_structured_issue() {
    assert_rejects_with_issue_la(
        "incipit { sit lista<copy numerus> xs }",
        "nested_access_marker",
    );
}

#[test]
fn copy_expression_form_does_not_exist() {
    assert_rejects_with_issue_la("incipit { nota copy([1, 2, 3]) }", "undefined_name");
}
