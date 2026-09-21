//! Parser resource budgets: grammar nesting depth and folded-operator count.
//!
//! Contract: source past either budget fails closed with a structured parse
//! diagnostic (never a stack abort, never a panic), while source at the budget
//! parses and yields a tree the AST walker admits without reporting
//! [`DepthExceeded`].

use crate::test_support::{ParseExpectation, assert_parse_error_matches, parse_ok, parse_program};
use crate::{MAX_PARSE_DEPTH, MAX_PARSE_OPERATORS, ParseErrorKind, ParseResult};
use radix_syntax::{DepthExceeded, Visitor};

/// Depth past the budget used by the rejection pins. Well below the point where
/// an unguarded descent would exhaust the test thread's stack.
const OVER_BUDGET_DEPTH: usize = MAX_PARSE_DEPTH as usize + 64;

/// Folded operators past the budget used by the rejection pins.
const OVER_BUDGET_OPERATORS: usize = MAX_PARSE_OPERATORS + 64;

/// One nesting shape under test: a name and a source generator for a depth.
type NestingShape = (&'static str, fn(usize) -> String);

fn nested_parens(depth: usize) -> String {
    format!(
        "fixum _ value ← {}1{}",
        "(".repeat(depth),
        ")".repeat(depth)
    )
}

fn ternary_chain(depth: usize) -> String {
    format!("fixum _ value ← {}1", "1 ? 2 : ".repeat(depth))
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

fn flat_fold_chain(operators: usize) -> String {
    format!("fixum _ value ← 1{}", " + 1".repeat(operators))
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
fn ternary_chain_past_the_budget_reports_a_nesting_diagnostic() {
    assert_parse_error_matches(
        &ternary_chain(OVER_BUDGET_DEPTH),
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

/// The count budget, not the depth counter, must reject an iteratively folded
/// chain: `1 + 1 + …` costs no parser recursion, so a flat source can build a
/// tree far deeper than the grammar nests.
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
    let _ = parse_ok(&ternary_chain(MAX_PARSE_DEPTH as usize / 4));
    let _ = parse_ok(&assignment_chain(MAX_PARSE_DEPTH as usize / 4));
    let _ = parse_ok(&nested_blocks(MAX_PARSE_DEPTH as usize / 4));
    let _ = parse_ok(&nested_types(MAX_PARSE_DEPTH as usize / 4));
    let _ = parse_ok(&flat_fold_chain(MAX_PARSE_OPERATORS / 4));
}

/// Budgets are aligned with the walker: the deepest tree the parser admits for
/// every nesting shape is walked without reporting the depth limit.
#[test]
fn deepest_admitted_nesting_stays_inside_the_walker_budget() {
    let shapes: [NestingShape; 5] = [
        ("parens", nested_parens),
        ("ternary", ternary_chain),
        ("assignment", assignment_chain),
        ("blocks", nested_blocks),
        ("types", nested_types),
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
