//! Coverage for a method call reached through an optional-chain member.
//!
//! `a?.area()` folds to a `Call` whose callee is an
//! `OptionalChain(object, Member(area))`. The member segment must resolve a
//! declared method, not only a field, and the call result stays option-wrapped.

use radix_diagnostics::DiagnosticArg;

fn compile(source: &str) -> crate::test_support::AnalyzedFixture {
    crate::test_support::analyze_source_la(source)
}

fn rejects_with_issue(source: &str, issue: &str) {
    let result = compile(source);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diag| diag.args == vec![DiagnosticArg::new("issue", issue)]),
        "expected diagnostic issue {issue:?}, got: {:?}",
        result.diagnostics
    );
}

const FORMA: &str = "genus Forma {\n    varia numerus latitudo = 1\n    varia numerus altitudo = 2\n\n    functio area() → numerus {\n        redde ego.latitudo * ego.altitudo\n    }\n}\n";

#[test]
fn optional_chain_method_call_checks_clean() {
    let source = format!(
        "{FORMA}\nincipit {{\n    sit Forma ∪ nihil a ← Forma {{ latitudo = 2, altitudo = 3 }}\n    sit Forma ∪ nihil b ← nulla\n    sit numerus ∪ nihil area ← a?.area()\n    nota area\n    nota b?.area()\n}}"
    );
    let result = compile(&source);
    assert!(
        result.is_ok(),
        "expected clean, got: {:?}",
        result.diagnostics
    );
}

#[test]
fn direct_method_call_on_optional_still_requires_chain() {
    let source =
        format!("{FORMA}\nincipit {{\n    sit Forma ∪ nihil a ← nulla\n    nota a.area()\n}}");
    rejects_with_issue(&source, "optional_receiver_requires_chain");
}

#[test]
fn optional_chain_method_bad_arity_still_fails() {
    let source =
        format!("{FORMA}\nincipit {{\n    sit Forma ∪ nihil a ← nulla\n    nota a?.area(1)\n}}");
    let result = compile(&source);
    assert!(
        !result.is_ok(),
        "expected failure, got: {:?}",
        result.diagnostics
    );
}
