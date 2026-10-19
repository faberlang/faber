//! K8 foreign method calls on an `ignotum` receiver (wave-one purity rule).
//!
//! A method called on a binding that names a foreign module (here a Python
//! import) has no Faber body to look through, so the call is an effect: it
//! declines by name (`foreign_call_effect`) rather than as a generic
//! `unproved_call_effect`.

use crate::test_support::analyze_source_for_target_la;
use radix_target::Target;

const ISSUE: &str = "filum_independence_unproven";
const GRID: &str = "tensor<f32, [4]>";

fn rejected(source: &str, reason: &str) {
    let result = analyze_source_for_target_la(source, Target::HirPython);
    let errors: Vec<_> = result.diagnostics.iter().filter(|d| d.is_error()).collect();
    assert!(!errors.is_empty(), "body unexpectedly certified");
    assert!(
        errors.iter().all(|d| d.issue() == Some(ISSUE)),
        "not independence errors: {errors:?}"
    );
    assert!(
        errors.iter().any(|d| d
            .args
            .iter()
            .any(|arg| arg.name == "reason" && arg.value == reason)),
        "missing reason {reason}: {errors:?}"
    );
}

/// A Python module import, then a public kernel whose `filum` body calls a
/// method of the imported module.
fn kernel(helpers: &str, statement: &str) -> String {
    format!(
        r#"{helpers}
@ nucleum
@ publica
functio k(in {GRID} out, de {GRID} src) → vacuum {{
    itera ex src apud [i] filum lane sit value {{
        {statement}
    }}
}}
"#
    )
}

#[test]
fn foreign_call_on_an_imported_module_is_rejected() {
    let source = kernel(
        r#"importa ex "python:math" math ut m
"#,
        "m.sqrt(value)",
    );
    rejected(&source, "foreign_call_effect");
}
