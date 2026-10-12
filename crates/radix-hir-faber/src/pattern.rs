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
use radix_lexer::{Symbol, TokenKind};
use rustc_hash::FxHashMap;

impl super::FaberCodegen<'_> {
    /// Write `discerne` arms from lowered pattern data.
    ///
    /// Arm bodies may already be lowered blocks or single expressions. This
    /// function preserves that distinction only as far as the grammar requires:
    /// every arm is braced, and non-block bodies are emitted as one expression
    /// statement inside the arm.
    pub(super) fn write_match_arms(
        &self,
        arms: &[HirCasuArm],
        authored_elige: bool,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        for (idx, arm) in arms.iter().enumerate() {
            // An authored `elige` default (`ceterum`) lowers to a trailing
            // bare wildcard arm; re-emit the authored default spelling
            // instead of `casu _`.
            let elige_default = authored_elige
                && idx + 1 == arms.len()
                && arm.patterns.len() == 1
                && matches!(arm.patterns[0], HirPattern::Wildcard);
            if elige_default {
                writer.write(surface.contextual_keyword("ceterum"));
            } else {
                writer.write(surface.contextual_keyword("casu"));
                writer.write(" ");
                self.write_arm_patterns(&arm.patterns, names, surface, writer);
            }
            writer.writeln(" {");
            writer.indented(|writer| {
                self.write_match_arm_body(&arm.body, names, surface, writer);
                if let HirExpressionKind::Block(block) = &arm.body.kind {
                    self.write_block_end_trivia(block, writer);
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
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match pattern {
            HirPattern::Wildcard => writer.write("_"),
            HirPattern::Binding(def_id, name) => {
                let name = Self::resolved_binding_symbol(*def_id, *name, names);
                writer.write(&self.symbol_to_string(name));
            }
            HirPattern::Alias(def_id, name, pattern) => {
                self.write_pattern(pattern, names, surface, writer);
                let name = Self::resolved_binding_symbol(*def_id, *name, names);
                writer.write(" ");
                writer.write(surface.keyword(TokenKind::Ut));
                writer.write(" ");
                writer.write(&self.symbol_to_string(name));
            }
            HirPattern::Or(patterns) => {
                for (index, pattern) in patterns.iter().enumerate() {
                    if index > 0 {
                        writer.write(" ");
                        writer.write(surface.keyword(TokenKind::Aut));
                        writer.write(" ");
                    }
                    self.write_pattern(pattern, names, surface, writer);
                }
            }
            HirPattern::Variant(def_id, patterns) => {
                let spelling = self.variant_pattern_spelling(*def_id, names);
                writer.write(&spelling);
                if !patterns.is_empty() {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("fixum"));
                    writer.write(" ");
                    self.write_binding_patterns(patterns, names, surface, writer);
                }
            }
            HirPattern::TypeTest {
                ty,
                binding,
                candidate,
            } => {
                if let Some(candidate) = candidate {
                    writer.write(&self.name_for_def_imported(*candidate, names));
                } else {
                    writer.write(&self.type_to_faber(*ty, names, surface));
                }
                if let Some((def_id, name)) = binding {
                    writer.write(" ");
                    writer.write(surface.contextual_keyword("fixum"));
                    writer.write(" ");
                    let name = Self::resolved_binding_symbol(*def_id, *name, names);
                    writer.write(&self.symbol_to_string(name));
                }
            }
            HirPattern::Literal(lit) => {
                self.write_literal(lit, surface, writer);
            }
            HirPattern::NegatedLiteral(lit) => {
                writer.write("-");
                self.write_literal(lit, surface, writer);
            }
        }
    }

    /// Resolve the canonical spelling for a variant pattern.
    ///
    /// Keep an authored qualified path when lowering erased its segments. If
    /// no path fact exists, qualify only when the shorter form cannot
    /// re-resolve.
    fn variant_pattern_spelling(&self, def_id: DefId, names: &FxHashMap<DefId, Symbol>) -> String {
        self.authored_variant_spellings
            .get(&def_id)
            .cloned()
            .or_else(|| self.qualified_local_variants.get(&def_id).cloned())
            .or_else(|| {
                self.imported_variant_union_path_ids
                    .contains(&def_id)
                    .then(|| {
                        self.imported_pattern_spellings
                            .get(&def_id)
                            .cloned()
                            .unwrap_or_default()
                    })
                    .filter(|spelling| !spelling.is_empty())
            })
            .unwrap_or_else(|| self.name_for_def_imported(def_id, names))
    }

    /// Arm-head patterns, one per subject, separated by `et`.
    fn write_arm_patterns(
        &self,
        patterns: &[HirPattern],
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        for (idx, pattern) in patterns.iter().enumerate() {
            if idx > 0 {
                writer.write(" ");
                writer.write(surface.contextual_keyword("et"));
                writer.write(" ");
            }
            self.write_pattern(pattern, names, surface, writer);
        }
    }

    /// Payload bindings of one variant pattern: a comma list.
    fn write_binding_patterns(
        &self,
        patterns: &[HirPattern],
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        for (idx, pattern) in patterns.iter().enumerate() {
            if idx > 0 {
                writer.write(", ");
            }
            self.write_pattern(pattern, names, surface, writer);
        }
    }

    fn write_match_arm_body(
        &self,
        body: &HirExpression,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let HirExpressionKind::Block(block) = &body.kind {
            self.write_block(block, names, surface, writer);
        } else {
            self.write_expr(body, names, surface, writer);
            writer.newline();
        }
    }
}
