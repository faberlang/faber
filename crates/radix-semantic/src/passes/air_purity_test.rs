//! Coverage for AIR-lane purity constraints.
//!
//! These tests prove that the pass accepts pure scalar and tensor operations,
//! rejects mutation, deferred init, `ad` invocation, and side-effecting
//! control flow (`mori`, `iace`) in `"air"`-annotated functions.

use crate::test_support::args_have_no_legacy_message;
use crate::{PassConfig, SemanticError, SemanticErrorKind};
use radix_target::Target;

fn analyze(source: &str) -> Vec<crate::SemanticError> {
    let lex = radix_lexer::lex(source);
    let mut parse = radix_parser::parse(lex);
    let program = parse.program.expect("parse");
    let config = PassConfig::for_target(Target::MirWasm);
    crate::analyze(&program, &config, &mut parse.interner).diagnostics
}

fn air_errors(source: &str) -> Vec<crate::SemanticError> {
    analyze(source)
        .into_iter()
        .filter(|err| err.kind == SemanticErrorKind::AirPurityViolation)
        .collect()
}

fn expect_air_issue<'a>(errors: &'a [SemanticError], issue: &str) -> &'a SemanticError {
    let err = errors
        .iter()
        .find(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == issue)
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "lane" && arg.value == "air")
        })
        .expect("structured AIR purity diagnostic");
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
    err
}

#[test]
fn accepts_pure_scalar_air_function() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio loss(numerus x, numerus y) → numerus {
    fixum numerus sum ← x + y
    si sum > 0 {
        redde sum
    } secus {
        redde 0
    }
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn accepts_calls_to_other_air_lane_functions() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio square(numerus x) → numerus {
    redde x * x
}

@ radix lane "air"
functio loss(numerus x) → numerus {
    redde square(x)
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn accepts_v1_tensor_intrinsic_method_names_on_tensor_receivers() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio combine(tf32[2, 2] x, tf32[2, 2] y) → tf32[2, 2] {
    redde x.addita(y).multiplica(y)
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn rejects_tensor_intrinsic_method_names_on_scalar_receivers() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio combine(fractus x, fractus y) → fractus {
    redde x.addita(y)
}
"#,
    );

    let err = expect_air_issue(&errors, "tensor_method_receiver");
    assert!(err
        .args
        .iter()
        .any(|arg| arg.name == "method" && arg.value == "addita"));
}

#[test]
fn rejects_summa_reduce_expression_by_design() {
    // SFR-3 non_goals: AIR declines the summa reduce expression in v1 by
    // design (device admission only) — the pin test the row calls for.
    let errors = air_errors(
        r#"
@ radix lane "air"
functio loss(tensor<f32, [8]> a) → f32 {
    redde summa ex a apud [i] fixum s { redde s }
}
"#,
    );
    expect_air_issue(&errors, "summa_expression_unsupported");
}

#[test]
fn rejects_mutation_and_deferred_init() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio loss(numerus x) → numerus {
    varia numerus total ← x
    total ← total + 1
    redde total
}
"#,
    );

    expect_air_issue(&errors, "mutable_varia");
    expect_air_issue(&errors, "assignment_mutation");
}

#[test]
fn rejects_expression_ad_in_air_lane() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio send() → vacuum {
    redde ad 'runtime:echo' ("x")
}
"#,
    );

    expect_air_issue(&errors, "expression_ad");
}

#[test]
fn rejects_panic_and_throw() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio fail(bivalens bad) → numerus ⇥ textus {
    si bad {
        iace "bad"
    } secus {
        mori "worse"
    }
}
"#,
    );

    expect_air_issue(&errors, "iace");
    expect_air_issue(&errors, "mori");
}

#[test]
fn accepts_transpone_tensor_method_in_air_lane() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio trans(tf32[2, 3] x) → tf32[3, 2] {
    redde x.transpone()
}
"#,
    );

    assert!(
        errors.is_empty(),
        "unexpected AIR errors for transpone: {errors:?}"
    );
}

#[test]
fn accepts_forma_tensor_method_in_air_lane() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio reshape(tf32[6] x) → tf32[2, 3] {
    redde x.forma()
}
"#,
    );

    assert!(
        errors.is_empty(),
        "unexpected AIR errors for forma: {errors:?}"
    );
}

#[test]
fn accepts_exponentia_tensor_method_in_air_lane() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio exp_op(tf32[2, 2] x) → tf32[2, 2] {
    redde x.exponentia()
}
"#,
    );

    assert!(
        errors.is_empty(),
        "unexpected AIR errors for exponentia: {errors:?}"
    );
}

#[test]
fn accepts_logarithmus_tensor_method_in_air_lane() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio log_op(tf32[2, 2] x) → tf32[2, 2] {
    redde x.logarithmus()
}
"#,
    );

    assert!(
        errors.is_empty(),
        "unexpected AIR errors for logarithmus: {errors:?}"
    );
}

#[test]
fn accepts_activatio_relu_tensor_method_in_air_lane() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio relu_op(tf32[2, 2] x) → tf32[2, 2] {
    redde x.activatio_relu()
}
"#,
    );

    assert!(
        errors.is_empty(),
        "unexpected AIR errors for activatio_relu: {errors:?}"
    );
}

#[test]
fn accepts_radix_tensor_method_in_air_lane() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio sqrt_op(tf32[2, 2] x) → tf32[2, 2] {
    redde x.radix()
}
"#,
    );

    assert!(
        errors.is_empty(),
        "unexpected AIR errors for radix: {errors:?}"
    );
}

#[test]
fn accepts_gelu_tensor_method_in_air_lane() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio gelu_op(tf32[2, 2] x) → tf32[2, 2] {
    redde x.gelu()
}
"#,
    );

    assert!(
        errors.is_empty(),
        "unexpected AIR errors for gelu: {errors:?}"
    );
}

#[test]
fn accepts_activatio_tanh_tensor_method_in_air_lane() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio tanh_op(tf32[2, 2] x) → tf32[2, 2] {
    redde x.activatio_tanh()
}
"#,
    );

    assert!(
        errors.is_empty(),
        "unexpected AIR errors for activatio_tanh: {errors:?}"
    );
}

#[test]
fn accepts_activatio_softmax_tensor_method_in_air_lane() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio softmax_op(tf32[2, 2] x) → tf32[2, 2] {
    redde x.activatio_softmax()
}
"#,
    );

    assert!(
        errors.is_empty(),
        "unexpected AIR errors for activatio_softmax: {errors:?}"
    );
}

#[test]
fn accepts_crux_entropia_tensor_method_in_air_lane() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio ce_loss(tf32[2, 2] logits, tf32[2, 2] targets) → fractus {
    redde logits.crux_entropia(targets)
}
"#,
    );

    assert!(
        errors.is_empty(),
        "unexpected AIR errors for crux_entropia: {errors:?}"
    );
}

// ---------------------------------------------------------------------------
// ARC-1 (air-reducta-cumulata): red drivers pinning the decline paths.
// Named combines must themselves target the AIR lane; closure combines
// additionally hit the stable closure reject.
// ---------------------------------------------------------------------------

#[test]
fn air_lane_reducta_on_tensor_receiver_declines_for_non_air_combine() {
    let errors = air_errors(
        r#"
functio comb(fractus a, fractus b) → fractus {
    redde a + b
}

@ radix lane "air"
functio loss(tf32[2, 2] x) → fractus {
    redde x.reducta(comb, 0.0)
}
"#,
    );

    expect_air_issue(&errors, "non_air_call");
}

#[test]
fn air_lane_cumulata_on_tensor_receiver_declines_for_non_air_combine() {
    let errors = air_errors(
        r#"
functio comb(fractus a, fractus b) → fractus {
    redde a + b
}

@ radix lane "air"
functio loss(tf32[2, 2] x) → tf32[2, 2] {
    redde x.cumulata(comb, 0.0)
}
"#,
    );

    expect_air_issue(&errors, "non_air_call");
}

#[test]
fn air_lane_capture_free_closure_combine_is_lifted() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio loss(tf32[2, 2] x) → fractus {
    redde x.reducta((fractus a, fractus b) ∴ a + b, 0.0)
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn base_lane_cumulata_is_registered_on_lista_only() {
    // ARC-2: the base lane now owns the lista scan row, while tensor
    // admission remains reserved for ARC-3+.
    assert!(
        radix_intrinsics::lookup_method(radix_intrinsics::IntrinsicReceiver::Lista, "cumulata", 2,)
            .is_some(),
        "lista cumulata must be registered on the base lane"
    );
    assert!(
        radix_intrinsics::lookup_method(
            radix_intrinsics::IntrinsicReceiver::Tensor,
            "cumulata",
            2,
        )
        .is_none(),
        "tensor cumulata remains outside the base-lane registry"
    );

    let errors = analyze(
        r#"
functio probe(lista<fractus> xs) → lista<fractus> {
    redde xs.cumulata((fractus a, fractus b) ∴ a + b, 0.0)
}
"#,
    );

    assert!(
        !errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::AirPurityViolation),
        "base-lane cumulata must not reach any AIR admit path: {errors:?}"
    );
}

// ---------------------------------------------------------------------------
// ARC-1 admit fixtures — the post-goal state ARC-3 delivers: named-combine
// `reducta`/`cumulata` on typed tensor receivers are AIR-eligible, while
// closure combines keep the stable `closure` decline.
// ---------------------------------------------------------------------------

#[test]
fn air_lane_admits_named_combine_reducta_on_tensor_receiver() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio comb(fractus a, fractus b) → fractus {
    redde a + b
}

@ radix lane "air"
functio loss(tf32[2, 2] x) → fractus {
    redde x.reducta(comb, 0.0)
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn air_lane_admits_named_combine_cumulata_on_tensor_receiver() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio comb(fractus a, fractus b) → fractus {
    redde a + b
}

@ radix lane "air"
functio loss(tf32[2, 2] x) → tf32[2, 2] {
    redde x.cumulata(comb, 0.0)
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn air_lane_admits_named_mapper_mappata_on_tensor_receiver() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio map_one(fractus x) → fractus {
    redde x
}

@ radix lane "air"
functio map_tensor(tf32[2, 2] x) → tf32[2, 2] {
    redde x.mappata(map_one)
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn air_lane_admits_named_predicate_filtrata_on_tensor_receiver() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio keep(fractus x) → bivalens {
    redde x > 0.0
}

@ radix lane "air"
functio filter_tensor(tf32[2, 2] x) → tf32[2, 2] {
    redde x.filtrata(keep)
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn rejects_opaque_non_air_calls() {
    let errors = air_errors(
        r#"
functio helper(numerus x) → numerus {
    redde x
}

@ radix lane "air"
functio loss(numerus x) → numerus {
    redde helper(x)
}
"#,
    );

    expect_air_issue(&errors, "non_air_call");
}

// ---------------------------------------------------------------------------
// S3-A2 (council B3): a DIFFERENTIABLE device function (`@ nucleum`/`@ nucleum
// fragment` + `@ radix backward`) is admitted to the AIR lane — its AIR form
// generates the backward companion while its kernel form is the device
// forward. The purity gate still rejects plain kernels/fragments without a
// backward annotation (no over-relaxation).
// ---------------------------------------------------------------------------

#[test]
fn accepts_backward_annotated_kernel_in_air_lane() {
    // S3-B1 fixture shape: `@ nucleum` + `@ radix lane "air"` + `@ radix
    // backward` — a differentiable device kernel. Both semantic gates pass:
    // SEM018 (backward requires the air lane) and SEM059 (the air-purity gate
    // admits the backward-annotated kernel; the generated AIR stays pure).
    let errors = analyze(
        r#"
@ nucleum
@ radix lane "air"
@ radix backward "loss_backward"
functio simple_loss(tf32[2, 2] x, tf32[2, 2] w) → tf32[2, 2] {
    redde x.multiplica(w)
}
"#,
    );

    assert!(
        !errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::AirPurityViolation),
        "backward-annotated device kernel must pass the air-purity gate: {errors:?}"
    );
    assert!(
        !errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::AnnotationInvalid),
        "air-lane + backward must still pass SEM018: {errors:?}"
    );
}

#[test]
fn rejects_plain_kernel_in_air_lane() {
    // No over-relaxation: a kernel without a `@ radix backward` annotation is
    // NOT a differentiable device kernel and stays outside the v1 AIR subset.
    let errors = air_errors(
        r#"
@ nucleum
@ radix lane "air"
functio forward(tf32[2, 2] x) → tf32[2, 2] {
    redde x.multiplica(x)
}
"#,
    );

    expect_air_issue(&errors, "kernel_device_function");
}

#[test]
fn rejects_plain_nucleum_fragment_in_air_lane() {
    // `@ nucleum fragment` is a device helper; without a backward annotation
    // it is outside the v1 AIR subset (the fragment branch of the same
    // rejection).
    let errors = air_errors(
        r#"
@ nucleum fragment
@ radix lane "air"
functio forward(tf32[2, 2] x) → tf32[2, 2] {
    redde x.multiplica(x)
}
"#,
    );

    expect_air_issue(&errors, "kernel_device_function");
}

#[test]
fn rejects_conversio_assign_mutation_in_air_lane() {
    // NO-BYPASS: `↤` mutates its target place exactly like `←`, so AIR-lane
    // purity rejects the conversion-directed store with the same issue.
    let errors = air_errors(
        r#"
@ radix lane "air"
functio loss(numerus x) → numerus {
    varia numerus total ← x
    total ↤ "5"
    redde total
}
"#,
    );

    expect_air_issue(&errors, "mutable_varia");
    expect_air_issue(&errors, "assignment_mutation");
}
