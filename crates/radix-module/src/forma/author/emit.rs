//! Author-mode (lossy) emitter: walk the parsed AST and emit comment-preserving
//! Faber source.
//!
//! Preserves leading trivia (comments and blank lines) but does not guarantee
//! bit-for-bit identity. The canonical mode in `super::canonical` owns the
//! semantically-verified re-emission path.

use crate::codegen::CodeWriter;
use crate::lexer::{keyword_spelling_for_token, Interner, Symbol, Token, TokenKind};
use crate::locale::KeywordSurface;
#[allow(clippy::wildcard_imports)]
use crate::syntax::*;

use super::super::options::FormatOptions;
use super::layout::{new_writer, top_level_trivia_scope, TriviaScope};
use super::trivia::write_leading_trivia;
use crate::FaberOutput;

pub struct AuthorEmitter;

impl AuthorEmitter {
    #[must_use]
    pub fn emit(
        program: &Program,
        surface: &KeywordSurface,
        interner: &Interner,
        _options: &FormatOptions,
    ) -> FaberOutput {
        let mut w = new_writer();
        for (index, stmt) in program.statements.iter().enumerate() {
            Self::write_stmt(stmt, top_level_trivia_scope(index), surface, interner, &mut w);
        }
        let mut code = w.finish();
        if !code.ends_with('\n') {
            code.push('\n');
        }
        FaberOutput { code }
    }

    fn write_stmt(stmt: &Stmt, scope: TriviaScope, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        Self::write_leading_trivia_before_annotations(
            &stmt.leading_trivia,
            !stmt.annotations.is_empty(),
            scope,
            surface,
            interner,
            w,
        );
        for annotation in &stmt.annotations {
            Self::write_annotation(annotation, surface, interner, w);
            w.newline();
        }

        match &stmt.kind {
            StmtKind::Var(decl) => Self::write_var_decl(decl, surface, interner, w),
            StmtKind::Func(func) => Self::write_func_decl(func, surface, interner, w),
            StmtKind::Class(class) => Self::write_class_decl(class, surface, interner, w),
            StmtKind::Interface(iface) => Self::write_interface_decl(iface, surface, interner, w),
            StmtKind::TypeAlias(alias) => Self::write_type_alias(alias, surface, interner, w),
            StmtKind::Enum(enum_decl) => Self::write_enum_decl(enum_decl, surface, interner, w),
            StmtKind::Union(union) => Self::write_union_decl(union, surface, interner, w),
            StmtKind::Schema(schema) => Self::write_schema_decl(schema, surface, interner, w),
            StmtKind::Import(import) => Self::write_import(import, surface, interner, w),
            StmtKind::Probandum(suite) => Self::write_probandum(suite, surface, interner, w),
            StmtKind::Proba(case) => Self::write_proba(case, surface, interner, w),
            StmtKind::Ex(ex) => Self::write_ex_stmt(ex, surface, interner, w),
            StmtKind::Block(block) => {
                Self::write_block(block, surface, interner, w);
                w.newline();
            }
            StmtKind::Expr(expr) => {
                Self::write_expr(&expr.expr, surface, interner, w);
                w.newline();
            }
            StmtKind::IncDec(inc) => {
                Self::write_expr(&inc.target, surface, interner, w);
                w.write(match inc.op {
                    IncDecOp::Inc => " ↑",
                    IncDecOp::Dec => " ↓",
                });
                w.newline();
            }
            StmtKind::Si(si) => Self::write_si_stmt(si, surface, interner, w),
            StmtKind::Dum(dum) => Self::write_dum_stmt(dum, surface, interner, w),
            StmtKind::Itera(itera) => Self::write_itera_stmt(itera, surface, interner, w),
            StmtKind::Elige(elige) => Self::write_elige_stmt(elige, surface, interner, w),
            StmtKind::Discerne(discerne) => Self::write_discerne_stmt(discerne, surface, interner, w),
            StmtKind::Custodi(custodi) => Self::write_custodi_stmt(custodi, surface, interner, w),
            StmtKind::Fac(fac) => Self::write_fac_stmt(fac, surface, interner, w),
            StmtKind::Redde(redde) => {
                w.write(surface.contextual_keyword("redde"));
                if let Some(value) = &redde.value {
                    w.write(" ");
                    Self::write_expr(value, surface, interner, w);
                }
                w.newline();
            }
            StmtKind::Rumpe(_) => w.writeln(surface.contextual_keyword("rumpe")),
            StmtKind::Perge(_) => w.writeln(surface.contextual_keyword("perge")),
            StmtKind::Iace(iace) => {
                w.write(surface.contextual_keyword("iace"));
                w.write(" ");
                Self::write_expr(&iace.value, surface, interner, w);
                w.newline();
            }
            StmtKind::Mori(mori) => {
                w.write(surface.contextual_keyword("mori"));
                w.write(" ");
                Self::write_expr(&mori.value, surface, interner, w);
                w.newline();
            }
            StmtKind::Tacet(_) => w.writeln(surface.contextual_keyword("tacet")),
            StmtKind::Reddet(stmt) => {
                w.write(surface.contextual_keyword("reddet"));
                w.write(" ");
                Self::write_expr(&stmt.future, surface, interner, w);
                w.newline();
            }
            StmtKind::Tacebit(stmt) => {
                w.write(surface.contextual_keyword("tacebit"));
                w.write(" ");
                Self::write_expr(&stmt.future, surface, interner, w);
                w.newline();
            }
            StmtKind::Adfirma(adfirma) => {
                w.write(surface.contextual_keyword("adfirma"));
                w.write(" ");
                Self::write_expr(&adfirma.cond, surface, interner, w);
                if let Some(message) = &adfirma.message {
                    w.write(" ");
                    w.write(surface.contextual_keyword("mori"));
                    w.write(" ");
                    Self::write_expr(message, surface, interner, w);
                }
                w.newline();
            }
            StmtKind::Require(require) => {
                w.write(surface.contextual_keyword("requirit"));
                w.write(" ");
                Self::write_expr(&require.cond, surface, interner, w);
                w.write(" ");
                w.write(surface.contextual_keyword("iace"));
                w.write(" ");
                Self::write_expr(&require.error, surface, interner, w);
                w.newline();
            }
            StmtKind::Scribe(scribe) => Self::write_scribe_stmt(scribe, surface, interner, w),
            StmtKind::Incipit(incipit) => Self::write_incipit_stmt(incipit, surface, interner, w),
            StmtKind::Cura(cura) => Self::write_cura_stmt(cura, surface, interner, w),
        }
    }

    // Parser-leading trivia for annotated statements includes the line break
    // after the final annotation. Emit that boundary after `@ ...`, not before it.
    fn write_leading_trivia_before_annotations(
        trivia: &[Trivia],
        has_annotations: bool,
        scope: TriviaScope,
        _surface: &KeywordSurface,
        interner: &Interner,
        w: &mut CodeWriter,
    ) {
        if !has_annotations {
            write_leading_trivia(trivia, scope, interner, w);
            return;
        }

        let mut end = trivia.len();
        if trivia
            .last()
            .is_some_and(|item| matches!(item, Trivia::Newline(_)))
        {
            end -= 1;
        }
        write_leading_trivia(&trivia[..end], scope, interner, w);
    }

    fn write_block(block: &BlockStmt, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.writeln("{");
        w.indented(|w| {
            for stmt in &block.statements {
                Self::write_stmt(stmt, TriviaScope::Inner, surface, interner, w);
            }
        });
        // Continuation keywords are emitted by their callers on the next line.
        w.write("}");
    }

    // Returns true when an ergo body already emitted the line terminator.
    fn write_if_body(body: &IfBody, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) -> bool {
        match body {
            IfBody::Block(block) => {
                w.write(" ");
                Self::write_block(block, surface, interner, w);
                false
            }
            IfBody::Ergo(stmt) => {
                w.write(" ");
                w.write(surface.contextual_keyword("ergo"));
                w.write(" ");
                Self::write_stmt(stmt, TriviaScope::Inner, surface, interner, w);
                true
            }
        }
    }

    fn write_cape(cape: &CapeClause, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("cape"));
        w.write(" ");
        Self::write_ident(&cape.binding, surface, interner, w);
        w.write(" ");
        Self::write_block(&cape.body, surface, interner, w);
    }

    fn write_cape_continuation(
        cape: &CapeClause,
        prior_body_ended_line: bool,
        surface: &KeywordSurface,
        interner: &Interner,
        w: &mut CodeWriter,
    ) {
        if !prior_body_ended_line {
            w.newline();
        }
        Self::write_cape(cape, surface, interner, w);
    }

    fn write_si_stmt(si: &SiStmt, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("si"));
        w.write(" ");
        Self::write_expr(&si.cond, surface, interner, w);
        let mut body_ended_line = Self::write_if_body(&si.then, surface, interner, w);
        if let Some(catch) = &si.catch {
            Self::write_cape_continuation(catch, body_ended_line, surface, interner, w);
            body_ended_line = false;
        }
        if let Some(else_) = &si.else_ {
            Self::write_secus_clause(else_, body_ended_line, surface, interner, w);
        } else if !body_ended_line {
            w.newline();
        }
    }

    fn write_secus_clause(
        secus: &SecusClause,
        prior_body_ended_line: bool,
        surface: &KeywordSurface,
        interner: &Interner,
        w: &mut CodeWriter,
    ) {
        if !prior_body_ended_line {
            w.newline();
        }
        let leading_trivia = match secus {
            SecusClause::Sin { leading_trivia, .. } => leading_trivia,
            SecusClause::Block { leading_trivia, .. } => leading_trivia,
            SecusClause::Stmt { leading_trivia, .. } => leading_trivia,
        };
        write_leading_trivia(leading_trivia, TriviaScope::Inner, interner, w);
        match secus {
            SecusClause::Sin { stmt: si, .. } => {
                w.write(surface.contextual_keyword("sin"));
                w.write(" ");
                Self::write_expr(&si.cond, surface, interner, w);
                let mut body_ended_line = Self::write_if_body(&si.then, surface, interner, w);
                if let Some(catch) = &si.catch {
                    Self::write_cape_continuation(catch, body_ended_line, surface, interner, w);
                    body_ended_line = false;
                }
                if let Some(else_) = &si.else_ {
                    Self::write_secus_clause(else_, body_ended_line, surface, interner, w);
                } else if !body_ended_line {
                    w.newline();
                }
            }
            SecusClause::Block { body, catch, .. } => {
                w.write(surface.contextual_keyword("secus"));
                w.write(" ");
                Self::write_block(body, surface, interner, w);
                if let Some(catch) = catch {
                    Self::write_cape_continuation(catch, false, surface, interner, w);
                }
                w.newline();
            }
            SecusClause::Stmt { stmt, catch, .. } => {
                w.write(surface.contextual_keyword("secus"));
                w.write(" ");
                w.write(surface.contextual_keyword("ergo"));
                w.write(" ");
                Self::write_stmt(stmt, TriviaScope::Inner, surface, interner, w);
                if let Some(catch) = catch {
                    Self::write_cape_continuation(catch, true, surface, interner, w);
                    w.newline();
                }
            }
        }
    }

    fn write_dum_stmt(dum: &DumStmt, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("dum"));
        w.write(" ");
        Self::write_expr(&dum.cond, surface, interner, w);
        let mut body_ended_line = Self::write_if_body(&dum.body, surface, interner, w);
        if let Some(catch) = &dum.catch {
            Self::write_cape_continuation(catch, body_ended_line, surface, interner, w);
            body_ended_line = false;
        }
        if !body_ended_line {
            w.newline();
        }
    }

    fn write_itera_stmt(itera: &IteraStmt, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("itera"));
        w.write(" ");
        w.write(match itera.mode {
            IteraMode::Ex => surface.contextual_keyword("ex"),
            IteraMode::De => surface.contextual_keyword("de"),
            IteraMode::Ab => surface.contextual_keyword("ab"),
        });
        w.write(" ");
        Self::write_expr(&itera.iterable, surface, interner, w);
        w.write(" ");
        w.write(match itera.mutability {
            AstMutability::Immutable => surface.contextual_keyword("fixum"),
            AstMutability::Mutable => surface.contextual_keyword("varia"),
        });
        w.write(" ");
        Self::write_ident(&itera.binding, surface, interner, w);
        let mut body_ended_line = Self::write_if_body(&itera.body, surface, interner, w);
        if let Some(catch) = &itera.catch {
            Self::write_cape_continuation(catch, body_ended_line, surface, interner, w);
            body_ended_line = false;
        }
        if !body_ended_line {
            w.newline();
        }
    }

    fn write_elige_stmt(elige: &EligeStmt, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("elige"));
        w.write(" ");
        Self::write_expr(&elige.expr, surface, interner, w);
        w.writeln(" {");
        w.indented(|w| {
            for case in &elige.cases {
                w.write(surface.contextual_keyword("casu"));
                w.write(" ");
                Self::write_expr(&case.value, surface, interner, w);
                if !Self::write_if_body(&case.body, surface, interner, w) {
                    w.newline();
                }
            }
            if let Some(default) = &elige.default {
                w.write(surface.contextual_keyword("ceterum"));
                w.write(" ");
                if !Self::write_if_body(&default.body, surface, interner, w) {
                    w.newline();
                }
            }
        });
        w.write("}");
        if let Some(catch) = &elige.catch {
            Self::write_cape_continuation(catch, false, surface, interner, w);
        }
        w.newline();
    }

    fn write_discerne_arms(discerne: &DiscerneStmt, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        for arm in &discerne.arms {
            w.write(surface.contextual_keyword("casu"));
            w.write(" ");
            for (index, pattern) in arm.patterns.iter().enumerate() {
                if index > 0 {
                    w.write(", ");
                }
                Self::write_pattern(pattern, surface, interner, w);
            }
            if !Self::write_if_body(&arm.body, surface, interner, w) {
                w.newline();
            }
        }
        if let Some(default) = &discerne.default {
            w.write(surface.contextual_keyword("ceterum"));
            w.write(" ");
            if !Self::write_if_body(&default.body, surface, interner, w) {
                w.newline();
            }
        }
    }

    fn write_discerne_stmt(discerne: &DiscerneStmt, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("discerne"));
        w.write(" ");
        if discerne.exhaustive {
            w.write(surface.contextual_keyword("omnia"));
            w.write(" ");
        }
        for (index, subject) in discerne.subjects.iter().enumerate() {
            if index > 0 {
                w.write(", ");
            }
            Self::write_expr(subject, surface, interner, w);
        }
        w.writeln(" {");
        w.indented(|w| Self::write_discerne_arms(discerne, surface, interner, w));
        w.writeln("}");
    }

    fn write_custodi_stmt(custodi: &CustodiStmt, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("custodi"));
        w.write(" {");
        w.newline();
        w.indented(|w| {
            for clause in &custodi.clauses {
                w.write(surface.contextual_keyword("si"));
                w.write(" ");
                Self::write_expr(&clause.cond, surface, interner, w);
                if !Self::write_if_body(&clause.body, surface, interner, w) {
                    w.newline();
                }
            }
        });
        w.writeln("}");
    }

    fn write_fac(fac: &FacStmt, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("fac"));
        w.write(" ");
        Self::write_block(&fac.body, surface, interner, w);
        if let Some(catch) = &fac.catch {
            Self::write_cape_continuation(catch, false, surface, interner, w);
        }
        if let Some(cond) = &fac.while_ {
            w.write(" ");
            w.write(surface.contextual_keyword("dum"));
            w.write(" ");
            Self::write_expr(cond, surface, interner, w);
        }
    }

    fn write_fac_stmt(fac: &FacStmt, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        Self::write_fac(fac, surface, interner, w);
        w.newline();
    }

    fn write_scribe_stmt(scribe: &ScribeStmt, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(match scribe.kind {
            ScribeKind::Scribe => surface.contextual_keyword("scribe"),
            ScribeKind::Vide => surface.contextual_keyword("vide"),
            ScribeKind::Mone => surface.contextual_keyword("mone"),
            ScribeKind::Nota => surface.contextual_keyword("nota"),
        });
        for (index, arg) in scribe.args.iter().enumerate() {
            if index == 0 {
                w.write(" ");
            } else {
                w.write(", ");
            }
            Self::write_expr(arg, surface, interner, w);
        }
        w.newline();
    }

    fn write_incipit_stmt(incipit: &IncipitStmt, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(if incipit.is_async {
            surface.contextual_keyword("incipiet")
        } else {
            surface.contextual_keyword("incipit")
        });
        if let Some(args) = &incipit.args {
            w.write(" ");
            w.write(surface.contextual_keyword("argumenta"));
            w.write(" ");
            Self::write_ident(args, surface, interner, w);
        }
        if let Some(exitus) = &incipit.exitus {
            w.write(" ");
            w.write(surface.contextual_keyword("exitus"));
            w.write(" ");
            Self::write_expr(exitus, surface, interner, w);
        }
        if !Self::write_if_body(&incipit.body, surface, interner, w) {
            w.newline();
        }
    }

    fn write_cura_stmt(cura: &CuraStmt, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("cura"));
        w.write(" ");
        w.write(match cura.kind {
            CuraKind::Arena => "\"arena\" ",
            CuraKind::Page => "\"page\" ",
        });
        w.write(match cura.mutability {
            AstMutability::Immutable => surface.contextual_keyword("fixum"),
            AstMutability::Mutable => surface.contextual_keyword("varia"),
        });
        w.write(" ");
        Self::write_type_expr(&cura.ty, surface, interner, w);
        w.write(" ");
        Self::write_ident(&cura.binding, surface, interner, w);
        w.write(" ");
        Self::write_block(&cura.body, surface, interner, w);
        if let Some(catch) = &cura.catch {
            Self::write_cape_continuation(catch, false, surface, interner, w);
        }
        w.newline();
    }

    fn write_ex_stmt(ex: &ExStmt, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("ex"));
        w.write(" ");
        Self::write_expr(&ex.source, surface, interner, w);
        w.write(" ");
        w.write(match ex.mutability {
            AstMutability::Immutable => surface.contextual_keyword("fixum"),
            AstMutability::Mutable => surface.contextual_keyword("varia"),
        });
        for (index, field) in ex.fields.iter().enumerate() {
            if index > 0 {
                w.write(", ");
            } else {
                w.write(" ");
            }
            Self::write_ident(&field.name, surface, interner, w);
            if let Some(alias) = &field.alias {
                w.write(" ");
                w.write(surface.contextual_keyword("ut"));
                w.write(" ");
                Self::write_ident(alias, surface, interner, w);
            }
        }
        if let Some(rest) = &ex.rest {
            if ex.fields.is_empty() {
                w.write(" ");
            } else {
                w.write(", ");
            }
            w.write(surface.contextual_keyword("ceteri"));
            w.write(" ");
            Self::write_ident(rest, surface, interner, w);
        }
        w.newline();
    }

    fn write_var_decl(decl: &VarDecl, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        if decl.is_await {
            w.write(match decl.mutability {
                AstMutability::Immutable => surface.contextual_keyword("figendum"),
                AstMutability::Mutable => surface.contextual_keyword("variandum"),
            });
        } else {
            w.write(match decl.mutability {
                AstMutability::Immutable => surface.contextual_keyword("fixum"),
                AstMutability::Mutable => surface.contextual_keyword("varia"),
            });
        }
        if let Some(ty) = &decl.ty {
            w.write(" ");
            Self::write_type_expr(ty, surface, interner, w);
        }
        w.write(" ");
        Self::write_binding_pattern(&decl.binding, surface, interner, w);
        if let Some(init) = &decl.init {
            if let ExprKind::Assign(assign) = &init.kind {
                if matches!(assign.op, AssignOp::ConversioAssign) {
                    // Typed `↤` initializer: the parser stored the binding
                    // as the assignment target, so re-emit as
                    // `binding ↤ value [⇥ recovery]` — never `binding ←
                    // binding ↤ value`.
                    w.write(" ↤ ");
                    Self::write_expr_prec(&assign.value, 1, surface, interner, w);
                    if let Some(recovery) = &assign.recovery {
                        w.write(" ⇥ ");
                        Self::write_expr_prec(recovery, 12, surface, interner, w);
                    }
                } else {
                    w.write(" ← ");
                    Self::write_expr(init, surface, interner, w);
                }
            } else {
                w.write(" ← ");
                Self::write_expr(init, surface, interner, w);
            }
        }
        w.newline();
    }

    fn write_func_decl(func: &FuncDecl, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        Self::write_leading_trivia_before_annotations(
            &func.leading_trivia,
            !func.annotations.is_empty(),
            TriviaScope::Inner,
            surface,
            interner,
            w,
        );
        for annotation in &func.annotations {
            Self::write_annotation(annotation, surface, interner, w);
            w.newline();
        }
        w.write(surface.contextual_keyword("functio"));
        w.write(" ");
        Self::write_ident(&func.name, surface, interner, w);
        Self::write_generic_params(&func.type_params, surface, interner, w);
        w.write("(");
        for (index, param) in func.params.iter().enumerate() {
            if index > 0 {
                w.write(", ");
            }
            Self::write_param(param, surface, interner, w);
        }
        w.write(")");
        for modifier in &func.modifiers {
            w.write(" ");
            Self::write_func_modifier(modifier, surface, interner, w);
        }
        Self::write_callable_posture(func.posture, surface, w);
        if let Some(ret) = &func.ret {
            w.write(" → ");
            Self::write_type_expr(ret, surface, interner, w);
        }
        if let Some(err) = &func.err {
            w.write(" ⇥ ");
            Self::write_type_expr(err, surface, interner, w);
        }
        if let Some(body) = &func.body {
            w.write(" ");
            Self::write_block(body, surface, interner, w);
        }
        w.newline();
    }

    fn write_class_members(class: &ClassDecl, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        for member in &class.members {
            Self::write_leading_trivia_before_annotations(
                &member.leading_trivia,
                !member.annotations.is_empty(),
                TriviaScope::Inner,
                surface,
                interner,
                w,
            );
            for annotation in &member.annotations {
                Self::write_annotation(annotation, surface, interner, w);
                w.newline();
            }
            match &member.kind {
                ClassMemberKind::Field(field) => Self::write_field_decl(field, surface, interner, w),
                ClassMemberKind::Method(func) => Self::write_func_decl(func, surface, interner, w),
            }
        }
    }

    fn write_class_decl(class: &ClassDecl, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        if class.is_abstract {
            w.write(surface.contextual_keyword("abstractus"));
            w.write(" ");
        }
        w.write(surface.contextual_keyword("genus"));
        w.write(" ");
        Self::write_ident(&class.name, surface, interner, w);
        Self::write_generic_params(&class.type_params, surface, interner, w);
        if let Some(extends) = &class.extends {
            w.write(" ");
            w.write(surface.contextual_keyword("sub"));
            w.write(" ");
            Self::write_ident(extends, surface, interner, w);
        }
        if !class.implements.is_empty() {
            w.write(" ");
            w.write(surface.contextual_keyword("implet"));
            w.write(" ");
            for (index, iface) in class.implements.iter().enumerate() {
                if index > 0 {
                    w.write(", ");
                }
                Self::write_ident(iface, surface, interner, w);
            }
        }
        w.writeln(" {");
        w.indented(|w| Self::write_class_members(class, surface, interner, w));
        w.writeln("}");
    }

    fn write_field_decl(field: &FieldDecl, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        if field.is_static {
            w.write(surface.contextual_keyword("generis"));
            w.write(" ");
        }
        if field.is_bound {
            w.write(surface.contextual_keyword("nexum"));
            w.write(" ");
        }
        Self::write_type_expr(&field.ty, surface, interner, w);
        w.write(" ");
        Self::write_ident(&field.name, surface, interner, w);
        if field.sponte {
            w.write(" ");
            w.write(surface.contextual_keyword("sponte"));
        }
        if let Some(init) = &field.init {
            w.write(" = ");
            Self::write_expr(init, surface, interner, w);
        }
        w.newline();
    }

    fn write_interface_methods(
        iface: &InterfaceDecl,
        surface: &KeywordSurface,
        interner: &Interner,
        w: &mut CodeWriter,
    ) {
        for method in &iface.methods {
            Self::write_leading_trivia_before_annotations(
                &method.leading_trivia,
                !method.annotations.is_empty(),
                TriviaScope::Inner,
                surface,
                interner,
                w,
            );
            for annotation in &method.annotations {
                Self::write_annotation(annotation, surface, interner, w);
                w.newline();
            }
            w.write(surface.contextual_keyword("functio"));
            w.write(" ");
            Self::write_ident(&method.name, surface, interner, w);
            w.write("(");
            for (index, param) in method.params.iter().enumerate() {
                if index > 0 {
                    w.write(", ");
                }
                Self::write_param(param, surface, interner, w);
            }
            w.write(")");
            for modifier in &method.modifiers {
                w.write(" ");
                Self::write_func_modifier(modifier, surface, interner, w);
            }
            Self::write_callable_posture(method.posture, surface, w);
            if let Some(ret) = &method.ret {
                w.write(" → ");
                Self::write_type_expr(ret, surface, interner, w);
            }
            if let Some(err) = &method.err {
                w.write(" ⇥ ");
                Self::write_type_expr(err, surface, interner, w);
            }
            w.newline();
        }
    }

    fn write_callable_posture(posture: CallablePosture, surface: &KeywordSurface, w: &mut CodeWriter) {
        match posture {
            CallablePosture::SyncFinite => {}
            CallablePosture::AsyncFinite => {
                w.write(" ");
                w.write(surface.contextual_keyword("fiet"));
            }
            CallablePosture::SyncStream => {
                w.write(" ");
                w.write(surface.contextual_keyword("fiunt"));
            }
            CallablePosture::AsyncStream => {
                w.write(" ");
                w.write(surface.contextual_keyword("fient"));
            }
        }
    }

    fn write_interface_decl(iface: &InterfaceDecl, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("implendum"));
        w.write(" ");
        Self::write_ident(&iface.name, surface, interner, w);
        Self::write_generic_params(&iface.type_params, surface, interner, w);
        w.writeln(" {");
        w.indented(|w| Self::write_interface_methods(iface, surface, interner, w));
        w.writeln("}");
    }

    fn write_type_alias(alias: &TypeAliasDecl, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("typus"));
        w.write(" ");
        Self::write_ident(&alias.name, surface, interner, w);
        Self::write_generic_params(&alias.type_params, surface, interner, w);
        w.write(" = ");
        Self::write_type_expr(&alias.ty, surface, interner, w);
        w.newline();
    }

    fn write_enum_decl(enum_decl: &EnumDecl, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("ordo"));
        w.write(" ");
        Self::write_ident(&enum_decl.name, surface, interner, w);
        w.writeln(" {");
        w.indented(|w| {
            for member in &enum_decl.members {
                Self::write_ident(&member.name, surface, interner, w);
                if let Some(value) = &member.value {
                    w.write(" = ");
                    match value {
                        EnumValue::Integer(n) => w.write(&n.to_string()),
                        EnumValue::String(sym) => Self::write_quoted_symbol(*sym, surface, interner, w),
                    }
                }
                w.newline();
            }
        });
        w.writeln("}");
    }

    fn write_schema_decl(schema: &SchemaDecl, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("schema"));
        w.write(" ");
        Self::write_ident(&schema.name, surface, interner, w);
        w.writeln(" {");
        w.indented(|w| {
            for column in &schema.columns {
                Self::write_schema_column(column, surface, interner, w);
            }
        });
        w.writeln("}");
    }

    fn write_schema_column(column: &SchemaColumn, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        write_leading_trivia(&column.leading_trivia, TriviaScope::Inner, interner, w);
        w.write(surface.contextual_keyword("columna"));
        w.write(" ");
        Self::write_type_expr(&column.ty, surface, interner, w);
        w.write(" ");
        Self::write_ident(&column.name, surface, interner, w);
        if let Some(alias) = &column.source_alias {
            w.write(" : ");
            Self::write_ident(alias, surface, interner, w);
        }
        w.newline();
    }

    fn write_union_decl(union: &UnionDecl, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("discretio"));
        w.write(" ");
        Self::write_ident(&union.name, surface, interner, w);
        Self::write_generic_params(&union.type_params, surface, interner, w);
        w.writeln(" {");
        w.indented(|w| {
            for (index, variant) in union.variants.iter().enumerate() {
                Self::write_ident(&variant.name, surface, interner, w);
                if !variant.fields.is_empty() {
                    w.write(" {");
                    for (field_index, field) in variant.fields.iter().enumerate() {
                        if field_index > 0 {
                            w.write(", ");
                        } else {
                            w.write(" ");
                        }
                        Self::write_type_expr(&field.ty, surface, interner, w);
                        w.write(" ");
                        Self::write_ident(&field.name, surface, interner, w);
                    }
                    w.write(" ");
                    w.write("}");
                }
                if index + 1 < union.variants.len() {
                    w.write(",");
                }
                w.newline();
            }
        });
        w.writeln("}");
    }

    fn write_import(import: &ImportDecl, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("importa"));
        w.write(" ");
        w.write(surface.contextual_keyword("ex"));
        w.write(" ");
        Self::write_quoted_symbol(import.path, surface, interner, w);
        if import.visibility == Visibility::Publica {
            w.write(" ");
            w.write(surface.contextual_keyword("publica"));
        }
        match &import.kind {
            ImportKind::Named { name, alias, inferred } => {
                if !inferred {
                    w.write(" ");
                    Self::write_ident(name, surface, interner, w);
                    if let Some(alias) = alias {
                        w.write(" ");
                        w.write(surface.contextual_keyword("ut"));
                        w.write(" ");
                        Self::write_ident(alias, surface, interner, w);
                    }
                }
            }
            ImportKind::Wildcard { alias } => {
                w.write(" * ");
                w.write(surface.contextual_keyword("ut"));
                w.write(" ");
                Self::write_ident(alias, surface, interner, w);
            }
        }
        w.newline();
    }

    fn write_probandum(suite: &ProbandumDecl, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("probandum"));
        w.write(" ");
        Self::write_quoted_symbol(suite.name, surface, interner, w);
        for modifier in &suite.modifiers {
            w.write(" ");
            Self::write_proba_modifier(modifier, surface, interner, w);
        }
        w.writeln(" {");
        w.indented(|w| {
            for setup in &suite.body.setup {
                Self::write_praepara(setup, surface, interner, w);
            }
            for test in &suite.body.tests {
                Self::write_proba(test, surface, interner, w);
            }
            for nested in &suite.body.nested {
                Self::write_probandum(nested, surface, interner, w);
            }
        });
        w.writeln("}");
    }

    fn write_praepara(block: &PraeparaBlock, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(match block.kind {
            PraeparaKind::Praepara => surface.contextual_keyword("praepara"),
            PraeparaKind::Praeparabit => surface.contextual_keyword("praeparabit"),
            PraeparaKind::Postpara => surface.contextual_keyword("postpara"),
            PraeparaKind::Postparabit => surface.contextual_keyword("postparabit"),
        });
        if block.all {
            w.write(" ");
            w.write(surface.contextual_keyword("omnia"));
        }
        w.write(" ");
        Self::write_block(&block.body, surface, interner, w);
        w.newline();
    }

    fn write_proba(case: &ProbaCase, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword("proba"));
        w.write(" ");
        Self::write_quoted_symbol(case.name, surface, interner, w);
        for modifier in &case.modifiers {
            w.write(" ");
            Self::write_proba_modifier(modifier, surface, interner, w);
        }
        w.write(" ");
        Self::write_block(&case.body, surface, interner, w);
        w.newline();
    }

    fn write_proba_modifier(
        modifier: &ProbaModifier,
        surface: &KeywordSurface,
        interner: &Interner,
        w: &mut CodeWriter,
    ) {
        match modifier {
            ProbaModifier::Omitte(reason) => {
                Self::write_quoted_proba_token(surface, "omitte", *reason, interner, w);
            }
            ProbaModifier::Futurum(reason) => {
                Self::write_quoted_proba_token(surface, "futurum", *reason, interner, w);
            }
            ProbaModifier::Solum => w.write(surface.contextual_keyword("solum")),
            ProbaModifier::Tag(tag) => {
                Self::write_quoted_proba_token(surface, "tag", *tag, interner, w);
            }
            ProbaModifier::Temporis(n) => {
                Self::write_numeric_proba_token(surface, "temporis", *n, w);
            }
            ProbaModifier::Metior => w.write(surface.contextual_keyword("metior")),
            ProbaModifier::Repete(n) => {
                Self::write_numeric_proba_token(surface, "repete", *n, w);
            }
            ProbaModifier::Fragilis(n) => {
                Self::write_numeric_proba_token(surface, "fragilis", *n, w);
            }
            ProbaModifier::SolumIn(env) => {
                Self::write_quoted_proba_token(surface, "solum_in", *env, interner, w);
            }
        }
    }

    fn write_quoted_proba_token(
        surface: &KeywordSurface,
        latin: &'static str,
        symbol: Symbol,
        interner: &Interner,
        w: &mut CodeWriter,
    ) {
        w.write(surface.contextual_keyword(latin));
        w.write(" ");
        Self::write_quoted_symbol(symbol, surface, interner, w);
    }

    fn write_numeric_proba_token(surface: &KeywordSurface, latin: &'static str, value: i64, w: &mut CodeWriter) {
        w.write(surface.contextual_keyword(latin));
        w.write(" ");
        w.write(&value.to_string());
    }

    fn write_param(param: &Param, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(match param.mode {
            AstParamMode::Owned => "",
            AstParamMode::De => surface.contextual_keyword("de"),
            AstParamMode::In => surface.contextual_keyword("in"),
            AstParamMode::Ex => surface.contextual_keyword("ex"),
        });
        if !matches!(param.mode, AstParamMode::Owned) {
            w.write(" ");
        }
        if param.rest {
            w.write(surface.contextual_keyword("ceteri"));
            w.write(" ");
        }
        Self::write_type_expr(&param.ty, surface, interner, w);
        w.write(" ");
        Self::write_ident(&param.name, surface, interner, w);
        if param.sponte {
            w.write(" ");
            w.write(surface.contextual_keyword("sponte"));
        }
        if let Some(alias) = &param.alias {
            w.write(" ");
            w.write(surface.contextual_keyword("ut"));
            w.write(" ");
            Self::write_ident(alias, surface, interner, w);
        }
        if let Some(default) = &param.default {
            w.write(" ");
            w.write(surface.contextual_keyword("vel"));
            w.write(" ");
            Self::write_expr(default, surface, interner, w);
        }
    }

    fn write_func_modifier(modifier: &FuncModifier, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        match modifier {
            FuncModifier::Argumenta(name) => {
                w.write(surface.contextual_keyword("argumenta"));
                w.write(" ");
                Self::write_ident(name, surface, interner, w);
            }
            FuncModifier::Curata { required, alias } => {
                w.write(surface.contextual_keyword("curata"));
                w.write(" ");
                Self::write_ident(required, surface, interner, w);
                if let Some(alias) = alias {
                    w.write(" ");
                    w.write(surface.contextual_keyword("ut"));
                    w.write(" ");
                    Self::write_ident(alias, surface, interner, w);
                }
            }
            FuncModifier::Errata(name) => {
                w.write(surface.contextual_keyword("errata"));
                w.write(" ");
                Self::write_ident(name, surface, interner, w);
            }
            FuncModifier::Exitus(value) => {
                w.write(surface.contextual_keyword("exitus"));
                w.write(" ");
                match value {
                    ExitusValue::Name(name) => Self::write_ident(name, surface, interner, w),
                    ExitusValue::Number(n) => w.write(&n.to_string()),
                }
            }
            FuncModifier::Iacit => w.write(surface.contextual_keyword("iacit")),
            FuncModifier::Immutata => w.write(surface.contextual_keyword("immutata")),
            FuncModifier::Optiones(name) => {
                w.write(surface.contextual_keyword("optiones"));
                w.write(" ");
                Self::write_ident(name, surface, interner, w);
            }
        }
    }

    fn write_generic_params(params: &[TypeParam], surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        if params.is_empty() {
            return;
        }
        w.write("<");
        for (index, param) in params.iter().enumerate() {
            if index > 0 {
                w.write(", ");
            }
            match param.kind {
                GenericParamKind::Typus => Self::write_ident(&param.name, surface, interner, w),
                GenericParamKind::Magnitudo => {
                    w.write(surface.contextual_keyword("magnitudo"));
                    w.write(" ");
                    Self::write_ident(&param.name, surface, interner, w);
                }
            }
        }
        w.write(">");
    }

    fn write_annotation(annotation: &Annotation, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write("@ ");
        match &annotation.kind {
            AnnotationKind::Publica => w.write(surface.contextual_keyword("publica")),
            AnnotationKind::Protecta => w.write(surface.contextual_keyword("protecta")),
            AnnotationKind::Privata => w.write(surface.contextual_keyword("privata")),
            AnnotationKind::Interna => w.write(surface.contextual_keyword("interna")),
            AnnotationKind::Futura => w.write(surface.contextual_keyword("futura")),
            AnnotationKind::Cursor => w.write(surface.contextual_keyword("cursor")),
            AnnotationKind::Tag => w.write(surface.contextual_keyword("tag")),
            AnnotationKind::Solum => w.write(surface.contextual_keyword("solum")),
            AnnotationKind::Omitte => w.write(surface.contextual_keyword("omitte")),
            AnnotationKind::Metior => w.write(surface.contextual_keyword("metior")),
            AnnotationKind::Cli(cli) => {
                w.write(surface.contextual_keyword("cli"));
                w.write(" ");
                Self::write_quoted_symbol(cli.name, surface, interner, w);
            }
            AnnotationKind::Imperium(imperium) => {
                w.write(surface.contextual_keyword("imperium"));
                w.write(" ");
                Self::write_quoted_symbol(imperium.name, surface, interner, w);
            }
            AnnotationKind::Optio(optio) => Self::write_optio_annotation(optio, surface, interner, w),
            AnnotationKind::Operandus(operandus) => Self::write_operandus_annotation(operandus, surface, interner, w),
            AnnotationKind::RadixTypusDomain(domain) => {
                w.write(surface.contextual_keyword("radix"));
                w.write(" ");
                w.write(surface.contextual_keyword("typus"));
                w.write(" ");
                Self::write_ident(&domain.param, surface, interner, w);
                w.write(" ");
                w.write(surface.contextual_keyword("in"));
                for ty in &domain.allowed {
                    w.write(" ");
                    Self::write_type_expr(ty, surface, interner, w);
                }
            }
            AnnotationKind::RadixLane(lane) => {
                w.write(surface.contextual_keyword("radix"));
                w.write(" ");
                w.write(surface.contextual_keyword("lane"));
                w.write(" ");
                Self::write_quoted_symbol(lane.lane, surface, interner, w);
            }
            AnnotationKind::Nucleum(nucleum) => {
                w.write(surface.contextual_keyword("nucleum"));
                if nucleum.fragment {
                    w.write(" ");
                    w.write(surface.contextual_keyword("fragment"));
                }
            }
            AnnotationKind::Braced(braced) => {
                Self::write_ident(&braced.name, surface, interner, w);
                w.write(" {");
                for (index, field) in braced.fields.iter().enumerate() {
                    if index > 0 {
                        w.write(", ");
                    }
                    Self::write_ident(&field.name, surface, interner, w);
                    w.write(" = ");
                    match &field.value {
                        AnnotationFieldValue::Expr(expr) => Self::write_expr(expr, surface, interner, w),
                        AnnotationFieldValue::Type(ty) => Self::write_type_expr(ty, surface, interner, w),
                    }
                }
                w.write(" }");
            }
            AnnotationKind::Statement(stmt) => {
                Self::write_ident(&stmt.name, surface, interner, w);
                for token in &stmt.args {
                    w.write(" ");
                    Self::write_token(token, surface, interner, w);
                }
            }
        }
    }

    fn write_optio_annotation(
        optio: &OptioAnnotation,
        surface: &KeywordSurface,
        interner: &Interner,
        w: &mut CodeWriter,
    ) {
        w.write(surface.contextual_keyword("optio"));
        w.write(" ");
        Self::write_ident(&optio.binding, surface, interner, w);
        if let Some(ty) = &optio.ty {
            w.write(" ");
            w.write(surface.contextual_keyword("typus"));
            w.write(" ");
            Self::write_type_expr(ty, surface, interner, w);
        }
        if let Some(short) = optio.short {
            w.write(" ");
            w.write(surface.contextual_keyword("brevis"));
            w.write(" ");
            Self::write_quoted_symbol(short, surface, interner, w);
        }
        if let Some(long) = optio.long {
            w.write(" ");
            w.write(surface.contextual_keyword("longum"));
            w.write(" ");
            Self::write_quoted_symbol(long, surface, interner, w);
        }
        if let Some(description) = optio.description {
            w.write(" ");
            w.write(surface.contextual_keyword("descriptio"));
            w.write(" ");
            Self::write_quoted_symbol(description, surface, interner, w);
        }
        if optio.global {
            w.write(" ");
            w.write(surface.contextual_keyword("ubique"));
        }
        if let Some(default) = &optio.default {
            w.write(" ");
            w.write(surface.contextual_keyword("vel"));
            w.write(" ");
            Self::write_expr(default, surface, interner, w);
        }
    }

    fn write_operandus_annotation(
        operandus: &OperandusAnnotation,
        surface: &KeywordSurface,
        interner: &Interner,
        w: &mut CodeWriter,
    ) {
        w.write(surface.contextual_keyword("operandus"));
        if operandus.rest {
            w.write(" ");
            w.write(surface.contextual_keyword("ceteri"));
        }
        w.write(" ");
        Self::write_type_expr(&operandus.ty, surface, interner, w);
        w.write(" ");
        Self::write_ident(&operandus.binding, surface, interner, w);
        if let Some(description) = operandus.description {
            w.write(" ");
            w.write(surface.contextual_keyword("descriptio"));
            w.write(" ");
            Self::write_quoted_symbol(description, surface, interner, w);
        }
        if operandus.global {
            w.write(" ");
            w.write(surface.contextual_keyword("ubique"));
        }
        if let Some(default) = &operandus.default {
            w.write(" ");
            w.write(surface.contextual_keyword("vel"));
            w.write(" ");
            Self::write_expr(default, surface, interner, w);
        }
    }

    fn write_binding_pattern(
        pattern: &BindingPattern,
        surface: &KeywordSurface,
        interner: &Interner,
        w: &mut CodeWriter,
    ) {
        match pattern {
            BindingPattern::Ident(ident) => Self::write_ident(ident, surface, interner, w),
            BindingPattern::Wildcard(_) => w.write("_"),
            BindingPattern::Array { elements, rest, .. } => {
                w.write("[");
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        w.write(", ");
                    }
                    Self::write_binding_pattern(element, surface, interner, w);
                }
                if let Some(rest) = rest {
                    if !elements.is_empty() {
                        w.write(", ");
                    }
                    w.write(surface.contextual_keyword("ceteri"));
                    w.write(" ");
                    Self::write_ident(rest, surface, interner, w);
                }
                w.write("]");
            }
            BindingPattern::Object { fields, rest, .. } => {
                w.write("{");
                for (index, field) in fields.iter().enumerate() {
                    if index > 0 {
                        w.write(", ");
                    }
                    Self::write_ident(&field.name, surface, interner, w);
                    if let Some(alias) = &field.alias {
                        w.write(" ");
                        w.write(surface.contextual_keyword("ut"));
                        w.write(" ");
                        Self::write_ident(alias, surface, interner, w);
                    }
                }
                if let Some(rest) = rest {
                    if !fields.is_empty() {
                        w.write(", ");
                    }
                    w.write(surface.contextual_keyword("ceteri"));
                    w.write(" ");
                    Self::write_ident(rest, surface, interner, w);
                }
                w.write("}");
            }
        }
    }

    fn write_pattern(pattern: &Pattern, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        match pattern {
            Pattern::Wildcard(_) => w.write("_"),
            Pattern::Ident(ident, bind) => {
                Self::write_ident(ident, surface, interner, w);
                if let Some(bind) = bind {
                    Self::write_pattern_bind(bind, surface, interner, w);
                }
            }
            Pattern::Literal(lit, _) => Self::write_literal(lit, surface, interner, w),
            Pattern::Path(path) => {
                for (index, segment) in path.segments.iter().enumerate() {
                    if index > 0 {
                        w.write(".");
                    }
                    Self::write_ident(segment, surface, interner, w);
                }
                if let Some(bind) = &path.bind {
                    Self::write_pattern_bind(bind, surface, interner, w);
                }
            }
        }
    }

    fn write_pattern_bind(bind: &PatternBind, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        match bind {
            PatternBind::Alias(alias) => {
                w.write(" ");
                w.write(surface.contextual_keyword("ut"));
                w.write(" ");
                Self::write_ident(alias, surface, interner, w);
            }
            PatternBind::Bindings { mutability, bindings } => {
                w.write(" ");
                w.write(match mutability {
                    AstMutability::Immutable => surface.contextual_keyword("fixum"),
                    AstMutability::Mutable => surface.contextual_keyword("varia"),
                });
                for (index, field) in bindings.iter().enumerate() {
                    if index > 0 {
                        w.write(", ");
                    } else {
                        w.write(" ");
                    }
                    Self::write_ident(&field.name, surface, interner, w);
                    if let Some(alias) = &field.alias {
                        w.write(" ");
                        w.write(surface.contextual_keyword("ut"));
                        w.write(" ");
                        Self::write_ident(alias, surface, interner, w);
                    }
                }
            }
        }
    }

    fn write_type_expr(ty: &TypeExpr, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        if let Some(mode) = ty.mode {
            w.write(match mode {
                TypeMode::De => surface.contextual_keyword("de"),
                TypeMode::In => surface.contextual_keyword("in"),
            });
            w.write(" ");
        }
        match &ty.kind {
            TypeExprKind::Infer => w.write("_"),
            TypeExprKind::UnionHole => w.write("∪"),
            TypeExprKind::Named(name, params) => {
                Self::write_ident(name, surface, interner, w);
                if !params.is_empty() {
                    w.write("<");
                    for (index, param) in params.iter().enumerate() {
                        if index > 0 {
                            w.write(", ");
                        }
                        Self::write_type_expr(param, surface, interner, w);
                    }
                    w.write(">");
                }
            }
            TypeExprKind::Qualified(path, params) => {
                for (index, segment) in path.iter().enumerate() {
                    if index > 0 {
                        w.write(".");
                    }
                    Self::write_ident(segment, surface, interner, w);
                }
                if !params.is_empty() {
                    w.write("<");
                    for (index, param) in params.iter().enumerate() {
                        if index > 0 {
                            w.write(", ");
                        }
                        Self::write_type_expr(param, surface, interner, w);
                    }
                    w.write(">");
                }
            }
            TypeExprKind::Tensor(inner, figura) => {
                w.write(surface.contextual_keyword("tensor"));
                w.write("<");
                Self::write_type_expr(inner, surface, interner, w);
                w.write(", ");
                Self::write_figura(figura, surface, interner, w);
                w.write(">");
            }
            TypeExprKind::Vector(inner, figura) => {
                w.write(surface.contextual_keyword("vector"));
                w.write("<");
                Self::write_type_expr(inner, surface, interner, w);
                w.write(", ");
                Self::write_figura(figura, surface, interner, w);
                w.write(">");
            }
            TypeExprKind::Matrix(inner, figura) => {
                w.write(surface.contextual_keyword("matrix"));
                w.write("<");
                Self::write_type_expr(inner, surface, interner, w);
                w.write(", ");
                Self::write_figura(figura, surface, interner, w);
                w.write(">");
            }
            TypeExprKind::Sparsa(inner, figura) => {
                w.write(surface.contextual_keyword("sparsa"));
                w.write("<");
                Self::write_type_expr(inner, surface, interner, w);
                w.write(", ");
                Self::write_figura(figura, surface, interner, w);
                w.write(">");
            }
            TypeExprKind::Atomic(inner) => {
                w.write(surface.contextual_keyword("atomic"));
                w.write("<");
                Self::write_type_expr(inner, surface, interner, w);
                w.write(">");
            }
            TypeExprKind::PromissumFailable(success, alternate) => {
                w.write(surface.contextual_keyword("promissum"));
                w.write("<");
                Self::write_type_expr(success, surface, interner, w);
                w.write(" ⇥ ");
                Self::write_type_expr(alternate, surface, interner, w);
                w.write(">");
            }
            TypeExprKind::Func(func_ty) => {
                w.write("(");
                for (index, param) in func_ty.params.iter().enumerate() {
                    if index > 0 {
                        w.write(", ");
                    }
                    Self::write_type_expr(param, surface, interner, w);
                }
                w.write(") → ");
                Self::write_type_expr(&func_ty.ret, surface, interner, w);
                if let Some(err) = &func_ty.err {
                    w.write(" ⇥ ");
                    Self::write_type_expr(err, surface, interner, w);
                }
            }
            TypeExprKind::Union(members) => {
                for (index, member) in members.iter().enumerate() {
                    if index > 0 {
                        w.write(" ∪ ");
                    }
                    Self::write_type_expr(member, surface, interner, w);
                }
            }
        }
        // Emit legacy nullable marker so the field is not silently lost.
        // Modern nullables use `TypeExprKind::Union` with `nihil`.
        if ty.nullable {
            w.write(" ∪ ");
            w.write(surface.contextual_keyword("nihil"));
        }
    }

    fn write_figura(figura: &FiguraExpr, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        match figura {
            FiguraExpr::Infer => w.write("_"),
            FiguraExpr::Literal(n) => w.write(&n.to_string()),
            FiguraExpr::Var(ident) => Self::write_ident(ident, surface, interner, w),
            FiguraExpr::Tuple(items) => {
                w.write("[");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        w.write(", ");
                    }
                    Self::write_figura(item, surface, interner, w);
                }
                w.write("]");
            }
        }
    }

    fn write_expr(expr: &Expr, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        Self::write_expr_prec(expr, 0, surface, interner, w);
    }

    fn write_expr_prec(
        expr: &Expr,
        parent_prec: u8,
        surface: &KeywordSurface,
        interner: &Interner,
        w: &mut CodeWriter,
    ) {
        let expr_prec = Self::expr_precedence(expr);
        let needs_parens = expr_prec <= parent_prec && parent_prec != 0;
        if needs_parens {
            w.write("(");
        }

        match &expr.kind {
            ExprKind::Ident(ident) => Self::write_ident(ident, surface, interner, w),
            ExprKind::Literal(lit) => Self::write_literal(lit, surface, interner, w),
            ExprKind::Vacua(_) => w.write(surface.contextual_keyword("vacua")),
            ExprKind::Binary(binary) => {
                let op_prec = Self::binop_precedence(binary.op);
                if matches!(binary.op, BinOp::Is | BinOp::IsNot) {
                    Self::write_expr_prec(&binary.lhs, op_prec, surface, interner, w);
                    if matches!(binary.op, BinOp::IsNot) {
                        w.write(" ");
                        w.write(surface.contextual_keyword("non"));
                    }
                    w.write(" ");
                    w.write(surface.contextual_keyword("est"));
                    w.write(" ");
                    Self::write_expr_prec(&binary.rhs, op_prec, surface, interner, w);
                } else {
                    Self::write_expr_prec(&binary.lhs, op_prec, surface, interner, w);
                    w.write(" ");
                    w.write(Self::binop_to_faber(binary.op, surface));
                    w.write(" ");
                    Self::write_expr_prec(&binary.rhs, op_prec, surface, interner, w);
                }
            }
            ExprKind::Unary(unary) => {
                w.write(Self::unop_to_faber(unary.op, surface));
                if matches!(unary.op, UnOp::Not) {
                    w.write(" ");
                }
                Self::write_expr_prec(&unary.operand, 12, surface, interner, w);
            }
            ExprKind::Ternary(ternary) => match ternary.style {
                TernaryStyle::QuestionColon => {
                    Self::write_expr_prec(&ternary.cond, 2, surface, interner, w);
                    w.write(" ? ");
                    Self::write_expr_prec(&ternary.then, 2, surface, interner, w);
                    w.write(" : ");
                    Self::write_expr_prec(&ternary.else_, 2, surface, interner, w);
                }
                TernaryStyle::SicSecus => {
                    Self::write_expr_prec(&ternary.cond, 2, surface, interner, w);
                    w.write(" ");
                    w.write(surface.contextual_keyword("sic"));
                    w.write(" ");
                    Self::write_expr_prec(&ternary.then, 2, surface, interner, w);
                    w.write(" ");
                    w.write(surface.contextual_keyword("secus"));
                    w.write(" ");
                    Self::write_expr_prec(&ternary.else_, 2, surface, interner, w);
                }
            },
            ExprKind::Call(call) => {
                // WHY: callee Member/Index prec is 13; parent 13 would emit `(obj.method)()`.
                Self::write_expr_prec(&call.callee, 12, surface, interner, w);
                w.write("(");
                for (index, arg) in call.args.iter().enumerate() {
                    if index > 0 {
                        w.write(", ");
                    }
                    Self::write_argument(arg, surface, interner, w);
                }
                w.write(")");
            }
            ExprKind::Member(member) => {
                // WHY: object prec 12 keeps `a.b.c` flat; 13 would emit `(a.b).c`.
                Self::write_expr_prec(&member.object, 12, surface, interner, w);
                w.write(".");
                Self::write_ident(&member.member, surface, interner, w);
            }
            ExprKind::Index(index) => {
                // WHY: object prec 12 keeps `a[i][j]` / `a.b[i]` flat; 13 would
                // emit `(a[i])[j]`. After `scribe`/`print` that form re-parses
                // as a parenthesized call (`(print(a[i]))[j]`).
                Self::write_expr_prec(&index.object, 12, surface, interner, w);
                w.write("[");
                Self::write_expr(&index.index, surface, interner, w);
                w.write("]");
            }
            ExprKind::Gradient(gradient) => {
                // Canonical spaced form: one space on each side of `∇` when a
                // selector list follows (mirrors the pretty emitter).
                Self::write_expr_prec(&gradient.call, 12, surface, interner, w);
                w.write(" ∇");
                if !gradient.selectors.is_empty() {
                    w.write(" [");
                    for (index, selector) in gradient.selectors.iter().enumerate() {
                        if index > 0 {
                            w.write(", ");
                        }
                        Self::write_expr(selector, surface, interner, w);
                    }
                    w.write("]");
                }
            }
            ExprKind::OptionalChain(chain) => {
                // WHY: object prec 12 keeps `a?.b?.c` flat; 13 would emit
                // `(a?.b)?.c`, which after `scribe`/`print` re-parses as a call.
                Self::write_expr_prec(&chain.object, 12, surface, interner, w);
                match &chain.chain {
                    OptionalChainKind::Member(member) => {
                        w.write("?.");
                        Self::write_ident(member, surface, interner, w);
                    }
                    OptionalChainKind::Index(index) => {
                        w.write("?[");
                        Self::write_expr(index, surface, interner, w);
                        w.write("]");
                    }
                    OptionalChainKind::Call(args) => {
                        w.write("?(");
                        for (index, arg) in args.iter().enumerate() {
                            if index > 0 {
                                w.write(", ");
                            }
                            Self::write_argument(arg, surface, interner, w);
                        }
                        w.write(")");
                    }
                }
            }
            ExprKind::NonNull(nonnull) => {
                // WHY: same postfix-chain rule as Index / OptionalChain.
                Self::write_expr_prec(&nonnull.object, 12, surface, interner, w);
                match &nonnull.chain {
                    NonNullKind::Member(member) => {
                        w.write("!.");
                        Self::write_ident(member, surface, interner, w);
                    }
                    NonNullKind::Index(index) => {
                        w.write("![");
                        Self::write_expr(index, surface, interner, w);
                        w.write("]");
                    }
                    NonNullKind::Call(args) => {
                        w.write("!(");
                        for (index, arg) in args.iter().enumerate() {
                            if index > 0 {
                                w.write(", ");
                            }
                            Self::write_argument(arg, surface, interner, w);
                        }
                        w.write(")");
                    }
                }
            }
            ExprKind::Assign(assign) => {
                Self::write_expr_prec(&assign.target, 1, surface, interner, w);
                match assign.op {
                    AssignOp::Assign => w.write(" ← "),
                    AssignOp::ConversioAssign => w.write(" ↤ "),
                }
                Self::write_expr_prec(&assign.value, 1, surface, interner, w);
                if let Some(recovery) = &assign.recovery {
                    // `⇥ recovery` is `inlineRecovery := '⇥' unary`: unary is
                    // the tightest binding, so the recovery operand must keep
                    // parens around anything looser than unary to reparse
                    // attached to the nearest `↤`, never to the outer level.
                    w.write(" ⇥ ");
                    Self::write_expr_prec(recovery, 12, surface, interner, w);
                }
            }
            ExprKind::Verte(verte) => {
                if !Self::write_typed_object_constructor(verte, surface, interner, w) {
                    Self::write_expr_prec(&verte.expr, 12, surface, interner, w);
                    w.write(" ∷ ");
                    Self::write_type_expr(&verte.ty, surface, interner, w);
                }
            }
            ExprKind::Finge(finge) => {
                w.write(surface.contextual_keyword("finge"));
                w.write(" ");
                Self::write_ident_path(&finge.variant, surface, interner, w);
                if !finge.fields.is_empty() {
                    w.write(" {");
                    for (index, field) in finge.fields.iter().enumerate() {
                        if index > 0 {
                            w.write(", ");
                        } else {
                            w.write(" ");
                        }
                        Self::write_ident(&field.name, surface, interner, w);
                        w.write(" = ");
                        Self::write_expr(&field.value, surface, interner, w);
                    }
                    w.write(" }");
                }
                if let Some(cast) = &finge.cast {
                    w.write(" ∷ ");
                    Self::write_type_expr(cast, surface, interner, w);
                }
            }
            ExprKind::Clausura(clausura) => Self::write_clausura(clausura, surface, interner, w),
            ExprKind::Cede(cede) => {
                w.write(surface.contextual_keyword("cede"));
                w.write(" ");
                Self::write_expr(&cede.expr, surface, interner, w);
            }
            ExprKind::Array(array) => {
                w.write("[");
                for (index, element) in array.elements.iter().enumerate() {
                    if index > 0 {
                        w.write(", ");
                    }
                    match element {
                        ArrayElement::Expr(expr) => Self::write_expr(expr, surface, interner, w),
                        ArrayElement::Spread(expr) => {
                            w.write(surface.contextual_keyword("sparge"));
                            w.write(" ");
                            Self::write_expr(expr, surface, interner, w);
                        }
                    }
                }
                w.write("]");
            }
            ExprKind::Object(object) => {
                w.write("{");
                if !object.fields.is_empty() {
                    w.write(" ");
                }
                for (index, field) in object.fields.iter().enumerate() {
                    if index > 0 {
                        w.write(", ");
                    }
                    Self::write_object_field(field, surface, interner, w);
                }
                if !object.fields.is_empty() {
                    w.write(" ");
                }
                w.write("}");
            }
            ExprKind::Intervallum(range) => {
                Self::write_expr_prec(&range.start, 6, surface, interner, w);
                w.write(match range.kind {
                    RangeKind::Exclusive => "‥",
                    RangeKind::Inclusive => "…",
                });
                Self::write_expr_prec(&range.end, 6, surface, interner, w);
                if let Some(step) = &range.step {
                    w.write(" ");
                    w.write(surface.contextual_keyword("per"));
                    w.write(" ");
                    Self::write_expr(step, surface, interner, w);
                }
            }
            ExprKind::Conversio(conversio) => Self::write_conversio(conversio, surface, interner, w),
            ExprKind::Scriptum(scriptum) => {
                w.write(surface.contextual_keyword("scriptum"));
                w.write(" ");
                Self::write_quoted_symbol(scriptum.template, surface, interner, w);
                for arg in &scriptum.args {
                    w.write(", ");
                    Self::write_expr(arg, surface, interner, w);
                }
            }
            ExprKind::Ad(ad) => {
                w.write(surface.contextual_keyword("ad"));
                w.write(" ");
                Self::write_literal(&Literal::Ascii(ad.path), surface, interner, w);
                if let Some(opener) = &ad.opener {
                    w.write(" (");
                    Self::write_expr(opener, surface, interner, w);
                    w.write(")");
                }
            }
            ExprKind::Lege(lege) => {
                w.write(surface.contextual_keyword("lege"));
                if lege.line {
                    w.write(" ");
                    w.write(surface.contextual_keyword("lineam"));
                }
            }
            ExprKind::Praefixum(praefixum) => {
                w.write(surface.contextual_keyword("praefixum"));
                w.write("(");
                match &praefixum.body {
                    PraefixumBody::Block(block) => Self::write_block(block, surface, interner, w),
                    PraefixumBody::Expr(expr) => Self::write_expr(expr, surface, interner, w),
                }
                w.write(")");
            }
            ExprKind::Ego(_) => w.write(surface.contextual_keyword("ego")),
            ExprKind::Paren(inner) => {
                w.write("(");
                Self::write_expr(inner, surface, interner, w);
                w.write(")");
            }
            ExprKind::TypeCheck(type_check) => {
                Self::write_expr_prec(&type_check.expr, 5, surface, interner, w);
                if !type_check.positive {
                    w.write(" ");
                    w.write(surface.contextual_keyword("non"));
                }
                w.write(" ");
                w.write(surface.contextual_keyword("est"));
                w.write(" ");
                Self::write_type_expr(&type_check.ty, surface, interner, w);
            }
            ExprKind::Iuncta(iuncta) => {
                w.write(surface.contextual_keyword("iuncta"));
                w.write("<");
                for (i, ty) in iuncta.type_args.iter().enumerate() {
                    if i > 0 {
                        w.write(", ");
                    }
                    Self::write_type_expr(ty, surface, interner, w);
                }
                w.write("> [");
                for (i, expr) in iuncta.elements.iter().enumerate() {
                    if i > 0 {
                        w.write(", ");
                    }
                    Self::write_expr(expr, surface, interner, w);
                }
                w.write("]");
            }
        }

        if needs_parens {
            w.write(")");
        }
    }

    fn write_clausura(clausura: &ClausuraExpr, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        if clausura.params.len() == 1 {
            let param = &clausura.params[0];
            Self::write_type_expr(&param.ty, surface, interner, w);
            w.write(" ");
            Self::write_ident(&param.name, surface, interner, w);
        } else {
            w.write("(");
            for (index, param) in clausura.params.iter().enumerate() {
                if index > 0 {
                    w.write(", ");
                }
                Self::write_type_expr(&param.ty, surface, interner, w);
                w.write(" ");
                Self::write_ident(&param.name, surface, interner, w);
            }
            w.write(")");
        }
        if let Some(ret) = &clausura.ret {
            w.write(" → ");
            Self::write_type_expr(ret, surface, interner, w);
        }
        if let Some(err) = &clausura.err {
            w.write(" ⇥ ");
            Self::write_type_expr(err, surface, interner, w);
        }
        w.write(" ∴ ");
        match &clausura.body {
            ClausuraBody::Expr(expr) => Self::write_expr(expr, surface, interner, w),
            ClausuraBody::Block(block) => Self::write_block(block, surface, interner, w),
            ClausuraBody::Fac(fac) => Self::write_fac(fac, surface, interner, w),
        }
    }

    fn write_conversio(conversio: &ConversioExpr, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        Self::write_expr_prec(&conversio.expr, 2, surface, interner, w);
        w.write(" ↦ ");
        match &conversio.target {
            ConversioTarget::Explicit(ty) => Self::write_type_expr(ty, surface, interner, w),
            ConversioTarget::Intervallum(expr) => Self::write_expr(expr, surface, interner, w),
        }
        if !conversio.type_params.is_empty() {
            w.write("<");
            for (index, type_param) in conversio.type_params.iter().enumerate() {
                if index > 0 {
                    w.write(", ");
                }
                Self::write_type_expr(type_param, surface, interner, w);
            }
            w.write(">");
        }
        if let Some(recovery) = &conversio.recovery {
            w.write(" ⇥ ");
            Self::write_expr_prec(recovery, 2, surface, interner, w);
        }
    }

    /// Emit `Type { field = value }` sugar so an ascribe wrapping a genus
    /// constructor re-parses as the constructor, not `expr ∷ Type`.
    fn write_typed_object_constructor(
        verte: &VerteExpr,
        surface: &KeywordSurface,
        interner: &Interner,
        w: &mut CodeWriter,
    ) -> bool {
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
                    w.write(".");
                }
                Self::write_ident(segment, surface, interner, w);
            }
        } else {
            Self::write_ident(name, surface, interner, w);
        }
        w.write(" {");
        if !object.fields.is_empty() {
            w.write(" ");
        }
        for (index, field) in object.fields.iter().enumerate() {
            if index > 0 {
                w.write(", ");
            }
            Self::write_object_field(field, surface, interner, w);
        }
        if !object.fields.is_empty() {
            w.write(" ");
        }
        w.write("}");
        true
    }

    fn write_object_field(field: &ObjectField, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        match &field.key {
            ObjectKey::Ident(ident) => Self::write_ident(ident, surface, interner, w),
            ObjectKey::String(sym) => Self::write_quoted_symbol(*sym, surface, interner, w),
            ObjectKey::Computed(expr) => {
                w.write("[");
                Self::write_expr(expr, surface, interner, w);
                w.write("]");
            }
            ObjectKey::Spread(expr) => {
                w.write(surface.contextual_keyword("sparge"));
                w.write(" ");
                Self::write_expr(expr, surface, interner, w);
                return;
            }
        }
        if let Some(value) = &field.value {
            w.write(" = ");
            Self::write_expr(value, surface, interner, w);
        }
    }

    fn write_argument(arg: &Argument, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        if arg.spread {
            w.write(surface.contextual_keyword("sparge"));
            w.write(" ");
        }
        Self::write_expr(&arg.value, surface, interner, w);
    }

    fn write_literal(lit: &Literal, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        match lit {
            Literal::Int(n) => w.write(&n.to_string()),
            Literal::Float(f) => w.write(&Self::format_float(*f)),
            Literal::String(sym) => Self::write_textus_literal(*sym, surface, interner, w),
            Literal::Ascii(sym) => {
                w.write("'");
                w.write(interner.resolve(*sym));
                w.write("'");
            }
            Literal::Forma(sym) => {
                w.write("`");
                w.write_raw(interner.resolve(*sym));
                w.write("`");
            }
            Literal::Octeti(sym) => {
                w.write("|");
                w.write(interner.resolve(*sym));
                w.write("|");
            }
            Literal::JsonValor(value) => Self::write_json_value(value, surface, interner, w),
            Literal::Bool(true) => w.write(surface.contextual_keyword("verum")),
            Literal::Bool(false) => w.write(surface.contextual_keyword("falsum")),
            Literal::Nil => w.write(surface.contextual_keyword("nihil")),
        }
    }

    fn write_json_value(value: &JsonValue, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        match value {
            JsonValue::Object(members) => {
                w.write("{");
                if !members.is_empty() {
                    w.write(" ");
                }
                for (index, member) in members.iter().enumerate() {
                    if index > 0 {
                        w.write(", ");
                    }
                    Self::write_quoted_symbol(member.key, surface, interner, w);
                    w.write(": ");
                    Self::write_json_value(&member.value, surface, interner, w);
                }
                if !members.is_empty() {
                    w.write(" ");
                }
                w.write("}");
            }
            JsonValue::Array(elements) => {
                w.write("[");
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        w.write(", ");
                    }
                    Self::write_json_value(element, surface, interner, w);
                }
                w.write("]");
            }
            JsonValue::String(sym) => Self::write_quoted_symbol(*sym, surface, interner, w),
            JsonValue::Numerus(n) => w.write(&n.to_string()),
            JsonValue::Fractus(f) => w.write(&Self::format_float(*f)),
            JsonValue::Bool(true) => w.write("true"),
            JsonValue::Bool(false) => w.write("false"),
            JsonValue::Null => w.write("null"),
        }
    }

    fn write_textus_literal(sym: Symbol, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        let text = interner.resolve(sym);
        if text.contains('\n') || text.contains('"') {
            w.write("«");
            w.write_raw(text);
            w.write("»");
        } else {
            Self::write_quoted_symbol(sym, surface, interner, w);
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

    fn write_ident(ident: &Ident, _surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write(interner.resolve(ident.name));
    }

    fn write_ident_path(path: &[Ident], _surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        for (index, segment) in path.iter().enumerate() {
            if index > 0 {
                w.write(".");
            }
            w.write(interner.resolve(segment.name));
        }
    }

    fn write_quoted_symbol(sym: Symbol, _surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        w.write("\"");
        w.write(interner.resolve(sym));
        w.write("\"");
    }

    fn write_token(token: &Token, surface: &KeywordSurface, interner: &Interner, w: &mut CodeWriter) {
        if keyword_spelling_for_token(&token.kind).is_some() {
            w.write(surface.keyword(token.kind.clone()));
            return;
        }
        match &token.kind {
            TokenKind::Ident(sym) | TokenKind::Underscore(sym) => w.write(interner.resolve(*sym)),
            TokenKind::Integer(n) => w.write(&n.to_string()),
            TokenKind::Float(f) => w.write(&Self::format_float(*f)),
            TokenKind::String(sym) => Self::write_quoted_symbol(*sym, surface, interner, w),
            TokenKind::AsciiString(sym) => {
                w.write("'");
                w.write(interner.resolve(*sym));
                w.write("'");
            }
            TokenKind::BacktickString(sym) => {
                w.write("`");
                w.write(interner.resolve(*sym));
                w.write("`");
            }
            TokenKind::OctetiString(sym) => {
                w.write("|");
                w.write(interner.resolve(*sym));
                w.write("|");
            }
            TokenKind::LParen => w.write("("),
            TokenKind::RParen => w.write(")"),
            TokenKind::LBrace => w.write("{"),
            TokenKind::RBrace => w.write("}"),
            TokenKind::LBracket => w.write("["),
            TokenKind::RBracket => w.write("]"),
            TokenKind::Comma => w.write(","),
            TokenKind::Colon => w.write(":"),
            TokenKind::Semicolon => w.write(";"),
            TokenKind::Dot => w.write("."),
            TokenKind::Arrow => w.write("→"),
            TokenKind::ExitArrow => w.write("⇥"),
            TokenKind::Cup => w.write("∪"),
            TokenKind::At => w.write("@"),
            TokenKind::Plus => w.write("+"),
            TokenKind::Minus => w.write("-"),
            TokenKind::Star => w.write("*"),
            TokenKind::Slash => w.write("/"),
            TokenKind::Percent => w.write("%"),
            TokenKind::BitwiseAnd => w.write("∧"),
            TokenKind::BitwiseOr => w.write("∨"),
            TokenKind::BitwiseXor => w.write("⊻"),
            TokenKind::BitwiseNot => w.write("¬"),
            TokenKind::BitwiseShl => w.write("⇐"),
            TokenKind::BitwiseShr => w.write("⇒"),
            TokenKind::Bang => w.write("!"),
            TokenKind::Question => w.write("?"),
            TokenKind::Eq => w.write("="),
            TokenKind::Assign => w.write("←"),
            TokenKind::ConversioAssign => w.write("↤"),
            TokenKind::EqEq => w.write("≡"),
            TokenKind::ApproxEq => w.write("≈"),
            TokenKind::ApproxNotEq => w.write("≉"),
            TokenKind::BangEq => w.write("≠"),
            TokenKind::Lt => w.write("≺"),
            TokenKind::Gt => w.write("≻"),
            TokenKind::LtAscii => w.write("<"),
            TokenKind::GtAscii => w.write(">"),
            TokenKind::LtEq => w.write("≤"),
            TokenKind::GtEq => w.write("≥"),
            TokenKind::PostInc => w.write("↑"),
            TokenKind::PostDec => w.write("↓"),
            TokenKind::QuestionDot => w.write("?."),
            TokenKind::QuestionBracket => w.write("?["),
            TokenKind::QuestionParen => w.write("?("),
            TokenKind::BangDot => w.write("!."),
            TokenKind::BangBracket => w.write("!["),
            TokenKind::BangParen => w.write("!("),
            TokenKind::DotDot => w.write("‥"),
            TokenKind::Ellipsis => w.write("…"),
            TokenKind::Therefore => w.write("∴"),
            TokenKind::LineComment(sym) => {
                w.write("#");
                w.write(interner.resolve(*sym));
            }
            _ => w.write("?"),
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
            // Local nullable elimination: tighter than arithmetic.
            BinOp::Coalesce => 12,
        }
    }

    fn binop_to_faber<'a>(op: BinOp, surface: &KeywordSurface<'a>) -> &'a str {
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
            BinOp::And => surface.contextual_keyword("et"),
            BinOp::Or => surface.contextual_keyword("aut"),
            BinOp::Coalesce => surface.contextual_keyword("vel"),
            BinOp::BitAnd => "∧",
            BinOp::BitOr => "∨",
            BinOp::BitXor => "⊻",
            BinOp::Shl => "⇐",
            BinOp::Shr => "⇒",
            BinOp::Is => surface.contextual_keyword("est"),
            BinOp::IsNot => surface.contextual_keyword("non est"),
            BinOp::InRange => surface.contextual_keyword("intra"),
            BinOp::Between => surface.contextual_keyword("inter"),
        }
    }

    fn unop_to_faber<'a>(op: UnOp, surface: &KeywordSurface<'a>) -> &'a str {
        match op {
            UnOp::Neg => "-",
            UnOp::Not => surface.contextual_keyword("non"),
            UnOp::BitNot => "¬",
        }
    }
}

/// Emit author-mode Faber source from a parsed program.
#[must_use]
pub fn emit(program: &Program, surface: &KeywordSurface, interner: &Interner, options: &FormatOptions) -> FaberOutput {
    AuthorEmitter::emit(program, surface, interner, options)
}

#[cfg(test)]
#[path = "emit_test.rs"]
mod tests;
