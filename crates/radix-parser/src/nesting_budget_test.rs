//! Parser resource budgets: grammar nesting depth and folded-operator count.
//!
//! Contract: source past either budget fails closed with a structured parse
//! diagnostic (never a stack abort, never a panic), while source at the budget
//! parses and yields a tree the AST walker admits without reporting
//! [`DepthExceeded`].

use crate::test_support::{
    ParseExpectation, assert_parse_error_matches, has_arg, parse_ok, parse_program,
};
use crate::{MAX_PARSE_DEPTH, MAX_PARSE_OPERATORS, ParseError, ParseErrorKind, ParseResult};
use radix_syntax::{DepthExceeded, Visitor};

/// Depth past the budget used by the rejection pins. Well below the point where
/// an unguarded descent would exhaust the test thread's stack.
const OVER_BUDGET_DEPTH: usize = MAX_PARSE_DEPTH as usize + 64;

/// Folded operators past the budget used by the rejection pins.
const OVER_BUDGET_OPERATORS: usize = MAX_PARSE_OPERATORS + 64;

/// Stack the hostile-shape probes run on: a quarter of the 2 MiB test thread and
/// a sixteenth of the 8 MiB production stack. The unguarded parser aborts on it
/// at the probe depth (measured, 768 levels; on 256 KiB it already aborts
/// between 64 and 128), so a probe that returns at all has proved the budget
/// fired instead of the stack.
const PROBE_STACK_BYTES: usize = 512 * 1024;

/// Stack the walker-alignment probes run on: the 8 MiB production compile stack
/// (`faber` main thread, package analysis) the budgets are sized for.
///
/// Those probes parse the deepest tree the budget admits, and an unoptimised
/// parser spends tens of KB of stack per grammar level (measured: `parse_primary`
/// 12 KB, `parse_postfix_inner` 11 KB, ~70 KB for one paren level), so the
/// admitted depth no longer fits the 2 MiB test thread. The walker-alignment
/// contract is about the tree, not about that thread, so these probes run on
/// the stack the budget is documented against; the fail-closed probes above
/// stay on [`PROBE_STACK_BYTES`].
const PRODUCTION_STACK_BYTES: usize = 8 * 1024 * 1024;

/// One nesting shape under test: a name and a source generator for a depth.
type NestingShape = (&'static str, fn(usize) -> String);

fn nested_parens(depth: usize) -> String {
    format!(
        "fixum _ value ← {}1{}",
        "(".repeat(depth),
        ")".repeat(depth)
    )
}

fn assignment_chain(depth: usize) -> String {
    format!("fixum _ value ← {}1", "a ← ".repeat(depth))
}

/// Bare nested block statements: block nesting with no expression in the way,
/// so the budget is spent in the statement grammar.
fn nested_blocks(depth: usize) -> String {
    format!("{}{}{}", "{ ".repeat(depth), "tacet", " }".repeat(depth))
}

fn nested_types(depth: usize) -> String {
    format!(
        "typus Nested = {}{}{}",
        "lista<".repeat(depth),
        "numerus",
        " >".repeat(depth)
    )
}

/// Prefix-unary chain: `-` recurses through the unary level, which is not an
/// expression entry point, so a chain nests the `unary` rule without entering
/// the budgeted expression entry.
fn unary_prefix_chain(depth: usize) -> String {
    format!("fixum _ value ← {}1", "-".repeat(depth))
}

/// Nested JSON arrays inside one bare `{ }` literal. The island's values are
/// parsed by the dedicated JSON parser, which never re-enters the expression
/// grammar.
fn nested_json_arrays(depth: usize) -> String {
    format!(
        "fixum _ value ← {{\"k\": {}1{}}}",
        "[".repeat(depth),
        "]".repeat(depth)
    )
}

/// Nested array binding pattern: `[[[target]]]`.
fn nested_array_pattern(depth: usize) -> String {
    format!("fixum {}target{} ← 0", "[".repeat(depth), "]".repeat(depth))
}

fn flat_fold_chain(operators: usize) -> String {
    format!("fixum _ value ← 1{}", " + 1".repeat(operators))
}

fn postfix_member_chain(members: usize) -> String {
    format!("fixum _ value ← root{}", ".b".repeat(members))
}

fn type_test_chain(tests: usize) -> String {
    format!("fixum _ value ← root{}", " est T".repeat(tests))
}

fn nested_figura(depth: usize) -> String {
    format!(
        "functio f(tensor<f32, {}1{}> value) → vacuum {{}}",
        "[".repeat(depth),
        "]".repeat(depth)
    )
}

/// Parenthesized figura arithmetic: `(` recurses through the figura atom, a
/// separate entry from the `[` tuple.
fn nested_figura_parens(depth: usize) -> String {
    format!(
        "functio f(tensor<f32, {}1{}> value) → vacuum {{}}",
        "(".repeat(depth),
        ")".repeat(depth)
    )
}

/// Spelled-lift parenthesis: `magnitudo ( magnitudo ( ...` recurses through the
/// lift extent.
fn nested_figura_lift_parens(depth: usize) -> String {
    format!(
        "functio f(tensor<f32, {}1{}> value) → vacuum {{}}",
        "magnitudo (".repeat(depth),
        ")".repeat(depth)
    )
}

/// Two folded chains under one root expression.
fn split_fold_chain(operators_per_side: usize) -> String {
    let side = format!("1{}", " + 1".repeat(operators_per_side));
    format!("fixum _ value ← ({side}) + ({side})")
}

/// Records whether a walk reported the depth limit.
#[derive(Default)]
struct DepthProbe {
    exceeded: Option<usize>,
}

impl Visitor for DepthProbe {
    fn on_depth_exceeded(&mut self, exceeded: DepthExceeded) {
        self.exceeded = Some(exceeded.depth);
    }
}

/// Parse `source` on a thread with an explicit stack size and return the
/// diagnostics it reported.
///
/// Running the hostile shapes here rather than on the test thread makes "small
/// stack" the probe's own measured property. A panic from the join means the
/// probe thread aborted instead of failing closed, which is the defect the pin
/// exists to catch.
fn parse_on_stack(source: &str, stack_bytes: usize) -> Vec<ParseError> {
    let source = source.to_owned();
    std::thread::Builder::new()
        .stack_size(stack_bytes)
        .spawn(move || parse_program(&source).errors)
        .expect("probe thread spawns")
        .join()
        .expect("probe must fail closed, not abort: the budget must fire before the stack does")
}

/// Run `probe` on a thread with the production stack, re-raising its panic.
fn on_production_stack(probe: impl FnOnce() + Send + 'static) {
    let handle = std::thread::Builder::new()
        .stack_size(PRODUCTION_STACK_BYTES)
        .spawn(probe)
        .expect("probe thread spawns");
    if let Err(panic) = handle.join() {
        std::panic::resume_unwind(panic);
    }
}

/// Walk a parsed program and return the depth the walker refused to enter.
fn walker_stop_depth(result: &ParseResult) -> Option<usize> {
    let program = result.program.as_ref().expect("parsed program");
    let mut probe = DepthProbe::default();
    probe.visit_program(program);
    probe.exceeded
}

/// Deepest size the parser admits for `build`, found by bisection between
/// `rejected_at` (known to be rejected) and zero. Returns the admitted size and
/// its parse result.
fn deepest_admitted_within(
    build: impl Fn(usize) -> String,
    rejected_at: usize,
) -> (usize, ParseResult) {
    let mut admitted = 0usize;
    let mut rejected = rejected_at;
    while rejected > admitted + 1 {
        let mid = admitted.midpoint(rejected);
        if parse_program(&build(mid)).success() {
            admitted = mid;
        } else {
            rejected = mid;
        }
    }
    (admitted, parse_program(&build(admitted)))
}

#[test]
fn parenthesized_nesting_past_the_budget_reports_a_nesting_diagnostic() {
    assert_parse_error_matches(
        &nested_parens(OVER_BUDGET_DEPTH),
        ParseExpectation::Issue("nesting_too_deep"),
    );
}

#[test]
fn assignment_chain_past_the_budget_reports_a_nesting_diagnostic() {
    assert_parse_error_matches(
        &assignment_chain(OVER_BUDGET_DEPTH),
        ParseExpectation::Issue("nesting_too_deep"),
    );
}

#[test]
fn block_nesting_past_the_budget_reports_a_nesting_diagnostic() {
    assert_parse_error_matches(
        &nested_blocks(OVER_BUDGET_DEPTH),
        ParseExpectation::Issue("nesting_too_deep"),
    );
}

#[test]
fn type_nesting_past_the_budget_reports_a_nesting_diagnostic() {
    assert_parse_error_matches(
        &nested_types(OVER_BUDGET_DEPTH),
        ParseExpectation::Issue("nesting_too_deep"),
    );
}

/// The three recursions that do not pass through a budgeted entry point — the
/// unary prefix chain, the JSON island, and the nested array pattern — each
/// report the depth diagnostic rather than exhausting the stack.
///
/// They run on [`PROBE_STACK_BYTES`] so the abort an unguarded descent produces
/// is a real measurement of this probe, not a claim about it: the same input
/// aborted the pre-change parser, and the guarded one returns a single
/// diagnostic however deep it is.
fn assert_recursion_fails_closed(name: &str, build: fn(usize) -> String) {
    let errors = parse_on_stack(&build(OVER_BUDGET_DEPTH * 8), PROBE_STACK_BYTES);
    assert_eq!(
        errors.len(),
        1,
        "{name}: a spent budget reports exactly once, got {errors:?}"
    );
    assert!(
        has_arg(&errors[0], "issue", "nesting_too_deep"),
        "{name}: the refusal must carry the nesting diagnostic, got {:?}",
        errors[0]
    );
}

#[test]
fn unary_prefix_chain_past_the_budget_reports_a_nesting_diagnostic() {
    let source = unary_prefix_chain(OVER_BUDGET_DEPTH);
    assert_parse_error_matches(&source, ParseExpectation::Issue("nesting_too_deep"));
    assert_parse_error_matches(
        &source,
        ParseExpectation::Kind(ParseErrorKind::ExpressionInvalid),
    );
    assert_recursion_fails_closed("unary_prefix_chain", unary_prefix_chain);
}

#[test]
fn json_value_nesting_past_the_budget_reports_a_nesting_diagnostic() {
    let source = nested_json_arrays(OVER_BUDGET_DEPTH);
    assert_parse_error_matches(&source, ParseExpectation::Issue("nesting_too_deep"));
    assert_parse_error_matches(
        &source,
        ParseExpectation::Kind(ParseErrorKind::ExpressionInvalid),
    );
    assert_recursion_fails_closed("nested_json_arrays", nested_json_arrays);
}

#[test]
fn array_binding_pattern_nesting_past_the_budget_reports_a_nesting_diagnostic() {
    let source = nested_array_pattern(OVER_BUDGET_DEPTH);
    assert_parse_error_matches(&source, ParseExpectation::Issue("nesting_too_deep"));
    assert_parse_error_matches(
        &source,
        ParseExpectation::Kind(ParseErrorKind::StatementInvalid),
    );
    assert_recursion_fails_closed("nested_array_pattern", nested_array_pattern);
}

/// The count budget, not the depth counter, must reject an iteratively folded
/// chain: `1 + 1 + …` costs no parser recursion, so a flat source can build a
/// tree far deeper than the grammar nests.
#[test]
fn postfix_member_chain_past_the_budget_reports_a_count_diagnostic() {
    assert_parse_error_matches(
        &postfix_member_chain(MAX_PARSE_OPERATORS + 64),
        ParseExpectation::Issue("expression_too_long"),
    );
}

#[test]
fn repeated_type_tests_past_the_budget_report_a_count_diagnostic() {
    assert_parse_error_matches(
        &type_test_chain(MAX_PARSE_OPERATORS + 64),
        ParseExpectation::Issue("expression_too_long"),
    );
}

#[test]
fn nested_figura_past_the_budget_reports_a_nesting_diagnostic() {
    assert_parse_error_matches(
        &nested_figura(OVER_BUDGET_DEPTH),
        ParseExpectation::Issue("nesting_too_deep"),
    );
}

#[test]
fn nested_figura_parens_past_the_budget_fail_closed() {
    assert_recursion_fails_closed("nested_figura_parens", nested_figura_parens);
    assert_recursion_fails_closed("nested_figura_lift_parens", nested_figura_lift_parens);
}

#[test]
fn flat_fold_chain_past_the_operator_budget_reports_a_count_diagnostic() {
    let source = flat_fold_chain(OVER_BUDGET_OPERATORS);
    assert_parse_error_matches(&source, ParseExpectation::Issue("expression_too_long"));
    assert_parse_error_matches(
        &source,
        ParseExpectation::Kind(ParseErrorKind::ExpressionInvalid),
    );
    let result = parse_program(&source);
    assert!(
        !result
            .errors
            .iter()
            .any(|err| err.args.iter().any(|arg| arg.value == "nesting_too_deep")),
        "a flat chain must trip the count budget, not the depth budget: {:?}",
        result.errors
    );
}

/// Operators accumulate across the sub-expressions of one root, so nested
/// spines cannot compose past the budget the way per-sub-expression counting
/// would allow.
#[test]
fn fold_operators_accumulate_across_sub_expressions_of_one_root() {
    let per_side = MAX_PARSE_OPERATORS / 2 + 1;
    assert!(
        per_side < MAX_PARSE_OPERATORS,
        "each side must stay under the budget on its own"
    );
    let source = split_fold_chain(per_side);
    assert_parse_error_matches(&source, ParseExpectation::Issue("expression_too_long"));
}

#[test]
fn nesting_and_folding_within_the_budgets_parse_clean() {
    let _ = parse_ok(&nested_parens(MAX_PARSE_DEPTH as usize / 4));
    let _ = parse_ok(&assignment_chain(MAX_PARSE_DEPTH as usize / 4));
    let _ = parse_ok(&nested_blocks(MAX_PARSE_DEPTH as usize / 4));
    let _ = parse_ok(&nested_types(MAX_PARSE_DEPTH as usize / 4));
    let _ = parse_ok(&unary_prefix_chain(MAX_PARSE_DEPTH as usize / 4));
    let _ = parse_ok(&nested_json_arrays(MAX_PARSE_DEPTH as usize / 4));
    let _ = parse_ok(&nested_array_pattern(MAX_PARSE_DEPTH as usize / 4));
    let _ = parse_ok(&flat_fold_chain(MAX_PARSE_OPERATORS / 4));
}

/// Budgets are aligned with the walker: the deepest tree the parser admits for
/// every nesting shape is walked without reporting the depth limit.
#[test]
fn deepest_admitted_nesting_stays_inside_the_walker_budget() {
    on_production_stack(deepest_admitted_nesting_inside_walker_budget);
}

fn deepest_admitted_nesting_inside_walker_budget() {
    let shapes: [NestingShape; 7] = [
        ("parens", nested_parens),
        ("assignment", assignment_chain),
        ("blocks", nested_blocks),
        ("types", nested_types),
        ("unary", unary_prefix_chain),
        ("json", nested_json_arrays),
        ("pattern", nested_array_pattern),
    ];
    for (name, build) in shapes {
        let (admitted, result) = deepest_admitted_within(build, OVER_BUDGET_DEPTH);
        assert!(
            admitted >= 8,
            "{name}: generator must admit real nesting, got {admitted}"
        );
        assert!(
            result.success(),
            "{name}: depth {admitted} must parse clean, got {:?}",
            result.errors
        );
        assert_eq!(
            walker_stop_depth(&result),
            None,
            "{name}: the deepest admitted tree (depth {admitted}) must stay inside the walker budget"
        );
    }
}

#[test]
fn deepest_admitted_fold_chain_stays_inside_the_walker_budget() {
    let (admitted, result) = deepest_admitted_within(flat_fold_chain, OVER_BUDGET_OPERATORS);
    assert!(
        admitted > MAX_PARSE_OPERATORS / 2,
        "generator must admit a real chain, got {admitted}"
    );
    assert!(result.success(), "chain of {admitted} must parse clean");
    assert_eq!(
        walker_stop_depth(&result),
        None,
        "an admitted chain of {admitted} operators must stay inside the walker budget"
    );
}

/// A spent budget reports once and stops the parse: the enclosing statement
/// loops must not re-enter the refused position level by level.
#[test]
fn spent_budget_reports_one_diagnostic_and_stops() {
    for source in [
        nested_parens(OVER_BUDGET_DEPTH * 8),
        nested_blocks(OVER_BUDGET_DEPTH * 4),
        flat_fold_chain(OVER_BUDGET_OPERATORS * 4),
    ] {
        let result = parse_program(&source);
        assert_eq!(
            result.errors.len(),
            1,
            "a spent budget must report exactly once, got {:?}",
            result.errors
        );
    }
}

/// Depth and count compose: a source at both budgets still yields a tree inside
/// the walker budget.
#[test]
fn deep_nesting_with_a_long_fold_chain_stays_inside_the_walker_budget() {
    on_production_stack(deep_nesting_with_a_long_fold_chain_inside_walker_budget);
}

fn deep_nesting_with_a_long_fold_chain_inside_walker_budget() {
    let depth = MAX_PARSE_DEPTH as usize - 2;
    let operators = MAX_PARSE_OPERATORS - 8;
    let chain = format!("1{}", " + 1".repeat(operators));
    let source = format!(
        "fixum _ value ← {}{}{}",
        "(".repeat(depth),
        chain,
        ")".repeat(depth)
    );
    let result = parse_ok(&source);
    assert_eq!(
        walker_stop_depth(&result),
        None,
        "nested parens at the depth budget over a chain at the count budget must stay inside the walker budget"
    );
}
