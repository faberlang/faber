//! Canonical braced `@`-annotation re-emission for Faber surface form.
//!
//! Owns the re-emission of normalized [`HirAnnotation`] records back
//! to Faber source syntax. Handles field-based and raw-token annotation
//! bodies; keyword surface dispatch goes through [`KeywordSurface`].
//! Emission only — annotation analysis belongs in semantic phases.

use crate::surface::KeywordSurface;
use radix_codegen_shared::CodeWriter;
use radix_hir::{
    DefId, HirAnnotation, HirAnnotationField, HirAnnotationValue, HirExpression, HirExpressionKind,
    HirLiteral,
};
use radix_lexer::{lookup_keyword_spec, Interner, Symbol, Token, TokenKind};
use radix_types::TypeTable;
use rustc_hash::FxHashMap;

impl super::FaberCodegen {
    pub(super) fn write_hir_annotations(
        &self,
        annotations: &[HirAnnotation],
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        for annotation in annotations {
            self.write_hir_annotation(annotation, types, names, interner, surface, writer);
            writer.newline();
        }
    }

    pub(super) fn write_hir_annotation(
        &self,
        annotation: &HirAnnotation,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.write("@ ");
        writer.write(&self.annotation_surface_spelling(annotation.family, interner, surface));

        if annotation.fields.is_empty() && annotation.raw_args.is_empty() {
            // Only an authored empty braced record re-emits braces; a bare
            // marker (`@ public`) stays bare. `@ conversio` is always bare:
            // its parser production accepts no braced record, so the braced
            // form fails PARSE018 on re-parse and breaks the locale round
            // trip.
            if annotation.brace_authored
                && self.symbol_to_string(annotation.family, interner) != "conversio"
            {
                writer.write(" { }");
            }
            return;
        }

        if !annotation.fields.is_empty() {
            writer.write(" { ");
            for (index, field) in annotation.fields.iter().enumerate() {
                if index > 0 {
                    writer.write(", ");
                }
                self.write_hir_annotation_field(field, types, names, interner, surface, writer);
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
            self.write_annotation_token(token, interner, surface, writer);
        }
    }

    /// Project a Latin annotation family/field symbol through the active pack.
    ///
    /// Registered keyword identities use [`KeywordSurface::contextual_keyword`];
    /// unknown / user contract names pass through unchanged.
    fn annotation_surface_spelling(
        &self,
        symbol: Symbol,
        interner: &Interner,
        surface: &impl KeywordSurface,
    ) -> String {
        let spelling = self.symbol_to_string(symbol, interner);
        if let Some(spec) = lookup_keyword_spec(&spelling) {
            surface.contextual_keyword(spec.text).to_owned()
        } else {
            spelling
        }
    }

    fn write_hir_annotation_field(
        &self,
        field: &HirAnnotationField,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        writer.write(&self.annotation_surface_spelling(field.name, interner, surface));
        writer.write(" = ");
        self.write_hir_annotation_value(&field.value, types, names, interner, surface, writer);
    }

    fn write_hir_annotation_value(
        &self,
        value: &HirAnnotationValue,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match value {
            HirAnnotationValue::String(sym) => self.write_textus_literal(*sym, interner, writer),
            HirAnnotationValue::Symbol(sym) => writer.write(&self.symbol_to_string(*sym, interner)),
            HirAnnotationValue::Bool(true) => writer.write(surface.contextual_keyword("verum")),
            HirAnnotationValue::Bool(false) => writer.write(surface.contextual_keyword("falsum")),
            HirAnnotationValue::Int(n) => writer.write(&n.to_string()),
            HirAnnotationValue::Float(n) => writer.write(&n.to_string()),
            HirAnnotationValue::Nihil => writer.write(surface.contextual_keyword("nihil")),
            HirAnnotationValue::Type(ty) => {
                writer.write(&self.type_to_faber(*ty, types, names, interner, surface));
            }
            HirAnnotationValue::Expr(expr) => {
                self.write_hir_annotation_expr(expr, types, names, interner, surface, writer);
            }
        }
    }

    fn write_hir_annotation_expr(
        &self,
        expr: &HirExpression,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        if let HirExpressionKind::Literal(HirLiteral::String(sym)) = &expr.kind {
            self.write_textus_literal(*sym, interner, writer);
            return;
        }
        self.write_expr(expr, types, names, interner, surface, writer);
    }

    fn write_annotation_token(
        &self,
        token: &Token,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match &token.kind {
            TokenKind::String(sym)
            | TokenKind::AsciiString(sym)
            | TokenKind::BacktickString(sym) => {
                self.write_textus_literal(*sym, interner, writer);
            }
            TokenKind::Integer(value) => writer.write(&value.to_string()),
            // decimal-widths D1: emit paths stay byte-identical (f64 spelling).
            TokenKind::Float(value) | TokenKind::FloatText { value, .. } => {
                writer.write(&value.to_string());
            }
            TokenKind::Ident(sym) | TokenKind::Underscore(sym) => {
                let spelling = interner.resolve(*sym);
                // Tokenless keyword values (e.g. `verum` since Batch C) still
                // emit through the localized surface, matching the old
                // token-shaped path.
                if let Some(spec) = lookup_keyword_spec(spelling) {
                    if spec.token_kind.is_none() {
                        writer.write(surface.contextual_keyword(spec.text));
                        return;
                    }
                }
                writer.write(spelling);
            }
            TokenKind::Colon => writer.write(":"),
            other => writer.write(&format!("{other:?}")),
        }
    }
}
