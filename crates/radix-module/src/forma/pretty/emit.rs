//! Pretty-v1 emitter: width-aware Faber layout from AST + leading trivia.
//!
//! Layout rules:
//! - one statement per line inside a block, 4-space indent, braces attached;
//! - `si`/`sin`/`secus` chains and `fac`/`cape` handler blocks render with
//!   non-cuddled brace-attached constructs (operator decision 2026-08-09): the
//!   closing brace sits on its own line and every continuation keyword starts
//!   on a fresh line — never `} sin … {`, `} cape … {`;
//! - compact one-statement arms stay compact when they fit, expand to a block
//!   when the arm exceeds the soft width; empty blocks stay compact;
//! - width-aware wrapping at syntactic boundaries only (params, args, record
//!   fields, already-safe grouping); fit-if-possible, one per line when
//!   wrapped, no trailing commas; source-multiline constructs are preserved;
//! - comment text and blank-line runs preserved byte-for-byte (marker spacing
//!   normalized), no prose reflow.

use crate::lexer::{keyword_spelling_for_token, Interner, Span, Symbol, Token, TokenKind};
#[allow(clippy::wildcard_imports)]
use crate::syntax::*;

use super::super::author::TriviaScope;
use super::layout::{LineIndex, PrettyWriter, SOFT_WIDTH};
use super::trivia::write_leading_trivia;
use crate::FaberOutput;

pub struct PrettyEmitter<'a> {
    interner: &'a Interner,
    line_index: LineIndex,
    w: PrettyWriter,
}

/// Render a program under the `pretty-v1` layout policy.
///
/// `source` is the peeled body the spans point into; it supplies the
/// source-multiline preservation signal.
#[must_use]
pub fn emit(program: &Program, interner: &Interner, source: &str) -> FaberOutput {
    let mut emitter = PrettyEmitter { interner, line_index: LineIndex::new(source), w: PrettyWriter::new() };
    for (index, stmt) in program.statements.iter().enumerate() {
        let scope = if index == 0 {
            TriviaScope::ProgramStart
        } else {
            TriviaScope::TopLevel
        };
        emitter.write_stmt(stmt, scope);
    }
    let mut code = emitter.w.finish();
    if !code.ends_with('\n') {
        code.push('\n');
    }
    FaberOutput { code }
}

impl PrettyEmitter<'_> {
    // =======================================================================
    // Measurement helpers
    // =======================================================================

    fn source_multiline(&self, span: Span) -> bool {
        self.line_index.is_multiline(span)
    }

    /// A span covering a list's item range (first item start to last item end),
    /// used where the AST carries no direct list span.
    fn span_between(&self, first: Span, last: Span) -> Span {
        Span::new(first.start.min(last.start), first.end.max(last.end))
    }

    /// Render `f` into an inline trial at the current column and commit the
    /// result when the inline form fits (no line breaks, width within the soft
    /// limit).
    fn try_inline<F>(&mut self, f: F) -> bool
    where
        F: FnOnce(&mut Self),
    {
        let start_col = self.w.column();
        let saved = std::mem::replace(&mut self.w, PrettyWriter::trial_at(start_col));
        f(self);
        let trial = std::mem::replace(&mut self.w, saved);
        if trial.is_failed() || trial.column() > SOFT_WIDTH {
            return false;
        }
        self.w.write(&trial.finish());
        true
    }

    /// Inline-if-fits list layout. On inline failure, propagates the failure
    /// when nested inside an inline trial, otherwise runs the expanded form.
    fn write_list_layout<F, G>(&mut self, inline: F, expanded: G)
    where
        F: FnOnce(&mut Self),
        G: FnOnce(&mut Self),
    {
        if self.try_inline(inline) {
            return;
        }
        if self.w.is_inline_trial() {
            self.w.fail();
            return;
        }
        expanded(self);
    }

    /// Run a closure with one extra indentation level. Scoped on the emitter
    /// (not the writer) so nested code can keep calling emitter methods.
    fn indented<F>(&mut self, f: F)
    where
        F: FnOnce(&mut Self),
    {
        self.w.indent();
        f(self);
        self.w.dedent();
    }

    // =======================================================================
    // Statements
    // =======================================================================

    fn write_stmt(&mut self, stmt: &Stmt, scope: TriviaScope) {
        self.write_leading_trivia_before_annotations(&stmt.leading_trivia, !stmt.annotations.is_empty(), scope);
        for annotation in &stmt.annotations {
            self.write_annotation(annotation);
            self.w.newline();
        }
        self.write_stmt_body(stmt);
    }

    fn write_leading_trivia_before_annotations(
        &mut self,
        trivia: &[Trivia],
        has_annotations: bool,
        scope: TriviaScope,
    ) {
        if !has_annotations {
            write_leading_trivia(trivia, scope, self.interner, &mut self.w);
            return;
        }
        let mut end = trivia.len();
        if trivia
            .last()
            .is_some_and(|item| matches!(item, Trivia::Newline(_)))
        {
            end -= 1;
        }
        write_leading_trivia(&trivia[..end], scope, self.interner, &mut self.w);
    }

    fn write_stmt_body(&mut self, stmt: &Stmt) {
        match &stmt.kind {
            StmtKind::Var(decl) => self.write_var_decl(decl),
            StmtKind::Func(func) => self.write_func_decl(func),
            StmtKind::Class(class) => self.write_class_decl(class),
            StmtKind::Interface(iface) => self.write_interface_decl(iface),
            StmtKind::TypeAlias(alias) => self.write_type_alias(alias),
            StmtKind::Enum(enum_decl) => self.write_enum_decl(enum_decl),
            StmtKind::Union(union) => self.write_union_decl(union),
            StmtKind::Schema(schema) => self.write_schema_decl(schema),
            StmtKind::Import(import) => self.write_import(import),
            StmtKind::Probandum(suite) => self.write_probandum(suite),
            StmtKind::Proba(case) => self.write_proba(case),
            StmtKind::Ex(ex) => self.write_ex_stmt(ex),
            StmtKind::Block(block) => {
                self.write_block(block);
                self.w.newline();
            }
            StmtKind::Expr(expr) => {
                self.write_expr(&expr.expr);
                self.w.newline();
            }
            StmtKind::IncDec(inc) => {
                self.write_expr(&inc.target);
                self.w.write(match inc.op {
                    IncDecOp::Inc => " ↑",
                    IncDecOp::Dec => " ↓",
                });
                self.w.newline();
            }
            StmtKind::Si(si) => self.write_si_stmt(si),
            StmtKind::Dum(dum) => self.write_dum_stmt(dum),
            StmtKind::Itera(itera) => self.write_itera_stmt(itera),
            StmtKind::Elige(elige) => self.write_elige_stmt(elige),
            StmtKind::Discerne(discerne) => self.write_discerne_stmt(discerne),
            StmtKind::Custodi(custodi) => self.write_custodi_stmt(custodi),
            StmtKind::Fac(fac) => self.write_fac_stmt(fac),
            StmtKind::Redde(redde) => {
                self.w.write("redde");
                if let Some(value) = &redde.value {
                    self.w.write(" ");
                    self.write_expr(value);
                }
                self.w.newline();
            }
            StmtKind::Rumpe(_) => self.w.writeln("rumpe"),
            StmtKind::Perge(_) => self.w.writeln("perge"),
            StmtKind::Iace(iace) => {
                self.w.write("iace ");
                self.write_expr(&iace.value);
                self.w.newline();
            }
            StmtKind::Mori(mori) => {
                self.w.write("mori ");
                self.write_expr(&mori.value);
                self.w.newline();
            }
            StmtKind::Tacet(_) => self.w.writeln("tacet"),
            StmtKind::Reddet(stmt) => {
                self.w.write("reddet ");
                self.write_expr(&stmt.future);
                self.w.newline();
            }
            StmtKind::Tacebit(stmt) => {
                self.w.write("tacebit ");
                self.write_expr(&stmt.future);
                self.w.newline();
            }
            StmtKind::Adfirma(adfirma) => {
                self.w.write("adfirma ");
                self.write_expr(&adfirma.cond);
                if let Some(message) = &adfirma.message {
                    self.w.write(" mori ");
                    self.write_expr(message);
                }
                self.w.newline();
            }
            StmtKind::Require(require) => {
                self.w.write("requirit ");
                self.write_expr(&require.cond);
                self.w.write(" iace ");
                self.write_expr(&require.error);
                self.w.newline();
            }
            StmtKind::Scribe(scribe) => self.write_scribe_stmt(scribe),
            StmtKind::Incipit(incipit) => self.write_incipit_stmt(incipit),
            StmtKind::Cura(cura) => self.write_cura_stmt(cura),
        }
    }

    // -----------------------------------------------------------------------
    // Blocks and chains
    // -----------------------------------------------------------------------

    fn write_block(&mut self, block: &BlockStmt) {
        if block.statements.is_empty() && block.end_trivia.is_empty() {
            self.w.write("{}");
            return;
        }
        self.w.writeln("{");
        self.indented(|s| {
            for stmt in &block.statements {
                s.write_stmt(stmt, TriviaScope::Inner);
            }
            for item in &block.end_trivia {
                if let Trivia::CommentLine(comment) = item {
                    super::trivia::write_comment_line(s.interner.resolve(comment.text), &mut s.w);
                }
            }
        });
        self.w.write("}");
    }

    /// Write an `ergo` arm. Kept compact when the head fits; expanded to a
    /// one-statement block when the head exceeds the soft width.
    fn write_compact_ergo(&mut self, stmt: &Stmt) {
        let start_col = self.w.column();
        let trial = PrettyWriter::first_line_trial_at(start_col);
        let saved = std::mem::replace(&mut self.w, trial);
        self.w.write(" ergo ");
        self.write_stmt(stmt, TriviaScope::Inner);
        let trial = std::mem::replace(&mut self.w, saved);
        if trial.column() <= SOFT_WIDTH {
            self.w.write(&trial.finish());
            self.w.newline();
        } else {
            self.w.write(" {");
            self.w.newline();
            self.indented(|s| s.write_stmt(stmt, TriviaScope::Inner));
            self.w.write("}");
            self.w.newline();
        }
    }

    /// Returns true when a body already ended the line (compact ergo form).
    fn write_if_body(&mut self, body: &IfBody) -> bool {
        match body {
            IfBody::Block(block) => {
                self.w.write(" ");
                self.write_block(block);
                false
            }
            IfBody::Ergo(stmt) => {
                self.write_compact_ergo(stmt);
                true
            }
        }
    }

    fn write_cape(&mut self, cape: &CapeClause) {
        self.w.write("cape ");
        self.write_ident(&cape.binding);
        self.w.write(" ");
        self.write_block(&cape.body);
    }

    fn write_cape_continuation(&mut self, cape: &CapeClause, prior_body_ended_line: bool) {
        if !prior_body_ended_line {
            self.w.newline();
        }
        self.write_cape(cape);
    }

    fn write_si_stmt(&mut self, si: &SiStmt) {
        self.w.write("si ");
        self.write_expr(&si.cond);
        let mut body_ended_line = self.write_if_body(&si.then);
        if let Some(catch) = &si.catch {
            self.write_cape_continuation(catch, body_ended_line);
            body_ended_line = false;
        }
        if let Some(else_) = &si.else_ {
            self.write_secus_clause(else_, body_ended_line);
        } else if !body_ended_line {
            self.w.newline();
        }
    }

    fn write_secus_clause(&mut self, secus: &SecusClause, prior_body_ended_line: bool) {
        if !prior_body_ended_line {
            self.w.newline();
        }
        let leading_trivia = match secus {
            SecusClause::Sin { leading_trivia, .. } => leading_trivia,
            SecusClause::Block { leading_trivia, .. } => leading_trivia,
            SecusClause::Stmt { leading_trivia, .. } => leading_trivia,
        };
        write_leading_trivia(leading_trivia, TriviaScope::Inner, self.interner, &mut self.w);
        match secus {
            SecusClause::Sin { stmt: si, .. } => {
                self.w.write("sin ");
                self.write_expr(&si.cond);
                let mut body_ended_line = self.write_if_body(&si.then);
                if let Some(catch) = &si.catch {
                    self.write_cape_continuation(catch, body_ended_line);
                    body_ended_line = false;
                }
                if let Some(else_) = &si.else_ {
                    self.write_secus_clause(else_, body_ended_line);
                } else if !body_ended_line {
                    self.w.newline();
                }
            }
            SecusClause::Block { body, catch, .. } => {
                self.w.write("secus ");
                self.write_block(body);
                if let Some(catch) = catch {
                    self.write_cape_continuation(catch, false);
                }
                self.w.newline();
            }
            SecusClause::Stmt { stmt, catch, .. } => {
                self.w.write("secus");
                self.write_compact_ergo(stmt);
                if let Some(catch) = catch {
                    self.write_cape_continuation(catch, true);
                }
            }
        }
    }

    fn write_dum_stmt(&mut self, dum: &DumStmt) {
        self.w.write("dum ");
        self.write_expr(&dum.cond);
        let mut body_ended_line = self.write_if_body(&dum.body);
        if let Some(catch) = &dum.catch {
            self.write_cape_continuation(catch, body_ended_line);
            body_ended_line = false;
        }
        if !body_ended_line {
            self.w.newline();
        }
    }

    fn write_itera_stmt(&mut self, itera: &IteraStmt) {
        self.w.write("itera ");
        self.w.write(match itera.mode {
            IteraMode::Ex => "ex ",
            IteraMode::De => "de ",
            IteraMode::Ab => "ab ",
        });
        self.write_expr(&itera.iterable);
        self.w.write(" ");
        self.w.write(match itera.mutability {
            AstMutability::Immutable => "fixum ",
            AstMutability::Mutable => "varia ",
        });
        self.write_ident(&itera.binding);
        let mut body_ended_line = self.write_if_body(&itera.body);
        if let Some(catch) = &itera.catch {
            self.write_cape_continuation(catch, body_ended_line);
            body_ended_line = false;
        }
        if !body_ended_line {
            self.w.newline();
        }
    }

    fn write_elige_stmt(&mut self, elige: &EligeStmt) {
        self.w.write("elige ");
        self.write_expr(&elige.expr);
        self.w.writeln(" {");
        self.indented(|s| {
            for case in &elige.cases {
                s.w.write("casu ");
                s.write_expr(&case.value);
                if !s.write_if_body(&case.body) {
                    s.w.newline();
                }
            }
            if let Some(default) = &elige.default {
                // `write_if_body` supplies the space before both body forms
                // (block and compact ergo), so the keyword is written bare.
                s.w.write("ceterum");
                if !s.write_if_body(&default.body) {
                    s.w.newline();
                }
            }
        });
        self.w.write("}");
        if let Some(catch) = &elige.catch {
            self.write_cape_continuation(catch, false);
        }
        self.w.newline();
    }

    fn write_discerne_arms(&mut self, discerne: &DiscerneStmt) {
        for arm in &discerne.arms {
            self.w.write("casu ");
            for (index, pattern) in arm.patterns.iter().enumerate() {
                if index > 0 {
                    self.w.write(", ");
                }
                self.write_pattern(pattern);
            }
            if !self.write_if_body(&arm.body) {
                self.w.newline();
            }
        }
        if let Some(default) = &discerne.default {
            // `write_if_body` supplies the space before both body forms
            // (block and compact ergo), so the keyword is written bare.
            self.w.write("ceterum");
            if !self.write_if_body(&default.body) {
                self.w.newline();
            }
        }
    }

    fn write_discerne_stmt(&mut self, discerne: &DiscerneStmt) {
        self.w.write("discerne ");
        if discerne.exhaustive {
            self.w.write("omnia ");
        }
        for (index, subject) in discerne.subjects.iter().enumerate() {
            if index > 0 {
                self.w.write(", ");
            }
            self.write_expr(subject);
        }
        self.w.writeln(" {");
        self.indented(|s| s.write_discerne_arms(discerne));
        self.w.writeln("}");
    }

    fn write_custodi_stmt(&mut self, custodi: &CustodiStmt) {
        self.w.writeln("custodi {");
        self.indented(|s| {
            for clause in &custodi.clauses {
                s.w.write("si ");
                s.write_expr(&clause.cond);
                if !s.write_if_body(&clause.body) {
                    s.w.newline();
                }
            }
        });
        self.w.writeln("}");
    }

    fn write_fac(&mut self, fac: &FacStmt) {
        self.w.write("fac ");
        self.write_block(&fac.body);
        if let Some(catch) = &fac.catch {
            self.write_cape_continuation(catch, false);
        }
        if let Some(cond) = &fac.while_ {
            self.w.write(" dum ");
            self.write_expr(cond);
        }
    }

    fn write_fac_stmt(&mut self, fac: &FacStmt) {
        self.write_fac(fac);
        self.w.newline();
    }

    fn write_incipit_stmt(&mut self, incipit: &IncipitStmt) {
        self.w
            .write(if incipit.is_async { "incipiet" } else { "incipit" });
        if let Some(args) = &incipit.args {
            self.w.write(" argumenta ");
            self.write_ident(args);
        }
        if let Some(exitus) = &incipit.exitus {
            self.w.write(" exitus ");
            self.write_expr(exitus);
        }
        if !self.write_if_body(&incipit.body) {
            self.w.newline();
        }
    }

    fn write_cura_stmt(&mut self, cura: &CuraStmt) {
        self.w.write("cura ");
        self.w.write(match cura.kind {
            CuraKind::Arena => "\"arena\" ",
            CuraKind::Page => "\"page\" ",
        });
        self.w.write(match cura.mutability {
            AstMutability::Immutable => "fixum ",
            AstMutability::Mutable => "varia ",
        });
        self.write_type_expr(&cura.ty);
        self.w.write(" ");
        self.write_ident(&cura.binding);
        self.w.write(" ");
        self.write_block(&cura.body);
        if let Some(catch) = &cura.catch {
            self.write_cape_continuation(catch, false);
        }
        self.w.newline();
    }

    // -----------------------------------------------------------------------
    // Declarations
    // -----------------------------------------------------------------------

    fn write_var_decl(&mut self, decl: &VarDecl) {
        if decl.is_await {
            self.w.write(match decl.mutability {
                AstMutability::Immutable => "figendum",
                AstMutability::Mutable => "variandum",
            });
        } else {
            self.w.write(match decl.mutability {
                AstMutability::Immutable => "fixum",
                AstMutability::Mutable => "varia",
            });
        }
        if let Some(ty) = &decl.ty {
            self.w.write(" ");
            self.write_type_expr(ty);
        }
        self.w.write(" ");
        self.write_binding_pattern(&decl.binding);
        if let Some(init) = &decl.init {
            if let ExprKind::Assign(assign) = &init.kind {
                if matches!(assign.op, AssignOp::ConversioAssign) {
                    self.w.write(" ↤ ");
                    self.write_expr_prec(&assign.value, 1);
                    if let Some(recovery) = &assign.recovery {
                        self.w.write(" ⇥ ");
                        self.write_expr_prec(recovery, 12);
                    }
                } else {
                    self.w.write(" ← ");
                    self.write_expr(init);
                }
            } else {
                self.w.write(" ← ");
                self.write_expr(init);
            }
        }
        self.w.newline();
    }

    fn write_params(&mut self, params: &[Param]) {
        if params.is_empty() {
            self.w.write("()");
            return;
        }
        let multiline = self.source_multiline(self.span_between(params[0].span, params[params.len() - 1].span));
        if multiline {
            self.write_params_expanded(params);
            return;
        }
        self.write_list_layout(
            |s| {
                s.w.write("(");
                for (index, param) in params.iter().enumerate() {
                    if index > 0 {
                        s.w.write(", ");
                    }
                    s.write_param(param);
                }
                s.w.write(")");
            },
            |s| s.write_params_expanded(params),
        );
    }

    fn write_params_expanded(&mut self, params: &[Param]) {
        self.w.write("(");
        self.indented(|s| {
            for (index, param) in params.iter().enumerate() {
                s.w.newline();
                s.write_param(param);
                if index + 1 < params.len() {
                    s.w.write(",");
                }
            }
        });
        self.w.newline();
        self.w.write(")");
    }

    fn write_func_decl(&mut self, func: &FuncDecl) {
        self.write_leading_trivia_before_annotations(
            &func.leading_trivia,
            !func.annotations.is_empty(),
            TriviaScope::Inner,
        );
        for annotation in &func.annotations {
            self.write_annotation(annotation);
            self.w.newline();
        }
        self.w.write("functio ");
        self.write_ident(&func.name);
        self.write_generic_params(&func.type_params);
        self.write_params(&func.params);
        for modifier in &func.modifiers {
            self.w.write(" ");
            self.write_func_modifier(modifier);
        }
        self.write_callable_posture(func.posture);
        if let Some(ret) = &func.ret {
            self.w.write(" → ");
            self.write_type_expr(ret);
        }
        if let Some(err) = &func.err {
            self.w.write(" ⇥ ");
            self.write_type_expr(err);
        }
        if let Some(body) = &func.body {
            self.w.write(" ");
            self.write_block(body);
        }
        self.w.newline();
    }

    fn write_class_members(&mut self, class: &ClassDecl) {
        for member in &class.members {
            self.write_leading_trivia_before_annotations(
                &member.leading_trivia,
                !member.annotations.is_empty(),
                TriviaScope::Inner,
            );
            for annotation in &member.annotations {
                self.write_annotation(annotation);
                self.w.newline();
            }
            match &member.kind {
                ClassMemberKind::Field(field) => self.write_field_decl(field),
                ClassMemberKind::Method(func) => self.write_func_decl(func),
            }
        }
    }

    fn write_class_decl(&mut self, class: &ClassDecl) {
        if class.is_abstract {
            self.w.write("abstractus ");
        }
        self.w.write("genus ");
        self.write_ident(&class.name);
        self.write_generic_params(&class.type_params);
        if let Some(extends) = &class.extends {
            self.w.write(" sub ");
            self.write_ident(extends);
        }
        if !class.implements.is_empty() {
            self.w.write(" implet ");
            for (index, iface) in class.implements.iter().enumerate() {
                if index > 0 {
                    self.w.write(", ");
                }
                self.write_ident(iface);
            }
        }
        self.w.writeln(" {");
        self.indented(|s| s.write_class_members(class));
        self.w.writeln("}");
    }

    fn write_field_decl(&mut self, field: &FieldDecl) {
        if field.is_static {
            self.w.write("generis ");
        }
        if field.is_bound {
            self.w.write("nexum ");
        }
        self.write_type_expr(&field.ty);
        self.w.write(" ");
        self.write_ident(&field.name);
        if field.sponte {
            self.w.write(" sponte");
        }
        if let Some(init) = &field.init {
            self.w.write(" = ");
            self.write_expr(init);
        }
        self.w.newline();
    }

    fn write_interface_methods(&mut self, iface: &InterfaceDecl) {
        for method in &iface.methods {
            self.write_leading_trivia_before_annotations(
                &method.leading_trivia,
                !method.annotations.is_empty(),
                TriviaScope::Inner,
            );
            for annotation in &method.annotations {
                self.write_annotation(annotation);
                self.w.newline();
            }
            self.w.write("functio ");
            self.write_ident(&method.name);
            self.write_params(&method.params);
            for modifier in &method.modifiers {
                self.w.write(" ");
                self.write_func_modifier(modifier);
            }
            self.write_callable_posture(method.posture);
            if let Some(ret) = &method.ret {
                self.w.write(" → ");
                self.write_type_expr(ret);
            }
            if let Some(err) = &method.err {
                self.w.write(" ⇥ ");
                self.write_type_expr(err);
            }
            self.w.newline();
        }
    }

    fn write_callable_posture(&mut self, posture: CallablePosture) {
        match posture {
            CallablePosture::SyncFinite => {}
            CallablePosture::AsyncFinite => self.w.write(" fiet"),
            CallablePosture::SyncStream => self.w.write(" fiunt"),
            CallablePosture::AsyncStream => self.w.write(" fient"),
        }
    }

    fn write_interface_decl(&mut self, iface: &InterfaceDecl) {
        self.w.write("implendum ");
        self.write_ident(&iface.name);
        self.write_generic_params(&iface.type_params);
        self.w.writeln(" {");
        self.indented(|s| s.write_interface_methods(iface));
        self.w.writeln("}");
    }

    fn write_type_alias(&mut self, alias: &TypeAliasDecl) {
        self.w.write("typus ");
        self.write_ident(&alias.name);
        self.write_generic_params(&alias.type_params);
        self.w.write(" = ");
        self.write_type_expr(&alias.ty);
        self.w.newline();
    }

    fn write_enum_decl(&mut self, enum_decl: &EnumDecl) {
        self.w.write("ordo ");
        self.write_ident(&enum_decl.name);
        self.w.writeln(" {");
        self.indented(|s| {
            for member in &enum_decl.members {
                s.write_ident(&member.name);
                if let Some(value) = &member.value {
                    s.w.write(" = ");
                    match value {
                        EnumValue::Integer(n) => s.w.write(&n.to_string()),
                        EnumValue::String(sym) => s.write_quoted_symbol(*sym),
                    }
                }
                s.w.newline();
            }
        });
        self.w.writeln("}");
    }

    fn write_schema_decl(&mut self, schema: &SchemaDecl) {
        self.w.write("schema ");
        self.write_ident(&schema.name);
        self.w.writeln(" {");
        self.indented(|s| {
            for column in &schema.columns {
                s.write_schema_column(column);
            }
            for item in &schema.end_trivia {
                if let Trivia::CommentLine(comment) = item {
                    super::trivia::write_comment_line(s.interner.resolve(comment.text), &mut s.w);
                }
            }
        });
        self.w.writeln("}");
    }

    fn write_schema_column(&mut self, column: &SchemaColumn) {
        self.w.write("columna ");
        self.write_type_expr(&column.ty);
        self.w.write(" ");
        self.write_ident(&column.name);
        if let Some(alias) = &column.source_alias {
            self.w.write(" : ");
            self.write_ident(alias);
        }
        self.w.newline();
    }

    fn write_union_decl(&mut self, union: &UnionDecl) {
        self.w.write("discretio ");
        self.write_ident(&union.name);
        self.write_generic_params(&union.type_params);
        self.w.writeln(" {");
        self.indented(|s| {
            for (index, variant) in union.variants.iter().enumerate() {
                s.write_ident(&variant.name);
                if !variant.fields.is_empty() {
                    s.w.write(" {");
                    for (field_index, field) in variant.fields.iter().enumerate() {
                        if field_index > 0 {
                            s.w.write(", ");
                        } else {
                            s.w.write(" ");
                        }
                        s.write_type_expr(&field.ty);
                        s.w.write(" ");
                        s.write_ident(&field.name);
                    }
                    if !variant.fields.is_empty() {
                        s.w.write(" ");
                    }
                    s.w.write("}");
                }
                if index + 1 < union.variants.len() {
                    s.w.write(",");
                }
                s.w.newline();
            }
        });
        self.w.writeln("}");
    }

    fn write_import(&mut self, import: &ImportDecl) {
        self.w.write("importa ex ");
        self.write_quoted_symbol(import.path);
        if import.visibility == Visibility::Publica {
            self.w.write(" publica");
        }
        match &import.kind {
            ImportKind::Named { name, alias, inferred } => {
                if !inferred {
                    self.w.write(" ");
                    self.write_ident(name);
                    if let Some(alias) = alias {
                        self.w.write(" ut ");
                        self.write_ident(alias);
                    }
                }
            }
            ImportKind::Wildcard { alias } => {
                self.w.write(" * ut ");
                self.write_ident(alias);
            }
        }
        self.w.newline();
    }

    fn write_probandum(&mut self, suite: &ProbandumDecl) {
        self.w.write("probandum ");
        self.write_quoted_symbol(suite.name);
        for modifier in &suite.modifiers {
            self.w.write(" ");
            self.write_proba_modifier(modifier);
        }
        self.w.writeln(" {");
        self.indented(|s| {
            for setup in &suite.body.setup {
                s.write_praepara(setup);
            }
            for test in &suite.body.tests {
                s.write_proba(test);
            }
            for nested in &suite.body.nested {
                s.write_probandum(nested);
            }
        });
        self.w.writeln("}");
    }

    fn write_praepara(&mut self, block: &PraeparaBlock) {
        self.w.write(match block.kind {
            PraeparaKind::Praepara => "praepara",
            PraeparaKind::Praeparabit => "praeparabit",
            PraeparaKind::Postpara => "postpara",
            PraeparaKind::Postparabit => "postparabit",
        });
        if block.all {
            self.w.write(" omnia");
        }
        self.w.write(" ");
        self.write_block(&block.body);
        self.w.newline();
    }

    fn write_proba(&mut self, case: &ProbaCase) {
        self.w.write("proba ");
        self.write_quoted_symbol(case.name);
        for modifier in &case.modifiers {
            self.w.write(" ");
            self.write_proba_modifier(modifier);
        }
        self.w.write(" ");
        self.write_block(&case.body);
        self.w.newline();
    }

    fn write_proba_modifier(&mut self, modifier: &ProbaModifier) {
        match modifier {
            ProbaModifier::Omitte(reason) => {
                self.write_quoted_proba_token("omitte ", *reason);
            }
            ProbaModifier::Futurum(reason) => {
                self.write_quoted_proba_token("futurum ", *reason);
            }
            ProbaModifier::Solum => self.w.write("solum"),
            ProbaModifier::Tag(tag) => {
                self.write_quoted_proba_token("tag ", *tag);
            }
            ProbaModifier::Temporis(n) => {
                self.write_numeric_proba_token("temporis ", *n);
            }
            ProbaModifier::Metior => self.w.write("metior"),
            ProbaModifier::Repete(n) => {
                self.write_numeric_proba_token("repete ", *n);
            }
            ProbaModifier::Fragilis(n) => {
                self.write_numeric_proba_token("fragilis ", *n);
            }
            ProbaModifier::SolumIn(env) => {
                self.write_quoted_proba_token("solum_in ", *env);
            }
        }
    }

    fn write_quoted_proba_token(&mut self, prefix: &str, symbol: Symbol) {
        self.w.write(prefix);
        self.write_quoted_symbol(symbol);
    }

    fn write_numeric_proba_token(&mut self, prefix: &str, value: i64) {
        self.w.write(prefix);
        self.w.write(&value.to_string());
    }

    fn write_ex_stmt(&mut self, ex: &ExStmt) {
        self.w.write("ex ");
        self.write_expr(&ex.source);
        self.w.write(" ");
        self.w.write(match ex.mutability {
            AstMutability::Immutable => "fixum",
            AstMutability::Mutable => "varia",
        });
        for (index, field) in ex.fields.iter().enumerate() {
            if index > 0 {
                self.w.write(", ");
            } else {
                self.w.write(" ");
            }
            self.write_ident(&field.name);
            if let Some(alias) = &field.alias {
                self.w.write(" ut ");
                self.write_ident(alias);
            }
        }
        if let Some(rest) = &ex.rest {
            if ex.fields.is_empty() {
                self.w.write(" ");
            } else {
                self.w.write(", ");
            }
            self.w.write("ceteri ");
            self.write_ident(rest);
        }
        self.w.newline();
    }

    fn write_scribe_stmt(&mut self, scribe: &ScribeStmt) {
        self.w.write(match scribe.kind {
            ScribeKind::Scribe => "scribe",
            ScribeKind::Vide => "vide",
            ScribeKind::Mone => "mone",
            ScribeKind::Nota => "nota",
        });
        for (index, arg) in scribe.args.iter().enumerate() {
            if index == 0 {
                self.w.write(" ");
            } else {
                self.w.write(", ");
            }
            self.write_expr(arg);
        }
        self.w.newline();
    }

    fn write_param(&mut self, param: &Param) {
        self.w.write(match param.mode {
            AstParamMode::Owned => "",
            AstParamMode::De => "de ",
            AstParamMode::In => "in ",
            AstParamMode::Ex => "ex ",
        });
        if param.rest {
            self.w.write("ceteri ");
        }
        self.write_type_expr(&param.ty);
        self.w.write(" ");
        self.write_ident(&param.name);
        if param.sponte {
            self.w.write(" sponte");
        }
        if let Some(alias) = &param.alias {
            self.w.write(" ut ");
            self.write_ident(alias);
        }
        if let Some(default) = &param.default {
            self.w.write(" vel ");
            self.write_expr(default);
        }
    }

    fn write_func_modifier(&mut self, modifier: &FuncModifier) {
        match modifier {
            FuncModifier::Argumenta(name) => {
                self.w.write("argumenta ");
                self.write_ident(name);
            }
            FuncModifier::Curata { required, alias } => {
                self.w.write("curata ");
                self.write_ident(required);
                if let Some(alias) = alias {
                    self.w.write(" ut ");
                    self.write_ident(alias);
                }
            }
            FuncModifier::Errata(name) => {
                self.w.write("errata ");
                self.write_ident(name);
            }
            FuncModifier::Exitus(value) => {
                self.w.write("exitus ");
                match value {
                    ExitusValue::Name(name) => self.write_ident(name),
                    ExitusValue::Number(n) => self.w.write(&n.to_string()),
                }
            }
            FuncModifier::Iacit => self.w.write("iacit"),
            FuncModifier::Immutata => self.w.write("immutata"),
            FuncModifier::Optiones(name) => {
                self.w.write("optiones ");
                self.write_ident(name);
            }
        }
    }

    fn write_generic_params(&mut self, params: &[TypeParam]) {
        if params.is_empty() {
            return;
        }
        self.w.write("<");
        for (index, param) in params.iter().enumerate() {
            if index > 0 {
                self.w.write(", ");
            }
            match param.kind {
                GenericParamKind::Typus => self.write_ident(&param.name),
                GenericParamKind::Magnitudo => {
                    self.w.write("magnitudo ");
                    self.write_ident(&param.name);
                }
            }
        }
        self.w.write(">");
    }

    // -----------------------------------------------------------------------
    // Annotations
    // -----------------------------------------------------------------------

    fn write_annotation(&mut self, annotation: &Annotation) {
        self.w.write("@ ");
        match &annotation.kind {
            AnnotationKind::Publica => self.w.write("publica"),
            AnnotationKind::Protecta => self.w.write("protecta"),
            AnnotationKind::Privata => self.w.write("privata"),
            AnnotationKind::Interna => self.w.write("interna"),
            AnnotationKind::Futura => self.w.write("futura"),
            AnnotationKind::Cursor => self.w.write("cursor"),
            AnnotationKind::Tag => self.w.write("tag"),
            AnnotationKind::Solum => self.w.write("solum"),
            AnnotationKind::Omitte => self.w.write("omitte"),
            AnnotationKind::Metior => self.w.write("metior"),
            AnnotationKind::Cli(cli) => {
                self.w.write("cli ");
                self.write_quoted_symbol(cli.name);
            }
            AnnotationKind::Imperium(imperium) => {
                self.w.write("imperium ");
                self.write_quoted_symbol(imperium.name);
            }
            AnnotationKind::Optio(optio) => self.write_optio_annotation(optio),
            AnnotationKind::Operandus(operandus) => self.write_operandus_annotation(operandus),
            AnnotationKind::RadixTypusDomain(domain) => {
                self.w.write("radix typus ");
                self.write_ident(&domain.param);
                self.w.write(" in");
                for ty in &domain.allowed {
                    self.w.write(" ");
                    self.write_type_expr(ty);
                }
            }
            AnnotationKind::RadixLane(lane) => {
                self.w.write("radix lane ");
                self.write_quoted_symbol(lane.lane);
            }
            AnnotationKind::Nucleum(nucleum) => {
                self.w.write("nucleum");
                if nucleum.fragment {
                    self.w.write(" fragment");
                }
            }
            AnnotationKind::Braced(braced) => {
                self.write_ident(&braced.name);
                self.w.write(" {");
                for (index, field) in braced.fields.iter().enumerate() {
                    if index > 0 {
                        self.w.write(", ");
                    }
                    self.write_ident(&field.name);
                    self.w.write(" = ");
                    match &field.value {
                        AnnotationFieldValue::Expr(expr) => self.write_expr(expr),
                        AnnotationFieldValue::Type(ty) => self.write_type_expr(ty),
                    }
                }
                self.w.write(" }");
            }
            AnnotationKind::Statement(stmt) => {
                self.write_ident(&stmt.name);
                for token in &stmt.args {
                    self.w.write(" ");
                    self.write_token(token);
                }
            }
        }
    }

    fn write_optio_annotation(&mut self, optio: &OptioAnnotation) {
        self.w.write("optio ");
        self.write_ident(&optio.binding);
        if let Some(ty) = &optio.ty {
            self.w.write(" typus ");
            self.write_type_expr(ty);
        }
        if let Some(short) = optio.short {
            self.w.write(" brevis ");
            self.write_quoted_symbol(short);
        }
        if let Some(long) = optio.long {
            self.w.write(" longum ");
            self.write_quoted_symbol(long);
        }
        if let Some(description) = optio.description {
            self.w.write(" descriptio ");
            self.write_quoted_symbol(description);
        }
        if optio.global {
            self.w.write(" ubique");
        }
        if let Some(default) = &optio.default {
            self.w.write(" vel ");
            self.write_expr(default);
        }
    }

    fn write_operandus_annotation(&mut self, operandus: &OperandusAnnotation) {
        self.w.write("operandus");
        if operandus.rest {
            self.w.write(" ceteri");
        }
        self.w.write(" ");
        self.write_type_expr(&operandus.ty);
        self.w.write(" ");
        self.write_ident(&operandus.binding);
        if let Some(description) = operandus.description {
            self.w.write(" descriptio ");
            self.write_quoted_symbol(description);
        }
        if operandus.global {
            self.w.write(" ubique");
        }
        if let Some(default) = &operandus.default {
            self.w.write(" vel ");
            self.write_expr(default);
        }
    }

    // -----------------------------------------------------------------------
    // Patterns
    // -----------------------------------------------------------------------

    fn write_binding_pattern(&mut self, pattern: &BindingPattern) {
        match pattern {
            BindingPattern::Ident(ident) => self.write_ident(ident),
            BindingPattern::Wildcard(_) => self.w.write("_"),
            BindingPattern::Array { elements, rest, .. } => {
                self.w.write("[");
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        self.w.write(", ");
                    }
                    self.write_binding_pattern(element);
                }
                if let Some(rest) = rest {
                    if !elements.is_empty() {
                        self.w.write(", ");
                    }
                    self.w.write("ceteri ");
                    self.write_ident(rest);
                }
                self.w.write("]");
            }
            BindingPattern::Object { fields, rest, .. } => {
                self.w.write("{");
                for (index, field) in fields.iter().enumerate() {
                    if index > 0 {
                        self.w.write(", ");
                    }
                    self.write_ident(&field.name);
                    if let Some(alias) = &field.alias {
                        self.w.write(" ut ");
                        self.write_ident(alias);
                    }
                }
                if let Some(rest) = rest {
                    if !fields.is_empty() {
                        self.w.write(", ");
                    }
                    self.w.write("ceteri ");
                    self.write_ident(rest);
                }
                self.w.write("}");
            }
        }
    }

    fn write_pattern(&mut self, pattern: &Pattern) {
        match pattern {
            Pattern::Wildcard(_) => self.w.write("_"),
            Pattern::Ident(ident, bind) => {
                self.write_ident(ident);
                if let Some(bind) = bind {
                    self.write_pattern_bind(bind);
                }
            }
            Pattern::Literal(lit, _) => self.write_literal(lit),
            Pattern::Path(path) => {
                for (index, segment) in path.segments.iter().enumerate() {
                    if index > 0 {
                        self.w.write(".");
                    }
                    self.write_ident(segment);
                }
                if let Some(bind) = &path.bind {
                    self.write_pattern_bind(bind);
                }
            }
        }
    }

    fn write_pattern_bind(&mut self, bind: &PatternBind) {
        match bind {
            PatternBind::Alias(alias) => {
                self.w.write(" ut ");
                self.write_ident(alias);
            }
            PatternBind::Bindings { mutability, bindings } => {
                self.w.write(" ");
                self.w.write(match mutability {
                    AstMutability::Immutable => "fixum",
                    AstMutability::Mutable => "varia",
                });
                for (index, field) in bindings.iter().enumerate() {
                    if index > 0 {
                        self.w.write(", ");
                    } else {
                        self.w.write(" ");
                    }
                    self.write_ident(&field.name);
                    if let Some(alias) = &field.alias {
                        self.w.write(" ut ");
                        self.write_ident(alias);
                    }
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Types
    // -----------------------------------------------------------------------

    fn write_type_expr(&mut self, ty: &TypeExpr) {
        if let Some(mode) = ty.mode {
            self.w.write(match mode {
                TypeMode::De => "de ",
                TypeMode::In => "in ",
            });
        }
        match &ty.kind {
            TypeExprKind::Infer => self.w.write("_"),
            TypeExprKind::UnionHole => self.w.write("∪"),
            TypeExprKind::Named(name, params) => {
                self.write_ident(name);
                if !params.is_empty() {
                    self.w.write("<");
                    for (index, param) in params.iter().enumerate() {
                        if index > 0 {
                            self.w.write(", ");
                        }
                        self.write_type_expr(param);
                    }
                    self.w.write(">");
                }
            }
            TypeExprKind::Qualified(path, params) => {
                for (index, segment) in path.iter().enumerate() {
                    if index > 0 {
                        self.w.write(".");
                    }
                    self.write_ident(segment);
                }
                if !params.is_empty() {
                    self.w.write("<");
                    for (index, param) in params.iter().enumerate() {
                        if index > 0 {
                            self.w.write(", ");
                        }
                        self.write_type_expr(param);
                    }
                    self.w.write(">");
                }
            }
            TypeExprKind::Tensor(inner, figura) => {
                self.w.write("tensor<");
                self.write_type_expr(inner);
                self.w.write(", ");
                self.write_figura(figura);
                self.w.write(">");
            }
            TypeExprKind::Vector(inner, figura) => {
                self.w.write("vector<");
                self.write_type_expr(inner);
                self.w.write(", ");
                self.write_figura(figura);
                self.w.write(">");
            }
            TypeExprKind::Matrix(inner, figura) => {
                self.w.write("matrix<");
                self.write_type_expr(inner);
                self.w.write(", ");
                self.write_figura(figura);
                self.w.write(">");
            }
            TypeExprKind::Sparsa(inner, figura) => {
                self.w.write("sparsa<");
                self.write_type_expr(inner);
                self.w.write(", ");
                self.write_figura(figura);
                self.w.write(">");
            }
            TypeExprKind::Atomic(inner) => {
                self.w.write("atomic<");
                self.write_type_expr(inner);
                self.w.write(">");
            }
            TypeExprKind::PromissumFailable(success, alternate) => {
                self.w.write("promissum<");
                self.write_type_expr(success);
                self.w.write(" ⇥ ");
                self.write_type_expr(alternate);
                self.w.write(">");
            }
            TypeExprKind::Func(func_ty) => {
                self.w.write("(");
                for (index, param) in func_ty.params.iter().enumerate() {
                    if index > 0 {
                        self.w.write(", ");
                    }
                    self.write_type_expr(param);
                }
                self.w.write(") → ");
                self.write_type_expr(&func_ty.ret);
                if let Some(err) = &func_ty.err {
                    self.w.write(" ⇥ ");
                    self.write_type_expr(err);
                }
            }
            TypeExprKind::Union(members) => {
                for (index, member) in members.iter().enumerate() {
                    if index > 0 {
                        self.w.write(" ∪ ");
                    }
                    self.write_type_expr(member);
                }
            }
        }
        if ty.nullable {
            self.w.write(" ∪ nihil");
        }
    }

    fn write_figura(&mut self, figura: &FiguraExpr) {
        match figura {
            FiguraExpr::Infer => self.w.write("_"),
            FiguraExpr::Literal(n) => self.w.write(&n.to_string()),
            FiguraExpr::Var(ident) => self.write_ident(ident),
            FiguraExpr::Tuple(items) => {
                self.w.write("[");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        self.w.write(", ");
                    }
                    self.write_figura(item);
                }
                self.w.write("]");
            }
        }
    }

    // -----------------------------------------------------------------------
    // Expressions
    // -----------------------------------------------------------------------

    fn write_expr(&mut self, expr: &Expr) {
        self.write_expr_prec(expr, 0);
    }

    fn write_expr_prec(&mut self, expr: &Expr, parent_prec: u8) {
        let expr_prec = Self::expr_precedence(expr);
        let needs_parens = expr_prec <= parent_prec && parent_prec != 0;
        if needs_parens {
            self.w.write("(");
        }

        match &expr.kind {
            ExprKind::Ident(ident) => self.write_ident(ident),
            ExprKind::Literal(lit) => self.write_literal(lit),
            ExprKind::Vacua(_) => self.w.write("vacua"),
            ExprKind::Binary(binary) => {
                let op_prec = Self::binop_precedence(binary.op);
                if matches!(binary.op, BinOp::Is | BinOp::IsNot) {
                    self.write_expr_prec(&binary.lhs, op_prec);
                    if matches!(binary.op, BinOp::IsNot) {
                        self.w.write(" non");
                    }
                    self.w.write(" est ");
                    self.write_expr_prec(&binary.rhs, op_prec);
                } else {
                    self.write_expr_prec(&binary.lhs, op_prec);
                    self.w.write(" ");
                    self.w.write(Self::binop_to_faber(binary.op));
                    self.w.write(" ");
                    self.write_expr_prec(&binary.rhs, op_prec);
                }
            }
            ExprKind::Unary(unary) => {
                self.w.write(Self::unop_to_faber(unary.op));
                self.write_expr_prec(&unary.operand, 12);
            }
            ExprKind::Ternary(ternary) => match ternary.style {
                TernaryStyle::QuestionColon => {
                    self.write_expr_prec(&ternary.cond, 2);
                    self.w.write(" ? ");
                    self.write_expr_prec(&ternary.then, 2);
                    self.w.write(" : ");
                    self.write_expr_prec(&ternary.else_, 2);
                }
                TernaryStyle::SicSecus => {
                    self.write_expr_prec(&ternary.cond, 2);
                    self.w.write(" sic ");
                    self.write_expr_prec(&ternary.then, 2);
                    self.w.write(" secus ");
                    self.write_expr_prec(&ternary.else_, 2);
                }
            },
            ExprKind::Call(call) => {
                self.write_expr_prec(&call.callee, 12);
                self.write_call_args(expr.span, &call.args);
            }
            ExprKind::Member(member) => {
                self.write_expr_prec(&member.object, 12);
                self.w.write(".");
                self.write_ident(&member.member);
            }
            ExprKind::Index(index) => {
                self.write_expr_prec(&index.object, 13);
                self.w.write("[");
                self.write_expr(&index.index);
                self.w.write("]");
            }
            ExprKind::Gradient(gradient) => {
                // Canonical spaced form: one space on each side of `∇` when a
                // selector list follows. The space keeps `∇` reading as an
                // operator and the bracketed terms as its selector operand,
                // not as an index call on the glyph.
                self.write_expr_prec(&gradient.call, 12);
                self.w.write(" ∇");
                if !gradient.selectors.is_empty() {
                    self.w.write(" [");
                    for (index, selector) in gradient.selectors.iter().enumerate() {
                        if index > 0 {
                            self.w.write(", ");
                        }
                        self.write_expr(selector);
                    }
                    self.w.write("]");
                }
            }
            ExprKind::OptionalChain(chain) => {
                self.write_expr_prec(&chain.object, 13);
                match &chain.chain {
                    OptionalChainKind::Member(member) => {
                        self.w.write("?.");
                        self.write_ident(member);
                    }
                    OptionalChainKind::Index(index) => {
                        self.w.write("?[");
                        self.write_expr(index);
                        self.w.write("]");
                    }
                    OptionalChainKind::Call(args) => {
                        self.w.write("?(");
                        self.write_args_only(expr.span, args, ")");
                    }
                }
            }
            ExprKind::NonNull(nonnull) => {
                self.write_expr_prec(&nonnull.object, 13);
                match &nonnull.chain {
                    NonNullKind::Member(member) => {
                        self.w.write("!.");
                        self.write_ident(member);
                    }
                    NonNullKind::Index(index) => {
                        self.w.write("![");
                        self.write_expr(index);
                        self.w.write("]");
                    }
                    NonNullKind::Call(args) => {
                        self.w.write("!(");
                        self.write_args_only(expr.span, args, ")");
                    }
                }
            }
            ExprKind::Assign(assign) => {
                self.write_expr_prec(&assign.target, 1);
                match assign.op {
                    AssignOp::Assign => self.w.write(" ← "),
                    AssignOp::ConversioAssign => self.w.write(" ↤ "),
                }
                self.write_expr_prec(&assign.value, 1);
                if let Some(recovery) = &assign.recovery {
                    self.w.write(" ⇥ ");
                    self.write_expr_prec(recovery, 12);
                }
            }
            ExprKind::Verte(verte) => {
                if !self.write_typed_object_constructor(verte) {
                    self.write_expr_prec(&verte.expr, 12);
                    self.w.write(" ∷ ");
                    self.write_type_expr(&verte.ty);
                }
            }
            ExprKind::Finge(finge) => {
                self.w.write("finge ");
                self.write_ident_path(&finge.variant);
                if !finge.fields.is_empty() {
                    let fields = &finge.fields;
                    self.write_brace_list(expr.span, " ", fields.len(), |s, index| {
                        let field = &fields[index];
                        s.write_ident(&field.name);
                        s.w.write(" = ");
                        s.write_expr(&field.value);
                    });
                }
                if let Some(cast) = &finge.cast {
                    self.w.write(" ∷ ");
                    self.write_type_expr(cast);
                }
            }
            ExprKind::Clausura(clausura) => self.write_clausura(clausura),
            ExprKind::Cede(cede) => {
                self.w.write("cede ");
                self.write_expr(&cede.expr);
            }
            ExprKind::Array(array) => {
                self.write_array_elements(expr.span, &array.elements);
            }
            ExprKind::Object(object) => {
                let fields = &object.fields;
                self.write_brace_list(expr.span, "", fields.len(), |s, index| {
                    s.write_object_field(&fields[index]);
                });
            }
            ExprKind::Intervallum(range) => {
                self.write_expr_prec(&range.start, 6);
                self.w.write(match range.kind {
                    RangeKind::Exclusive => "‥",
                    RangeKind::Inclusive => "…",
                });
                self.write_expr_prec(&range.end, 6);
                if let Some(step) = &range.step {
                    self.w.write(" per ");
                    self.write_expr(step);
                }
            }
            ExprKind::Conversio(conversio) => self.write_conversio(conversio),
            ExprKind::Scriptum(scriptum) => {
                self.w.write("scriptum ");
                self.write_quoted_symbol(scriptum.template);
                for arg in &scriptum.args {
                    self.w.write(", ");
                    self.write_expr(arg);
                }
            }
            ExprKind::Ad(ad) => {
                self.w.write("ad ");
                self.write_literal(&Literal::Ascii(ad.path));
                if let Some(opener) = &ad.opener {
                    self.w.write(" (");
                    self.write_expr(opener);
                    self.w.write(")");
                }
            }
            ExprKind::Lege(lege) => {
                self.w.write("lege");
                if lege.line {
                    self.w.write(" lineam");
                }
            }
            ExprKind::Praefixum(praefixum) => {
                self.w.write("praefixum(");
                match &praefixum.body {
                    PraefixumBody::Block(block) => self.write_block(block),
                    PraefixumBody::Expr(expr) => self.write_expr(expr),
                }
                self.w.write(")");
            }
            ExprKind::Ego(_) => self.w.write("ego"),
            ExprKind::Paren(inner) => {
                self.w.write("(");
                self.write_expr(inner);
                self.w.write(")");
            }
            ExprKind::TypeCheck(type_check) => {
                self.write_expr_prec(&type_check.expr, 5);
                if !type_check.positive {
                    self.w.write(" non");
                }
                self.w.write(" est ");
                self.write_type_expr(&type_check.ty);
            }
            ExprKind::Iuncta(iuncta) => {
                self.w.write("iuncta<");
                for (i, ty) in iuncta.type_args.iter().enumerate() {
                    if i > 0 {
                        self.w.write(", ");
                    }
                    self.write_type_expr(ty);
                }
                self.w.write("> ");
                let elements = &iuncta.elements;
                self.write_bracket_list(expr.span, elements.len(), |s, index| {
                    s.write_expr(&elements[index]);
                });
            }
        }

        if needs_parens {
            self.w.write(")");
        }
    }

    fn write_clausura(&mut self, clausura: &ClausuraExpr) {
        if clausura.params.len() == 1 {
            let param = &clausura.params[0];
            self.write_type_expr(&param.ty);
            self.w.write(" ");
            self.write_ident(&param.name);
        } else {
            self.w.write("(");
            for (index, param) in clausura.params.iter().enumerate() {
                if index > 0 {
                    self.w.write(", ");
                }
                self.write_type_expr(&param.ty);
                self.w.write(" ");
                self.write_ident(&param.name);
            }
            self.w.write(")");
        }
        if let Some(ret) = &clausura.ret {
            self.w.write(" → ");
            self.write_type_expr(ret);
        }
        if let Some(err) = &clausura.err {
            self.w.write(" ⇥ ");
            self.write_type_expr(err);
        }
        self.w.write(" ∴ ");
        match &clausura.body {
            ClausuraBody::Expr(expr) => self.write_expr(expr),
            ClausuraBody::Block(block) => self.write_block(block),
            ClausuraBody::Fac(fac) => self.write_fac(fac),
        }
    }

    fn write_conversio(&mut self, conversio: &ConversioExpr) {
        self.write_expr_prec(&conversio.expr, 2);
        self.w.write(" ↦ ");
        match &conversio.target {
            ConversioTarget::Explicit(ty) => self.write_type_expr(ty),
            ConversioTarget::Intervallum(expr) => self.write_expr(expr),
        }
        if !conversio.type_params.is_empty() {
            self.w.write("<");
            for (index, type_param) in conversio.type_params.iter().enumerate() {
                if index > 0 {
                    self.w.write(", ");
                }
                self.write_type_expr(type_param);
            }
            self.w.write(">");
        }
        if let Some(recovery) = &conversio.recovery {
            self.w.write(" ⇥ ");
            self.write_expr_prec(recovery, 2);
        }
    }

    /// Emit `Type { field = value }` when the ascribe wraps a genus constructor object.
    fn write_typed_object_constructor(&mut self, verte: &VerteExpr) -> bool {
        let ExprKind::Object(object) = &verte.expr.kind else {
            return false;
        };
        let (name, qualifier) = match &verte.ty.kind {
            TypeExprKind::Named(name, _) => (name, None),
            TypeExprKind::Qualified(path, _) => {
                let Some(name) = path.last() else {
                    return false;
                };
                (name, Some(path.as_slice()))
            }
            _ => return false,
        };
        if let Some(path) = qualifier {
            for (index, segment) in path.iter().enumerate() {
                if index > 0 {
                    self.w.write(".");
                }
                self.write_ident(segment);
            }
        } else {
            self.write_ident(name);
        }
        let fields = &object.fields;
        self.write_brace_list(verte.expr.span, " ", fields.len(), |s, index| {
            s.write_object_field(&fields[index]);
        });
        true
    }

    fn write_object_field(&mut self, field: &ObjectField) {
        match &field.key {
            ObjectKey::Ident(ident) => self.write_ident(ident),
            ObjectKey::String(sym) => self.write_quoted_symbol(*sym),
            ObjectKey::Computed(expr) => {
                self.w.write("[");
                self.write_expr(expr);
                self.w.write("]");
            }
            ObjectKey::Spread(expr) => {
                self.w.write("sparge ");
                self.write_expr(expr);
                return;
            }
        }
        if let Some(value) = &field.value {
            self.w.write(" = ");
            self.write_expr(value);
        }
    }

    fn write_argument(&mut self, arg: &Argument) {
        if arg.spread {
            self.w.write("sparge ");
        }
        self.write_expr(&arg.value);
    }

    // -----------------------------------------------------------------------
    // Wrapping lists
    // -----------------------------------------------------------------------

    fn write_call_args(&mut self, span: Span, args: &[Argument]) {
        if args.is_empty() {
            self.w.write("()");
            return;
        }
        if self.source_multiline(span) {
            self.write_args_expanded(args);
            return;
        }
        self.write_list_layout(
            |s| {
                s.w.write("(");
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        s.w.write(", ");
                    }
                    s.write_argument(arg);
                }
                s.w.write(")");
            },
            |s| s.write_args_expanded(args),
        );
    }

    /// Args after a `?(` / `!(` glyph already written by the caller.
    fn write_args_only(&mut self, span: Span, args: &[Argument], close: &str) {
        if args.is_empty() {
            self.w.write(close);
            return;
        }
        let multiline = self.source_multiline(span);
        if multiline {
            self.write_args_only_expanded(args, close);
            return;
        }
        self.write_list_layout(
            |s| {
                for (index, arg) in args.iter().enumerate() {
                    if index > 0 {
                        s.w.write(", ");
                    }
                    s.write_argument(arg);
                }
                s.w.write(close);
            },
            |s| s.write_args_only_expanded(args, close),
        );
    }

    fn write_args_only_expanded(&mut self, args: &[Argument], close: &str) {
        self.indented(|s| {
            for (index, arg) in args.iter().enumerate() {
                s.w.newline();
                s.write_argument(arg);
                if index + 1 < args.len() {
                    s.w.write(",");
                }
            }
        });
        self.w.newline();
        self.w.write(close);
    }

    fn write_args_expanded(&mut self, args: &[Argument]) {
        self.w.write("(");
        self.indented(|s| {
            for (index, arg) in args.iter().enumerate() {
                s.w.newline();
                s.write_argument(arg);
                if index + 1 < args.len() {
                    s.w.write(",");
                }
            }
        });
        self.w.newline();
        self.w.write(")");
    }

    fn write_array_elements(&mut self, span: Span, elements: &[ArrayElement]) {
        self.write_bracket_list(span, elements.len(), |s, index| {
            s.write_array_element(&elements[index]);
        });
    }

    fn write_bracket_list<F>(&mut self, span: Span, count: usize, render_element: F)
    where
        F: Fn(&mut Self, usize),
    {
        if count == 0 {
            self.w.write("[]");
            return;
        }
        let multiline = self.source_multiline(span);
        if multiline {
            self.write_bracket_list_expanded(count, &render_element);
            return;
        }
        self.write_list_layout(
            |s| {
                s.w.write("[");
                for index in 0..count {
                    if index > 0 {
                        s.w.write(", ");
                    }
                    render_element(s, index);
                }
                s.w.write("]");
            },
            |s| s.write_bracket_list_expanded(count, &render_element),
        );
    }

    fn write_bracket_list_expanded<F>(&mut self, count: usize, render_element: &F)
    where
        F: Fn(&mut Self, usize),
    {
        self.w.write("[");
        self.indented(|s| {
            for index in 0..count {
                s.w.newline();
                render_element(s, index);
                if index + 1 < count {
                    s.w.write(",");
                }
            }
        });
        self.w.newline();
        self.w.write("]");
    }

    fn write_array_element(&mut self, element: &ArrayElement) {
        match element {
            ArrayElement::Expr(expr) => self.write_expr(expr),
            ArrayElement::Spread(expr) => {
                self.w.write("sparge ");
                self.write_expr(expr);
            }
        }
    }

    /// Record-like `{ … }` list: inline when it fits, one field per line when
    /// wrapped, no trailing commas. `prefix` precedes the opening brace (e.g.
    /// `""` for a bare object, `" "` for a typed constructor or `finge`).
    fn write_brace_list<F>(&mut self, span: Span, prefix: &str, count: usize, render_field: F)
    where
        F: Fn(&mut Self, usize),
    {
        if count == 0 {
            self.w.write(prefix);
            self.w.write("{}");
            return;
        }
        let multiline = self.source_multiline(span);
        if multiline {
            self.write_brace_list_expanded(prefix, count, &render_field);
            return;
        }
        self.write_list_layout(
            |s| {
                s.w.write(prefix);
                s.w.write("{ ");
                for index in 0..count {
                    if index > 0 {
                        s.w.write(", ");
                    }
                    render_field(s, index);
                }
                s.w.write(" }");
            },
            |s| s.write_brace_list_expanded(prefix, count, &render_field),
        );
    }

    fn write_brace_list_expanded<F>(&mut self, prefix: &str, count: usize, render_field: &F)
    where
        F: Fn(&mut Self, usize),
    {
        self.w.write(prefix);
        self.w.write("{");
        self.indented(|s| {
            for index in 0..count {
                s.w.newline();
                render_field(s, index);
                if index + 1 < count {
                    s.w.write(",");
                }
            }
        });
        self.w.newline();
        self.w.write("}");
    }

    fn write_literal(&mut self, lit: &Literal) {
        match lit {
            Literal::Int(n) => self.w.write(&n.to_string()),
            Literal::Float(f) => self.w.write(&Self::format_float(*f)),
            Literal::String(sym) => self.write_textus_literal(*sym),
            Literal::Ascii(sym) => {
                self.w.write("'");
                self.w.write(self.interner.resolve(*sym));
                self.w.write("'");
            }
            Literal::Forma(sym) => {
                self.w.write("`");
                self.w.write_raw(self.interner.resolve(*sym));
                self.w.write("`");
            }
            Literal::Octeti(sym) => {
                self.w.write("|");
                self.w.write(self.interner.resolve(*sym));
                self.w.write("|");
            }
            Literal::JsonValor(value) => self.write_json_value(value),
            Literal::Bool(true) => self.w.write("verum"),
            Literal::Bool(false) => self.w.write("falsum"),
            Literal::Nil => self.w.write("nihil"),
        }
    }

    fn write_json_value(&mut self, value: &JsonValue) {
        match value {
            JsonValue::Object(members) => {
                if members.is_empty() {
                    self.w.write("{}");
                    return;
                }
                self.write_list_layout(
                    |s| {
                        s.w.write("{ ");
                        for (index, member) in members.iter().enumerate() {
                            if index > 0 {
                                s.w.write(", ");
                            }
                            s.write_quoted_symbol(member.key);
                            s.w.write(": ");
                            s.write_json_value(&member.value);
                        }
                        s.w.write(" }");
                    },
                    |s| {
                        s.w.write("{");
                        s.indented(|s| {
                            for (index, member) in members.iter().enumerate() {
                                s.w.newline();
                                s.write_quoted_symbol(member.key);
                                s.w.write(": ");
                                s.write_json_value(&member.value);
                                if index + 1 < members.len() {
                                    s.w.write(",");
                                }
                            }
                        });
                        s.w.newline();
                        s.w.write("}");
                    },
                );
            }
            JsonValue::Array(elements) => {
                if elements.is_empty() {
                    self.w.write("[]");
                    return;
                }
                self.write_list_layout(
                    |s| {
                        s.w.write("[");
                        for (index, element) in elements.iter().enumerate() {
                            if index > 0 {
                                s.w.write(", ");
                            }
                            s.write_json_value(element);
                        }
                        s.w.write("]");
                    },
                    |s| {
                        s.w.write("[");
                        s.indented(|s| {
                            for (index, element) in elements.iter().enumerate() {
                                s.w.newline();
                                s.write_json_value(element);
                                if index + 1 < elements.len() {
                                    s.w.write(",");
                                }
                            }
                        });
                        s.w.newline();
                        s.w.write("]");
                    },
                );
            }
            JsonValue::String(sym) => self.write_quoted_symbol(*sym),
            JsonValue::Numerus(n) => self.w.write(&n.to_string()),
            JsonValue::Fractus(f) => self.w.write(&Self::format_float(*f)),
            JsonValue::Bool(true) => self.w.write("true"),
            JsonValue::Bool(false) => self.w.write("false"),
            JsonValue::Null => self.w.write("null"),
        }
    }

    fn write_textus_literal(&mut self, sym: Symbol) {
        let text = self.interner.resolve(sym);
        if text.contains('\n') || text.contains('"') {
            self.w.write("«");
            self.w.write_raw(text);
            self.w.write("»");
        } else {
            self.write_quoted_symbol(sym);
        }
    }

    fn format_float(value: f64) -> String {
        let rendered = value.to_string();
        if rendered.contains('.') || rendered.contains('e') || rendered.contains('E') {
            rendered
        } else {
            format!("{rendered}.0")
        }
    }

    fn write_ident(&mut self, ident: &Ident) {
        self.w.write(self.interner.resolve(ident.name));
    }

    /// Write a dotted identifier path (`pkg.Bonum`) with the authored spelling
    /// of every segment.
    fn write_ident_path(&mut self, path: &[Ident]) {
        for (index, segment) in path.iter().enumerate() {
            if index > 0 {
                self.w.write(".");
            }
            self.w.write(self.interner.resolve(segment.name));
        }
    }

    fn write_quoted_symbol(&mut self, sym: Symbol) {
        self.w.write("\"");
        self.w.write(self.interner.resolve(sym));
        self.w.write("\"");
    }

    fn write_token(&mut self, token: &Token) {
        if let Some(spelling) = keyword_spelling_for_token(&token.kind) {
            self.w.write(spelling);
            return;
        }
        match &token.kind {
            TokenKind::Ident(sym) | TokenKind::Underscore(sym) => self.w.write(self.interner.resolve(*sym)),
            TokenKind::Integer(n) => self.w.write(&n.to_string()),
            TokenKind::Float(f) => self.w.write(&Self::format_float(*f)),
            TokenKind::String(sym) => self.write_quoted_symbol(*sym),
            TokenKind::AsciiString(sym) => {
                self.w.write("'");
                self.w.write(self.interner.resolve(*sym));
                self.w.write("'");
            }
            TokenKind::BacktickString(sym) => {
                self.w.write("`");
                self.w.write(self.interner.resolve(*sym));
                self.w.write("`");
            }
            TokenKind::OctetiString(sym) => {
                self.w.write("|");
                self.w.write(self.interner.resolve(*sym));
                self.w.write("|");
            }
            TokenKind::LParen => self.w.write("("),
            TokenKind::RParen => self.w.write(")"),
            TokenKind::LBrace => self.w.write("{"),
            TokenKind::RBrace => self.w.write("}"),
            TokenKind::LBracket => self.w.write("["),
            TokenKind::RBracket => self.w.write("]"),
            TokenKind::Comma => self.w.write(","),
            TokenKind::Colon => self.w.write(":"),
            TokenKind::Semicolon => self.w.write(";"),
            TokenKind::Dot => self.w.write("."),
            TokenKind::Arrow => self.w.write("→"),
            TokenKind::ExitArrow => self.w.write("⇥"),
            TokenKind::Cup => self.w.write("∪"),
            TokenKind::At => self.w.write("@"),
            TokenKind::Plus => self.w.write("+"),
            TokenKind::Minus => self.w.write("-"),
            TokenKind::Star => self.w.write("*"),
            TokenKind::Slash => self.w.write("/"),
            TokenKind::Percent => self.w.write("%"),
            TokenKind::BitwiseAnd => self.w.write("∧"),
            TokenKind::BitwiseOr => self.w.write("∨"),
            TokenKind::BitwiseXor => self.w.write("⊻"),
            TokenKind::BitwiseNot => self.w.write("¬"),
            TokenKind::BitwiseShl => self.w.write("⇐"),
            TokenKind::BitwiseShr => self.w.write("⇒"),
            TokenKind::Bang => self.w.write("!"),
            TokenKind::Question => self.w.write("?"),
            TokenKind::Eq => self.w.write("="),
            TokenKind::Assign => self.w.write("←"),
            TokenKind::ConversioAssign => self.w.write("↤"),
            TokenKind::EqEq => self.w.write("≡"),
            TokenKind::ApproxEq => self.w.write("≈"),
            TokenKind::ApproxNotEq => self.w.write("≉"),
            TokenKind::BangEq => self.w.write("≠"),
            TokenKind::Lt => self.w.write("≺"),
            TokenKind::Gt => self.w.write("≻"),
            TokenKind::LtAscii => self.w.write("<"),
            TokenKind::GtAscii => self.w.write(">"),
            TokenKind::LtEq => self.w.write("≤"),
            TokenKind::GtEq => self.w.write("≥"),
            TokenKind::PostInc => self.w.write("↑"),
            TokenKind::PostDec => self.w.write("↓"),
            TokenKind::QuestionDot => self.w.write("?."),
            TokenKind::QuestionBracket => self.w.write("?["),
            TokenKind::QuestionParen => self.w.write("?("),
            TokenKind::BangDot => self.w.write("!."),
            TokenKind::BangBracket => self.w.write("!["),
            TokenKind::BangParen => self.w.write("!("),
            TokenKind::DotDot => self.w.write("‥"),
            TokenKind::Ellipsis => self.w.write("…"),
            TokenKind::Therefore => self.w.write("∴"),
            TokenKind::LineComment(sym) => {
                self.w.write("#");
                self.w.write(self.interner.resolve(*sym));
            }
            _ => self.w.write("?"),
        }
    }

    fn expr_precedence(expr: &Expr) -> u8 {
        match &expr.kind {
            ExprKind::Assign(_) => 1,
            ExprKind::Ternary(_) | ExprKind::Conversio(_) => 2,
            ExprKind::Binary(binary) => Self::binop_precedence(binary.op),
            ExprKind::Unary(_) => 12,
            ExprKind::Call(_)
            | ExprKind::Member(_)
            | ExprKind::Index(_)
            | ExprKind::Gradient(_)
            | ExprKind::OptionalChain(_)
            | ExprKind::NonNull(_)
            | ExprKind::Lege(_) => 13,
            _ => 14,
        }
    }

    fn binop_precedence(op: BinOp) -> u8 {
        match op {
            BinOp::Or => 2,
            BinOp::And => 3,
            BinOp::Eq
            | BinOp::NotEq
            | BinOp::ApproxEq
            | BinOp::ApproxNotEq
            | BinOp::StrictEq
            | BinOp::StrictNotEq
            | BinOp::Is
            | BinOp::IsNot => 4,
            BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq | BinOp::InRange | BinOp::Between => 5,
            BinOp::BitOr => 6,
            BinOp::BitXor => 7,
            BinOp::BitAnd => 8,
            BinOp::Shl | BinOp::Shr => 9,
            BinOp::Add | BinOp::Sub => 10,
            BinOp::Mul
            | BinOp::Div
            | BinOp::Mod
            | BinOp::Dot
            | BinOp::Cross
            | BinOp::TensorProduct
            | BinOp::Hadamard => 11,
            BinOp::Coalesce => 12,
        }
    }

    fn binop_to_faber(op: BinOp) -> &'static str {
        match op {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Mod => "%",
            BinOp::Dot => "·",
            BinOp::Cross => "×",
            BinOp::TensorProduct => "⊗",
            BinOp::Hadamard => "⊙",
            BinOp::Eq | BinOp::StrictEq => "≡",
            BinOp::ApproxEq => "≈",
            BinOp::ApproxNotEq => "≉",
            BinOp::NotEq | BinOp::StrictNotEq => "≠",
            BinOp::Lt => "≺",
            BinOp::Gt => "≻",
            BinOp::LtEq => "≤",
            BinOp::GtEq => "≥",
            BinOp::And => "et",
            BinOp::Or => "aut",
            BinOp::Coalesce => "vel",
            BinOp::BitAnd => "∧",
            BinOp::BitOr => "∨",
            BinOp::BitXor => "⊻",
            BinOp::Shl => "⇐",
            BinOp::Shr => "⇒",
            BinOp::Is => "est",
            BinOp::IsNot => "non est",
            BinOp::InRange => "intra",
            BinOp::Between => "inter",
        }
    }

    fn unop_to_faber(op: UnOp) -> &'static str {
        match op {
            UnOp::Neg => "-",
            UnOp::Not => "non ",
            UnOp::BitNot => "¬",
        }
    }
}

#[cfg(test)]
#[path = "emit_test.rs"]
mod tests;
