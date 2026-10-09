use super::program_needs_frame_types;
use radix_lexer::{Interner, Span};
use radix_syntax::{
    BinOp, BinaryExpr, Expr, ExprKind, ExprStmt, Literal, MAX_VISIT_DEPTH, Program, Stmt, StmtKind,
};

/// Left-nested `1 + (1 + (…))` spine; its deepest leaf is `depth` nodes down.
fn deep_spine(depth: usize) -> Expr {
    let mut expr = Expr {
        id: 0,
        kind: ExprKind::Literal(Literal::Int(1)),
        span: Span::default(),
    };
    for i in 0..depth {
        let id = u32::try_from(i).unwrap_or(u32::MAX);
        expr = Expr {
            id,
            kind: ExprKind::Binary(BinaryExpr {
                op: BinOp::Add,
                lhs: Box::new(expr),
                rhs: Box::new(Expr {
                    id,
                    kind: ExprKind::Literal(Literal::Int(1)),
                    span: Span::default(),
                }),
            }),
            span: Span::default(),
        };
    }
    expr
}

fn program_with(expr: Expr) -> Program {
    Program {
        statements: vec![Stmt {
            id: 0,
            kind: StmtKind::Expr(ExprStmt {
                expr: Box::new(expr),
            }),
            span: Span::default(),
            annotations: Vec::new(),
            leading_trivia: Vec::new(),
        }],
        span: Span::default(),
        end_trivia: Vec::new(),
        region: None,
    }
}

#[test]
fn shallow_program_without_frame_use_registers_nothing() {
    let interner = Interner::new();
    let program = program_with(Expr {
        id: 0,
        kind: ExprKind::Literal(Literal::Int(1)),
        span: Span::default(),
    });
    assert!(!program_needs_frame_types(&program, &interner, None));
}

#[test]
fn scan_fails_closed_when_the_walker_stops() {
    let interner = Interner::new();
    let program = program_with(deep_spine(MAX_VISIT_DEPTH + 8));
    assert!(
        program_needs_frame_types(&program, &interner, None),
        "an incomplete scan must register the builtins rather than report no frame use"
    );
}
