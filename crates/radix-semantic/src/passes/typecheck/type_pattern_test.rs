//! N1 type-pattern grammar, arm narrowing, and scrutinee-directed selection.

use crate::test_support;

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
fn alias_over_a_binding_pattern_types_the_inner_binding() {
    test_support::assert_accepts(
        r"functio probe(numerus n) → numerus {
    discerne n {
        casu x ut y { redde x + y }
    }
}",
    );
}
