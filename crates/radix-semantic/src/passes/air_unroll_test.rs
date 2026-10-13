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
use radix_hir::visit::{HirVisitor, walk_expr, walk_program, walk_stmt};
use radix_hir::{
    HirExpression, HirExpressionKind, HirLiteral, HirModule, HirStatement, HirStatementKind,
};
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
        if matches!(expr.kind, HirExpressionKind::Itera { .. }) {
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
        acc ← acc + x
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

// Coordinate-free rank-2 `itera ex` is an arity type error (AC-5, SEM010),
// so it never reaches this pass; the pass-side `defer_coordinates` decline
// stays as fail-closed defense. The explicit `apud [i, j]` form is Route M
// and admits below.

const APUD_RANK2_GRID: &str = r#"
@ radix lane "air"
@ radix backward "grid_backward"
functio grid_sum(tensor<f32, [2, 3]> x) → f32 {
    varia tensor<f32, [2, 3]> acc ← x
    itera ex x apud [i, j] fixum cell {
        acc ← acc + x
    }
    redde acc.media()
}
"#;

#[test]
fn apud_rank2_unrolls_six_guard_free_lanes_first_bound_outer() {
    let result = analyze(APUD_RANK2_GRID);
    assert!(
        result.is_ok(),
        "rank-2 apud outer product must reach reverse AD: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "admitted apud iteration must be erased");
    assert!(!has_si(hir), "apud unroll is guard-free");
    let mut gathers = CountIndexes(0);
    walk_program(&mut gathers, hir);
    assert_eq!(
        gathers.0, 6,
        "one tensor element gather must be emitted per lane"
    );
    assert_eq!(
        coordinate_lane_tuples(hir, 2),
        vec![
            vec![0, 0],
            vec![0, 1],
            vec![0, 2],
            vec![1, 0],
            vec![1, 1],
            vec![1, 2]
        ],
        "lane tuples for [2, 3] must run first-bound=outer (i slowest, j fastest)"
    );
}

#[test]
fn apud_rank1_driver_reaches_reverse_ad_with_lane_count() {
    let result = analyze(
        r#"
@ radix lane "air"
@ radix backward "line_backward"
functio line_sum(tensor<f32, [8]> x) → f32 {
    varia tensor<f32, [8]> acc ← x
    itera ex x apud [i] fixum cell {
        acc ← acc + x
    }
    redde acc.media()
}
"#,
    );
    assert!(
        result.is_ok(),
        "rank-1 apud must reach reverse AD: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "rank-1 apud iteration must be erased");
    assert!(!has_si(hir), "rank-1 apud unroll is guard-free");
    let mut gathers = CountIndexes(0);
    walk_program(&mut gathers, hir);
    assert_eq!(gathers.0, 8, "lane count must equal the product of dims");
    assert_eq!(
        coordinate_lane_tuples(hir, 1),
        (0..8).map(|lane| vec![lane]).collect::<Vec<_>>(),
        "rank-1 lane constants must run in source order"
    );
}

#[test]
fn apud_rank3_driver_reaches_reverse_ad_with_lane_count() {
    let result = analyze(
        r#"
@ radix lane "air"
@ radix backward "cube_backward"
functio cube_sum(tensor<f32, [2, 2, 2]> x) → f32 {
    varia tensor<f32, [2, 2, 2]> acc ← x
    itera ex x apud [i, j, k] fixum cell {
        acc ← acc + x
    }
    redde acc.media()
}
"#,
    );
    assert!(
        result.is_ok(),
        "rank-3 apud must reach reverse AD: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "rank-3 apud iteration must be erased");
    assert!(!has_si(hir), "rank-3 apud unroll is guard-free");
    let mut gathers = CountIndexes(0);
    walk_program(&mut gathers, hir);
    assert_eq!(gathers.0, 8, "lane count must equal 2 * 2 * 2");
    assert_eq!(
        coordinate_lane_tuples(hir, 3),
        vec![
            vec![0, 0, 0],
            vec![0, 0, 1],
            vec![0, 1, 0],
            vec![0, 1, 1],
            vec![1, 0, 0],
            vec![1, 0, 1],
            vec![1, 1, 0],
            vec![1, 1, 1],
        ],
        "first bound i must vary slowest, last bound k fastest"
    );
}

#[test]
fn apud_product_over_budget_declines_whole_with_causa() {
    let result = analyze(
        r#"
@ radix lane "air"
functio too_wide(tensor<f32, [9, 8]> x) → f32 {
    itera ex x apud [i, j] fixum cell { }
    redde 0.0
}
"#,
    );
    expect_decline_causa(&air_errors_from(&result), "budget");
    let hir = result.hir.as_ref().expect("typed HIR despite AIR decline");
    assert!(
        has_itera(hir),
        "the over-budget apud loop must remain whole"
    );
}

#[test]
fn apud_generic_template_is_retained_until_a_concrete_call() {
    let result = analyze(
        r#"
@ radix lane "air"
functio generic_grid<magnitudo M, magnitudo N>(tensor<f32, [M, N]> x) → f32 {
    itera ex x apud [i, j] fixum cell { }
    redde 0.0
}
"#,
    );
    assert!(
        air_errors_from(&result).is_empty(),
        "an uninstantiated generic template is not an executable AIR body: {:?}",
        result.diagnostics
    );
    assert!(has_itera(result.hir.as_ref().expect("typed HIR")));
}

#[test]
fn apud_on_non_air_lane_loop_is_left_in_place() {
    let result = analyze(
        r"
functio host_grid(tensor<f32, [2, 3]> x) → vacuum {
    itera ex x apud [i, j] fixum cell { }
}
",
    );
    assert!(
        result.is_ok(),
        "non-AIR apud loop must keep ordinary lowering: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(
        has_itera(hir),
        "non-AIR-lane apud itera must remain untouched"
    );
}

struct CollectCoordLets(Vec<u64>);
impl HirVisitor for CollectCoordLets {
    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if let HirStatementKind::Local(local) = &stmt.kind {
            if let Some(init) = &local.init {
                if let HirExpressionKind::Literal(HirLiteral::Int(value)) = init.kind {
                    self.0.push(value);
                }
            }
        }
        walk_stmt(self, stmt);
    }
}

/// Per-lane coordinate tuples in emission order, chunked by rank: only the
/// per-lane `numerus` constant lets bound by Route M qualify.
fn coordinate_lane_tuples(hir: &HirModule, rank: usize) -> Vec<Vec<u64>> {
    let mut found = CollectCoordLets(Vec::new());
    walk_program(&mut found, hir);
    assert!(
        found.0.len().is_multiple_of(rank),
        "coordinate constants must chunk evenly by rank"
    );
    found.0.chunks(rank).map(<[u64]>::to_vec).collect()
}

#[test]
fn tensor_ex_generic_template_is_retained_until_a_concrete_call() {
    let result = analyze(
        r#"
@ radix lane "air"
functio generic_sum<magnitudo N>(tensor<f32, [N]> x) → f32 {
    itera ex x fixum value { }
    redde 0.0
}
"#,
    );
    assert!(
        air_errors_from(&result).is_empty(),
        "an uninstantiated generic template is not an executable AIR body: {:?}",
        result.diagnostics
    );
    assert!(has_itera(result.hir.as_ref().expect("typed HIR")));
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

/// Range step ruling (2026-10-02): the step is a positive stride and the
/// bounds pick the direction, so `10‥0 per 2` unrolls to the same five lanes
/// as `0‥10 per 2`; a non-positive step never unrolls.
#[test]
fn stepped_range_unrolls_in_the_direction_of_its_bounds() {
    let lanes = |range: &str| {
        let source = format!(
            r#"
@ radix lane "air"
@ radix backward "range_backward"
functio loop_count() → f32 {{
    varia f32 total ← 0.0
    itera ab {range} fixum i {{
        total ← total + 1.0
    }}
    redde total
}}
"#
        );
        let result = analyze(&source);
        let hir = result.hir.as_ref().expect("typed HIR");
        let mut steps = CountBinaryInits(0);
        walk_program(&mut steps, hir);
        (has_itera(hir), steps.0)
    };
    assert_eq!(lanes("0‥10 per 2"), (false, 5));
    assert_eq!(lanes("10‥0 per 2"), (false, 5));
    assert_eq!(lanes("10…0 per 5"), (false, 3));
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
        si i ≻ 1 {
            iace "boom"
        }
        acc ← acc + x
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
        acc ← acc + x
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
        acc ← acc + x
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
        r"
functio host_loop() → numerus {
    varia numerus total ← 0
    itera ab 0‥4 fixum i {
        total ← total + i
    }
    redde total
}
",
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
        acc ← acc + x
    }
    redde (acc ⊙ mask).media()
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
        acc ← acc + b
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
        acc ← acc + x
    }
    redde (acc ⊙ mask).media()
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
        si i ≺ len {
            iace "pad"
        }
        acc ← acc + x
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
        acc ← acc + x
    }
    redde (acc ⊙ mask).media()
}
"#,
    );
    expect_decline_causa(&errors, "nan_unsafe");
}

#[test]
fn nan_unsafe_div_under_verte_declines_with_causa() {
    let errors = air_errors(
        r#"
@ radix lane "air"
@ radix backward "div_verte_backward"
functio unsafe_div_verte(tensor<f32, [4]> x, tensor<f32, [4]> mask) → f32 {
    varia tensor<f32, [4]> acc ← x.divisio(mask) ∷ tensor<f32, [4]>
    itera ab 0‥4 fixum i {
        acc ← acc + x
    }
    redde (acc ⊙ mask).media()
}
"#,
    );
    expect_decline_causa(&errors, "nan_unsafe");
}

#[test]
fn nan_unsafe_div_under_conversio_declines_with_causa() {
    let errors = air_errors(
        r#"
@ radix lane "air"
@ radix backward "div_conversio_backward"
functio unsafe_div_conversio(tensor<f32, [4]> x, tensor<f32, [4]> mask) → f32 {
    varia tensor<f32, [4]> acc ← x.divisio(mask) ↦ tensor<f32, [4]> ⊥ x
    itera ab 0‥4 fixum i {
        acc ← acc + x
    }
    redde (acc ⊙ mask).media()
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
        acc ← acc + x
    }
    redde acc.media()
}
"#,
    );
    expect_decline_causa(&errors, "unmasked_exit");
}

#[test]
fn bounded_lista_ex_full_literal_admits_guard_free_lanes() {
    let result = analyze(
        r#"
@ radix lane "air"
functio bounded_sum() → vacuum {
    fixum lista<f32, 4> xs ← [1.0, 2.0, 3.0, 4.0] ↦ lista<f32, 4>
    itera ex xs fixum value { }
}
"#,
    );
    assert!(
        air_errors_from(&result).iter().all(|error| {
            !error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "itera_loop")
        }),
        "full bounded lista must not decline its itera loop: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(
        !has_itera(hir),
        "full bounded lista iteration must be erased"
    );
    assert!(!has_si(hir), "full bounded lista iteration is guard-free");
    let mut gathers = CountIndexes(0);
    walk_program(&mut gathers, hir);
    assert_eq!(gathers.0, 4, "one gather must be emitted per full slot");
}

#[test]
fn bounded_lista_ex_inference_chain_preserves_fullness_fact() {
    let result = analyze(
        r#"
@ radix lane "air"
functio bounded_chain() → vacuum ⇥ textus {
    fixum lista<f32> raw ← [1.0, 2.0, 3.0, 4.0]
    fixum lista<f32, 4> full ← raw ↦ lista<f32, 4>
    fixum lista<f32, 4> alias ← full
    itera ex alias fixum value { }
}
"#,
    );
    assert!(
        air_errors_from(&result).iter().all(|error| {
            !error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "itera_loop")
        }),
        "inference-chained full bounded lista must not decline its itera loop: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "fullness chain must still erase iteration");
}

#[test]
fn bounded_lista_ex_non_full_value_declines_not_concrete() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio bounded_partial() → vacuum ⇥ textus {
    fixum lista<f32> raw ← [1.0, 2.0, 3.0]
    fixum lista<f32, 4> xs ← raw ↦ lista<f32, 4>
    itera ex xs fixum value { }
}
"#,
    );
    expect_decline_causa(&errors, "not_concrete");
}

#[test]
fn bounded_lista_ex_full_literal_n8_admits_guard_free_lanes() {
    let result = analyze(
        r#"
@ radix lane "air"
functio bounded_sum_8() → vacuum {
    fixum lista<f32, 8> xs ← [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0] ↦ lista<f32, 8>
    itera ex xs fixum value { }
}
"#,
    );
    assert!(
        air_errors_from(&result).iter().all(|error| {
            !error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "itera_loop")
        }),
        "full 8-slot bounded lista must not decline its itera loop: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "full 8-slot iteration must be erased");
    assert!(!has_si(hir), "full 8-slot iteration is guard-free");
    let mut gathers = CountIndexes(0);
    walk_program(&mut gathers, hir);
    assert_eq!(gathers.0, 8, "one gather must be emitted per full slot");
}

#[test]
fn bounded_lista_ex_over_budget_declines_whole_with_causa() {
    let elements = (0..65)
        .map(|index| format!("{index}.0"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        r#"
@ radix lane "air"
functio too_wide() → vacuum {{
    fixum lista<f32, 65> xs ← [{elements}] ↦ lista<f32, 65>
    itera ex xs fixum value {{ }}
}}
"#
    );
    let result = analyze(&source);
    expect_decline_causa(&air_errors_from(&result), "budget");
    let hir = result.hir.as_ref().expect("typed HIR despite AIR decline");
    assert!(
        has_itera(hir),
        "the over-budget bounded-lista loop must remain whole"
    );
}

#[test]
fn bounded_lista_ex_unbounded_declines_not_concrete() {
    let errors = air_errors(
        r#"
@ radix lane "air"
functio unbounded() → vacuum {
    fixum lista<f32> xs ← [1.0, 2.0, 3.0]
    itera ex xs fixum value { }
}
"#,
    );
    expect_decline_causa(&errors, "not_concrete");
}

// --- AEU-7: scalar bounded buffer forms (`octeti<N>` / `ascii<N>` / `textus<N>`)

/// Collects the element types of the emitted lane gathers.
struct GatherTys(Vec<crate::TypeId>);
impl HirVisitor for GatherTys {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if matches!(expr.kind, HirExpressionKind::Index(..)) {
            if let Some(ty) = expr.ty {
                self.0.push(ty);
            }
        }
        walk_expr(self, expr);
    }
}

fn assert_no_itera_decline(result: &crate::SemanticResult, why: &str) {
    assert!(
        air_errors_from(result).iter().all(|error| {
            !error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "itera_loop")
        }),
        "{why}: {:?}",
        result.diagnostics
    );
}

fn is_numerus_u8(types: &crate::TypeTable, ty: crate::TypeId) -> bool {
    matches!(
        types.get(ty),
        crate::Type::SizedNumeric(
            radix_types::Primitive::Numerus,
            radix_types::NumericWidth::U8
        )
    )
}

fn is_littera(types: &crate::TypeTable, ty: crate::TypeId) -> bool {
    matches!(
        types.get(ty),
        crate::Type::Primitive(radix_types::Primitive::Littera)
    )
}

#[test]
fn octeti_ex_full_literal_admits_guard_free_lanes() {
    let result = analyze(
        r#"
@ radix lane "air"
functio octeti_full() → vacuum {
    fixum octeti<8> xs ← |de ad be ef ba dc fe ca| ↦ octeti<8>
    itera ex xs fixum b { }
}
"#,
    );
    assert_no_itera_decline(&result, "full octeti<8> must not decline its itera loop");
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "full octeti<8> iteration must be erased");
    assert!(!has_si(hir), "full octeti<8> iteration is guard-free");
    let mut gathers = GatherTys(Vec::new());
    walk_program(&mut gathers, hir);
    assert_eq!(
        gathers.0.len(),
        8,
        "one gather must be emitted per full slot"
    );
    assert!(
        gathers.0.iter().all(|ty| is_numerus_u8(&result.types, *ty)),
        "every octeti<N> lane gathers u8: {:?}",
        gathers.0
    );
}

#[test]
fn ascii_ex_full_literal_admits_guard_free_lanes() {
    let result = analyze(
        r#"
@ radix lane "air"
functio ascii_full() → vacuum {
    fixum ascii<8> xs ← 'alphabet' ↦ ascii<8>
    itera ex xs fixum c { }
}
"#,
    );
    assert_no_itera_decline(&result, "full ascii<8> must not decline its itera loop");
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "full ascii<8> iteration must be erased");
    assert!(!has_si(hir), "full ascii<8> iteration is guard-free");
    let mut gathers = GatherTys(Vec::new());
    walk_program(&mut gathers, hir);
    assert_eq!(
        gathers.0.len(),
        8,
        "one gather must be emitted per full slot"
    );
    assert!(
        gathers.0.iter().all(|ty| is_littera(&result.types, *ty)),
        "every ascii<N> lane gathers littera (D10.1): {:?}",
        gathers.0
    );
}

#[test]
fn textus_ex_full_literal_admits_guard_free_lanes() {
    let result = analyze(
        r#"
@ radix lane "air"
functio textus_full() → vacuum {
    fixum textus<8> xs ← "abcdefgh" ↦ textus<8>
    itera ex xs fixum c { }
}
"#,
    );
    assert_no_itera_decline(&result, "full textus<8> must not decline its itera loop");
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "full textus<8> iteration must be erased");
    assert!(!has_si(hir), "full textus<8> iteration is guard-free");
    let mut gathers = GatherTys(Vec::new());
    walk_program(&mut gathers, hir);
    assert_eq!(
        gathers.0.len(),
        8,
        "one gather must be emitted per full slot"
    );
    // D10.1: each lane binds one Unicode scalar (`littera`); there is no
    // non-ASCII trap row on text iteration.
    assert!(
        gathers.0.iter().all(|ty| is_littera(&result.types, *ty)),
        "every textus<N> lane gathers littera: {:?}",
        gathers.0
    );
}

#[test]
fn scalar_bounded_ex_runtime_len_param_declines_not_concrete() {
    for source in [
        r#"
@ radix lane "air"
functio runtime(octeti<8> xs) → vacuum {
    itera ex xs fixum b { }
}"#,
        r#"
@ radix lane "air"
functio runtime(ascii<8> xs) → vacuum {
    itera ex xs fixum c { }
}"#,
        r#"
@ radix lane "air"
functio runtime(textus<8> xs) → vacuum {
    itera ex xs fixum c { }
}"#,
    ] {
        expect_decline_causa(&air_errors(source), "not_concrete");
    }
}

#[test]
fn textus_ex_over_budget_declines_whole_with_causa() {
    let literal = "a".repeat(65);
    let source = format!(
        r#"
@ radix lane "air"
functio too_wide() → vacuum {{
    fixum textus<65> xs ← "{literal}" ↦ textus<65>
    itera ex xs fixum c {{ }}
}}
"#
    );
    let result = analyze(&source);
    expect_decline_causa(&air_errors_from(&result), "budget");
    let hir = result.hir.as_ref().expect("typed HIR despite AIR decline");
    assert!(
        has_itera(hir),
        "the over-budget textus loop must remain whole"
    );
}

#[test]
fn pipeline_unroll_admits_arc3_intrinsic_before_purity() {
    let result = analyze(
        r#"
@ radix lane "air"
functio combine(fractus a, fractus b) → fractus {
    redde a + b
}

@ radix lane "air"
functio reduce_tensor(tensor<f32, [4]> x) → f32 {
    varia f32 total ← 0.0
    itera ab 0‥4 fixum i {
        total ← total + x.reducta(combine, 0.0)
    }
    redde total
}
"#,
    );
    assert!(
        result.is_ok(),
        "ARC-3 intrinsic must pass the first-running unroll pass and purity: {:?}",
        result.diagnostics
    );
    assert!(
        result.diagnostics.iter().all(|error| {
            !error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "itera_loop")
        }),
        "the pre-purity unroll pass must not decline an ARC-3 intrinsic: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "the pipeline must erase the admitted loop");
}

// AC-8: index-dependent bodies and the decline catalog. Coordinate-reading
// conditions are ordinary pure per-lane control — they admit through the
// unroll (constants substituted per lane) — while ragged iterables decline
// fail-closed at the shaped-iterable rule, and a mixed function keeps exactly
// the declined diagnostic.

#[test]
fn apud_causal_body_si_i_le_j_admits_per_lane_conditions() {
    let result = analyze(
        r#"
@ radix lane "air"
@ radix backward "causal_backward"
functio causal(tensor<f32, [2, 2]> x) → tensor<f32, [2, 2]> ⇥ textus {
    varia tensor<f32, [2, 2]> acc ← x
    itera ex x apud [i, j] fixum cell {
        si i ≻ j { iace "upper" }
        acc ← acc + x
    }
    redde acc
}
"#,
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|err| !err.args.iter().any(|arg| arg.name == "causa")),
        "causal index-dependent body must not decline: {:?}",
        result.diagnostics
    );
    expect_issue(&result.diagnostics, "iace");
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(
        !has_itera(hir),
        "admitted causal apud iteration must be erased"
    );
    assert!(
        has_si(hir),
        "per-lane `si` must remain so each lane keeps its own folded condition"
    );
    assert_eq!(
        coordinate_lane_tuples(hir, 2),
        vec![vec![0, 0], vec![0, 1], vec![1, 0], vec![1, 1]],
        "causal lanes must splice in source order"
    );
    let mut sis = CountSi(0);
    walk_program(&mut sis, hir);
    assert_eq!(sis.0, 4, "one per-lane condition must remain per lane");
}

#[test]
fn apud_triangular_arithmetic_body_admits_per_lane_conditions() {
    let result = analyze(
        r#"
@ radix lane "air"
@ radix backward "triangular_backward"
functio triangular(tensor<f32, [2, 2]> x) → tensor<f32, [2, 2]> ⇥ textus {
    varia tensor<f32, [2, 2]> acc ← x
    itera ex x apud [i, j] fixum cell {
        fixum numerus idx ← i * 2 + j
        si idx ≻ 2 { iace "upper" }
        acc ← acc + x
    }
    redde acc
}
"#,
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|err| !err.args.iter().any(|arg| arg.name == "causa")),
        "triangular i * 2 + j body must not decline: {:?}",
        result.diagnostics
    );
    expect_issue(&result.diagnostics, "iace");
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(
        !has_itera(hir),
        "admitted triangular apud iteration must be erased"
    );
    assert_eq!(
        coordinate_lane_tuples(hir, 2),
        vec![vec![0, 0], vec![0, 1], vec![1, 0], vec![1, 1]],
        "triangular lanes must splice in source order"
    );
    let mut sis = CountSi(0);
    walk_program(&mut sis, hir);
    assert_eq!(sis.0, 4, "one per-lane condition must remain per lane");
}

#[test]
fn apud_ragged_nested_lista_declines_fail_closed() {
    let result = analyze(
        r#"
@ radix lane "air"
functio ragged(lista<lista<f32>> pairs) → f32 {
    itera ex pairs apud [i, j] fixum cell { }
    redde 0.0
}
"#,
    );
    // Ragged `lista<lista<T>>` is rank 1 (D3.1), so two names fail closed
    // at the arity rule; the loop is never unrolled. With typecheck failed
    // the AIR purity pass is skipped (S03-9: it would read unreliable
    // types), so the decline is the arity error alone — no `itera_loop`
    // stacked on top.
    expect_issue(&result.diagnostics, "itera_apud_arity_mismatch");
    assert!(
        !result
            .diagnostics
            .iter()
            .any(|err| err.issue() == Some("itera_loop")),
        "purity decline must not stack on the failed typecheck: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR despite the decline");
    assert!(has_itera(hir), "ragged apud iteration must remain whole");
}

#[test]
fn apud_mixed_admit_and_decline_reports_only_the_declined_loop() {
    let result = analyze(
        r#"
@ radix lane "air"
@ radix backward "mixed_backward"
functio mixed(tensor<f32, [40]> x) → f32 {
    varia tensor<f32, [40]> acc ← x
    itera ex x apud [i] fixum cell {
        acc ← acc + x
    }
    itera ex x apud [i] fixum cell {
        acc ← acc + x
    }
    redde acc.media()
}
"#,
    );
    let declined: Vec<_> = air_errors_from(&result)
        .into_iter()
        .filter(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "itera_loop")
                && err.args.iter().any(|arg| arg.name == "causa")
        })
        .collect();
    assert_eq!(
        declined.len(),
        1,
        "exactly the over-budget loop declines: {:?}",
        result.diagnostics
    );
    assert!(
        declined[0]
            .args
            .iter()
            .any(|arg| arg.name == "causa" && arg.value == "budget")
    );
    let hir = result.hir.as_ref().expect("typed HIR despite the decline");
    let mut iteras = CountIteras(0);
    walk_program(&mut iteras, hir);
    assert_eq!(
        iteras.0, 1,
        "the admitted loop is erased; only the declined loop remains whole"
    );
}

struct CountIteras(usize);
impl HirVisitor for CountIteras {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if matches!(expr.kind, HirExpressionKind::Itera { .. }) {
            self.0 += 1;
        }
        walk_expr(self, expr);
    }
}

struct CountSi(usize);
impl HirVisitor for CountSi {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if matches!(expr.kind, HirExpressionKind::Si { .. }) {
            self.0 += 1;
        }
        walk_expr(self, expr);
    }
}

// AC-9: nesting and budget composition. Nested unroll units charge
// multiplicatively against the shared per-function UNROLL_BUDGET: an
// `itera` directly inside another loop's body admits when the product of
// the two lane counts fits, declines whole when it does not, and the whole
// nest stays intact on any decline (3E/3F: source stays intact). Lanes
// splice in source order; non-AIR-lane loops are untouched.

struct CountBinaryInits(usize);
impl HirVisitor for CountBinaryInits {
    fn visit_stmt(&mut self, stmt: &HirStatement) {
        if let HirStatementKind::Local(local) = &stmt.kind {
            if let Some(init) = &local.init {
                if matches!(init.kind, HirExpressionKind::Binary(..)) {
                    self.0 += 1;
                }
            }
        }
        walk_stmt(self, stmt);
    }
}

#[test]
fn apud_itera_ab_nest_admits_product_lanes_in_source_order() {
    let result = analyze(
        r#"
@ radix lane "air"
@ radix backward "nest_backward"
functio nested_sum(tensor<f32, [2, 3]> x) → f32 ⇥ textus {
    varia f32 total ← 0.0
    itera ex x apud [i, j] fixum cell {
        itera ab 0‥2 fixum k {
            si k ≻ 0 { iace "inner" }
            total ← total + 1.0
        }
    }
    redde total
}
"#,
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|err| !err.args.iter().any(|arg| arg.name == "causa")),
        "apud x itera ab nest must admit within budget (no decline): {:?}",
        result.diagnostics
    );
    expect_issue(&result.diagnostics, "iace");
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "admitted nest must be fully erased");
    assert!(
        has_si(hir),
        "per-lane inner conditions must remain (failure lanes preserved)"
    );
    let mut steps = CountBinaryInits(0);
    walk_program(&mut steps, hir);
    assert_eq!(
        steps.0, 12,
        "total lanes must be the product: 6 outer x 2 inner"
    );
    let mut sis = CountSi(0);
    walk_program(&mut sis, hir);
    assert_eq!(sis.0, 12, "one per-lane condition per emitted lane");
    assert_eq!(
        if_eq_literals(hir),
        vec![0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1],
        "inner lanes must splice in source order inside each outer lane"
    );
    assert_eq!(
        coordinate_lane_tuples(hir, 2),
        vec![
            vec![0, 0],
            vec![0, 1],
            vec![0, 2],
            vec![1, 0],
            vec![1, 1],
            vec![1, 2],
        ],
        "outer apud lanes must run first-bound=outer in source order"
    );
}

#[test]
fn itera_ab_apud_nest_admits_product_lanes() {
    let result = analyze(
        r#"
@ radix lane "air"
functio nested_ab_outer(tensor<f32, [2, 3]> x) → f32 {
    varia f32 total ← 0.0
    itera ab 0‥2 fixum k {
        itera ex x apud [i, j] fixum cell {
            total ← total + 1.0
        }
    }
    redde total
}
"#,
    );
    assert!(
        result.is_ok(),
        "itera ab x apud nest must admit within budget: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "admitted nest must be fully erased");
    let mut steps = CountBinaryInits(0);
    walk_program(&mut steps, hir);
    assert_eq!(
        steps.0, 12,
        "total lanes must be the product: 2 outer x 6 inner"
    );
    assert_eq!(
        coordinate_lane_tuples(hir, 2),
        vec![
            vec![0, 0],
            vec![0, 1],
            vec![0, 2],
            vec![1, 0],
            vec![1, 1],
            vec![1, 2],
            vec![0, 0],
            vec![0, 1],
            vec![0, 2],
            vec![1, 0],
            vec![1, 1],
            vec![1, 2],
        ],
        "inner apud lanes must splice in source order inside each outer lane"
    );
}

#[test]
fn apud_apud_nest_admits_product_lanes() {
    let result = analyze(
        r#"
@ radix lane "air"
functio nested_apud(tensor<f32, [2, 3]> x, tensor<f32, [4]> y) → f32 ⇥ textus {
    varia f32 total ← 0.0
    itera ex x apud [i, j] fixum cell {
        itera ex y apud [k] fixum other {
            si k ≻ 0 { iace "inner" }
            total ← total + 1.0
        }
    }
    redde total
}
"#,
    );
    assert!(
        result
            .diagnostics
            .iter()
            .all(|err| !err.args.iter().any(|arg| arg.name == "causa")),
        "apud x apud nest must admit within budget (no decline): {:?}",
        result.diagnostics
    );
    expect_issue(&result.diagnostics, "iace");
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "admitted apud x apud nest must be erased");
    let mut steps = CountBinaryInits(0);
    walk_program(&mut steps, hir);
    assert_eq!(
        steps.0, 24,
        "total lanes must be the product: 6 outer x 4 inner"
    );
    let mut sis = CountSi(0);
    walk_program(&mut sis, hir);
    assert_eq!(sis.0, 24, "one per-lane condition per emitted lane");
    let mut coords = CollectCoordLets(Vec::new());
    walk_program(&mut coords, hir);
    assert_eq!(
        coords.0,
        vec![
            0, 0, 0, 1, 2, 3, // outer (0,0): i, j, then k = 0..4
            0, 1, 0, 1, 2, 3, 0, 2, 0, 1, 2, 3, 1, 0, 0, 1, 2, 3, 1, 1, 0, 1, 2, 3, 1, 2, 0, 1, 2,
            3,
        ],
        "outer lanes in first-bound=outer order, inner k in source order per lane"
    );
}

#[test]
fn nested_product_over_budget_declines_whole_with_causa() {
    let result = analyze(
        r#"
@ radix lane "air"
functio too_deep(tensor<f32, [8]> x) → f32 {
    varia f32 total ← 0.0
    itera ex x apud [i] fixum cell {
        itera ab 0‥8 fixum k {
            total ← total + 1.0
        }
    }
    redde total
}
"#,
    );
    expect_decline_causa(&air_errors_from(&result), "budget");
    let hir = result.hir.as_ref().expect("typed HIR despite AIR decline");
    let mut iteras = CountIteras(0);
    walk_program(&mut iteras, hir);
    assert_eq!(
        iteras.0, 2,
        "the whole nest must remain intact: no partial admission"
    );
}

#[test]
fn nested_not_concrete_inner_declines_whole_with_causa() {
    let result = analyze(
        r#"
@ radix lane "air"
functio runtime_inner(tensor<f32, [2, 3]> x, numerus n) → f32 {
    varia f32 total ← 0.0
    itera ex x apud [i, j] fixum cell {
        itera ab 0‥n fixum k {
            total ← total + 1.0
        }
    }
    redde total
}
"#,
    );
    expect_decline_causa(&air_errors_from(&result), "not_concrete");
    let hir = result.hir.as_ref().expect("typed HIR despite AIR decline");
    let mut iteras = CountIteras(0);
    walk_program(&mut iteras, hir);
    assert_eq!(
        iteras.0, 2,
        "a failing nested unit keeps the whole nest intact"
    );
}

#[test]
fn si_conditioned_nested_itera_declines_fail_closed() {
    let result = analyze(
        r#"
@ radix lane "air"
functio conditioned(tensor<f32, [2, 3]> x) → f32 {
    varia f32 total ← 0.0
    itera ex x apud [i, j] fixum cell {
        si i ≻ j {
            itera ab 0‥2 fixum k {
                total ← total + 1.0
            }
        }
    }
    redde total
}
"#,
    );
    expect_decline_causa(&air_errors_from(&result), "unsupported_control");
    let hir = result.hir.as_ref().expect("typed HIR despite AIR decline");
    let mut iteras = CountIteras(0);
    walk_program(&mut iteras, hir);
    assert_eq!(iteras.0, 2, "conditional nesting stays whole (fail-closed)");
}

#[test]
fn sequential_apud_and_ab_share_the_function_budget() {
    let result = analyze(
        r#"
@ radix lane "air"
functio two_units(tensor<f32, [2, 3]> x) → f32 {
    varia f32 total ← 0.0
    itera ex x apud [i, j] fixum cell {
        total ← total + 1.0
    }
    itera ab 0‥4 fixum k {
        total ← total + 1.0
    }
    redde total
}
"#,
    );
    assert!(
        result.is_ok(),
        "apud and itera ab must share the budget within limits: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "both units must be erased");
    let mut steps = CountBinaryInits(0);
    walk_program(&mut steps, hir);
    assert_eq!(steps.0, 10, "lanes add for sequential units (6 + 4)");
}

#[test]
fn sequential_units_over_budget_declines_the_exceeding_loop() {
    let result = analyze(
        r#"
@ radix lane "air"
functio too_many(tensor<f32, [8]> x) → f32 {
    varia f32 total ← 0.0
    itera ex x apud [i] fixum cell {
        total ← total + 1.0
    }
    itera ab 0‥60 fixum k {
        total ← total + 1.0
    }
    redde total
}
"#,
    );
    let declined: Vec<_> = air_errors_from(&result)
        .into_iter()
        .filter(|err| {
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "itera_loop")
                && err.args.iter().any(|arg| arg.name == "causa")
        })
        .collect();
    assert_eq!(
        declined.len(),
        1,
        "exactly the over-budget loop declines: {:?}",
        result.diagnostics
    );
    assert!(
        declined[0]
            .args
            .iter()
            .any(|arg| arg.name == "causa" && arg.value == "budget")
    );
    let hir = result.hir.as_ref().expect("typed HIR despite the decline");
    let mut iteras = CountIteras(0);
    walk_program(&mut iteras, hir);
    assert_eq!(
        iteras.0, 1,
        "the admitted apud is erased; only the declined loop remains whole"
    );
}

#[test]
fn nested_non_air_loop_stays_in_place() {
    let result = analyze(
        r"
functio host_nested(tensor<f32, [2, 3]> x) → vacuum {
    itera ex x apud [i, j] fixum cell {
        itera ab 0‥2 fixum k { }
    }
}
",
    );
    assert!(
        result.is_ok(),
        "non-AIR nested loops must keep ordinary lowering: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    let mut iteras = CountIteras(0);
    walk_program(&mut iteras, hir);
    assert_eq!(
        iteras.0, 2,
        "non-AIR-lane iteras must remain byte-identical for MIR lowering"
    );
}

#[test]
fn apud_route_a_bounded_lista_nest_admits_product_lanes() {
    // `apud` x the sibling Route A (full-literal bounded lista, AEU-3 live):
    // the nest admits with the product lane count like any other unit pair.
    let result = analyze(
        r#"
@ radix lane "air"
functio nested_lista(tensor<f32, [4]> x) → f32 {
    varia f32 total ← 0.0
    fixum lista<f32, 4> xs ← [1.0, 2.0, 3.0, 4.0] ↦ lista<f32, 4>
    itera ex x apud [i] fixum cell {
        itera ex xs fixum value {
            total ← total + 1.0
        }
    }
    redde total
}
"#,
    );
    assert!(
        air_errors_from(&result).iter().all(|error| {
            !error
                .args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "itera_loop")
        }),
        "apud x Route A nest must admit within budget: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    assert!(!has_itera(hir), "admitted Route A nest must be erased");
    let mut steps = CountBinaryInits(0);
    walk_program(&mut steps, hir);
    assert_eq!(
        steps.0, 16,
        "total lanes must be the product: 4 outer x 4 inner"
    );
}

#[test]
fn nary_range_product_never_admits_an_unroll_recipe() {
    // NR-2 (nary-range-walk): the Cartesian `itera ab` product has no AIR
    // unroll recipe. `classify_itera_expr` only admits the single-source
    // `Itera`, so the product stays fail-closed: the air lane declines it
    // as SEM059.itera_loop and nothing is erased or spliced.
    let errors = air_errors(
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
    expect_issue(&errors, "itera_loop");
}

#[test]
fn nary_range_product_off_air_lane_is_left_in_place() {
    // Off the air lane the product is ordinary HIR for the MIR pipeline
    // (NR-3): the unroll pass must not touch it.
    let result = analyze(
        r"
functio grid(numerus m, numerus n) → vacuum {
    itera ab 0‥m, 0‥n fixum i, j { }
}
",
    );
    assert!(
        result.is_ok(),
        "non-AIR range product must keep ordinary lowering: {:?}",
        result.diagnostics
    );
    let hir = result.hir.as_ref().expect("typed HIR");
    let mut found = FindIteraRange(false);
    walk_program(&mut found, hir);
    assert!(
        found.0,
        "non-AIR-lane range product must remain untouched by the unroll pass"
    );
}

struct FindIteraRange(bool);
impl HirVisitor for FindIteraRange {
    fn visit_expr(&mut self, expr: &HirExpression) {
        if matches!(expr.kind, HirExpressionKind::IteraRange { .. }) {
            self.0 = true;
        }
        walk_expr(self, expr);
    }
}
