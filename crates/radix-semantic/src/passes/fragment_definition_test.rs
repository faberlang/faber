//! Coverage for nucleum-fragment definition checking.
//!
//! These tests prove the V1 device-safe effect subset: allowed scalar/tensor
//! bodies, explicit argument writes, fragment and device-intrinsic calls; and
//! forbidden postures, host effects, illegal role calls, and dynamic values.

use super::fragment_definition::{DeviceEffectKind, FragmentDefinitionReport, check};
use crate::test_support::args_have_no_legacy_message;
use crate::{PassConfig, SemanticError, SemanticErrorKind};
use radix_target::Target;

const ISSUE_EFFECT: &str = "nucleum_fragment_effect";
const ISSUE_CONTEXT: &str = "nucleum_fragment_context";

fn analyze_source(source: &str) -> crate::SemanticResult {
    let lex = radix_lexer::lex(source);
    let mut parse = radix_parser::parse(lex);
    let program = parse.program.expect("parse");
    let config = PassConfig::for_target(Target::MirWgsl);
    crate::analyze(&program, &config, &mut parse.interner)
}

fn check_source(source: &str) -> FragmentDefinitionReport {
    let lex = radix_lexer::lex(source);
    let mut parse = radix_parser::parse(lex);
    let program = parse.program.expect("parse");
    let config = PassConfig::for_target(Target::MirWgsl);
    let result = crate::analyze(&program, &config, &mut parse.interner);
    let hir = result.hir.as_ref().expect("hir after analyze");
    check(hir, &result.resolver, &result.types, &parse.interner)
}

fn expect_issue<'a>(errors: &'a [SemanticError], issue: &str, reason: &str) -> &'a SemanticError {
    let err = errors
        .iter()
        .find(|err| {
            err.kind == SemanticErrorKind::OperandTypesInvalid
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == issue)
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "reason" && arg.value == reason)
        })
        .unwrap_or_else(|| panic!("expected {issue} reason={reason}, got {errors:?}"));
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
    err
}

fn first_fragment_def(report: &FragmentDefinitionReport) -> radix_hir::DefId {
    report
        .summaries
        .iter()
        .next()
        .map(|(id, _)| id)
        .expect("expected a fragment summary")
}

#[test]
fn accepts_scalar_ops_structured_control_and_locals() {
    let report = check_source(
        r"
@ nucleum fragment
functio helper(numerus x) → numerus {
    sit numerus doubled ← x * 2
    si doubled ≻ 0 {
        redde doubled
    } secus {
        redde 0
    }
}
",
    );
    assert!(
        report.errors.is_empty(),
        "unexpected fragment errors: {:?}",
        report.errors
    );
    assert!(report.summaries.get(first_fragment_def(&report)).is_some());
}

#[test]
fn accepts_tensor_ops_supported_in_device_mir() {
    let report = check_source(
        r"
@ nucleum fragment
functio combine(tf32[2, 2] x, tf32[2, 2] y) → tf32[2, 2] {
    redde (x + y) ⊙ y
}
",
    );
    assert!(
        report.errors.is_empty(),
        "unexpected fragment errors: {:?}",
        report.errors
    );
    let summary = report
        .summaries
        .get(first_fragment_def(&report))
        .expect("summary");
    // Glyphs are binary expressions, not receiver intrinsic calls. Their
    // three operand reads must retain source spans without invented effects.
    assert_eq!(
        summary
            .effects
            .iter()
            .map(|effect| effect.kind)
            .collect::<Vec<_>>(),
        vec![DeviceEffectKind::ArgumentRead; 3],
        "expected only glyph operand reads, got {:?}",
        summary.effects
    );
    assert!(
        summary
            .effects
            .iter()
            .all(|effect| effect.span != radix_lexer::Span::default()),
        "glyph operand reads must retain source spans"
    );
}

#[test]
fn accepts_explicit_argument_writes() {
    let report = check_source(
        r"
@ nucleum fragment
functio scale(varia numerus value) → numerus {
    value ← value * 2
    redde value
}
",
    );
    assert!(
        report.errors.is_empty(),
        "unexpected fragment errors: {:?}",
        report.errors
    );
    let summary = report
        .summaries
        .get(first_fragment_def(&report))
        .expect("summary");
    assert!(
        summary
            .effects
            .iter()
            .any(|effect| effect.kind == DeviceEffectKind::ArgumentWrite
                && effect.span != radix_lexer::Span::default()),
        "expected argument-write span, got {:?}",
        summary.effects
    );
}

#[test]
fn accepts_approved_device_intrinsic() {
    let report = check_source(
        r"
@ nucleum fragment
functio helper() → u32 {
    redde workgroup_size_y()
}
",
    );
    assert!(
        report.errors.is_empty(),
        "unexpected fragment errors: {:?}",
        report.errors
    );
    let summary = report
        .summaries
        .get(first_fragment_def(&report))
        .expect("summary");
    assert!(
        summary
            .effects
            .iter()
            .any(|effect| effect.kind == DeviceEffectKind::DeviceIntrinsic),
        "expected device-intrinsic summary, got {:?}",
        summary.effects
    );
}

#[test]
fn accepts_call_to_another_fragment() {
    let report = check_source(
        r"
@ nucleum fragment
functio inner(numerus x) → numerus {
    redde x + 1
}

@ nucleum fragment
functio outer(numerus x) → numerus {
    redde inner(x)
}
",
    );
    assert!(
        report.errors.is_empty(),
        "unexpected fragment errors: {:?}",
        report.errors
    );
    assert!(
        report.summaries.iter().any(|(_, summary)| {
            summary
                .effects
                .iter()
                .any(|effect| effect.kind == DeviceEffectKind::FragmentCall)
        }),
        "expected fragment-call effect, summaries={:?}",
        report.summaries.iter().map(|(_, s)| s).collect::<Vec<_>>()
    );
}

#[test]
fn does_not_check_entry_bodies() {
    let report = check_source(
        r"
@ nucleum
functio k(numerus x) → numerus {
    scribe x
    redde x
}
",
    );
    assert!(
        report.errors.is_empty(),
        "Entry bodies are not this pass: {:?}",
        report.errors
    );
    assert!(report.summaries.iter().next().is_none());
}

#[test]
fn rejects_alternate_exit_posture() {
    let report = check_source(
        r"
@ nucleum fragment
functio helper(numerus x) → numerus ⇥ textus {
    redde x
}
",
    );
    expect_issue(&report.errors, ISSUE_EFFECT, "alternate_exit");
}

#[test]
fn rejects_async_posture() {
    let report = check_source(
        r"
@ nucleum fragment
functio helper(numerus x) fiet → numerus {
    redde x
}
",
    );
    expect_issue(&report.errors, ISSUE_EFFECT, "async_posture");
}

#[test]
fn rejects_generator_posture() {
    let report = check_source(
        r"
@ nucleum fragment
functio helper(numerus x) fiunt → numerus {
    redde x
}
",
    );
    expect_issue(&report.errors, ISSUE_EFFECT, "generator_posture");
}

#[test]
fn analyze_registers_definition_effect() {
    let result = analyze_source(
        r"
@ nucleum fragment
functio helper(numerus x) → numerus {
    scribe x
    redde x
}
",
    );
    expect_issue(&result.diagnostics, ISSUE_EFFECT, "host_io");
}

#[test]
fn rejects_host_io() {
    let report = check_source(
        r"
@ nucleum fragment
functio helper(numerus x) → numerus {
    scribe x
    redde x
}
",
    );
    expect_issue(&report.errors, ISSUE_EFFECT, "host_io");
}

#[test]
fn rejects_host_runtime_helper() {
    let report = check_source(
        r#"
@ nucleum fragment
functio helper() → vacuum {
    redde ad 'runtime:echo' ("x")
}
"#,
    );
    expect_issue(&report.errors, ISSUE_EFFECT, "host_runtime");
}

#[test]
fn rejects_host_function_call_at_definition_site() {
    let report = check_source(
        r"
functio host_add(numerus x) → numerus {
    redde x + 1
}

@ nucleum fragment
functio helper(numerus x) → numerus {
    redde host_add(x)
}
",
    );
    let err = expect_issue(&report.errors, ISSUE_CONTEXT, "host_callable");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "site" && arg.value == "definition"),
        "expected definition-site CONTEXT, got {:?}",
        err.args
    );
}

#[test]
fn rejects_entry_callee_at_definition_site() {
    let report = check_source(
        r"
@ nucleum
functio k(numerus x) → numerus {
    redde x
}

@ nucleum fragment
functio helper(numerus x) → numerus {
    redde k(x)
}
",
    );
    expect_issue(&report.errors, ISSUE_CONTEXT, "entry_callee");
}

#[test]
fn rejects_dynamic_function_value() {
    let report = check_source(
        r"
functio host_add(numerus x) → numerus {
    redde x + 1
}

@ nucleum fragment
functio helper() → vacuum {
    sit _ f ← host_add
    redde
}
",
    );
    expect_issue(&report.errors, ISSUE_EFFECT, "dynamic_function");
}

#[test]
fn device_methods_are_a_derived_subset_of_tensor_intrinsics() {
    use super::air_tensor_intrinsics::allowed_tensor_intrinsics;
    use super::fragment_definition::{DEVICE_EXCLUDED, allowed_device_methods};
    use std::collections::BTreeSet;

    let tensor: BTreeSet<&str> = allowed_tensor_intrinsics().iter().copied().collect();
    let device: BTreeSet<&str> = allowed_device_methods().iter().copied().collect();

    // The device vocabulary is derived, never an independent literal list:
    // every device method must be an admitted AIR tensor intrinsic.
    assert!(
        device.is_subset(&tensor),
        "device methods must be a subset of the tensor intrinsics; extra: {:?}",
        device.difference(&tensor).collect::<Vec<_>>()
    );

    // The tensor/device delta is exactly the explicit exclusion set — no name
    // may be silently dropped from the device surface.
    let delta: BTreeSet<&str> = tensor.difference(&device).copied().collect();
    let excluded: BTreeSet<&str> = DEVICE_EXCLUDED.iter().copied().collect();
    assert_eq!(
        delta.len(),
        8,
        "device/tensor delta must be the 8-name DEVICE_EXCLUDED set, got {delta:?}"
    );
    assert_eq!(
        delta, excluded,
        "device/tensor delta must equal DEVICE_EXCLUDED"
    );
}
