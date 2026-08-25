//! Literal and object-field spelling for the canonical Faber backend.
//!
//! Literal emission is shared by expressions, patterns, object literals, and
//! conversion entries. Keeping these rules in one module prevents subtle drift
//! between JSON `{"key": value}`, genus `field = value`, regex literals, and
//! pattern literals.
//!
//! INVARIANTS
//! ==========
//! - JSON valor / tabula ascribes use `"key": value` (colon) spellings.
//! - Genus struct construction uses `field = value` (equals) spellings.
//! - Block `textus` literals that cannot round-trip as `"..."` re-emit as `«…»`.
//! - Fractus float literals keep a decimal point when whole (`7.0`, not `7`).
//!
//! LIMITS
//! ======
//! Block-string detection is payload-heuristic (newlines / embedded `"`) until
//! HIR carries an explicit delimiter kind. Quoted text is written from interned
//! payloads without full escape reconstruction.

use crate::surface::KeywordSurface;
use radix_codegen_shared::CodeWriter;
use radix_hir::{DefId, HirLiteral, HirObjectField, HirObjectKey};
use radix_lexer::{Interner, Symbol, TokenKind};
use radix_types::TypeTable;
use rustc_hash::FxHashMap;

/// Key/value separator for genus vs JSON object-field spellings.
enum ObjectFieldStyle {
    /// Genus struct construction: `field = value`.
    Genus,
    /// JSON valor / tabula ascribes: `"key": value`.
    Json,
}

impl ObjectFieldStyle {
    fn value_separator(self) -> &'static str {
        match self {
            Self::Genus => " = ",
            Self::Json => ": ",
        }
    }
}

impl super::FaberCodegen {
    /// Write one genus/struct field (`field = value`).
    pub(super) fn write_genus_field(
        &self,
        field: &HirObjectField,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        self.write_object_field(
            field,
            ObjectFieldStyle::Genus,
            types,
            names,
            interner,
            surface,
            writer,
        );
    }

    /// Write one JSON valor / tabula field (`"key": value`).
    pub(super) fn write_json_field(
        &self,
        field: &HirObjectField,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        self.write_object_field(
            field,
            ObjectFieldStyle::Json,
            types,
            names,
            interner,
            surface,
            writer,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn write_object_field(
        &self,
        field: &HirObjectField,
        style: ObjectFieldStyle,
        types: &TypeTable,
        names: &FxHashMap<DefId, Symbol>,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match &field.key {
            HirObjectKey::Ident(name) | HirObjectKey::String(name) => {
                match style {
                    ObjectFieldStyle::Genus => {
                        writer.write(&self.symbol_to_string(*name, interner));
                    }
                    ObjectFieldStyle::Json => {
                        self.write_quoted_symbol(*name, interner, writer);
                    }
                }
                if let Some(value) = &field.value {
                    writer.write(style.value_separator());
                    self.write_expr(value, types, names, interner, surface, writer);
                }
            }
            HirObjectKey::Computed(expr) => {
                writer.write("[");
                self.write_expr(expr, types, names, interner, surface, writer);
                writer.write("]");
                if let Some(value) = &field.value {
                    writer.write(style.value_separator());
                    self.write_expr(value, types, names, interner, surface, writer);
                }
            }
            HirObjectKey::Spread(expr) => {
                writer.write(surface.keyword(TokenKind::Sparge));
                writer.write(" ");
                self.write_expr(expr, types, names, interner, surface, writer);
            }
        }
    }

    /// Write a lowered literal using source-level Faber keywords and sigils.
    pub(super) fn write_literal(
        &self,
        lit: &HirLiteral,
        interner: &Interner,
        surface: &impl KeywordSurface,
        writer: &mut CodeWriter,
    ) {
        match lit {
            HirLiteral::Int(value) => writer.write(&value.to_string()),
            HirLiteral::Float(value) => {
                if value.is_nan() {
                    writer.write(surface.contextual_keyword("nonnumerus"));
                } else {
                    writer.write(&self.format_float_literal(*value));
                }
            }
            HirLiteral::String(sym) => self.write_textus_literal(*sym, interner, writer),
            HirLiteral::Ascii(sym) => {
                writer.write("'");
                writer.write(interner.resolve(*sym));
                writer.write("'");
            }
            HirLiteral::Octeti(sym) => {
                writer.write("|");
                writer.write(interner.resolve(*sym));
                writer.write("|");
            }
            HirLiteral::JsonValor(value) => {
                self.write_json_valor(value, interner, writer);
            }
            HirLiteral::Regex(pattern, _flags) => {
                self.write_quoted_symbol(*pattern, interner, writer);
                writer.write(" ↦ regex");
            }
            HirLiteral::Bool(value) => {
                writer.write(surface.contextual_keyword(if *value { "verum" } else { "falsum" }));
            }
            HirLiteral::Nil => writer.write(surface.contextual_keyword("nihil")),
        }
    }

    /// Emit a `textus` literal, preferring guillemets when `"..."` would break.
    pub(super) fn write_textus_literal(
        &self,
        sym: Symbol,
        interner: &Interner,
        writer: &mut CodeWriter,
    ) {
        let text = interner.resolve(sym);
        if text.contains('\n') || text.contains('"') {
            writer.write("«");
            writer.write_raw(text);
            writer.write("»");
        } else {
            self.write_quoted_symbol(sym, interner, writer);
        }
    }

    /// Format a fractus literal so whole values keep their decimal point (`7.0`).
    pub(super) fn format_float_literal(&self, value: f64) -> String {
        if value.is_nan() {
            return "nonnumerus".to_owned();
        }
        if value.is_infinite() {
            return if value.is_sign_negative() {
                "-∞".to_owned()
            } else {
                "∞".to_owned()
            };
        }

        let rendered = value.to_string();
        if rendered.contains('.') || rendered.contains('e') || rendered.contains('E') {
            rendered
        } else {
            format!("{rendered}.0")
        }
    }

    /// Quote interned source text without adding or normalizing escapes.
    pub(super) fn write_quoted_symbol(
        &self,
        sym: Symbol,
        interner: &Interner,
        writer: &mut CodeWriter,
    ) {
        writer.write("\"");
        writer.write(interner.resolve(sym));
        writer.write("\"");
    }

    /// Round-trip a compile-time JSON valor literal back to bare `{ ... }` JSON
    /// syntax. See `docs/factory/inline-json-valor/contract.md`.
    fn write_json_valor(
        &self,
        value: &radix_syntax::JsonValue,
        interner: &Interner,
        writer: &mut CodeWriter,
    ) {
        use radix_syntax::JsonValue;
        match value {
            JsonValue::Object(members) => {
                writer.write("{");
                if !members.is_empty() {
                    writer.write(" ");
                    for (index, member) in members.iter().enumerate() {
                        if index > 0 {
                            writer.write(", ");
                        }
                        self.write_quoted_symbol(member.key, interner, writer);
                        writer.write(": ");
                        self.write_json_valor(&member.value, interner, writer);
                    }
                    writer.write(" ");
                }
                writer.write("}");
            }
            JsonValue::Array(elements) => {
                writer.write("[");
                for (index, element) in elements.iter().enumerate() {
                    if index > 0 {
                        writer.write(", ");
                    }
                    self.write_json_valor(element, interner, writer);
                }
                writer.write("]");
            }
            JsonValue::String(sym) => self.write_quoted_symbol(*sym, interner, writer),
            JsonValue::Numerus(n) => writer.write(&n.to_string()),
            JsonValue::Fractus(n) => writer.write(&self.format_float_literal(*n)),
            JsonValue::Bool(b) => writer.write(if *b { "true" } else { "false" }),
            JsonValue::Null => writer.write("null"),
        }
    }

    /// Quote caller-provided text without adding or normalizing escapes.
    pub(super) fn write_quoted_text(&self, text: &str, writer: &mut CodeWriter) {
        writer.write("\"");
        writer.write(text);
        writer.write("\"");
    }

    /// Quote a symbol after resolving it through the backend name policy.
    pub(super) fn write_symbol_literal(
        &self,
        symbol: Symbol,
        interner: &Interner,
        writer: &mut CodeWriter,
    ) {
        writer.write("\"");
        writer.write(&self.symbol_to_string(symbol, interner));
        writer.write("\"");
    }
}

#[cfg(test)]
#[path = "literal_test.rs"]
mod tests;
