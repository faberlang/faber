//! The removed `@ nucleum lane` binding (gpu-reset rule 12 point 5).
//!
//! Every spelling of the old surface must fail with the migration diagnostic,
//! never parse as its former meaning.

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

fn removed_name(source: &str) -> Option<String> {
    analyze_source(source)
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.issue() == Some("nucleum_lane_removed"))
        .and_then(|diagnostic| {
            diagnostic
                .args
                .iter()
                .find(|arg| arg.name == "name")
                .map(|arg| arg.value.clone())
        })
}

#[test]
fn sugar_lane_binding_is_removed() {
    let source = "@ nucleum lane \"id\"\nfunctio k() → vacuum { redde }\n";
    assert!(issues(source).contains(&"nucleum_lane_removed".to_owned()));
    assert_eq!(removed_name(source).as_deref(), Some("id"));
}

#[test]
fn fragment_sugar_lane_binding_is_removed() {
    let source = "@ nucleum fragment lane \"pos\"\nfunctio k() → vacuum { redde }\n";
    assert_eq!(removed_name(source).as_deref(), Some("pos"));
}

#[test]
fn braced_lane_binding_is_removed() {
    let source = "@ nucleum { lane = \"id\" }\nfunctio k() → vacuum { redde }\n";
    assert_eq!(removed_name(source).as_deref(), Some("id"));
}

#[test]
fn braced_lane_binding_after_fragment_is_removed() {
    let source = "@ nucleum { fragment = verum, lane = \"pos\" }\nfunctio k() → vacuum { redde }\n";
    assert_eq!(removed_name(source).as_deref(), Some("pos"));
}

#[test]
fn a_kernel_without_a_lane_binding_parses() {
    assert!(
        !issues("@ nucleum\nfunctio k() → vacuum { redde }\n")
            .contains(&"nucleum_lane_removed".to_owned())
    );
}
