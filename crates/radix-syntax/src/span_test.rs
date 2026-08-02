//! Spanned trait implementation coverage: every AST node type returns its
//! parse-time `Span` value.

use super::Spanned;
use crate::{
    Annotation, AnnotationKind, Expr, ExprKind, Ident, Literal, Program, Stmt, StmtKind, TypeExpr,
    TypeExprKind,
};
use radix_lexer::{Span, Symbol};

#[test]
fn spanned_impls_return_direct_parse_spans() {
    assert_eq!(program().span(), Span::new(1, 9));
    assert_eq!(stmt().span(), Span::new(2, 8));
    assert_eq!(expr().span(), Span::new(3, 7));
    assert_eq!(ident().span(), Span::new(4, 6));
    assert_eq!(ty().span(), Span::new(5, 10));
    assert_eq!(annotation().span(), Span::new(6, 11));
}

fn program() -> Program {
    Program {
        statements: Vec::new(),
        span: Span::new(1, 9),
    }
}

fn stmt() -> Stmt {
    Stmt {
        id: 1,
        kind: StmtKind::Expr(crate::ExprStmt {
            expr: Box::new(expr()),
        }),
        span: Span::new(2, 8),
        annotations: Vec::new(),
        leading_trivia: Vec::new(),
    }
}

fn expr() -> Expr {
    Expr {
        id: 2,
        kind: ExprKind::Literal(Literal::Int(1)),
        span: Span::new(3, 7),
    }
}

fn ident() -> Ident {
    Ident {
        name: Symbol(1),
        span: Span::new(4, 6),
    }
}

fn ty() -> TypeExpr {
    TypeExpr {
        nullable: false,
        mode: None,
        kind: TypeExprKind::Infer,
        span: Span::new(5, 10),
    }
}

fn annotation() -> Annotation {
    Annotation {
        kind: AnnotationKind::Futura,
        span: Span::new(6, 11),
    }
}
