//! Statement lowering for executable HIR flow.
//!
//! This file turns source-level executable statements into the smaller set of
//! HIR statement and expression forms used by semantic analysis and codegen.
//! It is deliberately conservative: statement lowering preserves source spans,
//! creates synthetic locals for bindings introduced by control-flow forms, and
//! represents unsupported or misplaced syntax as error nodes instead of
//! deciding type legality early.
//!
//! CONTROL-FLOW MODEL
//! ==================
//! Most structured control flow is expression-shaped in HIR. `si`, `dum`,
//! `itera`, `fac`, `elige`, and `discerne` therefore lower to expression
//! statements whose inner `HirExpressionKind` carries the normalized flow. `redde`,
//! `rumpe`, and `perge` remain statement kinds because later passes reason
//! about them as control-transfer boundaries.
//!
//! EXPANSION POLICY
//! ================
//! A few source statements lower to multiple HIR statements before ordinary
//! statement lowering sees them. Destructuring declarations and `ex` introduce
//! several locals from one source span, so `lower_stmt_expanded` owns that
//! fan-out and the single-statement lowerer treats any missed destructuring as a
//! recoverable lowering error.
//!
//! ERROR STRATEGY
//! ==============
//! This pass checks statement shape assumptions only: required initializers for
//! destructuring, pattern arity against `discerne` subjects, and cases that must
//! be literals or enum variants. Type compatibility, exhaustiveness, loop
//! placement legality, and target-specific lowering remain downstream.

use super::{HirBlock, HirExpression, HirStatement, HirStatementKind, Lowerer, pattern};
use radix_hir::{HirCasuArm, HirExpressionKind, HirLiteral, HirPattern, HirScribeKind};
use radix_lexer::Span;
use radix_syntax::{Stmt, StmtKind};

fn error_expr(lowerer: &mut Lowerer, span: Span) -> HirExpression {
    HirExpression {
        id: lowerer.next_hir_id(),
        kind: HirExpressionKind::Error,
        ty: None,
        span,
    }
}

/// Source span of a binding pattern for diagnostics.
fn binding_pattern_span(pattern: &radix_syntax::BindingPattern) -> Span {
    match pattern {
        radix_syntax::BindingPattern::Ident(ident) => ident.span,
        radix_syntax::BindingPattern::Wildcard(span)
        | radix_syntax::BindingPattern::Array { span, .. }
        | radix_syntax::BindingPattern::Object { span, .. } => *span,
    }
}

/// Per-position `iuncta` labels of a list annotation's element type
/// (`lista<iuncta<gx: A, B>>`), or `None` for any other annotation shape.
fn lista_element_labels(
    ty: &radix_syntax::TypeExpr,
    interner: &radix_lexer::Interner,
) -> Option<Vec<Option<radix_lexer::Symbol>>> {
    let radix_syntax::TypeExprKind::Named(name, params) = &ty.kind else {
        return None;
    };
    if interner.resolve(name.name) != "lista" || params.len() != 1 {
        return None;
    }
    let radix_syntax::GenericArgExpr::Type(element) = &params[0] else {
        return None;
    };
    crate::tuple_labels::from_type_expr(element, interner)
}

fn binding_pattern_has_name(
    pattern: &radix_syntax::BindingPattern,
    name: radix_lexer::Symbol,
) -> bool {
    match pattern {
        radix_syntax::BindingPattern::Ident(ident) => ident.name == name,
        radix_syntax::BindingPattern::Wildcard(_) => false,
        radix_syntax::BindingPattern::Array {
            elements, is_rest, ..
        } => {
            elements
                .iter()
                .any(|element| binding_pattern_has_name(element, name))
                || is_rest.as_ref().is_some_and(|rest| rest.name == name)
        }
        radix_syntax::BindingPattern::Object {
            fields, is_rest, ..
        } => {
            fields
                .iter()
                .any(|field| field.alias.as_ref().unwrap_or(&field.name).name == name)
                || is_rest.as_ref().is_some_and(|rest| rest.name == name)
        }
    }
}

/// Lower one source statement into one HIR statement.
///
/// This function assumes expansion-only forms have already gone through
/// `lower_stmt_expanded`. If they appear here, lowering records a diagnostic and
/// emits an error expression so the containing block can still be analyzed.
pub fn lower_stmt(lowerer: &mut Lowerer, stmt: &Stmt) -> HirStatement {
    let id = lowerer.next_hir_id();
    let span = stmt.span;
    lowerer.current_span = span;

    let kind = match &stmt.kind {
        StmtKind::Var(decl) => lowerer.lower_var_stmt(decl),
        StmtKind::SizeDecl(decl) => {
            let def_id = lowerer.next_def_id();
            lowerer.bind_local(decl.name.name, def_id);
            // The name is a size term: its figura uses stay admissible (Z3
            // refuses bare *value* names, not declared sizes).
            lowerer.figura_extent_admissible_locals.insert(def_id);
            HirStatementKind::Local(radix_hir::HirLocal {
                def_id,
                name: decl.name.name,
                ty: None,
                init: Some(lowerer.lower_expr(&decl.init)),
                mutable: false,
                is_await: false,
                runtime_provided: false,
                await_store: None,
                size_decl: true,
                size_static: decl.is_static,
            })
        }
        StmtKind::Expr(expr) => lowerer.lower_expr_stmt(expr),
        StmtKind::IncDec(inc_dec) => lowerer.lower_inc_dec_stmt(inc_dec),
        StmtKind::Si(if_stmt) => lowerer.lower_si(if_stmt),
        StmtKind::Dum(while_stmt) => lowerer.lower_dum(while_stmt),
        StmtKind::Itera(iter_stmt) => lowerer.lower_itera(iter_stmt),
        StmtKind::Custodi(custodi_stmt) => lowerer.lower_custodi(custodi_stmt),
        StmtKind::Fac(fac_stmt) => lowerer.lower_fac(fac_stmt),
        StmtKind::Redde(ret) => lowerer.lower_redde(ret),
        StmtKind::ReddeFilum(ret) => {
            let value = ret.value.as_deref().map(|value| lowerer.lower_expr(value));
            HirStatementKind::ReddeFilum(value)
        }
        StmtKind::Rumpe(_) => HirStatementKind::Rumpe,
        StmtKind::Perge(_) => HirStatementKind::Perge,
        StmtKind::Iace(iace_stmt) => lowerer.lower_iace(iace_stmt),
        StmtKind::Mori(mori_stmt) => lowerer.lower_mori(mori_stmt),
        StmtKind::Tacet(_) => HirStatementKind::Tacet,
        StmtKind::Reddet(stmt) => {
            let future = lowerer.lower_expr(&stmt.future);
            HirStatementKind::Redde(Some(HirExpression {
                id: lowerer.next_hir_id(),
                kind: HirExpressionKind::Reddet(Box::new(future)),
                ty: None,
                span,
            }))
        }
        StmtKind::Tacebit(stmt) => {
            let future = lowerer.lower_expr(&stmt.future);
            HirStatementKind::Expr(HirExpression {
                id: lowerer.next_hir_id(),
                kind: HirExpressionKind::Tacebit(Box::new(future)),
                ty: None,
                span,
            })
        }
        StmtKind::Adfirma(adfirma_stmt) => lowerer.lower_adfirma(adfirma_stmt),
        StmtKind::Require(require_stmt) => lowerer.lower_require(require_stmt),
        StmtKind::Reject(reject_stmt) => lowerer.lower_reject(reject_stmt),
        StmtKind::Scribe(scribe_stmt) => lowerer.lower_scribe(scribe_stmt),
        StmtKind::Incipit(incipit_stmt) => lowerer.lower_incipit_stmt(incipit_stmt),
        StmtKind::Ex(ex_stmt) => lowerer.lower_ex(ex_stmt),
        StmtKind::Elige(elige_stmt) => lowerer.lower_elige(elige_stmt),
        StmtKind::Discerne(discerne_stmt) => lowerer.lower_discerne(discerne_stmt),
        StmtKind::Block(block) => {
            let block = lowerer.lower_block(block);
            HirStatementKind::Expr(HirExpression {
                id: lowerer.next_hir_id(),
                kind: HirExpressionKind::Block(block),
                ty: None,
                span,
            })
        }
        _ => {
            lowerer.error("unhandled_statement_kind");
            HirStatementKind::Expr(error_expr(lowerer, span))
        }
    };

    HirStatement { id, kind, span }
}

/// Lower a statement that may expand into several HIR statements.
///
/// Destructuring and `ex` are declaration-like fan-out operations: they bind
/// locals and synthesize field/index reads before ordinary statement lowering.
/// All other statements keep a one-to-one source-to-HIR statement relationship.
pub fn lower_stmt_expanded(lowerer: &mut Lowerer, stmt: &Stmt) -> Vec<HirStatement> {
    match &stmt.kind {
        StmtKind::Var(decl)
            if matches!(&decl.binding, radix_syntax::BindingPattern::Array { .. }) =>
        {
            lowerer.lower_array_destructure_stmt(decl)
        }
        StmtKind::Var(decl)
            if matches!(&decl.binding, radix_syntax::BindingPattern::Object { .. }) =>
        {
            lowerer.lower_object_destructure_stmt(decl)
        }
        StmtKind::Ex(stmt) => lowerer.lower_ex_stmt(stmt),
        _ => vec![lower_stmt(lowerer, stmt)],
    }
}

/// Collect the identifier segments of a right-leaning member chain
/// (`a.b.c` → `[a, b, c]`). Returns `None` when any step is not a plain
/// identifier (index, call, and optional-chain steps are not variant paths).
fn member_segments(member: &radix_syntax::MemberExpr) -> Option<Vec<radix_lexer::Symbol>> {
    let mut segments = Vec::new();
    segments.push(member.member.name);
    let mut object = member.object.as_ref();
    loop {
        match &object.kind {
            radix_syntax::ExprKind::Ident(ident) => {
                segments.push(ident.name);
                break;
            }
            radix_syntax::ExprKind::Member(inner) => {
                segments.push(inner.member.name);
                object = inner.object.as_ref();
            }
            _ => return None,
        }
    }
    segments.reverse();
    Some(segments)
}

impl Lowerer<'_> {
    fn lower_array_destructure_stmt(&mut self, decl: &radix_syntax::VarDecl) -> Vec<HirStatement> {
        let Some(init) = &decl.init else {
            self.error("array_destructuring_initializer");
            return Vec::new();
        };

        self.lower_array_destructure_pattern(&decl.binding, init, decl.mutability, decl.is_await())
    }

    fn lower_array_destructure_pattern(
        &mut self,
        pattern: &radix_syntax::BindingPattern,
        init: &radix_syntax::Expr,
        mutability: radix_syntax::AstMutability,
        is_await: bool,
    ) -> Vec<HirStatement> {
        let (elements, rest, span) = match pattern {
            radix_syntax::BindingPattern::Array {
                elements,
                is_rest: rest,
                span,
            } => (elements, rest, *span),
            _ => return Vec::new(),
        };
        let mutable = mutability == radix_syntax::AstMutability::Mutable;
        // A pattern of holes only binds nothing; its initializer lowers to one
        // expression statement below, which already evaluates it once.
        let binds = rest.is_some()
            || elements
                .iter()
                .any(|element| !matches!(element, radix_syntax::BindingPattern::Wildcard(_)));
        let (mut out, hoisted_subject) = if binds {
            self.destructure_subject(init, is_await, span)
        } else {
            (Vec::new(), None)
        };
        let hoisted = hoisted_subject.is_some();
        let subject = hoisted_subject.as_ref().unwrap_or(init);
        let mut authored = Vec::with_capacity(elements.len());

        for (idx, pattern) in elements.iter().enumerate() {
            match pattern {
                radix_syntax::BindingPattern::Ident(ident) => {
                    authored.push(Some(ident.name));
                    let def_id = self.next_def_id();
                    self.bind_local(ident.name, def_id);
                    let index_expr = HirExpression {
                        id: self.next_hir_id(),
                        kind: HirExpressionKind::Literal(HirLiteral::Int(idx as u64)),
                        ty: None,
                        span: ident.span,
                    };
                    let init_expr = HirExpression {
                        id: self.next_hir_id(),
                        kind: HirExpressionKind::Index(
                            Box::new(self.lower_expr(subject)),
                            Box::new(index_expr),
                        ),
                        ty: None,
                        span: ident.span,
                    };
                    out.push(self.destructured_local(def_id, ident.name, init_expr, mutable));
                }
                radix_syntax::BindingPattern::Wildcard(_) => authored.push(None),
                radix_syntax::BindingPattern::Array { .. } => {
                    self.error("nested_array_destructuring");
                }
                radix_syntax::BindingPattern::Object { .. } => {
                    self.error("nested_object_destructuring");
                }
            }
        }

        // The rest binding is the tail after every authored position (holes
        // included), not the whole subject: `omissa(n)` drops the first `n`.
        if let Some(rest_ident) = rest {
            let def_id = self.next_def_id();
            self.bind_local(rest_ident.name, def_id);
            let count = HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Literal(HirLiteral::Int(elements.len() as u64)),
                ty: None,
                span: rest_ident.span,
            };
            let tail = HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::MethodCall(
                    Box::new(self.lower_expr(subject)),
                    self.intrinsic_surface_symbol("omissa"),
                    Vec::new(),
                    vec![radix_hir::HirCallArg {
                        name: None,
                        spread: false,
                        marker: None,
                        expr: count,
                        span: rest_ident.span,
                    }],
                    None,
                ),
                ty: None,
                span: rest_ident.span,
            };
            out.push(self.destructured_local(def_id, rest_ident.name, tail, mutable));
        }

        if out.is_empty() {
            out.push(HirStatement {
                id: self.next_hir_id(),
                kind: HirStatementKind::Expr(self.lower_expr(init)),
                span,
            });
        }

        self.record_destructure(
            &out,
            hoisted,
            radix_hir::HirDestructurePattern::Array {
                elements: authored,
                rest: rest.as_ref().map(|ident| ident.name),
            },
            mutable,
            is_await,
        );
        out
    }

    /// Evaluate a destructuring initializer once.
    ///
    /// A side-effect-free place (`xs`, `(xs)`, `ego.xs`) is re-read by each
    /// expanded local, which keeps target ownership unchanged. Anything else
    /// (a call, a literal, an awaited value) is bound to a hidden immutable
    /// local first, so `sit [a, b] ← f()` calls `f` once and awaits once.
    /// Returns the hoisted statement (if any) and the subject expression the
    /// expanded locals must read instead of `init`.
    fn destructure_subject(
        &mut self,
        init: &radix_syntax::Expr,
        is_await: bool,
        span: Span,
    ) -> (Vec<HirStatement>, Option<radix_syntax::Expr>) {
        if !is_await && is_pure_place(init) {
            return (Vec::new(), None);
        }
        let def_id = self.next_def_id();
        let name = self.interner.intern(&format!("__destructura_{}", def_id.0));
        let init_expr = self.lower_expr(init);
        self.bind_local(name, def_id);
        let local = HirStatement {
            id: self.next_hir_id(),
            kind: HirStatementKind::Local(radix_hir::HirLocal {
                def_id,
                name,
                ty: None,
                init: Some(init_expr),
                mutable: false,
                is_await,
                runtime_provided: false,
                await_store: None,
                size_decl: false,
                size_static: false,
            }),
            span,
        };
        let subject = radix_syntax::Expr {
            id: u32::MAX,
            kind: radix_syntax::ExprKind::Ident(radix_syntax::Ident { name, span }),
            span,
        };
        (vec![local], Some(subject))
    }

    /// Reader-pack spelling of a canonical intrinsic for a synthesized call.
    ///
    /// Type checking resolves intrinsic method names through the active pack
    /// (non-`la` packs reject the canonical Latin spelling), so synthesized
    /// calls use the same surface an author would write.
    pub(super) fn intrinsic_surface_symbol(&mut self, canonical: &str) -> radix_lexer::Symbol {
        let surface = self
            .locale_pack
            .map_or(canonical, |pack| pack.intrinsic_surface(canonical))
            .to_owned();
        self.interner.intern(&surface)
    }

    /// One expanded destructuring local; the subject was already awaited.
    fn destructured_local(
        &mut self,
        def_id: radix_hir::DefId,
        name: radix_lexer::Symbol,
        init: HirExpression,
        mutable: bool,
    ) -> HirStatement {
        let span = init.span;
        HirStatement {
            id: self.next_hir_id(),
            kind: HirStatementKind::Local(radix_hir::HirLocal {
                def_id,
                name,
                ty: None,
                init: Some(init),
                mutable,
                is_await: false,
                runtime_provided: false,
                await_store: None,
                size_decl: false,
                size_static: false,
            }),
            span,
        }
    }

    /// Record the authored pattern over its expanded statement run so Faber
    /// re-emission restores the destructuring form.
    fn record_destructure(
        &mut self,
        out: &[HirStatement],
        hoisted: bool,
        pattern: radix_hir::HirDestructurePattern,
        mutable: bool,
        is_await: bool,
    ) {
        if out.is_empty() {
            return;
        }
        self.presentation
            .destructure_bindings
            .push(radix_hir::HirDestructureBinding {
                stmt_ids: out.iter().map(|stmt| stmt.id).collect(),
                hoisted,
                pattern,
                mutable,
                is_await,
            });
    }

    fn lower_object_destructure_stmt(&mut self, decl: &radix_syntax::VarDecl) -> Vec<HirStatement> {
        let Some(init) = &decl.init else {
            self.error("object_destructuring_initializer");
            return Vec::new();
        };

        self.lower_object_destructure_pattern(&decl.binding, init, decl.mutability, decl.is_await())
    }

    fn lower_object_destructure_pattern(
        &mut self,
        pattern: &radix_syntax::BindingPattern,
        init: &radix_syntax::Expr,
        mutability: radix_syntax::AstMutability,
        is_await: bool,
    ) -> Vec<HirStatement> {
        let radix_syntax::BindingPattern::Object {
            fields,
            is_rest: rest,
            span,
        } = pattern
        else {
            return Vec::new();
        };

        // `objectPattern` is also the named face of an `iuncta` tuple. The
        // resolver's side table is keyed by the subject binding, so ordinary
        // genus/record destructuring keeps the existing Field lowering path.
        let tuple_labels = self.tuple_labels_for_subject(init);
        let mut bound_positions = tuple_labels
            .as_ref()
            .map(|labels| vec![false; labels.len()]);
        let mutable = mutability == radix_syntax::AstMutability::Mutable;
        let (mut out, hoisted_subject) = self.destructure_subject(init, is_await, *span);
        let hoisted = hoisted_subject.is_some();
        let subject = hoisted_subject.as_ref().unwrap_or(init);

        for field in fields {
            let name = field
                .alias
                .as_ref()
                .map_or(field.name.name, |ident| ident.name);
            let def_id = self.next_def_id();
            self.bind_local(name, def_id);
            let init_expr = if let Some(labels) = tuple_labels.as_ref() {
                if let Some(position) = crate::tuple_labels::position_of(labels, field.name.name) {
                    if let Some(bound) = bound_positions.as_mut() {
                        bound[position] = true;
                    }
                    self.lower_tuple_destructure_index(subject, position, field.name.span)
                } else {
                    self.current_span = field.name.span;
                    self.error("unknown_tuple_element_label");
                    error_expr(self, field.name.span)
                }
            } else {
                HirExpression {
                    id: self.next_hir_id(),
                    kind: HirExpressionKind::Field(
                        Box::new(self.lower_expr(subject)),
                        field.name.name,
                    ),
                    ty: None,
                    span: field.name.span,
                }
            };
            out.push(self.destructured_local(def_id, name, init_expr, mutable));
        }

        if let Some(rest_ident) = rest {
            let init_expr = if let Some(labels) = tuple_labels.as_ref() {
                let elements = labels
                    .iter()
                    .enumerate()
                    .filter(|(position, _)| {
                        bound_positions
                            .as_ref()
                            .is_none_or(|bound| !bound[*position])
                    })
                    .map(|(position, _)| {
                        self.lower_tuple_destructure_index(subject, position, rest_ident.span)
                    })
                    .collect();
                HirExpression {
                    id: self.next_hir_id(),
                    kind: HirExpressionKind::Tuple(elements, None),
                    ty: None,
                    span: rest_ident.span,
                }
            } else {
                self.lower_record_rest_initializer(init, subject, fields, rest_ident.span)
                    .unwrap_or_else(|| self.lower_expr(subject))
            };
            let def_id = self.next_def_id();
            self.bind_local(rest_ident.name, def_id);
            out.push(self.destructured_local(def_id, rest_ident.name, init_expr, mutable));
        }

        self.record_destructure(
            &out,
            hoisted,
            radix_hir::HirDestructurePattern::Object {
                fields: fields
                    .iter()
                    .map(|field| {
                        (
                            field.name.name,
                            field.alias.as_ref().map(|alias| alias.name),
                        )
                    })
                    .collect(),
                rest: rest.as_ref().map(|ident| ident.name),
            },
            mutable,
            is_await,
        );
        out
    }

    /// Build a fresh record value for a typed record's object-pattern rest.
    /// The annotation fixes the source field set before typechecking, so each
    /// unbound field can be projected into a constructor of the remainder.
    fn lower_record_rest_initializer(
        &mut self,
        initializer: &radix_syntax::Expr,
        subject: &radix_syntax::Expr,
        fields: &[radix_syntax::ExField],
        span: Span,
    ) -> Option<HirExpression> {
        let source_def = match &initializer.kind {
            radix_syntax::ExprKind::Ident(ident) => self.lookup_name(ident.name)?,
            radix_syntax::ExprKind::Paren(inner) => match &inner.kind {
                radix_syntax::ExprKind::Ident(ident) => self.lookup_name(ident.name)?,
                _ => return None,
            },
            _ => return None,
        };
        let mut source_ty = *self.local_types.get(&source_def)?;
        let source_fields = loop {
            match self.types.get(source_ty) {
                crate::Type::Record(fields) => break fields.clone(),
                crate::Type::Alias(_, inner) => source_ty = *inner,
                _ => return None,
            }
        };
        let excluded: Vec<_> = fields.iter().map(|field| field.name.name).collect();
        let mut remaining: Vec<_> = source_fields
            .into_iter()
            .filter(|(name, _)| !excluded.contains(name))
            .collect();
        remaining.sort_by_key(|(name, _)| name.0);
        let target = self.types.record(remaining.iter().copied());
        let mut entries = Vec::with_capacity(remaining.len());
        for (name, _) in remaining {
            let value = HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Field(Box::new(self.lower_expr(subject)), name),
                ty: None,
                span,
            };
            entries.push(radix_hir::HirObjectField {
                key: radix_hir::HirObjectKey::Ident(name),
                value: Some(value),
            });
        }
        let source = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Tuple(Vec::new(), None),
            ty: None,
            span,
        };
        Some(HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Verte {
                source: Box::new(source),
                target,
                entries: Some(entries),
            },
            ty: None,
            span,
        })
    }

    /// Return the label table for a direct binding subject, including a parenthesized
    /// subject. Other expressions retain the ordinary record-field path.
    fn tuple_labels_for_subject(
        &self,
        expr: &radix_syntax::Expr,
    ) -> Option<Vec<Option<radix_lexer::Symbol>>> {
        match &expr.kind {
            radix_syntax::ExprKind::Ident(ident) => self
                .lookup_name(ident.name)
                .and_then(|def_id| self.resolver.tuple_element_labels(def_id))
                .map(ToOwned::to_owned),
            radix_syntax::ExprKind::Paren(inner) => self.tuple_labels_for_subject(inner),
            _ => None,
        }
    }

    /// Lower a tuple destructuring read with the same constant-index HIR shape
    /// used by positional destructuring and `.label` member access.
    fn lower_tuple_destructure_index(
        &mut self,
        init: &radix_syntax::Expr,
        position: usize,
        span: Span,
    ) -> HirExpression {
        let index = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Literal(HirLiteral::Int(position as u64)),
            ty: None,
            span,
        };
        HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Index(Box::new(self.lower_expr(init)), Box::new(index)),
            ty: None,
            span,
        }
    }

    fn lower_ex_stmt(&mut self, stmt: &radix_syntax::ExStmt) -> Vec<HirStatement> {
        let mut out = Vec::new();
        let mutable = stmt.mutability == radix_syntax::AstMutability::Mutable;
        let (hoisted, hoisted_subject) = self.destructure_subject(&stmt.source, false, stmt.span);
        out.extend(hoisted);
        let subject = hoisted_subject.as_ref().unwrap_or(&stmt.source);

        for field in &stmt.fields {
            let name = field
                .alias
                .as_ref()
                .map_or(field.name.name, |ident| ident.name);
            let def_id = self.next_def_id();
            self.bind_local(name, def_id);
            let init = HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Field(Box::new(self.lower_expr(subject)), field.name.name),
                ty: None,
                span: field.name.span,
            };
            out.push(self.destructured_local(def_id, name, init, mutable));
        }

        if let Some(rest) = &stmt.is_rest {
            let def_id = self.next_def_id();
            self.bind_local(rest.name, def_id);
            let local = radix_hir::HirLocal {
                def_id,
                name: rest.name,
                ty: None,
                init: Some(self.lower_expr(subject)),
                mutable: stmt.mutability == radix_syntax::AstMutability::Mutable,
                is_await: false,
                runtime_provided: false,
                await_store: None,
                size_decl: false,
                size_static: false,
            };
            out.push(HirStatement {
                id: self.next_hir_id(),
                kind: HirStatementKind::Local(local),
                span: rest.span,
            });
        }
        out
    }

    /// Lower a local variable declaration once destructuring has been expanded.
    fn lower_var_stmt(&mut self, decl: &radix_syntax::VarDecl) -> HirStatementKind {
        match &decl.binding {
            radix_syntax::BindingPattern::Ident(ident) => {
                let def_id = self.next_def_id();
                if matches!(
                    decl.await_form,
                    Some(radix_syntax::AwaitBindingForm::Assignment)
                ) {
                    self.presentation.await_assignment_bindings.push(def_id);
                }
                if decl.compile_time {
                    self.presentation.compile_time_locals.push(def_id);
                }
                if let Some(ty) = &decl.ty {
                    crate::tuple_labels::record_from_type_expr(
                        self.resolver,
                        self.interner,
                        def_id,
                        ty,
                    );
                    if let Some(labels) = lista_element_labels(ty, self.interner) {
                        self.list_element_labels.insert(def_id, labels);
                    }
                }
                self.bind_local(ident.name, def_id);
                let ty = decl.ty.as_ref().map(|ty| {
                    self.lower_authored_type(
                        radix_hir::HirTypeSite {
                            owner: radix_hir::HirOwnerRef::Def(def_id),
                            slot: radix_hir::HirTypeSlot::Declared,
                        },
                        ty,
                        false,
                        false,
                    )
                });
                if let Some(ty) = ty {
                    self.local_types.insert(def_id, ty);
                }
                // A declared integer-list binding keeps the whole-shape value
                // reading (`tensor<f32, shape>`); scalar value bindings do
                // not (size-type Z3 refuses them in extent positions).
                if ty.is_some_and(|ty| radix_types::is_integer_index_vector(self.types, ty)) {
                    self.figura_extent_admissible_locals.insert(def_id);
                }
                let init = decl.init.as_ref().map(|expr| {
                    if decl.compile_time {
                        self.lower_compile_time_initializer(expr)
                    } else {
                        self.lower_expr(expr)
                    }
                });
                HirStatementKind::Local(radix_hir::HirLocal {
                    def_id,
                    name: ident.name,
                    ty,
                    init,
                    mutable: decl.mutability == radix_syntax::AstMutability::Mutable,
                    is_await: decl.is_await(),
                    runtime_provided: false,
                    await_store: None,
                    size_decl: false,
                    size_static: false,
                })
            }
            radix_syntax::BindingPattern::Wildcard(span) => {
                let init = match decl.init.as_ref() {
                    Some(expr) => self.lower_expr(expr),
                    None => error_expr(self, *span),
                };
                // Surface fidelity: a wildcard-bound declaration is a discard
                // binding (`sit T _ ← expr`), not a bare effect call. The
                // lowered statement keeps no `HirLocal`, so the authored
                // declaration facts ride the presentation sidecar keyed by
                // the initializer expression id.
                let ty = decl.ty.as_ref().map(|ty| {
                    self.lower_authored_type(
                        radix_hir::HirTypeSite {
                            owner: radix_hir::HirOwnerRef::Hir(init.id),
                            slot: radix_hir::HirTypeSlot::DiscardBinding,
                        },
                        ty,
                        false,
                        false,
                    )
                });
                self.presentation
                    .discard_binding_stmts
                    .push(radix_hir::HirDiscardBinding {
                        expr_id: init.id,
                        ty,
                        mutable: decl.mutability == radix_syntax::AstMutability::Mutable,
                        is_await: decl.is_await(),
                    });
                HirStatementKind::Expr(init)
            }
            radix_syntax::BindingPattern::Array { span, .. } => {
                self.error("array_destructuring_not_expanded");
                HirStatementKind::Expr(error_expr(self, *span))
            }
            radix_syntax::BindingPattern::Object { span, .. } => {
                self.error("object_destructuring_not_expanded");
                HirStatementKind::Expr(error_expr(self, *span))
            }
        }
    }

    /// Lower an expression statement without changing its evaluation shape.
    fn lower_expr_stmt(&mut self, expr: &radix_syntax::ExprStmt) -> HirStatementKind {
        let expr_hir = self.lower_expr(&expr.expr);
        HirStatementKind::Expr(expr_hir)
    }

    fn lower_inc_dec_stmt(&mut self, inc_dec: &radix_syntax::IncDecStmt) -> HirStatementKind {
        let target = self.lower_expr(&inc_dec.target);
        let op = match inc_dec.op {
            radix_syntax::IncDecOp::Inc => radix_hir::HirIncDecOp::Inc,
            radix_syntax::IncDecOp::Dec => radix_hir::HirIncDecOp::Dec,
        };
        HirStatementKind::IncDec(radix_hir::HirIncDec { target, op })
    }

    /// Lower `si` as an expression statement.
    fn lower_si(&mut self, if_stmt: &radix_syntax::SiStmt) -> HirStatementKind {
        let expr = self.lower_si_expr(if_stmt);
        HirStatementKind::Expr(expr)
    }

    /// Lower `dum` as loop-like expression HIR, preserving any `cape` handler.
    fn lower_dum(&mut self, while_stmt: &radix_syntax::DumStmt) -> HirStatementKind {
        let cond = self.lower_expr(&while_stmt.cond);
        let body = self.lower_ergo_body(&while_stmt.body);
        let dum = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Dum(Box::new(cond), body),
            ty: None,
            span: self.current_span,
        };
        let expr = self.with_optional_catch(while_stmt.catch.as_ref(), dum);

        HirStatementKind::Expr(expr)
    }

    fn fresh_itera_binding_name(
        &mut self,
        pattern: &radix_syntax::BindingPattern,
    ) -> radix_lexer::Symbol {
        let mut suffix = 0_u32;
        loop {
            let candidate_text = format!("__itera_element_{suffix}");
            let candidate = self.interner.intern(&candidate_text);
            if !binding_pattern_has_name(pattern, candidate) {
                return candidate;
            }
            suffix += 1;
        }
    }

    /// Propagate the iterable's annotated element labels onto the synthetic
    /// loop element local so by-label patterns (`{gx}`) lower exactly like
    /// binding-position destructuring over the element type.
    fn record_itera_element_labels(
        &mut self,
        iterable: &radix_syntax::Expr,
        binding: radix_hir::DefId,
    ) {
        let radix_syntax::ExprKind::Ident(ident) = &iterable.kind else {
            return;
        };
        let Some(def_id) = self.lookup_name(ident.name) else {
            return;
        };
        let Some(labels) = self.list_element_labels.get(&def_id) else {
            return;
        };
        self.resolver
            .record_tuple_element_labels(binding, labels.clone());
    }

    /// Lower `itera` with a synthetic binding local for the iteration variable.
    fn lower_itera(&mut self, iter_stmt: &radix_syntax::IteraStmt) -> HirStatementKind {
        // The series forms each lower to their own node so the mode-owned
        // arity law governs every count mismatch; the single-source
        // single-binder form keeps the shared itera path untouched (need
        // 400da00a: legal N=1 is byte-identical). `ab` is the Cartesian
        // range product (never a zip) and `ex` stays the lockstep series.
        if iter_stmt.sources.len() > 1 || iter_stmt.bindings.len() > 1 {
            if matches!(iter_stmt.mode, radix_syntax::IteraMode::Ab) {
                return self.lower_range_product_itera(iter_stmt);
            }
            return self.lower_lockstep_itera(iter_stmt);
        }
        let mode = match iter_stmt.mode {
            radix_syntax::IteraMode::Ex => radix_hir::HirIteraMode::Ex,
            radix_syntax::IteraMode::De => radix_hir::HirIteraMode::De,
            radix_syntax::IteraMode::Ab => radix_hir::HirIteraMode::Ab,
        };

        let binding = self.next_def_id();
        let iter = self.lower_expr(&iter_stmt.sources[0]);
        // The parser always supplies one binder; fail closed if it did not.
        let Some(first_binding) = iter_stmt.bindings.first() else {
            self.error_with_args_at(
                vec![radix_diagnostics::DiagnosticArg::new(
                    "issue",
                    "itera_binder_pattern_unsupported",
                )],
                iter.span,
            );
            return HirStatementKind::Expr(error_expr(self, iter.span));
        };
        let binding_span = binding_pattern_span(first_binding);
        let binding_name = match first_binding {
            radix_syntax::BindingPattern::Ident(ident) => ident.name,
            radix_syntax::BindingPattern::Array { .. }
            | radix_syntax::BindingPattern::Object { .. } => {
                self.fresh_itera_binding_name(first_binding)
            }
            // The normalized loop grammar has no top-level wildcard binder.
            // Keep the parser's broader shared-pattern recovery fail-closed.
            radix_syntax::BindingPattern::Wildcard(_) => {
                self.error_with_args_at(
                    vec![radix_diagnostics::DiagnosticArg::new(
                        "issue",
                        "itera_binder_pattern_unsupported",
                    )],
                    binding_span,
                );
                return HirStatementKind::Expr(error_expr(self, binding_span));
            }
        };
        let binding_expr = radix_syntax::Expr {
            id: u32::MAX,
            kind: radix_syntax::ExprKind::Ident(radix_syntax::Ident {
                name: binding_name,
                span: binding_span,
            }),
            span: binding_span,
        };

        let (coordinate_bindings, filum_binding, body) = enter_itera_scope(
            self,
            iter_stmt,
            first_binding,
            binding_name,
            binding,
            &binding_expr,
        );
        let expr = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Itera {
                mode,
                coordinates: coordinate_bindings,
                filum: filum_binding,
                binding,
                binding_name,
                iter: Box::new(iter),
                body,
                mutable: iter_stmt.mutability == radix_syntax::AstMutability::Mutable,
            },
            ty: None,
            span: self.current_span,
        };

        let expr = self.with_optional_catch(iter_stmt.catch.as_ref(), expr);

        HirStatementKind::Expr(expr)
    }

    /// Lower the N-ary lockstep `itera ex` series to its dedicated node.
    ///
    /// Binders are names in v1: a pattern or wildcard binder
    /// on the series declines here because the node carries name bindings
    /// only. Source/binder count equality and the shared `apud` cardinality
    /// are typecheck laws over the node — same family as the single-source
    /// `apud` arity rule.
    fn lower_lockstep_itera(&mut self, iter_stmt: &radix_syntax::IteraStmt) -> HirStatementKind {
        // The series node carries no lane-binder metadata; a declared `filum`
        // clause declines whole instead of silently dropping the binder.
        if let Some(lane) = iter_stmt.filum.as_ref() {
            self.error_with_args_at(
                vec![radix_diagnostics::DiagnosticArg::new(
                    "issue",
                    "itera_filum_requires_single_source",
                )],
                lane.span,
            );
            return HirStatementKind::Expr(error_expr(self, self.current_span));
        }
        let Some(bindings) =
            self.lower_itera_name_bindings(iter_stmt, "itera_lockstep_binder_pattern_unsupported")
        else {
            return HirStatementKind::Expr(error_expr(self, self.current_span));
        };

        self.push_scope();
        let coordinate_bindings = iter_stmt
            .apud
            .iter()
            .map(|ident| {
                let def_id = self.next_def_id();
                self.bind_local(ident.name, def_id);
                (def_id, ident.name)
            })
            .collect();
        for (def_id, name) in &bindings {
            self.bind_local(*name, *def_id);
        }
        let sources = iter_stmt
            .sources
            .iter()
            .map(|source| self.lower_expr(source))
            .collect();
        let body = self.lower_ergo_body(&iter_stmt.body);
        self.pop_scope();
        let expr = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::IteraLockstep {
                sources,
                coordinates: coordinate_bindings,
                bindings,
                body,
                mutable: iter_stmt.mutability == radix_syntax::AstMutability::Mutable,
            },
            ty: None,
            span: self.current_span,
        };

        let expr = self.with_optional_catch(iter_stmt.catch.as_ref(), expr);

        HirStatementKind::Expr(expr)
    }

    /// Lower the name-only binders shared by the `itera ex` / `itera ab` series
    /// nodes; a pattern binder declines with `unsupported_issue`.
    fn lower_itera_name_bindings(
        &mut self,
        iter_stmt: &radix_syntax::IteraStmt,
        unsupported_issue: &'static str,
    ) -> Option<Vec<(radix_hir::DefId, radix_lexer::Symbol)>> {
        let mut bindings = Vec::new();
        for binding in &iter_stmt.bindings {
            let radix_syntax::BindingPattern::Ident(ident) = binding else {
                self.error_with_args_at(
                    vec![radix_diagnostics::DiagnosticArg::new(
                        "issue",
                        unsupported_issue,
                    )],
                    binding_pattern_span(binding),
                );
                return None;
            };
            let def_id = self.next_def_id();
            bindings.push((def_id, ident.name));
        }
        Some(bindings)
    }

    /// Lower the N-ary Cartesian `itera ab` range product to its node.
    ///
    /// Unlike the lockstep series, range sources lower OUTSIDE the binder
    /// scope: an extent referencing an outer name keeps that outer reference,
    /// so the extent/binder shadowing law is checkable at typecheck instead
    /// of silently re-binding the extent to the loop binder. Binders are
    /// names in v1 (same stance as the lockstep series). The product carries
    /// no `apud` coordinates — coordinates require `ex` iteration, so a
    /// product head with an `apud` clause declines through the same issue as
    /// the single-source form.
    fn lower_range_product_itera(
        &mut self,
        iter_stmt: &radix_syntax::IteraStmt,
    ) -> HirStatementKind {
        if let Some(coordinate) = iter_stmt.apud.first() {
            self.error_with_args_at(
                vec![radix_diagnostics::DiagnosticArg::new(
                    "issue",
                    "itera_apud_requires_ex",
                )],
                coordinate.span,
            );
            return HirStatementKind::Expr(error_expr(self, self.current_span));
        }
        // The range-product node carries no lane-binder metadata; a declared
        // `filum` clause declines whole instead of silently dropping it.
        if let Some(lane) = iter_stmt.filum.as_ref() {
            self.error_with_args_at(
                vec![radix_diagnostics::DiagnosticArg::new(
                    "issue",
                    "itera_filum_requires_single_source",
                )],
                lane.span,
            );
            return HirStatementKind::Expr(error_expr(self, self.current_span));
        }
        let Some(bindings) =
            self.lower_itera_name_bindings(iter_stmt, "itera_binder_pattern_unsupported")
        else {
            return HirStatementKind::Expr(error_expr(self, self.current_span));
        };

        let ranges = iter_stmt
            .sources
            .iter()
            .map(|source| self.lower_expr(source))
            .collect();

        self.push_scope();
        for (def_id, name) in &bindings {
            self.bind_local(*name, *def_id);
        }
        let body = self.lower_ergo_body(&iter_stmt.body);
        self.pop_scope();
        let expr = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::IteraRange {
                ranges,
                bindings,
                body,
                mutable: iter_stmt.mutability == radix_syntax::AstMutability::Mutable,
            },
            ty: None,
            span: self.current_span,
        };

        let expr = self.with_optional_catch(iter_stmt.catch.as_ref(), expr);

        HirStatementKind::Expr(expr)
    }

    /// Lower `redde` as a control-transfer statement.
    fn lower_redde(&mut self, ret: &radix_syntax::ReddeStmt) -> HirStatementKind {
        let value = ret.value.as_ref().map(|e| self.lower_expr(e));
        HirStatementKind::Redde(value)
    }

    /// Lower recoverable `iace` as a throw expression statement.
    fn lower_iace(&mut self, stmt: &radix_syntax::IaceStmt) -> HirStatementKind {
        let value = self.lower_expr(&stmt.value);
        HirStatementKind::Expr(HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Throw(Box::new(value)),
            ty: None,
            span: self.current_span,
        })
    }

    /// Lower unrecoverable `mori` as a panic expression statement.
    fn lower_mori(&mut self, stmt: &radix_syntax::MoriStmt) -> HirStatementKind {
        let value = self.lower_expr(&stmt.value);
        HirStatementKind::Expr(HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Panic(Box::new(value)),
            ty: None,
            span: self.current_span,
        })
    }

    /// Lower `adfirma` as an assertion expression statement.
    fn lower_adfirma(&mut self, stmt: &radix_syntax::AdfirmaStmt) -> HirStatementKind {
        let cond = self.lower_expr(&stmt.cond);
        HirStatementKind::Expr(HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Adfirma(
                Box::new(cond),
                stmt.message
                    .as_ref()
                    .map(|message| Box::new(self.lower_expr(message))),
            ),
            ty: None,
            span: self.current_span,
        })
    }

    /// Lower `requirit` as the recoverable-guard desugar `si non (cond) { iace err }`.
    ///
    /// The throw enters the enclosing function's `⇥ E` channel; a `⇥`-less
    /// function is rejected downstream by the same `iace` channel check. This
    /// mirrors `adfirma`'s shape with a recoverable, catchable payload.
    fn lower_require(&mut self, stmt: &radix_syntax::RequireStmt) -> HirStatementKind {
        self.lower_guard_desugar(&stmt.cond, &stmt.error, true)
    }

    /// Lower `reice` as the reject-guard desugar `si (cond) { iace err }` — the
    /// boolean opposite of `requirit`: it fails when the condition holds.
    fn lower_reject(&mut self, stmt: &radix_syntax::RejectStmt) -> HirStatementKind {
        self.lower_guard_desugar(&stmt.cond, &stmt.error, false)
    }

    /// Shared `requirit` / `reice` guard desugar: a single `iace` in a branch
    /// whose condition is `cond` (or its negation) throws the authored error.
    fn lower_guard_desugar(
        &mut self,
        cond: &radix_syntax::Expr,
        error: &radix_syntax::Expr,
        negate: bool,
    ) -> HirStatementKind {
        let cond = self.lower_expr(cond);
        let error = self.lower_expr(error);
        let cond = if negate {
            HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Unary(radix_hir::HirUnOp::Not, Box::new(cond)),
                ty: None,
                span: self.current_span,
            }
        } else {
            cond
        };
        let throw_stmt = HirStatement {
            id: self.next_hir_id(),
            kind: HirStatementKind::Expr(HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Throw(Box::new(error)),
                ty: None,
                span: self.current_span,
            }),
            span: self.current_span,
        };
        let then_block = HirBlock::simple(vec![throw_stmt], None, self.current_span);
        let guard_id = self.next_hir_id();
        self.presentation.guard_si_ids.push(guard_id);
        HirStatementKind::Expr(HirExpression {
            id: guard_id,
            kind: HirExpressionKind::Si {
                cond: Box::new(cond),
                then_block,
                then_catch: None,
                else_block: None,
            },
            ty: None,
            span: self.current_span,
        })
    }

    /// Lower diagnostic output (`scribe` / `vide` / `mone` / `nota`) as an expression statement.
    fn lower_scribe(&mut self, stmt: &radix_syntax::ScribeStmt) -> HirStatementKind {
        let args = stmt.args.iter().map(|arg| self.lower_expr(arg)).collect();
        let kind = match stmt.kind {
            radix_syntax::ScribeKind::Nota => HirScribeKind::Nota,
            radix_syntax::ScribeKind::Vide => HirScribeKind::Vide,
            radix_syntax::ScribeKind::Mone => HirScribeKind::Mone,
            radix_syntax::ScribeKind::Scribe => HirScribeKind::Scribe,
        };
        HirStatementKind::Expr(HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Scribe(kind, args),
            ty: None,
            span: self.current_span,
        })
    }

    /// Lower `incipit` / `incipiet` entry blocks with optional CLI argument bindings.
    fn lower_incipit_stmt(&mut self, stmt: &radix_syntax::IncipitStmt) -> HirStatementKind {
        self.push_scope();
        let mut args_binding = None;
        if let Some(args) = &stmt.args {
            let def_id = self.next_def_id();
            self.bind_local(args.name, def_id);
            args_binding = Some((args.name, args.span, def_id));
        }
        let mut block = self.lower_ergo_body(&stmt.body);
        if let Some((name, span, def_id)) = args_binding {
            // Entry arguments are source-visible locals, whether they are the
            // default text array or a package CLI record.
            block.statements.insert(
                0,
                HirStatement {
                    id: self.next_hir_id(),
                    kind: HirStatementKind::Local(radix_hir::HirLocal {
                        def_id,
                        name,
                        ty: Some(self.incipit_args_type()),
                        init: None,
                        mutable: false,
                        is_await: false,
                        runtime_provided: true,
                        await_store: None,
                        size_decl: false,
                        size_static: false,
                    }),
                    span,
                },
            );
        }
        // Preserve exitus as the tail expression of the incipit block.
        if let Some(exitus) = &stmt.exitus {
            let exitus_expr = self.lower_expr(exitus);
            block.expr = Some(Box::new(exitus_expr));
        }
        self.pop_scope();
        HirStatementKind::Expr(HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Block(block),
            ty: None,
            span: self.current_span,
        })
    }

    fn lower_ex(&mut self, _stmt: &radix_syntax::ExStmt) -> HirStatementKind {
        self.error("ex_destructuring_not_expanded");
        HirStatementKind::Expr(error_expr(self, self.current_span))
    }

    /// Lower `custodi` guard sections into a first-class HIR statement.
    fn lower_custodi(&mut self, stmt: &radix_syntax::CustodiStmt) -> HirStatementKind {
        if stmt.clauses.is_empty() {
            return HirStatementKind::Expr(error_expr(self, self.current_span));
        }

        let clauses = stmt
            .clauses
            .iter()
            .map(|clause| radix_hir::HirCustodiClause {
                cond: self.lower_expr(&clause.cond),
                body: self.lower_ergo_body(&clause.body),
                span: clause.span,
            })
            .collect();

        HirStatementKind::Custodi(radix_hir::HirCustodi { clauses })
    }

    fn lower_fac(&mut self, stmt: &radix_syntax::FacStmt) -> HirStatementKind {
        let expr = self.lower_fac_expr(stmt);
        HirStatementKind::Expr(expr)
    }

    /// Lower value dispatch `elige` into a single-subject `discerne` expression.
    fn lower_elige(&mut self, elige_stmt: &radix_syntax::EligeStmt) -> HirStatementKind {
        let scrutinee = self.lower_expr(&elige_stmt.expr);
        let mut arms = Vec::new();

        for case in &elige_stmt.cases {
            let pattern = self.lower_elige_case_pattern(&case.value, case.span);

            let block = self.lower_ergo_body(&case.body);
            let body = self.expr_from_block(block, case.span);
            arms.push(HirCasuArm {
                patterns: vec![pattern],
                body,
                span: case.span,
            });
        }

        if let Some(default) = &elige_stmt.default {
            let block = self.lower_ergo_body(&default.body);
            let body = self.expr_from_block(block, default.span);
            arms.push(HirCasuArm {
                patterns: vec![HirPattern::Wildcard],
                body,
                span: default.span,
            });
        }

        let expr = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Discerne {
                scrutinees: vec![scrutinee],
                arms,
                exhaustive: false,
            },
            ty: None,
            span: self.current_span,
        };

        // Surface fidelity: authored `elige` re-sugars with the `elige`
        // vocabulary (`switch`/`default` under `en`), never `discerne`/`casu _`.
        // Record the `Discerne` node's own id: a trailing `cape` wraps the
        // expression in a fresh node, but emission matches the inner id.
        self.presentation.elige_expr_ids.push(expr.id);
        let expr = self.with_optional_catch(elige_stmt.catch.as_ref(), expr);

        HirStatementKind::Expr(expr)
    }

    /// Lower one `elige` `casu` value into the literal or enum-variant pattern HIR expects.
    fn lower_elige_case_pattern(
        &mut self,
        value: &radix_syntax::Expr,
        span: radix_lexer::Span,
    ) -> HirPattern {
        match &value.kind {
            radix_syntax::ExprKind::Literal(lit) => pattern::lower_literal(self, lit, span),
            radix_syntax::ExprKind::KeywordLiteral(keyword) => {
                pattern::lower_literal(self, &keyword.literal, span)
            }
            // D22.2: `casu -1` / `casu -∞` — a minus directly over a literal.
            // `lower_negated_literal` rejects a non-number operand.
            radix_syntax::ExprKind::Unary(unary)
                if unary.op == radix_syntax::UnOp::Neg
                    && let Some(lit) = unary.operand.kind.literal() =>
            {
                pattern::lower_negated_literal(self, lit, span)
            }
            radix_syntax::ExprKind::Ident(ident) => {
                self.lower_elige_variant_pattern(ident.name, span)
            }
            radix_syntax::ExprKind::Member(member) => {
                // CG2-R07 mirror: a member chain with leading path segments
                // must resolve the leading path as a real qualifier before
                // the penultimate segment can name the parent enum/union —
                // `casu missing.A.X { }` must never silently bind the local
                // `A.X` through the bare-name fallback.
                if let Some(segments) = member_segments(member) {
                    if segments.len() >= 3 {
                        match self.resolver.qualified_variant_path(&segments) {
                            crate::QualifiedVariantPathLookup::Variant(def_id) => {
                                self.record_variant_path(def_id, &segments);
                                return HirPattern::Variant(def_id, Vec::new());
                            }
                            crate::QualifiedVariantPathLookup::UnknownVariant
                            | crate::QualifiedVariantPathLookup::NotQualified => {
                                self.current_span = span;
                                self.error("unknown_variant");
                                return HirPattern::Wildcard;
                            }
                        }
                    }
                    if segments.len() == 2
                        && let Some((def_id, _)) = self
                            .resolver
                            .namespace_file_variant(segments[0], segments[1])
                    {
                        self.record_variant_path(def_id, &segments);
                        return HirPattern::Variant(def_id, Vec::new());
                    }
                }
                // Qualified `casu A.X`: resolve the suffix against A's union
                // when A is a known union/enum; otherwise fall back to the
                // bare-name path (single-union behavior preserved).
                if let radix_syntax::ExprKind::Ident(base) = &member.object.kind
                    && let Some(base_def) = self.lookup_name(base.name)
                    && self
                        .resolver
                        .get_symbol(base_def)
                        .is_some_and(|symbol| symbol.kind == crate::SymbolKind::Enum)
                {
                    if let Some(variant_def) =
                        self.resolver.variant_child(base_def, member.member.name)
                    {
                        if let Some(segments) = member_segments(member) {
                            self.record_variant_path(variant_def, &segments);
                        }
                        return HirPattern::Variant(variant_def, Vec::new());
                    }
                    self.current_span = span;
                    self.error("elige_case_value_shape");
                    return HirPattern::Wildcard;
                }
                self.lower_elige_variant_pattern(member.member.name, span)
            }
            _ => {
                self.current_span = span;
                self.error("elige_case_value_shape");
                HirPattern::Wildcard
            }
        }
    }

    fn lower_elige_variant_pattern(&mut self, name: radix_lexer::Symbol, span: Span) -> HirPattern {
        let Some(def_id) =
            self.lookup_name(name)
                .or_else(|| match self.resolver.lookup_variant_bare(name) {
                    crate::BareVariantLookup::Single(def_id) => Some(def_id),
                    crate::BareVariantLookup::Ambiguous { .. } | crate::BareVariantLookup::None => {
                        None
                    }
                })
        else {
            self.current_span = span;
            self.error("elige_case_value_shape");
            return HirPattern::Wildcard;
        };
        if matches!(
            self.resolver.get_symbol(def_id).map(|symbol| symbol.kind),
            Some(crate::SymbolKind::Variant)
        ) {
            HirPattern::Variant(def_id, Vec::new())
        } else {
            self.current_span = span;
            self.error("elige_case_value_shape");
            HirPattern::Wildcard
        }
    }

    /// Lower pattern dispatch `discerne` into a multi-subject match expression.
    fn lower_discerne(&mut self, discerne_stmt: &radix_syntax::DiscerneStmt) -> HirStatementKind {
        let scrutinees = match discerne_stmt.subjects.as_slice() {
            [] => {
                self.error("discerne_subject_required");
                vec![error_expr(self, self.current_span)]
            }
            many => many.iter().map(|expr| self.lower_expr(expr)).collect(),
        };

        let mut arms = Vec::new();
        for arm in &discerne_stmt.arms {
            self.current_span = arm.span;
            self.push_scope();

            // Pattern lowering may bind names, so each arm receives its own
            // scope before the arm body is lowered.
            let patterns = match arm.patterns.as_slice() {
                [] => {
                    self.error("discerne_casu_pattern_required");
                    vec![HirPattern::Wildcard]
                }
                many => {
                    let lowered: Vec<_> = many
                        .iter()
                        .map(|pattern| pattern::lower_pattern(self, pattern))
                        .collect();
                    if lowered.len() != scrutinees.len() {
                        self.error("discerne_pattern_count");
                    }
                    lowered
                }
            };

            let block = self.lower_ergo_body(&arm.body);
            let body = self.expr_from_block(block, arm.span);
            arms.push(HirCasuArm {
                patterns,
                body,
                span: arm.span,
            });
            self.pop_scope();
        }

        if let Some(default) = &discerne_stmt.default {
            let block = self.lower_ergo_body(&default.body);
            let body = self.expr_from_block(block, default.span);
            arms.push(HirCasuArm {
                patterns: (0..scrutinees.len().max(1))
                    .map(|_| HirPattern::Wildcard)
                    .collect(),
                body,
                span: default.span,
            });
        }

        let expr = HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Discerne {
                scrutinees,
                arms,
                exhaustive: discerne_stmt.exhaustive,
            },
            ty: None,
            span: self.current_span,
        };

        HirStatementKind::Expr(expr)
    }

    fn lower_si_expr(&mut self, if_stmt: &radix_syntax::SiStmt) -> HirExpression {
        let cond = self.lower_expr(&if_stmt.cond);
        let then_block = self.lower_ergo_body(&if_stmt.then);
        let then_catch = if_stmt
            .catch
            .as_ref()
            .map(|catch| Box::new(self.lower_cape_clause(catch)));
        let else_block = if_stmt
            .else_
            .as_ref()
            .map(|secus| self.lower_secus_clause(secus));

        HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Si {
                cond: Box::new(cond),
                then_block,
                then_catch,
                else_block,
            },
            ty: None,
            span: self.current_span,
        }
    }

    fn lower_secus_clause(&mut self, clause: &radix_syntax::SecusClause) -> HirBlock {
        match clause {
            radix_syntax::SecusClause::Sin { stmt, .. } => {
                let expr = self.lower_si_expr(stmt);
                self.block_from_expr(expr)
            }
            radix_syntax::SecusClause::Block { body, catch, .. } => {
                let block = self.lower_block(body);
                if let Some(catch) = catch {
                    let cape = self.lower_cape_clause(catch);
                    let expr = self.handled_block_expr(block, cape, body.span);
                    self.block_from_expr(expr)
                } else {
                    block
                }
            }
            radix_syntax::SecusClause::Stmt { stmt, catch, .. } => {
                let lowered = lower_stmt_expanded(self, stmt);
                self.attach_stmt_trivia(stmt, &lowered);
                let block = HirBlock::simple(lowered, None, stmt.span);
                if let Some(catch) = catch {
                    let cape = self.lower_cape_clause(catch);
                    let expr = self.handled_block_expr(block, cape, stmt.span);
                    self.block_from_expr(expr)
                } else {
                    block
                }
            }
        }
    }

    fn expr_from_block(&mut self, block: HirBlock, span: Span) -> HirExpression {
        HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Block(block),
            ty: None,
            span,
        }
    }

    pub(super) fn block_from_expr(&mut self, expr: HirExpression) -> HirBlock {
        HirBlock::simple(Vec::new(), Some(Box::new(expr)), self.current_span)
    }

    fn lower_cape_clause(&mut self, catch: &radix_syntax::CapeClause) -> radix_hir::HirCape {
        self.push_scope();
        let catch_def_id = self.next_def_id();
        self.bind_local(catch.binding.name, catch_def_id);
        let body = self.lower_block(&catch.body);
        self.pop_scope();
        radix_hir::HirCape {
            binding_def_id: catch_def_id,
            binding_name: catch.binding.name,
            binding_ty: None,
            body,
            span: catch.span,
        }
    }

    fn handled_expr(&mut self, body: HirExpression, catch: radix_hir::HirCape) -> HirExpression {
        let span = self.current_span;
        let body = self.block_from_expr(body);
        self.handled_block_expr(body, catch, span)
    }

    /// Wrap a lowered loop or dispatch expression in its authored `cape`
    /// clause when one is present.
    fn with_optional_catch(
        &mut self,
        catch: Option<&radix_syntax::CapeClause>,
        expr: HirExpression,
    ) -> HirExpression {
        if let Some(catch) = catch {
            let cape = self.lower_cape_clause(catch);
            self.handled_expr(expr, cape)
        } else {
            expr
        }
    }

    fn handled_block_expr(
        &mut self,
        body: HirBlock,
        catch: radix_hir::HirCape,
        span: Span,
    ) -> HirExpression {
        HirExpression {
            id: self.next_hir_id(),
            kind: HirExpressionKind::Handled {
                body,
                catch: Box::new(catch),
            },
            ty: None,
            span,
        }
    }

    pub(super) fn lower_fac_expr(&mut self, stmt: &radix_syntax::FacStmt) -> HirExpression {
        let mut block = self.lower_block(&stmt.body);
        block.breakable = Some(match stmt.modifier {
            radix_syntax::FacModifier::Omnia => radix_hir::HirBreakableKind::FacOmnia,
            radix_syntax::FacModifier::Machina => radix_hir::HirBreakableKind::FacMachina,
            radix_syntax::FacModifier::None => radix_hir::HirBreakableKind::Fac,
        });
        let expr = if let Some(cond) = &stmt.while_ {
            let negate_cond = HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Unary(
                    radix_hir::HirUnOp::Not,
                    Box::new(self.lower_expr(cond)),
                ),
                ty: None,
                span: cond.span,
            };
            let break_stmt = HirStatement {
                id: self.next_hir_id(),
                kind: HirStatementKind::Rumpe,
                span: cond.span,
            };
            // The authored `fac` body boundary owns one structural end already
            // recorded by `block_end_for`; carry it onto the loop wrapper so
            // the end-count parity with `presentation.block_ends` holds.
            let end = block.end;
            let mut loop_block = HirBlock::simple(
                block
                    .statements
                    .into_iter()
                    .chain(std::iter::once(HirStatement {
                        id: self.next_hir_id(),
                        kind: HirStatementKind::Expr(HirExpression {
                            id: self.next_hir_id(),
                            kind: HirExpressionKind::Si {
                                cond: Box::new(negate_cond),
                                then_block: HirBlock::simple(vec![break_stmt], None, cond.span),
                                then_catch: None,
                                else_block: None,
                            },
                            ty: None,
                            span: cond.span,
                        }),
                        span: cond.span,
                    }))
                    .collect(),
                None,
                stmt.body.span,
            );
            loop_block.end = end;
            // The authored `omnia` marker rides the synthesized loop block so
            // the re-emitter prints it. `FacOmniaLoop` is a marker, not a
            // scope: every backend and MIR lower the loop as plain `fac … dum`.
            match stmt.modifier {
                radix_syntax::FacModifier::Omnia => {
                    loop_block.breakable = Some(radix_hir::HirBreakableKind::FacOmniaLoop);
                }
                radix_syntax::FacModifier::Machina => {
                    loop_block.breakable = Some(radix_hir::HirBreakableKind::FacMachinaLoop);
                }
                radix_syntax::FacModifier::None => {}
            }
            HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Loop(loop_block),
                ty: None,
                span: self.current_span,
            }
        } else {
            HirExpression {
                id: self.next_hir_id(),
                kind: HirExpressionKind::Block(block),
                ty: None,
                span: self.current_span,
            }
        };

        if let Some(catch) = &stmt.catch {
            let cape = self.lower_cape_clause(catch);
            self.handled_expr(expr, cape)
        } else {
            expr
        }
    }
}

/// Coordinate bindings, the optional single binding, and the loop body scope.
type IteraScopeResult = (
    Vec<(radix_hir::DefId, radix_lexer::Symbol)>,
    Option<(radix_hir::DefId, radix_lexer::Symbol)>,
    HirBlock,
);

fn enter_itera_scope(
    lowerer: &mut Lowerer,
    iter_stmt: &radix_syntax::IteraStmt,
    first_binding: &radix_syntax::BindingPattern,
    binding_name: radix_lexer::Symbol,
    binding: radix_hir::DefId,
    binding_expr: &radix_syntax::Expr,
) -> IteraScopeResult {
    lowerer.push_scope();
    let coordinate_bindings = iter_stmt
        .apud
        .iter()
        .map(|ident| {
            let def_id = lowerer.next_def_id();
            lowerer.bind_local(ident.name, def_id);
            (def_id, ident.name)
        })
        .collect();
    // The lane binder is declared alongside the loop bindings (same
    // shape as `summa`'s filum binder); the runner lowers the plain
    // loop, so the binder never becomes a runtime local.
    let filum_binding = iter_stmt.filum.as_ref().map(|ident| {
        let def_id = lowerer.next_def_id();
        lowerer.bind_local(ident.name, def_id);
        (def_id, ident.name)
    });
    lowerer.bind_local(binding_name, binding);
    if matches!(first_binding, radix_syntax::BindingPattern::Object { .. }) {
        lowerer.record_itera_element_labels(&iter_stmt.sources[0], binding);
    }
    let pattern_locals = match first_binding {
        radix_syntax::BindingPattern::Array { .. } => lowerer.lower_array_destructure_pattern(
            first_binding,
            binding_expr,
            iter_stmt.mutability,
            false,
        ),
        radix_syntax::BindingPattern::Object { .. } => lowerer.lower_object_destructure_pattern(
            first_binding,
            binding_expr,
            iter_stmt.mutability,
            false,
        ),
        // Wildcard binders returned above, so only identifiers reach here.
        radix_syntax::BindingPattern::Ident(_) | radix_syntax::BindingPattern::Wildcard(_) => {
            Vec::new()
        }
    };
    let mut body = lowerer.lower_ergo_body(&iter_stmt.body);
    body.statements.splice(0..0, pattern_locals);
    lowerer.pop_scope();
    (coordinate_bindings, filum_binding, body)
}

/// Whether a destructuring subject is a side-effect-free place that each
/// expanded local may re-read (identifiers, parenthesized places, and member
/// reads rooted at one). Faber members are plain fields, never getters.
fn is_pure_place(expr: &radix_syntax::Expr) -> bool {
    match &expr.kind {
        radix_syntax::ExprKind::Ident(_) | radix_syntax::ExprKind::Ego(_) => true,
        radix_syntax::ExprKind::Paren(inner) => is_pure_place(inner),
        radix_syntax::ExprKind::Member(member) => is_pure_place(&member.object),
        _ => false,
    }
}
