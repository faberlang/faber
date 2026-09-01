//! Statement and block emission for the canonical Faber backend.
//!
//! This module reconstructs grammar-valid Faber statements from HIR. The goal
//! is normalized source output that can be parsed again, not recovery of the
//! user's exact source layout, comments, or spelling choices. Helpers here
//! recognize HIR shapes that came from compact Faber syntax and rebuild the
//! canonical form only when the shape is unambiguous.
//!
//! INVARIANTS
//! ==========
//! - Blocks preserve statement order and optional tail expressions.
//! - Compact branch output is used only when a block is equivalent to returning
//!   one expression.
//! - `si`/`sin`/`secus` chains are reconstructed from nested conditional HIR
//!   without changing branch semantics.
//! - Expression-level constructs such as `fac`/`dum` loops and `discerne` arms
//!   can reuse these block helpers without reintroducing source-layout trivia.
//! - Catch blocks, `ad` bodies, and control statements are emitted as canonical
//!   Faber syntax accepted by the parser.

use crate::surface::KeywordSurface;
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    DefId, HirBlock, HirCape, HirExpression, HirExpressionKind, HirLocal, HirOwnerRef,
    HirStatement, HirStatementKind, HirTypeSite, HirTypeSlot,
};
use radix_lexer::{Interner, Symbol, TokenKind};
use radix_types::TypeTable;
use rustc_hash::FxHashMap;

impl super::FaberCodegen {
    /// Recognize the normalized HIR shape for a `fac ... dum` loop.
    ///
    /// The parser represents this as a body followed by a final conditional
    /// break on the negated loop condition. Re-emission only treats that as
    /// `fac`/`dum` when the final block is exactly the synthetic `rumpe`.
    ///
    /// Also accepts `else_block: Some(empty)` when the compiler produces an
    /// empty else clause for the break condition.
    pub(super) fn as_fac_loop<'a>(
        &self,
        block: &'a HirBlock,
    ) -> Option<(&'a [HirStatement], &'a HirExpression)> {
        let last = block.statements.last()?;
        let HirStatementKind::Expr(expr) = &last.kind else {
            return None;
        };
        let HirExpressionKind::Si {
            cond,
            then_block,
            then_catch: None,
            else_block,
        } = &expr.kind
        else {
            return None;
        };
        if let Some(eb) = else_block
            && (!eb.statements.is_empty() || eb.expr.is_some())
        {
            return None;
        }
        let HirExpressionKind::Unary(radix_hir::HirUnOp::Not, inner) = &cond.kind else {
            return None;
        };
        if then_block.expr.is_some() || then_block.statements.len() != 1 {
            return None;
        }
        if !matches!(then_block.statements[0].kind, HirStatementKind::Rumpe) {
            return None;
        }
        Some((&block.statements[..block.statements.len() - 1], inner))
    }

    /// Return the expression represented by a single-expression return block.
    ///
    /// This is the gate for compact `ergo redde` and `sic ... secus ...`
    /// output. Blocks with additional statements or mixed tail forms stay in
    /// brace form to avoid changing control-flow meaning. The shape check is
    /// trivia-free; [`FaberCodegen::can_write_ergo`] and
    /// [`FaberCodegen::can_write_sic_secus_chain`] additionally require that
    /// the candidate blocks carry no attached trivia.
    pub(super) fn return_expr<'a>(&self, block: &'a HirBlock) -> Option<&'a HirExpression> {
        if block.statements.is_empty() {
            return block.expr.as_deref();
        }
        if block.expr.is_some() || block.statements.len() != 1 {
            return None;
        }
        match &block.statements[0].kind {
            HirStatementKind::Redde(Some(expr)) => Some(expr),
            _ => None,
        }
    }

    /// Return the bare tail expression of a block, if any.
    ///
    /// Unlike [`FaberCodegen::return_expr`], a `redde` statement is not
    /// eligible. The compact `sic ... secus ...` expression form has no
    /// statement slot, so a branch whose value is an explicit `redde` must stay
    /// in the statement chain (`ergo redde`): compressing it into a bare
    /// expression drops the return and the re-emitted function no longer
    /// terminates (SEM033 on re-check).
    pub(super) fn tail_expression<'a>(&self, block: &'a HirBlock) -> Option<&'a HirExpression> {
        if block.statements.is_empty() {
            return block.expr.as_deref();
        }
        None
    }

    /// Gate for compact `ergo redde` output.
    ///
    /// The candidate block must be a single-expression return shape with no
    /// attached trivia on its statements or structural end; comment-bearing
    /// blocks stay braced (compact joints have no comment slot).
    pub(super) fn can_write_ergo(&self, block: &HirBlock) -> bool {
        self.return_expr(block).is_some() && !self.trivia.block_has_trivia(block)
    }

    pub(super) fn write_statement_trivia(
        &self,
        stmt: &HirStatement,
        interner: &Interner,
        writer: &mut CodeWriter,
    ) {
        if let Some(trivia) = self.trivia.for_hir(stmt.id) {
            crate::trivia::write_leading_trivia(trivia, interner, writer);
        }
    }

    /// Emit a block's structural-end trivia immediately before its closing
    /// brace. Synthesized blocks (no authored end) render nothing.
    pub(super) fn write_block_end_trivia(
        &self,
        block: &HirBlock,
        interner: &Interner,
        writer: &mut CodeWriter,
    ) {
        if let Some(end) = &block.end
            && let Some(trivia) = self.trivia.for_anchor(&end.id)
        {
            crate::trivia::write_leading_trivia(trivia, interner, writer);
        }
    }

    /// Detect an else-block that is semantically a `sin` branch.
    ///
    /// HIR nests `else if` as an else block whose tail expression is another
    /// conditional. The canonical Faber backend flattens that shape back into a
    /// `sin` chain when there are no extra statements to preserve.
    pub(super) fn as_sin_branch<'a>(
        &self,
        block: &'a HirBlock,
    ) -> Option<(&'a HirExpression, &'a HirBlock, Option<&'a HirBlock>)> {
        if !block.statements.is_empty() {
            return None;
        }

        let expr = block.expr.as_ref()?;
        if let HirExpressionKind::Si {
            cond,
            then_block,
            then_catch: None,
            else_block,
        } = &expr.kind
        {
            Some((cond, then_block, else_block.as_ref()))
        } else {
            None
        }
    }

    /// Write a full `si`/`sin`/`secus` statement chain.
    ///
    /// The chain is syntactic normalization only: every `sin` comes from a
    /// nested conditional else-tail, and any else-block that does not fit that
    /// exact shape remains a terminal `secus` block. An optional `cape` handler
    /// is written before the `sin`/`secus` chain — cape belongs to the then-block,
    /// not after else.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn write_si_chain(
        &self,
        cond: &HirExpression,
        then_block: &HirBlock,
        catch: Option<&HirCape>,
        else_block: Option<&HirBlock>,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.write(surface.contextual_keyword("si"));
        writer.write(" ");
        self.write_expr(cond, types, names, interner, surface, writer);
        self.write_si_branch_body(then_block, types, names, interner, surface, writer);

        if let Some(catch) = catch {
            writer.newline();
            writer.write(surface.keyword(TokenKind::Cape));
            writer.write(" ");
            self.write_cape_block_named(
                &catch.body,
                Some(catch.binding_name),
                types,
                names,
                interner,
                surface,
                writer,
            );
        }

        let mut next_else = else_block;
        while let Some(block) = next_else {
            writer.newline();
            if let Some((sin_cond, sin_then, sin_else)) = self.as_sin_branch(block) {
                writer.write(surface.contextual_keyword("sin"));
                writer.write(" ");
                self.write_expr(sin_cond, types, names, interner, surface, writer);
                self.write_si_branch_body(sin_then, types, names, interner, surface, writer);
                next_else = sin_else;
            } else {
                writer.write(surface.contextual_keyword("secus"));
                self.write_si_branch_body(block, types, names, interner, surface, writer);
                break;
            }
        }
    }

    /// Re-sugar a guard-desugared conditional back to `requirit`/`reice`.
    ///
    /// Requires the then-block to be exactly one `iace` statement with no tail
    /// and no authored trivia (compact guards have no comment slot). The
    /// negated condition marks `requirit`; the plain condition marks `reice`.
    /// Returns `false` when the shape is not a guard, leaving the caller to
    /// emit the ordinary conditional.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn write_guard_stmt(
        &self,
        cond: &HirExpression,
        then_block: &HirBlock,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) -> bool {
        if then_block.expr.is_some() || then_block.statements.len() != 1 {
            return false;
        }
        if self.trivia.block_has_trivia(then_block) {
            return false;
        }
        let HirStatementKind::Expr(expr) = &then_block.statements[0].kind else {
            return false;
        };
        let HirExpressionKind::Throw(error) = &expr.kind else {
            return false;
        };

        if let HirExpressionKind::Unary(radix_hir::HirUnOp::Not, inner) = &cond.kind {
            writer.write(surface.keyword(TokenKind::Requirit));
            writer.write(" ");
            self.write_expr(inner, types, names, interner, surface, writer);
        } else {
            writer.write(surface.keyword(TokenKind::Reice));
            writer.write(" ");
            self.write_expr(cond, types, names, interner, surface, writer);
        }
        writer.write(" ");
        writer.write(surface.keyword(TokenKind::Iace));
        writer.write(" ");
        self.write_expr(error, types, names, interner, surface, writer);
        true
    }

    /// Try to write a conditional expression as compact `sic ... secus ...`.
    ///
    /// Returns `false` when either branch needs block syntax. Callers then fall
    /// back to the ordinary conditional form, which preserves grammar validity
    /// without pretending arbitrary blocks are expressions.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn write_sic_secus_chain(
        &self,
        cond: &HirExpression,
        then_block: &HirBlock,
        else_block: Option<&HirBlock>,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) -> bool {
        if !self.can_write_sic_secus_chain(then_block, else_block) {
            return false;
        }

        let Some(then_expr) = self.tail_expression(then_block) else {
            return false;
        };
        let Some(else_block) = else_block else {
            return false;
        };

        self.write_expr_prec(cond, 2, types, names, interner, surface, writer);
        writer.write(" ");
        writer.write(surface.keyword(TokenKind::Sic));
        writer.write(" ");
        self.write_expr_prec(then_expr, 2, types, names, interner, surface, writer);
        writer.write(" ");
        writer.write(surface.contextual_keyword("secus"));
        writer.write(" ");

        if let Some((sin_cond, sin_then, sin_else)) = self.as_sin_branch(else_block) {
            return self.write_sic_secus_chain(
                sin_cond, sin_then, sin_else, types, names, interner, surface, writer,
            );
        }

        let Some(else_expr) = self.tail_expression(else_block) else {
            return false;
        };
        self.write_expr_prec(else_expr, 2, types, names, interner, surface, writer);
        true
    }

    /// Validate that a whole conditional chain can be rendered compactly.
    ///
    /// Every candidate branch must be a single-return shape with no attached
    /// trivia on its statements or structural end; comment-bearing branches
    /// stay braced. Only bare tail expressions are eligible — a branch whose
    /// value is an explicit `redde` statement would lose its return in the
    /// expression form (see [`FaberCodegen::tail_expression`]).
    pub(super) fn can_write_sic_secus_chain(
        &self,
        then_block: &HirBlock,
        else_block: Option<&HirBlock>,
    ) -> bool {
        if self.tail_expression(then_block).is_none() || self.trivia.block_has_trivia(then_block) {
            return false;
        }

        let Some(else_block) = else_block else {
            return false;
        };

        if let Some((_, sin_then, sin_else)) = self.as_sin_branch(else_block) {
            return self.can_write_sic_secus_chain(sin_then, sin_else);
        }

        self.tail_expression(else_block).is_some() && !self.trivia.block_has_trivia(else_block)
    }

    pub(super) fn write_si_branch_body(
        &self,
        block: &HirBlock,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if self.can_write_ergo(block) {
            let return_expr = self.return_expr(block).expect("gated by can_write_ergo");
            writer.write(" ");
            writer.write(surface.contextual_keyword("ergo"));
            writer.write(" ");
            writer.write(surface.contextual_keyword("redde"));
            writer.write(" ");
            self.write_expr(return_expr, types, names, interner, surface, writer);
            return;
        }

        writer.writeln(" {");
        writer.indented(|writer| {
            self.write_block(block, types, names, interner, surface, writer);
            self.write_block_end_trivia(block, interner, writer);
        });
        writer.write("}");
    }

    /// Write a `cape` handler body, using `binding_name` when HIR carries it
    /// outside the block's leading local (structured `fac`/`cape` forms).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn write_cape_block_named(
        &self,
        block: &HirBlock,
        binding_name: Option<Symbol>,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        // Only the synthetic runtime-provided binding local is skipped. A
        // `cape err { … }` handler whose first authored statement is a real
        // local (`fixum textus c ← …`) must keep it — skipping on "first
        // statement is a local" alone dropped authored bindings.
        let skip_binding_local = block.statements.first().is_some_and(
            |stmt| matches!(&stmt.kind, HirStatementKind::Local(local) if local.runtime_provided),
        );
        let name = binding_name.or_else(|| {
            block.statements.first().and_then(|stmt| match &stmt.kind {
                HirStatementKind::Local(local) => Some(local.name),
                _ => None,
            })
        });

        if let Some(name) = name {
            writer.write(&self.symbol_to_string(name, interner));
            writer.writeln(" {");
            writer.indented(|writer| {
                for stmt in block
                    .statements
                    .iter()
                    .skip(usize::from(skip_binding_local))
                {
                    self.write_stmt(stmt, types, names, interner, surface, writer);
                }
                if let Some(expr) = &block.expr {
                    self.write_expr(expr, types, names, interner, surface, writer);
                    writer.newline();
                }
                self.write_block_end_trivia(block, interner, writer);
            });
            writer.write("}");
            return;
        }

        writer.writeln("_ {");
        writer.indented(|writer| {
            self.write_block(block, types, names, interner, surface, writer);
            self.write_block_end_trivia(block, interner, writer);
        });
        writer.write("}");
    }

    /// Emit a block's statements followed by its tail expression, if present.
    ///
    /// Tail expressions stay as expression statements because this backend
    /// emits normalized source text, not lowered target-language control flow.
    pub(super) fn write_block(
        &self,
        block: &HirBlock,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        for stmt in &block.statements {
            // Omit compiler-synthesized locals (shader implicit IO, CLI args);
            // they are runtime-provided, not authored source.
            if let HirStatementKind::Local(local) = &stmt.kind
                && local.runtime_provided
            {
                continue;
            }
            self.write_stmt(stmt, types, names, interner, surface, writer);
        }
        if let Some(expr) = &block.expr {
            self.write_expr(expr, types, names, interner, surface, writer);
            writer.newline();
        }
    }

    pub(super) fn write_stmt(
        &self,
        stmt: &HirStatement,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        self.write_statement_trivia(stmt, interner, writer);
        match &stmt.kind {
            HirStatementKind::Local(local) => {
                self.write_local_stmt(local, types, names, interner, surface, writer);
            }
            HirStatementKind::Expr(expr) => {
                // Authored discard binding (`fixum T _ ← expr`): lowering drops
                // the declaration, so the presentation facts restore it.
                if let Some(discard) = self.discard_bindings.get(&expr.id) {
                    let keyword = match (discard.is_await, discard.mutable) {
                        (true, false) => surface.contextual_keyword("figendum"),
                        (true, true) => surface.contextual_keyword("variandum"),
                        (false, false) => surface.contextual_keyword("fixum"),
                        (false, true) => surface.contextual_keyword("varia"),
                    };
                    writer.write(keyword);
                    writer.write(" ");
                    if let Some(ty) = discard.ty {
                        let site = HirTypeSite {
                            owner: HirOwnerRef::Hir(expr.id),
                            slot: HirTypeSlot::DiscardBinding,
                        };
                        let rendered = self
                            .type_to_faber_at(&site, ty, types, names, interner, surface)
                            .unwrap_or_else(|error| self.record_type_emission_error(error));
                        writer.write(&rendered);
                        writer.write(" ");
                    }
                    writer.write("_ ← ");
                    self.write_expr(expr, types, names, interner, surface, writer);
                    writer.newline();
                    return;
                }
                self.write_expr(expr, types, names, interner, surface, writer);
                writer.newline();
            }
            HirStatementKind::Redde(value) => {
                if let Some(HirExpression {
                    kind: HirExpressionKind::Reddet(future),
                    ..
                }) = value
                {
                    writer.write(surface.contextual_keyword("reddet"));
                    writer.write(" ");
                    self.write_expr(future, types, names, interner, surface, writer);
                    writer.newline();
                    return;
                }
                writer.write(surface.contextual_keyword("redde"));
                if let Some(expr) = value {
                    writer.write(" ");
                    self.write_expr(expr, types, names, interner, surface, writer);
                }
                writer.newline();
            }
            HirStatementKind::Rumpe => {
                writer.writeln(surface.contextual_keyword("rumpe"));
            }
            HirStatementKind::Perge => {
                writer.writeln(surface.contextual_keyword("perge"));
            }
            HirStatementKind::Tacet => {
                writer.writeln(surface.contextual_keyword("tacet"));
            }
            HirStatementKind::IncDec(inc_dec) => {
                self.write_expr(&inc_dec.target, types, names, interner, surface, writer);
                writer.write(match inc_dec.op {
                    radix_hir::HirIncDecOp::Inc => " ↑",
                    radix_hir::HirIncDecOp::Dec => " ↓",
                });
                writer.newline();
            }
            HirStatementKind::Custodi(custodi) => {
                writer.write(surface.keyword(TokenKind::Custodi));
                writer.writeln(" {");
                writer.indented(|writer| {
                    for clause in &custodi.clauses {
                        writer.write(surface.contextual_keyword("si"));
                        writer.write(" ");
                        self.write_expr(&clause.cond, types, names, interner, surface, writer);
                        writer.writeln(" {");
                        writer.indented(|writer| {
                            self.write_block(&clause.body, types, names, interner, surface, writer);
                            self.write_block_end_trivia(&clause.body, interner, writer);
                        });
                        writer.writeln("}");
                    }
                });
                writer.writeln("}");
            }
        }
    }

    /// Emit a type-first local declaration.
    fn write_local_stmt(
        &self,
        local: &HirLocal,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        let keyword = match (local.is_await, local.mutable) {
            (true, false) => surface.contextual_keyword("figendum"),
            (true, true) => surface.contextual_keyword("variandum"),
            (false, false) => surface.contextual_keyword("fixum"),
            (false, true) => surface.contextual_keyword("varia"),
        };
        writer.write(keyword);
        writer.write(" ");
        let site = HirTypeSite {
            owner: HirOwnerRef::Def(local.def_id),
            slot: HirTypeSlot::Declared,
        };
        if let Some(ty) = local.ty {
            let hole_authored = self.authored_type_forms.get(&site).is_some_and(|form| {
                matches!(
                    form.head,
                    radix_hir::HirTypeFormHead::Infer | radix_hir::HirTypeFormHead::UnionHole
                )
            });
            if local.init.is_some() || !hole_authored {
                // The authored form at this site is the sole authority for
                // concrete versus hole spelling. An absent site falls back to
                // the canonical concrete type in `type_to_faber_at`.
                let rendered = self
                    .type_to_faber_at(&site, ty, types, names, interner, surface)
                    .unwrap_or_else(|error| self.record_type_emission_error(error));
                writer.write(&rendered);
            } else {
                // An initializer-free hole is materialized so the bare
                // binding still type-checks, preserving the existing
                // re-check-safety policy.
                writer.write(&self.type_to_faber(ty, types, names, interner, surface));
            }
            writer.write(" ");
        } else if local.init.is_some() {
            // A pre-finalization or hand-built local may not have a semantic
            // `ty` field yet. A present form still owns its authored spelling;
            // use its witness for the same site-aware render path.
            if let Some(form_ty) = self.authored_type_forms.get(&site).map(|form| form.ty) {
                let rendered = self
                    .type_to_faber_at(&site, form_ty, types, names, interner, surface)
                    .unwrap_or_else(|error| self.record_type_emission_error(error));
                writer.write(&rendered);
                writer.write(" ");
            }
        }
        writer.write(&self.symbol_to_string(local.name, interner));
        if let Some(init) = &local.init {
            if let HirExpressionKind::ConversioAssign {
                source, recovery, ..
            } = &init.kind
            {
                // Typed `↤` initializer: lowering stores the binding as the
                // assignment target, so re-emit as
                // `binding ↤ source [⇥ recovery]` — never `binding ← binding
                // ↤ source`.
                writer.write(" ↤ ");
                self.write_expr_prec(source, 1, types, names, interner, surface, writer);
                if let Some(recovery) = recovery {
                    writer.write(" ⇥ ");
                    self.write_expr_prec(recovery, 12, types, names, interner, surface, writer);
                }
            } else {
                writer.write(" ← ");
                self.write_expr(init, types, names, interner, surface, writer);
            }
        }
        writer.newline();
    }
}
