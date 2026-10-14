use super::program_needs_frame_types;
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

#[test]
fn frame_member_only_in_incipit_exitus_is_a_use() {
    let mut interner = Interner::new();
    let exitus = Expr {
        id: 0,
        kind: ExprKind::Member(MemberExpr {
            object: Box::new(Expr {
                id: 0,
                kind: ExprKind::Literal(Literal::Int(1)),
                span: Span::default(),
            }),
            member: Ident {
                name: interner.intern("meus"),
                span: Span::default(),
            },
        }),
        span: Span::default(),
    };
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
    let program = Program {
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
    };
    assert!(
        program_needs_frame_types(&program, &interner, None),
        "a frame member reachable only through the entry exitus must register the builtins"
    );
}

#[test]
fn builtin_frame_symbols_are_defined_without_debug_assertions() {
    let mut types = crate::TypeTable::new();
    let mut resolver = crate::Resolver::new();
    let mut interner = Interner::new();

    super::register(&mut resolver, &mut types, &mut interner);

    for name in ["scrinium", "status", "sermo", "meus", "tuus"] {
        assert!(
            resolver.lookup_type(interner.intern(name)).is_some(),
            "builtin frame type must be resolvable after register"
        );
    }

    let sermo = super::sermo_def_id(&resolver);
    assert!(sermo.is_some(), "sermo builtin def id must be recorded");
    assert_eq!(
        resolver.lookup_type(interner.intern("sermo")),
        super::sermo_def_id(&resolver),
        "the sermo scope entry must agree with the recorded builtin def id"
    );
}
