//! Canonical Faber source emission for analyzed HIR.
//!
//! Emits grammar-valid canonical Faber from HIR; comments and author
//! formatting require `radix::forma::author`.

use crate::surface::KeywordSurface;
use crate::trivia::{write_leading_trivia, TriviaIndex};
use radix_codegen_shared::{CodeWriter, CodegenError};
use radix_diagnostics::DiagnosticArg;
use radix_hir::{DefId, HirExpressionKind};
use radix_hir::{
    HirItem, HirItemKind, HirPresentation, HirModule, HirModuleEnd, HirSourceAnchorId,
};
use radix_lexer::{Interner, Span, TokenKind};
use radix_types::{TypeId, TypeTable};
use rustc_hash::{FxHashMap, FxHashSet};

/// Canonical Faber output from HIR re-emission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FaberOutput {
    /// Emitted Faber source, including a trailing newline when required by callers.
    pub code: String,
}

/// Canonical Faber code generator.
///
/// Carries the resolved trivia index for the current generation; render
/// methods read it through `&self`, so a fresh [`FaberCodegen::default`] with
/// an empty index reproduces trivia-free canonical output exactly.
#[derive(Debug, Default)]
pub struct FaberCodegen {
    /// Flattened trivia attachments for the presentation being generated.
    pub(crate) trivia: TriviaIndex,
    /// `TypeId` → qualified source spelling (`base.Vector3`) for imported
    /// nominal types, so re-emitted types keep their namespace even though
    /// the local name table has no entry for them.
    pub(crate) imported_type_spellings: FxHashMap<TypeId, String>,
    /// `DefId` → qualified source spelling for imported nominals in
    /// struct-literal construction expressions.
    pub(crate) imported_def_spellings: FxHashMap<DefId, String>,
    /// Def ids of this unit's struct/enum items: a `Path` to one of these in
    /// value position is a `ego` (self) reference and must re-emit as the
    /// self keyword, not the type name.
    pub(crate) self_def_ids: FxHashSet<DefId>,
    /// Def ids of this unit's union variants: a `Call(Path(…), …)` whose
    /// callee is one of these is a `finge` variant construction lowered to a
    /// call shape, and must re-emit with the `finge` keyword (a bare
    /// `<variant>()` call is not re-parseable source).
    pub(crate) variant_def_ids: FxHashSet<DefId>,
}

/// Emit canonical Faber source from analyzed HIR and its presentation.
///
/// The presentation sidecar is inseparable from the HIR value it annotates;
/// an [`empty_presentation`] reproduces trivia-free canonical output.
///
/// `imported_type_spellings` / `imported_def_spellings` map imported nominal
/// `TypeId`s and `DefId`s to their qualified source spelling (`base.Vector3`);
/// pass empty maps when no import contract is available and imported types
/// fall back to `def_N`.
///
/// # Errors
///
/// Returns `CodegenError` if the HIR program contains error expressions that
/// cannot be represented in canonical Faber output.
///
/// `FxHashMap` parameters are intentional (mirrors `TypeTable` / interner maps);
/// do not generalize over `BuildHasher` for call-site simplicity.
#[allow(clippy::implicit_hasher)]
pub fn emit(
    hir: &HirModule,
    presentation: &HirPresentation,
    types: &TypeTable,
    interner: &Interner,
    surface: &impl KeywordSurface,
    imported_type_spellings: &FxHashMap<TypeId, String>,
    imported_def_spellings: &FxHashMap<DefId, String>,
) -> Result<FaberOutput, CodegenError> {
    let mut codegen = FaberCodegen {
        imported_type_spellings: imported_type_spellings.clone(),
        imported_def_spellings: imported_def_spellings.clone(),
        ..Default::default()
    };
    codegen.generate(hir, presentation, types, interner, surface)
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
    }
}

impl FaberCodegen {
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
        types: &TypeTable,
        interner: &Interner,
        surface: &impl KeywordSurface,
    ) -> Result<FaberOutput, CodegenError> {
        radix_codegen_shared::reject_hir_errors(hir)?;
        reject_unsupported_legacy_hir(hir)?;
        self.trivia = TriviaIndex::from_presentation(presentation);

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

        let mut rendered_any_item = false;
        for (idx, item) in hir.items.iter().enumerate() {
            if is_compiler_injected_item(item) {
                continue;
            }
            self.write_item_trivia(item, !rendered_any_item, interner, &mut writer);
            rendered_any_item = true;
            self.generate_item(item, types, &names, interner, surface, &mut writer);
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
            interner,
            &mut writer,
        );
        self.write_hir_annotations(
            &hir.entry_annotations,
            types,
            &names,
            interner,
            surface,
            &mut writer,
        );

        if let Some(entry) = &hir.entry {
            writer.write(if hir.entry_is_async {
                surface.keyword(TokenKind::Incipiet)
            } else {
                surface.keyword(TokenKind::Incipit)
            });
            if let Some(args) = hir.entry_args_name {
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Argumenta));
                writer.write(" ");
                writer.write(&self.symbol_to_string(args, interner));
            }
            writer.writeln(" {");
            writer.indented(|writer| {
                self.write_block(entry, types, &names, interner, surface, writer);
                self.write_block_end_trivia(entry, interner, writer);
            });
            writer.writeln("}");
        }

        self.write_program_end_trivia(
            !rendered_any_item && hir.entry.is_none(),
            interner,
            &mut writer,
        );

        Ok(FaberOutput {
            code: writer.finish(),
        })
    }

    /// Emit item leading trivia before the item (and before its annotations).
    fn write_item_trivia(
        &self,
        item: &HirItem,
        file_start: bool,
        interner: &Interner,
        writer: &mut CodeWriter,
    ) {
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
        write_leading_trivia(trivia, interner, writer);
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
    fn write_entry_trivia(
        &self,
        file_start: bool,
        has_annotations: bool,
        interner: &Interner,
        writer: &mut CodeWriter,
    ) {
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
        write_leading_trivia(trivia, interner, writer);
    }

    /// Emit program-end trivia before EOF.
    fn write_program_end_trivia(
        &self,
        file_start: bool,
        interner: &Interner,
        writer: &mut CodeWriter,
    ) {
        if let Some(trivia) = self.trivia.program_end_trivia() {
            let trivia = if file_start {
                crate::trivia::at_file_start(trivia)
            } else {
                trivia
            };
            write_leading_trivia(trivia, interner, writer);
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
    item.def_id.0 < radix_types::USER_DEF_ID_BASE
}

/// Whether `generate_item` will re-emit an annotation sequence for this item.
///
/// Only functions write annotations today; structs/enums/interfaces do not.
/// The item trivia then drops its trailing annotation-line terminator (see
/// `trivia::without_following_annotation`).
fn item_has_rendered_annotations(item: &HirItem) -> bool {
    match &item.kind {
        HirItemKind::Function(func) => !func.annotations.is_empty(),
        _ => false,
    }
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
mod tests {
    use super::*;
    use crate::surface::TypeBindingKind;
    use radix_hir::{HirBlock, HirExpression, HirId, HirLiteral, HirStatement, HirStatementKind};
    use radix_lexer::Span;

    struct TestSurface;

    impl KeywordSurface for TestSurface {
        fn keyword(&self, _kind: TokenKind) -> &'static str {
            "keyword"
        }

        fn type_surface(&self, kind: TypeBindingKind) -> &'static str {
            match kind {
                // Explicit arms for the tensor family so the new kind is not
                // silently swallowed; this stub emits "type" for every kind.
                #[allow(clippy::match_same_arms)]
                TypeBindingKind::TensorFamily(
                    radix_types::TensorFamilyKind::Tensor
                    | radix_types::TensorFamilyKind::Vector
                    | radix_types::TensorFamilyKind::Matrix
                    | radix_types::TensorFamilyKind::Sparsa
                    | radix_types::TensorFamilyKind::Atomic
                    | radix_types::TensorFamilyKind::Intervallum,
                ) => "type",
                _ => "type",
            }
        }
    }

    fn literal_int(id: u32, value: u64, span: Span) -> HirExpression {
        HirExpression {
            id: HirId(id),
            kind: HirExpressionKind::Literal(HirLiteral::Int(value)),
            ty: None,
            span,
        }
    }

    #[test]
    fn emit_rejects_legacy_context_dependent_cede_hir() {
        let cede_span = Span { start: 10, end: 20 };
        let hir = HirModule {
            items: Vec::new(),
            entry: Some(HirBlock::simple(
                vec![HirStatement {
                    id: HirId(1),
                    kind: HirStatementKind::Expr(HirExpression {
                        id: HirId(2),
                        kind: HirExpressionKind::Cede(Box::new(literal_int(
                            3,
                            1,
                            Span { start: 15, end: 16 },
                        ))),
                        ty: None,
                        span: cede_span,
                    }),
                    span: cede_span,
                }],
                None,
                Span { start: 0, end: 24 },
            )),
            entry_annotations: Vec::new(),
            entry_args_name: None,
            entry_is_async: false,
        };
        let types = TypeTable::new();
        let interner = Interner::new();

        let err = emit(
            &hir,
            &empty_presentation(),
            &types,
            &interner,
            &TestSurface,
            &FxHashMap::default(),
            &FxHashMap::default(),
        )
        .expect_err("legacy Cede HIR must fail closed before Faber emission");

        assert!(
            err.message.contains("legacy context-dependent cede HIR"),
            "unexpected error message: {}",
            err.message
        );
        assert!(
            err.args
                .iter()
                .any(|arg| arg.name == "issue" && arg.value == "legacy_cede_reached_faber_codegen"),
            "missing legacy cede issue arg: {:?}",
            err.args
        );
    }
}
