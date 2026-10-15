//! Purity gate for `@ conversion` function bodies (CTR-04).
//!
//! A marked row is a pure re-presentation, so a mutation anywhere in the body
//! fails closed with `conversio_impure_body`, however deeply the control flow
//! nests it. Each test hides one mutation inside one construct and nothing
//! else, so a gate that only inspects the top-level statement list (and plain
//! `Block` expressions) lets the program through.

use crate::PassConfig;
use crate::locale::LocalePack;
use crate::test_support::{AnalyzedFixture, analyze_source_with_config};
use radix_target::Target;

const IMPURE: &str = "conversio_impure_body";

const PRELUDE: &str = "\
class Celsius {
    mut float degrees
}

class Fahrenheit {
    mut float degrees
}

class Reading {
    mut int value
}

union Condicio {
    Agens,
    Iners
}

";

/// The fixtures are written in the English surface.
fn analyze_en(source: &str) -> AnalyzedFixture {
    let mut config = PassConfig::for_target(Target::HirRust);
    config.locale_pack = Some(
        LocalePack::from_toml_path(crate::locale::workspace_locale_pack_path("en"))
            .expect("load English locale pack"),
    );
    analyze_source_with_config(source, config)
}

fn assert_rejects_with_issue(source: &str, issue: &str) {
    let fixture = analyze_en(source);
    assert!(
        fixture
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.is_error() && diagnostic.issue() == Some(issue)),
        "expected diagnostic issue {issue:?}, got: {:?}",
        fixture.diagnostics
    );
}

/// One conversion `Celsius → Fahrenheit` whose body is `BODY` followed by a
/// pure `return`.
fn celsius_conversion(body: &str) -> String {
    let template = "\
@ conversion
fn toFahrenheit(Celsius c) → Fahrenheit {
BODY
    return Fahrenheit { degrees = c.degrees * 9.0 / 5.0 + 32.0 }
}
";
    format!("{PRELUDE}{}", template.replace("BODY", body))
}

fn assert_body_is_impure(body: &str) {
    assert_rejects_with_issue(&celsius_conversion(body), IMPURE);
}

#[test]
fn top_level_assignment_is_still_impure() {
    assert_body_is_impure("    c.degrees ← 1.0");
}

#[test]
fn mutation_inside_si_then_branch_is_impure() {
    assert_body_is_impure(
        "    if c.degrees ≻ 0.0 {
        c.degrees ← 1.0
    }",
    );
}

#[test]
fn mutation_inside_si_else_branch_is_impure() {
    assert_body_is_impure(
        "    if c.degrees ≻ 0.0 {
        print \"positive\"
    }
    else {
        c.degrees ← 1.0
    }",
    );
}

#[test]
fn mutable_local_inside_si_is_impure() {
    assert_body_is_impure(
        "    if c.degrees ≻ 0.0 {
        mut float d ← c.degrees
        print d
    }",
    );
}

#[test]
fn mutation_inside_dum_is_impure() {
    assert_body_is_impure(
        "    while c.degrees ≺ 1.0 {
        c.degrees ← c.degrees + 1.0
    }",
    );
}

#[test]
fn mutation_inside_itera_is_impure() {
    assert_body_is_impure(
        "    for range 0‥3 const _ {
        c.degrees ← 1.0
    }",
    );
}

#[test]
fn mutation_inside_fac_dum_loop_is_impure() {
    assert_body_is_impure(
        "    do {
        c.degrees ← 1.0
    } while c.degrees ≺ 1.0",
    );
}

#[test]
fn mutation_inside_custodi_clause_is_impure() {
    assert_body_is_impure(
        "    guard {
        if c.degrees ≺ 0.0 {
            c.degrees ← 0.0
            return Fahrenheit { degrees = 0.0 }
        }
    }",
    );
}

#[test]
fn increment_inside_nested_control_flow_is_impure() {
    let source = format!(
        "{PRELUDE}\
@ conversion
fn toFahrenheit(Reading r) → Fahrenheit {{
    for range 0‥3 const _ {{
        if r.value ≻ 0 {{
            r.value ↑
        }}
    }}
    return Fahrenheit {{ degrees = 0.0 }}
}}
"
    );
    assert_rejects_with_issue(&source, IMPURE);
}

#[test]
fn mutation_inside_discerne_arm_is_impure() {
    let source = format!(
        "{PRELUDE}\
@ conversion
fn toReading(Condicio s) → Reading {{
    let Reading out ← Reading {{ value = 0 }}
    match s {{
        case Agens {{
            out.value ← 1
        }}
        case Iners {{
            print \"iners\"
        }}
    }}
    return out
}}
"
    );
    assert_rejects_with_issue(&source, IMPURE);
}

#[test]
fn control_flow_without_mutation_stays_pure() {
    let fixture = analyze_en(&celsius_conversion(
        "    if c.degrees ≺ 0.0 {
        return Fahrenheit { degrees = 0.0 }
    }
    else {
        print \"warm\"
    }
    for range 0‥3 const _ {
        print c.degrees
    }",
    ));
    assert!(
        fixture
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.issue() != Some(IMPURE)),
        "pure control flow must not trip the purity gate: {:?}",
        fixture.diagnostics
    );
}
