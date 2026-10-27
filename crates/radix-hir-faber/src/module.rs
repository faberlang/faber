//! Canonical Faber source emission for analyzed HIR.

use crate::surface::KeywordSurface;
use crate::trivia::{TriviaIndex, write_leading_trivia};
use handroll::{FxHashMap, FxHashSet};
use radix_codegen_shared::{CodeWriter, CodegenError};
use radix_diagnostics::DiagnosticArg;
use radix_hir::{DefId, HirExpressionKind};
use radix_hir::{
    HirItem, HirItemKind, HirModule, HirModuleEnd, HirPresentation, HirSourceAnchorId, HirTypeForm,
    HirTypeSite,
};
use radix_lexer::{Interner, Span, Symbol, TokenKind};
use radix_types::{TypeId, TypeTable};
use std::cell::RefCell;

/// Canonical Faber output from HIR re-emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaberOutput {
    /// Emitted Faber source, including a trailing newline when required by callers.
    pub code: String,
}

/// Canonical Faber code generator.
///
/// Carries the per-unit fact pair — the analyzed [`TypeTable`] and the
/// [`Interner`] — on the context itself (installed once at
/// [`FaberCodegen::new`]) plus the resolved trivia index for the current
/// generation; render methods read them through `&self`, so a fresh
/// [`FaberCodegen::new`] with empty spellings reproduces trivia-free
/// canonical output exactly.
pub struct FaberCodegen<'a> {
    /// Analyzed type facts for the unit being emitted.
    pub(crate) types: &'a TypeTable,
    /// The unit's interner (symbol spelling resolution).
    pub(crate) interner: &'a Interner,
    /// Current unit bindings used to recover source names for runtime Figura
    /// leaves that retain a semantic `DefId` rather than a Symbol.
    pub(crate) runtime_value_names: RefCell<FxHashMap<DefId, Symbol>>,
    /// Enclosing function's formal names, keyed by declaration-order ordinal.
    pub(crate) runtime_parameter_names: RefCell<FxHashMap<u32, Symbol>>,
    /// Flattened trivia attachments for the presentation being generated.
    pub(crate) trivia: TriviaIndex,
    /// `TypeId` → qualified source spelling (`base.Vector3`) for imported
    /// nominal types, so re-emitted types keep their namespace even though
    /// the local name table has no entry for them.
    pub(crate) imported_type_spellings: FxHashMap<TypeId, String>,
    /// `DefId` → qualified source spelling for imported nominals in
    /// struct-literal construction expressions.
    pub(crate) imported_def_spellings: FxHashMap<DefId, String>,
    /// `DefId` → union-qualified spelling (`base.Kind.VARIANT`) for imported
    /// union variants in `casu` pattern arms.
    pub(crate) imported_pattern_spellings: FxHashMap<DefId, String>,
    /// Authored qualified variant spellings captured before lowering erased
    /// their path segments. These facts preserve whether a source reference
    /// used the two-part or three-part form.
    pub(crate) authored_variant_spellings: FxHashMap<DefId, String>,
    /// Def ids of this unit's struct/enum items: a `Path` to one of these in
    /// value position is a `ego` (self) reference and must re-emit as the
    /// self keyword, not the type name.
    pub(crate) self_def_ids: FxHashSet<DefId>,
    /// `(struct def, field name)` of this unit's const-member (`fixum`) fields: a
    /// `Field(Path(struct), name)` read of one of these is the authored
    /// type-qualified read `Tab.Z`, not an `ego` member read.
    pub(crate) static_fields: FxHashSet<(DefId, Symbol)>,
    /// Def ids of this unit's union variants: a `Call(Path(…), …)` whose
    /// callee is one of these is a `finge` variant construction lowered to a
    /// call shape, and must re-emit with the `finge` keyword (a bare
    /// `<variant>()` call is not re-parseable source).
    pub(crate) variant_def_ids: FxHashSet<DefId>,
    /// Local variant def ids whose bare name is ambiguous (the same variant
    /// name in two local unions), mapped to the union-qualified
    /// `Union.VARIANT` spelling. Bare re-emit of these fails re-check with
    /// SEM001/SEM008; the two-part local form re-parses.
    pub(crate) qualified_local_variants: FxHashMap<DefId, String>,
    /// Imported variant def ids that need the three-part `ns.Union.VARIANT`
    /// pattern spelling; all other imported variants keep the authored
    /// two-part `ns.VARIANT` form.
    pub(crate) imported_variant_union_path_ids: FxHashSet<DefId>,
    /// Ids of `Si` expressions lowered from `requirit`/`reice` guards; only
    /// these re-sugar to the guard statement form on emission.
    pub(crate) guard_si_ids: FxHashSet<radix_hir::HirId>,
    /// Ids of `Discerne` expressions lowered from `elige` statements; only
    /// these re-emit with the `elige` vocabulary and a `ceterum` default arm.
    pub(crate) elige_expr_ids: FxHashSet<radix_hir::HirId>,
    /// Ids of transpose `MethodCall`s authored with the `ᵀ` glyph; only these
    /// re-emit as `receiverᵀ` rather than `.transpose()`.
    pub(crate) glyph_ids: FxHashSet<radix_hir::HirId>,
    /// Ids of the outer `MethodCall` expressions lowered from an authored ∜
    /// glyph; only these re-emit as "∜ operand" rather than nested `.radix()`
    /// calls.
    pub(crate) radical_quarta_ids: FxHashSet<radix_hir::HirId>,
    /// Ids of expressions desugared from numeric twin method calls (operator,
    /// assignment, or effect-hoisted block); only these re-emit as
    /// `place.method(arg)`.
    pub(crate) numeric_twin_ids: FxHashSet<radix_hir::HirId>,
    /// Ids of `empty` literals the checker rewrote into the typed zero; only
    /// these re-emit as `empty` rather than `Type {}`.
    pub(crate) empty_dense_ids: FxHashSet<radix_hir::HirId>,
    pub(crate) inferred_constructor_ids: FxHashSet<radix_hir::HirId>,
    /// `HirId` → authored axis-reduction call facts for the expression
    /// typecheck expanded from `t.summa(axis n)` / `t.media(axis n)`: those
    /// nodes re-emit as the authored call, never the grouped reduction.
    pub(crate) axis_sugar: FxHashMap<radix_hir::HirId, radix_hir::HirAxisSugar>,
    /// Initializer-expression ids of authored discard bindings (`sit T _ ← e`)
    /// with their declaration facts, for re-emitting the binding.
    pub(crate) discard_bindings: FxHashMap<radix_hir::HirId, radix_hir::HirDiscardBinding>,
    /// Authored destructuring declarations keyed by the first statement id
    /// of their lowered run; emission re-sugars the run to the pattern.
    pub(crate) destructure_bindings: FxHashMap<radix_hir::HirId, radix_hir::HirDestructureBinding>,
    /// Definitions whose awaited initializer used the `↢` spelling.
    pub(crate) await_assignment_bindings: FxHashSet<DefId>,
    /// Locals authored `fixum T x = e` (D5.10), printed with `=`.
    pub(crate) compile_time_locals: FxHashSet<DefId>,
    /// Authored `ordo` enumerations by enum def id: emission re-sugars these
    /// with the `ordo` vocabulary and member discriminants.
    pub(crate) ordo_enums: FxHashMap<DefId, Vec<radix_hir::HirOrdoMember>>,
    /// Authored type forms by occurrence site: type emission re-renders
    /// exactly these authored long/short/named shapes; sites absent from the
    /// map keep the canonical fallback spelling.
    pub(crate) authored_type_forms: FxHashMap<HirTypeSite, HirTypeForm>,
    /// Authored labels of by-label `iuncta` reads, keyed by the rewritten
    /// index node's `HirId`: those nodes re-emit as `.label`, every other
    /// index read keeps `[i]`.
    pub(crate) tuple_label_accesses: FxHashMap<radix_hir::HirId, Symbol>,
    /// Authored build-time embed paths (`insere "path"`, D8.10), keyed by the
    /// folded literal's `HirId`: those literals re-emit as the embed.
    pub(crate) insere_exprs: FxHashMap<radix_hir::HirId, Symbol>,
    /// Authored template text and actual labels, keyed by application id.
    pub(crate) template_holes: FxHashMap<radix_hir::HirId, radix_hir::HirTemplateHoles>,
    /// Function modifier bindings (`errors e`, `options o`, `exit code`) by the
    /// runtime-provided local each introduced: those locals re-emit as the
    /// signature modifier, not as body statements.
    pub(crate) function_modifier_bindings: FxHashMap<DefId, radix_hir::HirFunctionModifierKind>,
    /// First malformed authored type form observed during emission.
    type_emission_error: RefCell<Option<CodegenError>>,
}

impl std::fmt::Debug for FaberCodegen<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The borrowed fact pair (`types`, `interner`) is arena-large and not
        // `Debug`; the emission state below it is what a debug dump needs.
        f.debug_struct("FaberCodegen")
            .field("imported_type_spellings", &self.imported_type_spellings)
            .field("imported_def_spellings", &self.imported_def_spellings)
            .field(
                "imported_pattern_spellings",
                &self.imported_pattern_spellings,
            )
            .field("type_emission_error", &self.type_emission_error)
            .finish_non_exhaustive()
    }
}

impl<'a> FaberCodegen<'a> {
    /// Build a code generator for one emission over the given analyzed facts.
    ///
    /// The type table and interner ride the context for the generator's
    /// lifetime instead of threading through every emit signature; emission
    /// methods read them through `&self`.
    #[must_use]
    pub fn new(types: &'a TypeTable, interner: &'a Interner) -> Self {
        Self {
            types,
            interner,
            runtime_value_names: RefCell::new(FxHashMap::default()),
            runtime_parameter_names: RefCell::new(FxHashMap::default()),
            trivia: TriviaIndex::default(),
            imported_type_spellings: FxHashMap::default(),
            imported_def_spellings: FxHashMap::default(),
            imported_pattern_spellings: FxHashMap::default(),
            authored_variant_spellings: FxHashMap::default(),
            self_def_ids: FxHashSet::default(),
            static_fields: FxHashSet::default(),
            variant_def_ids: FxHashSet::default(),
            qualified_local_variants: FxHashMap::default(),
            imported_variant_union_path_ids: FxHashSet::default(),
            guard_si_ids: FxHashSet::default(),
            elige_expr_ids: FxHashSet::default(),
            glyph_ids: FxHashSet::default(),
            radical_quarta_ids: FxHashSet::default(),
            numeric_twin_ids: FxHashSet::default(),
            empty_dense_ids: FxHashSet::default(),
            inferred_constructor_ids: FxHashSet::default(),
            axis_sugar: FxHashMap::default(),
            discard_bindings: FxHashMap::default(),
            destructure_bindings: FxHashMap::default(),
            await_assignment_bindings: FxHashSet::default(),
            compile_time_locals: FxHashSet::default(),
            ordo_enums: FxHashMap::default(),
            authored_type_forms: FxHashMap::default(),
            tuple_label_accesses: FxHashMap::default(),
            insere_exprs: FxHashMap::default(),
            template_holes: FxHashMap::default(),
            function_modifier_bindings: FxHashMap::default(),
            type_emission_error: RefCell::new(None),
        }
    }

    /// Install the import-contract spellings before [`Self::generate`].
    ///
    /// `imported_type_spellings` / `imported_def_spellings` /
    /// `imported_pattern_spellings` map imported nominal [`TypeId`]s and
    /// [`DefId`]s to their qualified source spelling (`base.Vector3`); pass
    /// empty maps when no import contract is available and imported types
    /// fall back to `def_N`.
    ///
    /// `FxHashMap` parameters are intentional (mirrors `TypeTable` /
    /// interner maps); do not generalize over `BuildHasher` for call-site
    /// simplicity.
    #[allow(clippy::implicit_hasher)]
    #[must_use]
    pub fn with_imported_spellings(
        mut self,
        imported_type_spellings: &FxHashMap<TypeId, String>,
        imported_def_spellings: &FxHashMap<DefId, String>,
        imported_pattern_spellings: &FxHashMap<DefId, String>,
    ) -> Self {
        self.imported_type_spellings
            .clone_from(imported_type_spellings);
        self.imported_def_spellings
            .clone_from(imported_def_spellings);
        self.imported_pattern_spellings
            .clone_from(imported_pattern_spellings);
        self
    }
}

/// An empty presentation: no owners, no attachments, and a structural program
/// end carrying no trivia.
///
/// Used by bridge call sites that have no presentation data (loose `HIR`+types
/// entries); rendering with it emits trivia-free canonical output.
#[must_use]
pub fn empty_presentation() -> HirPresentation {
    HirPresentation {
        owners: Vec::new(),
        attachments: Vec::new(),
        block_ends: Vec::new(),
        program_end: HirModuleEnd {
            id: HirSourceAnchorId(0),
            span: Span::default(),
        },
        entry_start_anchor: None,
        // Bridge callers (hand-built HIR, loose artifacts) have no surface
        // facts; the canonical default keeps the explicit `incipit` wrapper.
        entry_explicit: true,
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
    }
}

impl FaberCodegen<'_> {
    /// Generate canonical Faber source from analyzed HIR and its presentation.
    ///
    /// Item trivia renders before each item (and before its annotation
    /// sequence), entry trivia before the `incipit` keyword, and program-end
    /// trivia before EOF.
    ///
    /// # Errors
    ///
    /// Returns `CodegenError` if the HIR program contains error expressions that
    /// cannot be represented in canonical Faber output.
    pub fn generate(
        &mut self,
        hir: &HirModule,
        presentation: &HirPresentation,
        surface: &impl KeywordSurface,
    ) -> Result<FaberOutput, CodegenError> {
        radix_codegen_shared::reject_hir_errors(hir)?;
        reject_unsupported_legacy_hir(hir)?;
        self.install_presentation(presentation);

        let mut writer = CodeWriter::new();
        let names = self.collect_names(hir);
        (*self.runtime_value_names.borrow_mut()).clone_from(&names);
        self.runtime_parameter_names.borrow_mut().clear();
        self.self_def_ids = hir
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                HirItemKind::Struct(_) | HirItemKind::Enum(_) => Some(item.def_id),
                _ => None,
            })
            .collect();
        self.static_fields = hir
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                HirItemKind::Struct(strukt) => Some((item.def_id, strukt)),
                _ => None,
            })
            .flat_map(|(def_id, strukt)| {
                strukt
                    .fields
                    .iter()
                    .filter(|field| field.is_static())
                    .map(move |field| (def_id, field.name))
            })
            .collect();
        self.variant_def_ids = hir
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                HirItemKind::Enum(enm) => Some(&enm.variants),
                _ => None,
            })
            .flatten()
            .map(|variant| variant.def_id)
            .collect();
        // Imported union variants ride the pattern-spelling contract (its keys
        // are exactly the imported variant def ids). Without them a guard
        // `iace` payload targeting an imported variant re-emits as a bare
        // positional call — not re-parseable, not check-green source.
        self.variant_def_ids
            .extend(self.imported_pattern_spellings.keys().copied());
        self.qualified_local_variants = self.ambiguous_local_variant_spellings(hir);
        self.imported_variant_union_path_ids = self.imported_variants_needing_union_path(hir);

        let rendered_any_item =
            self.write_module_items(presentation, hir, &names, surface, &mut writer);

        self.write_entry_trivia(
            !rendered_any_item,
            !hir.entry_annotations.is_empty(),
            &mut writer,
        );
        self.write_hir_annotations(&hir.entry_annotations, &names, surface, None, &mut writer);

        self.write_entry_block(hir, presentation, &names, surface, &mut writer);

        self.write_program_end_trivia(!rendered_any_item && hir.entry.is_none(), &mut writer);

        if let Some(error) = self.type_emission_error.borrow_mut().take() {
            return Err(error);
        }
        Ok(FaberOutput {
            code: writer.finish(),
        })
    }

    /// Install every per-unit presentation fact on the generator. Called once
    /// at the head of [`Self::generate`], before any item is emitted.
    fn install_presentation(&mut self, presentation: &HirPresentation) {
        *self.type_emission_error.borrow_mut() = None;
        self.trivia = TriviaIndex::from_presentation(presentation);
        self.guard_si_ids = presentation.guard_si_ids.iter().copied().collect();
        self.elige_expr_ids = presentation.elige_expr_ids.iter().copied().collect();
        self.glyph_ids = presentation.glyph_ids.iter().copied().collect();
        self.radical_quarta_ids = presentation.radical_quarta_ids.iter().copied().collect();
        self.numeric_twin_ids = presentation.numeric_twin_ids.iter().copied().collect();
        self.empty_dense_ids = presentation.empty_dense_ids.iter().copied().collect();
        self.inferred_constructor_ids = presentation
            .inferred_constructor_ids
            .iter()
            .copied()
            .collect();
        self.axis_sugar = presentation
            .axis_sugar
            .iter()
            .map(|sugar| (sugar.id, sugar.clone()))
            .collect();
        self.discard_bindings = presentation
            .discard_binding_stmts
            .iter()
            .map(|binding| (binding.expr_id, binding.clone()))
            .collect();
        self.destructure_bindings = presentation
            .destructure_bindings
            .iter()
            .filter_map(|binding| Some((*binding.stmt_ids.first()?, binding.clone())))
            .collect();
        self.await_assignment_bindings = presentation
            .await_assignment_bindings
            .iter()
            .copied()
            .collect();
        self.compile_time_locals = presentation.compile_time_locals.iter().copied().collect();
        self.ordo_enums = presentation
            .ordo_enums
            .iter()
            .map(|ordo| (ordo.def_id, ordo.members.clone()))
            .collect();
        self.authored_variant_spellings = presentation
            .variant_paths
            .iter()
            .map(|path| {
                let spelling = path
                    .path
                    .iter()
                    .map(|segment| self.interner.resolve(*segment))
                    .collect::<Vec<_>>()
                    .join(".");
                (path.def_id, spelling)
            })
            .collect();
        self.authored_type_forms = presentation
            .authored_types
            .iter()
            .map(|authored| (authored.site.clone(), authored.form.clone()))
            .collect();
        self.tuple_label_accesses = presentation
            .tuple_label_accesses
            .iter()
            .map(|access| (access.expr_id, access.label))
            .collect();
        self.insere_exprs = presentation
            .insere_exprs
            .iter()
            .map(|insere| (insere.expr, insere.path))
            .collect();
        self.template_holes = presentation
            .template_holes
            .iter()
            .map(|holes| (holes.expr, holes.clone()))
            .collect();
        self.function_modifier_bindings = presentation
            .function_modifier_bindings
            .iter()
            .map(|binding| (binding.local, binding.kind))
            .collect();
    }

    /// Emit the `regio` header, then the top-level items — re-opening the
    /// `describe` chain around each run of consecutive tests — and close every
    /// suite still open. Returns whether any item rendered.
    fn write_module_items(
        &self,
        presentation: &HirPresentation,
        hir: &HirModule,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) -> bool {
        let mut rendered_any_item = self.write_region(presentation, hir, surface, writer);
        // Test items are flattened by lowering: each carries its enclosing
        // `describe` chain as `suite_path`. Re-open that chain around runs of
        // consecutive tests so suite names survive re-emission.
        let mut open_suites: Vec<Symbol> = Vec::new();
        for (idx, item) in hir.items.iter().enumerate() {
            if is_compiler_injected_item(item) {
                continue;
            }
            let path = item_suite_path(item);
            let keep = shared_prefix_len(&open_suites, path);
            self.close_suites(&mut open_suites, keep, &hir.items[idx..], writer);
            // An item that opens suites owns the leading trivia of the source
            // statement that opened them, so the trivia precedes the outermost
            // new `describe` header.
            self.write_item_trivia(item, !rendered_any_item, writer);
            rendered_any_item = true;
            for suite in &path[keep..] {
                writer.write(surface.keyword(TokenKind::Probandum));
                writer.write(" ");
                self.write_quoted_text(self.interner.resolve(*suite), writer);
                writer.writeln(" {");
                writer.indent();
                open_suites.push(*suite);
            }
            self.generate_item(item, names, surface, writer);
            // Canonical blank separator between items. When the following slot
            // carries leading trivia, the trivia run owns the boundary: this
            // separator would otherwise double the line break the run already
            // reproduces (boundary-newline normalization / idempotency). A
            // following item that leaves the open suites closes them first, so
            // no blank line precedes the closing brace.
            let rest = &hir.items[idx + 1..];
            let next_path = rest
                .iter()
                .find(|next| !is_compiler_injected_item(next))
                .map_or(&[][..], item_suite_path);
            let leaves_suite = shared_prefix_len(&open_suites, next_path) < open_suites.len();
            if !leaves_suite && !self.following_slot_has_trivia(rest) {
                writer.newline();
            }
        }
        self.close_suites(&mut open_suites, 0, &[], writer);
        rendered_any_item
    }

    /// Emit the program entry (`incipit`) block: the explicit keyword form when
    /// the source carried one, else the authored top-level statements in place.
    fn write_entry_block(
        &self,
        hir: &HirModule,
        presentation: &HirPresentation,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        let Some(entry) = &hir.entry else {
            return;
        };
        // Fidelity: an implicit entry (top-level statements with no
        // `incipit` keyword) re-emits in place. Wrapping it in an explicit
        // block changes the source form and can outlaw top-level-only
        // statements (template strings are TLA-only inside blocks).
        let explicit_entry = presentation.entry_explicit
            || presentation.entry_start_anchor.is_some()
            || hir.entry_is_async
            || hir.entry_args_name.is_some()
            || entry.expr.is_some()
            || !hir.entry_annotations.is_empty();
        if explicit_entry {
            writer.write(if hir.entry_is_async {
                surface.keyword(TokenKind::Incipiet)
            } else {
                surface.keyword(TokenKind::Incipit)
            });
            if let Some(args) = hir.entry_args_name {
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Argumenta));
                writer.write(" ");
                writer.write(&self.symbol_to_string(args));
            }
            // The entry block's tail expression is the authored `exitus`
            // value (lowering stores it there). It re-emits in the header,
            // never as a trailing body statement.
            if let Some(exit) = &entry.expr {
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Exitus));
                writer.write(" ");
                self.write_expr(exit, names, surface, writer);
            }
            writer.writeln(" {");
            writer.indented(|writer| {
                self.write_stmts(&entry.statements, names, surface, writer);
                self.write_block_end_trivia(entry, writer);
            });
            writer.writeln("}");
        } else {
            self.write_block(entry, names, surface, writer);
        }
    }

    /// Keep emission running long enough to collect the first malformed-form
    /// error, then make `generate` return it instead of emitting bad source.
    pub(super) fn record_type_emission_error(&self, error: CodegenError) -> String {
        let mut recorded = self.type_emission_error.borrow_mut();
        if recorded.is_none() {
            *recorded = Some(error);
        }
        "<error>".to_owned()
    }

    /// Render a type at its authored occurrence site, recording the first
    /// malformed-form error (surfaced at the end of [`Self::generate`]) rather
    /// than aborting the item that contains it.
    pub(super) fn render_type_at(
        &self,
        site: &HirTypeSite,
        ty: TypeId,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
    ) -> String {
        self.type_to_faber_at(site, ty, names, surface)
            .unwrap_or_else(|error| self.record_type_emission_error(error))
    }

    /// u32 ordinal for an enumerated type slot. A count beyond `u32::MAX`
    /// cannot render canonically, so it records the emission error (surfaces
    /// at the end of `generate`) and yields a placeholder ordinal instead of
    /// panicking mid-emission.
    pub(super) fn slot_ordinal(&self, index: usize, what: &str) -> u32 {
        u32::try_from(index).unwrap_or_else(|_| {
            self.record_type_emission_error(CodegenError {
                message: format!("{what} exceeds u32::MAX"),
                args: vec![DiagnosticArg::new("issue", "slot_ordinal_exceeds_u32")],
            });
            u32::MAX
        })
    }

    /// Emit the file's `regio NAME` declaration (D7.7) with its leading
    /// trivia (the module doc), first in the file. Returns whether it rendered.
    fn write_region(
        &self,
        presentation: &HirPresentation,
        hir: &HirModule,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) -> bool {
        let Some(region) = &presentation.region else {
            return false;
        };
        if let Some(trivia) = region
            .anchor
            .as_ref()
            .and_then(|anchor| self.trivia.for_anchor(anchor))
        {
            write_leading_trivia(crate::trivia::at_file_start(trivia), self.interner, writer);
        }
        writer.write(surface.contextual_keyword("regio"));
        writer.write(" ");
        writer.writeln(&self.symbol_to_string(region.name));
        if !self.following_slot_has_trivia(&hir.items) {
            writer.newline();
        }
        true
    }

    /// Emit item leading trivia before the item (and before its annotations).
    fn write_item_trivia(&self, item: &HirItem, file_start: bool, writer: &mut CodeWriter) {
        let Some(trivia) = self.trivia.for_hir(item.id) else {
            return;
        };
        self.write_slot_trivia(
            trivia,
            file_start,
            item_has_rendered_annotations(item),
            writer,
        );
    }

    /// Write one trivia slot with its boundary normalization.
    ///
    /// `file_start` drops every leading boundary newline (the first rendered
    /// slot has no preceding construct newline, so all of them are boundary and
    /// canonical output never begins with blank lines). `annotation_follows`
    /// drops the trailing line terminator the annotation re-emitter writes
    /// itself. Together these keep the emit idempotent.
    fn write_slot_trivia(
        &self,
        trivia: &[radix_hir::HirTrivia],
        file_start: bool,
        annotation_follows: bool,
        writer: &mut CodeWriter,
    ) {
        let trivia = if file_start {
            crate::trivia::at_file_start(trivia)
        } else {
            trivia
        };
        let trivia = if annotation_follows {
            crate::trivia::without_following_annotation(trivia)
        } else {
            trivia
        };
        write_leading_trivia(trivia, self.interner, writer);
    }

    /// Whether the next top-level slot carries leading trivia.
    ///
    /// The next slot is the next rendered item, or — after the last item — the
    /// entry's or the program-end's trivia. When it does, the canonical item
    /// separator newline is suppressed so the boundary newline is not emitted
    /// twice (the trivia run's first stored boundary newline is dropped and
    /// covers the item's own line terminator only).
    fn following_slot_has_trivia(&self, remaining: &[HirItem]) -> bool {
        if let Some(next) = remaining
            .iter()
            .find(|item| !is_compiler_injected_item(item))
        {
            return self.trivia.for_hir(next.id).is_some();
        }
        self.trivia.entry_trivia().is_some() || self.trivia.program_end_trivia().is_some()
    }

    /// Close open `describe` blocks down to `keep`, innermost first.
    ///
    /// `remaining` starts at the item about to render (empty at end of items)
    /// and decides whether the last closing brace owes a blank separator.
    fn close_suites(
        &self,
        open: &mut Vec<Symbol>,
        keep: usize,
        remaining: &[HirItem],
        writer: &mut CodeWriter,
    ) {
        while open.len() > keep {
            open.pop();
            writer.dedent();
            writer.writeln("}");
            if open.len() == keep && !self.following_slot_has_trivia(remaining) {
                writer.newline();
            }
        }
    }

    /// Emit entry (`incipit`) leading trivia before the entry keyword.
    fn write_entry_trivia(&self, file_start: bool, has_annotations: bool, writer: &mut CodeWriter) {
        let Some(trivia) = self.trivia.entry_trivia() else {
            return;
        };
        self.write_slot_trivia(trivia, file_start, has_annotations, writer);
    }

    /// Emit program-end trivia before EOF.
    fn write_program_end_trivia(&self, file_start: bool, writer: &mut CodeWriter) {
        if let Some(trivia) = self.trivia.program_end_trivia() {
            self.write_slot_trivia(trivia, file_start, false, writer);
        }
    }
}

/// Compiler-injected items (builtins allocated via `fresh_builtin_def_id`) must
/// not be re-emitted — they are not present in user source.
///
/// Builtin [`DefId`]s occupy the range `[1, USER_DEF_ID_BASE)`; user [`DefId`]s start
/// at `USER_DEF_ID_BASE`. The range check is structural and covers every
/// builtin allocated through the standard mechanism, regardless of its name.
fn is_compiler_injected_item(item: &HirItem) -> bool {
    item.def_id.is_builtin()
}

/// The `describe` chain lowering recorded for a test item (empty otherwise).
fn item_suite_path(item: &HirItem) -> &[Symbol] {
    match &item.kind {
        HirItemKind::Function(func) => func.test.as_ref().map_or(&[][..], |test| &test.suite_path),
        _ => &[],
    }
}

fn shared_prefix_len(open: &[Symbol], path: &[Symbol]) -> usize {
    open.iter().zip(path).take_while(|(a, b)| a == b).count()
}

/// Whether `generate_item` will re-emit the item's annotation sequence.
///
/// Item trivia drops its trailing annotation-line terminator when the shared
/// envelope carries annotations (see `trivia::without_following_annotation`).
fn item_has_rendered_annotations(item: &HirItem) -> bool {
    !item.annotations.is_empty()
}

/// Reject HIR programs containing legacy unsupported nodes before emission.
///
/// Generic `Cede` is rejected here. Source `cede` must lower to the
/// distinct `Yield` HIR node; await forms must lower to `Reddet`, `Tacebit`, or
/// await-local metadata. Re-emitting generic `Cede` would revive the old
/// context-dependent await/yield ambiguity.
fn reject_unsupported_legacy_hir(hir: &HirModule) -> Result<(), CodegenError> {
    if let Some((issue, message, span)) = find_unsupported_legacy_expr_in_program(hir) {
        return Err(CodegenError {
            message: format!("cannot emit {message} at span {}..{}", span.start, span.end),
            args: vec![DiagnosticArg::new("issue", issue)],
        });
    }
    Ok(())
}

fn find_unsupported_legacy_expr_in_program(
    hir: &HirModule,
) -> Option<(&'static str, &'static str, radix_lexer::Span)> {
    let mut finder = UnsupportedLegacyExprFinder::default();
    radix_hir::visit::HirVisitor::visit_program(&mut finder, hir);
    finder.found
}

#[derive(Default)]
struct UnsupportedLegacyExprFinder {
    found: Option<(&'static str, &'static str, radix_lexer::Span)>,
}

impl radix_hir::visit::HirVisitor for UnsupportedLegacyExprFinder {
    fn visit_expr(&mut self, expr: &radix_hir::HirExpression) {
        if self.found.is_some() {
            return;
        }
        match expr.kind {
            HirExpressionKind::Cede(_) => {
                self.found = Some((
                    "legacy_cede_reached_faber_codegen",
                    "legacy context-dependent cede HIR; source cede must lower to Yield and await forms must lower to distinct await operations",
                    expr.span,
                ));
            }
            _ => radix_hir::visit::walk_expr(self, expr),
        }
    }
}

#[cfg(test)]
#[path = "module_test.rs"]
mod tests;
