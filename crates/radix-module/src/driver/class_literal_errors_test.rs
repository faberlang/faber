//! Named errors for class-literal shape mistakes: an extra field, a missing
//! required field, and a bracket literal against a multi-member union.
//! Residual item 222376cb; operator ruling 2026-10-10.

use super::compile;
use crate::codegen::Target;
use crate::driver::ModuleCompileRequest;
use crate::test_support::{assert_compile_fails_with_issue, test_session};

const EXTRA_FIELD_BARE: &str = "+++\nlocale = \"en\"\n+++\n\nclass P {\n    var int x = 0\n}\n\nmain {\n    const P p ← { x = 1, z = 2 }\n    print p.x\n}\n";

const EXTRA_FIELD_HOLE: &str = "+++\nlocale = \"en\"\n+++\n\nclass P {\n    var int x = 0\n}\n\nmain {\n    const P p ← _ { x = 1, z = 2 }\n    print p.x\n}\n";

const MISSING_FIELD_BARE: &str = "+++\nlocale = \"en\"\n+++\n\nclass Q {\n    var int x\n    var int y\n}\n\nmain {\n    const Q q ← { x = 1 }\n    print q.x\n}\n";

const UNION_HOLE_BRACKETS: &str =
    "+++\nlocale = \"en\"\n+++\n\nmain {\n    const int ∪ string v ← _ [1, 2]\n    print v\n}\n";

const UNION_BARE_BRACKETS: &str =
    "+++\nlocale = \"en\"\n+++\n\nmain {\n    const int ∪ string v ← [1, 2]\n    print v\n}\n";

const OPTIONAL_LIST_BARE_BRACKETS: &str =
    "+++\nlocale = \"en\"\n+++\n\nmain {\n    const list<int> ∪ none xs ← [1, 2]\n    print xs\n}\n";

#[test]
fn class_literal_extra_field_bare_is_named() {
    let session = test_session(Target::HirRust);
    assert_compile_fails_with_issue(&session, "class-literal-extra-bare.fab", EXTRA_FIELD_BARE, "unknown_field");
}

#[test]
fn class_literal_extra_field_hole_is_named() {
    let session = test_session(Target::HirRust);
    assert_compile_fails_with_issue(&session, "class-literal-extra-hole.fab", EXTRA_FIELD_HOLE, "unknown_field");
}

#[test]
fn class_literal_missing_required_field_is_named() {
    let session = test_session(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "class-literal-missing.fab",
        MISSING_FIELD_BARE,
        "missing_required_struct_field",
    );
}

#[test]
fn inferred_bracket_hole_into_multi_member_union_is_ambiguous() {
    let session = test_session(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "class-literal-union-hole.fab",
        UNION_HOLE_BRACKETS,
        "inferred_constructor_ambiguous_union",
    );
}

#[test]
fn bare_bracket_literal_into_multi_member_union_is_ambiguous() {
    let session = test_session(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "class-literal-union-bare.fab",
        UNION_BARE_BRACKETS,
        "inferred_constructor_ambiguous_union",
    );
}

#[test]
fn bare_bracket_literal_into_optional_list_still_builds_the_list() {
    let session = test_session(Target::HirRust);
    let result = compile(
        &session,
        "class-literal-optional-list.fab",
        OPTIONAL_LIST_BARE_BRACKETS,
        ModuleCompileRequest::default(),
    );
    assert!(
        result.success(),
        "`list<int> ∪ none ← [1, 2]` must still build the list member: {:?}",
        result.diagnostics
    );
}
