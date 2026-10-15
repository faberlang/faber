//! Focused proofs for ARC-4 Tier-B closure lifting.

use crate::SemanticErrorKind;
use crate::test_support::analyze_source_for_target;
use radix_hir::HirItemKind;
use radix_target::Target;

fn air_errors(source: &str) -> Vec<crate::SemanticError> {
    analyze_source_for_target(source, Target::MirWasm)
        .result
        .diagnostics
        .into_iter()
        .filter(|error| error.kind == SemanticErrorKind::AirPurityViolation)
        .collect()
}

#[test]
fn capture_free_combine_becomes_an_air_module_item() {
    let fixture = analyze_source_for_target(
        r#"
@ radix lane "air"
functio loss(tf32[2, 2] x) → fractus {
    redde x.reducta((fractus a, fractus b) ∴ a + b, 0.0)
}
"#,
        Target::MirWasm,
    );
    assert!(
        fixture.result.success(),
        "capture-free combine should be admitted: {:?}",
        fixture.result.diagnostics
    );

    let hir = fixture.result.hir.as_ref().expect("typed HIR");
    let lifted = hir
        .items
        .iter()
        .filter(|item| {
            matches!(
                &item.kind,
                HirItemKind::Function(function)
                    if fixture
                        .interner
                        .resolve(function.name)
                        .starts_with("__air_closure_")
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(lifted.len(), 1, "one synthetic combine item is expected");
    assert_eq!(
        fixture.result.radix_lanes.lane_for(lifted[0].def_id),
        Some(crate::RadixLane::Air)
    );
}

#[test]
fn capture_decline_names_the_local_and_capture_span() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio loss(tf32[2, 2] x) → fractus {
    sit fractus seed ← 1.0
    redde x.reducta((fractus a, fractus b) ∴ a + b + seed, 0.0)
}
"#,
    );
    let error = errors
        .iter()
        .find(|error| error.issue() == Some("closure_capture"))
        .expect("capture-specific AIR decline");
    assert!(
        error
            .args
            .iter()
            .any(|arg| arg.name == "capture" && arg.value.contains("seed")),
        "capture name must be carried: {:?}",
        error.args
    );
    assert!(
        error
            .args
            .iter()
            .any(|arg| arg.name == "capture_span" && !arg.value.is_empty()),
        "capture span must be carried: {:?}",
        error.args
    );
}
