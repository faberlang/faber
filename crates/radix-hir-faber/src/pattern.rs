//! Pattern and match-arm emission for canonical Faber output.
//!
//! `discerne` expressions delegate arm and pattern spelling here so match
//! output stays aligned across variants, bindings, aliases, guards, and literal
//! patterns. The writer preserves the semantic pattern tree in grammar-valid
//! Faber rather than attempting to recover the user's original punctuation or
//! whitespace.
//!
//! INVARIANTS
//! ==========
//! - Match arms print as `casu ... { ... }` with optional `si` guards.
//! - Binding names prefer resolved definitions so renamed or shadowed symbols
//!   follow the same name policy as expressions.
//! - Aliases use the canonical `pattern ut name` spelling.
//! - Variant patterns print the variant name followed by `fixum` bindings when
//!   the HIR pattern carries payload patterns.
//! - Literal patterns share literal spelling with expression literals.

use crate::surface::KeywordSurface;
use radix_codegen_shared::CodeWriter;
use radix_hir::{DefId, HirCasuArm, HirExpression, HirExpressionKind, HirPattern};
use radix_lexer::{Interner, Symbol, TokenKind};
use radix_types::TypeTable;
use rustc_hash::FxHashMap;

impl super::FaberCodegen {
    /// Write `discerne` arms from lowered pattern data.
    ///
    /// Arm bodies may already be lowered blocks or single expressions. This
    /// function preserves that distinction only as far as the grammar requires:
    /// every arm is braced, and non-block bodies are emitted as one expression
    /// statement inside the arm.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn write_match_arms(
        &self,
        arms: &[HirCasuArm],
        authored_elige: bool,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        for (idx, arm) in arms.iter().enumerate() {
            // An authored `elige` default (`ceterum`) lowers to a trailing
            // bare wildcard arm; re-emit the authored default spelling
            // instead of `casu _`.
            let elige_default = authored_elige
                && idx + 1 == arms.len()
                && arm.guard.is_none()
                && arm.patterns.len() == 1
                && matches!(arm.patterns[0], HirPattern::Wildcard);
            if elige_default {
                writer.write(surface.contextual_keyword("ceterum"));
            } else {
                writer.write(surface.contextual_keyword("casu"));
                writer.write(" ");
                self.write_pattern_list(&arm.patterns, types, names, interner, surface, writer);
                if let Some(guard) = &arm.guard {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("si"));
                    writer.write(" ");
                    self.write_expr(guard, types, names, interner, surface, writer);
                }
            }
            writer.writeln(" {");
            writer.indented(|writer| {
                self.write_match_arm_body(&arm.body, types, names, interner, surface, writer);
                if let HirExpressionKind::Block(block) = &arm.body.kind {
                    self.write_block_end_trivia(block, interner, writer);
                }
            });
            writer.writeln("}");
        }
    }

    /// Write one pattern in canonical Faber syntax.
    ///
    /// Pattern output is intentionally name-policy aware but not token
    /// preserving. It uses resolved names for bindings and variants where
    /// available, then delegates literal payloads to the shared literal writer.
    pub(super) fn write_pattern(
        &self,
        pattern: &HirPattern,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match pattern {
            HirPattern::Wildcard => writer.write("_"),
            HirPattern::Binding(def_id, name) => {
                let name = Self::resolved_binding_symbol(*def_id, *name, names);
                writer.write(&self.symbol_to_string(name, interner));
            }
            HirPattern::Alias(def_id, name, pattern) => {
                self.write_pattern(pattern, types, names, interner, surface, writer);
                let name = Self::resolved_binding_symbol(*def_id, *name, names);
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Ut));
                writer.write(" ");
                writer.write(&self.symbol_to_string(name, interner));
            }
            HirPattern::Variant(def_id, patterns) => {
                // Pattern position takes a union-qualified spelling only when
                // the shorter form cannot re-resolve: two local unions (or a
                // local and imported union) sharing a variant name (SEM001 on
                // re-check), or one variant name under two imported
                // namespaces. Authored spellings otherwise survive verbatim.
                let spelling = self
                    .qualified_local_variants
                    .get(def_id)
                    .cloned()
                    .or_else(|| {
                        self.imported_variant_union_path_ids
                            .contains(def_id)
                            .then(|| {
                                self.imported_pattern_spellings
                                    .get(def_id)
                                    .cloned()
                                    .unwrap_or_default()
                            })
                            .filter(|spelling| !spelling.is_empty())
                    })
                    .unwrap_or_else(|| self.name_for_def_imported(*def_id, names, interner));
                writer.write(&spelling);
                if !patterns.is_empty() {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("fixum"));
                    writer.write(" ");
                    self.write_pattern_list(patterns, types, names, interner, surface, writer);
                }
            }
            HirPattern::TypeTest {
                ty,
                binding,
                candidate,
            } => {
                if let Some(candidate) = candidate {
                    writer.write(&self.name_for_def_imported(*candidate, names, interner));
                } else {
                    writer.write(&self.type_to_faber(*ty, types, names, interner, surface));
                }
                if let Some((def_id, name)) = binding {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("fixum"));
                    writer.write(" ");
                    let name = Self::resolved_binding_symbol(*def_id, *name, names);
                    writer.write(&self.symbol_to_string(name, interner));
                }
            }
            HirPattern::Literal(lit) => {
                self.write_literal(lit, interner, surface, writer);
            }
        }
    }

    fn write_pattern_list(
        &self,
        patterns: &[HirPattern],
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        for (idx, pattern) in patterns.iter().enumerate() {
            if idx > 0 {
                writer.write(", ");
            }
            self.write_pattern(pattern, types, names, interner, surface, writer);
        }
    }

    fn write_match_arm_body(
        &self,
        body: &HirExpression,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let HirExpressionKind::Block(block) = &body.kind {
            self.write_block(block, types, names, interner, surface, writer);
        } else {
            self.write_expr(body, types, names, interner, surface, writer);
            writer.newline();
        }
    }
}
