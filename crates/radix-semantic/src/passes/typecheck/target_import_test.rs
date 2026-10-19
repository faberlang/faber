//! Target-native imports (`importa python "math" m`): the bound names are
//! trusted `ignotum` values whose calls and member access are admitted
//! unchecked, and the program is bound to the import's target.
//!
//! A different build target is SEM017 `import_target_bound` (args `target`,
//! `specifier`, `build`). Tests identify diagnostics by issue and args, never
//! by message text.

use crate::test_support;
use radix_target::Target;

const ISSUE: &str = "import_target_bound";

const CALL_AND_MEMBER: &str = r#"
importa python "math" m

incipit {
    scribe m.sqrt(2.0)
    scribe m.pi
    scribe m.constants.e
}
"#;

#[test]
fn calls_and_member_access_on_a_bound_name_are_admitted_for_the_target() {
    test_support::assert_accepts_for_target_la(CALL_AND_MEMBER, Target::HirPython);
}

#[test]
fn trust_reaches_through_locals() {
    test_support::assert_accepts_for_target_la(
        r#"
importa python "os" os

incipit {
    sit _ env ← os.environ
    scribe env.get("HOME")
    scribe env.anything.at.all
}
"#,
        Target::HirPython,
    );
}

#[test]
fn selective_and_wildcard_bindings_are_trusted_too() {
    test_support::assert_accepts_for_target_la(
        r#"
importa python "math" fixum sqrt, pi ut tau
importa python "os.path" * ut osp

incipit {
    scribe sqrt(16.0)
    scribe tau
    scribe osp.join("a", "b")
}
"#,
        Target::HirPython,
    );
}

#[test]
fn another_build_target_is_refused_by_name() {
    for build in [Target::HirRust, Target::HirTypeScript, Target::HirGo] {
        let result = test_support::analyze_source_for_target_la(CALL_AND_MEMBER, build);
        let found: Vec<_> = result
            .diagnostics
            .iter()
            .filter(|err| err.is_error() && test_support::issue(err) == Some(ISSUE))
            .collect();
        assert_eq!(found.len(), 1, "{build}: {:?}", result.diagnostics);
        let arg = |name: &str| {
            found[0]
                .args
                .iter()
                .find(|arg| arg.name == name)
                .map(|arg| arg.value.as_str())
        };
        assert_eq!(arg("target"), Some("python"));
        assert_eq!(arg("specifier"), Some("math"));
        assert_eq!(arg("build"), Some(build.as_str()));
    }
}

#[test]
fn a_target_alias_spelling_binds_to_the_same_target() {
    test_support::assert_accepts_for_target_la(
        r#"
importa typescript "node:fs" fs

incipit {
    scribe fs.readFileSync("x", "utf8")
}
"#,
        Target::HirTypeScript,
    );
}

#[test]
fn member_access_stays_rejected_without_a_target_import() {
    test_support::assert_rejects_with_issue_la(
        r#"
incipit {
    sit ignotum x ← 1
    scribe x.field
}
"#,
        "type_has_no_field",
    );
}

#[test]
fn the_runner_is_refused_by_name() {
    let result = test_support::analyze_source_for_target_la(CALL_AND_MEMBER, Target::MirStepper);
    let found: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|err| err.is_error() && test_support::issue(err) == Some(ISSUE))
        .collect();
    assert_eq!(found.len(), 1, "{:?}", result.diagnostics);
    let build = found[0]
        .args
        .iter()
        .find(|arg| arg.name == "build")
        .map(|arg| arg.value.as_str());
    assert_eq!(build, Some("runner"));
}
