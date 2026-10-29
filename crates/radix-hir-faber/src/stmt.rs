//! Statement and block emission for the canonical Faber backend.
//!
//! This module reconstructs grammar-valid Faber statements from HIR, placing
//! authored comments from the presentation sidecar at their render slots.
//! Layout is normalized rather than reproduced verbatim. Helpers here recognize
//! HIR shapes that came from compact Faber syntax and rebuild the canonical
//! form only when the shape is unambiguous and no comment slot is lost.
//!
//! INVARIANTS
//! ==========
//! - Blocks preserve statement order and optional tail expressions.
//! - Compact branch output is used only when a block is equivalent to returning
//!   one expression.
//! - `si`/`sin`/`secus` chains are reconstructed from nested conditional HIR
//!   without changing branch semantics.
//! - Expression-level constructs such as `fac`/`dum` loops and `discerne` arms
//!   reuse the same block helpers and presentation-trivia slots.
//! - Catch blocks, `ad` bodies, and control statements are emitted as canonical
//!   Faber syntax accepted by the parser.

use crate::surface::KeywordSurface;
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    DefId, HirBlock, HirCape, HirExpression, HirExpressionKind, HirLocal, HirOwnerRef,
    HirStatement, HirStatementKind, HirTypeSite, HirTypeSlot,
};
use radix_lexer::{Symbol, TokenKind};

impl super::FaberCodegen<'_> {
    /// Print ` omnia` or ` machina` after `fac` when the synthesized `fac … dum` loop block
    /// carries the authored marker.
    pub(super) fn write_fac_omnia_marker(
        &self,
        loop_block: &HirBlock,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let Some(kind) = loop_block
            .breakable
            .filter(|kind| kind.is_loop_marker())
            .and_then(radix_hir::HirBreakableKind::fac_modifier_key)
        {
            writer.write(" ");
            writer.write(surface.contextual_keyword(kind));
        }
    }

    /// Recognize the normalized HIR shape for a `fac ... dum` loop
    /// (see [`HirBlock::fac_dum_parts`]).
    pub(super) fn as_fac_loop<'a>(
        &self,
        block: &'a HirBlock,
    ) -> Option<(&'a [HirStatement], &'a HirExpression)> {
        block.fac_dum_parts().map(|(body, _, cond)| (body, cond))
    }

    /// Return the expression represented by a single-expression return block.
    ///
    /// This is the gate for compact `ergo redde` and `c ✓ a ✗ b`
    /// output. Blocks with additional statements or mixed tail forms stay in
    /// brace form to avoid changing control-flow meaning. The shape check is
    /// trivia-free; [`FaberCodegen::ergo_return_expr`] and
    /// [`FaberCodegen::check_ballot_branches`] additionally require that
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
    /// eligible. The compact `c ✓ a ✗ b` expression form has no
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

    /// Compact `ergo redde` gate: the eligible return expression when the
    /// candidate block is a single-expression return shape with no attached
    /// trivia on its statements or structural end. Comment-bearing blocks
    /// stay braced (compact joints have no comment slot). Sharing one lookup
    /// between the gate and the emission keeps them from drifting.
    pub(super) fn ergo_return_expr<'a>(&self, block: &'a HirBlock) -> Option<&'a HirExpression> {
        if self.trivia.block_has_trivia(block) {
            return None;
        }
        self.return_expr(block)
    }

    pub(super) fn write_statement_trivia(&self, stmt: &HirStatement, writer: &mut CodeWriter) {
        if let Some(trivia) = self.trivia.for_hir(stmt.id) {
            crate::trivia::write_leading_trivia(trivia, self.interner, writer);
        }
    }

    /// Emit a block's structural-end trivia immediately before its closing
    /// brace. Synthesized blocks (no authored end) render nothing.
    pub(super) fn write_block_end_trivia(&self, block: &HirBlock, writer: &mut CodeWriter) {
        if let Some(end) = &block.end
            && let Some(trivia) = self.trivia.for_anchor(&end.id)
        {
            crate::trivia::write_leading_trivia(trivia, self.interner, writer);
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
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.write(surface.contextual_keyword("si"));
        writer.write(" ");
        self.write_expr(cond, names, surface, writer);
        self.write_si_branch_body(then_block, names, surface, writer);

        if let Some(catch) = catch {
            self.write_arm_cape(catch, names, surface, writer);
        }

        let mut next_else = else_block;
        while let Some(block) = next_else {
            writer.newline();
            if let Some((sin_cond, sin_then, sin_catch, sin_else)) = self.as_sin_arm(block) {
                writer.write(surface.contextual_keyword("sin"));
                writer.write(" ");
                self.write_expr(sin_cond, names, surface, writer);
                self.write_si_branch_body(sin_then, names, surface, writer);
                if let Some(sin_catch) = sin_catch {
                    self.write_arm_cape(sin_catch, names, surface, writer);
                }
                next_else = sin_else;
            } else if let Some((body, else_catch)) = self.as_secus_arm_catch(block) {
                // The handled body is the authored arm block itself; bracing
                // it keeps the handler attached to the `secus` arm instead of
                // compacting the synthetic tail into `ergo redde fac …`.
                writer.write(surface.contextual_keyword("secus"));
                writer.writeln(" {");
                writer.indented(|writer| {
                    self.write_block(body, names, surface, writer);
                    self.write_block_end_trivia(body, writer);
                });
                writer.write("}");
                self.write_arm_cape(else_catch, names, surface, writer);
                break;
            } else {
                writer.write(surface.contextual_keyword("secus"));
                self.write_si_branch_body(block, names, surface, writer);
                break;
            }
        }
    }

    /// Write an arm-attached `cape` clause on its own line after the arm body.
    fn write_arm_cape(
        &self,
        catch: &HirCape,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.newline();
        writer.write(surface.keyword(TokenKind::Cape));
        writer.write(" ");
        self.write_cape_block_named(
            &catch.body,
            Some(catch.binding_name),
            names,
            surface,
            writer,
        );
    }

    /// Detect an else-block that is a `sin` arm, with or without its own
    /// arm-attached `cape` handler.
    ///
    /// Unlike [`FaberCodegen::as_sin_branch`] (the compact `c ✓ a ✗ b`
    /// gate, which has no handler slot), the statement chain can carry a
    /// per-arm `cape`, so a nested conditional with `then_catch` still
    /// flattens to `sin cond { … } cape err { … }`.
    #[allow(clippy::type_complexity)]
    fn as_sin_arm<'a>(
        &self,
        block: &'a HirBlock,
    ) -> Option<(
        &'a HirExpression,
        &'a HirBlock,
        Option<&'a HirCape>,
        Option<&'a HirBlock>,
    )> {
        if !block.statements.is_empty() {
            return None;
        }
        let expr = block.expr.as_ref()?;
        if let HirExpressionKind::Si {
            cond,
            then_block,
            then_catch,
            else_block,
        } = &expr.kind
        {
            Some((cond, then_block, then_catch.as_deref(), else_block.as_ref()))
        } else {
            None
        }
    }

    /// Detect a `secus { … } cape err { … }` arm.
    ///
    /// Lowering wraps a handled else-arm as a block whose only content is a
    /// tail `Handled` expression over the authored arm block. The authored
    /// arm block never carries a tail expression, which separates it from a
    /// `fac` statement (whose `Handled` body wraps a tail `Block`/`Loop`)
    /// and from an authored `secus { fac { … } cape … }` (a statement, not a
    /// tail).
    fn as_secus_arm_catch<'a>(&self, block: &'a HirBlock) -> Option<(&'a HirBlock, &'a HirCape)> {
        if !block.statements.is_empty() {
            return None;
        }
        let expr = block.expr.as_ref()?;
        let HirExpressionKind::Handled { body, catch } = &expr.kind else {
            return None;
        };
        if body.expr.is_some() {
            return None;
        }
        Some((body, catch))
    }

    /// Re-sugar a guard-desugared conditional back to `requirit`/`reice`.
    ///
    /// Requires the then-block to be exactly one `iace` statement with no tail
    /// and no authored trivia (compact guards have no comment slot). The
    /// negated condition marks `requirit`; the plain condition marks `reice`.
    /// Returns `false` when the shape is not a guard, leaving the caller to
    /// emit the ordinary conditional.
    pub(super) fn write_guard_stmt(
        &self,
        cond: &HirExpression,
        then_block: &HirBlock,
        names: &FxHashMap<DefId, Symbol>,
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
            self.write_expr(inner, names, surface, writer);
        } else {
            writer.write(surface.keyword(TokenKind::Reice));
            writer.write(" ");
            self.write_expr(cond, names, surface, writer);
        }
        writer.write(" ");
        writer.write(surface.keyword(TokenKind::Iace));
        writer.write(" ");
        self.write_expr(error, names, surface, writer);
        true
    }

    /// Try to write a conditional expression as the value conditional
    /// `c ✓ a ✗ b` (D4.5). The glyphs are the same in every reader locale.
    ///
    /// Returns `false` when either branch needs block syntax or the else
    /// branch continues an `si`/`sin` chain. The conditional is one level only
    /// (D4.6), so a chain stays in the ordinary conditional form, which
    /// preserves grammar validity without pretending arbitrary blocks are
    /// expressions.
    pub(super) fn write_check_ballot(
        &self,
        cond: &HirExpression,
        then_block: &HirBlock,
        else_block: Option<&HirBlock>,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) -> bool {
        let Some((then_expr, else_expr)) = self.check_ballot_branches(then_block, else_block)
        else {
            return false;
        };

        self.write_expr_prec(cond, 2, names, surface, writer);
        writer.write(" ✓ ");
        self.write_expr_prec(then_expr, 2, names, surface, writer);
        writer.write(" ✗ ");
        self.write_expr_prec(else_expr, 2, names, surface, writer);
        true
    }

    /// Return both branch values when a conditional fits `c ✓ a ✗ b`.
    ///
    /// Each branch must be a bare tail expression with no attached trivia on
    /// its statements or structural end; comment-bearing branches stay braced.
    /// A branch whose value is an explicit `redde` statement would lose its
    /// return in the expression form (see [`FaberCodegen::tail_expression`]),
    /// and an else branch that is itself a `sin` conditional would nest.
    pub(super) fn check_ballot_branches<'a>(
        &self,
        then_block: &'a HirBlock,
        else_block: Option<&'a HirBlock>,
    ) -> Option<(&'a HirExpression, &'a HirExpression)> {
        let else_block = else_block?;
        if self.trivia.block_has_trivia(then_block)
            || self.trivia.block_has_trivia(else_block)
            || self.as_sin_branch(else_block).is_some()
        {
            return None;
        }
        Some((
            self.tail_expression(then_block)?,
            self.tail_expression(else_block)?,
        ))
    }

    pub(super) fn write_si_branch_body(
        &self,
        block: &HirBlock,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let Some(return_expr) = self.ergo_return_expr(block) {
            writer.write(" ");
            writer.write(surface.contextual_keyword("ergo"));
            writer.write(" ");
            writer.write(surface.contextual_keyword("redde"));
            writer.write(" ");
            self.write_expr(return_expr, names, surface, writer);
            return;
        }

        writer.writeln(" {");
        writer.indented(|writer| {
            self.write_block(block, names, surface, writer);
            self.write_block_end_trivia(block, writer);
        });
        writer.write("}");
    }

    /// Write a `cape` handler body, using `binding_name` when HIR carries it
    /// outside the block's leading local (structured `fac`/`cape` forms).
    pub(super) fn write_cape_block_named(
        &self,
        block: &HirBlock,
        binding_name: Option<Symbol>,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        // Only the synthetic runtime-provided binding local is skipped. A
        // `cape err { … }` handler whose first authored statement is a real
        // local (`sit textus c ← …`) must keep it — skipping on "first
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
            writer.write(&self.symbol_to_string(name));
            writer.writeln(" {");
            writer.indented(|writer| {
                self.write_stmts(
                    &block.statements[usize::from(skip_binding_local)..],
                    names,
                    surface,
                    writer,
                );
                if let Some(expr) = &block.expr {
                    self.write_expr(expr, names, surface, writer);
                    writer.newline();
                }
                self.write_block_end_trivia(block, writer);
            });
            writer.write("}");
            return;
        }

        writer.writeln("_ {");
        writer.indented(|writer| {
            self.write_block(block, names, surface, writer);
            self.write_block_end_trivia(block, writer);
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
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        self.write_block_from(block, 0, names, surface, writer);
    }

    /// Emit a block starting at statement `start` (a loop binder may already
    /// own the leading destructuring run), followed by its tail expression.
    pub(super) fn write_block_from(
        &self,
        block: &HirBlock,
        start: usize,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        self.write_stmts(&block.statements[start..], names, surface, writer);
        if let Some(expr) = &block.expr {
            if let Some(trivia) = self.trivia.for_hir(expr.id) {
                crate::trivia::write_leading_trivia(trivia, self.interner, writer);
            }
            self.write_expr(expr, names, surface, writer);
            writer.newline();
        }
    }

    /// Emit a statement run, re-sugaring authored destructuring expansions
    /// back to their pattern declaration.
    pub(super) fn write_stmts(
        &self,
        stmts: &[HirStatement],
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        let mut index = 0;
        while index < stmts.len() {
            let stmt = &stmts[index];
            if let Some((binding, subject)) = self.destructure_run(&stmts[index..]) {
                self.write_statement_trivia(stmt, writer);
                writer.write(destructure_keyword(binding, surface));
                writer.write(" ");
                self.write_destructure_pattern(&binding.pattern, surface, writer);
                writer.write(" ← ");
                self.write_expr(subject, names, surface, writer);
                writer.newline();
                index += binding.stmt_ids.len();
                continue;
            }
            index += 1;
            // Omit compiler-synthesized locals (shader implicit IO, CLI args);
            // they are runtime-provided, not authored source.
            if let HirStatementKind::Local(local) = &stmt.kind
                && local.runtime_provided
            {
                continue;
            }
            self.write_stmt(stmt, names, surface, writer);
        }
    }

    /// The authored destructuring binding whose lowered run starts `stmts`,
    /// with the subject expression the pattern reads. `None` when the run is
    /// not intact (emission then falls back to the expanded locals).
    pub(super) fn destructure_run<'s>(
        &'s self,
        stmts: &'s [HirStatement],
    ) -> Option<(&'s radix_hir::HirDestructureBinding, &'s HirExpression)> {
        let first = stmts.first()?;
        let binding = self.destructure_bindings.get(&first.id)?;
        let run = stmts.get(..binding.stmt_ids.len())?;
        if !run
            .iter()
            .zip(&binding.stmt_ids)
            .all(|(stmt, id)| stmt.id == *id)
        {
            return None;
        }
        let subject = match &first.kind {
            HirStatementKind::Local(local) if binding.hoisted => local.init.as_ref()?,
            HirStatementKind::Local(local) => destructure_place(local.init.as_ref()?)?,
            HirStatementKind::Expr(expr) if !binding.hoisted => expr,
            _ => return None,
        };
        Some((binding, subject))
    }

    /// Write `[a, _, rest r]` or `{x, y ut z, rest r}`.
    pub(super) fn write_destructure_pattern(
        &self,
        pattern: &radix_hir::HirDestructurePattern,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        let (open, close, mut items, rest) = match pattern {
            radix_hir::HirDestructurePattern::Array { elements, rest } => (
                "[",
                "]",
                elements
                    .iter()
                    .map(|element| {
                        element.map_or_else(|| "_".to_owned(), |name| self.symbol_to_string(name))
                    })
                    .collect::<Vec<_>>(),
                rest,
            ),
            radix_hir::HirDestructurePattern::Object { fields, rest } => (
                "{",
                "}",
                fields
                    .iter()
                    .map(|(name, alias)| match alias {
                        Some(alias) => format!(
                            "{} {} {}",
                            self.symbol_to_string(*name),
                            surface.keyword(TokenKind::Ut),
                            self.symbol_to_string(*alias)
                        ),
                        None => self.symbol_to_string(*name),
                    })
                    .collect::<Vec<_>>(),
                rest,
            ),
        };
        if let Some(rest) = rest {
            items.push(format!(
                "{} {}",
                surface.keyword(TokenKind::Ceteri),
                self.symbol_to_string(*rest)
            ));
        }
        writer.write(open);
        writer.write(&items.join(", "));
        writer.write(close);
    }

    pub(super) fn write_stmt(
        &self,
        stmt: &HirStatement,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        self.write_statement_trivia(stmt, writer);
        match &stmt.kind {
            HirStatementKind::Local(local) => {
                self.write_local_stmt(local, names, surface, writer);
            }
            HirStatementKind::Expr(expr) => {
                // Authored discard binding (`sit T _ ← expr`): lowering drops
                // the declaration, so the presentation facts restore it.
                if let Some(discard) = self.discard_bindings.get(&expr.id) {
                    let keyword = match (discard.is_await, discard.mutable) {
                        (true, false) => surface.contextual_keyword("figendum"),
                        (true, true) => surface.contextual_keyword("variandum"),
                        (false, false) if discard.ty.is_some() => surface.contextual_keyword("sit"),
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
                        let rendered = self.render_type_at(&site, ty, names, surface);
                        writer.write(&rendered);
                        writer.write(" ");
                    }
                    writer.write("_ ← ");
                    self.write_expr(expr, names, surface, writer);
                    writer.newline();
                    return;
                }
                self.write_expr(expr, names, surface, writer);
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
                    self.write_expr(future, names, surface, writer);
                    writer.newline();
                    return;
                }
                writer.write(surface.contextual_keyword("redde"));
                if let Some(expr) = value {
                    writer.write(" ");
                    self.write_expr(expr, names, surface, writer);
                }
                writer.newline();
            }
            HirStatementKind::ReddeFilum(value) => {
                writer.write(surface.contextual_keyword("redde"));
                writer.write(" ");
                writer.write(surface.contextual_keyword("filum"));
                if let Some(expr) = value {
                    writer.write(" ");
                    self.write_expr(expr, names, surface, writer);
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
                self.write_expr(&inc_dec.target, names, surface, writer);
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
                        self.write_expr(&clause.cond, names, surface, writer);
                        writer.writeln(" {");
                        writer.indented(|writer| {
                            self.write_block(&clause.body, names, surface, writer);
                            self.write_block_end_trivia(&clause.body, writer);
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
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if local.size_decl {
            // `const size H ← h` / static `const size N = 3` (la `magnitudo`):
            // the declaration spelling is the whole statement — no type
            // annotation, no generic params; the glyph law picks `=` for the
            // compile-time natural and `←` for the runtime lift.
            writer.write(surface.contextual_keyword("fixum"));
            writer.write(" ");
            writer.write(surface.keyword(TokenKind::Magnitudo));
            writer.write(" ");
            writer.write(&self.symbol_to_string(local.name));
            if let Some(init) = &local.init {
                if local.size_static {
                    writer.write(" = ");
                } else {
                    writer.write(" ← ");
                }
                self.write_expr(init, names, surface, writer);
            }
            writer.newline();
            return;
        }
        let await_assignment = self.await_assignment_bindings.contains(&local.def_id);
        // `fixum` states a compile-time fact (`=`); a run-time read-only name
        // is `sit`. A `ref` view of a writable-name cell leads with the mode
        // word the type renders, with no `varia`.
        let compile_time = self.compile_time_locals.contains(&local.def_id);
        let site = HirTypeSite {
            owner: HirOwnerRef::Def(local.def_id),
            slot: HirTypeSlot::Declared,
        };
        let ref_prefix = format!("{} ", surface.keyword(TokenKind::De));
        let ref_local = local.mutable
            && !local.is_await
            && !await_assignment
            && local.init.is_some()
            && local.ty.is_some_and(|ty| {
                self.render_type_at(&site, ty, names, surface)
                    .starts_with(&ref_prefix)
            });
        let keyword = match (local.is_await, await_assignment, local.mutable) {
            (false, _, false) | (true, true, false) if !compile_time => {
                surface.contextual_keyword("sit")
            }
            (true, true, false) | (false, _, false) => surface.contextual_keyword("fixum"),
            (false, _, true) if ref_local => "",
            (true, true, true) | (false, _, true) => surface.contextual_keyword("varia"),
            (true, false, false) => surface.contextual_keyword("figendum"),
            (true, false, true) => surface.contextual_keyword("variandum"),
        };
        if !keyword.is_empty() {
            writer.write(keyword);
            writer.write(" ");
        }
        let mut value_marker: Option<&str> = None;
        if let Some(ty) = local.ty {
            // The authored form at this site is the sole authority for
            // concrete versus hole spelling. An absent site falls back to
            // the canonical concrete type in `type_to_faber_at`. An
            // initializer-free authored hole re-emits as `_`: a bare hole
            // binding with a later assignment type-checks, so materializing
            // the inferred type would change the authored surface and fail
            // output re-validation.
            let mut rendered = self.render_type_at(&site, ty, names, surface);
            // Value-side marker: an exclusive `copy`/`own` binding re-emits as
            // `T b ← copy a` / `T b ← own a` (operator ruling 2026-10-08), so
            // the mode word leaves the type and is written before the
            // initializer.
            if local
                .init
                .as_ref()
                .is_some_and(|init| !matches!(init.kind, HirExpressionKind::ConversioAssign { .. }))
                && let Some(marker) = self.exclusive_binding_marker(ty)
            {
                let word = surface.keyword(marker);
                if let Some(rest) = rendered.strip_prefix(&format!("{word} ")) {
                    rendered = rest.to_owned();
                    value_marker = Some(word);
                }
            }
            // Every declaration writes its type slot: an inferred local
            // re-emits as `let _ name ← e`.
            writer.write(&rendered);
            writer.write(" ");
        } else {
            // A pre-finalization or hand-built local may not have a semantic
            // `ty` field yet. A present form still owns its authored spelling;
            // use its witness for the same site-aware render path.
            let rendered = match self.authored_type_forms.get(&site).map(|form| form.ty) {
                Some(form_ty) => self.render_type_at(&site, form_ty, names, surface),
                None => "_".to_owned(),
            };
            writer.write(&rendered);
            writer.write(" ");
        }
        writer.write(&self.symbol_to_string(local.name));
        if let Some(init) = &local.init {
            if let HirExpressionKind::ConversioAssign {
                source,
                recovery,
                params,
                ..
            } = &init.kind
            {
                // Typed `↤` initializer: lowering stores the binding as the
                // assignment target, so re-emit as
                // `binding ↤ source [⊥ default]` — never `binding ← binding
                // ↤ source`.
                writer.write(" ↤ ");
                self.write_expr_prec(source, 1, names, surface, writer);
                self.write_conversio_assign_via(params, surface, writer);
                if let Some(recovery) = recovery {
                    writer.write(" ⊥ ");
                    self.write_expr_prec(recovery, 12, names, surface, writer);
                }
            } else {
                let glyph = if self.compile_time_locals.contains(&local.def_id) {
                    " = "
                } else if await_assignment {
                    " ↢ "
                } else {
                    " ← "
                };
                writer.write(glyph);
                if let Some(word) = value_marker {
                    writer.write(word);
                    writer.write(" ");
                }
                self.write_expr(init, names, surface, writer);
            }
        }
        writer.newline();
    }
}

/// Declaration keyword for an authored destructuring binding.
pub(super) fn destructure_keyword<'s>(
    binding: &radix_hir::HirDestructureBinding,
    surface: &'s impl KeywordSurface,
) -> &'s str {
    match (binding.is_await, binding.mutable) {
        (false, false) => surface.contextual_keyword("sit"),
        (false, true) => surface.contextual_keyword("varia"),
        (true, false) => surface.contextual_keyword("figendum"),
        (true, true) => surface.contextual_keyword("variandum"),
    }
}

/// Recover the place subject from the first expanded local of an un-hoisted
/// destructuring run: the receiver of its index, field, or `omissa` read, or
/// the whole initializer for a whole-value rest.
fn destructure_place(init: &HirExpression) -> Option<&HirExpression> {
    match &init.kind {
        HirExpressionKind::Index(subject, _)
        | HirExpressionKind::Field(subject, _)
        | HirExpressionKind::MethodCall(subject, ..) => Some(subject),
        HirExpressionKind::Tuple(elements, _) => match &elements.first()?.kind {
            HirExpressionKind::Index(subject, _) => Some(subject),
            _ => None,
        },
        _ => Some(init),
    }
}
