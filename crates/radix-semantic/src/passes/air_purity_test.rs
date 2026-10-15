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

/// Guard for every fixture handed to [`air_errors`]: analysis skips the AIR
/// passes once typecheck has reported an error (their HIR types would be
/// unreliable), which leaves the AIR diagnostics empty and makes every
/// `is_empty` / `!has_air_issue` assertion vacuous. A fixture that
/// intentionally carries a typecheck error needs its own explicitly named
/// helper, never this assertion.
///
/// Diagnostics that only post-AIR passes can emit are exempt: they cannot
/// have suppressed the AIR passes, so an AIR observation made beside them is
/// still real. Every other error still fails this guard.
#[track_caller]
fn assert_no_typecheck_error(diagnostics: &[SemanticError]) {
    let pre_air_errors: Vec<_> = diagnostics
        .iter()
        .filter(|err| {
            err.is_error()
                && err.kind != SemanticErrorKind::AirPurityViolation
                && !is_post_air_only(err)
        })
        .collect();
    assert!(
        pre_air_errors.is_empty(),
        "fixture must typecheck before AIR assertions: {pre_air_errors:?}"
    );
}

/// Emitted only by passes scheduled after the AIR gate: borrow mode analysis
/// (`tensor_view_escape`) and definite-assignment / borrow write-once
/// (`immutable_second_assignment`). Neither can skip the AIR passes.
fn is_post_air_only(err: &SemanticError) -> bool {
    matches!(
        (err.kind, err.issue()),
        (SemanticErrorKind::ModeMismatch, Some("tensor_view_escape"))
            | (
                SemanticErrorKind::ImmutableAssignment,
                Some("immutable_second_assignment")
            )
    )
}

fn air_errors(source: &str) -> Vec<crate::SemanticError> {
    let diagnostics = analyze(source);
    assert_no_typecheck_error(&diagnostics);
    diagnostics
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
functio loss(i32 x, i32 y) → numerus {
    sit numerus sum ← x + y
    si sum ≻ 0 {
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
functio square(i32 x) → numerus {
    redde x * x
}

@ radix lane "air"
functio loss(i32 x) → numerus {
    redde square(x)
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn accepts_tensor_arithmetic_glyphs_on_tensor_receivers() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio combine(tf32[2, 2] x, tf32[2, 2] y) → tf32[2, 2] {
    redde (x + y) ⊙ y
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn accepts_scalar_numeric_twin_as_its_operator() {
    // ss1: `x.addita(y)` on a scalar is exactly `x + y`, so it is the pure
    // operator the lane already admits, not a tensor method call.
    let errors = air_errors(
        r#"
@ radix lane "air"
functio combine(fractus x, fractus y) → fractus {
    redde x.addita(y)
}
"#,
    );
    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn tensor_intrinsic_method_names_on_collection_receivers_fail_in_typecheck() {
    // need: a list receiver is a closed method surface, so
    // `x.gelu()` is `unknown_method` at check time and never reaches the AIR
    // purity pass (same posture as the scalar receivers below).
    let source = r#"
@ radix lane "air"
functio combine(lista<fractus> x) → lista<fractus> {
    redde x.gelu()
}
"#;
    let errors = analyze(source);
    assert!(errors.iter().any(|err| {
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "unknown_method")
    }));
    assert!(
        !errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::AirPurityViolation)
    );
}

#[test]
fn tensor_intrinsic_method_names_on_scalar_receivers_fail_in_typecheck() {
    // 433f616de: a scalar numeric receiver is a closed method surface, so
    // `x.gelu()` is `unknown_method` at check time and never reaches the AIR
    // purity pass.
    let source = r#"
@ radix lane "air"
functio combine(fractus x) → fractus {
    redde x.gelu()
}
"#;
    let errors = analyze(source);
    assert!(errors.iter().any(|err| {
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "unknown_method")
    }));
    assert!(
        !errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::AirPurityViolation)
    );
}

#[test]
fn rejects_reducta_reduce_expression_by_design() {
    // SFR-3 non_goals: AIR declines reduction expressions in v1 by design
    // (device admission only) — the pin test the row calls for.
    let errors = air_errors(
        r#"
@ radix lane "air"
functio loss(tensor<f32, [8]> a) → f32 {
    redde reducta via summa ex a apud [i] fixum s { redde s }
}
"#,
    );
    expect_air_issue(&errors, "reducta_expression_unsupported");
}

#[test]
fn rejects_mutation_and_deferred_init() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio loss(numerus x) → numerus {
    varia numerus total ← x
    total ← 1
    redde total
}
"#,
    );

    expect_air_issue(&errors, "mutable_varia");
    expect_air_issue(&errors, "assignment_mutation");
}

#[test]
fn rejects_expression_ad_in_air_lane() {
    // `ad` types as `sermo`, so this probe typechecks cleanly — the decline
    // is proven in a healthy compilation, not stacked on a type error
    // (air_purity is skipped once typecheck has failed).
    let errors = air_errors(
        r#"
@ radix lane "air"
functio send() → sermo {
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
    redde x.forma([2, 3])
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
fn accepts_silu_tensor_method_in_air_lane() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio silu_op(tf32[2, 2] x) → tf32[2, 2] {
    redde x.silu()
}
"#,
    );

    assert!(
        errors.is_empty(),
        "unexpected AIR errors for silu: {errors:?}"
    );
}

#[test]
fn air_allowlist_admits_activatio_tanh_row() {
    // `activatio_tanh` is an AIR-lane allowlist row (`AirTensorOp::Tanh`),
    // but the source tensor catalog deliberately does not own it: a fixture
    // calling `x.activatio_tanh()` never typechecks (`unknown_method`), so
    // the old probe only ever observed an empty, AIR-suppressed list. Pin the
    // admission row directly instead of a fixture that cannot reach the AIR
    // passes.
    assert!(
        super::air_tensor_intrinsics::is_allowed_tensor_intrinsic("activatio_tanh"),
        "AIR-lane tanh row must stay admitted"
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
        r"
functio probe(lista<fractus> xs) → lista<fractus> {
    redde xs.cumulata((fractus a, fractus b) ∴ a + b, 0.0)
}
",
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
    redde x ≻ 0.0
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
    redde x ⊙ w
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
    redde x ⊙ x
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
    redde x ⊙ x
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
functio loss(numerus x) → numerus ⇥ textus {
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
                    matches!(k, radix_hir::HirExpressionKind::Itera { .. })
                }) {
                    self.handled_wrapping_itera += 1;
                }
                if !block_contains_kind(body, |k| {
                    matches!(
                        k,
                        radix_hir::HirExpressionKind::Dum(..)
                            | radix_hir::HirExpressionKind::Itera { .. }
                    )
                }) {
                    self.handled_wrapping_block += 1;
                }
            }
            radix_hir::HirExpressionKind::Si { then_catch, .. } if then_catch.is_some() => {
                self.si_then_catch += 1;
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
    // `Itera` is the mode+bindings+source+body form (its `filum` slot is
    // declared metadata, never a catch) — neither has a catch slot, so a
    // per-iteration handler cannot be represented on the loop node itself.
    // This match fails to compile if either variant gains (or drops) a field.
    let _ = |kind: &radix_hir::HirExpressionKind| match kind {
        radix_hir::HirExpressionKind::Dum(cond, block) => {
            // Carrier-shape pin: the field really is Box<HirExpression>.
            #[allow(clippy::borrowed_box)]
            let _: &Box<radix_hir::HirExpression> = cond;
            let _: &radix_hir::HirBlock = block;
        }
        radix_hir::HirExpressionKind::Itera {
            mode,
            coordinates: bindings,
            filum: None,
            binding: index_def,
            binding_name: index_name,
            iter: source,
            body,
            mutable: false,
        } => {
            let _: &radix_hir::HirIteraMode = mode;
            let _: &Vec<(radix_hir::DefId, radix_lexer::Symbol)> = bindings;
            let _: &radix_hir::DefId = index_def;
            let _: &radix_lexer::Symbol = index_name;
            // Carrier-shape pin: the field really is Box<HirExpression>.
            #[allow(clippy::borrowed_box)]
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
        r"
functio f(bivalens bad) → numerus {
    si bad {
        redde 1
    } cape err {
        redde 0
    } secus {
        redde 2
    }
}
",
    );

    assert_eq!(facts.si_then_catch, 1, "then-arm cape must ride then_catch");
    assert_eq!(facts.handled, 0, "si per-arm cape must not wrap in Handled");
}

#[test]
fn ar2_fac_elige_cape_lower_to_handled_whole_construct() {
    // Whole-construct attachment: `fac`/`elige` cape wraps the
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

    redde 3
}
"#,
    );

    assert_eq!(facts.handled, 2, "fac/elige cape each lower to one Handled");
    assert_eq!(
        facts.handled_wrapping_block, 2,
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
        r"
functio h(numerus n) → numerus {
    sit numerus total ← 0
    dum n ≻ 0 {
        total ← 1
    } cape err { redde 0 }

    itera ab 0‥4 fixum i {
        total ← i
    } cape err { redde 0 }
    redde total
}
",
    );

    assert_eq!(facts.handled, 2, "one Handled per loop cape");
    assert_eq!(facts.handled_wrapping_dum, 1);
    assert_eq!(facts.handled_wrapping_itera, 1);
    assert_eq!(
        facts.si_then_catch, 0,
        "loop cape never rides Si.then_catch"
    );
}

// ATH-2 (red→green flip of the ATH-1 decline pins): the per-arm and
// whole-construct handlers above now ADMIT when locally resolved — recovery
// arm resolves to the outer type and every throw is caught in-region. The
// keep-out pins (loop cape, unroll scan, method allowlist, unhandled `iace`,
// bare conversion, `optional_null_chain`) below stay green unchanged.

#[test]
fn air_lane_admits_si_per_arm_cape_when_locally_resolved() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio then_arm(bivalens bad) → numerus {
    si bad {
        iace "x"
    } cape err {
        redde 0
    } secus {
        redde 2
    }
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn air_lane_admits_secus_arm_cape_when_locally_resolved() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio secus_arm(bivalens bad) → numerus {
    si bad {
        redde 1
    } secus {
        iace "x"
    } cape err {
        redde 0
    }
    redde 3
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn air_lane_admits_sin_arm_cape_when_locally_resolved() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio sin_arm(bivalens bad) → numerus {
    si bad {
        redde 1
    } sin bad {
        iace "x"
    } cape err {
        redde 0
    }
    redde 3
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn air_lane_admits_whole_construct_fac_and_elige_cape() {
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
    assert!(fac.is_empty(), "unexpected AIR errors: {fac:?}");

    let elige = air_errors(
        r#"
@ radix lane "air"
functio b(bivalens bad) → numerus {
    elige bad {
        casu verum { iace "x" }
        ceterum { redde 2 }
    } cape err { redde 0 }
    redde 3
}
"#,
    );
    assert!(elige.is_empty(), "unexpected AIR errors: {elige:?}");
}

#[test]
fn air_lane_declines_recovery_arm_type_mismatch() {
    // Statement form: the region's normal exit is a numerus value (redde 1),
    // but the recovery arm exits without a value of that type.
    let handled = air_errors(
        r#"
@ radix lane "air"
functio d(bivalens bad) → numerus {
    fac {
        si bad { iace "x" }
        redde 1
    } cape err { sit numerus z ← 0 }
    redde 2
}
"#,
    );
    expect_air_issue(&handled, "recovery_type_mismatch");

    // Per-arm form: the then arm completes with a numerus value, so the
    // recovery arm must resolve to the same value type.
    let then_catch = air_errors(
        r#"
@ radix lane "air"
functio e(bivalens bad) → numerus {
    si bad {
        redde 1
    } cape err { sit numerus z ← 0 } secus {
        redde 2
    }
}
"#,
    );
    expect_air_issue(&then_catch, "recovery_type_mismatch");
}

#[test]
fn air_lane_declines_in_catch_rethrow() {
    // A throw from inside the catch arm is a rethrow — Tier 2, fail-closed
    // with its own named cause (never silently treated as a recovered value).
    let errors = air_errors(
        r#"
@ radix lane "air"
functio f(bivalens bad) → numerus ⇥ textus {
    fac {
        si bad { iace "x" }
        redde 1
    } cape err {
        iace "rethrown"
    }
    redde 2
}
"#,
    );

    expect_air_issue(&errors, "rethrow_in_recovery");
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
    varia numerus total ← 0
    dum n ≻ 0 {
        total ← 1
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
    varia numerus total ← 0
    itera ab 0‥4 fixum i {
        total ← i
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
    si x ≺ 0 { iace "neg" }
    redde x
}
"#,
    );
    expect_air_issue(&iace, "iace");

    let conversio = air_errors(
        r#"
@ radix lane "air"
functio g() → numerus {
    sit numerus v ← 1 ↦ numerus
    redde v
}
"#,
    );
    expect_air_issue(&conversio, "runtime_conversion");

    let optional = air_errors(
        r#"
@ radix lane "air"
functio h() → numerus ∪ nihil {
    sit lista<numerus> xs ← [1, 2, 3]
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
    sit f32 total ← 0.0
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
    // `method_call` allowlist rows — a resolving method on a typed tensor
    // receiver still declines with the stable `method_call` cause. The probe
    // must typecheck (`foobar` would stack the decline on unknown_method,
    // which the purity gate now skips), so it names a real catalog method
    // that stays off the AIR allowlist.
    let errors = air_errors(
        r#"
@ radix lane "air"
functio loss(tf32[2, 2] x) → numerus {
    redde x.longitudo()
}
"#,
    );

    let err = expect_air_issue(&errors, "method_call");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "method" && arg.value == "longitudo"),
        "method_call decline must name the method: {:?}",
        err.args
    );
}

// ---------------------------------------------------------------------------
// ATH-3 (AIR Tier 0.5): recovery-arm Conversio slice. A conversion with an
// inline `⊥` default resolves its failure inside its own expression —
// self-resolved union dataflow, admitted like a locally-resolved handler. A
// conversion without recovery is a boundary throw and keeps the
// `runtime_conversion` decline everywhere in v1 — including inside admitted
// handler regions — with a cause distinct from every handler cause.
// ---------------------------------------------------------------------------

#[test]
fn air_lane_admits_inline_recovery_conversio() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio parse_or_zero(textus s) → numerus {
    sit numerus v ← s ↦ numerus ⊥ 0
    redde v
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn air_lane_admits_inline_recovery_conversio_inside_handler_region() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio in_region(textus s, bivalens bad) → numerus {
    fac {
        si bad { iace "x" }
        sit numerus v ← s ↦ numerus ⊥ 0
        redde v
    } cape err { redde 1 }
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn air_lane_bare_conversio_declines_inside_admitted_handler_region() {
    // OQ-THROW-SOURCE default: a no-recovery conversion stays out in v1 even
    // inside an admitted handler region, despite the base lane routing its
    // failure into the region's ErrorSink::Local channel. The region itself
    // admits; only the conversion declines, under its own named cause.
    let errors = air_errors(
        r#"
@ radix lane "air"
functio in_region(textus s, bivalens bad) → numerus {
    fac {
        si bad { iace "x" }
        sit numerus v ← s ↦ numerus
        redde v
    } cape err { redde 1 }
}
"#,
    );

    expect_air_issue(&errors, "runtime_conversion");
    for cause in [
        "recoverable_handler",
        "conditional_recovery",
        "recovery_type_mismatch",
        "rethrow_in_recovery",
    ] {
        assert!(
            !errors.iter().any(|err| err
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == cause)),
            "handler cause {cause} must stay distinct from the conversion decline: {errors:?}"
        );
    }
}

#[test]
fn air_lane_admits_non_literal_inline_recovery_expression() {
    // The inline recovery grammar is `'⊥' unary_expr` (EBNF #153): a control
    // expression carrying `iace` cannot appear in the recovery operand, so
    // the rethrow-in-recovery case is unrepresentable at the v1 surface.
    // The gate still checks the recovery arm as a Catch region (defensive
    // against a future grammar widening); a path operand admits like a
    // literal fallback.
    let errors = air_errors(
        r#"
@ radix lane "air"
functio parse_or_zero(textus s, numerus fallback) → numerus {
    sit numerus v ← s ↦ numerus ⊥ fallback
    redde v
}
"#,
    );

    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn rejects_nary_range_product_loop_fail_closed() {
    // NR-2 (nary-range-walk): the Cartesian `itera ab` product keeps the AIR
    // fail-closed loop policy — the same `itera_loop` reject as the
    // single-range and lockstep forms, with ranges and body still walked for
    // nested purity facts.
    let itera = air_errors(
        r#"
@ radix lane "air"
functio grid(numerus m, numerus n) → numerus {
    varia numerus total ← 0
    itera ab 0‥m, 0‥n fixum i, j {
        total ← i
    }
    redde total
}
"#,
    );
    expect_air_issue(&itera, "itera_loop");
}

// ---------------------------------------------------------------------------
// S03-9: `air_purity` runs on HIR whose types are known-unreliable once
// typecheck has failed — the same reason `air_unroll` above it is skipped.
// A wrong decline must not stack on the real type error.
// ---------------------------------------------------------------------------

#[test]
fn suppresses_purity_declines_once_typecheck_has_failed() {
    // `x + flag` fails typecheck (tf32 + bivalens) and leaves the binary
    // expression error-typed, so the `primus_quem` source has no reliable
    // tensor shape. The purity gate must be skipped rather than read that
    // broken type and stack `first_match_unbounded_source` on the type
    // error — the same skip `air_unroll` above it already applies.
    let errors = analyze(
        r#"
@ radix lane "air"
functio pick(tf32[2, 2] x, bivalens flag) → fractus ∪ nihil {
    redde primus_quem(x + flag, ubi v { v ≻ 0.0 })
}
"#,
    );

    assert!(
        errors
            .iter()
            .any(|err| err.is_error() && err.kind != SemanticErrorKind::AirPurityViolation),
        "probe must carry a real typecheck error: {errors:?}"
    );
    assert!(
        !errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::AirPurityViolation),
        "purity gate must be skipped once typecheck failed: {errors:?}"
    );
}

// Ruling 25: an AIR-lane integer store must be proven to fit; there is no
// exemption and no store node.

#[test]
fn air_accepts_integer_store_proven_by_the_range_rule() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio diff(u8 a, u8 b) → i16 {
    redde a - b
}
"#,
    );
    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn air_rejects_unproven_integer_store_with_its_own_issue() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio sum(numerus a, numerus b) → numerus {
    redde a + b
}
"#,
    );
    expect_air_issue(&errors, "air_store_not_proven");
    expect_air_issue(&errors, "air_integer_range_unproven");
    assert!(
        !errors.iter().any(|err| err
            .args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == "runtime_conversion")),
        "an implicit store is not a runtime_conversion decline: {errors:?}"
    );
}

#[test]
fn air_rejects_narrowing_store_that_needs_a_check() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio narrow(i16 a, i16 b) → u8 {
    redde a - b
}
"#,
    );
    expect_air_issue(&errors, "air_store_not_proven");
}

#[test]
fn air_still_admits_conversion_with_bottom_default() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio narrow(i16 a) → u8 {
    redde a ↦ u8 ⊥ 0
}
"#,
    );
    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

// air2: range facts that hold on every path prove bounded index arithmetic.

fn has_air_issue(errors: &[crate::SemanticError], issue: &str) -> bool {
    errors.iter().any(|err| {
        err.args
            .iter()
            .any(|arg| arg.name == "issue" && arg.value == issue)
    })
}

#[test]
fn air_admits_apud_coordinate_arithmetic_proven_by_tensor_dims() {
    // `iace` in the body is an unrelated keep-out; the range facts must not add
    // a range, store or loop-unroll decline.
    let errors = air_errors(
        r#"
@ radix lane "air"
functio pick(tensor<f32, [4, 4]> x) → tensor<f32, [4, 4]> ⇥ textus {
    varia tensor<f32, [4, 4]> acc ← copy x
    itera ex x apud [i, j] fixum cell {
        sit numerus idx ← i * 2 + j
        si idx ≻ 20 { iace "upper" }
        acc ← acc + x
    }
    redde acc
}
"#,
    );
    for issue in [
        "air_integer_range_unproven",
        "air_store_not_proven",
        "itera_loop",
    ] {
        assert!(!has_air_issue(&errors, issue), "{issue}: {errors:?}");
    }
}

#[test]
fn air_admits_range_loop_index_arithmetic_proven_by_constant_bounds() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio pick(tensor<f32, [2, 2]> x) → tensor<f32, [2, 2]> {
    varia tensor<f32, [2, 2]> acc ← copy x
    itera ab 0‥16 fixum i {
        sit numerus k ← i * 2 + 1
        acc ← acc + x
    }
    redde acc
}
"#,
    );
    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn air_rejects_index_arithmetic_over_unbounded_operands() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio pick(numerus a, numerus b) → numerus {
    sit numerus idx ← a * 2 + b
    redde idx
}
"#,
    );
    expect_air_issue(&errors, "air_integer_range_unproven");
}

#[test]
fn air_rejects_apud_arithmetic_over_symbolic_tensor_dims() {
    // A non-constant dimension gives no range fact: the coordinate stays
    // `numerus` and its arithmetic is unproven.
    let errors = air_errors(
        r#"
@ radix lane "air"
functio pick(numerus n, numerus m) → numerus {
    itera ab 0‥n fixum i {
        sit numerus k ← i * 2 + m
    }
    redde 0
}
"#,
    );
    assert!(
        has_air_issue(&errors, "air_integer_range_unproven")
            || has_air_issue(&errors, "itera_loop"),
        "{errors:?}"
    );
}

#[test]
fn air_admits_arithmetic_over_inferred_fixum_of_proven_index_expression() {
    // `fixum _` keeps the range type of its proven initializer; a fact that
    // holds on every path (immutable binding of a proven expression).
    let errors = air_errors(
        r#"
@ radix lane "air"
functio pick(tensor<f32, [2, 2]> x) → tensor<f32, [2, 2]> {
    varia tensor<f32, [2, 2]> acc ← copy x
    itera ab 0‥16 fixum i {
        sit k ← i * 2 + 1
        sit m ← k * 2 + i
        acc ← acc + x
    }
    redde acc
}
"#,
    );
    assert!(errors.is_empty(), "unexpected AIR errors: {errors:?}");
}

#[test]
fn air_rejects_arithmetic_over_declared_numerus_binding_even_if_initializer_is_bounded() {
    // A declared `numerus` slot has the whole i64 range (ruling 12): no fact
    // survives the annotation.
    let errors = air_errors(
        r#"
@ radix lane "air"
functio pick(tensor<f32, [2, 2]> x) → tensor<f32, [2, 2]> {
    varia tensor<f32, [2, 2]> acc ← copy x
    itera ab 0‥16 fixum i {
        sit numerus k ← i * 2 + 1
        sit numerus m ← k * k
        acc ← acc + x
    }
    redde acc
}
"#,
    );
    // The unproven store inside the loop body makes the unroll decline it.
    assert!(
        has_air_issue(&errors, "itera_loop")
            || has_air_issue(&errors, "air_integer_range_unproven"),
        "{errors:?}"
    );
}

#[test]
fn copy_marked_tensor_receiver_keeps_tensor_method_admission() {
    // `copy x` puts the copy access marker on the local's declared tensor
    // type; receiver admission must see through it. The loop is the AIR
    // accumulator form (a bare `varia` outside an unrolled loop is rejected
    // independently by `mutable_varia`).
    let errors = air_errors(
        r#"
@ radix lane "air"
functio f(tensor<f32, [4]> x) → f32 {
    varia tensor<f32, [4]> acc ← copy x
    itera ab 0‥4 fixum i {
        acc ← acc + x
    }
    redde acc.media()
}
"#,
    );
    assert!(
        !has_air_issue(&errors, "tensor_method_receiver"),
        "{errors:?}"
    );
    assert!(
        !errors
            .iter()
            .any(|err| err.kind == SemanticErrorKind::AirPurityViolation),
        "{errors:?}"
    );
}
