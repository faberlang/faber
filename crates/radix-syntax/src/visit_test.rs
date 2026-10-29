use super::{DepthExceeded, MAX_VISIT_DEPTH, Visitor, walk_expr, walk_stmt, walk_type_expr};
use crate::TypeFormLevel;
use crate::{AstMutability, ExField, PathPattern};
use crate::{
    AstParamMode, BinOp, BinaryExpr, BindingPattern, BlockStmt, CapeClause, CasuArm, ClausuraBody,
    ClausuraExpr, ContractRefExpr, DiscerneStmt, Expr, ExprKind, FacStmt, FuncDecl, FuncTypeExpr,
    GenericParamDefault, GenericParamKind, Ident, IfBody, IteraMode, IteraStmt, Literal, Param,
    Pattern, PatternBind, ProbandumBody, ProbandumDecl, SiStmt, Stmt, StmtKind, TypeExpr,
    TypeExprKind, TypeParam,
};
use crate::{PraeparaBlock, PraeparaKind};
use radix_lexer::{Span, Symbol};

/// Visitor that records depth-limit hits instead of panicking.
struct DepthProbe {
    exceeded_at: Option<usize>,
    expr_visits: usize,
    type_visits: usize,
}

impl DepthProbe {
    fn new() -> Self {
        Self {
            exceeded_at: None,
            expr_visits: 0,
            type_visits: 0,
        }
    }
}

impl Visitor for DepthProbe {
    fn on_depth_exceeded(&mut self, exceeded: DepthExceeded) {
        self.exceeded_at = Some(exceeded.depth);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        if self.exceeded_at.is_some() {
            return;
        }
        self.expr_visits += 1;
        walk_expr(self, expr);
    }

    fn visit_type_expr(&mut self, ty: &TypeExpr) {
        if self.exceeded_at.is_some() {
            return;
        }
        self.type_visits += 1;
        walk_type_expr(self, ty);
    }
}

fn lit_expr(id: u32) -> Expr {
    Expr {
        id,
        kind: ExprKind::Literal(Literal::Int(1)),
        span: Span::new(0, 1),
    }
}

/// Left-nested binary chain: depth `n` means `n` Binary nodes above a leaf.
/// Walk depth along the left spine reaches `n` at the deepest leaf entry.
fn deep_binary_chain(depth: usize) -> Expr {
    let mut expr = lit_expr(0);
    for i in 0..depth {
        let id = u32::try_from(i.saturating_add(1)).unwrap_or(u32::MAX);
        let rhs_id = u32::try_from(i.saturating_add(10_000)).unwrap_or(u32::MAX);
        expr = Expr {
            id,
            kind: ExprKind::Binary(BinaryExpr {
                op: BinOp::Add,
                lhs: Box::new(expr),
                rhs: Box::new(lit_expr(rhs_id)),
            }),
            span: Span::new(0, 1),
        };
    }
    expr
}

fn infer_type() -> TypeExpr {
    TypeExpr {
        nullable: false,
        mode: None,
        kind: TypeExprKind::Infer,
        span: Span::new(0, 1),
        form: TypeFormLevel::LONG,
        iuncta_label: None,
    }
}

/// Nested `atomic<...atomic<_>...>` chain of the given depth.
fn deep_atomic_chain(depth: usize) -> TypeExpr {
    let mut ty = infer_type();
    for _ in 0..depth {
        ty = TypeExpr {
            nullable: false,
            mode: None,
            kind: TypeExprKind::Atomic(Box::new(ty)),
            span: Span::new(0, 1),
            form: TypeFormLevel::LONG,
            iuncta_label: None,
        };
    }
    ty
}

#[test]
fn shallow_expr_walk_succeeds() {
    let expr = deep_binary_chain(3);
    let mut probe = DepthProbe::new();
    probe.visit_expr(&expr);
    assert!(probe.exceeded_at.is_none());
    // 3 Binary + 1 leftmost leaf + 3 rhs leaves = 7 expression nodes
    assert_eq!(probe.expr_visits, 7);
}

#[test]
fn expr_at_max_depth_succeeds() {
    // Left spine depths 0..=MAX: root Binary at 0, leaf entry at MAX.
    let expr = deep_binary_chain(MAX_VISIT_DEPTH);
    let mut probe = DepthProbe::new();
    probe.visit_expr(&expr);
    assert!(
        probe.exceeded_at.is_none(),
        "depth {MAX_VISIT_DEPTH} must be allowed, got {:?}",
        probe.exceeded_at
    );
    assert!(probe.expr_visits > MAX_VISIT_DEPTH);
}

#[test]
fn expr_beyond_max_depth_fails_closed() {
    let expr = deep_binary_chain(MAX_VISIT_DEPTH + 1);
    let mut probe = DepthProbe::new();
    probe.visit_expr(&expr);
    assert_eq!(
        probe.exceeded_at,
        Some(MAX_VISIT_DEPTH + 1),
        "walker must fail closed past MAX_VISIT_DEPTH"
    );
}

#[test]
fn type_expr_beyond_max_depth_fails_closed() {
    let ty = deep_atomic_chain(MAX_VISIT_DEPTH + 1);
    let mut probe = DepthProbe::new();
    probe.visit_type_expr(&ty);
    assert_eq!(probe.exceeded_at, Some(MAX_VISIT_DEPTH + 1));
}

#[test]
fn type_expr_at_max_depth_succeeds() {
    let ty = deep_atomic_chain(MAX_VISIT_DEPTH);
    let mut probe = DepthProbe::new();
    probe.visit_type_expr(&ty);
    assert!(probe.exceeded_at.is_none());
    // MAX Atomic wrappers + 1 Infer leaf
    assert_eq!(probe.type_visits, MAX_VISIT_DEPTH + 1);
}

/// Default hook: a walk past [`MAX_VISIT_DEPTH`] stops descending and returns
/// cleanly, so the depth limit can never abort the process.
#[test]
fn default_on_depth_exceeded_stops_descending() {
    struct DefaultVisitor {
        exprs: usize,
        visited_spine_foot: bool,
    }

    impl Visitor for DefaultVisitor {
        fn visit_expr(&mut self, expr: &Expr) {
            self.exprs += 1;
            // The spine foot is the one node below the limit: `deep_binary_chain`
            // gives it id 0 and buries it `depth` levels down.
            self.visited_spine_foot |= expr.id == 0;
            walk_expr(self, expr);
        }
    }

    let expr = deep_binary_chain(MAX_VISIT_DEPTH + 8);
    let mut visitor = DefaultVisitor {
        exprs: 0,
        visited_spine_foot: false,
    };
    visitor.visit_expr(&expr);

    // Walked the spine down to the limit, then stopped instead of panicking.
    assert!(visitor.exprs > MAX_VISIT_DEPTH, "walk must reach the limit");
    assert!(
        !visitor.visited_spine_foot,
        "the walk must stop at the limit, not descend past it"
    );
    // Each admitted level contributes the spine node plus its rhs leaf.
    assert!(
        visitor.exprs <= 2 * (MAX_VISIT_DEPTH + 2),
        "walk must not expand past the limit, got {} visits",
        visitor.exprs
    );
}

/// The default hook reports the limit it stopped at.
#[test]
fn default_on_depth_exceeded_reports_the_limit() {
    struct DepthReport {
        exceeded: Option<(usize, usize)>,
    }

    impl Visitor for DepthReport {
        fn on_depth_exceeded(&mut self, exceeded: DepthExceeded) {
            self.exceeded = Some((exceeded.depth, exceeded.limit));
        }

        fn visit_expr(&mut self, expr: &Expr) {
            walk_expr(self, expr);
        }
    }

    let expr = deep_binary_chain(MAX_VISIT_DEPTH + 1);
    let mut visitor = DepthReport { exceeded: None };
    visitor.visit_expr(&expr);

    assert_eq!(
        visitor.exceeded,
        Some((MAX_VISIT_DEPTH + 1, MAX_VISIT_DEPTH)),
        "the hook must report the depth and the limit that stopped the walk"
    );
}

fn test_ident(sym: u32) -> Ident {
    Ident {
        name: Symbol(sym),
        span: Span::new(0, 1),
    }
}

fn test_block() -> BlockStmt {
    BlockStmt {
        statements: vec![],
        end_trivia: Vec::new(),
        span: Span::new(0, 1),
    }
}

struct CountVisitor {
    idents: usize,
    types: usize,
    blocks: usize,
}

impl CountVisitor {
    fn new() -> Self {
        Self {
            idents: 0,
            types: 0,
            blocks: 0,
        }
    }
}

impl Visitor for CountVisitor {
    fn visit_ident(&mut self, _ident: &Ident) {
        self.idents += 1;
    }
    fn visit_type_expr(&mut self, ty: &TypeExpr) {
        self.types += 1;
        walk_type_expr(self, ty);
    }
    fn visit_block(&mut self, block: &BlockStmt) {
        self.blocks += 1;
        for stmt in &block.statements {
            walk_stmt(self, stmt);
        }
    }
}

#[test]
fn walk_func_decl_err() {
    let func = FuncDecl {
        name: test_ident(1),
        type_params: vec![],
        params: vec![],
        modifiers: vec![],
        posture: crate::CallablePosture::SyncFinite,
        ret: None,
        err: Some(infer_type()),
        body: None,
        annotations: vec![],
        leading_trivia: vec![],
    };
    let stmt = Stmt {
        id: 0,
        kind: StmtKind::Func(func),
        span: Span::new(0, 1),
        annotations: vec![],
        leading_trivia: vec![],
    };

    let mut visitor = CountVisitor::new();
    walk_stmt(&mut visitor, &stmt);

    assert_eq!(visitor.idents, 1, "must visit func name");
    assert_eq!(visitor.types, 1, "must visit decl.err");
}

#[test]
fn walk_probandum_setup() {
    let probandum = ProbandumDecl {
        name: Symbol(1),
        modifiers: vec![],
        body: ProbandumBody {
            setup: vec![PraeparaBlock {
                kind: PraeparaKind::Praepara,
                all: false,
                body: test_block(),
                span: Span::new(0, 1),
            }],
            tests: vec![],
            nested: vec![],
        },
        span: Span::new(0, 1),
    };
    let stmt = Stmt {
        id: 0,
        kind: StmtKind::Probandum(probandum),
        span: Span::new(0, 1),
        annotations: vec![],
        leading_trivia: vec![],
    };

    let mut visitor = CountVisitor::new();
    walk_stmt(&mut visitor, &stmt);

    assert_eq!(visitor.blocks, 1, "must visit setup block");
}

#[test]
fn walk_si_walks_catch_binding_and_body() {
    let si = SiStmt {
        cond: Box::new(lit_expr(0)),
        then: IfBody::Block(test_block()),
        catch: Some(CapeClause {
            binding: test_ident(1),
            body: test_block(),
            span: Span::new(0, 1),
        }),
        else_: None,
    };
    let stmt = Stmt {
        id: 0,
        kind: StmtKind::Si(si),
        span: Span::new(0, 1),
        annotations: vec![],
        leading_trivia: vec![],
    };

    let mut visitor = CountVisitor::new();
    walk_stmt(&mut visitor, &stmt);

    assert_eq!(visitor.idents, 1, "must visit catch binding");
    assert_eq!(visitor.blocks, 2, "must visit then and catch bodies");
}

#[test]
fn walk_discern_patterns() {
    // Ident pattern with alias bind + Path pattern with destructure bindings.
    let ident_pattern = Pattern::Ident(test_ident(1), Some(PatternBind::Alias(test_ident(2))));
    let path_pattern = Pattern::Path(PathPattern {
        segments: vec![test_ident(3), test_ident(4)],
        bind: Some(PatternBind::Bindings {
            mutability: AstMutability::Immutable,
            bindings: vec![
                ExField {
                    name: test_ident(5),
                    alias: Some(test_ident(6)),
                },
                ExField {
                    name: test_ident(7),
                    alias: None,
                },
            ],
        }),
        span: Span::new(0, 1),
    });

    let discerne = DiscerneStmt {
        exhaustive: false,
        subjects: vec![],
        arms: vec![CasuArm {
            patterns: vec![ident_pattern, path_pattern],
            body: IfBody::Block(test_block()),
            span: Span::new(0, 1),
        }],
        default: None,
    };
    let stmt = Stmt {
        id: 0,
        kind: StmtKind::Discerne(discerne),
        span: Span::new(0, 1),
        annotations: vec![],
        leading_trivia: vec![],
    };

    let mut visitor = CountVisitor::new();
    walk_stmt(&mut visitor, &stmt);

    // Expected ident visits:
    //   - Ident pattern: name=1 + alias=2           = 2
    //   - Path segments: 3 + 4                      = 2
    //   - Bindings: name=5 + alias=6 + name=7       = 3
    // Total: 7
    assert_eq!(visitor.idents, 7, "must visit all pattern idents");
}

/// Visitor that records every identifier the walker hands it, in visit order.
struct IdentLog {
    idents: Vec<u32>,
}

impl IdentLog {
    fn new() -> Self {
        Self { idents: Vec::new() }
    }
}

impl Visitor for IdentLog {
    fn visit_ident(&mut self, ident: &Ident) {
        self.idents.push(ident.name.0);
    }
}

fn named_type(sym: u32) -> TypeExpr {
    TypeExpr {
        nullable: false,
        mode: None,
        kind: TypeExprKind::Named(test_ident(sym), vec![]),
        span: Span::new(0, 1),
        form: TypeFormLevel::LONG,
        iuncta_label: None,
    }
}

fn ident_expr(sym: u32) -> Expr {
    Expr {
        id: 0,
        kind: ExprKind::Ident(test_ident(sym)),
        span: Span::new(0, 1),
    }
}

fn func_stmt(decl: FuncDecl) -> Stmt {
    Stmt {
        id: 0,
        kind: StmtKind::Func(decl),
        span: Span::new(0, 1),
        annotations: vec![],
        leading_trivia: vec![],
    }
}

fn plain_func(name: u32) -> FuncDecl {
    FuncDecl {
        name: test_ident(name),
        type_params: vec![],
        params: vec![],
        modifiers: vec![],
        posture: crate::CallablePosture::SyncFinite,
        ret: None,
        err: None,
        body: None,
        annotations: vec![],
        leading_trivia: vec![],
    }
}

fn plain_param(ty: u32, name: u32) -> Param {
    Param {
        sponte: false,
        mode: AstParamMode::Owned,
        is_rest: false,
        ty: named_type(ty),
        name: test_ident(name),
        alias: None,
        default: None,
        span: Span::new(0, 1),
    }
}

/// A-203: `apud` coordinates and the `filum` lane binder are identifiers the
/// iteration head declares; they are visited between the sources and the
/// element binder, in source order.
#[test]
fn walk_itera_walks_apud_and_filum_idents() {
    let itera = IteraStmt {
        mode: IteraMode::Ex,
        sources: vec![lit_expr(0)],
        apud: vec![test_ident(10), test_ident(11)],
        filum: Some(test_ident(12)),
        mutability: AstMutability::Immutable,
        bindings: vec![BindingPattern::Ident(test_ident(13))],
        body: IfBody::Block(test_block()),
        catch: None,
    };
    let stmt = Stmt {
        id: 0,
        kind: StmtKind::Itera(itera),
        span: Span::new(0, 1),
        annotations: vec![],
        leading_trivia: vec![],
    };

    let mut visitor = IdentLog::new();
    walk_stmt(&mut visitor, &stmt);

    assert_eq!(
        visitor.idents,
        vec![10, 11, 12, 13],
        "must visit apud coordinates, the filum binder, then the element binder"
    );
}

/// A-204: the `cape` clause of a closure's `fac` body is walked exactly like the
/// statement-level `fac`: binding identifier, then body block.
#[test]
fn walk_clausura_fac_walks_catch_binding_and_body() {
    let closure = Expr {
        id: 0,
        kind: ExprKind::Clausura(Box::new(ClausuraExpr {
            params: vec![],
            ret: None,
            err: None,
            body: ClausuraBody::Fac(FacStmt {
                modifier: crate::FacModifier::None,
                body: test_block(),
                catch: Some(CapeClause {
                    binding: test_ident(1),
                    body: test_block(),
                    span: Span::new(0, 1),
                }),
                while_: None,
            }),
        })),
        span: Span::new(0, 1),
    };

    let mut visitor = CountVisitor::new();
    walk_expr(&mut visitor, &closure);

    assert_eq!(
        visitor.idents, 1,
        "must visit the closure fac catch binding"
    );
    assert_eq!(visitor.blocks, 2, "must visit the fac body and catch body");
}

/// A-205: `(T) → R ⇥ E` carries its error type on `FuncTypeExpr::err`.
#[test]
fn walk_type_expr_func_walks_err() {
    let ty = TypeExpr {
        nullable: false,
        mode: None,
        kind: TypeExprKind::Func(FuncTypeExpr {
            params: vec![named_type(1)],
            ret: Box::new(named_type(2)),
            err: Some(Box::new(named_type(3))),
        }),
        span: Span::new(0, 1),
        form: TypeFormLevel::LONG,
        iuncta_label: None,
    };

    let mut visitor = IdentLog::new();
    walk_type_expr(&mut visitor, &ty);

    assert_eq!(
        visitor.idents,
        vec![1, 2, 3],
        "must visit the parameter, return, and error types"
    );
}

/// A-206: a function's generic parameters are walked: name, default type, and
/// each bound's contract name and type arguments, before the value parameters.
#[test]
fn walk_func_decl_walks_type_params() {
    let mut decl = plain_func(19);
    decl.type_params = vec![TypeParam {
        kind: GenericParamKind::Typus,
        name: test_ident(20),
        default: Some(GenericParamDefault::Typus(Box::new(named_type(21)))),
        bounds: vec![ContractRefExpr {
            name: test_ident(22),
            args: vec![named_type(23)],
            span: Span::new(0, 1),
        }],
        span: Span::new(0, 1),
    }];
    decl.params = vec![plain_param(24, 25)];
    let stmt = func_stmt(decl);

    let mut visitor = IdentLog::new();
    walk_stmt(&mut visitor, &stmt);

    assert_eq!(
        visitor.idents,
        vec![19, 20, 21, 22, 23, 24, 25],
        "must visit the type parameter name, default, and bounds before the params"
    );
}

/// A-206: a parameter's `ut` alias and `←` default are children of the
/// parameter; the default is an expression, so it goes through `visit_expr`.
#[test]
fn walk_func_decl_walks_param_alias_and_default() {
    let mut decl = plain_func(30);
    let mut param = plain_param(31, 32);
    param.alias = Some(test_ident(33));
    param.default = Some(Box::new(ident_expr(34)));
    decl.params = vec![param];
    let stmt = func_stmt(decl);

    let mut visitor = IdentLog::new();
    walk_stmt(&mut visitor, &stmt);

    assert_eq!(
        visitor.idents,
        vec![30, 31, 32, 33, 34],
        "must visit the param type, name, alias, then default expression"
    );
}
