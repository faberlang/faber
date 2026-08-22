//! Coverage for the named AIR-lane unroll pass.
//!
//! Pins the unit-3E contract: concrete-`N` `itera ab` range loops admit as
//! guard-free straight line; declines stay on `SEM059.itera_loop` with a
//! visible `causa`; non-AIR-lane loops are left for MIR lowering.
//!
//! Unit-3F extends the same pass with the guard-as-mask subset: a caller-built
//! 0/1 mask operand, fail-closed in-region / NaN-unsafe / unmasked-exit
//! causae, and a still-green full-prefix path.

use crate::test_support::args_have_no_legacy_message;
use crate::{PassConfig, SemanticError, SemanticErrorKind};
use radix_hir::visit::{walk_expr, walk_program, HirVisitor};
use radix_hir::{HirExpression, HirExpressionKind, HirLiteral, HirModule};
use radix_target::Target;

fn analyze(source: &str) -> crate::SemanticResult {
    let lex = radix_lexer::lex(source);
    let mut parse = radix_parser::parse(lex);
    let program = parse.program.expect("parse");
    let config = PassConfig::for_target(Target::MirWasm);
    crate::analyze(&program, &config, &mut parse.interner)
}

fn air_errors(source: &str) -> Vec<SemanticError> {
    analyze(source)
        .diagnostics
        .into_iter()
        .filter(|err| err.kind == SemanticErrorKind::AirPurityViolation)
        .collect()
}

fn expect_issue<'a>(errors: &'a [SemanticError], issue: &str) -> &'a SemanticError {
    let err = errors
        .iter()
        .find(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == issue)
        })
        .expect("structured AIR diagnostic");
    assert!(
        args_have_no_legacy_message(&err.args),
        "migrated diagnostic should not choose prose at emit site"
    );
    err
}

fn expect_decline_causa<'a>(errors: &'a [SemanticError], causa: &str) -> &'a SemanticError {
    let err = errors
        .iter()
        .find(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "itera_loop")
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "causa" && arg.value == causa)
        })
        .expect("declined itera should keep SEM059.itera_loop with a causa");
    assert!(
        err.args
            .iter()
            .any(|arg| arg.name == "lane" && arg.value == "air"),
        "decline must stay on the air lane"
    );
    err
}

fn has_itera(hir: &HirModule) -> bool {
    let mut found = FindItera(false);
    walk_program(&mut found, hir);
    found.0
}

fn has_si(hir: &HirModule) -> bool {
    let mut found = FindSi(false);
    walk_program(&mut found, hir);
    found.0
}

fn if_eq_literals(hir: &HirModule) -> Vec<u64> {
    let mut found = CollectIfEq(Vec::new());
    walk_program(&mut found, hir);
    found.0
}

struct FindItera(bool);
impl HirVisitor for FindItera {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if matches!(expr.kind, HirExpressionKind::Itera(..)) {
            self.0 = true;
        }
        walk_expr(self, expr);
    }
}

struct FindSi(bool);
impl HirVisitor for FindSi {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if matches!(expr.kind, HirExpressionKind::Si { .. }) {
            self.0 = true;
        }
        walk_expr(self, expr);
    }
}

struct CollectIfEq(Vec<u64>);
impl HirVisitor for CollectIfEq {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if let HirExpressionKind::Si { cond, .. } = &expr.kind {
            if let Some(value) = cond_lane_literal(cond) {
                self.0.push(value);
            }
        }
        walk_expr(self, expr);
    }
}

fn cond_lane_literal(cond: &HirExpression) -> Option<u64> {
    match &cond.kind {
        HirExpressionKind::Binary(_, lhs, rhs) => match (&lhs.kind, &rhs.kind) {
            (HirExpressionKind::Literal(HirLiteral::Int(value)), _)
            | (_, HirExpressionKind::Literal(HirLiteral::Int(value))) => Some(*value),
            _ => None,
        },
        HirExpressionKind::Literal(HirLiteral::Int(value)) => Some(*value),
        _ => None,
    }
}

const SECTION3_SCALAR_LOOP: &str = r#"
@ radix lane "air"
@ radix backward "range_backward"
functio loop_count() → f32 {
    fixum f32 total ← 0.0
    itera ab 0‥10 per 2 fixum i {
        total ← total + 1.0
    }
    redde total
}
"#;

const FULL_PREFIX_TENSOR_LOOP: &str = r#"
@ radix lane "air"
@ radix backward "lane_sum_backward"
functio lane_sum(tensor<f32, [4]> x) → f32 {
    varia tensor<f32, [4]> acc ← x
    itera ab 0‥4 fixum i {
        acc ← acc.addita(x)
    }
    redde acc.media()
}
"#;

const TENSOR_EX_LOOP: &str = r#"
@ radix lane "air"
functio tensor_sum(tensor<f32, [4]> x) → f32 {
    varia f32 total ← 0.0
    itera ex x fixum value {
        total ← total + value
    }
    redde total
}
"#;

#[test]
fn tensor_ex_rank1_admits_guard_free_element_lets() {
    let result = analyze(TENSOR_EX_LOOP);
    assert!(
        result.is_ok(),
        "rank-1 tensor itera ex must analyze cleanly: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "admitted tensor iteration must be erased");
    assert!(!has_si(hir), "tensor iteration is guard-free");
    let mut gathers = CountIndexes(0);
    walk_program(&mut gathers, hir);
    assert_eq!(
        gathers.0, 4,
        "one tensor element gather must be emitted per lane"
    );
}

#[test]
fn tensor_ex_rank2_defers_to_coordinates_with_distinct_causa() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio grid_sum(tensor<f32, [2, 2]> x) → f32 {
    itera ex x fixum value { }
    redde 0.0
}
"#,
    );
    expect_decline_causa(&errors, "defer_coordinates");
    assert!(
        !errors.iter().any(|error| {
            error
                .args
                .iter()
                .any(|arg| arg.name == "causa" && arg.value == "not_concrete")
        }),
        "rank-2 defer must not collapse into not_concrete: {errors:?}"
    );
}

#[test]
fn tensor_ex_symbolic_shape_declines_not_concrete() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio generic_sum<magnitudo N>(tensor<f32, [N]> x) → f32 {
    itera ex x fixum value { }
    redde 0.0
}
"#,
    );
    expect_decline_causa(&errors, "not_concrete");
}

#[test]
fn tensor_ex_single_loop_over_budget_declines() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio too_wide(tensor<f32, [65]> x) → f32 {
    itera ex x fixum value { }
    redde 0.0
}
"#,
    );
    expect_decline_causa(&errors, "budget");
}

#[test]
fn tensor_ex_budget_is_per_function_total() {
    let result = analyze(
        r#"
@ radix lane "air"
functio two_loops(tensor<f32, [40]> a, tensor<f32, [40]> b) → f32 {
    varia f32 total ← 0.0
    itera ex a fixum first {
        total ← total + first
    }
    itera ex b fixum second {
        total ← total + second
    }
    redde total
}
"#,
    );
    expect_decline_causa(&air_errors_from(&result), "budget");
    let hir = result.hir.as_ref().expect("typed HIR despite AIR decline");
    assert!(
        has_itera(hir),
        "the over-budget second loop must remain whole"
    );
}

fn air_errors_from(result: &crate::SemanticResult) -> Vec<SemanticError> {
    result
        .diagnostics
        .iter()
        .filter(|error| error.kind == SemanticErrorKind::AirPurityViolation)
        .cloned()
        .collect()
}

struct CountIndexes(usize);
impl HirVisitor for CountIndexes {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if matches!(expr.kind, HirExpressionKind::Index(..)) {
            self.0 += 1;
        }
        walk_expr(self, expr);
    }
}

#[test]
fn concrete_range_loop_admits_after_unroll() {
    let result = analyze(SECTION3_SCALAR_LOOP);
    let itera_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|err| {
            err.kind == SemanticErrorKind::AirPurityViolation
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "itera_loop")
        })
        .collect();
    assert!(
        itera_errors.is_empty(),
        "section-3 range loop must flip off SEM059.itera_loop: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(
        !has_itera(hir),
        "unrolled full-prefix loop must not leave an Itera node"
    );
    assert!(
        !has_si(hir),
        "full-prefix unroll is guard-free: no If reaches later AIR"
    );
}

#[test]
fn full_prefix_tensor_loop_is_guard_free_straight_line() {
    let result = analyze(FULL_PREFIX_TENSOR_LOOP);
    assert!(
        result.is_ok(),
        "full-prefix tensor accumulation must analyze cleanly: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "admitted unroll must erase the range loop");
    assert!(!has_si(hir), "len == N proof emits no runtime guard");
}

#[test]
fn evaluation_order_and_failure_lane_are_preserved() {
    let result = analyze(
        r#"
@ radix lane "air"
functio boom(tensor<f32, [4]> x) → tensor<f32, [4]> ⇥ textus {
    varia tensor<f32, [4]> acc ← x
    itera ab 0‥4 fixum i {
        si i > 1 {
            iace "boom"
        }
        acc ← acc.addita(x)
    }
    redde acc
}
"#,
    );
    let itera_errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|err| {
            err.kind == SemanticErrorKind::AirPurityViolation
                && err
                    .args
                    .iter()
                    .any(|arg| arg.name == "issue" && arg.value == "itera_loop")
        })
        .collect();
    assert!(
        itera_errors.is_empty(),
        "failure-lane body must still unroll: {:?}",
        result.diagnostics
    );
    expect_issue(&result.diagnostics, "iace");
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "unrolled failure body must not keep Itera");
    assert!(
        has_si(hir),
        "per-lane `si` must remain so the original failing lane still fails"
    );
    assert_eq!(
        if_eq_literals(hir),
        vec![0, 1, 2, 3],
        "lanes must splice in source order so lane 2 is still the failing lane"
    );
}

#[test]
fn budget_exceeding_n_declines_with_causa() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio too_wide(tensor<f32, [4]> x) → f32 {
    varia tensor<f32, [4]> acc ← x
    itera ab 0‥65 fixum i {
        acc ← acc.addita(x)
    }
    redde acc.media()
}
"#,
    );
    expect_decline_causa(&errors, "budget");
    expect_issue(&errors, "itera_loop");
}

#[test]
fn impure_effect_body_declines_with_causa() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio dirty() → numerus {
    itera ab 0‥4 fixum i {
        nota i
    }
    redde 0
}
"#,
    );
    expect_decline_causa(&errors, "impure");
}

#[test]
fn unsupported_return_declines_with_causa() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio early(tensor<f32, [4]> x) → f32 {
    varia tensor<f32, [4]> acc ← x
    itera ab 0‥4 fixum i {
        redde acc.media()
    }
    redde acc.media()
}
"#,
    );
    expect_decline_causa(&errors, "unsupported_control");
}

#[test]
fn generic_or_runtime_bound_declines_not_concrete() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio runtime_n(numerus n, tensor<f32, [4]> x) → f32 {
    varia tensor<f32, [4]> acc ← x
    itera ab 0‥n fixum i {
        acc ← acc.addita(x)
    }
    redde acc.media()
}
"#,
    );
    expect_decline_causa(&errors, "not_concrete");
}

#[test]
fn non_air_lane_range_loop_is_left_in_place() {
    let result = analyze(
        r#"
functio host_loop() → numerus {
    varia numerus total ← 0
    itera ab 0‥4 fixum i {
        total ← total + i
    }
    redde total
}
"#,
    );
    assert!(
        result.is_ok(),
        "non-AIR host loop must keep ordinary MIR lowering: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(
        has_itera(hir),
        "non-AIR-lane itera must remain for MIR lowering"
    );
}

const MASKED_LANE_MEAN: &str = r#"
@ radix lane "air"
@ radix backward "masked_mean_backward"
functio masked_lane_mean(tensor<f32, [4]> x, tensor<f32, [4]> mask) → f32 {
    varia tensor<f32, [4]> acc ← x
    itera ab 0‥4 fixum i {
        acc ← acc.addita(x)
    }
    redde acc.multiplica(mask).media()
}
"#;

#[test]
fn masked_len_lt_n_unrolls_guard_free() {
    let result = analyze(MASKED_LANE_MEAN);
    assert!(
        result.is_ok(),
        "mask-mode route must unroll and admit: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(
        !has_itera(hir),
        "mask-mode unroll must erase the range loop"
    );
    assert!(
        !has_si(hir),
        "mask-mode runs all N lanes: no If reaches later AIR"
    );
}

#[test]
fn two_data_tensors_stay_full_prefix() {
    let result = analyze(
        r#"
@ radix lane "air"
@ radix backward "pair_backward"
functio pair_sum(tensor<f32, [4]> a, tensor<f32, [4]> b) → f32 {
    varia tensor<f32, [4]> acc ← a
    itera ab 0‥4 fixum i {
        acc ← acc.addita(b)
    }
    redde acc.media()
}
"#,
    );
    assert!(
        result.is_ok(),
        "two data tensors are not a mask pair: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "full-prefix pair must still unroll");
    assert!(!has_si(hir), "full-prefix pair stays guard-free");
}

#[test]
fn in_region_fill_mask_declines_with_causa() {
    let errors = air_errors(
        r#"
@ radix lane "air"
@ radix backward "fill_backward"
functio fill_mask(tensor<f32, [4]> x) → f32 {
    fixum tensor<f32, [4]> mask ← x.reple(1.0)
    varia tensor<f32, [4]> acc ← x
    itera ab 0‥4 fixum i {
        acc ← acc.addita(x)
    }
    redde acc.multiplica(mask).media()
}
"#,
    );
    expect_decline_causa(&errors, "in_region_mask");
}

#[test]
fn in_region_scalar_comparison_declines_with_causa() {
    let errors = air_errors(
        r#"
@ radix lane "air"
@ radix backward "cmp_backward"
functio cmp_mask(tensor<f32, [4]> x, numerus len) → f32 ⇥ textus {
    varia tensor<f32, [4]> acc ← x
    itera ab 0‥4 fixum i {
        si i < len {
            iace "pad"
        }
        acc ← acc.addita(x)
    }
    redde acc.media()
}
"#,
    );
    expect_decline_causa(&errors, "in_region_mask");
}

#[test]
fn nan_unsafe_unclamped_div_declines_with_causa() {
    let errors = air_errors(
        r#"
@ radix lane "air"
@ radix backward "div_backward"
functio unsafe_div(tensor<f32, [4]> x, tensor<f32, [4]> mask) → f32 {
    varia tensor<f32, [4]> acc ← x.divisio(mask)
    itera ab 0‥4 fixum i {
        acc ← acc.addita(x)
    }
    redde acc.multiplica(mask).media()
}
"#,
    );
    expect_decline_causa(&errors, "nan_unsafe");
}

#[test]
fn unmasked_reduction_exit_declines_with_causa() {
    let errors = air_errors(
        r#"
@ radix lane "air"
@ radix backward "leak_backward"
functio unmasked_exit(tensor<f32, [4]> x, tensor<f32, [4]> mask) → f32 {
    varia tensor<f32, [4]> acc ← x
    itera ab 0‥4 fixum i {
        acc ← acc.addita(x)
    }
    redde acc.media()
}
"#,
    );
    expect_decline_causa(&errors, "unmasked_exit");
}
