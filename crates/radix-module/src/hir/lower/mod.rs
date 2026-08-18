//! Pass 3 AST-to-HIR lowering for compiler-friendly program structure.
//!
//! This module is the boundary between parsed, name-resolved syntax and the
//! High-level Intermediate Representation consumed by type checking, analysis,
//! and backend code generation. It preserves source spans and resolver `DefId`s
//! where they already exist, creates synthetic ids for bindings that only become
//! concrete during lowering, and normalizes entrypoint/declaration layout so
//! later passes do not need to understand every surface syntax form.
//!
//! HIR lowering is intentionally not a semantic pass. It resolves the shape of
//! declarations, statements, local scopes, and desugared control-flow nodes, but
//! leaves type compatibility, exhaustiveness, reachability, and most target
//! legality to the semantic/typecheck passes that have the full HIR graph.
//!
//! ERROR STRATEGY
//! ==============
//! Lowering reports structural problems and continues whenever possible. Invalid
//! expressions become `HirExpressionKind::Error`, lowering-created bindings receive
//! synthetic `DefId`s, and every produced node keeps the best source span known
//! at that point. This lets later diagnostics report more than the first
//! lowering issue without guessing about source locations.
//!
//! INVARIANTS
//! ==========
//! - Top-level declarations lower into `HirModule::items`; executable
//!   top-level statements lower into an explicit or implicit entry block.
//! - Resolver-owned names keep their resolver `DefId`; lowering-created locals,
//!   catch bindings, pattern bindings, and CLI bindings receive synthetic ids.
//! - The lowerer scope stack only tracks function/block-local names. Module and
//!   item lookup stays with the resolver.
//! - HIR construction preserves AST spans, falling back to the current lowering
//!   span only for nodes synthesized from a larger source form.
//!
//! MODULE SHAPE
//! ============
//! Declaration, statement, expression, pattern, and type lowering live in
//! focused sibling modules because each owns a different compiler contract:
//! declarations build item-level HIR, statements normalize executable flow,
//! expressions preserve evaluation shape, patterns bind names, and types map
//! parsed type syntax into the semantic type table.

mod annotation;
mod decl;
mod expr;
mod literal;
mod pattern;
mod stmt;
mod types;

use super::{
    HirBlock, HirBlockEnd, HirExpression, HirId, HirItem, HirModule, HirModuleEnd, HirOwnerRef, HirPresentation,
    HirSourceAnchorId, HirStatement, HirStatementKind, HirTestMetadata, HirTestModifier, HirTrivia,
    HirTriviaAttachment, HirTriviaOwner, HirTriviaOwnerKind,
};
use crate::cli::{CliProgram, CliType};
use crate::diagnostics::DiagnosticArg;
use crate::lexer::{Interner, Span, Symbol};
use crate::locale::LocalePack;
use crate::semantic::{AnnotationContractMetadata, Primitive, Resolver, Type, TypeId, TypeTable};
use crate::syntax::{
    BlockStmt, GenericParamKind, PraeparaBlock, PraeparaKind, ProbaCase, ProbaModifier, ProbandumDecl, Program, Stmt,
    StmtKind, Trivia,
};
use rustc_hash::{FxHashMap, FxHashSet};

/// Stateful coordinator for one AST-to-HIR lowering run.
///
/// A `Lowerer` borrows the resolver and type table for the duration of one
/// program. The resolver remains the source of truth for item-level symbols,
/// while this type owns per-run HIR ids, synthetic local `DefId`s, collected
/// lowering diagnostics, local scopes, and optional CLI metadata that can shape
/// generated entrypoint argument records.
pub struct Lowerer<'a> {
    /// Resolver produced by the previous compiler pass.
    resolver: &'a mut Resolver,

    /// Shared semantic type table used when lowering type syntax and CLI records.
    types: &'a mut TypeTable,

    /// Symbol interner used for annotation and diagnostic-sensitive spelling.
    interner: &'a mut Interner,

    /// Next per-HIR-node id assigned during this lowering run.
    next_id: u32,

    /// Next presentation source-anchor id assigned during this lowering run.
    ///
    /// A dedicated namespace, never overloaded with `HirId`: anchors identify
    /// legal trivia attachment boundaries, not semantic nodes.
    next_anchor_id: u32,

    /// Presentation sidecar accumulated while lowering: typed owners, ordered
    /// attachments, structural block ends, the program end, and the explicit
    /// entry-start anchor. Drained into `LoweredProgram` at the end of the run.
    presentation: HirPresentation,

    /// Next synthetic definition id for bindings that did not exist in resolver output.
    next_def_id: u32,

    /// Recoverable lowering diagnostics collected instead of aborting the pass.
    errors: Vec<LowerError>,

    /// Best source span for diagnostics emitted by helpers without a narrower span.
    current_span: Span,

    /// Stack of function/block-local bindings visible only during lowering.
    local_scopes: Vec<FxHashMap<Symbol, crate::hir::DefId>>,

    /// Lexical generic-parameter names visible while lowering signatures.
    type_param_scopes: Vec<FxHashMap<Symbol, GenericParamKind>>,

    /// Current self type for lowering `ego` expressions inside methods.
    current_ego_struct: Option<crate::hir::DefId>,

    /// Synthetic module import bindings created by `importa ex "faber:*"`.
    kernel_glob_imports: FxHashMap<(Symbol, Symbol), crate::hir::DefId>,

    /// Aliases introduced by `importa ex "faber:*"`.
    kernel_glob_aliases: FxHashSet<Symbol>,

    /// Validated CLI contract, when the entry point is a runnable CLI.
    cli_program: Option<&'a CliProgram>,

    /// Active reader pack for type-position source spelling bindings.
    locale_pack: Option<&'a LocalePack>,

    /// Resolved annotation-contract applications from the semantic contract pass.
    annotation_contracts: Option<&'a AnnotationContractMetadata>,
}

/// Recoverable diagnostic emitted while converting resolved AST into HIR.
///
/// Lowering errors describe shape problems, unsupported constructs, or missing
/// context discovered before type checking. They carry stable facts so semantic
/// diagnostic rendering owns human-readable prose.
#[derive(Debug, Clone)]
pub struct LowerError {
    /// Named facts for catalog templates and future locale renderers.
    pub args: Vec<DiagnosticArg>,

    /// Source span associated with the problematic AST or synthesized node.
    pub span: Span,
}

impl<'a> Lowerer<'a> {
    /// Create a lowerer for one program.
    ///
    /// `cli_program` is optional because ordinary file compilation and package
    /// CLI compilation share the same lowering pipeline. When present, it is
    /// used only to type synthetic entrypoint/command argument records.
    pub fn new(
        resolver: &'a mut Resolver,
        types: &'a mut TypeTable,
        interner: &'a mut Interner,
        cli_program: Option<&'a CliProgram>,
        locale_pack: Option<&'a LocalePack>,
    ) -> Self {
        Self::new_with_contracts(resolver, types, interner, cli_program, locale_pack, None)
    }

    /// Create a lowerer that attaches resolved annotation-contract applications to HIR.
    pub fn new_with_contracts(
        resolver: &'a mut Resolver,
        types: &'a mut TypeTable,
        interner: &'a mut Interner,
        cli_program: Option<&'a CliProgram>,
        locale_pack: Option<&'a LocalePack>,
        annotation_contracts: Option<&'a AnnotationContractMetadata>,
    ) -> Self {
        Self {
            resolver,
            types,
            interner,
            next_id: 0,
            next_anchor_id: 0,
            presentation: HirPresentation {
                owners: Vec::new(),
                attachments: Vec::new(),
                block_ends: Vec::new(),
                program_end: HirModuleEnd { id: HirSourceAnchorId(0), span: Span::default() },
                entry_start_anchor: None,
            },
            next_def_id: 1_000_000,
            errors: Vec::new(),
            current_span: Span::default(),
            local_scopes: Vec::new(),
            type_param_scopes: Vec::new(),
            current_ego_struct: None,
            kernel_glob_imports: FxHashMap::default(),
            kernel_glob_aliases: FxHashSet::default(),
            cli_program,
            locale_pack,
            annotation_contracts,
        }
    }

    /// Enter a lexical scope of generic-parameter names for signature lowering.
    pub(super) fn push_type_params(&mut self, params: impl IntoIterator<Item = (Symbol, GenericParamKind)>) {
        let mut layer = FxHashMap::default();
        for (name, kind) in params {
            layer.insert(name, kind);
        }
        self.type_param_scopes.push(layer);
    }

    /// Leave the innermost type-parameter scope.
    pub(super) fn pop_type_params(&mut self) {
        self.type_param_scopes.pop();
    }

    pub(super) fn is_lexical_type_param(&self, name: Symbol) -> bool {
        self.lexical_generic_param_kind(name)
            .is_some_and(|kind| kind == GenericParamKind::Typus)
    }

    pub(super) fn lexical_generic_param_kind(&self, name: Symbol) -> Option<GenericParamKind> {
        self.type_param_scopes
            .iter()
            .rev()
            .find_map(|layer| layer.get(&name).copied())
    }

    /// Lower a complete resolved syntax program into HIR.
    ///
    /// The program boundary is where declaration and entrypoint partitioning is
    /// established. Explicit `incipit` owns `HirModule::entry`; otherwise
    /// executable top-level statements become an implicit entry block, while
    /// top-level declarations stay as items. Later passes can therefore treat
    /// HIR like a target-language module with a separate runnable body.
    pub fn lower_program(&mut self, program: &Program) -> HirModule {
        self.collect_kernel_glob_imports(program);

        let mut items = Vec::new();
        let mut entry = None;
        let mut entry_annotations = Vec::new();
        let mut entry_args_name = None;
        let mut entry_is_async = false;
        let mut implicit_entry_stmts = Vec::new();
        let mut implicit_entry_scope = false;

        for stmt in &program.statements {
            self.current_span = stmt.span;

            if let StmtKind::Incipit(entry_stmt) = &stmt.kind {
                // Explicit entry (`incipit`) leading trivia renders at the
                // entry-start owner; the entry block itself has no semantic
                // node id, so the owner carries no back-reference.
                let entry_trivia = self.hir_trivia(&stmt.leading_trivia);
                if let Some(anchor) = self.register_attachment(HirTriviaOwnerKind::Entry, None, entry_trivia) {
                    self.presentation.entry_start_anchor = Some(anchor);
                }
                entry_annotations = self.lower_annotations(&stmt.annotations);
                entry_args_name = entry_stmt.args.as_ref().map(|ident| ident.name);
                entry_is_async = entry_stmt.is_async;
                // `incipit` introduces an entry-local scope and may synthesize
                // a CLI args local before the user-authored body statements.
                self.push_scope();
                let mut args_binding = None;
                if let Some(args) = &entry_stmt.args {
                    let def_id = self.next_def_id();
                    self.bind_local(args.name, def_id);
                    args_binding = Some((args.name, args.span, def_id));
                }
                let mut block = self.lower_ergo_body(&entry_stmt.body);
                if let Some((name, span, def_id)) = args_binding {
                    block.statements.insert(
                        0,
                        HirStatement {
                            id: self.next_hir_id(),
                            kind: HirStatementKind::Local(crate::hir::HirLocal {
                                def_id,
                                name,
                                ty: Some(self.incipit_args_type()),
                                init: None,
                                mutable: false,
                                is_await: false,
                                runtime_provided: true,
                            }),
                            span,
                        },
                    );
                }
                // Preserve exitus as the tail expression of the entry block.
                if let Some(exitus) = &entry_stmt.exitus {
                    let exitus_expr = self.lower_expr(exitus);
                    block.expr = Some(Box::new(exitus_expr));
                }
                self.pop_scope();
                entry = Some(block);
            } else {
                let lowered_items = self.lower_stmt_items(stmt);
                if lowered_items.is_empty() {
                    if !implicit_entry_scope {
                        self.push_scope();
                        implicit_entry_scope = true;
                    }
                    let lowered = stmt::lower_stmt(self, stmt);
                    self.attach_stmt_trivia(stmt, std::slice::from_ref(&lowered));
                    implicit_entry_stmts.push(lowered);
                } else {
                    self.attach_item_trivia(stmt, &lowered_items);
                    items.extend(lowered_items);
                }
            }
        }

        if implicit_entry_scope {
            self.pop_scope();
        }

        if entry.is_none() && !implicit_entry_stmts.is_empty() {
            let span = implicit_entry_stmts
                .first()
                .map_or_else(Span::default, |stmt| stmt.span);
            entry = Some(HirBlock::simple(implicit_entry_stmts, None, span));
        }

        // Program-end boundary: trivia before EOF (including a comment-only
        // file) attaches to the single program-end owner.
        let program_end_id = self.next_anchor();
        self.presentation.program_end = HirModuleEnd { id: program_end_id.clone(), span: program.span };
        let end_trivia = self.hir_trivia(&program.end_trivia);
        self.register_attachment_at(program_end_id, HirTriviaOwnerKind::ProgramEnd, None, end_trivia);

        crate::builtins::inject_hir_items(&mut items, program, self.resolver, self.types, self.interner);

        HirModule { items, entry, entry_annotations, entry_args_name, entry_is_async }
    }

    /// Drain diagnostics accumulated during the lowering run.
    pub fn take_errors(&mut self) -> Vec<LowerError> {
        std::mem::take(&mut self.errors)
    }

    /// Allocate a per-node HIR id.
    fn next_hir_id(&mut self) -> HirId {
        let id = self.next_id;
        self.next_id += 1;
        HirId(id)
    }

    /// Allocate a synthetic `DefId` for a binding introduced during lowering.
    pub(super) fn next_def_id(&mut self) -> crate::hir::DefId {
        let id = self.next_def_id;
        self.next_def_id += 1;
        crate::hir::DefId(id)
    }

    /// Allocate the next presentation source-anchor id.
    ///
    /// Anchors live in a dedicated namespace (never `HirId`); each legal
    /// attachment boundary in one analyzed unit receives one stable anchor.
    fn next_anchor(&mut self) -> HirSourceAnchorId {
        let id = self.next_anchor_id;
        self.next_anchor_id += 1;
        HirSourceAnchorId(id)
    }

    /// Convert syntax trivia into HIR presentation trivia, preserving order.
    fn hir_trivia(&self, trivia: &[Trivia]) -> Vec<HirTrivia> {
        trivia
            .iter()
            .map(|item| match item {
                Trivia::CommentLine(comment) => HirTrivia::CommentLine { text: comment.text, span: comment.span },
                Trivia::Newline(newline) => HirTrivia::Newline { span: newline.span },
            })
            .collect()
    }

    /// Register a typed owner plus its ordered attachment at a fresh anchor.
    ///
    /// No owner record is created for an empty trivia list: the sidecar stays
    /// small and only real attachment boundaries appear in it.
    fn register_attachment(
        &mut self,
        kind: HirTriviaOwnerKind,
        semantic: Option<HirOwnerRef>,
        trivia: Vec<HirTrivia>,
    ) -> Option<HirSourceAnchorId> {
        if trivia.is_empty() {
            return None;
        }
        let id = self.next_anchor();
        self.presentation
            .owners
            .push(HirTriviaOwner { id: id.clone(), kind, semantic });
        self.presentation
            .attachments
            .push(HirTriviaAttachment { anchor: id.clone(), leading: trivia });
        Some(id)
    }

    /// Register a typed owner plus its ordered attachment at an existing anchor
    /// (structural ends already own a pre-allocated anchor id).
    fn register_attachment_at(
        &mut self,
        id: HirSourceAnchorId,
        kind: HirTriviaOwnerKind,
        semantic: Option<HirOwnerRef>,
        trivia: Vec<HirTrivia>,
    ) {
        if trivia.is_empty() {
            return;
        }
        self.presentation
            .owners
            .push(HirTriviaOwner { id: id.clone(), kind, semantic });
        self.presentation
            .attachments
            .push(HirTriviaAttachment { anchor: id, leading: trivia });
    }

    /// Attach one source statement's leading trivia to its first rendered HIR
    /// node (the expansion-first-node rule).
    ///
    /// A one-to-many statement attaches exactly once, to the first rendered
    /// statement. When the statement renders zero nodes and still carries a
    /// comment, the comment has no owner: lowering diagnoses rather than
    /// silently dropping or guessing a fallback.
    fn attach_stmt_trivia(&mut self, stmt: &Stmt, lowered: &[HirStatement]) {
        let trivia = self.hir_trivia(&stmt.leading_trivia);
        if trivia.is_empty() {
            return;
        }
        let Some(first) = lowered.first() else {
            self.error_with_args_at(vec![DiagnosticArg::new("issue", "comment_without_owner")], stmt.span);
            return;
        };
        self.register_attachment(HirTriviaOwnerKind::Statement, Some(HirOwnerRef::Hir(first.id)), trivia);
    }

    /// Attach one top-level item statement's leading trivia to its first
    /// rendered item (the expansion-first-node rule for items).
    ///
    /// `FuncDecl` carries its own (parser-empty today) leading-trivia carrier;
    /// when present it merges after the statement carrier in source order so no
    /// claimed comment is dropped by lowering.
    fn attach_item_trivia(&mut self, stmt: &Stmt, items: &[HirItem]) {
        let mut trivia = self.hir_trivia(&stmt.leading_trivia);
        if let StmtKind::Func(decl) = &stmt.kind {
            trivia.extend(self.hir_trivia(&decl.leading_trivia));
        }
        if trivia.is_empty() {
            return;
        }
        let Some(first) = items.first() else {
            self.error_with_args_at(vec![DiagnosticArg::new("issue", "comment_without_owner")], stmt.span);
            return;
        };
        self.register_attachment(HirTriviaOwnerKind::Item, Some(HirOwnerRef::Hir(first.id)), trivia);
    }

    /// Allocate the structural end of one authored executable block.
    ///
    /// Every authored `BlockStmt` boundary owns exactly one end; the end is
    /// recorded in the presentation `block_ends` list (count parity with
    /// `HirBlock::end`) and block-end trivia attaches at the same anchor.
    fn block_end_for(&mut self, block: &BlockStmt) -> Option<HirBlockEnd> {
        let id = self.next_anchor();
        let end = HirBlockEnd { id: id.clone(), span: block.span };
        self.presentation.block_ends.push(end.clone());
        let end_trivia = self.hir_trivia(&block.end_trivia);
        self.register_attachment_at(id, HirTriviaOwnerKind::BlockEnd, None, end_trivia);
        Some(end)
    }

    /// Resolve an item-level name, falling back to a synthetic id for recovery.
    ///
    /// A fallback here keeps malformed or partially resolved programs lowering
    /// far enough for later passes to attach more diagnostics to the same HIR.
    pub(super) fn def_id_for(&mut self, name: Symbol) -> crate::hir::DefId {
        self.resolver
            .lookup(name)
            .unwrap_or_else(|| self.next_def_id())
    }

    pub(super) fn push_scope(&mut self) {
        self.local_scopes.push(FxHashMap::default());
    }

    pub(super) fn pop_scope(&mut self) {
        self.local_scopes.pop();
    }

    pub(super) fn bind_local(&mut self, name: Symbol, def_id: crate::hir::DefId) {
        if let Some(scope) = self.local_scopes.last_mut() {
            scope.insert(name, def_id);
        }
    }

    pub(super) fn lookup_name(&self, name: Symbol) -> Option<crate::hir::DefId> {
        for scope in self.local_scopes.iter().rev() {
            if let Some(def_id) = scope.get(&name) {
                return Some(*def_id);
            }
        }
        self.resolver.lookup(name)
    }

    fn collect_kernel_glob_imports(&mut self, program: &Program) {
        for stmt in &program.statements {
            let StmtKind::Import(decl) = &stmt.kind else {
                continue;
            };
            if self.interner.resolve(decl.path) != crate::kernel::KERNEL_GLOB_IMPORT {
                continue;
            }
            let crate::syntax::ImportKind::Named { name, alias, .. } = &decl.kind else {
                continue;
            };
            let alias = alias.as_ref().map_or(name.name, |ident| ident.name);
            self.kernel_glob_aliases.insert(alias);
            for entry in crate::kernel::kernel_manifest_entries() {
                let module = self.interner.intern(entry.module.name());
                if !self.kernel_glob_imports.contains_key(&(alias, module)) {
                    let def_id = self.next_def_id();
                    self.kernel_glob_imports.insert((alias, module), def_id);
                }
            }
        }
    }

    pub(super) fn incipit_args_type(&mut self) -> TypeId {
        if let Some(cli) = self.cli_program {
            let mut fields = FxHashMap::default();
            for option in cli.global_options.iter().chain(cli.options.iter()) {
                let ty = self.cli_value_type(&option.ty, option.default.is_none() && !option.flag, false);
                fields.insert(option.binding_symbol, ty);
            }
            for operand in cli.global_operands.iter().chain(cli.operands.iter()) {
                let ty = self.cli_value_type(&operand.ty, false, operand.rest);
                fields.insert(operand.binding_symbol, ty);
            }
            return self.types.intern(Type::Record(fields));
        }

        let textus = self.types.primitive(Primitive::Textus);
        self.types.array(textus)
    }

    /// Build the synthetic argument record type for a mounted CLI command.
    ///
    /// A command function receives already-parsed CLI data as a record. Missing
    /// CLI metadata falls back to `ignotum` so ordinary compilation does not
    /// need a second lowering path.
    pub(super) fn command_args_type(&mut self, function: Symbol) -> TypeId {
        if let Some(cli) = self.cli_program {
            if let Some(command) = cli
                .commands
                .iter()
                .find(|command| command.module_path.is_none() && command.function_symbol == function)
            {
                let mut fields = FxHashMap::default();
                for option in cli.global_options.iter().chain(command.options.iter()) {
                    let ty = self.cli_value_type(&option.ty, option.default.is_none() && !option.flag, false);
                    fields.insert(option.binding_symbol, ty);
                }
                for operand in cli.global_operands.iter().chain(command.operands.iter()) {
                    let ty = self.cli_value_type(&operand.ty, false, operand.rest);
                    fields.insert(operand.binding_symbol, ty);
                }
                return self.types.intern(Type::Record(fields));
            }
        }

        self.types.primitive(Primitive::Ignotum)
    }

    /// Translate validated CLI value metadata into a HIR-visible semantic type.
    ///
    /// Optionality here means "the parser may omit this value", not nullable
    /// source syntax. Rest operands become arrays unless the CLI type is already
    /// list-shaped.
    fn cli_value_type(&mut self, ty: &CliType, optional: bool, rest: bool) -> TypeId {
        let base = match ty {
            CliType::Textus | CliType::Ignotum => self.types.primitive(Primitive::Textus),
            CliType::Numerus => self.types.primitive(Primitive::Numerus),
            CliType::Fractus => self.types.primitive(Primitive::Fractus),
            CliType::Bivalens => self.types.primitive(Primitive::Bivalens),
            CliType::Octeti => self.types.primitive(Primitive::Octeti),
            CliType::ListaTextus => {
                let textus = self.types.primitive(Primitive::Textus);
                self.types.array(textus)
            }
            CliType::ListaNumerus => {
                let numerus = self.types.primitive(Primitive::Numerus);
                self.types.array(numerus)
            }
        };
        let value = if rest && !matches!(ty, CliType::ListaTextus | CliType::ListaNumerus) {
            self.types.array(base)
        } else {
            base
        };
        if optional {
            self.types.option(value)
        } else {
            value
        }
    }

    /// Record a recoverable lowering diagnostic at the current source span.
    fn error(&mut self, issue: &'static str) {
        self.error_with_args(vec![DiagnosticArg::new("issue", issue)]);
    }

    /// Record a recoverable lowering diagnostic with explicit named facts.
    fn error_with_args(&mut self, args: Vec<DiagnosticArg>) {
        self.errors
            .push(LowerError { args, span: self.current_span });
    }

    /// Record a recoverable lowering diagnostic with explicit named facts at a known span.
    pub(super) fn error_with_args_at(&mut self, args: Vec<DiagnosticArg>, span: Span) {
        self.errors.push(LowerError { args, span });
    }

    /// Convert legacy shared-helper messages to stable lowering issue facts.
    ///
    /// Some semantic helpers still report their contract with message strings.
    /// HIR lowering must not forward that prose across phase boundaries.
    fn error_from_message(&mut self, message: &str) {
        self.error_with_args(lowering_issue_args_from_message(message));
    }

    /// Lower a top-level syntax statement when it represents one or more items.
    ///
    /// Non-item statements return an empty vector so `lower_program` can route
    /// them into the explicit or implicit entry block.
    fn lower_stmt_items(&mut self, stmt: &Stmt) -> Vec<HirItem> {
        match &stmt.kind {
            StmtKind::Var(decl) => self.lower_varia(stmt, decl).into_iter().collect(),
            StmtKind::Func(decl) => self.lower_functio(stmt, decl).into_iter().collect(),
            StmtKind::Class(decl) => self.lower_gens(stmt, decl).into_iter().collect(),
            StmtKind::Enum(decl) => self.lower_ordo(stmt, decl).into_iter().collect(),
            StmtKind::Union(decl) => self.lower_discretio(stmt, decl).into_iter().collect(),
            StmtKind::Interface(decl) => self.lower_implendum(stmt, decl).into_iter().collect(),
            StmtKind::TypeAlias(decl) => self.lower_typus(stmt, decl).into_iter().collect(),
            StmtKind::Schema(decl) => self.lower_schema(stmt, decl).into_iter().collect(),
            StmtKind::Import(decl) => self.lower_importa(stmt, decl).into_iter().collect(),
            StmtKind::Proba(case) => vec![self.lower_proba_item(case, &[], &[], &[])],
            StmtKind::Probandum(suite) => {
                let mut items = Vec::new();
                self.lower_probandum_items(suite, &[], &[], &[], &mut items);
                items
            }
            _ => Vec::new(),
        }
    }

    fn lower_probandum_items(
        &mut self,
        suite: &ProbandumDecl,
        inherited_setup: &[&PraeparaBlock],
        inherited_modifiers: &[ProbaModifier],
        suite_path: &[Symbol],
        out: &mut Vec<HirItem>,
    ) {
        // Only `praepara ... omnia` setup/teardown blocks flow into nested
        // suites. Local setup blocks are intentionally owned by the suite level
        // where they were written so generated tests do not inherit more fixture
        // work than the source requested.
        let mut combined_setup = inherited_setup.to_vec();
        for setup in &suite.body.setup {
            if setup.all {
                combined_setup.push(setup);
            }
        }

        let mut combined_suite_path = suite_path.to_vec();
        combined_suite_path.push(suite.name);

        let mut combined_modifiers = inherited_modifiers.to_vec();
        combined_modifiers.extend(suite.modifiers.iter().cloned());

        for case in &suite.body.tests {
            out.push(self.lower_proba_item(case, &combined_setup, &combined_modifiers, &combined_suite_path));
        }

        for nested in &suite.body.nested {
            self.lower_probandum_items(nested, &combined_setup, &combined_modifiers, &combined_suite_path, out);
        }
    }

    fn lower_proba_item(
        &mut self,
        case: &ProbaCase,
        inherited_setup: &[&PraeparaBlock],
        inherited_modifiers: &[ProbaModifier],
        suite_path: &[Symbol],
    ) -> HirItem {
        let def_id = self.next_def_id();
        self.push_scope();

        // Test cases become ordinary zero-argument HIR functions with metadata.
        // Setup and teardown blocks are spliced into the function body here so
        // typecheck and codegen can use the normal function path instead of a
        // separate test-body AST model.
        let mut stmts = Vec::new();
        for setup in inherited_setup {
            if matches!(setup.kind, PraeparaKind::Praepara | PraeparaKind::Praeparabit) {
                stmts.extend(self.lower_block(&setup.body).statements);
            }
        }
        // The authored test-body boundary owns one structural end already
        // recorded by `block_end_for`; carry it onto the merged function body
        // so the end-count parity with `presentation.block_ends` holds.
        let case_block = self.lower_block(&case.body);
        let case_end = case_block.end;
        stmts.extend(case_block.statements);
        for setup in inherited_setup {
            if matches!(setup.kind, PraeparaKind::Postpara | PraeparaKind::Postparabit) {
                stmts.extend(self.lower_block(&setup.body).statements);
            }
        }

        self.pop_scope();

        let mut modifiers = inherited_modifiers.to_vec();
        modifiers.extend(case.modifiers.iter().cloned());

        let mut body = HirBlock::simple(stmts, None, case.span);
        body.end = case_end;

        HirItem {
            id: self.next_hir_id(),
            def_id,
            kind: crate::hir::HirItemKind::Function(Box::new(crate::hir::HirFunction {
                name: case.name,
                type_params: Vec::new(),
                params: Vec::new(),
                cli_args: None,
                ret_ty: Some(self.types.primitive(crate::semantic::Primitive::Vacuum)),
                err_ty: None,
                body: Some(body),
                posture: crate::syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_device: false,
                is_vertex: false,
                is_fragment: false,
                annotations: Vec::new(),
                test: Some(HirTestMetadata {
                    name: case.name,
                    suite_path: suite_path.to_vec(),
                    modifiers: Self::lower_test_modifiers(&modifiers),
                    span: case.span,
                }),
            })),
            span: case.span,
        }
    }

    fn lower_test_modifiers(modifiers: &[ProbaModifier]) -> Vec<HirTestModifier> {
        modifiers.iter().map(Self::lower_test_modifier).collect()
    }

    /// Preserve test-selection metadata without interpreting harness policy.
    fn lower_test_modifier(modifier: &ProbaModifier) -> HirTestModifier {
        match modifier {
            ProbaModifier::Omitte(reason) => HirTestModifier::Omitte(*reason),
            ProbaModifier::Futurum(reason) => HirTestModifier::Futurum(*reason),
            ProbaModifier::Solum => HirTestModifier::Solum,
            ProbaModifier::Tag(tag) => HirTestModifier::Tag(*tag),
            ProbaModifier::Temporis(n) => HirTestModifier::Temporis(*n),
            ProbaModifier::Metior => HirTestModifier::Metior,
            ProbaModifier::Repete(n) => HirTestModifier::Repete(*n),
            ProbaModifier::Fragilis(n) => HirTestModifier::Fragilis(*n),
            ProbaModifier::SolumIn(env) => HirTestModifier::SolumIn(*env),
        }
    }

    /// Lower either a brace block or single-statement `ergo` body to a block.
    ///
    /// Control-flow consumers downstream see one HIR block shape regardless of
    /// whether source used Stroustrup braces or the compact single-statement
    /// form.
    fn lower_ergo_body(&mut self, body: &crate::syntax::IfBody) -> HirBlock {
        match body {
            crate::syntax::IfBody::Block(block) => self.lower_block(block),
            crate::syntax::IfBody::Ergo(stmt) => {
                let stmts = stmt::lower_stmt_expanded(self, stmt);
                self.attach_stmt_trivia(stmt, &stmts);
                HirBlock::simple(stmts, None, self.current_span)
            }
        }
    }

    /// Lower a lexical block with its own local scope.
    ///
    /// This is the authored-executable-block entry: every `BlockStmt` lowers
    /// with one structural end (via `HirBlock::end`), and its statements carry
    /// their leading trivia through the expansion-first-node rule.
    fn lower_block(&mut self, block: &crate::syntax::BlockStmt) -> HirBlock {
        self.current_span = block.span;
        let mut stmts = Vec::new();
        self.push_scope();

        for stmt in &block.statements {
            let lowered = stmt::lower_stmt_expanded(self, stmt);
            self.attach_stmt_trivia(stmt, &lowered);
            stmts.extend(lowered);
        }
        self.pop_scope();

        let end = self.block_end_for(block);
        HirBlock { statements: stmts, expr: None, span: block.span, breakable: None, end }
    }

    /// Lower an expression (delegates to expr.rs)
    fn lower_expr(&mut self, expr: &crate::syntax::Expr) -> HirExpression {
        expr::lower_expr(self, expr)
    }
}

/// Lower resolved AST to HIR without package CLI metadata.
///
/// This is the ordinary compiler entrypoint for sources whose entry arguments
/// are the default `lista<textus>` shape rather than a validated CLI contract.
pub fn lower(
    program: &Program,
    resolver: &mut Resolver,
    types: &mut TypeTable,
    interner: &mut Interner,
) -> (crate::hir::LoweredProgram, Vec<LowerError>) {
    lower_with_cli(program, resolver, types, interner, None)
}

/// Lower resolved AST to HIR with an optional validated CLI contract.
///
/// Package CLI analysis runs before lowering and can provide record-shaped
/// argument metadata for `incipit` and command functions. The HIR still carries
/// ordinary locals and type ids; no CLI-specific semantic checks are performed
/// here beyond translating the already validated contract into HIR-friendly
/// binding types.
pub fn lower_with_cli(
    program: &Program,
    resolver: &mut Resolver,
    types: &mut TypeTable,
    interner: &mut Interner,
    cli_program: Option<&CliProgram>,
) -> (crate::hir::LoweredProgram, Vec<LowerError>) {
    lower_with_cli_and_locale_pack(program, resolver, types, interner, cli_program, None)
}

/// Lower resolved AST to HIR with optional CLI metadata and reader-pack type
/// surface bindings.
pub fn lower_with_cli_and_locale_pack(
    program: &Program,
    resolver: &mut Resolver,
    types: &mut TypeTable,
    interner: &mut Interner,
    cli_program: Option<&CliProgram>,
    locale_pack: Option<&LocalePack>,
) -> (crate::hir::LoweredProgram, Vec<LowerError>) {
    let mut lowerer = Lowerer::new(resolver, types, interner, cli_program, locale_pack);
    let hir = lowerer.lower_program(program);
    let errors = lowerer.take_errors();
    let presentation = lowerer.presentation;
    (crate::hir::LoweredProgram { hir, presentation }, errors)
}

/// Lower resolved AST to HIR with optional CLI, reader-pack, and annotation-contract metadata.
pub fn lower_with_cli_locale_pack_and_contracts(
    program: &Program,
    resolver: &mut Resolver,
    types: &mut TypeTable,
    interner: &mut Interner,
    cli_program: Option<&CliProgram>,
    locale_pack: Option<&LocalePack>,
    annotation_contracts: Option<&AnnotationContractMetadata>,
) -> (crate::hir::LoweredProgram, Vec<LowerError>) {
    let mut lowerer =
        Lowerer::new_with_contracts(resolver, types, interner, cli_program, locale_pack, annotation_contracts);
    let hir = lowerer.lower_program(program);
    let errors = lowerer.take_errors();
    let presentation = lowerer.presentation;
    (crate::hir::LoweredProgram { hir, presentation }, errors)
}

fn lowering_issue_args_from_message(message: &str) -> Vec<DiagnosticArg> {
    match message {
        "lista requires one type parameter" => issue_args("lista_arity"),
        "tabula requires two type parameters" => issue_args("tabula_arity"),
        "copia requires one type parameter" => issue_args("copia_arity"),
        "promissum requires one type parameter" => issue_args("promissum_arity"),
        "cursor requires one type parameter" => issue_args("cursor_arity"),
        "f16, f32, and f64 are fractus widths; use fractus<f16>, fractus<f32>, or fractus<f64>" => {
            issue_args("float_width_on_numerus")
        }
        "integer width markers are numerus widths; use numerus<…>" => issue_args("integer_width_on_fractus"),
        "numerus accepts at most one width marker" => issue_args("numerus_width_params"),
        "fractus accepts at most one width marker" => issue_args("fractus_width_params"),
        "primitive accepts at most one width marker" => issue_args("primitive_width_params"),
        "unknown width marker for numerus" => issue_args("invalid_numerus_width_marker"),
        "fractus accepts only f16, f32, or f64" => issue_args("invalid_fractus_width_marker"),
        "unknown width marker for this family" => issue_args("invalid_width_marker"),
        "instans accepts only ms, us, or ns as a precision marker" => issue_args("invalid_precision_marker"),
        "instans accepts at most one precision marker" => issue_args("instans_precision_params"),
        "modulus requires exactly one unsigned width marker; use modulus<u8>, modulus<u16>, modulus<u32>, or modulus<u64>" => {
            issue_args("modular_word_width_required")
        }
        "modulus width marker must be a bare unsigned spelling such as u32" => issue_args("modular_word_width_shape"),
        "modulus accepts only unsigned width markers u8, u16, u32, or u64" => {
            issue_args("unsupported_modular_word_width")
        }
        "modulus requires an unsigned integer width marker" => issue_args("modular_word_unsigned_width_required"),
        crate::semantic::FIGURA_UNKNOWN_INDEX_MSG => issue_args("figura_unknown_index"),
        crate::semantic::FIGURA_TYPE_PARAM_IN_INDEX_MSG => issue_args("figura_type_param_in_index"),
        crate::semantic::FIGURA_NOT_INDEX_PARAM_MSG => issue_args("figura_not_index_param"),
        crate::semantic::VECTOR_WIDTH_SINGLE_DIM_MSG => issue_args("vector_width_single_dim"),
        crate::semantic::MATRIX_SHAPE_TWO_DIM_MSG => issue_args("matrix_shape_two_dim"),
        crate::semantic::ATOMIC_ELEMENT_MSG => issue_args("atomic_element"),
        "forma template literals must lower through forma capture" => issue_args("forma_literal_lowering_path"),
        _ => issue_args("unknown_lowering_error"),
    }
}

fn issue_args(issue: &'static str) -> Vec<DiagnosticArg> {
    vec![DiagnosticArg::new("issue", issue)]
}

fn retired_bare_object_literal_args() -> Vec<DiagnosticArg> {
    vec![
        DiagnosticArg::new("issue", "retired_bare_object_literal"),
        DiagnosticArg::new("object_literal_forms", "JSON { ... } or Type { ... }"),
    ]
}

#[cfg(test)]
#[path = "generic_call_test.rs"]
mod generic_call_test;

#[cfg(test)]
#[path = "schema_test.rs"]
mod schema_test;

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;

#[cfg(test)]
#[path = "instans_type_test.rs"]
mod instans_type_tests;
