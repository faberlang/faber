//! Inferred set-cell bracket literals: bare `[..]` and `_ [..]` build the set.
//! Ruling 0ee37320e0; residual item 222376cb.

use super::compile;
use crate::codegen::Target;
use crate::driver::{Config, ModuleCompileRequest, Session};
use crate::test_support::{assert_compile_fails_with_issue, test_session};

const BARE_LITERAL: &str =
    "+++\nlocale = \"en\"\n+++\n\nmain {\n    const set<int> s ← [1, 2, 2]\n    print s.length()\n}\n";
const HOLE_LITERAL: &str =
    "+++\nlocale = \"en\"\n+++\n\nmain {\n    const set<int> s ← _ [1, 2]\n    print s.length()\n}\n";

#[test]
fn inferred_set_cell_bare_literal_builds_the_set() {
    let session = test_session(Target::HirRust);
    let result = compile(&session, "set-bare.fab", BARE_LITERAL, ModuleCompileRequest::default());
    assert!(result.success(), "bare set literal must typecheck: {:?}", result.diagnostics);
}

#[test]
fn inferred_set_cell_hole_literal_builds_the_set() {
    let session = test_session(Target::HirRust);
    let result = compile(&session, "set-hole.fab", HOLE_LITERAL, ModuleCompileRequest::default());
    assert!(result.success(), "hole set literal must typecheck: {:?}", result.diagnostics);
}

#[test]
fn inferred_set_cell_bare_literal_runs_on_the_mir_runner() {
    use crate::mir::{BufferHost, run_source};
    let session = test_session(Target::HirTypeScript);
    let mut host = BufferHost::default();
    run_source(&session, "set-bare-run.fab", BARE_LITERAL, &mut host).expect("set literal runs");
    assert_eq!(host.stdout_lines, ["2"], "duplicates must collapse into the set");
}

#[test]
fn inferred_set_cell_element_type_mismatch_is_named() {
    let session = test_session(Target::HirRust);
    let source =
        "+++\nlocale = \"en\"\n+++\n\nmain {\n    const set<int> s ← [\"a\"]\n    print s.length()\n}\n";
    assert_compile_fails_with_issue(&session, "set-mismatch.fab", source, "expression_type_mismatch");
}

#[cfg(feature = "hir-faber")]
fn emit_en(source: &str) -> String {
    let pack = crate::locale::LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
        .unwrap_or_else(|error| panic!("installed en pack loads: {error:?}"));
    let session = Session::new(
        Config::default()
            .with_resolved_locale_pack(pack.clone())
            .with_dev_locale_root(),
    );
    let result = crate::codegen::faber::reemit_faber(&session, &pack, "set-format.fab", source);
    assert!(
        result.success(),
        "format must succeed: {:?}",
        result
            .diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.issue()))
            .collect::<Vec<_>>()
    );
    result.output.expect("re-emitted program").code
}

#[cfg(feature = "hir-faber")]
#[test]
fn inferred_set_cell_bare_literal_round_trips_through_faber_format() {
    let first = emit_en(BARE_LITERAL);
    assert!(first.contains("[1, 2, 2]"), "bare set literal must print bare:\n{first}");
    assert_eq!(emit_en(&first), first, "bare set literal format is not idempotent");
}

#[cfg(feature = "hir-faber")]
#[test]
fn inferred_set_cell_hole_literal_round_trips_through_faber_format() {
    let first = emit_en(HOLE_LITERAL);
    assert!(first.contains("_ [1, 2]"), "hole set literal must print the hole:\n{first}");
    assert_eq!(emit_en(&first), first, "hole set literal format is not idempotent");
}
