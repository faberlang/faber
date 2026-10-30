//! `ref` locals and the re-pointable bare parameter.
//!
//! A `ref` local names a place: the name may be re-pointed, a write through it
//! is one `assign_through_ref` diagnostic, and a scalar type takes no marker
//! (`scalar_access_marker`). A bare parameter is a `ref`: re-pointing the name
//! is legal, a write through it is one `assign_to_de_parameter` diagnostic.
//! Tests assert issue slugs only.

use crate::test_support::{AnalyzedFixture, analyze_source_with_config, issue};

fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = crate::PassConfig::for_target(radix_target::Target::HirRust);
    config.locale_pack = Some(
        crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

const CELL: &str = "class Cell {\n    mut int value\n}\n";

fn error_issues(source: &str) -> Vec<String> {
    analyze_en(source)
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .map(|d| issue(d).unwrap_or("<no issue>").to_owned())
        .collect()
}

#[test]
fn ref_local_accepts_repointing() {
    let errors = error_issues(&format!(
        "{CELL}fn f(ref Cell a, ref Cell b) → int {{\n    ref Cell cur ← a\n    cur ← b\n    return cur.value\n}}\nmain {{}}\n"
    ));
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn write_through_a_ref_local_is_one_diagnostic() {
    let errors = error_issues(&format!(
        "{CELL}main {{\n    mut Cell c ← Cell {{ value = 1 }}\n    ref Cell cur ← c\n    cur.value ← 5\n    print c.value\n}}\n"
    ));
    assert_eq!(errors, ["assign_through_ref"], "{errors:?}");
}

#[test]
fn a_scalar_ref_local_is_refused() {
    let errors = error_issues("main {\n    let int n ← 3\n    ref int m ← n\n    print m\n}\n");
    assert!(
        errors.iter().any(|e| e == "scalar_access_marker"),
        "{errors:?}"
    );
}

#[test]
fn bare_parameter_may_be_repointed() {
    let errors = error_issues(&format!(
        "{CELL}fn f(Cell a, Cell b) → int {{\n    a ← b\n    return a.value\n}}\nmain {{}}\n"
    ));
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn write_through_a_bare_parameter_is_one_diagnostic() {
    let errors = error_issues(&format!(
        "{CELL}fn f(Cell a) → void {{\n    a.value ← 5\n}}\nmain {{}}\n"
    ));
    assert_eq!(errors, ["assign_to_de_parameter"], "{errors:?}");
}

#[test]
fn a_ref_local_that_is_never_repointed_is_not_an_unneeded_mut() {
    let fixture = analyze_en(&format!(
        "{CELL}main {{\n    let Cell c ← Cell {{ value = 1 }}\n    ref Cell cur ← c\n    print cur.value\n}}\n"
    ));
    assert!(
        !fixture
            .diagnostics
            .iter()
            .any(|d| issue(d) == Some("unnecessary_varia")),
        "{:?}",
        fixture.diagnostics
    );
}

// --- Option A (operator ruling 2026-10-15): optional ref cursors ---

const NODE: &str = "class Node {\n    mut int value\n    mut Node next optional\n}\n";

#[test]
fn ref_over_an_optional_reference_type_is_legal() {
    let errors = error_issues(&format!(
        "{NODE}fn last(Node head) → int {{\n    ref Node ∪ none cur ← head\n    cur ← head.next\n    return head.value\n}}\nmain {{}}\n"
    ));
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn mut_over_an_optional_reference_type_is_legal() {
    let errors = error_issues(&format!(
        "{NODE}main {{\n    mut Node n ← Node {{ value = 1 }}\n    mut Node ∪ none cur ← n\n    cur ← null\n}}\n"
    ));
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn ref_over_an_optional_scalar_is_refused() {
    let errors = error_issues(
        "fn f(int ∪ none maybe) → int {\n    ref int ∪ none cur ← maybe\n    return 5\n}\nmain {}\n",
    );
    assert!(
        errors.iter().any(|e| e == "scalar_access_marker"),
        "{errors:?}"
    );
}

#[test]
fn a_vector_ref_local_is_refused() {
    // Operator ruling 7b25889f R1: a vector is a silent value, so `ref` over
    // one is the scalar refusal, and `b ← a` copies on its own.
    let errors = error_issues(
        "main {\n    let vf32[3] a ← [1.0, 2.0, 3.0] ↦ vf32[3]\n    ref vf32[3] b ← a\n    print b[0]\n}\n",
    );
    assert!(
        errors.iter().any(|e| e == "scalar_access_marker"),
        "{errors:?}"
    );
}

#[test]
fn a_matrix_ref_local_is_refused() {
    let errors = error_issues(
        "main {\n    let mf32[2, 2] a ← [[1.0, 2.0], [3.0, 4.0]] ↦ mf32[2, 2]\n    ref mf32[2, 2] b ← a\n    print b[[0, 0]]\n}\n",
    );
    assert!(
        errors.iter().any(|e| e == "scalar_access_marker"),
        "{errors:?}"
    );
}

#[test]
fn ref_over_a_non_optional_union_keeps_the_nested_marker_refusal() {
    let errors = error_issues(
        "class A { mut int x }\nclass B { mut int y }\nmain {\n    let A a ← A { x = 1 }\n    let B b ← B { y = 2 }\n    ref A ∪ B cur ← a\n    print cur.x\n}\n",
    );
    assert!(
        errors.iter().any(|e| e == "nested_access_marker"),
        "{errors:?}"
    );
}

#[test]
fn a_dum_test_narrows_an_optional_ref_cursor_to_its_core() {
    // The motivating walk: reads inside the narrowed body need no `?.`, and
    // the re-point after them stays legal.
    let errors = error_issues(&format!(
        "{NODE}fn last(Node head) → int {{\n    ref Node ∪ none cur ← head\n    while cur not is none {{\n        if cur.next is none {{\n            return cur.value\n        }}\n        cur ← cur.next\n    }}\n    return head.value\n}}\nmain {{}}\n"
    ));
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn a_read_after_the_repoint_leaves_the_narrowed_state() {
    // After `cur ← cur.next` the name is the full optional again, so a member
    // read without a new test needs the chain (SEM004), exactly like the
    // pre-Option-A loop-entry policy.
    let errors = error_issues(&format!(
        "{NODE}fn last(Node head) → int {{\n    ref Node ∪ none cur ← head\n    while cur not is none {{\n        cur ← cur.next\n        if cur is none {{\n            return 0\n        }}\n        return cur.value\n    }}\n    return head.value\n}}\nmain {{}}\n"
    ));
    assert!(errors.is_empty(), "{errors:?}");
}

#[test]
fn an_unnarrowed_optional_cursor_read_requires_the_chain() {
    let errors = error_issues(&format!(
        "{NODE}fn f(Node head) → int {{\n    ref Node ∪ none cur ← head.next\n    return cur.value\n}}\nmain {{}}\n"
    ));
    assert!(
        errors
            .iter()
            .any(|e| e == "optional_receiver_requires_chain"),
        "{errors:?}"
    );
}

#[test]
fn write_through_an_optional_ref_cursor_is_one_diagnostic() {
    let errors = error_issues(&format!(
        "{NODE}fn bump(Node head) → void {{\n    ref Node ∪ none cur ← head\n    while cur not is none {{\n        cur.value ← 99\n        cur ← cur.next\n    }}\n}}\nmain {{}}\n"
    ));
    assert_eq!(errors, ["assign_through_ref"], "{errors:?}");
}

#[test]
fn la_optional_ref_cursor_walk_typechecks() {
    crate::test_support::assert_accepts_la(
        "genus Nodus {\n    varia numerus valor\n    varia Nodus proximus sponte\n}\n\nfunctio ultimus(Nodus caput) → numerus {\n    de Nodus ∪ nihil cursus ← caput\n    dum cursus non est nihil {\n        cursus ← cursus.proximus\n    }\n    redde caput.valor\n}\n\nincipit {\n    sit Nodus a ← Nodus { valor = 1 }\n    nota ultimus(a)\n}\n",
    );
}

#[test]
fn la_optional_ref_scalar_is_refused() {
    crate::test_support::assert_rejects_with_issue(
        "functio f(numerus ∪ nihil forte) → numerus {\n    de numerus ∪ nihil cursus ← forte\n    redde 5\n}\n\nincipit {\n    nota f(nulla)\n}\n",
        "scalar_access_marker",
    );
}
