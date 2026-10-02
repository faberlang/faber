//! MIR lowering of the membership operators (`∈` / `∉`, membership-glyph 3a).
//!
//! `∈` over a collection is one `Collection::Contains` call; `∉` is the same
//! call followed by the existing boolean `not`. Range containment has no MIR
//! lowering (OD-4), so `∉` over a range fails closed exactly like `∈` does.

use crate::codegen::Target;
use crate::driver::{AnalyzeRequest, AnalyzedModule, Config, Session};
use crate::mir::{BufferHost, dump_analyzed_unit, run_source};

fn analyze(source: &str) -> AnalyzedModule {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    crate::driver::analyze_source(&session, "test.fab", source, AnalyzeRequest::default()).expect("analysis succeeds")
}

fn dump(source: &str) -> String {
    dump_analyzed_unit(&analyze(source)).expect("MIR lowering succeeds")
}

/// The dump with interned-symbol ids blanked: the `non (…)` spelling interns one
/// more source word than the glyph does, which shifts every later `sym#N`.
fn shape(source: &str) -> String {
    dump(source)
        .lines()
        .map(|line| match line.find("sym#") {
            Some(at) => format!("{}sym#_", &line[..at]),
            None => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn program(condition: &str) -> String {
    format!(
        "incipit {{\n    fixum lista<numerus> xs ← [1, 2]\n    si {condition} {{\n        nota \"hit\"\n    }}\n}}\n"
    )
}

fn run(condition: &str) -> Vec<String> {
    let session = Session::new(Config::default().with_target(Target::HirTypeScript));
    let mut host = BufferHost::default();
    run_source(&session, "membership.fab", &program(condition), &mut host).expect("runs");
    host.stdout_lines
}

#[test]
fn member_lowers_to_one_contains_call_with_no_negation() {
    let dump = dump(&program("1 ∈ xs"));
    assert_eq!(
        dump.matches("runtime collection contains(_0, const int 1)")
            .count(),
        1,
        "{dump}"
    );
    assert!(!dump.contains(" = not "), "{dump}");
}

#[test]
fn not_member_is_contains_followed_by_not() {
    let dump = dump(&program("3 ∉ xs"));
    assert_eq!(
        dump.matches("runtime collection contains(_0, const int 3)")
            .count(),
        1,
        "{dump}"
    );
    assert_eq!(dump.matches(" = not %").count(), 1, "{dump}");
}

#[test]
fn not_member_dump_equals_the_negated_positive_form() {
    assert_eq!(shape(&program("3 ∉ xs")), shape(&program("non (3 ∈ xs)")));
}

#[test]
fn runner_takes_the_right_branch_for_both_polarities() {
    assert_eq!(run("1 ∈ xs"), ["hit"]);
    assert!(run("3 ∈ xs").is_empty());
    assert_eq!(run("3 ∉ xs"), ["hit"]);
    assert!(run("1 ∉ xs").is_empty());
}

#[test]
fn range_containment_fails_closed_for_both_polarities() {
    for condition in ["5 ∈ 0‥10", "5 ∉ 0‥10"] {
        let source = format!("incipit {{\n    si {condition} {{\n        nota \"hit\"\n    }}\n}}\n");
        let errors = dump_analyzed_unit(&analyze(&source)).expect_err("range containment has no MIR lowering");
        assert!(
            errors
                .iter()
                .any(|error| error.issue == "unsupported_mir_lowering_binary_operator_without_a_mir_primitive"),
            "{condition}: {errors:?}"
        );
    }
}
