//! Counting-contract coverage for the complexity pass.
//!
//! One test per contract row in
//! `docs/factory/complexity-diagnostics/goal.md` §Proposal, including the
//! three audit-added rows (`Conversio { recovery }`, `Rumpe`/`Perge`, and
//! the fold-family `vel` tail via `Reduction.identity`). Emission tests
//! assert diagnostic identity — kind, `issue`, named args — never rendered
//! text.

use super::complexity::{
    DEFAULT_COMPLEXITY_BUDGET, DEFAULT_KERNEL_COMPLEXITY_BUDGET, complexity, function_complexity,
};
use super::{SemanticErrorKind, WarningKind};
use crate::SemanticError;
use crate::catalog_semantic::semantic_spec;
use crate::test_support::{args_have_no_legacy_message, test_span};
use radix_hir::{
    DefId, HirBinOp, HirBlock, HirCape, HirCasuArm, HirConversioTarget, HirCustodi,
    HirCustodiClause, HirExpression, HirExpressionKind, HirFunction, HirId, HirItem, HirItemKind,
    HirIteraMode, HirLiteral, HirModule, HirNonNullKind, HirOptionalChainKind, HirPattern,
    HirReductionOp, HirStatement, HirStatementKind,
};
use radix_lexer::{Interner, Symbol};
use radix_syntax::CallablePosture;

// ── Fixture helpers ──────────────────────────────────────────────────────────

fn lit() -> HirExpression {
    expr(HirExpressionKind::Literal(HirLiteral::Int(1)))
}

fn expr(kind: HirExpressionKind) -> HirExpression {
    HirExpression {
        id: HirId(0),
        kind,
        ty: None,
        span: test_span(),
    }
}

fn boxed(kind: HirExpressionKind) -> Box<HirExpression> {
    Box::new(expr(kind))
}

fn block(exprs: Vec<HirExpression>) -> HirBlock {
    // Decision points live in statements and tail expressions alike; a
    // multi-expr body threads extras through Local initializers so every
    // fixture expression is reachable to the walk.
    let mut statements = Vec::new();
    for value in exprs {
        statements.push(HirStatement {
            id: HirId(0),
            kind: HirStatementKind::Redde(Some(value)),
            span: test_span(),
        });
    }
    HirBlock {
        statements,
        expr: None,
        span: test_span(),
        ..Default::default()
    }
}

fn function(body: HirBlock) -> HirFunction {
    HirFunction {
        name: Symbol(1),
        type_params: Vec::new(),
        params: Vec::new(),
        cli_args: None,
        ret_ty: None,
        err_ty: None,
        body: Some(body),
        posture: CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    }
}

/// Score of one declared function whose body is built from `exprs`.
fn score(exprs: Vec<HirExpression>) -> u32 {
    function_complexity(&function(block(exprs))).expect("scored function")
}

fn module_with_function(name: &str, body: HirBlock) -> (HirModule, Interner) {
    module_with_function_selected(name, body, false)
}

/// [`module_with_function`] with the kernel classification pinned
/// (`HirFunction.is_kernel`) so kernel-budget selection is under test.
fn module_with_function_selected(
    name: &str,
    body: HirBlock,
    is_kernel: bool,
) -> (HirModule, Interner) {
    let mut interner = Interner::new();
    let mut func = function(body);
    func.name = interner.intern(name);
    func.is_kernel = is_kernel;
    let module = HirModule {
        items: vec![HirItem {
            annotations: Vec::new(),
            id: HirId(0),
            def_id: DefId(1),
            kind: HirItemKind::Function(Box::new(func)),
            span: test_span(),
        }],
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    };
    (module, interner)
}

fn empty_module() -> HirModule {
    HirModule {
        items: Vec::new(),
        entry: None,
        entry_annotations: Vec::new(),
        entry_args_name: None,
        entry_is_async: false,
    }
}

fn find_complexity_warning(findings: &[SemanticError]) -> &SemanticError {
    findings
        .iter()
        .find(|f| f.kind == SemanticErrorKind::Warning(WarningKind::ComplexityBudgetExceeded))
        .expect("complexity budget warning present")
}

/// `count` standalone `si` branches: base 1 + one decision point per branch.
fn si_branches(count: usize) -> Vec<HirExpression> {
    (0..count)
        .map(|_| {
            expr(HirExpressionKind::Si {
                cond: boxed(HirExpressionKind::Literal(HirLiteral::Bool(true))),
                then_block: block(vec![]),
                then_catch: None,
                else_block: None,
            })
        })
        .collect()
}

fn arg<'a>(finding: &'a SemanticError, name: &str) -> &'a str {
    finding
        .args
        .iter()
        .find(|arg| arg.name == name)
        .map_or("", |arg| arg.value.as_str())
}

// ── Contract: score base ─────────────────────────────────────────────────────

#[test]
fn base_score_is_one_for_plain_body() {
    assert_eq!(score(vec![lit()]), 1);
}

// ── Contract rows: counted constructs ────────────────────────────────────────

#[test]
fn row_conditional_si_counts_one_each() {
    let si = || {
        expr(HirExpressionKind::Si {
            cond: boxed(HirExpressionKind::Literal(HirLiteral::Bool(true))),
            then_block: block(vec![]),
            then_catch: None,
            else_block: None,
        })
    };
    assert_eq!(score(vec![si()]), 2, "one si: base 1 + 1");

    // Each `else if` link is its own `Si` nested in the else block.
    let linked = expr(HirExpressionKind::Si {
        cond: boxed(HirExpressionKind::Literal(HirLiteral::Bool(true))),
        then_block: block(vec![]),
        then_catch: None,
        else_block: Some(block(vec![si()])),
    });
    assert_eq!(score(vec![linked]), 3, "si + else-if link: base 1 + 2");
}

#[test]
fn row_discerne_counts_one_per_match_not_per_arm() {
    let arm = || HirCasuArm {
        patterns: vec![HirPattern::Wildcard],
        body: lit(),
        span: test_span(),
    };
    let discerne = |arms: usize| {
        expr(HirExpressionKind::Discerne {
            scrutinees: vec![lit()],
            arms: (0..arms).map(|_| arm()).collect(),
            exhaustive: false,
        })
    };
    assert_eq!(score(vec![discerne(1)]), 2);
    assert_eq!(
        score(vec![discerne(5)]),
        2,
        "per-match, not per-arm: five arms still count once"
    );
    assert_eq!(score(vec![discerne(2), discerne(2)]), 3, "two matches");
}

#[test]
fn row_loops_count_one_each() {
    let body_block = block(vec![]);
    assert_eq!(
        score(vec![expr(HirExpressionKind::Loop(body_block.clone()))]),
        2,
        "Loop"
    );
    assert_eq!(
        score(vec![expr(HirExpressionKind::Dum(
            boxed(HirExpressionKind::Literal(HirLiteral::Bool(true))),
            body_block.clone()
        ))]),
        2,
        "Dum"
    );
    assert_eq!(
        score(vec![expr(HirExpressionKind::Itera {
            mode: HirIteraMode::Ex,
            coordinates: Vec::new(),
            binding: DefId(1),
            binding_name: Symbol(1),
            iter: boxed(HirExpressionKind::Literal(HirLiteral::Nil)),
            body: body_block.clone(),
            mutable: false,
            filum: None,
        })]),
        2,
        "Itera"
    );
    assert_eq!(
        score(vec![expr(HirExpressionKind::IteraLockstep {
            sources: vec![lit(), lit()],
            coordinates: Vec::new(),
            bindings: vec![(DefId(1), Symbol(1)), (DefId(2), Symbol(2))],
            body: body_block.clone(),
            mutable: false,
        })]),
        2,
        "IteraLockstep"
    );
    assert_eq!(
        score(vec![expr(HirExpressionKind::IteraRange {
            ranges: vec![lit(), lit()],
            bindings: vec![(DefId(1), Symbol(1)), (DefId(2), Symbol(2))],
            body: body_block.clone(),
            mutable: false,
        })]),
        2,
        "IteraRange"
    );
}

#[test]
fn row_short_circuit_connectives_count_one_each() {
    let operand = || boxed(HirExpressionKind::Literal(HirLiteral::Bool(true)));
    let and = || {
        expr(HirExpressionKind::Binary(
            HirBinOp::And,
            operand(),
            operand(),
        ))
    };
    assert_eq!(score(vec![and()]), 2, "single And");
    assert_eq!(
        score(vec![expr(HirExpressionKind::Binary(
            HirBinOp::Or,
            boxed(HirExpressionKind::Binary(
                HirBinOp::And,
                operand(),
                operand()
            )),
            operand(),
        ))]),
        3,
        "Or(And) chain: one each"
    );
}

#[test]
fn row_coalesce_vel_counts_one_each() {
    let score_one = score(vec![expr(HirExpressionKind::Binary(
        HirBinOp::Coalesce,
        boxed(HirExpressionKind::Literal(HirLiteral::Nil)),
        boxed(HirExpressionKind::Literal(HirLiteral::Int(0))),
    ))]);
    assert_eq!(score_one, 2, "vel coalesce");
}

#[test]
fn row_error_channel_catches_count() {
    let cape = || HirCape {
        binding_def_id: DefId(2),
        binding_name: Symbol(2),
        binding_ty: None,
        body: block(vec![]),
        span: test_span(),
    };
    assert_eq!(
        score(vec![expr(HirExpressionKind::Handled {
            body: block(vec![]),
            catch: Box::new(cape()),
        })]),
        2,
        "fac … cape"
    );
    assert_eq!(
        score(vec![expr(HirExpressionKind::Si {
            cond: boxed(HirExpressionKind::Literal(HirLiteral::Bool(true))),
            then_block: block(vec![]),
            then_catch: Some(Box::new(cape())),
            else_block: None,
        })]),
        3,
        "si with attached then_catch: conditional +1 and catch +1"
    );
}

#[test]
fn row_trap_capture_boundary_counts() {
    assert_eq!(
        score(vec![expr(HirExpressionKind::Trap(Box::new(block(vec![]))))]),
        2,
        "capta {{ }}"
    );
}

#[test]
fn row_custodi_clauses_count_one_each() {
    let clause = || HirCustodiClause {
        cond: expr(HirExpressionKind::Literal(HirLiteral::Bool(true))),
        body: block(vec![]),
        span: test_span(),
    };
    let custodi = |n: usize| {
        expr(HirExpressionKind::Block(HirBlock {
            statements: vec![HirStatement {
                id: HirId(0),
                kind: HirStatementKind::Custodi(HirCustodi {
                    clauses: (0..n).map(|_| clause()).collect(),
                }),
                span: test_span(),
            }],
            expr: None,
            span: test_span(),
            ..Default::default()
        }))
    };
    assert_eq!(score(vec![custodi(1)]), 2, "one guard clause");
    assert_eq!(score(vec![custodi(3)]), 4, "three guard clauses");
}

#[test]
fn row_conversio_assign_recovery_counts_when_present() {
    let target = || boxed(HirExpressionKind::Path(DefId(3)));
    let source = || boxed(HirExpressionKind::Literal(HirLiteral::Int(1)));
    assert_eq!(
        score(vec![expr(HirExpressionKind::ConversioAssign {
            target: target(),
            source: source(),
            recovery: None,
            params: Vec::new(),
        })]),
        1,
        "no ⊥ fallback: 0"
    );
    assert_eq!(
        score(vec![expr(HirExpressionKind::ConversioAssign {
            target: target(),
            source: source(),
            recovery: Some(source()),
            params: Vec::new(),
        })]),
        2,
        "⊥ fallback present: +1"
    );
}

#[test]
fn row_conversio_expression_recovery_counts_when_present() {
    let conv = |recovery: Option<Box<HirExpression>>| {
        expr(HirExpressionKind::Conversio {
            route: None,
            source: boxed(HirExpressionKind::Literal(HirLiteral::Int(1))),
            target: HirConversioTarget::Type(radix_types::TypeId(1)),
            params: Vec::new(),
            recovery,
            implicit: None,
        })
    };
    assert_eq!(score(vec![conv(None)]), 1, "no ⊥ fallback: 0");
    assert_eq!(
        score(vec![conv(Some(boxed(HirExpressionKind::Literal(
            HirLiteral::Int(0)
        ))))]),
        2,
        "⊥ fallback present: +1"
    );
}

#[test]
fn row_body_carrying_folds_count() {
    let binding = (DefId(1), Symbol(1));
    assert_eq!(
        score(vec![expr(HirExpressionKind::Reduction {
            op: HirReductionOp::Sum,
            source: boxed(HirExpressionKind::Literal(HirLiteral::Nil)),
            coordinates: Vec::new(),
            group: None,
            filum: None,
            binding: Some(binding.clone()),
            body: Some(block(vec![])),
            identity: None,
            mutable: false,
        })]),
        2,
        "Reduction per-element body"
    );
    assert_eq!(
        score(vec![expr(HirExpressionKind::FirstMatch {
            source: boxed(HirExpressionKind::Literal(HirLiteral::Nil)),
            coordinates: Vec::new(),
            binding,
            body: block(vec![]),
        })]),
        2,
        "FirstMatch per-candidate body"
    );
}

#[test]
fn row_reduction_without_body_counts_zero() {
    assert_eq!(
        score(vec![expr(HirExpressionKind::Reduction {
            op: HirReductionOp::Max,
            source: boxed(HirExpressionKind::Literal(HirLiteral::Nil)),
            coordinates: Vec::new(),
            group: None,
            filum: None,
            binding: None,
            body: None,
            identity: None,
            mutable: false,
        })]),
        1,
        "no-body reduction: 0"
    );
}

#[test]
fn row_reduction_identity_tail_counts_when_present() {
    assert_eq!(
        score(vec![expr(HirExpressionKind::Reduction {
            op: HirReductionOp::Max,
            source: boxed(HirExpressionKind::Literal(HirLiteral::Nil)),
            coordinates: Vec::new(),
            group: None,
            filum: None,
            binding: None,
            body: None,
            identity: Some(boxed(HirExpressionKind::Literal(HirLiteral::Int(0)))),
            mutable: false,
        })]),
        2,
        "reduction identity tail: +1"
    );
}

// ── Contract rows: excluded constructs ───────────────────────────────────────

#[test]
fn row_loop_jumps_rumpe_perge_do_not_count() {
    let mut body = block(vec![]);
    body.statements.push(HirStatement {
        id: HirId(0),
        kind: HirStatementKind::Rumpe,
        span: test_span(),
    });
    body.statements.push(HirStatement {
        id: HirId(0),
        kind: HirStatementKind::Perge,
        span: test_span(),
    });
    assert_eq!(
        function_complexity(&function(body)),
        Some(1),
        "break and continue are not decision points"
    );
}

#[test]
fn row_bitwise_operators_do_not_count() {
    let operand = || boxed(HirExpressionKind::Literal(HirLiteral::Int(1)));
    for op in [HirBinOp::BitAnd, HirBinOp::BitOr, HirBinOp::BitXor] {
        assert_eq!(
            score(vec![expr(HirExpressionKind::Binary(
                op,
                operand(),
                operand()
            ))]),
            1,
            "bitwise {op:?} has no short-circuit branch"
        );
    }
}

#[test]
fn row_null_safe_access_sugar_does_not_count() {
    assert_eq!(
        score(vec![
            expr(HirExpressionKind::OptionalChain(
                boxed(HirExpressionKind::Literal(HirLiteral::Nil)),
                HirOptionalChainKind::Member(Symbol(3)),
            )),
            expr(HirExpressionKind::NonNull(
                boxed(HirExpressionKind::Literal(HirLiteral::Nil)),
                HirNonNullKind::Member(Symbol(3)),
            )),
        ]),
        1,
        "OptionalChain and NonNull are access sugar"
    );
}

#[test]
fn row_everything_else_does_not_count() {
    let plain = score(vec![
        // calls
        expr(HirExpressionKind::Call(
            boxed(HirExpressionKind::Path(DefId(1))),
            Vec::new(),
            Vec::new(),
            None,
        )),
        expr(HirExpressionKind::MethodCall(
            boxed(HirExpressionKind::Path(DefId(1))),
            Symbol(2),
            Vec::new(),
            Vec::new(),
            None,
        )),
        expr(HirExpressionKind::Gradient {
            call: boxed(HirExpressionKind::Call(
                boxed(HirExpressionKind::Path(DefId(1))),
                Vec::new(),
                Vec::new(),
                None,
            )),
            selected_arguments: Vec::new(),
        }),
        // output and input
        expr(HirExpressionKind::Scribe(
            radix_hir::HirScribeKind::Nota,
            vec![lit()],
        )),
        expr(HirExpressionKind::Scriptum(Symbol(2), vec![lit()])),
        expr(HirExpressionKind::ReadLine(true)),
        // aggregates and plain structure
        expr(HirExpressionKind::Array(vec![
            radix_hir::HirArrayElement::Expr(lit()),
        ])),
        expr(HirExpressionKind::Tuple(vec![lit(), lit()], None)),
        expr(HirExpressionKind::Field(
            boxed(HirExpressionKind::Path(DefId(1))),
            Symbol(2),
        )),
        expr(HirExpressionKind::Index(
            boxed(HirExpressionKind::Path(DefId(1))),
            boxed(HirExpressionKind::Literal(HirLiteral::Int(0))),
        )),
        // markers, assertions, panics
        expr(HirExpressionKind::Praefixum(boxed(
            HirExpressionKind::Literal(HirLiteral::Int(1)),
        ))),
        expr(HirExpressionKind::Adfirma(
            boxed(HirExpressionKind::Literal(HirLiteral::Bool(true))),
            None,
        )),
        expr(HirExpressionKind::Panic(boxed(HirExpressionKind::Literal(
            HirLiteral::Int(1),
        )))),
        expr(HirExpressionKind::Throw(boxed(HirExpressionKind::Literal(
            HirLiteral::Int(1),
        )))),
        // async/generator carriers
        expr(HirExpressionKind::Cede(boxed(HirExpressionKind::Literal(
            HirLiteral::Int(1),
        )))),
        expr(HirExpressionKind::Reddet(boxed(
            HirExpressionKind::Literal(HirLiteral::Int(1)),
        ))),
        expr(HirExpressionKind::Tacebit(boxed(
            HirExpressionKind::Literal(HirLiteral::Int(1)),
        ))),
        expr(HirExpressionKind::Yield(boxed(HirExpressionKind::Literal(
            HirLiteral::Int(1),
        )))),
    ]);
    assert_eq!(plain, 1, "no listed zero-row node is a decision point");
}

// ── Contract laws across rows ────────────────────────────────────────────────

#[test]
fn closures_fold_into_the_parent_score() {
    let closure = expr(HirExpressionKind::Clausura(
        Vec::new(),
        None,
        None,
        boxed(HirExpressionKind::Si {
            cond: boxed(HirExpressionKind::Literal(HirLiteral::Bool(true))),
            then_block: block(vec![]),
            then_catch: None,
            else_block: None,
        }),
    ));
    assert_eq!(
        score(vec![closure]),
        2,
        "the closure's si counts toward the enclosing function"
    );
}

#[test]
fn bodyless_functions_are_skipped() {
    let mut func = function(block(vec![]));
    func.body = None;
    assert_eq!(function_complexity(&func), None);

    let (module, interner) = (empty_module(), Interner::new());
    assert!(
        complexity(
            &module,
            &interner,
            DEFAULT_COMPLEXITY_BUDGET,
            DEFAULT_KERNEL_COMPLEXITY_BUDGET
        )
        .is_empty(),
        "no declared function, no finding"
    );
}

// ── Emission identity (never rendered text) ──────────────────────────────────

#[test]
fn over_budget_function_warns_with_identity() {
    // Score 3: base + si + dum.
    let body = block(vec![
        expr(HirExpressionKind::Si {
            cond: boxed(HirExpressionKind::Literal(HirLiteral::Bool(true))),
            then_block: block(vec![]),
            then_catch: None,
            else_block: None,
        }),
        expr(HirExpressionKind::Dum(
            boxed(HirExpressionKind::Literal(HirLiteral::Bool(false))),
            block(vec![]),
        )),
    ]);
    let (module, interner) = module_with_function("solve", body);
    let findings = complexity(&module, &interner, 2, DEFAULT_KERNEL_COMPLEXITY_BUDGET);
    assert_eq!(findings.len(), 1, "exactly one finding");

    let finding = find_complexity_warning(&findings);
    assert!(args_have_no_legacy_message(&finding.args));
    assert!(!finding.is_error(), "warn-only");
    assert_eq!(finding.issue(), Some("complexity_budget_exceeded"));
    assert_eq!(arg(finding, "function"), "solve");
    assert_eq!(arg(finding, "score"), "3");
    assert_eq!(arg(finding, "budget"), "2");
    assert_eq!(
        finding.args.len(),
        4,
        "identity args only: issue, function, score, budget"
    );
}

#[test]
fn at_baked_default_score_does_not_warn() {
    // Boundary at the census-derived baked default: 11 branches score 12 —
    // exactly the budget.
    let (module, interner) = module_with_function("edge", block(si_branches(11)));
    assert!(
        complexity(
            &module,
            &interner,
            DEFAULT_COMPLEXITY_BUDGET,
            DEFAULT_KERNEL_COMPLEXITY_BUDGET
        )
        .is_empty()
    );
    assert_eq!(score(si_branches(11)), DEFAULT_COMPLEXITY_BUDGET);
}

#[test]
fn one_over_baked_default_warns() {
    // 12 branches score 13: the first score that warns at the baked default.
    let (module, interner) = module_with_function("over", block(si_branches(12)));
    let findings = complexity(
        &module,
        &interner,
        DEFAULT_COMPLEXITY_BUDGET,
        DEFAULT_KERNEL_COMPLEXITY_BUDGET,
    );
    assert_eq!(findings.len(), 1, "exactly one finding");
    let finding = find_complexity_warning(&findings);
    assert_eq!(arg(finding, "score"), "13");
    assert_eq!(arg(finding, "budget"), "12");
}

#[test]
fn complexity_warning_takes_catalog_code_warn027() {
    assert_eq!(
        semantic_spec(SemanticErrorKind::Warning(
            WarningKind::ComplexityBudgetExceeded
        ))
        .code,
        "WARN027"
    );
    assert_eq!(
        DEFAULT_COMPLEXITY_BUDGET, 12,
        "census-derived baked default (pooled p95 = 11)"
    );
}

// ── Kernel advisory budget (CC-U6) ───────────────────────────────────────────

/// Kernel boundary fixture: score 2 = base + one `si` (the census kernel
/// max), score 3 = base + two `si` (the first future kernel that trips).
fn kernel_body(si_count: usize) -> HirBlock {
    block(si_branches(si_count))
}

#[test]
fn kernel_score_two_is_silent_at_baked_kernel_default() {
    assert_eq!(
        function_complexity(&function(kernel_body(1))),
        Some(2),
        "fixture sanity: one si scores 2 (the census kernel max)"
    );
    let (module, interner) = module_with_function_selected("still", kernel_body(1), true);
    assert!(
        complexity(
            &module,
            &interner,
            DEFAULT_COMPLEXITY_BUDGET,
            DEFAULT_KERNEL_COMPLEXITY_BUDGET
        )
        .is_empty()
    );
}

#[test]
fn kernel_score_three_warns_with_kernel_arg() {
    assert_eq!(
        function_complexity(&function(kernel_body(2))),
        Some(3),
        "fixture sanity: two sis score 3 (the first tripping score)"
    );
    let (module, interner) = module_with_function_selected("dense", kernel_body(2), true);
    let findings = complexity(
        &module,
        &interner,
        DEFAULT_COMPLEXITY_BUDGET,
        DEFAULT_KERNEL_COMPLEXITY_BUDGET,
    );
    assert_eq!(findings.len(), 1, "exactly one finding");

    let finding = find_complexity_warning(&findings);
    assert!(!finding.is_error(), "advisory: warn-only, never admission");
    assert_eq!(finding.issue(), Some("complexity_budget_exceeded"));
    assert_eq!(arg(finding, "function"), "dense");
    assert_eq!(arg(finding, "score"), "3");
    assert_eq!(
        arg(finding, "budget"),
        "2",
        "the kernel budget is the applicable one"
    );
    assert_eq!(arg(finding, "kernel"), "true", "kernel arg marks the tier");
    assert_eq!(
        finding.args.len(),
        5,
        "kernel identity args: issue, function, score, budget, kernel"
    );
}

#[test]
fn non_kernel_finding_has_no_kernel_arg() {
    // The same score-3 body as a non-kernel function at budget 2 warns
    // without the kernel arg — the arg is tier evidence, not a constant.
    let (module, interner) = module_with_function("plain", kernel_body(2));
    let findings = complexity(&module, &interner, 2, DEFAULT_KERNEL_COMPLEXITY_BUDGET);
    assert_eq!(findings.len(), 1);
    let finding = find_complexity_warning(&findings);
    assert_eq!(arg(finding, "budget"), "2");
    assert!(
        finding.args.iter().all(|arg| arg.name != "kernel"),
        "non-kernel findings never carry the kernel arg"
    );
}

#[test]
fn kernel_budget_selection_is_independent_of_the_regular_budget() {
    // Kernel function: measured only by the kernel budget. The regular
    // budget neither silences nor tightens it.
    let (kernel_module, interner) = module_with_function_selected("k", kernel_body(2), true);
    assert!(
        complexity(&kernel_module, &interner, 2, 12).is_empty(),
        "kernel score 3 under kernel budget 12: silent — the regular budget 2 must not leak in"
    );
    assert_eq!(
        complexity(&kernel_module, &interner, 12, 2).len(),
        1,
        "kernel score 3 over kernel budget 2: warns — the regular budget 12 must not loosen it"
    );

    // Non-kernel function: measured only by the regular budget.
    let (module, interner) = module_with_function("host", kernel_body(2));
    assert!(
        complexity(&module, &interner, 12, 2).is_empty(),
        "non-kernel score 3 under regular budget 12: silent — the kernel budget 2 must not leak in"
    );
    assert_eq!(
        complexity(&module, &interner, 2, 12).len(),
        1,
        "non-kernel score 3 over regular budget 2: warns — the kernel budget must not silence it"
    );
}

#[test]
fn kernel_budget_constant_is_census_recommendation() {
    // Census kernel subset (84 kernel fns, max 2, six at 2): the budget is
    // a future tripwire firing on nothing measured today.
    assert_eq!(DEFAULT_KERNEL_COMPLEXITY_BUDGET, 2);
}
