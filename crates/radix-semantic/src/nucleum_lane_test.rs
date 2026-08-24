use crate::test_support::analyze_source;

fn issues(source: &str) -> Vec<String> {
    analyze_source(source)
        .diagnostics
        .iter()
        .filter_map(|diagnostic| {
            diagnostic
                .args
                .iter()
                .find(|arg| arg.name == "issue")
                .map(|arg| arg.value.clone())
        })
        .collect()
}

#[test]
fn invalid_nucleum_lane_name_fails_closed() {
    assert!(
        issues("@ nucleum lane \"not-valid\"\nfunctio k() → vacuum { redde }\n")
            .contains(&"nucleum_lane_invalid_identifier".to_owned())
    );
}

#[test]
fn nucleum_lane_parameter_collision_is_structured() {
    assert!(
        issues("@ nucleum lane \"x\"\nfunctio k(fractus<f32> x) → fractus<f32> { redde x }\n")
            .contains(&"nucleum_lane_collision".to_owned())
    );
}

#[test]
fn duplicate_braced_nucleum_lane_clauses_conflict() {
    assert!(
        issues("@ nucleum { lane = \"x\", lane = \"y\" }\nfunctio k() → vacuum { redde }\n")
            .contains(&"nucleum_lane_conflicting".to_owned())
    );
}
