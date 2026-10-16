//! Empty bare literals with no typed cell are the double hole and a named
//! error; a typed cell keeps the literal's ordinary construction.
//! Ruling 2026-10-10 (empty literals and `empty`); residual item 222376cb.

use super::compile;
use crate::codegen::Target;
use crate::driver::ModuleCompileRequest;
use crate::test_support::{assert_compile_fails_with_issue, test_session};

const OBJECT_HOLE_CELL: &str = "+++\nlocale = \"en\"\n+++\n\nmain {\n    let _ a ← {}\n    print a\n}\n";
const LIST_HOLE_CELL: &str = "+++\nlocale = \"en\"\n+++\n\nmain {\n    let _ a ← []\n    print a\n}\n";
const OBJECT_NO_CELL: &str = "+++\nlocale = \"en\"\n+++\n\nmain {\n    print {}\n}\n";
const LIST_NO_CELL: &str = "+++\nlocale = \"en\"\n+++\n\nmain {\n    print []\n}\n";
const TYPED_CELLS: &str = "+++\nlocale = \"en\"\n+++\n\nmain {\n    let json j ← {}\n    let map<string, int> m ← {}\n    let list<int> xs ← []\n    print j\n    print m\n    print xs\n}\n";

#[test]
fn empty_object_with_hole_cell_is_named_error() {
    let session = test_session(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "empty-object-hole-cell.fab",
        OBJECT_HOLE_CELL,
        "inferred_constructor_no_expected_type",
    );
}

#[test]
fn empty_list_with_hole_cell_is_named_error() {
    let session = test_session(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "empty-list-hole-cell.fab",
        LIST_HOLE_CELL,
        "inferred_constructor_no_expected_type",
    );
}

#[test]
fn empty_object_argument_without_cell_is_named_error() {
    let session = test_session(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "empty-object-argument.fab",
        OBJECT_NO_CELL,
        "inferred_constructor_no_expected_type",
    );
}

#[test]
fn empty_list_argument_without_cell_is_named_error() {
    let session = test_session(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "empty-list-argument.fab",
        LIST_NO_CELL,
        "inferred_constructor_no_expected_type",
    );
}

#[cfg(feature = "hir-rust")]
#[test]
fn typed_empty_cells_accept_empty_literals() {
    let session = test_session(Target::HirRust);
    let result = compile(&session, "typed-empty-cells.fab", TYPED_CELLS, ModuleCompileRequest::default());
    assert!(
        result.success(),
        "empty literals in typed cells must typecheck: {:?}",
        result.diagnostics
    );
}
