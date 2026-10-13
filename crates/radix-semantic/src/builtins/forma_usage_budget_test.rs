use super::program_uses_forma;
use crate::{Resolver, TypeTable};
use radix_lexer::{Interner, Span};
use radix_syntax::{
    BinOp, BinaryExpr, Expr, ExprKind, ExprStmt, Ident, IfBody, IncipitStmt, Literal,
    MAX_VISIT_DEPTH, MemberExpr, Program, Stmt, StmtKind,
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

fn registered_resolver() -> (Resolver, Interner) {
    let mut types = TypeTable::new();
    let mut resolver = Resolver::new();
    let mut interner = Interner::new();
    super::register(&mut resolver, &mut types, &mut interner);
    (resolver, interner)
}

#[test]
fn shallow_program_without_forma_use_injects_nothing() {
    let (resolver, _) = registered_resolver();
    let program = program_with(Expr {
        id: 0,
        kind: ExprKind::Literal(Literal::Int(1)),
        span: Span::default(),
    });
    assert!(!program_uses_forma(&program, &resolver));
}

#[test]
fn scan_fails_closed_when_the_walker_stops() {
    let (resolver, _) = registered_resolver();
    let program = program_with(deep_spine(MAX_VISIT_DEPTH + 8));
    assert!(
        program_uses_forma(&program, &resolver),
        "an incomplete scan must inject the builtin rather than report no forma use"
    );
}

#[test]
fn member_named_forma_is_not_a_genus_reference() {
    let (resolver, mut interner) = registered_resolver();
    let expr = Expr {
        id: 0,
        kind: ExprKind::Member(MemberExpr {
            object: Box::new(Expr {
                id: 0,
                kind: ExprKind::Literal(Literal::Int(1)),
                span: Span::default(),
            }),
            member: Ident {
                name: interner.intern("forma"),
                span: Span::default(),
            },
        }),
        span: Span::default(),
    };
    assert!(
        !program_uses_forma(&program_with(expr), &resolver),
        "a .forma(...) member name must not inject the builtin"
    );
}

#[test]
fn bare_forma_identifier_is_a_genus_reference() {
    let (resolver, mut interner) = registered_resolver();
    let expr = Expr {
        id: 0,
        kind: ExprKind::Ident(Ident {
            name: interner.intern("forma"),
            span: Span::default(),
        }),
        span: Span::default(),
    };
    assert!(program_uses_forma(&program_with(expr), &resolver));
}

fn incipit_with_exitus(exitus: Expr) -> Program {
    let body = Stmt {
        id: 0,
        kind: StmtKind::Expr(ExprStmt {
            expr: Box::new(Expr {
                id: 0,
                kind: ExprKind::Literal(Literal::Int(1)),
                span: Span::default(),
            }),
        }),
        span: Span::default(),
        annotations: Vec::new(),
        leading_trivia: Vec::new(),
    };
    Program {
        statements: vec![Stmt {
            id: 0,
            kind: StmtKind::Incipit(IncipitStmt {
                is_async: false,
                body: IfBody::Ergo(Box::new(body)),
                args: None,
                exitus: Some(Box::new(exitus)),
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
fn forma_literal_only_in_incipit_exitus_is_a_use() {
    let (resolver, mut interner) = registered_resolver();
    let exitus = Expr {
        id: 0,
        kind: ExprKind::Literal(Literal::Forma(interner.intern("select 1"))),
        span: Span::default(),
    };
    assert!(
        program_uses_forma(&incipit_with_exitus(exitus), &resolver),
        "a forma literal reachable only through the entry exitus must inject the builtin"
    );
}
