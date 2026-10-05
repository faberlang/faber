//! Focused recognizer and proof-source tests for APA-L3.

use super::{
    GATE_PURITY_PRELUDE, GateHoleIssue, GatePurityFinding, PurityTerm, emit_gate_purity_theorem, gate_purity_finding,
};
use crate::hir::{
    DefId, HirBinOp, HirBlock, HirCallArg, HirExpression, HirExpressionKind, HirFunction, HirId, HirLiteral, HirLocal,
    HirStatement, HirStatementKind,
};
use crate::lexer::Span;

fn expr(id: u32, kind: HirExpressionKind) -> HirExpression {
    HirExpression { id: HirId(id), kind, ty: None, span: Span::new(0, 1) }
}

fn int_expr(id: u32, value: u64) -> HirExpression {
    expr(id, HirExpressionKind::Literal(HirLiteral::Int(value)))
}

fn path_expr(id: u32, def_id: u32) -> HirExpression {
    expr(id, HirExpressionKind::Path(DefId(def_id)))
}

fn call(id: u32, callee: HirExpression, args: Vec<HirExpression>) -> HirExpression {
    expr(
        id,
        HirExpressionKind::Call(
            Box::new(callee),
            Vec::new(),
            args.into_iter()
                .map(|arg| HirCallArg { name: None, spread: false, span: arg.span, expr: arg })
                .collect(),
            None,
        ),
    )
}

fn local(id: u32, init: HirExpression) -> HirStatement {
    HirStatement {
        id: HirId(id),
        kind: HirStatementKind::Local(HirLocal {
            def_id: DefId(id + 100),
            name: crate::lexer::Symbol(id + 1),
            ty: None,
            init: Some(init),
            mutable: false,
            is_await: false,
            runtime_provided: false,
            await_store: None,
            size_decl: false,
            size_static: false,
        }),
        span: Span::new(0, 1),
    }
}

fn redde(id: u32, value: HirExpression) -> HirStatement {
    HirStatement { id: HirId(id), kind: HirStatementKind::Redde(Some(value)), span: Span::new(0, 1) }
}

fn function(statements: Vec<HirStatement>) -> HirFunction {
    HirFunction {
        name: crate::lexer::Symbol(1),
        type_params: Vec::new(),
        params: Vec::new(),
        cli_args: None,
        ret_ty: None,
        err_ty: None,
        body: Some(HirBlock { statements, expr: None, span: Span::new(0, 1), ..Default::default() }),
        posture: radix_syntax::CallablePosture::SyncFinite,
        is_async: false,
        is_generator: false,
        is_kernel: false,
        nucleum_fragment: false,
        is_vertex: false,
        is_fragment: false,
        test: None,
    }
}

#[test]
fn scalar_let_call_body_is_admitted_with_its_structural_term() {
    // `let bound = 3; redde f(bound, 2)` — one let, one two-argument call.
    let function = function(vec![
        local(1, int_expr(2, 3)),
        redde(3, call(4, path_expr(5, 700), vec![path_expr(6, 101), int_expr(7, 2)])),
    ]);

    assert_eq!(
        gate_purity_finding(&function),
        GatePurityFinding::Admitted(PurityTerm::Let(
            Box::new(PurityTerm::Scalar(3)),
            Box::new(PurityTerm::Call(
                Box::new(PurityTerm::Scalar(0)),
                vec![PurityTerm::Scalar(0), PurityTerm::Scalar(2)]
            ))
        ))
    );
}

#[test]
fn chained_lets_nest_right_around_the_value_term() {
    let function = function(vec![
        local(1, int_expr(2, 5)),
        local(3, path_expr(4, 101)),
        redde(5, path_expr(6, 103)),
    ]);

    assert_eq!(
        gate_purity_finding(&function),
        GatePurityFinding::Admitted(PurityTerm::Let(
            Box::new(PurityTerm::Scalar(5)),
            Box::new(PurityTerm::Let(
                Box::new(PurityTerm::Scalar(0)),
                Box::new(PurityTerm::Scalar(0))
            ))
        ))
    );
}

#[test]
fn tail_expression_bodies_end_in_the_value_term() {
    let function = function(vec![]);
    let mut function = function;
    function.body.as_mut().expect("body").expr = Some(Box::new(int_expr(9, 7)));

    assert_eq!(
        gate_purity_finding(&function),
        GatePurityFinding::Admitted(PurityTerm::Scalar(7))
    );
}

#[test]
fn awaiting_local_inside_an_admissible_body_is_a_named_gate_hole() {
    // The live gate never inspects `HirLocal::is_await`; the binding is an
    // otherwise-admissible scalar `let`.  Monadic suspension admitted by the
    // gate: named hole, never a downgraded failed proof.
    let mut awaiting = local(1, int_expr(2, 3));
    let HirStatementKind::Local(slot) = &mut awaiting.kind else {
        unreachable!()
    };
    slot.is_await = true;

    let function = function(vec![awaiting, redde(3, int_expr(4, 1))]);

    assert_eq!(
        gate_purity_finding(&function),
        GatePurityFinding::GateHole { issue: GateHoleIssue::AwaitingLocalInit }
    );
    assert_eq!(GateHoleIssue::AwaitingLocalInit.label(), "gate_hole.awaiting_local_init");
}

#[test]
fn silently_accepted_error_node_is_a_named_gate_hole() {
    // `air_purity` accepts `HirExpressionKind::Error` without a reject; the
    // model has no pure translation for it.
    let function = function(vec![redde(1, expr(2, HirExpressionKind::Error))]);

    assert_eq!(
        gate_purity_finding(&function),
        GatePurityFinding::GateHole { issue: GateHoleIssue::UntranslatableErrorNode }
    );
    assert_eq!(
        GateHoleIssue::UntranslatableErrorNode.label(),
        "gate_hole.untranslatable_error_node"
    );
}

#[test]
fn gate_rejects_suppress_holes_the_gate_never_admits() {
    // A mutable local anywhere in the body means the gate rejects the whole
    // declaration — the awaiting binding never reaches admission, so there
    // is no hole and no obligation.
    let mut mutable = local(1, int_expr(2, 3));
    let HirStatementKind::Local(slot) = &mut mutable.kind else {
        unreachable!()
    };
    slot.mutable = true;

    let mut awaiting = local(3, int_expr(4, 4));
    let HirStatementKind::Local(slot) = &mut awaiting.kind else {
        unreachable!()
    };
    slot.is_await = true;

    let function = function(vec![mutable, awaiting, redde(5, int_expr(6, 1))]);

    assert_eq!(gate_purity_finding(&function), GatePurityFinding::NoObligation);
}

#[test]
fn gate_rejected_shapes_carry_no_obligation() {
    // Mutable local, deferred init, runtime-provided binding, assignment,
    // named call argument, async function, defaulted parameter.
    let mut mutable = local(1, int_expr(2, 3));
    let HirStatementKind::Local(slot) = &mut mutable.kind else {
        unreachable!()
    };
    slot.mutable = true;
    assert_eq!(gate_purity_finding(&function(vec![mutable])), GatePurityFinding::NoObligation);

    let mut deferred = local(3, int_expr(4, 3));
    let HirStatementKind::Local(slot) = &mut deferred.kind else {
        unreachable!()
    };
    slot.init = None;
    assert_eq!(gate_purity_finding(&function(vec![deferred])), GatePurityFinding::NoObligation);

    let mut provided = local(5, int_expr(6, 3));
    let HirStatementKind::Local(slot) = &mut provided.kind else {
        unreachable!()
    };
    slot.runtime_provided = true;
    assert_eq!(gate_purity_finding(&function(vec![provided])), GatePurityFinding::NoObligation);

    let named_arg = expr(
        7,
        HirExpressionKind::Call(
            Box::new(path_expr(8, 700)),
            Vec::new(),
            vec![HirCallArg {
                name: Some(crate::lexer::Symbol(9)),
                spread: false,
                span: Span::new(0, 1),
                expr: int_expr(10, 1),
            }],
            None,
        ),
    );
    assert_eq!(
        gate_purity_finding(&function(vec![redde(11, named_arg)])),
        GatePurityFinding::NoObligation
    );

    let mut async_function = function(vec![redde(12, int_expr(13, 1))]);
    async_function.is_async = true;
    assert_eq!(gate_purity_finding(&async_function), GatePurityFinding::NoObligation);

    let mut spread_arg = expr(
        14,
        HirExpressionKind::Call(
            Box::new(path_expr(15, 700)),
            Vec::new(),
            vec![HirCallArg { name: None, spread: true, span: Span::new(0, 1), expr: int_expr(16, 1) }],
            None,
        ),
    );
    spread_arg.id = HirId(14);
    assert_eq!(
        gate_purity_finding(&function(vec![redde(17, spread_arg)])),
        GatePurityFinding::NoObligation
    );
}

#[test]
fn outside_subset_shapes_carry_no_obligation() {
    // Binary arithmetic, float literals, vacua, and control expression
    // statements are gate-admitted but outside the v1 scalar/let/call
    // subset: decline, no obligation, no float leaf ever admitted.
    let binary = expr(
        1,
        HirExpressionKind::Binary(HirBinOp::Add, Box::new(int_expr(2, 1)), Box::new(int_expr(3, 2))),
    );
    assert_eq!(
        gate_purity_finding(&function(vec![redde(4, binary)])),
        GatePurityFinding::NoObligation
    );

    let float = expr(5, HirExpressionKind::Literal(HirLiteral::Float(1.5)));
    assert_eq!(
        gate_purity_finding(&function(vec![redde(6, float)])),
        GatePurityFinding::NoObligation
    );

    let vacua = expr(7, HirExpressionKind::Vacua);
    assert_eq!(
        gate_purity_finding(&function(vec![redde(8, vacua)])),
        GatePurityFinding::NoObligation
    );

    let control = expr(
        9,
        HirExpressionKind::Block(HirBlock {
            statements: Vec::new(),
            expr: None,
            span: Span::new(0, 1),
            ..Default::default()
        }),
    );
    assert_eq!(
        gate_purity_finding(&function(vec![HirStatement {
            id: HirId(10),
            kind: HirStatementKind::Expr(control),
            span: Span::new(0, 1),
        }])),
        GatePurityFinding::NoObligation
    );
}

#[test]
fn valueless_and_bodyless_functions_carry_no_obligation() {
    // A body with only bindings (no redde/tail) has no value term.
    assert_eq!(
        gate_purity_finding(&function(vec![local(1, int_expr(2, 1))])),
        GatePurityFinding::NoObligation
    );

    // Interface-style bodyless functions decline too.
    let mut bodyless = function(Vec::new());
    bodyless.body = None;
    assert_eq!(gate_purity_finding(&bodyless), GatePurityFinding::NoObligation);
}

#[test]
fn defaulted_parameter_and_device_flags_decline() {
    let mut defaulted = function(vec![redde(1, int_expr(2, 1))]);
    defaulted.params = vec![crate::hir::HirParam {
        def_id: DefId(50),
        name: crate::lexer::Symbol(51),
        ty: crate::semantic::TypeId(0),
        mode: crate::hir::HirParamMode::Owned,
        optional: false,
        sponte: false,
        default: Some(int_expr(3, 0)),
        span: Span::new(0, 1),
    }];
    assert_eq!(gate_purity_finding(&defaulted), GatePurityFinding::NoObligation);

    let mut kernel = function(vec![redde(4, int_expr(5, 1))]);
    kernel.is_kernel = true;
    assert_eq!(gate_purity_finding(&kernel), GatePurityFinding::NoObligation);
}

#[test]
fn generated_gate_purity_proof_instantiates_the_master_correspondence() {
    let term = PurityTerm::Let(
        Box::new(PurityTerm::Scalar(3)),
        Box::new(PurityTerm::Call(
            Box::new(PurityTerm::Scalar(0)),
            vec![PurityTerm::Scalar(0), PurityTerm::Scalar(2)],
        )),
    );
    let source = format!(
        "{}\n{}",
        GATE_PURITY_PRELUDE,
        emit_gate_purity_theorem("aspect_gate_purity_9", &term)
    );

    // Real theorem shape: inhabitation plus effect exclusion at the
    // declaration's structural term, with an explicit admission derivation.
    assert!(source.contains("gatePurityCorrespondence"));
    assert!(source.contains("gateAdmittedInhabitsPureFragment"));
    assert!(source.contains("gateAdmittedCarriesNoEffects"));
    assert!(source.contains(
        "Term.letIn (Term.scalar 3) (Term.call (Term.call (Term.scalar 0) (Term.scalar 0)) (Term.scalar 2))"
    ));
    assert!(source.contains("GateAdmits.letIn (GateAdmits.scalar 3)"));
    // IO and monadic effects are excluded by name in the model.
    assert!(source.contains("| io : Effect"));
    assert!(source.contains("| monadic : Effect"));
    // Non-vacuity: the excluded forms exist in the model.
    assert!(source.contains("ioOpIsNeverAdmitted"));
    assert!(source.contains("monadicBindIsNeverAdmitted"));
    assert!(!source.contains(": True := trivial"));
    assert!(!source.contains("f32"));
    assert!(!source.contains("f64"));
    assert!(!source.contains("Float"));
    assert_eq!(
        crate::aspect::LeanAspectChecker::scan_for_sorry(&source),
        crate::aspect::SorryVerdict::Clean
    );
}

#[test]
fn generated_prelude_carries_no_float_types() {
    let lowered = GATE_PURITY_PRELUDE.to_lowercase();
    assert!(!lowered.contains("f32"));
    assert!(!lowered.contains("f64"));
    assert!(!lowered.contains(": float"));
    assert!(!lowered.contains("float _"));
}

#[test]
fn multi_argument_calls_render_curried_left() {
    let term = PurityTerm::Call(
        Box::new(PurityTerm::Scalar(0)),
        vec![PurityTerm::Scalar(1), PurityTerm::Scalar(2)],
    );
    let source = emit_gate_purity_theorem("aspect_gate_purity_4", &term);

    assert!(source.contains("Term.call (Term.call (Term.scalar 0) (Term.scalar 1)) (Term.scalar 2)"));
    assert!(source.contains(
        "GateAdmits.call (GateAdmits.call (GateAdmits.scalar 0) (GateAdmits.scalar 1)) (GateAdmits.scalar 2)"
    ));
}
