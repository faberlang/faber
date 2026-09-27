//! Builtin compiler-owned `forma` genus (`template: textus`, `params:
//! lista<valor>`).
//!
//! Registers `forma` in the resolver and injects the corresponding HIR item
//! when the program uses forma literals. Validation and rendering of forma
//! payloads belong here, not in the lexer.

use crate::{Primitive, Resolver, SymbolKind, Type, TypeId, TypeTable};
use radix_hir::{DefId, HirField, HirId, HirItem, HirItemKind, HirStruct};
use radix_lexer::{Interner, Span, Symbol};

const FORMA_NAME: &str = "forma";
const TEMPLATE_FIELD: &str = "template";
const PARAMS_FIELD: &str = "params";

/// Register the builtin `forma` genus in the resolver.
///
/// `forma` is reserved like `numerus` or `textus`: user source cannot shadow it.
/// The def id is allocated from the builtin low range via `fresh_builtin_def_id`,
/// so user declaration ids stay stable regardless of builtin count.
pub fn register(resolver: &mut Resolver, types: &mut TypeTable, interner: &mut Interner) {
    let name = interner.intern(FORMA_NAME);
    if resolver.lookup_type(name).is_some() {
        return;
    }

    let def_id = resolver.fresh_builtin_def_id();
    let symbol = crate::Symbol {
        def_id,
        name,
        kind: SymbolKind::Struct,
        ty: Some(types.intern(Type::Struct(def_id))),
        mutable: false,
        span: Span::default(),
    };
    if resolver.define(symbol).is_err() {
        return;
    }
    resolver.register_builtin_forma(
        def_id,
        interner.intern(TEMPLATE_FIELD),
        interner.intern(PARAMS_FIELD),
    );
}

/// Lookup the builtin `forma` definition id.
#[must_use]
pub fn def_id(resolver: &Resolver) -> Option<DefId> {
    resolver.builtin_forma_def_id()
}

pub(crate) fn template_field(resolver: &Resolver) -> Option<Symbol> {
    resolver.builtin_forma_template_field()
}

pub(crate) fn params_field(resolver: &Resolver) -> Option<Symbol> {
    resolver.builtin_forma_params_field()
}

pub(crate) fn type_id(types: &mut TypeTable, def_id: DefId) -> TypeId {
    types.intern(Type::Struct(def_id))
}

/// Return whether parsed source references captured `forma` templates.
///
/// Answers fail-closed: if the walker refuses to descend past its depth limit
/// the scan is incomplete, so the builtin item is injected rather than reporting
/// "not used" for a subtree that was never read. The refused depth is recorded
/// on the visitor for callers that can report it.
#[must_use]
pub fn program_uses_forma(program: &radix_syntax::Program, resolver: &Resolver) -> bool {
    use radix_syntax::{
        DepthExceeded, ExprKind, IfBody, Literal, Program, StmtKind, Visitor, walk_expr, walk_stmt,
    };

    let Some(def_id) = def_id(resolver) else {
        return false;
    };
    let Some(symbol) = resolver.get_symbol(def_id) else {
        return false;
    };

    struct FormaUsageVisitor {
        forma_name: Symbol,
        found: bool,
        /// Depth the walker refused to enter, if it stopped short.
        depth_exceeded: Option<DepthExceeded>,
    }

    impl FormaUsageVisitor {
        /// Walk the program, stopping at the first `forma` reference.
        fn scan(&mut self, program: &Program) {
            for stmt in &program.statements {
                self.visit_stmt(stmt);
                if self.found {
                    break;
                }
            }
        }
    }

    impl Visitor for FormaUsageVisitor {
        fn on_depth_exceeded(&mut self, exceeded: DepthExceeded) {
            self.depth_exceeded = Some(exceeded);
        }

        fn visit_stmt(&mut self, stmt: &radix_syntax::Stmt) {
            if self.found {
                return;
            }
            if let StmtKind::Incipit(entry) = &stmt.kind {
                match &entry.body {
                    IfBody::Block(block) => self.visit_block(block),
                    IfBody::Ergo(inner) => self.visit_stmt(inner),
                }
                return;
            }
            walk_stmt(self, stmt);
        }

        fn visit_expr(&mut self, expr: &radix_syntax::Expr) {
            if self.found {
                return;
            }
            if matches!(&expr.kind, ExprKind::Literal(Literal::Forma(_))) {
                self.found = true;
                return;
            }
            walk_expr(self, expr);
        }

        fn visit_ident(&mut self, ident: &radix_syntax::Ident) {
            if ident.name == self.forma_name {
                self.found = true;
            }
        }
    }

    let mut visitor = FormaUsageVisitor {
        forma_name: symbol.name,
        found: false,
        depth_exceeded: None,
    };
    visitor.scan(program);
    visitor.found || visitor.depth_exceeded.is_some()
}

/// Append the builtin `forma` struct item to lowered HIR when needed for codegen.
pub fn inject_hir_item(
    items: &mut Vec<HirItem>,
    resolver: &mut Resolver,
    types: &mut TypeTable,
    interner: &mut Interner,
) {
    let Some(def_id) = def_id(resolver) else {
        return;
    };

    if items.iter().any(|item| item.def_id == def_id) {
        return;
    }

    let Some(template_name) = template_field(resolver) else {
        return;
    };
    let Some(params_name) = params_field(resolver) else {
        return;
    };

    let textus = types.primitive(Primitive::Textus);
    let valor = types.primitive(Primitive::Valor);
    let params_ty = types.array(valor);

    let strukt = HirStruct {
        name: interner.intern(FORMA_NAME),
        type_params: Vec::new(),
        fields: vec![
            HirField {
                def_id: resolver.fresh_def_id(),
                name: template_name,
                ty: textus,
                sponte: false,
                modifier: None,
                annotations: Vec::new(),
                init: None,
                span: Span::default(),
            },
            HirField {
                def_id: resolver.fresh_def_id(),
                name: params_name,
                ty: params_ty,
                sponte: false,
                modifier: None,
                annotations: Vec::new(),
                init: None,
                span: Span::default(),
            },
        ],
        methods: Vec::new(),
        implements: Vec::new(),
    };

    items.push(HirItem {
        annotations: Vec::new(),
        id: HirId(0),
        def_id,
        kind: HirItemKind::Struct(strukt),
        span: Span::default(),
    });
}

// In-file pin for the scan above.
#[cfg(test)]
mod forma_usage_budget_test {
    use super::program_uses_forma;
    use crate::{Resolver, TypeTable};
    use radix_lexer::{Interner, Span};
    use radix_syntax::{
        BinOp, BinaryExpr, Expr, ExprKind, ExprStmt, Literal, MAX_VISIT_DEPTH, Program, Stmt,
        StmtKind,
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

    fn registered_resolver() -> Resolver {
        let mut types = TypeTable::new();
        let mut resolver = Resolver::new();
        let mut interner = Interner::new();
        super::register(&mut resolver, &mut types, &mut interner);
        resolver
    }

    #[test]
    fn shallow_program_without_forma_use_injects_nothing() {
        let resolver = registered_resolver();
        let program = program_with(Expr {
            id: 0,
            kind: ExprKind::Literal(Literal::Int(1)),
            span: Span::default(),
        });
        assert!(!program_uses_forma(&program, &resolver));
    }

    #[test]
    fn scan_fails_closed_when_the_walker_stops() {
        let resolver = registered_resolver();
        let program = program_with(deep_spine(MAX_VISIT_DEPTH + 8));
        assert!(
            program_uses_forma(&program, &resolver),
            "an incomplete scan must inject the builtin rather than report no forma use"
        );
    }
}
