//! Partial holes in a written type argument: `const list<_> a ← list<_>[1, 2]`
//! solves the element hole from the element types. Residual item 222376cb.

use super::compile;
use crate::codegen::Target;
use crate::driver::ModuleCompileRequest;
use crate::test_support::test_session;

const HOLE_LIST: &str =
    "+++\nlocale = \"en\"\n+++\n\nmain {\n    const list<_> a ← list<_>[1, 2]\n    print a.length()\n}\n";

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
