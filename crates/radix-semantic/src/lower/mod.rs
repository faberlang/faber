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

/// Lowered HIR for one source unit.
#[derive(Debug)]
pub struct LoweredProgram {
    pub hir: HirModule,

    /// Presentation sidecar produced by the same lowering pass: source
    /// anchors, typed trivia owners, attachments, structural block ends, the
    /// program end, and the explicit entry-start anchor. Always travels with
    /// `hir`; semantic consumers stay trivia-blind by construction.
    pub presentation: HirPresentation,
}

pub mod annotation;
pub(crate) mod decl;
mod expr;
mod insere;
mod literal;
mod pattern;
mod stmt;
mod types;

/// One in-progress authored form node while an authored site lowers.
///
/// Child forms land here in lowering order as [`HirTypeFormArg`] entries;
/// `named` captures the resolved user-definition identity when the node
/// spelled a nominal/alias path, so the completed head can carry the authored
/// symbol path beside the semantic `DefId`.
#[derive(Default)]
pub(super) struct TypeFormFrame {
    args: Vec<HirTypeFormArg>,
    named: Option<(radix_types::DefId, Vec<Symbol>)>,
    /// The authored path of an imported alias that has no definition of its own.
    imported_alias: Option<Vec<Symbol>>,
}

use crate::cli::{CliOperand, CliOption, CliProgram, CliType};
use crate::locale::LocalePack;
use crate::{
    AnnotationContractMetadata, Primitive, Resolver, SemanticErrorKind, SemanticParamMode, Type,
    TypeId, TypeTable,
};
use handroll::{FxHashMap, FxHashSet};
use radix_diagnostics::DiagnosticArg;
use radix_hir::{
    HirAnnotation, HirBlock, HirBlockEnd, HirExpression, HirId, HirItem, HirModule, HirModuleEnd,
    HirOwnerRef, HirParamMode, HirPresentation, HirRegion, HirSourceAnchorId, HirStatement,
    HirStatementKind, HirTestMetadata, HirTestModifier, HirTrivia, HirTriviaAttachment,
    HirTriviaOwner, HirTriviaOwnerKind, HirTypeFormArg, HirVariantPath,
};
use radix_lexer::{Interner, Span, Symbol};
use radix_syntax::{
    BlockStmt, GenericParamKind, IncipitStmt, PraeparaBlock, PraeparaKind, ProbaCase,
    ProbaModifier, ProbandumDecl, Program, Stmt, StmtKind, Trivia, TypeMode,
};

/// Map a type-position access marker onto the HIR parameter-mode fact.
///
/// The type-position marker set (de/in/own/copy) is the source of truth for
/// parameter mode after MU-5. `own`/`copy` are value-transfer markers whose
/// fine distinction lives on the marked type; the HIR param mode collapses
/// both to `Owned` (owned-by-callee), matching the legacy default.
pub(super) fn type_mode_hir_param_mode(mode: TypeMode) -> HirParamMode {
    match mode {
        TypeMode::De => HirParamMode::De,
        TypeMode::In => HirParamMode::In,
        TypeMode::Own | TypeMode::Copy => HirParamMode::Owned,
    }
}

/// Map a type-position access marker onto the semantic parameter mode.
///
/// This is the fn-type/closure counterpart to [`type_mode_hir_param_mode`]:
/// de → shared ref, in → mutable ref, own/copy → value-owned.
pub(super) fn type_mode_semantic_param_mode(mode: TypeMode) -> SemanticParamMode {
    match mode {
        TypeMode::De => SemanticParamMode::Ref,
        TypeMode::In => SemanticParamMode::MutRef,
        TypeMode::Own | TypeMode::Copy => SemanticParamMode::Owned,
    }
}

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

    /// Indices into `presentation.variant_paths` of local parent-qualified
    /// spellings (`Color.rubrum`). Pruned per variant at the end of lowering
    /// when the same variant is also referenced bare (see
    /// [`prune_mixed_local_variant_paths`]).
    local_variant_path_indices: Vec<usize>,

    /// Owner context for authored-type capture inside annotation lowering.
    /// Set while lowering a carrying item's annotations; `None` leaves
    /// annotation type fields presentation-free.
    annotation_capture: Option<HirOwnerRef>,
    annotation_ordinal: Option<u32>,
    annotation_field_ordinal: u32,

    /// Authored type-form capture stack. Empty when no authored site is being
    /// lowered; every node inside a capture pushes one frame and completes it
    /// into a [`HirTypeForm`] when its lowering ends.
    form_frames: Vec<TypeFormFrame>,

    /// Next synthetic definition id for bindings that did not exist in resolver output.
    next_def_id: u32,

    /// Recoverable lowering diagnostics collected instead of aborting the pass.
    errors: Vec<LowerError>,

    /// Best source span for diagnostics emitted by helpers without a narrower span.
    current_span: Span,

    /// Stack of function/block-local bindings visible only during lowering.
    local_scopes: Vec<FxHashMap<Symbol, radix_hir::DefId>>,

    /// Explicit local value types needed by lowering shape-dependent
    /// destructuring, keyed by the unique local definition identity.
    local_types: FxHashMap<radix_hir::DefId, TypeId>,

    /// Current function's scalar/value parameter ordinals, used to keep
    /// runtime Figura references portable across file interfaces.
    parameter_ordinals: FxHashMap<radix_hir::DefId, u32>,

    /// Locals whose names stay admissible in extent positions: declared-size
    /// bindings (`const size H ← h` / `const size N = 3`) are size terms, and
    /// declared integer-list bindings keep the whole-shape value reading
    /// (conversions track). Other value bindings are refused there (size-type
    /// Z3: the bare `tf32[2, n]` annotation crossing).
    figura_extent_admissible_locals: FxHashSet<radix_hir::DefId>,

    /// Figura recursion inside a tuple/width extent position. Bare value
    /// names are refused only here; a root-position name keeps the
    /// whole-shape list reading (conversions track).
    figura_extent_nesting: u32,

    /// Figura recursion under a spelled `size` lift. The lift is the spelled
    /// store the same-page test requires, so its payload is admissible.
    figura_lift_nesting: u32,

    /// Type lowering inside a callable signature (parameters, return,
    /// alternate exit). There the Z3 annotation refusal stands down: the
    /// signature-position crossing is the signature law's diagnostic, whose
    /// did-you-mean points at the `<size N>` slot, not at a store lift.
    signature_type_nesting: u32,

    /// Lexical generic-parameter names visible while lowering signatures.
    type_param_scopes: Vec<FxHashMap<Symbol, (radix_hir::DefId, GenericParamKind)>>,

    /// Current self type for lowering `ego` expressions inside methods.
    current_ego_struct: Option<radix_hir::DefId>,

    /// Allow a bare generic nominal while lowering the target of a constructor
    /// object. Type annotations still reject omitted required arguments, but
    /// `C { ... }` needs to reach typecheck so its enclosing expected type can
    /// supply the witnesses.
    allow_bare_generic_construction: bool,

    /// Synthetic module import bindings created by `importa ex "faber:*"`.
    kernel_glob_imports: FxHashMap<(Symbol, Symbol), radix_hir::DefId>,

    /// Aliases introduced by `importa ex "faber:*"`.
    kernel_glob_aliases: FxHashSet<Symbol>,

    /// Validated CLI contract, when the entry point is a runnable CLI.
    cli_program: Option<&'a CliProgram>,

    /// Active reader pack for type-position source spelling bindings.
    locale_pack: Option<&'a LocalePack>,

    /// Resolved annotation-contract applications from the semantic contract pass.
    annotation_contracts: Option<&'a AnnotationContractMetadata>,

    /// Per-position `iuncta` labels extracted from the element type of a
    /// list-annotated local (`lista<iuncta<gx: A, B>>`), keyed by the local's
    /// `DefId`. Consumed by `itera` lowering to label the synthetic element
    /// local so by-label loop patterns resolve like binding-position forms.
    list_element_labels: FxHashMap<radix_hir::DefId, Vec<Option<Symbol>>>,

    /// Nesting depth of compile-time initializer trees being lowered (a
    /// module constant, a `=` local, or a genus field default). `insere` is
    /// legal only while this is non-zero (st1 R5); a `praefixum` body inherits
    /// the depth of the initializer that owns it.
    compile_time_depth: u32,
}

/// Recoverable diagnostic emitted while converting resolved AST into HIR.
///
/// Lowering errors describe shape problems, unsupported constructs, or missing
/// context discovered before type checking. They carry stable facts so semantic
/// diagnostic rendering owns human-readable prose.
#[derive(Debug, Clone)]
pub struct LowerError {
    /// Diagnostic family; ordinary lowering failures are `LoweringError`.
    pub kind: SemanticErrorKind,

    /// Named facts for catalog templates and future locale renderers.
    pub args: Vec<DiagnosticArg>,

    /// Source span associated with the problematic AST or synthesized node.
    pub span: Span,
}

impl<'a> Lowerer<'a> {
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
                program_end: HirModuleEnd {
                    id: HirSourceAnchorId(0),
                    span: Span::default(),
                },
                entry_start_anchor: None,
                entry_explicit: false,
                guard_si_ids: Vec::new(),
                elige_expr_ids: Vec::new(),
                glyph_ids: Vec::new(),
                radical_quarta_ids: Vec::new(),
                numeric_twin_ids: Vec::new(),
                empty_dense_ids: Vec::new(),
                inferred_constructor_ids: Vec::new(),
                axis_sugar: Vec::new(),
                discard_binding_stmts: Vec::new(),
                destructure_bindings: Vec::new(),
                await_assignment_bindings: Vec::new(),
                compile_time_locals: Vec::new(),
                ordo_enums: Vec::new(),
                variant_paths: Vec::new(),
                authored_types: Vec::new(),
                tuple_label_accesses: Vec::new(),
                template_holes: Vec::new(),
                insere_exprs: Vec::new(),
                function_modifier_bindings: Vec::new(),
                region: None,
            },
            local_variant_path_indices: Vec::new(),
            form_frames: Vec::new(),
            annotation_capture: None,
            annotation_ordinal: None,
            annotation_field_ordinal: 0,
            next_def_id: radix_types::HIR_SYNTHETIC_DEF_ID_BASE,
            errors: Vec::new(),
            current_span: Span::default(),
            local_scopes: Vec::new(),
            local_types: FxHashMap::default(),
            parameter_ordinals: FxHashMap::default(),
            figura_extent_admissible_locals: FxHashSet::default(),
            figura_extent_nesting: 0,
            figura_lift_nesting: 0,
            signature_type_nesting: 0,
            type_param_scopes: Vec::new(),
            current_ego_struct: None,
            allow_bare_generic_construction: false,
            kernel_glob_imports: FxHashMap::default(),
            kernel_glob_aliases: FxHashSet::default(),
            cli_program,
            locale_pack,
            annotation_contracts,
            list_element_labels: FxHashMap::default(),
            compile_time_depth: 0,
        }
    }

    /// Preserve a qualified variant spelling that would otherwise disappear
    /// when lowering the reference to its resolved `DefId`: a module-qualified
    /// path (`ns.Variant`, `ns.Type.Variant`) or a local parent-qualified one
    /// (`Color.rubrum`). The formatter must re-emit what was written (D8.9).
    pub(super) fn record_variant_path(&mut self, def_id: radix_hir::DefId, path: &[Symbol]) {
        if path.len() < 2 {
            return;
        }
        let Some(namespace_def) = self.lookup_name(path[0]) else {
            return;
        };
        let namespace_kind = self
            .resolver
            .get_symbol(namespace_def)
            .map(|symbol| symbol.kind);
        match namespace_kind {
            Some(crate::SymbolKind::Module) => {}
            Some(crate::SymbolKind::Enum) if path.len() == 2 => {
                self.local_variant_path_indices
                    .push(self.presentation.variant_paths.len());
            }
            _ => return,
        }
        self.presentation.variant_paths.push(HirVariantPath {
            def_id,
            path: path.to_vec(),
        });
    }

    /// Enter a lexical scope of generic-parameter names for signature lowering.
    pub(super) fn push_type_params(
        &mut self,
        params: impl IntoIterator<Item = (radix_hir::DefId, Symbol, GenericParamKind)>,
    ) {
        let mut layer = FxHashMap::default();
        for (def_id, name, kind) in params {
            layer.insert(name, (def_id, kind));
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
        self.lexical_type_param(name).map(|(_, kind)| kind)
    }

    /// The HIR type-parameter entry (its `HirTypeParam` def and kind) for a
    /// body-visible generic parameter name, innermost scope first.
    fn lexical_type_param(&self, name: Symbol) -> Option<(radix_hir::DefId, GenericParamKind)> {
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
        self.lower_region(program);

        let mut items = Vec::new();
        let mut entry = None;
        let mut entry_annotations = Vec::new();
        let mut entry_args_name = None;
        let mut entry_is_async = false;
        let mut implicit_entry_stmts = Vec::new();
        let mut implicit_entry_scope = false;
        let mut beside_entry_spans = Vec::new();

        for stmt in &program.statements {
            self.current_span = stmt.span;

            if let StmtKind::Incipit(entry_stmt) = &stmt.kind {
                let (block, annotations, args_name, is_async) =
                    self.lower_incipit_entry(stmt, entry_stmt);
                entry_annotations = annotations;
                entry_args_name = args_name;
                entry_is_async = is_async;
                entry = Some(block);
            } else {
                let lowered_items = self.lower_stmt_items(stmt);
                // A suite holding no case lowers to no item; it is still a
                // declaration, never an executable statement for the entry.
                let is_suite = matches!(&stmt.kind, StmtKind::Probandum(_));
                if lowered_items.is_empty() && !is_suite {
                    if !implicit_entry_scope {
                        self.push_scope();
                        implicit_entry_scope = true;
                    }
                    // Declarations whose lowering failed already carry their own
                    // error; only executable statement kinds can sit beside `incipit`.
                    if !matches!(
                        &stmt.kind,
                        StmtKind::Var(_)
                            | StmtKind::Func(_)
                            | StmtKind::Class(_)
                            | StmtKind::Enum(_)
                            | StmtKind::Union(_)
                            | StmtKind::Interface(_)
                            | StmtKind::TypeAlias(_)
                            | StmtKind::Schema(_)
                            | StmtKind::Import(_)
                            | StmtKind::Proba(_)
                            | StmtKind::Probandum(_)
                            | StmtKind::Incipit(_)
                    ) {
                        beside_entry_spans.push(stmt.span);
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

        if entry.is_some() {
            for span in beside_entry_spans {
                self.error_with_args_at(
                    vec![DiagnosticArg::new(
                        "issue",
                        "top_level_statement_beside_entry",
                    )],
                    span,
                );
            }
        }

        if entry.is_none() && !implicit_entry_stmts.is_empty() {
            let span = implicit_entry_stmts
                .first()
                .map_or_else(Span::default, |stmt| stmt.span);
            entry = Some(HirBlock::simple(implicit_entry_stmts, None, span));
        }

        self.attach_program_end(program);

        crate::builtins::inject_hir_items(
            &mut items,
            program,
            self.resolver,
            self.types,
            self.interner,
            self.locale_pack,
        );

        HirModule {
            items,
            entry,
            entry_annotations,
            entry_args_name,
            entry_is_async,
        }
    }

    /// Lower one explicit `incipit` entry into its block and entry metadata.
    ///
    /// Explicit entry (`incipit`) leading trivia renders at the entry-start
    /// owner; the entry block itself has no semantic node id, so the owner
    /// carries no back-reference.
    fn lower_incipit_entry(
        &mut self,
        stmt: &Stmt,
        entry_stmt: &IncipitStmt,
    ) -> (HirBlock, Vec<HirAnnotation>, Option<Symbol>, bool) {
        let entry_trivia = Self::hir_trivia(&stmt.leading_trivia);
        if let Some(anchor) =
            self.register_attachment(HirTriviaOwnerKind::Entry, None, entry_trivia)
        {
            self.presentation.entry_start_anchor = Some(anchor);
        }
        self.presentation.entry_explicit = true;
        let entry_annotations = self.lower_annotations(&stmt.annotations);
        let entry_args_name = entry_stmt.args.as_ref().map(|ident| ident.name);
        let entry_is_async = entry_stmt.is_async;
        // `incipit` introduces an entry-local scope and may synthesize a CLI
        // args local before the user-authored body statements.
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
        // Preserve exitus as the tail expression of the entry block.
        if let Some(exitus) = &entry_stmt.exitus {
            let exitus_expr = self.lower_expr(exitus);
            block.expr = Some(Box::new(exitus_expr));
        }
        self.pop_scope();
        (block, entry_annotations, entry_args_name, entry_is_async)
    }

    /// Attach the program-end trivia owner (trivia before EOF, including a
    /// comment-only file) and record it in the presentation.
    fn attach_program_end(&mut self, program: &Program) {
        let program_end_id = self.next_anchor();
        self.presentation.program_end = HirModuleEnd {
            id: program_end_id.clone(),
            span: program.span,
        };
        let end_trivia = Self::hir_trivia(&program.end_trivia);
        self.register_attachment_at(
            program_end_id,
            HirTriviaOwnerKind::ProgramEnd,
            None,
            end_trivia,
        );
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
    pub(super) fn next_def_id(&mut self) -> radix_hir::DefId {
        let id = self.next_def_id;
        self.next_def_id += 1;
        radix_hir::DefId(id)
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

    /// Record the file's `regio` declaration and its module doc (D7.7).
    ///
    /// The declaration's leading trivia is registered like any other owner so
    /// re-emission keeps every comment; the doc is only the comment block
    /// directly above the declaration (D7.6) — a block separated from it by a
    /// blank line is a note, not documentation.
    fn lower_region(&mut self, program: &Program) {
        let Some(region) = &program.region else {
            return;
        };
        let trivia = Self::hir_trivia(&region.leading_trivia);
        let anchor = self.register_attachment(HirTriviaOwnerKind::Region, None, trivia);
        self.presentation.region = Some(HirRegion {
            name: region.name.name,
            span: region.span,
            anchor,
            doc: attached_doc_lines(&region.leading_trivia),
        });
    }

    /// Convert syntax trivia into HIR presentation trivia, preserving order.
    fn hir_trivia(trivia: &[Trivia]) -> Vec<HirTrivia> {
        trivia
            .iter()
            .map(|item| match item {
                Trivia::CommentLine(comment) => HirTrivia::CommentLine {
                    text: comment.text,
                    span: comment.span,
                },
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
        self.presentation.owners.push(HirTriviaOwner {
            id: id.clone(),
            kind,
            semantic,
        });
        self.presentation.attachments.push(HirTriviaAttachment {
            anchor: id.clone(),
            leading: trivia,
        });
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
        self.presentation.owners.push(HirTriviaOwner {
            id: id.clone(),
            kind,
            semantic,
        });
        self.presentation.attachments.push(HirTriviaAttachment {
            anchor: id,
            leading: trivia,
        });
    }

    /// Attach one source statement's leading trivia to its first rendered HIR
    /// node (the expansion-first-node rule).
    ///
    /// A one-to-many statement attaches exactly once, to the first rendered
    /// statement. When the statement renders zero nodes and still carries a
    /// comment, the comment has no owner: lowering diagnoses rather than
    /// silently dropping or guessing a fallback.
    fn attach_stmt_trivia(&mut self, stmt: &Stmt, lowered: &[HirStatement]) {
        let trivia = Self::hir_trivia(&stmt.leading_trivia);
        if trivia.is_empty() {
            return;
        }
        let Some(first) = lowered.first() else {
            self.error_with_args_at(
                vec![DiagnosticArg::new("issue", "comment_without_owner")],
                stmt.span,
            );
            return;
        };
        self.register_attachment(
            HirTriviaOwnerKind::Statement,
            Some(HirOwnerRef::Hir(first.id)),
            trivia,
        );
    }

    /// Attach one top-level item statement's leading trivia to its first
    /// rendered item (the expansion-first-node rule for items).
    ///
    /// `FuncDecl` carries its own (parser-empty today) leading-trivia carrier;
    /// when present it merges after the statement carrier in source order so no
    /// claimed comment is dropped by lowering.
    fn attach_item_trivia(&mut self, stmt: &Stmt, items: &[HirItem]) {
        let mut trivia = Self::hir_trivia(&stmt.leading_trivia);
        if let StmtKind::Func(decl) = &stmt.kind {
            trivia.extend(Self::hir_trivia(&decl.leading_trivia));
        }
        if trivia.is_empty() {
            return;
        }
        let Some(first) = items.first() else {
            self.error_with_args_at(
                vec![DiagnosticArg::new("issue", "comment_without_owner")],
                stmt.span,
            );
            return;
        };
        self.register_attachment(
            HirTriviaOwnerKind::Item,
            Some(HirOwnerRef::Hir(first.id)),
            trivia,
        );
    }

    /// Allocate the structural end of one authored executable block.
    ///
    /// Every authored `BlockStmt` boundary owns exactly one end; the end is
    /// recorded in the presentation `block_ends` list (count parity with
    /// `HirBlock::end`) and block-end trivia attaches at the same anchor.
    fn block_end_for(&mut self, block: &BlockStmt) -> HirBlockEnd {
        let id = self.next_anchor();
        let end = HirBlockEnd {
            id: id.clone(),
            span: block.span,
        };
        self.presentation.block_ends.push(end.clone());
        let end_trivia = Self::hir_trivia(&block.end_trivia);
        self.register_attachment_at(id, HirTriviaOwnerKind::BlockEnd, None, end_trivia);
        end
    }

    /// Resolve an item-level name, falling back to a synthetic id for recovery.
    ///
    /// A fallback here keeps malformed or partially resolved programs lowering
    /// far enough for later passes to attach more diagnostics to the same HIR.
    pub(super) fn def_id_for(&mut self, name: Symbol) -> radix_hir::DefId {
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

    pub(super) fn bind_local(&mut self, name: Symbol, def_id: radix_hir::DefId) {
        if let Some(scope) = self.local_scopes.last_mut() {
            scope.insert(name, def_id);
        }
    }

    pub(super) fn lookup_name(&self, name: Symbol) -> Option<radix_hir::DefId> {
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
            if decl.target.is_some() {
                continue;
            }
            if self.interner.resolve(decl.path) != crate::kernel::KERNEL_GLOB_IMPORT {
                continue;
            }
            let radix_syntax::ImportKind::Named { name, alias, .. } = &decl.kind else {
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
            return self.cli_surface_record(
                cli.global_options.iter().chain(cli.options.iter()),
                cli.global_operands.iter().chain(cli.operands.iter()),
            );
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
        if let Some(cli) = self.cli_program
            && let Some(command) = cli.commands.iter().find(|command| {
                command.module_path.is_none() && command.function_symbol == function
            })
        {
            return self.cli_surface_record(
                cli.global_options.iter().chain(command.options.iter()),
                cli.global_operands.iter().chain(command.operands.iter()),
            );
        }

        self.types.primitive(Primitive::Ignotum)
    }

    /// Build one CLI surface's argument record from its option and operand chains.
    fn cli_surface_record(
        &mut self,
        options: impl Iterator<Item = &'a CliOption>,
        operands: impl Iterator<Item = &'a CliOperand>,
    ) -> TypeId {
        let mut fields = FxHashMap::default();
        for option in options {
            let ty =
                self.cli_value_type(&option.ty, option.default.is_none() && !option.flag, false);
            fields.insert(option.binding_symbol, ty);
        }
        for operand in operands {
            let ty = self.cli_value_type(&operand.ty, false, operand.rest);
            fields.insert(operand.binding_symbol, ty);
        }
        self.types.intern(Type::Record(fields))
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
            CliType::Octeti => self.types.octeti_list(),
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
    pub(super) fn error(&mut self, issue: &'static str) {
        self.error_with_args(vec![DiagnosticArg::new("issue", issue)]);
    }

    /// Record a recoverable lowering diagnostic with explicit named facts.
    fn error_with_args(&mut self, args: Vec<DiagnosticArg>) {
        self.errors.push(LowerError {
            kind: SemanticErrorKind::LoweringError,
            args,
            span: self.current_span,
        });
    }

    /// Record a recoverable diagnostic of a specific semantic family.
    pub(super) fn push_error(
        &mut self,
        kind: SemanticErrorKind,
        args: Vec<DiagnosticArg>,
        span: Span,
    ) {
        self.errors.push(LowerError { kind, args, span });
    }

    /// Record a recoverable lowering diagnostic with explicit named facts at a known span.
    pub(super) fn error_with_args_at(&mut self, args: Vec<DiagnosticArg>, span: Span) {
        self.errors.push(LowerError {
            kind: SemanticErrorKind::LoweringError,
            args,
            span,
        });
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
            StmtKind::Union(decl) => self.lower_discretio_items(stmt, decl),
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
        // Every hook declared on this suite runs for this suite's own cases, in
        // source order after the inherited ones. Only `omnia` hooks also flow
        // into nested suites; a local hook stays with the suite that wrote it.
        let mut case_setup = inherited_setup.to_vec();
        let mut nested_setup = inherited_setup.to_vec();
        for setup in &suite.body.setup {
            case_setup.push(setup);
            if setup.all {
                nested_setup.push(setup);
            }
        }

        let mut combined_suite_path = suite_path.to_vec();
        combined_suite_path.push(suite.name);

        let mut combined_modifiers = inherited_modifiers.to_vec();
        combined_modifiers.extend(suite.modifiers.iter().cloned());

        for case in &suite.body.tests {
            out.push(self.lower_proba_item(
                case,
                &case_setup,
                &combined_modifiers,
                &combined_suite_path,
            ));
        }

        for nested in &suite.body.nested {
            self.lower_probandum_items(
                nested,
                &nested_setup,
                &combined_modifiers,
                &combined_suite_path,
                out,
            );
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
            if matches!(
                setup.kind,
                PraeparaKind::Praepara | PraeparaKind::Praeparabit
            ) {
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
            if matches!(
                setup.kind,
                PraeparaKind::Postpara | PraeparaKind::Postparabit
            ) {
                stmts.extend(self.lower_block(&setup.body).statements);
            }
        }

        self.pop_scope();

        let mut modifiers = inherited_modifiers.to_vec();
        modifiers.extend(case.modifiers.iter().cloned());

        let mut body = HirBlock::simple(stmts, None, case.span);
        body.end = case_end;

        HirItem {
            annotations: Vec::new(),
            id: self.next_hir_id(),
            def_id,
            kind: radix_hir::HirItemKind::Function(Box::new(radix_hir::HirFunction {
                name: case.name,
                type_params: Vec::new(),
                params: Vec::new(),
                cli_args: None,
                ret_ty: Some(self.types.primitive(crate::Primitive::Vacuum)),
                err_ty: None,
                body: Some(body),
                posture: radix_syntax::CallablePosture::SyncFinite,
                is_async: false,
                is_generator: false,
                is_kernel: false,
                nucleum_fragment: false,
                is_vertex: false,
                is_fragment: false,
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
            ProbaModifier::Erratur => HirTestModifier::Erratur,
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
    fn lower_ergo_body(&mut self, body: &radix_syntax::IfBody) -> HirBlock {
        match body {
            radix_syntax::IfBody::Block(block) => self.lower_block(block),
            radix_syntax::IfBody::Ergo(stmt) => {
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
    fn lower_block(&mut self, block: &radix_syntax::BlockStmt) -> HirBlock {
        self.current_span = block.span;
        let mut stmts = Vec::new();
        self.push_scope();

        for stmt in &block.statements {
            let lowered = stmt::lower_stmt_expanded(self, stmt);
            self.attach_stmt_trivia(stmt, &lowered);
            stmts.extend(lowered);
        }
        self.pop_scope();

        let end = Some(self.block_end_for(block));
        HirBlock {
            statements: stmts,
            expr: None,
            span: block.span,
            breakable: None,
            end,
        }
    }

    fn lower_expr(&mut self, expr: &radix_syntax::Expr) -> HirExpression {
        expr::lower_expr(self, expr)
    }
}

/// HIR lowering request: the resolved program plus the optional metadata that
/// feeds binding shapes and library bindings.
///
/// `cli_program` carries an already validated CLI contract (record-shaped
/// entry arguments); `locale_pack` carries reader-pack type surface bindings;
/// `annotation_contracts` carries annotation-contract metadata. All three are
/// optional — plain sources lower with `None` for each.
pub struct LowerRequest<'a> {
    pub program: &'a Program,
    pub resolver: &'a mut Resolver,
    pub types: &'a mut TypeTable,
    pub interner: &'a mut Interner,
    pub cli_program: Option<&'a CliProgram>,
    pub locale_pack: Option<&'a LocalePack>,
    pub annotation_contracts: Option<&'a AnnotationContractMetadata>,
}

/// Lower resolved AST to HIR.
#[must_use]
pub fn lower(request: LowerRequest<'_>) -> (LoweredProgram, Vec<LowerError>) {
    let LowerRequest {
        program,
        resolver,
        types,
        interner,
        cli_program,
        locale_pack,
        annotation_contracts,
    } = request;
    let mut lowerer = Lowerer::new_with_contracts(
        resolver,
        types,
        interner,
        cli_program,
        locale_pack,
        annotation_contracts,
    );
    let hir = lowerer.lower_program(program);
    let errors = lowerer.take_errors();
    let mut presentation = lowerer.presentation;
    prune_mixed_local_variant_paths(&hir, &mut presentation, &lowerer.local_variant_path_indices);
    (LoweredProgram { hir, presentation }, errors)
}

/// Keep a local parent-qualified variant spelling only when every HIR
/// reference to that variant was authored qualified.
///
/// The spelling sidecar is keyed by variant `DefId`, not by site, so the
/// re-emitter writes one spelling for all of a variant's references. When a
/// source mixes `Color.rubrum` and bare `rubrum`, dropping the qualified
/// records re-emits every site bare — the same variant, so the same program
/// (D8.9) — instead of qualifying sites that were written bare.
fn prune_mixed_local_variant_paths(
    hir: &radix_hir::HirModule,
    presentation: &mut HirPresentation,
    local_indices: &[usize],
) {
    use radix_hir::visit::{HirVisitor, walk_expr, walk_pattern};
    use radix_hir::{HirExpressionKind, HirPattern};

    struct VariantReferenceCounter<'a> {
        tracked: &'a FxHashSet<radix_hir::DefId>,
        counts: FxHashMap<radix_hir::DefId, usize>,
    }

    impl VariantReferenceCounter<'_> {
        fn count(&mut self, def_id: radix_hir::DefId) {
            if self.tracked.contains(&def_id) {
                *self.counts.entry(def_id).or_default() += 1;
            }
        }
    }

    impl HirVisitor for VariantReferenceCounter<'_> {
        fn visit_expr(&mut self, expr: &radix_hir::HirExpression) {
            if let HirExpressionKind::Path(def_id) = &expr.kind {
                self.count(*def_id);
            }
            walk_expr(self, expr);
        }

        fn visit_pattern(&mut self, pattern: &HirPattern) {
            match pattern {
                HirPattern::Variant(def_id, _)
                | HirPattern::TypeTest {
                    candidate: Some(def_id),
                    ..
                } => self.count(*def_id),
                _ => {}
            }
            walk_pattern(self, pattern);
        }
    }

    if local_indices.is_empty() {
        return;
    }
    let mut qualified: FxHashMap<radix_hir::DefId, usize> = FxHashMap::default();
    for &index in local_indices {
        *qualified
            .entry(presentation.variant_paths[index].def_id)
            .or_default() += 1;
    }
    let tracked: FxHashSet<radix_hir::DefId> = qualified.keys().copied().collect();
    let mut counter = VariantReferenceCounter {
        tracked: &tracked,
        counts: FxHashMap::default(),
    };
    counter.visit_program(hir);
    let local: FxHashSet<usize> = local_indices.iter().copied().collect();
    let mut index = 0;
    presentation.variant_paths.retain(|path| {
        let keep = !local.contains(&index)
            || counter.counts.get(&path.def_id).copied().unwrap_or(0) == qualified[&path.def_id];
        index += 1;
        keep
    });
}

/// Convert residual shared-helper messages to stable lowering issue facts.
///
/// Width-family helpers report typed `WidthLowerIssue` values, so this
/// handles only collection arity, figura/vector/matrix/atomic, instans
/// precision, and forma messages that still cross the boundary as strings.
fn lowering_issue_args_from_message(message: &str) -> Vec<DiagnosticArg> {
    if let Some(reason) = crate::figura::figura_arithmetic_reason(message) {
        return vec![
            DiagnosticArg::new("issue", "figura_arithmetic_invalid"),
            DiagnosticArg::new("reason", reason),
        ];
    }
    match message {
        "lista requires one or two type parameters" => issue_args("lista_arity"),
        "tabula requires two type parameters" => issue_args("tabula_arity"),
        "tabulae requires two type parameters" => issue_args("tabulae_arity"),
        "copia requires one type parameter" => issue_args("copia_arity"),
        "promissum requires one type parameter" => issue_args("promissum_arity"),
        "cursor requires one type parameter" => issue_args("cursor_arity"),
        "queue requires one or two type parameters" => issue_args("queue_arity"),
        "stack requires one or two type parameters" => issue_args("stack_arity"),
        "instans accepts only ms, us, or ns as a precision marker" => {
            issue_args("invalid_precision_marker")
        }
        "instans accepts at most one precision marker" => issue_args("instans_precision_params"),
        crate::FIGURA_UNKNOWN_INDEX_MSG => issue_args("figura_unknown_index"),
        crate::FIGURA_TYPE_PARAM_IN_INDEX_MSG => issue_args("figura_type_param_in_index"),
        crate::FIGURA_NOT_INDEX_PARAM_MSG => issue_args("figura_not_index_param"),
        crate::figura::FIGURA_LIFT_SHAPE_MSG => issue_args("size_shape_outside_call_args"),
        crate::VECTOR_WIDTH_SINGLE_DIM_MSG => issue_args("vector_width_single_dim"),
        crate::MATRIX_SHAPE_TWO_DIM_MSG => issue_args("matrix_shape_two_dim"),
        crate::ATOMIC_ELEMENT_MSG => issue_args("atomic_element"),
        "forma template literals must lower through forma capture" => {
            issue_args("forma_literal_lowering_path")
        }
        _ => issue_args("unknown_lowering_error"),
    }
}

fn issue_args(issue: &'static str) -> Vec<DiagnosticArg> {
    vec![DiagnosticArg::new("issue", issue)]
}

/// The comment block directly above an anchor (D7.6): trailing `#` lines of
/// `trivia` with no blank line between them and the anchor, in source order.
///
/// Each comment line ends with its own newline token, so two newlines in a
/// row (read backwards) are a blank line and end the block.
fn attached_doc_lines(trivia: &[Trivia]) -> Vec<Symbol> {
    let mut lines = Vec::new();
    let mut line_ended = false;
    for item in trivia.iter().rev() {
        match item {
            Trivia::Newline(_) if line_ended => break,
            Trivia::Newline(_) => line_ended = true,
            Trivia::CommentLine(comment) => {
                lines.push(comment.text);
                line_ended = false;
            }
        }
    }
    lines.reverse();
    lines
}

#[cfg(test)]
#[path = "ascribed_literal_test.rs"]
mod ascribed_literal_test;
#[cfg(test)]
#[path = "authored_form_test.rs"]
mod authored_form_test;
#[cfg(test)]
#[path = "construct_test.rs"]
mod construct_test;
#[cfg(test)]
#[path = "destructure_test.rs"]
mod destructure_test;
#[cfg(test)]
#[path = "empty_suite_test.rs"]
mod empty_suite_test;
#[cfg(test)]
#[path = "generic_alias_test.rs"]
mod generic_alias_test;
#[cfg(test)]
#[path = "regex_literal_test.rs"]
mod regex_literal_test;
#[cfg(test)]
#[path = "size_decl_test.rs"]
mod size_decl_test;
#[cfg(test)]
#[path = "template_holes_test.rs"]
mod template_holes_test;
#[cfg(test)]
#[path = "type_algebra_test.rs"]
mod type_algebra_test;

#[cfg(test)]
#[path = "signed_pattern_test.rs"]
mod signed_pattern_test;

#[cfg(test)]
#[path = "pattern_import_test.rs"]
mod pattern_import_test;

#[cfg(test)]
#[path = "radical_glyph_test.rs"]
mod radical_glyph_test;
