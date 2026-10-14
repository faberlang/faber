//! D9.2 union overlap access: a `∪` of nominal members exposes exactly the
//! methods and fields every member declares with the same signature / type.
//!
//! Access in the overlap is plain (no teaching warning); access outside it is
//! an error that names each member (`union_member_not_common` when a member
//! lacks the name, `union_member_differs` when every member has it but the
//! types or signatures disagree). Writes follow the read rule; field
//! mutability (D16.1) on writes is covered in `field_modifier_write_test.rs`; member visibility
//! (D5.3) has no per-field surface yet.
//!
//! Fixtures use latin spellings because the radix unit-test harness compiles
//! bare source without the en-locale lex pack.

use radix_diagnostics::DiagnosticArg;

use crate::test_support;

const FIGURA: &str = "\
genus Circulus {
    varia textus nomen
    varia fractus radius

    functio area() → fractus { redde 3.0 * ego.radius * ego.radius }
}

genus Quadratum {
    varia textus nomen
    varia fractus latus

    functio area() → fractus { redde ego.latus * ego.latus }
}

typus Figura = Circulus ∪ Quadratum

";

fn figura(body: &str) -> String {
    format!("{FIGURA}{body}")
}

fn has_warning_issue(source: &str, expected: &str) -> bool {
    test_support::analyze_and_get_diagnostics(source)
        .iter()
        .any(|err| !err.is_error() && test_support::issue(err) == Some(expected))
}

#[test]
fn overlap_method_call_is_accepted_without_teaching_warning() {
    let source = figura("functio f(Figura x) → fractus { redde x.area() }");
    test_support::assert_accepts(&source);
    assert!(!has_warning_issue(&source, "union_nominal_members"));
}

#[test]
fn overlap_field_read_is_accepted_without_teaching_warning() {
    let source = figura("functio f(Figura x) → textus { redde x.nomen }");
    test_support::assert_accepts(&source);
    assert!(!has_warning_issue(&source, "union_nominal_members"));
}

#[test]
fn overlap_field_write_is_accepted() {
    test_support::assert_accepts(&figura(
        "functio f(Figura x) → textus {\n    varia Figura y ← copy x\n    y.nomen ← \"z\"\n    redde y.nomen\n}",
    ));
}

#[test]
fn field_outside_overlap_names_lacking_member() {
    test_support::assert_rejects_with_args(
        &figura("functio f(Figura x) → fractus { redde x.radius }"),
        &[
            DiagnosticArg::new("issue", "union_member_not_common"),
            DiagnosticArg::new("member", "radius"),
            DiagnosticArg::new("lacking", "Quadratum"),
        ],
    );
}

#[test]
fn field_type_mismatch_names_each_member() {
    test_support::assert_rejects_with_args(
        concat!(
            "genus Circulus {\n    varia fractus area\n}\n\n",
            "genus Quadratum {\n    varia numerus area\n}\n\n",
            "functio f(Circulus ∪ Quadratum x) → fractus { redde x.area }",
        ),
        &[
            DiagnosticArg::new("issue", "union_member_differs"),
            DiagnosticArg::new("member", "area"),
            DiagnosticArg::new("detail", "`fractus` in Circulus, `numerus` in Quadratum"),
        ],
    );
}

#[test]
fn field_write_type_mismatch_is_rejected() {
    test_support::assert_rejects_with_issue(
        concat!(
            "genus Circulus {\n    varia fractus area\n}\n\n",
            "genus Quadratum {\n    varia numerus area\n}\n\n",
            "functio f(Circulus ∪ Quadratum x) {\n    varia Circulus ∪ Quadratum y ← x\n    y.area ← 1\n}",
        ),
        "union_member_differs",
    );
}

#[test]
fn method_signature_mismatch_names_each_member() {
    test_support::assert_rejects_with_args(
        concat!(
            "genus Circulus {\n    functio area() → fractus { redde 1.0 }\n}\n\n",
            "genus Quadratum {\n    functio area() → numerus { redde 1 }\n}\n\n",
            "functio f(Circulus ∪ Quadratum x) → fractus { redde x.area() }",
        ),
        &[
            DiagnosticArg::new("issue", "union_member_differs"),
            DiagnosticArg::new("member", "area"),
            DiagnosticArg::new(
                "detail",
                "`() → fractus` in Circulus, `() → numerus` in Quadratum",
            ),
        ],
    );
}

#[test]
fn method_outside_overlap_names_lacking_member() {
    test_support::assert_rejects_with_args(
        concat!(
            "genus Circulus {\n    functio area() → fractus { redde 1.0 }\n}\n\n",
            "genus Quadratum {\n    varia textus nomen\n}\n\n",
            "functio f(Circulus ∪ Quadratum x) → fractus { redde x.area() }",
        ),
        &[
            DiagnosticArg::new("issue", "union_member_not_common"),
            DiagnosticArg::new("member", "area"),
            DiagnosticArg::new("lacking", "Quadratum"),
        ],
    );
}
