use super::function_body_contains_ad;
use radix_module::lexer::{Span, Symbol};
use radix_module::syntax::{
    AdExpr, BinOp, BinaryExpr, BlockStmt, Expr, ExprKind, ExprStmt, Literal, MAX_VISIT_DEPTH, Stmt,
    StmtKind,
};

fn literal() -> Expr {
    Expr {
        id: 0,
        kind: ExprKind::Literal(Literal::Int(1)),
        span: Span::default(),
    }
}

/// Left-nested `1 + (1 + (…))` spine; its deepest leaf is `depth` nodes down.
fn deep_spine(depth: usize) -> Expr {
    let mut expr = literal();
    for i in 0..depth {
        let id = u32::try_from(i).unwrap_or(u32::MAX);
        expr = Expr {
            id,
            kind: ExprKind::Binary(BinaryExpr {
                op: BinOp::Add,
                lhs: Box::new(expr),
                rhs: Box::new(literal()),
            }),
            span: Span::default(),
        };
    }
    expr
}

fn block_with(expr: Expr) -> BlockStmt {
    BlockStmt {
        statements: vec![Stmt {
            id: 0,
            kind: StmtKind::Expr(ExprStmt {
                expr: Box::new(expr),
            }),
            span: Span::default(),
            annotations: Vec::new(),
            leading_trivia: Vec::new(),
        }],
        end_trivia: Vec::new(),
        span: Span::default(),
    }
}

#[test]
fn body_without_host_stream_is_not_failable() {
    let block = block_with(literal());
    assert!(!function_body_contains_ad(Some(&block)).expect("shallow scan answers"));
}

#[test]
fn body_with_host_stream_is_failable() {
    let block = block_with(Expr {
        id: 0,
        kind: ExprKind::Ad(Box::new(AdExpr {
            path: Symbol(0),
            opener: None,
            span: Span::default(),
        })),
        span: Span::default(),
    });
    assert!(function_body_contains_ad(Some(&block)).expect("shallow scan answers"));
}

#[test]
fn body_the_walker_stops_in_is_reported() {
    let block = block_with(deep_spine(MAX_VISIT_DEPTH + 8));
    let result = function_body_contains_ad(Some(&block));
    let diagnostic = result.expect_err("an incomplete scan must be reported, not guessed");
    assert!(
        diagnostic.is_error(),
        "the incomplete scan must be reported as an error diagnostic"
    );
}
