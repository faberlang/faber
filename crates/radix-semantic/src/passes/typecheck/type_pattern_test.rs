//! N1 type-pattern grammar, arm narrowing, and scrutinee-directed selection.

use crate::test_support;
use radix_diagnostics::DiagnosticArg;

#[test]
fn class_member_type_patterns_narrow_arm_bindings() {
    test_support::assert_accepts(
        r"genus Formal {
    varia textus greeting
}

genus Casual {
    varia textus greeting
}

functio probe(Formal ∪ Casual u) → textus {
    discerne u {
        casu Formal fixum f { redde f.greeting }
        casu Casual fixum c { redde c.greeting }
    }
}",
    );
}

#[test]
fn type_pattern_narrows_the_scrutinee_binding_inside_an_arm() {
    test_support::assert_accepts(
        r"genus Formal { varia textus greeting }
genus Casual { varia textus greeting }
functio probe(Formal ∪ Casual u) → textus {
    discerne u {
        casu Formal { redde u.greeting }
        casu Casual { redde u.greeting }
    }
}",
    );
}

#[test]
fn primitive_and_nil_type_patterns_cover_nullable_union() {
    test_support::assert_accepts(
        r#"functio probe(numerus ∪ textus ∪ nihil u) → vacuum {
    discerne u {
        casu numerus fixum i { nota i }
        casu textus fixum s { nota s }
        casu nihil { nota "nihil" }
    }
}"#,
    );
}

#[test]
fn type_pattern_exhaustiveness_reports_missing_union_member() {
    test_support::assert_rejects_with_issue(
        r"genus Formal { varia textus greeting }
genus Casual { varia textus greeting }
genus Other { varia textus greeting }
functio probe(Formal ∪ Casual ∪ Other u) → textus {
    discerne u {
        casu Formal fixum f { redde f.greeting }
        casu Casual fixum c { redde c.greeting }
    }
}",
        "missing_union_members",
    );
}

#[test]
fn type_pattern_catchall_closes_union_coverage() {
    test_support::assert_accepts(
        r#"genus Formal { varia textus greeting }
genus Casual { varia textus greeting }
functio probe(Formal ∪ Casual u) → textus {
    discerne u {
        casu Formal fixum f { redde f.greeting }
        casu _ { redde "other" }
    }
}"#,
    );
}

#[test]
fn type_pattern_rejects_member_not_in_scrutinee_union() {
    test_support::assert_rejects_with_issue(
        r"genus Formal { varia textus greeting }
functio probe(textus u) → vacuum {
    discerne u {
        casu Formal fixum f { nota f }
    }
}",
        "discerne_pattern_not_in_closed_set",
    );
}

#[test]
fn alias_over_a_binding_pattern_in_a_variant_payload_types_the_alias() {
    // The alias-over-binding shape that is still legal: a variant payload
    // field renamed with `ut`. The alias takes the payload field's type.
    test_support::assert_accepts(
        r"discretio Mandatum {
    Moveo { numerus passus }
}
functio probe(Mandatum m) → numerus {
    discerne m {
        casu Moveo fixum passus ut n { redde n + n }
    }
}",
    );
}

#[test]
fn alias_over_a_binding_pattern_in_a_variant_payload_keeps_the_payload_type() {
    test_support::assert_rejects_with_issue(
        r#"discretio Mandatum {
    Moveo { numerus passus }
}
functio probe(Mandatum m) → textus {
    discerne m {
        casu Moveo fixum passus ut n { redde n }
    }
}"#,
        "return_type_mismatch",
    );
}

#[test]
fn alias_over_a_binding_pattern_naming_a_non_member_is_rejected() {
    // Retargeted: `casu x ut y` over a `numerus` used to bind
    // the scrutinee as `x` and alias it as `y`. A bare arm name now names a
    // member of the closed set or it is an error, so the pair never reaches
    // typechecking as a binding pattern.
    let result = test_support::analyze_source(
        r"functio probe(numerus n) → numerus {
    discerne n {
        casu x ut y { redde y }
    }
}",
    );
    assert!(!result.is_ok(), "{:?}", result.diagnostics);
    let diag = result
        .diagnostics
        .iter()
        .find(|diag| diag.issue() == Some("discerne_arm_not_member"))
        .unwrap_or_else(|| panic!("diagnostic not found: {:?}", result.diagnostics));
    assert!(diag.args.contains(&DiagnosticArg::new("name", "x")));
}
