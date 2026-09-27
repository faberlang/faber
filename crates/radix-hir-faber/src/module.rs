//! Canonical Faber source emission for analyzed HIR.

use crate::surface::KeywordSurface;
use crate::trivia::{TriviaIndex, write_leading_trivia};
use radix_codegen_shared::{CodeWriter, CodegenError};
use radix_diagnostics::DiagnosticArg;
use radix_hir::{DefId, HirExpressionKind};
use radix_hir::{
    HirItem, HirItemKind, HirModule, HirModuleEnd, HirPresentation, HirSourceAnchorId, HirTypeForm,
    HirTypeSite,
};
use radix_lexer::{Interner, Span, Symbol, TokenKind};
use radix_types::{TypeId, TypeTable};
use rustc_hash::{FxHashMap, FxHashSet};
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
    /// Initializer-expression ids of authored discard bindings (`fixum T _ ← e`)
    /// with their declaration facts, for re-emitting the binding.
    pub(crate) discard_bindings: FxHashMap<radix_hir::HirId, radix_hir::HirDiscardBinding>,
    /// Authored destructuring declarations keyed by the first statement id
    /// of their lowered run; emission re-sugars the run to the pattern.
    pub(crate) destructure_bindings: FxHashMap<radix_hir::HirId, radix_hir::HirDestructureBinding>,
    /// Definitions whose awaited initializer used the `↢` spelling.
    pub(crate) await_assignment_bindings: FxHashSet<DefId>,
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
            trivia: TriviaIndex::default(),
            imported_type_spellings: FxHashMap::default(),
            imported_def_spellings: FxHashMap::default(),
            imported_pattern_spellings: FxHashMap::default(),
            authored_variant_spellings: FxHashMap::default(),
            self_def_ids: FxHashSet::default(),
            variant_def_ids: FxHashSet::default(),
            qualified_local_variants: FxHashMap::default(),
            imported_variant_union_path_ids: FxHashSet::default(),
            guard_si_ids: FxHashSet::default(),
            elige_expr_ids: FxHashSet::default(),
            discard_bindings: FxHashMap::default(),
            destructure_bindings: FxHashMap::default(),
            await_assignment_bindings: FxHashSet::default(),
            ordo_enums: FxHashMap::default(),
            authored_type_forms: FxHashMap::default(),
            tuple_label_accesses: FxHashMap::default(),
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
        discard_binding_stmts: Vec::new(),
        destructure_bindings: Vec::new(),
        await_assignment_bindings: Vec::new(),
        ordo_enums: Vec::new(),
        variant_paths: Vec::new(),
        authored_types: Vec::new(),
        tuple_label_accesses: Vec::new(),
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
        *self.type_emission_error.borrow_mut() = None;
        self.trivia = TriviaIndex::from_presentation(presentation);
        self.guard_si_ids = presentation.guard_si_ids.iter().copied().collect();
        self.elige_expr_ids = presentation.elige_expr_ids.iter().copied().collect();
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

        let mut writer = CodeWriter::new();
        let names = self.collect_names(hir);
        self.self_def_ids = hir
            .items
            .iter()
            .filter_map(|item| match &item.kind {
                HirItemKind::Struct(_) | HirItemKind::Enum(_) => Some(item.def_id),
                _ => None,
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

        let mut rendered_any_item = self.write_region(presentation, hir, surface, &mut writer);
        for (idx, item) in hir.items.iter().enumerate() {
            if is_compiler_injected_item(item) {
                continue;
            }
            self.write_item_trivia(item, !rendered_any_item, &mut writer);
            rendered_any_item = true;
            self.generate_item(item, &names, surface, &mut writer);
            // Canonical blank separator between items. When the following slot
            // carries leading trivia, the trivia run owns the boundary: this
            // separator would otherwise double the line break the run already
            // reproduces (boundary-newline normalization / idempotency).
            if !self.following_slot_has_trivia(&hir.items[idx + 1..]) {
                writer.newline();
            }
        }

        self.write_entry_trivia(
            !rendered_any_item,
            !hir.entry_annotations.is_empty(),
            &mut writer,
        );
        self.write_hir_annotations(&hir.entry_annotations, &names, surface, None, &mut writer);

        if let Some(entry) = &hir.entry {
            // Fidelity: an implicit entry (top-level statements with no
            // `incipit` keyword) re-emits in place. Wrapping it in an explicit
            // block changes the source form and can outlaw top-level-only
            // statements (template strings are TLA-only inside blocks).
            let explicit_entry = presentation.entry_explicit
                || presentation.entry_start_anchor.is_some()
                || hir.entry_is_async
                || hir.entry_args_name.is_some()
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
                writer.writeln(" {");
                writer.indented(|writer| {
                    self.write_block(entry, &names, surface, writer);
                    self.write_block_end_trivia(entry, writer);
                });
                writer.writeln("}");
            } else {
                self.write_block(entry, &names, surface, &mut writer);
            }
        }

        self.write_program_end_trivia(!rendered_any_item && hir.entry.is_none(), &mut writer);

        if let Some(error) = self.type_emission_error.borrow_mut().take() {
            return Err(error);
        }
        Ok(FaberOutput {
            code: writer.finish(),
        })
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
        // The first rendered item opens the file: its leading newlines are all
        // boundary (see `trivia::at_file_start`), keeping the emit idempotent.
        let trivia = if file_start {
            crate::trivia::at_file_start(trivia)
        } else {
            trivia
        };
        // An annotation sequence follows for annotated functions; its line
        // terminator is re-emitted by the annotation writer (see
        // `trivia::without_following_annotation`).
        let trivia = if item_has_rendered_annotations(item) {
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

    /// Emit entry (`incipit`) leading trivia before the entry keyword.
    fn write_entry_trivia(&self, file_start: bool, has_annotations: bool, writer: &mut CodeWriter) {
        let Some(trivia) = self.trivia.entry_trivia() else {
            return;
        };
        let trivia = if file_start {
            crate::trivia::at_file_start(trivia)
        } else {
            trivia
        };
        let trivia = if has_annotations {
            crate::trivia::without_following_annotation(trivia)
        } else {
            trivia
        };
        write_leading_trivia(trivia, self.interner, writer);
    }

    /// Emit program-end trivia before EOF.
    fn write_program_end_trivia(&self, file_start: bool, writer: &mut CodeWriter) {
        if let Some(trivia) = self.trivia.program_end_trivia() {
            let trivia = if file_start {
                crate::trivia::at_file_start(trivia)
            } else {
                trivia
            };
            write_leading_trivia(trivia, self.interner, writer);
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
