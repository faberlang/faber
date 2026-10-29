//! Canonical braced `@`-annotation re-emission for Faber surface form.
//!
//! Owns the re-emission of normalized [`HirAnnotation`] records back
//! to Faber source syntax. Handles field-based and raw-token annotation
//! bodies; keyword surface dispatch goes through [`KeywordSurface`].
//! Emission only — annotation analysis belongs in semantic phases.

use crate::surface::KeywordSurface;
use handroll::FxHashMap;
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    DefId, HirAnnotation, HirAnnotationField, HirAnnotationValue, HirExpression, HirExpressionKind,
    HirLiteral, HirOwnerRef, HirTypeSite, HirTypeSlot,
};
use radix_lexer::{Symbol, Token, TokenKind, lookup_keyword_spec};

// Registered here, not in `lib.rs`, so the module stays beside its subject.
#[cfg(test)]
#[path = "annotation_test.rs"]
mod annotation_test;

impl super::FaberCodegen<'_> {
    pub(super) fn write_hir_annotations(
        &self,
        annotations: &[HirAnnotation],
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        owner: Option<DefId>,
        writer: &mut CodeWriter,
    ) {
        for (annotation_ordinal, annotation) in annotations.iter().enumerate() {
            self.write_hir_annotation(
                annotation,
                owner,
                self.slot_ordinal(annotation_ordinal, "annotation count"),
                names,
                surface,
                writer,
            );
            writer.newline();
        }
    }

    // Entry point for one annotation: `owner` and `annotation_ordinal` name
    // the site its type-valued fields are re-emitted against.
    pub(super) fn write_hir_annotation(
        &self,
        annotation: &HirAnnotation,
        owner: Option<DefId>,
        annotation_ordinal: u32,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.write("@ ");
        writer.write(&self.annotation_surface_spelling(annotation.family, surface));

        // `@ ad '<route>'` re-emits its single-quoted ASCII route literal —
        // the parser production has no braced record form.
        if let Some(route) = annotation.ad_route(self.interner) {
            writer.write(" '");
            writer.write(self.interner.resolve(route));
            writer.write("'");
            return;
        }

        // `@ kernel relaxed` / `@ kernel unsafe`: the tier is a sugar word
        // after the family, spelled in the source locale. The parser admits
        // no braced tier, so the sugar form is the only round trip.
        let tier = annotation.kernel_tier(self.interner);
        if let Some(identity) = tier.identity() {
            writer.write(" ");
            writer.write(surface.contextual_keyword(identity));
            return;
        }

        if annotation.fields.is_empty() && annotation.raw_args.is_empty() {
            // Only an authored empty braced record re-emits braces; a bare
            // marker (`@ public`) stays bare. `@ conversio` is always bare:
            // its parser production accepts no braced record, so the braced
            // form fails PARSE018 on re-parse and breaks the locale round
            // trip.
            if annotation.brace_authored && self.symbol_to_string(annotation.family) != "conversio"
            {
                writer.write(" { }");
            }
            return;
        }

        if !annotation.fields.is_empty() {
            writer.write(" { ");
            let mut type_field_ordinal = 0;
            for (index, field) in annotation.fields.iter().enumerate() {
                if index > 0 {
                    writer.write(", ");
                }
                let field_ordinal = if matches!(&field.value, HirAnnotationValue::Type(_)) {
                    let ordinal = type_field_ordinal;
                    type_field_ordinal += 1;
                    Some(ordinal)
                } else {
                    None
                };
                self.write_hir_annotation_field(
                    field,
                    owner,
                    annotation_ordinal,
                    field_ordinal,
                    names,
                    surface,
                    writer,
                );
            }
            writer.write(" }");
            // An annotation carrying both a promoted field payload and raw
            // trailing tokens must not drop the raw half.
            if annotation.raw_args.is_empty() {
                return;
            }
        }

        for token in &annotation.raw_args {
            writer.write(" ");
            self.write_annotation_token(token, surface, writer);
        }
    }

    /// Project a Latin annotation family/field symbol through the active pack.
    ///
    /// Registered keyword identities use [`KeywordSurface::contextual_keyword`];
    /// unknown / user contract names pass through unchanged.
    fn annotation_surface_spelling(&self, symbol: Symbol, surface: &impl KeywordSurface) -> String {
        let spelling = self.symbol_to_string(symbol);
        if let Some(spec) = lookup_keyword_spec(&spelling) {
            surface.contextual_keyword(spec.text).to_owned()
        } else {
            spelling
        }
    }

    // Site-threaded authored-form emission carries owner/slot beside the
    // shared rendering context; the arity is the policy, not an accident.
    #[allow(clippy::too_many_arguments)]
    fn write_hir_annotation_field(
        &self,
        field: &HirAnnotationField,
        owner: Option<DefId>,
        annotation_ordinal: u32,
        field_ordinal: Option<u32>,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.write(&self.annotation_surface_spelling(field.name, surface));
        writer.write(" = ");
        self.write_hir_annotation_value(
            &field.value,
            owner,
            annotation_ordinal,
            field_ordinal,
            names,
            surface,
            writer,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn write_hir_annotation_value(
        &self,
        value: &HirAnnotationValue,
        owner: Option<DefId>,
        annotation_ordinal: u32,
        field_ordinal: Option<u32>,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match value {
            HirAnnotationValue::String(sym) => self.write_textus_literal(*sym, writer),
            HirAnnotationValue::Symbol(sym) => writer.write(&self.symbol_to_string(*sym)),
            HirAnnotationValue::Bool(true) => writer.write(surface.contextual_keyword("verum")),
            HirAnnotationValue::Bool(false) => writer.write(surface.contextual_keyword("falsum")),
            HirAnnotationValue::Int(n) => writer.write(&n.to_string()),
            HirAnnotationValue::Float(n) => writer.write(&self.format_float_literal(*n)),
            HirAnnotationValue::Nihil => writer.write(surface.contextual_keyword("nulla")),
            HirAnnotationValue::Type(ty) => {
                let rendered = match (owner, field_ordinal) {
                    (Some(owner), Some(field)) => {
                        let site = HirTypeSite {
                            owner: HirOwnerRef::Def(owner),
                            slot: HirTypeSlot::AnnotationField {
                                annotation: annotation_ordinal,
                                field,
                            },
                        };
                        self.type_to_faber_at(&site, *ty, names, surface)
                            .unwrap_or_else(|error| self.record_type_emission_error(error))
                    }
                    _ => self.type_to_faber(*ty, names, surface),
                };
                writer.write(&rendered);
            }
            HirAnnotationValue::Expr(expr) => {
                self.write_hir_annotation_expr(expr, names, surface, writer);
            }
        }
    }

    fn write_hir_annotation_expr(
        &self,
        expr: &HirExpression,
        names: &FxHashMap<DefId, Symbol>,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let HirExpressionKind::Literal(HirLiteral::String(sym)) = &expr.kind {
            self.write_textus_literal(*sym, writer);
            return;
        }
        self.write_expr(expr, names, surface, writer);
    }

    fn write_annotation_token(
        &self,
        token: &Token,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match &token.kind {
            TokenKind::String(sym)
            | TokenKind::AsciiString(sym)
            | TokenKind::BacktickString(sym) => {
                self.write_textus_literal(*sym, writer);
            }
            TokenKind::Integer(value) => writer.write(&value.to_string()),
            TokenKind::BigInteger(text) => writer.write(self.interner.resolve(*text)),
            // decimal-widths D1: emit paths stay byte-identical (f64 spelling).
            TokenKind::Float(value) | TokenKind::FloatText { value, .. } => {
                writer.write(&value.to_string());
            }
            TokenKind::Ident(sym) | TokenKind::Underscore(sym) => {
                let spelling = self.interner.resolve(*sym);
                // Tokenless keyword values (e.g. `verum` since Batch C) still
                // emit through the localized surface, matching the old
                // token-shaped path.
                if let Some(spec) = lookup_keyword_spec(spelling)
                    && spec.token_kind.is_none()
                {
                    writer.write(surface.contextual_keyword(spec.text));
                    return;
                }
                writer.write(spelling);
            }
            TokenKind::Colon => writer.write(":"),
            other => writer.write(&format!("{other:?}")),
        }
    }
}
