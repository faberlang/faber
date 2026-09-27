//! Tests for radix lane collection and metadata.
//!
//! These tests prove that declaration-level lane annotations are collected
//! into RadixLaneMetadata and that malformed or conflicting lane usage
//! produces expected diagnostics.

use super::{RadixLane, RadixLaneMetadata};
use crate::test_support::args_have_no_legacy_message;
use crate::{PassConfig, SemanticError, SemanticErrorKind};
use radix_target::Target;

fn analyze(source: &str) -> (RadixLaneMetadata, Vec<crate::SemanticError>) {
    let lex = radix_lexer::lex(source);
    let mut parse = radix_parser::parse(lex);
    let program = parse.program.expect("parse");
    let config = PassConfig::for_target(Target::MirWasm);
    let result = crate::analyze(&program, &config, &mut parse.interner);
    (result.radix_lanes, result.diagnostics)
}

fn expect_structured_error<'a>(
    errors: &'a [SemanticError],
    kind: SemanticErrorKind,
    expected_args: &[(&str, &str)],
) -> &'a SemanticError {
    let err = errors
        .iter()
        .find(|err| {
            err.kind == kind
                && expected_args.iter().all(|(name, value)| {
                    err.args
                        .iter()
                        .any(|arg| arg.name == *name && arg.value == *value)
                })
        })
        .expect("structured semantic diagnostic");
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
    err
}

#[test]
fn braced_radix_lane_attaches_to_function_metadata() {
    let (lanes, errors) = analyze(
        r#"
@ radix { lane = "mir" }
functio loss() → numerus {
    redde 1
}
"#,
    );

    assert!(
        !errors.iter().any(super::error::SemanticError::is_error),
        "unexpected hard errors: {errors:?}"
    );
    let collected = lanes.iter().map(|(_, lane)| lane).collect::<Vec<_>>();
    assert_eq!(collected, vec![RadixLane::Mir]);
}

#[test]
fn radix_lane_air_attaches_to_function_metadata() {
    let (lanes, errors) = analyze(
        r#"
@ radix lane "air"
functio loss() → numerus {
    redde 1
}
"#,
    );

    assert!(
        !errors.iter().any(super::error::SemanticError::is_error),
        "unexpected hard errors: {errors:?}"
    );
    let collected = lanes.iter().map(|(_, lane)| lane).collect::<Vec<_>>();
    assert_eq!(collected, vec![RadixLane::Air]);
}

#[test]
fn radix_lane_accepts_mir_and_hir_direct_values() {
    let (lanes, errors) = analyze(
        r#"
@ radix lane "mir"
functio lowerable() → numerus {
    redde 1
}

@ radix lane "hir-direct"
functio direct() → numerus {
    redde 2
}
"#,
    );

    assert!(
        !errors.iter().any(super::error::SemanticError::is_error),
        "unexpected hard errors: {errors:?}"
    );
    let mut collected = lanes
        .iter()
        .map(|(_, lane)| lane.as_str())
        .collect::<Vec<_>>();
    collected.sort_unstable();
    assert_eq!(collected, vec!["hir-direct", "mir"]);
}

#[test]
fn radix_lane_rejects_unknown_lane() {
    let (_lanes, errors) = analyze(
        r#"
@ radix lane "gpu"
functio loss() → numerus {
    redde 1
}
"#,
    );

    expect_structured_error(
        &errors,
        SemanticErrorKind::AnnotationInvalid,
        &[
            ("issue", "unknown_lane"),
            ("annotation", "@ radix lane"),
            ("lane", "gpu"),
        ],
    );
}

#[test]
fn radix_lane_rejects_unknown_identifier_lane_value() {
    let (_lanes, errors) = analyze(
        r"
@ radix lane gpu
functio loss() → numerus {
    redde 1
}
",
    );

    expect_structured_error(
        &errors,
        SemanticErrorKind::AnnotationInvalid,
        &[
            ("issue", "unknown_lane"),
            ("annotation", "@ radix lane"),
            ("lane", "gpu"),
        ],
    );
}

#[test]
fn braced_radix_lane_rejects_non_string_lane_value() {
    let (_lanes, errors) = analyze(
        r"
@ radix { lane = air }
functio loss() → numerus {
    redde 1
}
",
    );

    expect_structured_error(
        &errors,
        SemanticErrorKind::AnnotationInvalid,
        &[
            ("issue", "non_string_lane_value"),
            ("annotation", "@ radix lane"),
        ],
    );
}

#[test]
fn radix_annotation_rejects_unknown_token_directive() {
    let (_lanes, errors) = analyze(
        r#"
@ radix target "air"
functio loss() → numerus {
    redde 1
}
"#,
    );

    expect_structured_error(
        &errors,
        SemanticErrorKind::AnnotationInvalid,
        &[("issue", "unknown_directive"), ("annotation", "@ radix")],
    );
}

#[test]
fn braced_radix_typus_rejects_missing_allowed_domain() {
    let (_lanes, errors) = analyze(
        r"
@ radix { param = T }
functio load<T>(T value) → T {
    redde value
}
",
    );

    expect_structured_error(
        &errors,
        SemanticErrorKind::AnnotationInvalid,
        &[
            ("issue", "empty_radix_typus_domain"),
            ("annotation", "@ radix typus"),
        ],
    );
}

#[test]
fn radix_lane_rejects_unsupported_placement() {
    let (_lanes, errors) = analyze(
        r#"
@ radix lane "mir"
genus Model {
    varia numerus x
}
"#,
    );

    expect_structured_error(
        &errors,
        SemanticErrorKind::AnnotationInvalid,
        &[
            ("issue", "unsupported_placement"),
            ("annotation", "@ radix lane"),
            ("placement", "genus declaration"),
        ],
    );
}

// ---------------------------------------------------------------------------
// Backward annotation tests
// ---------------------------------------------------------------------------

#[test]
fn radix_backward_attaches_with_air_lane() {
    let (lanes, errors) = analyze(
        r#"
@ radix lane "air"
@ radix backward "multiplica_backward"
functio loss(tf32[2] a, tf32[2] b) → tf32[2] {
    redde a.multiplica(b)
}
"#,
    );

    assert!(
        !errors.iter().any(super::error::SemanticError::is_error),
        "unexpected hard errors: {errors:?}",
    );
    let collected = lanes.iter().map(|(_, lane)| lane).collect::<Vec<_>>();
    assert_eq!(collected, vec![RadixLane::Air]);

    // Verify backward metadata was collected.
    let backward_defs: Vec<_> = lanes.iter_backward().collect();
    assert_eq!(backward_defs.len(), 1, "expected one backward annotation");
}

#[test]
fn radix_backward_without_air_lane_rejects() {
    let (_lanes, errors) = analyze(
        r#"
@ radix lane "mir"
@ radix backward "backward_name"
functio loss(numerus x) → numerus {
    redde x
}
"#,
    );

    expect_structured_error(
        &errors,
        SemanticErrorKind::AnnotationInvalid,
        &[
            ("issue", "conflicting_backward_without_air"),
            ("annotation", "@ radix backward"),
        ],
    );
}

#[test]
fn radix_backward_missing_backward_name_rejects() {
    let (_lanes, errors) = analyze(
        r#"
@ radix lane "air"
@ radix backward
functio loss(tf32[2] a, tf32[2] b) → tf32[2] {
    redde a.multiplica(b)
}
"#,
    );

    expect_structured_error(
        &errors,
        SemanticErrorKind::AnnotationInvalid,
        &[
            ("issue", "missing_backward_name"),
            ("annotation", "@ radix backward"),
        ],
    );
}

#[test]
fn radix_backward_non_string_value_rejects() {
    let (_lanes, errors) = analyze(concat!(
        // ALLOW_DIAGNOSTIC_RENDER_TEXT: test input, not diagnostic assertion
        "@ radix lane \"air\"\n",
        "@ radix backward 42\n",
        "functio loss(tf32[2] a, tf32[2] b) → tf32[2] {\n",
        "    redde a.multiplica(b)\n",
        "}\n",
    ));

    expect_structured_error(
        &errors,
        SemanticErrorKind::AnnotationInvalid,
        &[
            ("issue", "non_string_backward_name"),
            ("annotation", "@ radix backward"),
        ],
    );
}

#[test]
fn radix_backward_extra_args_rejects() {
    let (_lanes, errors) = analyze(
        r#"
@ radix lane "air"
@ radix backward "name" "extra"
functio loss(tf32[2] a, tf32[2] b) → tf32[2] {
    redde a.multiplica(b)
}
"#,
    );

    expect_structured_error(
        &errors,
        SemanticErrorKind::AnnotationInvalid,
        &[
            ("issue", "extra_backward_args"),
            ("annotation", "@ radix backward"),
        ],
    );
}

#[test]
fn radix_backward_conflicting_annotations_rejects() {
    let (_lanes, errors) = analyze(
        r#"
@ radix lane "air"
@ radix backward "first_name"
@ radix backward "second_name"
functio loss(tf32[2] a, tf32[2] b) → tf32[2] {
    redde a.multiplica(b)
}
"#,
    );

    expect_structured_error(
        &errors,
        SemanticErrorKind::AnnotationInvalid,
        &[
            ("issue", "conflicting_backward"),
            ("annotation", "@ radix backward"),
        ],
    );
}

#[test]
fn radix_backward_unsupported_placement_rejects() {
    let (_lanes, errors) = analyze(
        r#"
@ radix backward "name"
genus Model {
    varia numerus x
}
"#,
    );

    expect_structured_error(
        &errors,
        SemanticErrorKind::AnnotationInvalid,
        &[
            ("issue", "unsupported_placement"),
            ("annotation", "@ radix backward"),
            ("placement", "genus declaration"),
        ],
    );
}

#[test]
fn radix_backward_no_backward_when_not_annotated() {
    let (lanes, errors) = analyze(
        r#"
@ radix lane "air"
functio loss(tf32[2] a, tf32[2] b) → tf32[2] {
    redde a.multiplica(b)
}
"#,
    );

    assert!(
        !errors.iter().any(super::error::SemanticError::is_error),
        "unexpected hard errors: {errors:?}"
    );
    let backward_defs: Vec<_> = lanes.iter_backward().collect();
    assert!(backward_defs.is_empty(), "expected no backward annotations");
}

#[test]
fn radix_backward_air_lane_collects_both_metadata() {
    let (lanes, errors) = analyze(
        r#"
@ radix lane "air"
@ radix backward "loss_backward"
functio loss(tf32[2] a, tf32[2] b) → tf32[2] {
    redde a.addita(b)
}

@ radix lane "mir"
functio no_backward(numerus x) → numerus {
    redde x
}
"#,
    );

    assert!(
        !errors.iter().any(super::error::SemanticError::is_error),
        "unexpected hard errors: {errors:?}"
    );

    // Should have two lane entries.
    let lanes_count = lanes.iter().count();
    assert_eq!(lanes_count, 2);

    // Should have exactly one backward entry.
    let backward_count = lanes.iter_backward().count();
    assert_eq!(backward_count, 1);
}
