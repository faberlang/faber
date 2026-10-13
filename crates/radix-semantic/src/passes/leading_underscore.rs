//! Warn on declared identifiers that start with `_` (DECISIONS D5.6).
//!
//! A leading underscore is not Faber style: unmarked top-level declarations
//! are already module-private (D5.1), so `_name` carries no meaning the
//! language does not already give. The finding is a warning (WARN030,
//! `leading_underscore`); strict builds reject it through the ordinary
//! `deny_warnings` policy, never through a separate switch.
//!
//! INVARIANTS
//! ==========
//! - Only the identifier as declared is checked, never each use: function and
//!   method names, type names, fields, locals, parameters, constants, and
//!   `ordo` members.
//! - A bare `_` is the hole/wildcard and never warns.
//! - AST-only and source-shaped, so desugared HIR names cannot trip it.
//! - The walk is warn-only. A depth-limited subtree simply goes unlinted; it
//!   cannot change what compiles.

use crate::{SemanticError, SemanticErrorKind, WarningKind};
use radix_lexer::Interner;

use radix_syntax::{
    BindingPattern, ClassMemberKind, ExField, Expr, ExprKind, FieldDecl, FuncDecl, Ident, Param,
    Pattern, PatternBind, Program, Stmt, StmtKind, Visitor, walk_expr, walk_stmt,
};

const ISSUE_LEADING_UNDERSCORE: &str = "leading_underscore";

/// Declaration kind named in the `kind` argument of each finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeclarationKind {
    Function,
    Type,
    Field,
    Local,
    Parameter,
    Static,
}

impl DeclarationKind {
    /// Stable argument value; renderers and tests read this, not prose.
    #[must_use]
    pub const fn spelling(self) -> &'static str {
        match self {
            Self::Function => "function",
            Self::Type => "type",
            Self::Field => "field",
            Self::Local => "local",
            Self::Parameter => "parameter",
            Self::Static => "constant",
        }
    }
}

/// Report every declared identifier in `program` that begins with `_`.
#[must_use]
pub fn check(program: &Program, interner: &Interner) -> Vec<SemanticError> {
    let mut checker = Checker {
        interner,
        warnings: Vec::new(),
        top_level_stmt: None,
    };
    for stmt in &program.statements {
        checker.top_level_stmt = Some(stmt.id);
        checker.visit_stmt(stmt);
    }
    checker.warnings
}

/// Whether `name` is a `_`-prefixed identifier other than the bare hole.
fn has_leading_underscore(name: &str) -> bool {
    name.len() > 1 && name.starts_with('_')
}

struct Checker<'a> {
    interner: &'a Interner,
    warnings: Vec<SemanticError>,
    /// The top-level statement being walked; a `=` constant is a module
    /// constant only when it is that statement, never a nested local.
    top_level_stmt: Option<radix_syntax::NodeId>,
}

impl Checker<'_> {
    fn ident(&mut self, ident: &Ident, kind: DeclarationKind) {
        let name = self.interner.resolve(ident.name);
        if has_leading_underscore(name) {
            self.warnings.push(
                SemanticError::structured(
                    SemanticErrorKind::Warning(WarningKind::LeadingUnderscore),
                    ident.span,
                )
                .with_arg("issue", ISSUE_LEADING_UNDERSCORE)
                .with_arg("name", name)
                .with_arg("kind", kind.spelling()),
            );
        }
    }

    fn function(&mut self, decl: &FuncDecl) {
        self.ident(&decl.name, DeclarationKind::Function);
        self.params(&decl.params);
    }

    fn params(&mut self, params: &[Param]) {
        for param in params {
            self.ident(&param.name, DeclarationKind::Parameter);
        }
    }

    fn field(&mut self, field: &FieldDecl) {
        self.ident(&field.name, DeclarationKind::Field);
    }

    /// An `ex` / pattern field binds its alias when written, else its name.
    fn ex_field(&mut self, field: &ExField) {
        self.ident(
            field.alias.as_ref().unwrap_or(&field.name),
            DeclarationKind::Local,
        );
    }

    fn binding(&mut self, pattern: &BindingPattern, kind: DeclarationKind) {
        match pattern {
            BindingPattern::Ident(ident) => self.ident(ident, kind),
            BindingPattern::Wildcard(_) => {}
            BindingPattern::Array {
                elements, is_rest, ..
            } => {
                for element in elements {
                    self.binding(element, kind);
                }
                if let Some(rest) = is_rest {
                    self.ident(rest, kind);
                }
            }
            BindingPattern::Object {
                fields, is_rest, ..
            } => {
                for field in fields {
                    self.ident(field.alias.as_ref().unwrap_or(&field.name), kind);
                }
                if let Some(rest) = is_rest {
                    self.ident(rest, kind);
                }
            }
        }
    }

    fn pattern_bind(&mut self, bind: &PatternBind) {
        match bind {
            PatternBind::Alias(ident) => self.ident(ident, DeclarationKind::Local),
            PatternBind::Bindings { bindings, .. } => {
                for field in bindings {
                    self.ex_field(field);
                }
            }
        }
    }

    fn pattern(&mut self, pattern: &Pattern) {
        let bind = match pattern {
            Pattern::Ident(_, bind) | Pattern::Type(_, bind) => bind.as_ref(),
            Pattern::Path(path) => path.bind.as_ref(),
            Pattern::Or(alternatives) => {
                for alternative in alternatives {
                    self.pattern(alternative);
                }
                None
            }
            Pattern::Wildcard(_) | Pattern::Literal(..) | Pattern::NegatedLiteral(..) => None,
        };
        if let Some(bind) = bind {
            self.pattern_bind(bind);
        }
    }

    fn catch_binding(&mut self, catch: Option<&radix_syntax::CapeClause>) {
        if let Some(catch) = catch {
            self.ident(&catch.binding, DeclarationKind::Local);
        }
    }

    /// Declarations introduced directly by `stmt`; nested bodies are reached
    /// by the walker.
    fn declarations(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Var(decl) => {
                let module_constant = decl.compile_time && self.top_level_stmt == Some(stmt.id);
                let kind = if module_constant {
                    DeclarationKind::Static
                } else {
                    DeclarationKind::Local
                };
                self.binding(&decl.binding, kind);
            }
            StmtKind::Func(decl) => self.function(decl),
            StmtKind::Class(decl) => {
                self.ident(&decl.name, DeclarationKind::Type);
                for member in &decl.members {
                    match &member.kind {
                        ClassMemberKind::Field(field) => self.field(field),
                        ClassMemberKind::Method(method) => self.function(method),
                    }
                }
            }
            StmtKind::Interface(decl) => {
                self.ident(&decl.name, DeclarationKind::Type);
                for method in &decl.methods {
                    self.ident(&method.name, DeclarationKind::Function);
                    self.params(&method.params);
                }
            }
            StmtKind::TypeAlias(decl) => self.ident(&decl.name, DeclarationKind::Type),
            StmtKind::Schema(decl) => self.ident(&decl.name, DeclarationKind::Type),
            StmtKind::Enum(decl) => {
                self.ident(&decl.name, DeclarationKind::Type);
                for member in &decl.members {
                    self.ident(&member.name, DeclarationKind::Static);
                }
            }
            StmtKind::Union(decl) => {
                self.ident(&decl.name, DeclarationKind::Type);
                for shared in &decl.shared_fields {
                    self.field(&shared.field);
                }
                for variant in &decl.variants {
                    self.ident(&variant.name, DeclarationKind::Type);
                    for field in &variant.fields {
                        self.ident(&field.name, DeclarationKind::Field);
                    }
                }
            }
            StmtKind::Ex(stmt) => {
                for field in &stmt.fields {
                    self.ex_field(field);
                }
                if let Some(rest) = &stmt.is_rest {
                    self.ident(rest, DeclarationKind::Local);
                }
            }
            StmtKind::Itera(stmt) => {
                for binding in &stmt.bindings {
                    self.binding(binding, DeclarationKind::Local);
                }
                self.catch_binding(stmt.catch.as_ref());
            }
            StmtKind::Discerne(stmt) => {
                for arm in &stmt.arms {
                    for pattern in &arm.patterns {
                        self.pattern(pattern);
                    }
                }
            }
            StmtKind::Si(stmt) => self.catch_binding(stmt.catch.as_ref()),
            StmtKind::Dum(stmt) => self.catch_binding(stmt.catch.as_ref()),
            StmtKind::Elige(stmt) => self.catch_binding(stmt.catch.as_ref()),
            StmtKind::Fac(stmt) => self.catch_binding(stmt.catch.as_ref()),
            _ => {}
        }
    }
}

impl Visitor for Checker<'_> {
    fn visit_stmt(&mut self, stmt: &Stmt) {
        self.declarations(stmt);
        walk_stmt(self, stmt);
    }

    fn visit_expr(&mut self, expr: &Expr) {
        if let ExprKind::Clausura(closure) = &expr.kind {
            for param in &closure.params {
                self.ident(&param.name, DeclarationKind::Parameter);
            }
            if let radix_syntax::ClausuraBody::Fac(fac) = &closure.body {
                self.catch_binding(fac.catch.as_ref());
            }
        }
        walk_expr(self, expr);
    }
}

#[cfg(test)]
#[path = "leading_underscore_test.rs"]
mod tests;
