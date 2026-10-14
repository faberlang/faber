//! Partial holes in a written type argument: `const list<_> a ← list<_>[1, 2]`
//! solves the element hole from the element types. Residual item 222376cb.

use super::compile;
use crate::codegen::Target;
use crate::driver::ModuleCompileRequest;
use crate::test_support::{assert_compile_fails_with_issue, test_session};

const HOLE_LIST: &str =
    "+++\nlocale = \"en\"\n+++\n\nmain {\n    const list<_> a ← list<_>[1, 2]\n    print a.length()\n}\n";

#[cfg(feature = "hir-rust")]
#[test]
fn partial_hole_list_compiles() {
    let session = test_session(Target::HirRust);
    let result = compile(&session, "partial-hole-list.fab", HOLE_LIST, ModuleCompileRequest::default());
    assert!(result.success(), "list element hole must typecheck: {:?}", result.diagnostics);
}

#[test]
fn partial_hole_list_runs_on_the_mir_runner() {
    use crate::mir::{BufferHost, run_source};
    let session = test_session(Target::HirTypeScript);
    let mut host = BufferHost::default();
    run_source(&session, "partial-hole-list-run.fab", HOLE_LIST, &mut host).expect("list element hole runs");
    assert_eq!(host.stdout_lines, ["2"], "both elements are in the list");
}

const HOLE_TUPLE: &str = "+++\nlocale = \"en\"\n+++\n\nmain {\n    const tuple<_, string> t \u{2190} tuple<_, string>[1, \"x\"]\n    print t[1]\n}\n";

#[cfg(feature = "hir-rust")]
#[test]
fn partial_hole_tuple_compiles() {
    let session = test_session(Target::HirRust);
    let result = compile(&session, "partial-hole-tuple.fab", HOLE_TUPLE, ModuleCompileRequest::default());
    assert!(result.success(), "tuple element hole must typecheck: {:?}", result.diagnostics);
}

#[test]
fn partial_hole_tuple_runs_on_the_mir_runner() {
    use crate::mir::{BufferHost, run_source};
    let session = test_session(Target::HirTypeScript);
    let mut host = BufferHost::default();
    run_source(&session, "partial-hole-tuple-run.fab", HOLE_TUPLE, &mut host).expect("tuple element hole runs");
    assert_eq!(host.stdout_lines, ["x"], "the second tuple element is the string");
}

const NO_ELEMENT_LIST: &str =
    "+++\nlocale = \"en\"\n+++\n\nmain {\n    const list<_> a \u{2190} list<_>[]\n    print a.length()\n}\n";

#[test]
fn partial_hole_list_with_no_element_is_named() {
    let session = test_session(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "partial-hole-list-no-element.fab",
        NO_ELEMENT_LIST,
        "cannot_infer_empty_list_element_type",
    );
}

const HOLE_TUPLE_ELEMENT_MISMATCH: &str = "+++\nlocale = \"en\"\n+++\n\nmain {\n    const tuple<_, string> t \u{2190} tuple<_, string>[1, 2]\n    print t[1]\n}\n";

#[test]
fn partial_hole_tuple_element_mismatch_is_named() {
    let session = test_session(Target::HirRust);
    assert_compile_fails_with_issue(
        &session,
        "partial-hole-tuple-mismatch.fab",
        HOLE_TUPLE_ELEMENT_MISMATCH,
        "expression_type_mismatch",
    );
}
