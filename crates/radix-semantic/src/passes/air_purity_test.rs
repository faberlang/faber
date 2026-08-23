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

// ---------------------------------------------------------------------------
// ATH-1 (AIR Tier 0.5): AR-2 attachment-map pins and keep-out decline pins.
// Test-only unit — green at tip, no production change. Two families:
//
//   1. HIR carrier pins: `cape` rides `HirExpressionKind::Handled` or
//      `Si.then_catch` only; `Dum`/`Itera` carry no catch field and a loop
//      `cape` wraps the WHOLE loop expression as `Handled` (per-loop, never
//      re-armed per iteration).
//   2. AIR-lane decline pins at current behavior: per-arm
//      `conditional_recovery`, whole-construct `recoverable_handler`,
//      whole-loop under the loop-spectrum causes, and the unchanged
//      `iace`/`runtime_conversion`/`optional_null_chain` causes.
//
// Plus the air_unroll facts the goal's fact table pins read-only: the
// itera-body scan rejects `Handled` with causa `impure` (unroll runs before
// air_purity, so the decline surfaces from the unroll scan), and the
// `method_call` allowlist keep-out is untouched.
// ---------------------------------------------------------------------------

fn analyze_result(source: &str) -> crate::SemanticResult {
    let lex = radix_lexer::lex(source);
    let mut parse = radix_parser::parse(lex);
    let program = parse.program.expect("parse");
    let config = PassConfig::for_target(Target::MirWasm);
    crate::analyze(&program, &config, &mut parse.interner)
}

/// AR-2 structural facts collected from a lowered HIR tree.
#[derive(Default)]
struct Ar2Facts {
    handled: usize,
    si_then_catch: usize,
    handled_wrapping_dum: usize,
    handled_wrapping_itera: usize,
    handled_wrapping_block: usize,
}

impl radix_hir::visit::HirVisitor for Ar2Facts {
    fn visit_expr(&mut self, expr: &radix_hir::HirExpression) {
        match &expr.kind {
            radix_hir::HirExpressionKind::Handled { body, .. } => {
                self.handled += 1;
                if block_contains_kind(body, |k| matches!(k, radix_hir::HirExpressionKind::Dum(..)))
                {
                    self.handled_wrapping_dum += 1;
                }
                if block_contains_kind(body, |k| {
                    matches!(k, radix_hir::HirExpressionKind::Itera(..))
                }) {
                    self.handled_wrapping_itera += 1;
                }
                if !block_contains_kind(body, |k| {
                    matches!(
                        k,
                        radix_hir::HirExpressionKind::Dum(..)
                            | radix_hir::HirExpressionKind::Itera(..)
                    )
                }) {
                    self.handled_wrapping_block += 1;
                }
            }
            radix_hir::HirExpressionKind::Si { then_catch, .. } => {
                if then_catch.is_some() {
                    self.si_then_catch += 1;
                }
            }
            _ => {}
        }
        radix_hir::visit::walk_expr(self, expr);
    }
}

fn block_contains_kind(
    block: &radix_hir::HirBlock,
    pred: impl Fn(&radix_hir::HirExpressionKind) -> bool,
) -> bool {
    struct Contains<'a>(bool, &'a dyn Fn(&radix_hir::HirExpressionKind) -> bool);
    impl radix_hir::visit::HirVisitor for Contains<'_> {
        fn visit_expr(&mut self, expr: &radix_hir::HirExpression) {
            if (self.1)(&expr.kind) {
                self.0 = true;
            }
            radix_hir::visit::walk_expr(self, expr);
        }
    }
    let mut found = Contains(false, &pred);
    use radix_hir::visit::HirVisitor as _;
    found.visit_block(block);
    found.0
}

fn ar2_facts(source: &str) -> Ar2Facts {
    let result = analyze_result(source);
    let hir = result.hir.as_ref().expect("lowered HIR");
    let mut facts = Ar2Facts::default();
    radix_hir::visit::walk_program(&mut facts, hir);
    facts
}

#[test]
fn ar2_dum_and_itera_variants_carry_no_catch_field() {
    // Compile-time carrier-shape pin (AR-2): `Dum` is (cond, block) and
    // `Itera` is the 6-field mode+bindings+source+body form — neither has a
    // catch slot, so a per-iteration handler cannot be represented on the
    // loop node itself. This match fails to compile if either variant gains
    // (or drops) a field.
    let _shape_pin = |kind: &radix_hir::HirExpressionKind| match kind {
        radix_hir::HirExpressionKind::Dum(cond, block) => {
            let _: &Box<radix_hir::HirExpression> = cond;
            let _: &radix_hir::HirBlock = block;
        }
        radix_hir::HirExpressionKind::Itera(
            mode,
            bindings,
            index_def,
            index_name,
            source,
            body,
        ) => {
            let _: &radix_hir::HirIteraMode = mode;
            let _: &Vec<(radix_hir::DefId, radix_lexer::Symbol)> = bindings;
            let _: &radix_hir::DefId = index_def;
            let _: &radix_lexer::Symbol = index_name;
            let _: &Box<radix_hir::HirExpression> = source;
            let _: &radix_hir::HirBlock = body;
        }
        _ => unreachable!("carrier-shape pin only matches Dum/Itera"),
    };
}

#[test]
fn ar2_si_per_arm_cape_lowers_to_then_catch_carrier() {
    // Per-arm attachment: `cape` on a `si` then-arm rides `Si.then_catch`,
    // not a wrapping `Handled`.
    let facts = ar2_facts(
        r#"
functio f(bivalens bad) → numerus {
    si bad {
        redde 1
    } cape err {
        redde 0
    } secus {
        redde 2
    }
}
"#,
    );

    assert_eq!(facts.si_then_catch, 1, "then-arm cape must ride then_catch");
    assert_eq!(facts.handled, 0, "si per-arm cape must not wrap in Handled");
}

#[test]
fn ar2_fac_elige_cura_cape_lower_to_handled_whole_construct() {
    // Whole-construct attachment: `fac`/`elige`/`cura` cape wraps the
    // complete containing construct as one `Handled` node.
    let facts = ar2_facts(
        r#"
functio g(bivalens bad) → numerus {
    fac {
        si bad { iace "x" }
        redde 1
    } cape err { redde 0 }

    elige bad {
        casu verum { redde 1 }
        ceterum { redde 2 }
    } cape err { redde 0 }

    cura "arena" fixum _ res { redde 1 } cape err { redde 0 }
    redde 3
}
"#,
    );

    assert_eq!(
        facts.handled, 3,
        "fac/elige/cura cape each lower to one Handled"
    );
    assert_eq!(
        facts.handled_wrapping_block, 3,
        "whole-construct handlers wrap non-loop bodies"
    );
    assert_eq!(
        facts.handled_wrapping_dum + facts.handled_wrapping_itera,
        0,
        "no loop is involved in whole-construct handlers"
    );
}

#[test]
fn ar2_dum_itera_cape_wrap_the_whole_loop_once() {
    // Per-loop attachment: exactly one trailing `Handled` wraps the whole
    // `dum`/`itera` expression; the loop itself carries no handler field.
    let facts = ar2_facts(
        r#"
functio h(numerus n) → numerus {
    fixum numerus total ← 0
    dum n > 0 {
        total ← total + 1
    } cape err { redde 0 }

    itera ab 0‥4 fixum i {
        total ← total + i
    } cape err { redde 0 }
    redde total
}
"#,
    );

    assert_eq!(facts.handled, 2, "one Handled per loop cape");
    assert_eq!(facts.handled_wrapping_dum, 1);
    assert_eq!(facts.handled_wrapping_itera, 1);
    assert_eq!(
        facts.si_then_catch, 0,
        "loop cape never rides Si.then_catch"
    );
}

#[test]
fn air_lane_declines_si_per_arm_cape_as_conditional_recovery() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio loss(bivalens bad) → numerus {
    si bad {
        redde 1
    } cape err {
        redde 0
    } secus {
        redde 2
    }
}
"#,
    );

    expect_air_issue(&errors, "conditional_recovery");
}

#[test]
fn air_lane_declines_whole_construct_cape_as_recoverable_handler() {
    let fac = air_errors(
        r#"
@ radix lane "air"
functio a(bivalens bad) → numerus {
    fac {
        si bad { iace "x" }
        redde 1
    } cape err { redde 0 }
    redde 2
}
"#,
    );
    expect_air_issue(&fac, "recoverable_handler");

    let elige = air_errors(
        r#"
@ radix lane "air"
functio b(bivalens bad) → numerus {
    elige bad {
        casu verum { redde 1 }
        ceterum { redde 2 }
    } cape err { redde 0 }
    redde 3
}
"#,
    );
    expect_air_issue(&elige, "recoverable_handler");

    let cura = air_errors(
        r#"
@ radix lane "air"
functio c() → numerus {
    cura "arena" fixum _ res { redde 1 } cape err { redde 0 }
    redde 1
}
"#,
    );
    expect_air_issue(&cura, "recoverable_handler");
}

#[test]
fn air_lane_declines_whole_loop_cape_under_loop_spectrum_causes() {
    // Loop keep-out: the containing form's own ruling applies — a cape on
    // `dum`/`itera` declines under the loop-spectrum cause, not as an
    // admitted handler.
    let dum = air_errors(
        r#"
@ radix lane "air"
functio d(numerus n) → numerus {
    fixum numerus total ← 0
    dum n > 0 {
        total ← total + 1
    } cape err { redde 0 }
    redde total
}
"#,
    );
    expect_air_issue(&dum, "dum_loop");

    let itera = air_errors(
        r#"
@ radix lane "air"
functio e(numerus n) → numerus {
    fixum numerus total ← 0
    itera ab 0‥4 fixum i {
        total ← total + i
    } cape err { redde 0 }
    redde total
}
"#,
    );
    expect_air_issue(&itera, "itera_loop");
}

#[test]
fn air_lane_iace_runtime_conversion_and_optional_chain_causes_unchanged() {
    let iace = air_errors(
        r#"
@ radix lane "air"
functio f(numerus x) → numerus ⇥ textus {
    si x < 0 { iace "neg" }
    redde x
}
"#,
    );
    expect_air_issue(&iace, "iace");

    let conversio = air_errors(
        r#"
@ radix lane "air"
functio g() → numerus {
    fixum numerus v ← 1 ↦ numerus
    redde v
}
"#,
    );
    expect_air_issue(&conversio, "runtime_conversion");

    let optional = air_errors(
        r#"
@ radix lane "air"
functio h() → numerus {
    fixum lista<numerus> xs ← [1, 2, 3]
    redde xs?[0]
}
"#,
    );
    expect_air_issue(&optional, "optional_null_chain");
}

#[test]
fn air_unroll_scan_rejects_handled_in_itera_body_causa_impure() {
    // air_unroll fact pin (read-only): the unroll pass runs before
    // air_purity and its itera-body scan rejects `Handled` with causa
    // `impure` (CAUSA_IMPURE) — the decline surfaces as SEM059.itera_loop
    // with that causa, proving the unroll scan fired on the loop body.
    let errors = air_errors(
        r#"
@ radix lane "air"
functio loop_with_handler() → f32 {
    fixum f32 total ← 0.0
    itera ab 0‥4 fixum i {
        fac {
            iace "boom"
        } cape err {
            redde 0.0
        }
    }
    redde total
}
"#,
    );

    let err = expect_air_issue(&errors, "itera_loop");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "causa" && arg.value == "impure"),
        "unrolled-loop body scan must reject Handled with causa impure: {:?}",
        err.args
    );
}

#[test]
fn air_lane_method_call_allowlist_keep_out_unchanged() {
    // air_unroll/allowlist fact pin (read-only): handler work adds no
    // `method_call` allowlist rows — an arbitrary method on a typed tensor
    // receiver still declines with the stable `method_call` cause.
    let errors = air_errors(
        r#"
@ radix lane "air"
functio loss(tf32[2, 2] x) → tf32[2, 2] {
    redde x.foobar()
}
"#,
    );

    let err = expect_air_issue(&errors, "method_call");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "method" && arg.value == "foobar"),
        "method_call decline must name the method: {:?}",
        err.args
    );
}
