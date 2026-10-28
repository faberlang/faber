use crate::test_support::analyze_source_la;

fn prefix() -> &'static str {
    r#"functio make<magnitudo H, magnitudo W>() → numerus { redde 1 }

incipit {
    sit numerus w ← 4
"#
}

#[test]
fn slot_lift_runtime_value_is_captured() {
    let result = analyze_source_la(&format!(
        "{}    sit numerus a ← make<magnitudo 3, magnitudo w>()\n}}",
        prefix()
    ));
    assert!(result.is_ok(), "{:?}", result.diagnostics);
    assert!(
        result
            .result
            .shape_obligations
            .call_size_bindings
            .values()
            .flatten()
            .any(|binding| binding.parameter == 1),
        "{:?}",
        result.result.shape_obligations.call_size_bindings
    );
}

#[test]
fn slot_lift_bare_value_name_has_did_you_mean() {
    let result = analyze_source_la(&format!(
        "{}    sit numerus a ← make<magnitudo 3, w>()\n}}",
        prefix()
    ));
    assert!(
        result.diagnostics.iter().any(|diag| diag
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "size_slot_bare_value_name")),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn slot_lift_literal_and_size_param_remain_valid() {
    let source = r#"functio make<magnitudo H, magnitudo W>() → numerus { redde 1 }
functio forward<magnitudo W>() → numerus { redde make<magnitudo 3, magnitudo W>() }
incipit {
    sit numerus a ← make<magnitudo 3, magnitudo 4>()
}"#;
    let result = analyze_source_la(source);
    assert!(result.is_ok(), "{:?}", result.diagnostics);
}
