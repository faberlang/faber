//! Parser coverage for call-site access markers (`marker? expr`).

use crate::test_support::parse_ok_la;
use radix_syntax::{ExprKind, IfBody, StmtKind, TypeMode};

fn call_args(source: &str) -> Vec<(Option<TypeMode>, bool)> {
    let result = parse_ok_la(source);
    let program = result.program.as_ref().expect("program");
    let stmt = program.statements.last().expect("statement");
    let StmtKind::Incipit(entry) = &stmt.kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(block) = &entry.body else {
        panic!("expected block body");
    };
    let StmtKind::Expr(expr) = &block.statements[0].kind else {
        panic!("expected expression statement");
    };
    let ExprKind::Call(call) = &expr.expr.kind else {
        panic!("expected call, got {:?}", expr.expr.kind);
    };
    call.args
        .iter()
        .map(|arg| (arg.marker, matches!(arg.value.kind, ExprKind::Ident(_))))
        .collect()
}

#[test]
fn parses_each_call_marker() {
    let args = call_args("incipit {\n    f(in a, own b, copy c, de d, e)\n}");
    let modes: Vec<_> = args.iter().map(|(mode, _)| *mode).collect();
    assert_eq!(
        modes,
        vec![
            Some(TypeMode::In),
            Some(TypeMode::Own),
            Some(TypeMode::Copy),
            Some(TypeMode::De),
            None
        ]
    );
}

#[test]
fn marker_precedes_a_place_or_call_expression() {
    let args = call_args("incipit {\n    f(in a.b, own g(x), copy xs[0])\n}");
    assert!(args.iter().all(|(mode, _)| mode.is_some()));
}

#[test]
fn marker_words_stay_ordinary_identifiers_without_a_following_name() {
    let args = call_args("incipit {\n    f(copy, own)\n}");
    assert!(args.iter().all(|(mode, ident)| mode.is_none() && *ident));
}

#[test]
fn marker_precedes_a_literal_or_list_argument() {
    let args = call_args("incipit {\n    f(in [1, 2], own 3, copy \"s\")\n}");
    let modes: Vec<_> = args.iter().map(|(mode, _)| *mode).collect();
    assert_eq!(
        modes,
        vec![
            Some(TypeMode::In),
            Some(TypeMode::Own),
            Some(TypeMode::Copy)
        ]
    );
}

fn call_axis_flags(source: &str) -> Vec<(bool, Option<TypeMode>)> {
    let result = parse_ok_la(source);
    let program = result.program.as_ref().expect("program");
    let stmt = program.statements.last().expect("statement");
    let StmtKind::Incipit(entry) = &stmt.kind else {
        panic!("expected incipit");
    };
    let IfBody::Block(block) = &entry.body else {
        panic!("expected block body");
    };
    let StmtKind::Expr(expr) = &block.statements[0].kind else {
        panic!("expected expression statement");
    };
    let ExprKind::Call(call) = &expr.expr.kind else {
        panic!("expected call, got {:?}", expr.expr.kind);
    };
    call.args.iter().map(|arg| (arg.axis, arg.marker)).collect()
}

#[test]
fn axis_marker_parses_before_a_call_argument() {
    let args = call_axis_flags("incipit {\n    f(a, axis 1, in b)\n}");
    assert_eq!(
        args,
        vec![(false, None), (true, None), (false, Some(TypeMode::In))]
    );
}

#[test]
fn axis_marker_precedes_a_name_or_expression_argument() {
    let args = call_axis_flags("incipit {\n    f(axis n, axis k)\n}");
    assert!(args.iter().all(|(axis, mode)| *axis && mode.is_none()));
}

#[test]
fn axis_word_stays_an_ordinary_identifier_without_a_following_operand() {
    let args = call_axis_flags("incipit {\n    f(axis)\n}");
    assert_eq!(args, vec![(false, None)]);
}

#[test]
fn axis_marker_precedes_a_negative_literal() {
    let args = call_axis_flags("incipit {\n    f(axis -1)\n}");
    assert_eq!(args, vec![(true, None)]);
}

#[test]
fn axis_minus_with_spaces_stays_subtraction() {
    let args = call_axis_flags("incipit {\n    f(axis - 1)\n}");
    assert_eq!(args, vec![(false, None)]);
}

#[test]
fn axis_marker_parses_before_a_parameter_type() {
    let result = parse_ok_la("functio f(tensor<f32, [2, 3]> t, axis numerus a) → vacuum {}");
    let program = result.program.as_ref().expect("program");
    let StmtKind::Func(func) = &program.statements[0].kind else {
        panic!("expected function");
    };
    let modes: Vec<_> = func.params.iter().map(|param| param.mode).collect();
    assert_eq!(
        modes,
        vec![
            radix_syntax::AstParamMode::Owned,
            radix_syntax::AstParamMode::Axis
        ]
    );
}
